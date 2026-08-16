//! Integration tests for `GET /v1/sessions/{id}/logs/stream`.
//!
//! No tmux needed: the sessions here point at a tmux name that does not
//! exist, which is exactly the "session already over" path — the one whose
//! framing and lifecycle the acceptance criteria pin down. Following a live
//! pane is the tailing logic, unit-tested in `logtail`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use tower::ServiceExt;

use ariadne_core::{AgentKind, Role};
use ariadne_daemon::config::Config;
use ariadne_daemon::gitwt::GitManager;
use ariadne_daemon::http::{self, AppState};
use ariadne_daemon::launcher::Launcher;
use ariadne_daemon::tmux::{TmuxManager, TmuxSpawn};
use ariadne_store::{AgentSession, NewGoal, NewProfile, NewSession, Store};

const TIMEOUT: Duration = Duration::from_secs(5);

struct Harness {
    store: Store,
    launcher: Arc<Launcher>,
    router: Router,
    _dir: tempfile::TempDir,
}

async fn harness() -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("test.db")).await.unwrap();
    let bus = ariadne_daemon::bus::start(store.clone());
    let cfg = Arc::new(Config::load(Some(dir.path().join("home"))).unwrap());
    let launcher = Arc::new(Launcher {
        cfg,
        store: store.clone(),
        tmux: TmuxManager::default(),
        git: GitManager,
    });
    let state = AppState {
        store: store.clone(),
        started_at: Instant::now(),
        launcher: launcher.clone(),
        sched_tx: None,
        events: bus,
    };
    Harness {
        router: http::router(state),
        store,
        launcher,
        _dir: dir,
    }
}

impl Harness {
    /// A session whose tmux is not (and never was) running.
    async fn dead_session(&self) -> AgentSession {
        // Deliberately not a live tmux session.
        self.session("ariadne-test-no-such-session").await
    }

    /// A planner session bound to the tmux session `tmux_name`.
    async fn session(&self, tmux_name: &str) -> AgentSession {
        let planner = self
            .store
            .create_profile(NewProfile {
                name: "planner".into(),
                role: Role::Planner,
                agent_kind: Some(AgentKind::ClaudeCode),
                model: None,
                system_prompt: "You plan.".into(),
                extra_flags: vec![],
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
                repos: vec![("/tmp/repo".into(), "main".into())],
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
                tmux_session: tmux_name.into(),
                worktree_path: None,
                review_round: None,
            })
            .await
            .unwrap()
    }

    /// Write the console log tmux `pipe-pane` would have produced.
    fn write_console_log(&self, session_id: &str, contents: &str) {
        let dir = self.launcher.cfg.run_dir.join(session_id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("console.log"), contents).unwrap();
    }
}

fn get(uri: &str) -> Request<Body> {
    Request::builder().uri(uri).body(Body::empty()).unwrap()
}

/// Read from an SSE body until one complete message (`\n\n`-terminated) is in.
async fn next_sse_message(body: &mut Body) -> String {
    tokio::time::timeout(TIMEOUT, async {
        let mut buf = String::new();
        while let Some(frame) = body.frame().await {
            let frame = frame.expect("sse body error");
            if let Some(chunk) = frame.data_ref() {
                buf.push_str(&String::from_utf8_lossy(chunk));
                if buf.contains("\n\n") {
                    return buf;
                }
            }
        }
        panic!("sse stream ended before a complete message: {buf:?}");
    })
    .await
    .expect("timed out waiting for an sse message")
}

/// `event:` name and decoded `data:` payload of one SSE message.
fn parse(message: &str) -> (String, serde_json::Value) {
    let mut name = None;
    let mut data = None;
    for line in message.trim_end().lines() {
        if let Some(rest) = line.strip_prefix("event: ") {
            name = Some(rest.to_string());
        } else if let Some(rest) = line.strip_prefix("data: ") {
            assert!(
                data.is_none(),
                "payload must fit one data line: {message:?}"
            );
            data = Some(rest.to_string());
        }
    }
    let name = name.expect("every message carries an event name");
    let data = data.expect("every message carries a payload");
    (name, serde_json::from_str(&data).expect("payload is JSON"))
}

