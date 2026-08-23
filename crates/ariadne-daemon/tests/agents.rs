//! Integration tests for the agent-configuration endpoints.
//!
//! The contract is that every agent kind is configured out of the box, that
//! its defaults stay readable beside the flags in force (so a client resets by
//! sending them back), and that an edit reaches the next launch — spawn and
//! resume alike — rather than only the sessions started afterwards.

use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use http_body_util::BodyExt;
use serde::de::DeserializeOwned;
use tower::ServiceExt;

use ariadne_api::agents::AgentConfigDto;
use ariadne_api::error::ErrorBody;
use ariadne_core::spawn_plan::SpawnPlanFile;
use ariadne_core::{AgentKind, Role, SessionStatus};
use ariadne_daemon::config::Config;
use ariadne_daemon::gitwt::GitManager;
use ariadne_daemon::http::{self, AppState};
use ariadne_daemon::launcher::Launcher;
use ariadne_daemon::logbuf::LogBuffer;
use ariadne_daemon::tmux::{TmuxManager, session_name};
use ariadne_store::{NewGoal, NewProfile, NewRepository, NewSession, NewTask, Store};

struct Harness {
    store: Store,
    launcher: Arc<Launcher>,
    router: Router,
    dir: tempfile::TempDir,
}

async fn harness() -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("test.db")).await.unwrap();
    let bus = ariadne_daemon::bus::start(store.clone());
    let cfg = Arc::new(Config::load(Some(dir.path().join("home"))).unwrap());
    let launcher = Arc::new(Launcher {
        cfg,
        store: store.clone(),
        tmux: write_tmux_stub(dir.path()),
        git: GitManager,
    });
    let state = AppState {
        store: store.clone(),
        started_at: Instant::now(),
        launcher: launcher.clone(),
        sched_tx: None,
        events: bus,
        logs: LogBuffer::new(),
    };
    Harness {
        store,
        launcher,
        router: http::router(state),
        dir,
    }
}

/// A `tmux` that has no sessions and records every command it is given, so a
/// test can read back the argv the launcher asked for.
fn write_tmux_stub(dir: &Path) -> TmuxManager {
    use std::os::unix::fs::PermissionsExt;

    let bin = dir.join("tmux-stub.sh");
    let script = format!(
        "#!/bin/sh\n\
         echo \"$@\" >> '{log}'\n\
         case \"$1\" in\n\
         \x20 has-session) exit 1 ;;\n\
         esac\n\
         exit 0\n",
        log = dir.join("tmux-commands.log").display(),
    );
    std::fs::write(&bin, script).unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    TmuxManager::new(bin.display().to_string())
}

impl Harness {
    async fn send(&self, request: Request<Body>) -> (StatusCode, Vec<u8>) {
        let response = self.router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        (status, body.to_vec())
    }

    async fn json<T: DeserializeOwned>(&self, request: Request<Body>, expected: StatusCode) -> T {
        let (status, body) = self.send(request).await;
        assert_eq!(status, expected, "{}", String::from_utf8_lossy(&body));
        serde_json::from_slice(&body).unwrap()
    }

    async fn error(&self, request: Request<Body>, expected: StatusCode) -> ErrorBody {
        let (status, body) = self.send(request).await;
        assert_eq!(status, expected, "{}", String::from_utf8_lossy(&body));
        serde_json::from_slice(&body).unwrap()
    }

    /// A claude_code engineer session with a conversation to resume: what the
    /// launcher relaunches when the reviewers bounce a task back.
    async fn resumable_engineer(&self) -> String {
        let planner = self.profile("planner", Role::Planner).await;
        let engineer = self.profile("engineer", Role::Engineer).await;
        let reviewer = self.profile("reviewer", Role::Reviewer).await;
        let repo = self
            .store
            .create_repository(NewRepository {
                path: self.dir.path().join("repo").display().to_string(),
                base_branch: "main".into(),
                description: None,
            })
            .await
            .unwrap();
        let goal = self
            .store
            .create_goal(NewGoal {
                title: "Ship it".into(),
                description: "desc".into(),
                planner_profile_id: planner,
                max_tasks: None,
                required_approvals: 1,
                repository_ids: vec![repo.id.clone()],
            })
            .await
            .unwrap();
        let task = self
            .store
            .create_task(NewTask {
                goal_id: goal.id.clone(),
                repo_id: repo.id,
                title: "task".into(),
                description: "do things".into(),
                engineer_profile_id: engineer.clone(),
                integrator_profile_id: None,
                reviewer_profile_ids: vec![reviewer],
                depends_on: vec![],
            })
            .await
            .unwrap();
        let worktree = self.dir.path().join("wt-eng");
        std::fs::create_dir_all(&worktree).unwrap();
        self.store
            .set_task_worktree(&task.id, Some(&worktree.display().to_string()))
            .await
            .unwrap();
        let session = self
            .store
            .create_session(NewSession {
                goal_id: goal.id.clone(),
                task_id: Some(task.id.clone()),
                role: Role::Engineer,
                profile_id: engineer,
                agent_kind: AgentKind::ClaudeCode,
                model: None,
                tmux_session: session_name(&goal.id, Some(&task.id), "engineer", None),
                worktree_path: Some(worktree.display().to_string()),
                review_round: None,
            })
            .await
            .unwrap();
        self.store
            .set_session_internal_id(&session.id, "uuid-1234")
            .await
            .unwrap();
        self.store
            .set_session_status(&session.id, SessionStatus::Exited)
            .await
            .unwrap();
        task.id
    }

