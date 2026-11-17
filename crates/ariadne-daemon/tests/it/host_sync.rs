//! The sync of this daemon's catalogs and settings to each host (037).
//!
//! Two harness daemons: the far one served on a loopback TCP port, and the
//! local one with a `direct` host that points at it.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::common::{
    Harness, RUNS_OUT, TIMEOUT, delete, eventually, get, harness, next_event, post_json, put_json,
    roomy_hardware, sh,
};
use ariadne_api::hosts::{HostDto, HostSyncDto, MissingRepositoryDto, SyncState};
use ariadne_api::permissions::{AiPermissionsStatusDto, LearnedPermissionsResponse};
use ariadne_api::repositories::RepositoryDto;
use ariadne_api::skills::SkillDto;
use ariadne_api::stream::DomainEvent;
use ariadne_api::workflows::WorkflowDto;
use ariadne_core::PermissionMode;
use ariadne_daemon::bus::BusEvent;
use ariadne_daemon::timeouts::Timeouts;
use ariadne_store::NewLearnedPermission;
use axum::http::StatusCode;
use serde_json::json;
use tokio::sync::Semaphore;
use tokio::sync::broadcast::Receiver;

const WORKFLOW: &str =
    "workflow shared-flow\n  build[Build]\n    Do the work.\n    skills: coding\n";

/// A skill document named `name` whose summary is `summary`.
fn skill(name: &str, summary: &str) -> String {
    format!("---\nname: {name}\ndescription: {summary}\n---\n")
}

/// A local daemon whose one host `far` is `endpoint`, with a failed push
/// tried again after `RUNS_OUT`.
async fn local_with_host(endpoint: &str) -> (Harness, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(
        home.join("config.toml"),
        format!("[hosts.far]\nendpoint = \"{endpoint}\"\n"),
    )
    .unwrap();
    let h = harness()
        .ai_permissions_hardware(roomy_hardware())
        .home(home)
        .timeouts(Timeouts {
            host_sync_retry: RUNS_OUT,
            ..Timeouts::default()
        })
        .await;
    (h, dir)
}

/// Serve `far`'s router on `listener`.
fn serve(far: &Harness, listener: tokio::net::TcpListener) {
    let router = far.router.clone();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
}

/// What stands in front of the far daemon: while it holds, each request
/// waits for a permit the test hands out.
#[derive(Clone)]
struct Gate {
    held: Arc<AtomicBool>,
    permits: Arc<Semaphore>,
    /// Each request that waited at the gate, as `METHOD path`.
    held_requests: Arc<Mutex<Vec<String>>>,
    /// Each request the far daemon has answered, as `METHOD path`.
    answered: Arc<Mutex<Vec<String>>>,
}