#[tokio::test]
async fn an_exited_session_yields_its_full_log_then_ends() {
    let h = harness().await;
    let session = h.dead_session().await;
    // Raw terminal output: escape sequences, newlines, carriage returns and
    // a multi-byte glyph — none of which SSE framing tolerates unencoded.
    let console = "\u{1b}[2J\u{1b}[Hbuilding…\r\n│ done │\n\u{7}";
    h.write_console_log(&session.id, console);

    let response = h
        .router
        .clone()
        .oneshot(get(&format!("/v1/sessions/{}/logs/stream", session.id)))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some("text/event-stream")
    );
    let mut body = response.into_body();

    let (name, payload) = parse(&next_sse_message(&mut body).await);
    assert_eq!(name, "snapshot");
    assert_eq!(
        payload["chunk"], console,
        "the snapshot round-trips the console log byte for byte"
    );

    let (name, payload) = parse(&next_sse_message(&mut body).await);
    assert_eq!(name, "end");
    assert_eq!(payload["session_id"], session.id);

    let end = tokio::time::timeout(TIMEOUT, body.frame())
        .await
        .expect("a dead session's stream must close, not hang");
    assert!(end.is_none(), "nothing follows the end event");
}

/// Nothing was ever piped: the client still gets a well-formed (empty)
/// snapshot and a clean end rather than a hanging connection.
#[tokio::test]
async fn an_exited_session_without_a_console_log_still_ends() {
    let h = harness().await;
    let session = h.dead_session().await;

    let response = h
        .router
        .clone()
        .oneshot(get(&format!("/v1/sessions/{}/logs/stream", session.id)))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let mut body = response.into_body();

    let (name, payload) = parse(&next_sse_message(&mut body).await);
    assert_eq!(name, "snapshot");
    assert_eq!(payload["chunk"], "");
    let (name, _) = parse(&next_sse_message(&mut body).await);
    assert_eq!(name, "end");
}

/// The live path end to end: pane output shows up as deltas within a second
/// of being written, and killing the session closes the stream with `end`.
#[tokio::test]
#[ignore = "requires tmux"]
async fn a_live_session_streams_new_output_until_it_is_killed() {
    let h = harness().await;
    let tmux_name = format!("ariadne-test-logstream-{}", std::process::id());
    let session = h.session(&tmux_name).await;
    let run_dir = h.launcher.cfg.run_dir.join(&session.id);
    std::fs::create_dir_all(&run_dir).unwrap();

    // Emits forever: pipe-pane only sees output produced after it attaches.
    h.launcher
        .tmux
        .new_session(&TmuxSpawn {
            session: tmux_name.clone(),
            cwd: run_dir.clone(),
            env: vec![],
            argv: vec![
                "sh".into(),
                "-c".into(),
                "while true; do echo tick; sleep 0.2; done".into(),
            ],
            log_file: Some(run_dir.join("console.log")),
        })
        .await
        .unwrap();

    let response = h
        .router
        .clone()
        .oneshot(get(&format!("/v1/sessions/{}/logs/stream", session.id)))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let mut body = response.into_body();

    let (name, _) = parse(&next_sse_message(&mut body).await);
    assert_eq!(name, "snapshot", "the pane snapshot comes first");

    // New output reaches the client as a delta, not as a fresh snapshot.
    let (name, payload) = parse(&next_sse_message(&mut body).await);
    assert_eq!(name, "delta");
    assert!(
        payload["chunk"].as_str().unwrap().contains("tick"),
        "delta carries the new pane output: {payload}"
    );

    h.launcher.tmux.kill_session(&tmux_name).await.unwrap();

    // Trailing output may still be drained; `end` is what closes the stream.
    let mut last = String::new();
    for _ in 0..10 {
        let (name, _) = parse(&next_sse_message(&mut body).await);
        last = name;
        if last == "end" {
            break;
        }
    }
    assert_eq!(last, "end", "killing the session ends the stream");
    let eof = tokio::time::timeout(TIMEOUT, body.frame())
        .await
        .expect("the stream must close after end");
    assert!(eof.is_none(), "nothing follows the end event");
}

#[tokio::test]
async fn an_unknown_session_yields_the_standard_error_envelope() {
    let h = harness().await;

    let response = h
        .router
        .clone()
        .oneshot(get("/v1/sessions/01ARZ3NDEKTSV4RRFFQ69G5FAV/logs/stream"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload["error"]["code"], "not_found");
    assert!(payload["error"]["message"].is_string());
}
