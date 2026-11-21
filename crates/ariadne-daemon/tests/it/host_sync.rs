//! The sync of this daemon's catalogs and settings to each host (037).
//!
//! Two harness daemons: the far one served on a loopback TCP port, and the
//! local one with a `direct` host that points at it.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::common::{
    Harness, QUIET, RUNS_OUT, TIMEOUT, delete, eventually, eventually_some, get, harness,
    next_event, post_json, put_json, roomy_hardware, sh,
};
use ariadne_api::hosts::{HostDto, HostStatus, HostSyncDto, MissingRepositoryDto, SyncState};
use ariadne_api::permissions::{AiPermissionsStatusDto, LearnedPermissionsResponse};
use ariadne_api::repositories::RepositoryDto;
use ariadne_api::skills::SkillDto;
use ariadne_api::stream::DomainEvent;
use ariadne_api::workflows::WorkflowDto;
use ariadne_core::PermissionMode;
use ariadne_daemon::ai_permissions::hardware::HardwareOverride;
use ariadne_daemon::bus::BusEvent;
use ariadne_daemon::timeouts::Timeouts;
use ariadne_store::NewLearnedPermission;
use axum::http::StatusCode;
use axum::response::IntoResponse;
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
    local_with_host_on(endpoint, roomy_hardware()).await
}

