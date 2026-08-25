//! Integration tests for `POST /v1/sessions/{id}/input`.
//!
//! tmux is stubbed by a script that records its argv, so the assertions are on
//! the exact `send-keys` invocation the daemon makes — which is the whole
//! contract: control bytes and escape sequences have to reach the pane
//! unchanged, and nothing may be appended to them.

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
                system_prompt: Some("You plan.".into()),
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

    /// The `send-keys` calls only.
    fn send_keys_calls(&self) -> Vec<String> {
        self.tmux_calls()
            .into_iter()
            .filter(|call| call.starts_with("send-keys"))
            .collect()
    }
}

fn post_input(session_id: &str, data: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(format!("/v1/sessions/{session_id}/input"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::json!({ "data": data }).to_string()))
        .unwrap()
}

/// Hex-encoded argument list for `send-keys -H`, as the stub records it.
fn hex(data: &str) -> String {
    data.bytes()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[tokio::test]
async fn typing_reaches_the_pane_byte_for_byte() {
    let h = harness().await;
    let session = h.session("ariadne-typing").await;
    h.stub_alive();

    // What a terminal in front of a user actually emits: printable text, a
    // Return, a Ctrl-C, an Up-arrow escape sequence, and a multi-byte glyph.
    let typed = "ls -la\r\u{3}\u{1b}[A│";
    let response = h
        .router
        .clone()
        .oneshot(post_input(&session.id, typed))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    assert_eq!(
        h.send_keys_calls(),
        vec![format!("send-keys -t ariadne-typing -H {}", hex(typed))],
        "the input goes out as one send-keys, hex-encoded, with nothing appended"
    );
}

/// `send_submitted` presses Enter of its own; this endpoint must not — the
/// terminal already sends its own `\r` when the user presses Return.
#[tokio::test]
async fn nothing_is_appended_to_the_input() {
    let h = harness().await;
    let session = h.session("ariadne-no-enter").await;
    h.stub_alive();

    let response = h
        .router
        .clone()
        .oneshot(post_input(&session.id, "hi"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let calls = h.send_keys_calls();
    assert_eq!(calls.len(), 1, "one call, not a keystroke plus an Enter");
    assert!(
        !calls[0].contains("Enter"),
        "no key-name Enter is sent: {calls:?}"
    );
}

#[tokio::test]
async fn a_finished_session_refuses_input() {
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
        .oneshot(post_input(&session.id, "rm -rf /\r"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload["error"]["code"], "conflict");
    assert!(
        h.send_keys_calls().is_empty(),
        "nothing is typed into the pane of a session that is over"
    );
}

/// The status still says live but tmux is gone: the pane the daemon would
/// type into does not exist, so the request fails rather than silently doing
/// nothing.
#[tokio::test]
async fn a_session_without_a_pane_refuses_input() {
    let h = harness().await;
    let session = h.session("ariadne-no-pane").await;

    let response = h
        .router
        .clone()
        .oneshot(post_input(&session.id, "hello"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert!(h.send_keys_calls().is_empty());
}

#[tokio::test]
async fn an_unknown_session_yields_the_standard_error_envelope() {
    let h = harness().await;

    let response = h
        .router
        .clone()
        .oneshot(post_input("01ARZ3NDEKTSV4RRFFQ69G5FAV", "hello"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload["error"]["code"], "not_found");
}

/// A paste is longer than one argv can comfortably hold, so it is split — but
/// only into consecutive batches, in order, with no byte lost or duplicated.
#[tokio::test]
async fn a_long_paste_is_split_into_ordered_batches() {
    let h = harness().await;
    let session = h.session("ariadne-paste").await;
    h.stub_alive();

    let pasted: String = (0..1500)
        .map(|i| char::from(b'a' + (i % 26) as u8))
        .collect();
    let response = h
        .router
        .clone()
        .oneshot(post_input(&session.id, &pasted))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let sent: String = h
        .send_keys_calls()
        .iter()
        .flat_map(|call| {
            let hexes = call.split(" -H ").nth(1).expect("a -H payload").to_string();
            hexes
                .split_whitespace()
                .map(|byte| u8::from_str_radix(byte, 16).unwrap() as char)
                .collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(sent, pasted, "the batches reassemble into what was pasted");
    assert!(
        h.send_keys_calls().len() > 1,
        "1500 bytes do not fit one batch"
    );
}
