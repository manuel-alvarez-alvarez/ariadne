//! Integration tests for `POST /v1/sessions/{id}/resize`.
//!
//! tmux is stubbed by a script that records its argv, so the assertions are on
//! the exact invocation the daemon makes. That is the contract here: a
//! detached pane only honours a size when sizing has been taken off tmux's
//! hands, and only gives it back to a client that attaches later if the hook
//! that does so went out with it. The geometry a real tmux ends up at is
//! checked against a real tmux in `managers.rs`.

use std::sync::Arc;
use std::time::Instant;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use tower::ServiceExt;

use ariadne_core::{AgentKind, Role, SessionStatus};
use ariadne_daemon::config::Config;
use ariadne_daemon::gitwt::GitManager;
use ariadne_daemon::http::{self, AppState};
use ariadne_daemon::launcher::Launcher;
use ariadne_daemon::logbuf::LogBuffer;
use ariadne_daemon::tmux::TmuxManager;
use ariadne_store::{AgentSession, NewGoal, NewProfile, NewRepository, NewSession, Store};

struct Harness {
    store: Store,
    router: Router,
    dir: tempfile::TempDir,
}

/// A harness whose `tmux` is a stub script: `has-session` succeeds while a
/// marker file exists, and every call appends its arguments to a log the test
/// reads back.
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
        launcher,
        sched_tx: None,
        events: bus,
        logs: LogBuffer::new(),
    };
    Harness {
        router: http::router(state),
        store,
        dir,
    }
}

fn write_tmux_stub(dir: &std::path::Path) -> TmuxManager {
    use std::os::unix::fs::PermissionsExt;

    let bin = dir.join("tmux-stub.sh");
    let script = format!(
        "#!/bin/sh\n\
         echo \"$@\" >> '{calls}'\n\
         case \"$1\" in\n\
         \x20 has-session) [ -f '{alive}' ] || exit 1 ;;\n\
         esac\n\
         exit 0\n",
        calls = dir.join("tmux-calls").display(),
        alive = dir.join("tmux-alive").display(),
    );
    std::fs::write(&bin, script).unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    TmuxManager::new(bin.display().to_string())
}

impl Harness {
    /// A running planner session bound to the tmux session `tmux_name`.
    async fn session(&self, tmux_name: &str) -> AgentSession {
        let planner = self
            .store
            .create_profile(NewProfile {
                name: "planner".into(),
                role: Role::Planner,
                agent_kind: Some(AgentKind::ClaudeCode),
                model: None,
                system_prompt: "You plan.".into(),
                prompts: vec![],
            })
            .await
            .unwrap();
        let repo = self
            .store
            .create_repository(NewRepository {
                path: "/tmp/repo".into(),
                base_branch: "main".into(),
                description: None,
                merge_strategy: Default::default(),
            })
            .await
            .unwrap();
        let goal = self
            .store
            .create_goal(NewGoal {
                title: "Ship the UI".into(),
                description: "desc".into(),
                planner_profile_id: planner.id.clone(),
                max_tasks: None,
                required_approvals: 1,
                repository_ids: vec![repo.id],
            })
            .await
            .unwrap();
        self.store
            .create_session(NewSession {
                goal_id: goal.id,
                task_id: None,
                role: Role::Planner,
                profile_id: planner.id,
                agent_kind: AgentKind::ClaudeCode,
                model: None,
                tmux_session: tmux_name.into(),
                worktree_path: None,
                review_round: None,
            })
            .await
            .unwrap()
    }

    /// Give the stub tmux a live pane.
    fn stub_alive(&self) {
        std::fs::write(self.dir.path().join("tmux-alive"), "").unwrap();
    }

    /// The argv of every `tmux` call the daemon made, one per line.
    fn tmux_calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.dir.path().join("tmux-calls"))
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    /// The sizing calls only — everything but the liveness probes.
    fn sizing_calls(&self) -> Vec<String> {
        self.tmux_calls()
            .into_iter()
            .filter(|call| !call.starts_with("has-session"))
            .collect()
    }
}

fn post_resize(session_id: &str, body: serde_json::Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(format!("/v1/sessions/{session_id}/resize"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

fn size(cols: u16, rows: u16) -> serde_json::Value {
    serde_json::json!({ "cols": cols, "rows": rows })
}

async fn error_code(response: axum::response::Response) -> String {
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
    payload["error"]["code"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn a_resize_sizes_the_window_and_leaves_a_client_free_to_resize_it_again() {
    let h = harness().await;
    let session = h.session("ariadne-resize").await;
    h.stub_alive();

    let response = h
        .router
        .clone()
        .oneshot(post_resize(&session.id, size(137, 41)))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    assert_eq!(
        h.sizing_calls(),
        vec![
            "set-hook -t ariadne-resize client-attached set-window-option -u window-size ; \
             set-window-option -t ariadne-resize window-size manual ; \
             resize-window -t ariadne-resize -x 137 -y 41"
                .to_string()
        ],
        "one invocation: the hook that gives sizing back to a client, the manual \
         sizing a detached pane needs, and the size itself"
    );
}

#[tokio::test]
async fn a_finished_session_refuses_a_resize() {
    let h = harness().await;
    let session = h.session("ariadne-finished").await;
    // Its pane is alive — a successor took the name over — so only the stored
    // status says this session is done with.
    h.stub_alive();
    h.store
        .set_session_status(&session.id, SessionStatus::Exited)
        .await
        .unwrap();

    let response = h
        .router
        .clone()
        .oneshot(post_resize(&session.id, size(120, 40)))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(error_code(response).await, "conflict");
    assert!(
        h.sizing_calls().is_empty(),
        "the pane of a session that is over is left alone"
    );
}

/// The status still says live but tmux is gone: the pane to resize does not
/// exist, and a stale name may belong to a successor's pane by now.
#[tokio::test]
async fn a_session_without_a_pane_refuses_a_resize() {
    let h = harness().await;
    let session = h.session("ariadne-no-pane").await;

    let response = h
        .router
        .clone()
        .oneshot(post_resize(&session.id, size(120, 40)))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert!(h.sizing_calls().is_empty());
}

/// A viewer that measured its panel wrong must not reach tmux with the answer:
/// a zero side is not a grid, and a pane is a real allocation per cell.
#[tokio::test]
async fn a_size_outside_the_bounds_is_rejected_before_tmux_sees_it() {
    let h = harness().await;
    let session = h.session("ariadne-bounds").await;
    h.stub_alive();

    for out_of_range in [size(0, 24), size(80, 0), size(501, 24), size(80, 501)] {
        let response = h
            .router
            .clone()
            .oneshot(post_resize(&session.id, out_of_range.clone()))
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "{out_of_range} is not a pane size"
        );
        assert_eq!(error_code(response).await, "invalid_request");
    }
    // The largest grid that is still a grid is one, and so is the cap itself.
    for accepted in [size(1, 1), size(500, 500)] {
        let response = h
            .router
            .clone()
            .oneshot(post_resize(&session.id, accepted.clone()))
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::NO_CONTENT,
            "{accepted} is within the bounds"
        );
    }

    assert_eq!(
        h.sizing_calls().len(),
        2,
        "only the two accepted sizes reached tmux: {:?}",
        h.sizing_calls()
    );
}

#[tokio::test]
async fn an_unknown_session_yields_the_standard_error_envelope() {
    let h = harness().await;

    let response = h
        .router
        .clone()
        .oneshot(post_resize("01ARZ3NDEKTSV4RRFFQ69G5FAV", size(120, 40)))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(error_code(response).await, "not_found");
}
