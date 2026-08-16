//! Integration tests for `GET /v1/sessions/{id}/logs/stream`.
//!
//! No tmux needed: the sessions here point at a tmux name that does not
//! exist, which is exactly the "session already over" path — the one whose
//! framing and lifecycle the acceptance criteria pin down. Following a live
//! pane is the tailing logic, unit-tested in `logtail`.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use tokio::io::AsyncWriteExt;
use tower::ServiceExt;

use ariadne_core::{AgentKind, Role, SessionStatus};
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
    dir: tempfile::TempDir,
}

async fn harness() -> Harness {
    build(false).await
}

/// A harness whose `tmux` is a stub script: `has-session` succeeds while a
/// marker file exists and `capture-pane` prints a file the test controls.
/// That makes the live path — a running pane whose session ends underneath it
/// — reproducible without tmux and without a real agent.
async fn harness_with_stub_tmux() -> Harness {
    build(true).await
}

async fn build(stub_tmux: bool) -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("test.db")).await.unwrap();
    let bus = ariadne_daemon::bus::start(store.clone());
    let cfg = Arc::new(Config::load(Some(dir.path().join("home"))).unwrap());
    let tmux = if stub_tmux {
        write_tmux_stub(dir.path())
    } else {
        TmuxManager::default()
    };
    let launcher = Arc::new(Launcher {
        cfg,
        store: store.clone(),
        tmux,
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
        dir,
    }
}

fn write_tmux_stub(dir: &std::path::Path) -> TmuxManager {
    use std::os::unix::fs::PermissionsExt;

    let bin = dir.join("tmux-stub.sh");
    let script = format!(
        "#!/bin/sh\n\
         case \"$1\" in\n\
         \x20 has-session) [ -f '{alive}' ] || exit 1 ;;\n\
         \x20 capture-pane) cat '{pane}' 2>/dev/null ;;\n\
         esac\n\
         exit 0\n",
        alive = dir.join("tmux-alive").display(),
        pane = dir.join("pane.txt").display(),
    );
    std::fs::write(&bin, script).unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    TmuxManager::new(bin.display().to_string())
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
    fn write_console_log(&self, session_id: &str, contents: impl AsRef<[u8]>) {
        let path = self.console_log(session_id);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    fn console_log(&self, session_id: &str) -> PathBuf {
        self.launcher
            .cfg
            .run_dir
            .join(session_id)
            .join("console.log")
    }

    /// What the stub tmux's `capture-pane` prints, and whether it has a
    /// session at all.
    fn stub_pane(&self, contents: &str) {
        std::fs::write(self.dir.path().join("pane.txt"), contents).unwrap();
        std::fs::write(self.dir.path().join("tmux-alive"), "").unwrap();
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

/// tmux session names are reused: `revive_session` and `resume_engineer`
/// start the successor under the dead session's name. Asking for the old
/// session's logs must yield *its* console log, never the pane its successor
/// is now drawing.
#[tokio::test]
async fn an_exited_session_ignores_the_pane_that_took_over_its_name() {
    let h = harness_with_stub_tmux().await;
    let session = h.session("ariadne-reused-name").await;
    h.store
        .set_session_status(&session.id, SessionStatus::Exited)
        .await
        .unwrap();
    h.write_console_log(&session.id, "the old session's output\n");
    // Its successor is live under the very same tmux name.
    h.stub_pane("the successor's pane\n");

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
    assert_eq!(
        payload["chunk"], "the old session's output\n",
        "an exited session serves its own console log, not the live pane"
    );

    let (name, _) = parse(&next_sse_message(&mut body).await);
    assert_eq!(name, "end", "a session that is already over ends at once");
    let eof = tokio::time::timeout(TIMEOUT, body.frame()).await.unwrap();
    assert!(eof.is_none());
}

/// The stream must not be kept alive by its own traffic: a pane writing on
/// every poll still has to notice that the session behind it is finished.
#[tokio::test]
async fn a_terminal_status_ends_the_stream_even_while_output_keeps_coming() {
    let h = harness_with_stub_tmux().await;
    let session = h.session("ariadne-chatty").await;
    h.stub_pane("pane snapshot\n");
    h.write_console_log(&session.id, "");

    // Output on every single poll, for longer than this test can run.
    let log = h.console_log(&session.id);
    tokio::spawn(async move {
        for i in 0..1_000 {
            let mut file = tokio::fs::OpenOptions::new()
                .append(true)
                .open(&log)
                .await
                .unwrap();
            file.write_all(format!("tick {i}\n").as_bytes())
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    });

    let response = h
        .router
        .clone()
        .oneshot(get(&format!("/v1/sessions/{}/logs/stream", session.id)))
        .await
        .unwrap();
    let mut body = response.into_body();

    let (name, payload) = parse(&next_sse_message(&mut body).await);
    assert_eq!(name, "snapshot");
    assert_eq!(payload["chunk"], "pane snapshot\n");
    let (name, _) = parse(&next_sse_message(&mut body).await);
    assert_eq!(name, "delta", "the pane is producing output");

    // The process is gone as far as the daemon is concerned — the pane in
    // tmux (the stub still has one) is no longer this session's.
    h.store
        .set_session_status(&session.id, SessionStatus::Failed)
        .await
        .unwrap();

    let mut last = String::new();
    for _ in 0..40 {
        let (name, _) = parse(&next_sse_message(&mut body).await);
        last = name;
        if last == "end" {
            break;
        }
    }
    assert_eq!(last, "end", "a terminal status ends the stream");
    let eof = tokio::time::timeout(TIMEOUT, body.frame()).await.unwrap();
    assert!(eof.is_none(), "nothing follows the end event");
}

/// pipe-pane can stop mid-character. Those bytes are part of "whatever
/// remains", so they go out lossily instead of vanishing.
#[tokio::test]
async fn a_half_written_character_still_reaches_the_client() {
    let h = harness().await;
    let session = h.dead_session().await;
    let mut console = b"cut off: ".to_vec();
    // Two thirds of a three-byte character.
    console.extend_from_slice(&"│".as_bytes()[..2]);
    h.write_console_log(&session.id, &console);

    let response = h
        .router
        .clone()
        .oneshot(get(&format!("/v1/sessions/{}/logs/stream", session.id)))
        .await
        .unwrap();
    let mut body = response.into_body();

    let (name, payload) = parse(&next_sse_message(&mut body).await);
    assert_eq!(name, "snapshot");
    assert_eq!(
        payload["chunk"], "cut off: \u{fffd}",
        "the truncated character is replaced, not dropped"
    );
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