impl Gate {
    fn new() -> Self {
        Self {
            held: Arc::new(AtomicBool::new(false)),
            permits: Arc::new(Semaphore::new(0)),
            held_requests: Arc::new(Mutex::new(Vec::new())),
            answered: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn hold(&self) {
        self.held.store(true, Ordering::SeqCst);
    }

    /// Let the held requests through one at a time, each answered before
    /// the next, until `request` is answered.
    async fn pass_until(&self, request: &str) {
        loop {
            let before = self.answered.lock().unwrap().len();
            self.permits.add_permits(1);
            eventually(TIMEOUT, "the held request answered", || async {
                self.answered.lock().unwrap().len() > before
            })
            .await;
            if self.answered.lock().unwrap()[before] == request {
                return;
            }
        }
    }

    /// Stop holding, and let every held request through.
    fn release(&self) {
        self.held.store(false, Ordering::SeqCst);
        self.permits.add_permits(Semaphore::MAX_PERMITS / 2);
    }
}

/// Serve `far`'s router on `listener` behind `gate`.
fn serve_gated(far: &Harness, listener: tokio::net::TcpListener, gate: &Gate) {
    let gate = gate.clone();
    let router = far.router.clone().layer(axum::middleware::from_fn(
        move |request: axum::extract::Request, next: axum::middleware::Next| {
            let gate = gate.clone();
            async move {
                let what = format!("{} {}", request.method(), request.uri());
                if gate.held.load(Ordering::SeqCst) {
                    gate.held_requests.lock().unwrap().push(what.clone());
                    gate.permits.acquire().await.unwrap().forget();
                }
                let response = next.run(request).await;
                gate.answered.lock().unwrap().push(what);
                response
            }
        },
    ));
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
}

/// The far daemon on a loopback port, and a local daemon whose host `far`
/// points at it. No sync runs until the test starts the local hosts.
async fn far_and_local() -> (Harness, Harness, tempfile::TempDir) {
    let far = harness().ai_permissions_hardware(roomy_hardware()).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    serve(&far, listener);
    let (local, dir) = local_with_host(&format!("http://{address}")).await;
    (far, local, dir)
}

/// The sync of the host `far`, as `GET /v1/hosts` lists it.
async fn sync_of(h: &Harness) -> HostSyncDto {
    let hosts: Vec<HostDto> = h.get("/v1/hosts").await;
    assert_eq!(hosts[1].name, "far");
    hosts[1].sync.clone()
}

async fn until_in_sync(h: &Harness) {
    eventually(TIMEOUT, "the host in sync", || async {
        sync_of(h).await.state == SyncState::InSync
    })
    .await;
}

/// A checkout under `h` whose `origin` is `owner/name` on GitHub,
/// registered on `h` with `body`'s other fields.
async fn forge_repository(
    h: &Harness,
    dir: &str,
    owner_name: &str,
    body: serde_json::Value,
) -> RepositoryDto {
    let path: PathBuf = h.git_repo(dir);
    sh(
        &path,
        &format!("git remote add origin https://github.com/{owner_name}.git"),
    );
    let mut body = body;
    body["path"] = json!(path.display().to_string());
    h.json(post_json("/v1/repositories", body), StatusCode::CREATED)
        .await
}

/// A learned `Bash` approval of `command` on `repository_id`.
fn approval(repository_id: &str, command: &str) -> NewLearnedPermission {
    NewLearnedPermission {
        repository_id: repository_id.into(),
        tool_name: "Bash".into(),
        key: json!({"command": command}).to_string(),
        level: "command".into(),
        family: command.into(),
        risk_tags: Vec::new(),
        scope: "all".into(),
        tool_call: json!({"name": "Bash", "title": command, "rawInput": {"command": command}}),
        options: json!([{"optionId": "yes", "name": "Allow", "kind": "allow_once"}]),
        selected_option: "yes".into(),
        target: "learn".into(),
        output: None,
    }
}

async fn skill_on(h: &Harness, name: &str) -> Option<SkillDto> {
    let (status, body) = h.send(get(&format!("/v1/skills/{name}"))).await;
    (status == StatusCode::OK).then(|| serde_json::from_slice(&body).unwrap())
}

async fn learned_on(h: &Harness, repository_id: &str) -> LearnedPermissionsResponse {
    h.get(&format!(
        "/v1/permissions/learned?repository={repository_id}"
    ))
    .await
}

#[tokio::test]
async fn a_ready_host_takes_every_item_and_keeps_its_own() {
    let (far, local, _dir) = far_and_local().await;
    for (h, summary) in [(&far, "far words"), (&local, "local words")] {
        let _: SkillDto = h
            .json(
                post_json(
                    "/v1/skills",
                    json!({"name": "shared", "document": skill("shared", summary)}),
                ),
                StatusCode::CREATED,
            )
            .await;
    }
    let _: SkillDto = far
        .json(
            post_json(
                "/v1/skills",
                json!({"name": "far-only", "document": skill("far-only", "kept")}),
            ),
            StatusCode::CREATED,
        )
        .await;
    let _: WorkflowDto = local
        .json(
            post_json(
                "/v1/workflows",
                json!({"name": "shared-flow", "document": WORKFLOW}),
            ),
            StatusCode::CREATED,
        )
        .await;
    let _: AiPermissionsStatusDto = local
        .json(
            put_json(
                "/v1/permissions/ai",
                json!({"allow_threshold": 0.125, "deny_threshold": 0.875}),
            ),
            StatusCode::OK,
        )
        .await;
    let mine = forge_repository(&local, "mine", "acme/widgets", json!({})).await;
    let theirs = forge_repository(&far, "theirs", "acme/widgets", json!({})).await;
    local
        .store
        .record_learned_permission(approval(&mine.id, "cargo test"))
        .await
        .unwrap();

    local.state.hosts.start();
    until_in_sync(&local).await;

    let shared = skill_on(&far, "shared").await.unwrap();
    assert_eq!(shared.summary, "local words");
    assert_eq!(skill_on(&far, "far-only").await.unwrap().summary, "kept");
    let flow: WorkflowDto = far.get("/v1/workflows/shared-flow").await;
    assert_eq!(flow.document, WORKFLOW);
    let ai: AiPermissionsStatusDto = far.get("/v1/permissions/ai").await;
    assert_eq!((ai.allow_threshold, ai.deny_threshold), (0.125, 0.875));
    assert!(!ai.thresholds_default);
    let learned = learned_on(&far, &theirs.id).await.items;
    assert_eq!(learned.len(), 1);
    assert_eq!(learned[0].key, r#"{"command":"cargo test"}"#);
    assert_eq!(learned[0].repository_id, theirs.id);
    assert_eq!(sync_of(&local).await, HostSyncDto::in_sync());
}

#[tokio::test]
async fn each_later_change_reaches_the_host_and_a_delete_removes_it() {
    let (far, local, _dir) = far_and_local().await;
    let mine = forge_repository(&local, "mine", "acme/widgets", json!({})).await;
    let theirs = forge_repository(&far, "theirs", "acme/widgets", json!({})).await;
    local.state.hosts.start();
    until_in_sync(&local).await;

    let _: SkillDto = local
        .json(
            post_json(
                "/v1/skills",
                json!({"name": "later", "document": skill("later", "first")}),
            ),
            StatusCode::CREATED,
        )
        .await;
    eventually(TIMEOUT, "the new skill on the host", || async {
        skill_on(&far, "later")
            .await
            .is_some_and(|s| s.summary == "first")
    })
    .await;
    let _: SkillDto = local
        .json(
            put_json(
                "/v1/skills/later",
                json!({"document": skill("later", "second")}),
            ),
            StatusCode::OK,
        )
        .await;
    eventually(TIMEOUT, "the changed skill on the host", || async {
        skill_on(&far, "later")
            .await
            .is_some_and(|s| s.summary == "second")
    })
    .await;
    let (status, _) = local.send(delete("/v1/skills/later")).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    eventually(TIMEOUT, "the skill gone from the host", || async {
        skill_on(&far, "later").await.is_none()
    })
    .await;

    let _: WorkflowDto = local
        .json(
            post_json(
                "/v1/workflows",
                json!({"name": "shared-flow", "document": WORKFLOW}),
            ),
            StatusCode::CREATED,
        )
        .await;
    eventually(TIMEOUT, "the workflow on the host", || async {
        far.send(get("/v1/workflows/shared-flow")).await.0 == StatusCode::OK
    })
    .await;
    let (status, _) = local.send(delete("/v1/workflows/shared-flow")).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    eventually(TIMEOUT, "the workflow gone from the host", || async {
        far.send(get("/v1/workflows/shared-flow")).await.0 == StatusCode::NOT_FOUND
    })
    .await;

    let row = local
        .store
        .record_learned_permission(approval(&mine.id, "cargo build"))
        .await
        .unwrap();
    eventually(TIMEOUT, "the learned permission on the host", || async {
        learned_on(&far, &theirs.id).await.items.len() == 1
    })
    .await;
    local
        .store
        .delete_learned_permission(&row.id)
        .await
        .unwrap();
    eventually(
        TIMEOUT,
        "the learned permission gone from the host",
        || async { learned_on(&far, &theirs.id).await.items.is_empty() },
    )
    .await;

    let _: AiPermissionsStatusDto = local
        .json(
            put_json(
                "/v1/permissions/ai",
                json!({"allow_threshold": 0.25, "deny_threshold": 0.75}),
            ),
            StatusCode::OK,
        )
        .await;
    eventually(TIMEOUT, "the AI thresholds on the host", || async {
        let ai: AiPermissionsStatusDto = far.get("/v1/permissions/ai").await;
        (ai.allow_threshold, ai.deny_threshold) == (0.25, 0.75)
    })
    .await;
}

/// A row on a repository the host has no checkout of is counted, not
/// pushed, and the host stays `in_sync`. A delete takes it off the count.
#[tokio::test]
async fn a_learned_permission_without_a_checkout_on_the_host_is_counted_as_skipped() {
    let (far, local, _dir) = far_and_local().await;
    let lacking = forge_repository(&local, "lacking", "acme/gadgets", json!({})).await;
    let first = local
        .store
        .record_learned_permission(approval(&lacking.id, "cargo test"))
        .await
        .unwrap();
    local.state.hosts.start();
    eventually(TIMEOUT, "one skipped permission", || async {
        let sync = sync_of(&local).await;
        sync.state == SyncState::InSync && sync.skipped_permissions == 1
    })
    .await;
    let far_rows: LearnedPermissionsResponse = far.get("/v1/permissions/learned").await;
    assert!(far_rows.items.is_empty(), "{:?}", far_rows.items);

    local
        .store
        .record_learned_permission(approval(&lacking.id, "cargo build"))
        .await
        .unwrap();
    eventually(TIMEOUT, "two skipped permissions", || async {
        sync_of(&local).await.skipped_permissions == 2
    })
    .await;
    local
        .store
        .delete_learned_permission(&first.id)
        .await
        .unwrap();
    eventually(TIMEOUT, "one skipped permission again", || async {
        let sync = sync_of(&local).await;
        sync.state == SyncState::InSync && sync.skipped_permissions == 1
    })
    .await;
}

#[tokio::test]
async fn a_repository_takes_the_local_settings_by_forge_and_a_missing_one_is_listed() {
    let (far, local, _dir) = far_and_local().await;
    let mine = forge_repository(
        &local,
        "mine",
        "acme/widgets",
        json!({"description": "Local words.", "permission_mode": "ask"}),
    )
    .await;
    let lacking = forge_repository(&local, "lacking", "acme/gadgets", json!({})).await;
    let theirs = forge_repository(
        &far,
        "theirs",
        "acme/widgets",
        json!({"description": "Far words."}),
    )
    .await;
    local.state.hosts.start();

    let expected = vec![MissingRepositoryDto {
        id: lacking.id.clone(),
        forge: Some("acme/gadgets".into()),
    }];
    eventually(TIMEOUT, "the missing checkout listed", || async {
        let sync = sync_of(&local).await;
        sync.state == SyncState::InSync && sync.missing_repositories == expected
    })
    .await;
    let after: RepositoryDto = far.get(&format!("/v1/repositories/{}", theirs.id)).await;
    assert_eq!(after.description.as_deref(), Some("Local words."));
    assert_eq!(after.permission_mode, PermissionMode::Ask);
    assert_eq!(after.path, theirs.path);
    assert_ne!(after.path, mine.path);
    let all: Vec<RepositoryDto> = far.get("/v1/repositories").await;
    assert_eq!(
        all.len(),
        1,
        "nothing is registered for the missing checkout"
    );
}

/// A push the host refuses is tried again on its own: the changes made
/// after it still reach the host.
#[tokio::test]
async fn a_refused_push_holds_back_no_later_change() {
    let (far, local, _dir) = far_and_local().await;
    let _: SkillDto = local
        .json(
            post_json(
                "/v1/skills",
                json!({"name": "shared", "document": skill("shared", "local")}),
            ),
            StatusCode::CREATED,
        )
        .await;
    local.state.hosts.start();
    until_in_sync(&local).await;
    // A workflow only the host has names the skill, so the host refuses to
    // delete it.
    let far_flow = "workflow far-flow\n  build[Build]\n    Do the work.\n    skills: shared\n";
    let _: WorkflowDto = far
        .json(
            post_json(
                "/v1/workflows",
                json!({"name": "far-flow", "document": far_flow}),
            ),
            StatusCode::CREATED,
        )
        .await;

    let (status, _) = local.send(delete("/v1/skills/shared")).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    eventually(TIMEOUT, "the refused delete failed", || async {
        let sync = sync_of(&local).await;
        sync.state == SyncState::Failed
            && sync
                .error
                .as_deref()
                .is_some_and(|error| error.contains("skill shared"))
    })
    .await;

    let _: SkillDto = local
        .json(
            post_json(
                "/v1/skills",
                json!({"name": "later", "document": skill("later", "after")}),
            ),
            StatusCode::CREATED,
        )
        .await;
    eventually(TIMEOUT, "the later skill on the host", || async {
        skill_on(&far, "later").await.is_some()
    })
    .await;
    assert_eq!(sync_of(&local).await.state, SyncState::Failed);
    assert!(skill_on(&far, "shared").await.is_some());

    // Once the host lets it go, the delete that was refused lands.
    let (status, _) = far.send(delete("/v1/workflows/far-flow")).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    until_in_sync(&local).await;
    assert!(skill_on(&far, "shared").await.is_none());
    assert_eq!(sync_of(&local).await.error, None);
}

/// A newer push of an item ends its older failed push, so no retry sends
/// the older version over the newer one.
#[tokio::test]
async fn a_newer_push_of_an_item_ends_its_older_failed_push() {
    let far = harness().ai_permissions_hardware(roomy_hardware()).await;
    let gate = Gate::new();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    serve_gated(&far, listener, &gate);
    let (local, _dir) = local_with_host(&format!("http://{address}")).await;
    let _: SkillDto = local
        .json(
            post_json(
                "/v1/skills",
                json!({"name": "shared", "document": skill("shared", "first")}),
            ),
            StatusCode::CREATED,
        )
        .await;
    local.state.hosts.start();
    until_in_sync(&local).await;
    // A workflow only the host has names the skill, so the host refuses to
    // delete it.
    let far_flow = "workflow far-flow\n  build[Build]\n    Do the work.\n    skills: shared\n";
    let _: WorkflowDto = far
        .json(
            post_json(
                "/v1/workflows",
                json!({"name": "far-flow", "document": far_flow}),
            ),
            StatusCode::CREATED,
        )
        .await;

    // The host holds the push of another skill, so both changes of `shared`
    // are queued before either runs.
    gate.hold();
    let _: SkillDto = local
        .json(
            post_json(
                "/v1/skills",
                json!({"name": "lead", "document": skill("lead", "held")}),
            ),
            StatusCode::CREATED,
        )
        .await;
    eventually(TIMEOUT, "the push of the other skill held", || async {
        let held = gate.held_requests.lock().unwrap();
        held.iter().any(|request| request == "PUT /v1/skills/lead")
    })
    .await;
    let (status, _) = local.send(delete("/v1/skills/shared")).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let _: SkillDto = local
        .json(
            post_json(
                "/v1/skills",
                json!({"name": "shared", "document": skill("shared", "second")}),
            ),
            StatusCode::CREATED,
        )
        .await;
    // The host refuses the delete. Then it would take it, if it came again.
    gate.pass_until("DELETE /v1/skills/shared").await;
    let (status, _) = far.send(delete("/v1/workflows/far-flow")).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    gate.release();

    until_in_sync(&local).await;
    let shared = skill_on(&far, "shared").await;
    assert_eq!(shared.map(|skill| skill.summary).as_deref(), Some("second"));
}

#[tokio::test]
async fn a_failed_push_is_failed_with_its_error_and_tried_again_until_it_lands() {
    // A port nothing listens on yet.
    let address = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap()
    };
    let (local, _dir) = local_with_host(&format!("http://{address}")).await;
    let _: SkillDto = local
        .json(
            post_json(
                "/v1/skills",
                json!({"name": "mine", "document": skill("mine", "local")}),
            ),
            StatusCode::CREATED,
        )
        .await;
    local.state.hosts.start();
    eventually(TIMEOUT, "the push failed", || async {
        let sync = sync_of(&local).await;
        sync.state == SyncState::Failed
            && sync
                .error
                .as_deref()
                .is_some_and(|error| error.contains("cannot reach"))
    })
    .await;

    let far = harness().ai_permissions_hardware(roomy_hardware()).await;
    serve(&far, tokio::net::TcpListener::bind(address).await.unwrap());
    until_in_sync(&local).await;
    assert_eq!(skill_on(&far, "mine").await.unwrap().summary, "local");
    assert_eq!(sync_of(&local).await.error, None);
}

/// The sync of the next `host_updated` on the bus.
async fn next_sync(rx: &mut Receiver<BusEvent>) -> HostSyncDto {
    let event = next_event(rx, |event| {
        matches!(event.event, DomainEvent::HostUpdated(_))
    })
    .await;
    match event.event {
        DomainEvent::HostUpdated(host) => host.sync,
        _ => unreachable!(),
    }
}

#[tokio::test]
async fn each_change_of_the_sync_publishes_one_host_updated() {
    let (_far, local, _dir) = far_and_local().await;
    let mut rx = local.bus.subscribe();
    local.state.hosts.start();
    assert_eq!(next_sync(&mut rx).await, HostSyncDto::in_sync());

    let _: SkillDto = local
        .json(
            post_json(
                "/v1/skills",
                json!({"name": "later", "document": skill("later", "first")}),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert_eq!(next_sync(&mut rx).await.state, SyncState::Pending);
    assert_eq!(next_sync(&mut rx).await, HostSyncDto::in_sync());
}