/// [`local_with_host`] on the given hardware.
async fn local_with_host_on(
    endpoint: &str,
    hardware: HardwareOverride,
) -> (Harness, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(
        home.join("host-fixture.toml"),
        format!("[hosts.far]\nendpoint = \"{endpoint}\"\n"),
    )
    .unwrap();
    let h = harness()
        .ai_permissions_hardware(hardware)
        .home(home)
        .timeouts(Timeouts {
            host_sync_retry: RUNS_OUT,
            host_unanswered: RUNS_OUT,
            host_check: RUNS_OUT / 10,
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
    selected: Arc<Mutex<Vec<String>>>,
    health_off: Arc<AtomicBool>,
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
            selected: Arc::new(Mutex::new(Vec::new())),
            health_off: Arc::new(AtomicBool::new(false)),
            permits: Arc::new(Semaphore::new(0)),
            held_requests: Arc::new(Mutex::new(Vec::new())),
            answered: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn hold_only(&self, requests: &[&str]) {
        *self.selected.lock().unwrap() = requests.iter().map(|r| (*r).to_string()).collect();
        self.hold();
    }

    fn hold(&self) {
        self.held.store(true, Ordering::SeqCst);
    }

    /// Let the held requests through one at a time, each answered before
    /// the next, until `request` is answered.
    ///
    /// Every answer since the last permit is checked, not only the first:
    /// two can land between one look and the next, and the one asked for
    /// may be the second.
    async fn pass_until(&self, request: &str) {
        let mut seen = self.answered.lock().unwrap().len();
        loop {
            self.permits.add_permits(1);
            eventually(TIMEOUT, "the held request answered", || async {
                self.answered.lock().unwrap().len() > seen
            })
            .await;
            let answered = self.answered.lock().unwrap();
            if answered[seen..].iter().any(|answer| answer == request) {
                return;
            }
            seen = answered.len();
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
                if request.uri().path() == "/v1/health" {
                    if gate.health_off.load(Ordering::SeqCst) {
                        return StatusCode::SERVICE_UNAVAILABLE.into_response();
                    }
                    return next.run(request).await;
                }
                let selected = {
                    let requests = gate.selected.lock().unwrap();
                    requests.is_empty() || requests.contains(&what)
                };
                if gate.held.load(Ordering::SeqCst) && selected {
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

/// The sync sends the flavour and never the device. A device set on the host
/// by `PUT` with `device` alone stays through a push with the same flavour:
/// a local change of a threshold (a host that becomes `ready` again is
/// `ssh_forward.rs`'s). A local change of the flavour makes the
/// host pick its best device. Local's own best device is `cuda` and the
/// host's is `mlx`, so a device sent by the sync would show as `cuda`.
#[tokio::test]
async fn a_device_set_on_the_host_stays_until_local_changes_the_flavour() {
    let far = harness().ai_permissions_hardware(roomy_hardware()).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    serve(&far, listener);
    let endpoint = format!("http://{address}");
    let linux_cuda = HardwareOverride {
        os: "linux".into(),
        arch: "x86_64".into(),
        memory_gb: 64,
        gpu_name: Some("NVIDIA".into()),
        vram_gb: Some(24),
    };
    let (local, _dir) = local_with_host_on(&endpoint, linux_cuda).await;
    let _: AiPermissionsStatusDto = far
        .json(
            put_json("/v1/permissions/ai", json!({"device": "cpu"})),
            StatusCode::OK,
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
    let before: AiPermissionsStatusDto = local.get("/v1/permissions/ai").await;
    assert_eq!(before.device.as_str(), "cuda");

    local.state.hosts.start();
    until_in_sync(&local).await;
    let ai: AiPermissionsStatusDto = far.get("/v1/permissions/ai").await;
    assert_eq!((ai.allow_threshold, ai.deny_threshold), (0.125, 0.875));
    assert_eq!(ai.device.as_str(), "cpu", "the first push keeps the device");

    let _: AiPermissionsStatusDto = local
        .json(
            put_json("/v1/permissions/ai", json!({"deny_threshold": 0.75})),
            StatusCode::OK,
        )
        .await;
    eventually(TIMEOUT, "the new threshold on the host", || async {
        let ai: AiPermissionsStatusDto = far.get("/v1/permissions/ai").await;
        ai.deny_threshold == 0.75
    })
    .await;
    let ai: AiPermissionsStatusDto = far.get("/v1/permissions/ai").await;
    assert_eq!(ai.device.as_str(), "cpu", "a threshold change keeps it");

    let _: AiPermissionsStatusDto = local
        .json(
            put_json("/v1/permissions/ai", json!({"flavour": "0.8b"})),
            StatusCode::OK,
        )
        .await;
    eventually(TIMEOUT, "the new flavour on the host", || async {
        let ai: AiPermissionsStatusDto = far.get("/v1/permissions/ai").await;
        ai.flavour.as_str() == "0.8b"
    })
    .await;
    let ai: AiPermissionsStatusDto = far.get("/v1/permissions/ai").await;
    assert_eq!(ai.device.as_str(), "mlx", "the host's best device");
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
async fn a_refused_repository_holds_back_no_other_repository() {
    let (far, local, _dir) = far_and_local().await;
    forge_repository(
        &local,
        "a-widgets",
        "acme/widgets",
        json!({"base_branch": "next"}),
    )
    .await;
    forge_repository(
        &local,
        "b-gadgets",
        "acme/gadgets",
        json!({"description": "Local words."}),
    )
    .await;
    let lacking = forge_repository(&local, "c-lacking", "acme/lacking", json!({})).await;
    let far_widgets = forge_repository(&far, "a-widgets", "acme/widgets", json!({})).await;
    sh(Path::new(&far_widgets.path), "git branch -D next");
    let far_gadgets = forge_repository(&far, "b-gadgets", "acme/gadgets", json!({})).await;
    local.state.hosts.start();

    let expected = vec![MissingRepositoryDto {
        id: lacking.id.clone(),
        forge: Some("acme/lacking".into()),
    }];
    eventually(
        TIMEOUT,
        "the host failed with the missing checkout listed",
        || async {
            let sync = sync_of(&local).await;
            sync.state == SyncState::Failed && sync.missing_repositories == expected
        },
    )
    .await;
    eventually(
        TIMEOUT,
        "the other repository took its settings",
        || async {
            let after: RepositoryDto = far
                .get(&format!("/v1/repositories/{}", far_gadgets.id))
                .await;
            after.description.as_deref() == Some("Local words.")
        },
    )
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
    // A retry of the refused delete takes it off the failed list while it
    // runs, so the sync reads `pending` for that moment. Where it settles is
    // the claim.
    let settled = eventually_some(TIMEOUT, "the sync settled", || async {
        let state = sync_of(&local).await.state;
        (state != SyncState::Pending).then_some(state)
    })
    .await;
    assert_eq!(settled, SyncState::Failed);
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

/// The full host in the next sync change, retained for failure diagnostics.
async fn next_sync_host(rx: &mut Receiver<BusEvent>, last: &mut Option<HostDto>) -> HostSyncDto {
    loop {
        let event = next_event(rx, |event| {
            matches!(event.event, DomainEvent::HostUpdated(_))
        })
        .await;
        let DomainEvent::HostUpdated(host) = event.event else {
            unreachable!()
        };
        let previous = last.replace(host.clone());
        let Some(previous) = previous else {
            return host.sync;
        };
        assert_ne!(
            previous, host,
            "a host_updated that moved nothing: {host:?}"
        );
        if previous.sync != host.sync {
            return host.sync;
        }
    }
}

#[tokio::test]
async fn each_change_of_the_sync_publishes_one_host_updated() {
    let (_far, local, _dir) = far_and_local().await;
    let mut rx = local.bus.subscribe();
    let mut last = None;
    local.state.hosts.start();
    // The first event is the host's `ready`, before any push lands.
    assert_eq!(
        next_sync_host(&mut rx, &mut last).await.state,
        SyncState::Pending
    );
    assert_eq!(
        next_sync_host(&mut rx, &mut last).await,
        HostSyncDto::in_sync()
    );

    let _: SkillDto = local
        .json(
            post_json(
                "/v1/skills",
                json!({"name": "later", "document": skill("later", "first")}),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert_eq!(
        next_sync_host(&mut rx, &mut last).await.state,
        SyncState::Pending
    );
    assert_eq!(
        next_sync_host(&mut rx, &mut last).await,
        HostSyncDto::in_sync()
    );
}

#[tokio::test]
async fn sync_stops_blocked_and_queued_pushes_until_the_host_recovers() {
    let far = harness().ai_permissions_hardware(roomy_hardware()).await;
    let gate = Gate::new();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    serve_gated(&far, listener, &gate);
    let (local, _dir) = local_with_host(&format!("http://{address}")).await;
    local.state.hosts.start();
    until_in_sync(&local).await;
    gate.hold_only(&["PUT /v1/skills/lead", "PUT /v1/skills/queued"]);
    for name in ["lead", "queued"] {
        let _: SkillDto = local
            .json(
                post_json(
                    "/v1/skills",
                    json!({"name": name, "document": skill(name, "local")}),
                ),
                StatusCode::CREATED,
            )
            .await;
        if name == "lead" {
            eventually(TIMEOUT, "the first push is blocked", || async {
                gate.held_requests
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|s| s == "PUT /v1/skills/lead")
            })
            .await;
        }
    }
    gate.health_off.store(true, Ordering::SeqCst);
    eventually(TIMEOUT, "the host is down", || async {
        let hosts: Vec<HostDto> = local.get("/v1/hosts").await;
        hosts[1].status == HostStatus::Down
    })
    .await;
    gate.permits.add_permits(1);
    tokio::time::sleep(crate::common::QUIET).await;
    assert!(
        !gate
            .held_requests
            .lock()
            .unwrap()
            .iter()
            .any(|s| s == "PUT /v1/skills/queued")
    );
    assert!(
        !gate
            .answered
            .lock()
            .unwrap()
            .iter()
            .any(|s| s == "PUT /v1/skills/lead")
    );
    gate.release();
    gate.health_off.store(false, Ordering::SeqCst);
    until_in_sync(&local).await;
    assert!(skill_on(&far, "lead").await.is_some());
    assert!(skill_on(&far, "queued").await.is_some());
}

async fn skill_deletion_after_recovery(interrupted: bool, recreate: bool) {
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
                json!({"name": "victim", "document": skill("victim", "remove me")}),
            ),
            StatusCode::CREATED,
        )
        .await;
    local.state.hosts.start();
    until_in_sync(&local).await;
    assert!(skill_on(&far, "victim").await.is_some());

    if interrupted {
        gate.hold_only(&["DELETE /v1/skills/victim"]);
    } else {
        gate.hold_only(&["PUT /v1/skills/lead"]);
        let _: SkillDto = local
            .json(
                post_json(
                    "/v1/skills",
                    json!({"name": "lead", "document": skill("lead", "held")}),
                ),
                StatusCode::CREATED,
            )
            .await;
        eventually(TIMEOUT, "the leading push held", || async {
            gate.held_requests
                .lock()
                .unwrap()
                .iter()
                .any(|request| request == "PUT /v1/skills/lead")
        })
        .await;
    }
    assert_eq!(
        local.send(delete("/v1/skills/victim")).await.0,
        StatusCode::NO_CONTENT
    );
    if interrupted {
        eventually(TIMEOUT, "the delete held before recovery", || async {
            gate.held_requests
                .lock()
                .unwrap()
                .iter()
                .any(|request| request == "DELETE /v1/skills/victim")
        })
        .await;
    }
    gate.health_off.store(true, Ordering::SeqCst);
    eventually(TIMEOUT, "the host down with the delete queued", || async {
        let hosts: Vec<HostDto> = local.get("/v1/hosts").await;
        hosts[1].status == HostStatus::Down
    })
    .await;
    if recreate {
        let _: SkillDto = local
            .json(
                post_json(
                    "/v1/skills",
                    json!({"name": "victim", "document": skill("victim", "recreated")}),
                ),
                StatusCode::CREATED,
            )
            .await;
    }
    gate.release();
    gate.health_off.store(false, Ordering::SeqCst);
    until_in_sync(&local).await;
    let victim = skill_on(&far, "victim").await;
    if recreate {
        assert_eq!(
            victim.map(|skill| skill.summary).as_deref(),
            Some("recreated")
        );
    } else {
        assert!(
            victim.is_none(),
            "recovery must deliver the queued deletion"
        );
    }
}

#[tokio::test]
async fn a_queued_skill_deletion_reaches_the_host_after_recovery() {
    skill_deletion_after_recovery(false, false).await;
}

#[tokio::test]
async fn an_interrupted_skill_deletion_reaches_the_host_after_recovery() {
    skill_deletion_after_recovery(true, false).await;
}

#[tokio::test]
async fn a_recreated_skill_supersedes_its_queued_deletion_after_recovery() {
    skill_deletion_after_recovery(false, true).await;
}

#[tokio::test]
async fn supporting_files_reach_the_worker_and_later_changes_replace_them() {
    let (far, local, _dir) = far_and_local().await;
    let _: SkillDto = local
        .json(
            post_json(
                "/v1/skills",
                json!({"name": "bundle", "document": skill("bundle", "bundle")}),
            ),
            StatusCode::CREATED,
        )
        .await;
    let source = local.launcher.cfg.skills_dir.join("bundle");
    std::fs::create_dir_all(source.join("scripts")).unwrap();
    std::fs::write(source.join("scripts/run"), b"first").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            source.join("scripts/run"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }
    local.state.hosts.start();
    until_in_sync(&local).await;
    let target = far.launcher.cfg.skills_dir.join("bundle/scripts/run");
    assert_eq!(std::fs::read(&target).unwrap(), b"first");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&target).unwrap().permissions().mode() & 0o111,
            0o111
        );
    }
    std::fs::write(source.join("scripts/run"), b"second").unwrap();
    eventually(TIMEOUT, "the supporting file update", || async {
        std::fs::read(&target).ok().as_deref() == Some(b"second")
    })
    .await;
    std::fs::remove_file(source.join("scripts/run")).unwrap();
    eventually(TIMEOUT, "the supporting file removal", || async {
        !target.exists()
    })
    .await;
}

#[tokio::test]
async fn an_invalid_bundle_preserves_the_worker_skill() {
    let far = harness().await;
    let _: SkillDto = far
        .json(
            post_json(
                "/v1/skills",
                json!({"name": "bundle", "document": skill("bundle", "old")}),
            ),
            StatusCode::CREATED,
        )
        .await;
    let (status, _) = far.send(put_json("/v1/skills/bundle", json!({"document": skill("bundle", "new"), "files": [{"path": "../outside", "bytes": [1], "executable": false}]}))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(skill_on(&far, "bundle").await.unwrap().summary, "old");
}

#[cfg(unix)]
#[tokio::test]
async fn an_escaping_link_preserves_the_worker_and_does_not_block_other_skills() {
    let (far, local, dir) = far_and_local().await;
    for h in [&far, &local] {
        let _: SkillDto = h.json(post_json("/v1/skills", json!({"name": "bundle", "document": skill("bundle", if std::ptr::eq(h, &far) { "old" } else { "new" })})), StatusCode::CREATED).await;
    }
    let outside = dir.path().join("secret");
    std::fs::write(&outside, "secret").unwrap();
    std::os::unix::fs::symlink(
        &outside,
        local.launcher.cfg.skills_dir.join("bundle/escape"),
    )
    .unwrap();
    local.state.hosts.start();
    eventually(TIMEOUT, "the escaped path reported", || async {
        sync_of(&local)
            .await
            .error
            .is_some_and(|e| e.contains("escape"))
    })
    .await;
    assert_eq!(skill_on(&far, "bundle").await.unwrap().summary, "old");
    let _: SkillDto = local
        .json(
            post_json(
                "/v1/skills",
                json!({"name": "unrelated", "document": skill("unrelated", "arrives")}),
            ),
            StatusCode::CREATED,
        )
        .await;
    eventually(TIMEOUT, "the unrelated skill", || async {
        skill_on(&far, "unrelated").await.is_some()
    })
    .await;
}

#[tokio::test]
async fn external_markdown_links_warn_without_copying_external_files() {
    let (far, local, _dir) = far_and_local().await;
    let document = format!(
        "{}\n[external](../external.txt)\n[web](https://example.com/file)\n",
        skill("bundle", "links")
    );
    let _: SkillDto = local
        .json(
            post_json(
                "/v1/skills",
                json!({"name": "bundle", "document": document}),
            ),
            StatusCode::CREATED,
        )
        .await;
    local.state.hosts.start();
    until_in_sync(&local).await;
    let status = sync_of(&local).await;
    let warning = status.error.unwrap();
    assert!(warning.contains("../external.txt"));
    assert!(!warning.contains("https://"));
    assert!(!far.launcher.cfg.skills_dir.join("external.txt").exists());
    assert_eq!(skill_on(&far, "bundle").await.unwrap().summary, "links");
}

#[tokio::test]
async fn failed_staging_preserves_the_previous_worker_files() {
    let far = harness().await;
    let _: SkillDto = far
        .json(
            post_json(
                "/v1/skills",
                json!({"name": "bundle", "document": skill("bundle", "old")}),
            ),
            StatusCode::CREATED,
        )
        .await;
    let path = far.launcher.cfg.skills_dir.join("bundle/data.bin");
    std::fs::write(&path, [0, 255, 17]).unwrap();
    let (status, _) = far.send(put_json("/v1/skills/bundle", json!({"document": skill("bundle", "new"), "files": [{"path": "collision", "bytes": [7], "executable": false}, {"path": "collision/child", "bytes": [8], "executable": false}]}))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(std::fs::read(path).unwrap(), [0, 255, 17]);
    assert_eq!(skill_on(&far, "bundle").await.unwrap().summary, "old");
    assert!(!far.launcher.cfg.skills_dir.join("bundle/new.bin").exists());
}

#[tokio::test]
async fn a_failed_skill_waits_for_backoff_despite_bundle_scans() {
    let far = harness().ai_permissions_hardware(roomy_hardware()).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (attempts, mut received) = tokio::sync::mpsc::unbounded_channel();
    let router = far.router.clone().layer(axum::middleware::from_fn(
        move |request: axum::extract::Request, next: axum::middleware::Next| {
            let attempts = attempts.clone();
            async move {
                if request.method() == axum::http::Method::PUT
                    && request.uri().path() == "/v1/skills/bundle"
                {
                    attempts.send(()).unwrap();
                    return StatusCode::INTERNAL_SERVER_ERROR.into_response();
                }
                next.run(request).await
            }
        },
    ));
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("host-fixture.toml"),
        format!("[hosts.far]\nendpoint = \"http://{address}\"\n"),
    )
    .unwrap();
    let local = harness()
        .home(dir.path().to_path_buf())
        .ai_permissions_hardware(roomy_hardware())
        .timeouts(Timeouts {
            host_check: RUNS_OUT / 100,
            host_sync_retry: RUNS_OUT,
            ..Timeouts::default()
        })
        .await;
    let _: SkillDto = local
        .json(
            post_json(
                "/v1/skills",
                json!({"name": "bundle", "document": skill("bundle", "refused")}),
            ),
            StatusCode::CREATED,
        )
        .await;
    local.state.hosts.start();
    tokio::time::timeout(TIMEOUT, received.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(
        tokio::time::timeout(QUIET, received.recv()).await.is_err(),
        "a bundle scan retried before backoff"
    );
    tokio::time::timeout(TIMEOUT, received.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(
        tokio::time::timeout(QUIET, received.recv()).await.is_err(),
        "a bundle scan bypassed the doubled backoff"
    );
    tokio::time::timeout(TIMEOUT, received.recv())
        .await
        .unwrap()
        .unwrap();
    server.abort();
}

#[tokio::test]
async fn incompatible_skill_api_reports_upgrade_and_recovers() {
    let far = harness().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let incompatible = Arc::new(AtomicBool::new(true));
    let flag = incompatible.clone();
    let router = far.router.clone().layer(axum::middleware::from_fn(
        move |request: axum::extract::Request, next: axum::middleware::Next| {
            let flag = flag.clone();
            async move {
                if request.method() == axum::http::Method::PUT
                    && request.uri().path() == "/v1/skills/bundle"
                    && flag.load(Ordering::SeqCst)
                {
                    return (StatusCode::UNPROCESSABLE_ENTITY, "Failed to deserialize the JSON body into the target type: unknown field `files`, expected `document` at line 1 column 10").into_response();
                }
                next.run(request).await
            }
        },
    ));
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let (local, _dir) = local_with_host(&endpoint).await;
    let _: SkillDto = local
        .json(
            post_json(
                "/v1/skills",
                json!({"name":"bundle", "document":skill("bundle", "new")}),
            ),
            StatusCode::CREATED,
        )
        .await;
    local.state.hosts.start();
    eventually(TIMEOUT, "upgrade instruction", || async {
        sync_of(&local).await.error.is_some_and(|e| {
            e.contains("far")
                && e.contains("bundle")
                && e.contains("upgrade")
                && e.contains("incompatible")
        })
    })
    .await;
    incompatible.store(false, Ordering::SeqCst);
    until_in_sync(&local).await;
    assert!(skill_on(&far, "bundle").await.is_some());
}