    async fn profile(&self, name: &str, role: Role) -> String {
        self.store
            .create_profile(NewProfile {
                name: name.into(),
                role,
                agent_kind: Some(AgentKind::ClaudeCode),
                model: None,
                system_prompt: format!("You are {name}."),
                prompts: vec![],
            })
            .await
            .unwrap()
            .id
    }

    /// The argv of the last agent the launcher started, joined for reading.
    ///
    /// Read from the session's spawn plan: the tmux command line carries
    /// `ariadne _spawn <plan>` and none of the agent's own argv.
    fn launched_argv(&self, session_id: &str) -> String {
        let path = self
            .launcher
            .cfg
            .run_dir
            .join(session_id)
            .join("spawn.json");
        let raw = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
        SpawnPlanFile::from_json(&raw).unwrap().argv.join(" ")
    }
}

fn get(uri: &str) -> Request<Body> {
    Request::builder().uri(uri).body(Body::empty()).unwrap()
}

fn put_json(uri: &str, body: serde_json::Value) -> Request<Body> {
    Request::builder()
        .method(Method::PUT)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

#[tokio::test]
async fn every_agent_kind_is_listed_with_its_flags_and_its_defaults() {
    let h = harness().await;
    let configs: Vec<AgentConfigDto> = h.json(get("/v1/agents"), StatusCode::OK).await;
    assert_eq!(
        configs.iter().map(|c| c.agent_kind).collect::<Vec<_>>(),
        AgentKind::ALL.to_vec()
    );
    for config in &configs {
        assert_eq!(
            config.default_flags,
            config.agent_kind.default_flags(),
            "{:?}",
            config.agent_kind
        );
        // Nothing has been edited yet, so the two halves agree.
        assert_eq!(config.extra_flags, config.default_flags);
    }
}

/// The flags are replaced whole, an empty list included, and the defaults keep
/// being served beside them: that is what a "restore defaults" button sends.
#[tokio::test]
async fn flags_are_replaced_whole_and_the_defaults_stay_readable() {
    let h = harness().await;
    let updated: AgentConfigDto = h
        .json(
            put_json(
                "/v1/agents/claude_code",
                serde_json::json!({"extra_flags": ["--permission-mode=acceptEdits"]}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(updated.extra_flags, ["--permission-mode=acceptEdits"]);
    assert_eq!(updated.default_flags, ["--dangerously-skip-permissions"]);

    let emptied: AgentConfigDto = h
        .json(
            put_json("/v1/agents/codex", serde_json::json!({"extra_flags": []})),
            StatusCode::OK,
        )
        .await;
    assert!(emptied.extra_flags.is_empty());

    // Restoring is the same call with the defaults the GET handed out.
    let restored: AgentConfigDto = h
        .json(
            put_json(
                "/v1/agents/codex",
                serde_json::json!({"extra_flags": emptied.default_flags}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(
        restored.extra_flags,
        ["--dangerously-bypass-approvals-and-sandbox"]
    );

    let configs: Vec<AgentConfigDto> = h.json(get("/v1/agents"), StatusCode::OK).await;
    assert_eq!(
        configs[0].extra_flags,
        ["--permission-mode=acceptEdits"],
        "the edit survived the round trip"
    );
}

#[tokio::test]
async fn an_unknown_agent_kind_is_refused_by_name() {
    let h = harness().await;
    let err = h
        .error(
            put_json("/v1/agents/emacs", serde_json::json!({"extra_flags": []})),
            StatusCode::BAD_REQUEST,
        )
        .await;
    assert_eq!(err.error.code, "invalid_request");
    assert!(err.error.message.contains("emacs"), "{}", err.error.message);
    assert!(
        err.error.message.contains("claude_code"),
        "{}",
        err.error.message
    );
}

/// The point of the whole move: what the config says is what the agent is
/// launched with, on the spawn path and the resume path alike.
#[tokio::test]
async fn a_launch_takes_its_flags_from_the_agent_config() {
    let h = harness().await;
    let task = h.resumable_engineer().await;

    let session = h
        .launcher
        .resume_engineer(&task, "Round 1: please fix things.")
        .await
        .unwrap();
    let argv = h.launched_argv(&session.id);
    assert_eq!(
        argv.matches("--dangerously-skip-permissions").count(),
        1,
        "the seeded bypass, exactly once: {argv}"
    );

    // Edited over REST, the next launch of the same session picks it up.
    let _: AgentConfigDto = h
        .json(
            put_json(
                "/v1/agents/claude_code",
                serde_json::json!({"extra_flags": ["--permission-mode=acceptEdits"]}),
            ),
            StatusCode::OK,
        )
        .await;
    let session = h
        .launcher
        .resume_engineer(&task, "Round 2: please fix things.")
        .await
        .unwrap();
    let argv = h.launched_argv(&session.id);
    assert!(
        argv.contains("--permission-mode=acceptEdits"),
        "the edited flags: {argv}"
    );
    assert!(
        !argv.contains("--dangerously-skip-permissions"),
        "the flag the user dropped is gone: {argv}"
    );
}
