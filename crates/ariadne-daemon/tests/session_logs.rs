//! Integration tests for `GET /v1/sessions/{id}/logs/stream`.
//!
//! No tmux needed: the sessions here point at a tmux name that does not
//! exist, which is exactly the "session already over" path — the one whose
//! framing and lifecycle the acceptance criteria pin down. Following a live
//! pane is the tailing logic, unit-tested in `logtail`.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
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
use ariadne_daemon::logbuf::LogBuffer;
use ariadne_daemon::tmux::{TmuxManager, TmuxSpawn};
use ariadne_store::{AgentSession, NewGoal, NewProfile, NewRepository, NewSession, Store};

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
/// marker file exists, `capture-pane` prints a file the test controls and
/// `display-message` prints the pane size from another. That makes the live
/// path — a running pane whose session ends or is resized underneath it —
/// reproducible without tmux and without a real agent.
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
        logs: LogBuffer::new(),
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
    // `capture-pane` and `display-message` can each be made to fail on their
    // own — a pane that is there but says nothing is a different thing from
    // one that is gone — and `capture-pane` can resize the pane as it reads
    // it, which is the one window a stream cannot otherwise reach into.
    let script = format!(
        "#!/bin/sh\n\
         case \"$1\" in\n\
         \x20 has-session) [ -f '{alive}' ] || exit 1 ;;\n\
         \x20 capture-pane)\n\
         \x20   [ -f '{no_capture}' ] && exit 1\n\
         \x20   if [ -f '{resize}' ]; then cat '{resize}' > '{size}'; rm '{resize}'; fi\n\
         \x20   cat '{pane}' 2>/dev/null ;;\n\
         \x20 display-message)\n\
         \x20   [ -f '{no_measure}' ] && exit 1\n\
         \x20   [ -f '{alive}' ] || exit 1\n\
         \x20   cat '{size}' 2>/dev/null ;;\n\
         esac\n\
         exit 0\n",
        alive = dir.join("tmux-alive").display(),
        no_capture = dir.join("capture-fails").display(),
        no_measure = dir.join("measure-fails").display(),
        resize = dir.join("resize-on-capture.txt").display(),
        pane = dir.join("pane.txt").display(),
        size = dir.join("pane-size.txt").display(),
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

    /// A pane to capture, on a session that exists. The pane is tmux's default
    /// 80×24 with its cursor at the bottom left until a test says otherwise.
    fn stub_pane(&self, contents: &str) {
        std::fs::write(self.dir.path().join("tmux-alive"), "").unwrap();
        self.stub_pane_geometry(80, 24, 0, 23);
        self.stub_pane_screen(contents);
    }

    /// What the stub tmux's `capture-pane` prints, leaving the geometry as it
    /// is — the two are set apart so that a test changing what the pane draws
    /// does not quietly change the grid it draws it at.
    fn stub_pane_screen(&self, contents: &str) {
        std::fs::write(self.dir.path().join("pane.txt"), contents).unwrap();
    }

    /// What the stub tmux's `display-message` reports about the pane's screen.
    fn stub_pane_geometry(&self, cols: u16, rows: u16, cursor_x: u16, cursor_y: u16) {
        std::fs::write(
            self.dir.path().join("pane-size.txt"),
            format!("{cols}x{rows} {cursor_x},{cursor_y}\n"),
        )
        .unwrap();
    }

    /// Resize the pane during the next `capture-pane`, once: the capture comes
    /// back drawn at the new grid, and only a measurement taken *after* it can
    /// know that.
    fn stub_resize_on_capture(&self, cols: u16, rows: u16, cursor_x: u16, cursor_y: u16) {
        std::fs::write(
            self.dir.path().join("resize-on-capture.txt"),
            format!("{cols}x{rows} {cursor_x},{cursor_y}\n"),
        )
        .unwrap();
    }

    /// Whether the stub tmux's `capture-pane` fails — a pane that is there
    /// (`display-message` still answers) but cannot be read.
    fn stub_capture_fails(&self, fails: bool) {
        self.stub_marker("capture-fails", fails);
    }

    /// Whether the stub tmux's `display-message` fails — a pane that is there
    /// (`has-session` still succeeds) but cannot be measured.
    fn stub_measure_fails(&self, fails: bool) {
        self.stub_marker("measure-fails", fails);
    }

    /// Whether the stub tmux can be run at all. Taking the binary away is how
    /// a daemon that cannot spawn a process sees the world: every question is
    /// unanswered, rather than answered "no".
    fn stub_tmux_reachable(&self, reachable: bool) {
        let bin = self.dir.path().join("tmux-stub.sh");
        let parked = self.dir.path().join("tmux-stub.parked");
        let (from, to) = if reachable {
            (&parked, &bin)
        } else {
            (&bin, &parked)
        };
        if from.exists() {
            std::fs::rename(from, to).unwrap();
        }
    }

    fn stub_marker(&self, name: &str, set: bool) {
        let marker = self.dir.path().join(name);
        if set {
            std::fs::write(marker, "").unwrap();
        } else {
            let _ = std::fs::remove_file(marker);
        }
    }
}

fn get(uri: &str) -> Request<Body> {
    Request::builder().uri(uri).body(Body::empty()).unwrap()
}

/// What came out of an SSE body next. The three are worth telling apart: a
/// stream that closes is a different thing from one that says nothing, and
/// both are behaviours these tests assert.
#[derive(Debug)]
enum Sse {
    Message(String),
    /// The daemon closed the connection.
    Closed,
    /// Nothing arrived in the time allowed.
    Silent,
}

/// Read from an SSE body until one complete message (`\n\n`-terminated) is in.
async fn next_sse(body: &mut Body, within: Duration) -> Sse {
    let read = tokio::time::timeout(within, async {
        let mut buf = String::new();
        while let Some(frame) = body.frame().await {
            let frame = frame.expect("sse body error");
            if let Some(chunk) = frame.data_ref() {
                buf.push_str(&String::from_utf8_lossy(chunk));
                if buf.contains("\n\n") {
                    return Some(buf);
                }
            }
        }
        None
    })
    .await;
    match read {
        Ok(Some(message)) => Sse::Message(message),
        Ok(None) => Sse::Closed,
        Err(_) => Sse::Silent,
    }
}

async fn next_sse_message(body: &mut Body) -> String {
    match next_sse(body, TIMEOUT).await {
        Sse::Message(message) => message,
        other => panic!("expected an sse message, got {other:?}"),
    }
}

/// The next SSE message, or `None` if none arrives within `within` — for
/// asserting that a stream is deliberately saying nothing.
async fn sse_message_within(body: &mut Body, within: Duration) -> Option<String> {
    match next_sse(body, within).await {
        Sse::Message(message) => Some(message),
        Sse::Silent => None,
        Sse::Closed => panic!("the stream closed instead of staying open"),
    }
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

/// tmux session names are per (task, role), so another session can hold the
/// name of one that is over. Asking for the finished session's logs must
/// yield *its* console log, never the pane the live one is now drawing.
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

    let (name, _) = parse(&next_sse_message(&mut body).await);
    assert_eq!(name, "resize");
    let (name, payload) = parse(&next_sse_message(&mut body).await);
    assert_eq!(name, "snapshot");
    assert_eq!(payload["chunk"], "pane snapshot\u{1b}[24;1H");
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

/// The snapshot is wrapped at the pane's width and everything after it is
/// addressed in the pane's grid, so the grid has to arrive first — a viewer
/// that renders those bytes at any other size draws every repaint on the
/// wrong row.
#[tokio::test]
async fn a_live_stream_opens_with_the_grid_the_pane_draws_against() {
    let h = harness_with_stub_tmux().await;
    let session = h.session("ariadne-sized").await;
    h.stub_pane("pane snapshot\n");
    h.stub_pane_geometry(100, 30, 4, 9);

    let response = h
        .router
        .clone()
        .oneshot(get(&format!("/v1/sessions/{}/logs/stream", session.id)))
        .await
        .unwrap();
    let mut body = response.into_body();

    let (name, payload) = parse(&next_sse_message(&mut body).await);
    assert_eq!(name, "resize", "the grid comes before anything drawn in it");
    assert_eq!(payload["cols"], 100);
    assert_eq!(payload["rows"], 30);

    let (name, payload) = parse(&next_sse_message(&mut body).await);
    assert_eq!(name, "snapshot");
    assert_eq!(
        payload["chunk"], "pane snapshot\u{1b}[10;5H",
        "the capture is a screen: its last row is the pane's last row, and it \
         leaves the cursor where the pane has it"
    );
}

/// The capture's trailing newline and the cursor are the difference between a
/// copy of the screen and the screen itself: without them the repaints that
/// follow are addressed a row too high, on top of output that is still there.
#[tokio::test]
async fn a_snapshot_ends_where_the_pane_left_its_cursor() {
    let h = harness_with_stub_tmux().await;
    let session = h.session("ariadne-cursor").await;
    // Three rows, the cursor at the start of the second: what a TUI holding
    // its prompt above a status line looks like.
    h.stub_pane("first\nsecond\nthird\n");
    h.stub_pane_geometry(80, 3, 0, 1);

    let response = h
        .router
        .clone()
        .oneshot(get(&format!("/v1/sessions/{}/logs/stream", session.id)))
        .await
        .unwrap();
    let mut body = response.into_body();

    let (name, _) = parse(&next_sse_message(&mut body).await);
    assert_eq!(name, "resize");
    let (name, payload) = parse(&next_sse_message(&mut body).await);
    assert_eq!(name, "snapshot");
    assert_eq!(payload["chunk"], "first\nsecond\nthird\u{1b}[2;1H");
}

/// tmux resizes a session's window to whatever client attaches to it, so the
/// grid can change under a stream that is already running. The redraw that
/// follows is only legible at the new one.
#[tokio::test]
async fn a_pane_resized_under_the_stream_reports_its_new_grid() {
    let h = harness_with_stub_tmux().await;
    let session = h.session("ariadne-resized").await;
    h.stub_pane("pane snapshot\n");

    let response = h
        .router
        .clone()
        .oneshot(get(&format!("/v1/sessions/{}/logs/stream", session.id)))
        .await
        .unwrap();
    let mut body = response.into_body();

    let (name, payload) = parse(&next_sse_message(&mut body).await);
    assert_eq!(name, "resize");
    assert_eq!(payload["cols"], 80);
    let (name, _) = parse(&next_sse_message(&mut body).await);
    assert_eq!(name, "snapshot");

    // Somebody attached with a wider terminal.
    h.stub_pane_screen("the redrawn pane\n");
    h.stub_pane_geometry(120, 40, 0, 39);

    let mut sizes = Vec::new();
    for _ in 0..10 {
        let (name, payload) = parse(&next_sse_message(&mut body).await);
        if name == "resize" {
            sizes.push((payload["cols"].as_u64(), payload["rows"].as_u64()));
            break;
        }
    }
    assert_eq!(
        sizes,
        vec![(Some(120), Some(40))],
        "the new grid is reported, and only when it changes"
    );

    let (name, payload) = parse(&next_sse_message(&mut body).await);
    assert_eq!(
        name, "snapshot",
        "a resize is followed by the screen it applies to, not by more deltas"
    );
    assert_eq!(payload["chunk"], "the redrawn pane\u{1b}[40;1H");
}

/// Output waiting to go out when a pane is resized belongs to neither grid:
/// part of it was drawn before the change and part after, and nothing in the
/// byte stream says where the boundary is. Sending it either side of the
/// `resize` renders some of it at the wrong width — the corruption this whole
/// change is about — so it is dropped for a fresh screen instead.
///
/// The pane writes continuously, so there is always output in flight when the
/// resize is noticed: had it been ordered against the new grid rather than
/// replaced, a `delta` would follow the `resize` instead of a `snapshot`, and
/// old-grid lines would keep arriving after it.
#[tokio::test]
async fn output_in_flight_when_the_pane_resizes_is_replaced_rather_than_reordered() {
    let h = harness_with_stub_tmux().await;
    let session = h.session("ariadne-straddle").await;
    h.stub_pane("80-column screen\n");
    h.write_console_log(&session.id, "");

    // Writes every 50ms — faster than the stream polls — switching what it
    // draws the moment the pane changes shape.
    let resized_pane = Arc::new(AtomicBool::new(false));
    let writer = {
        let log = h.console_log(&session.id);
        let resized_pane = resized_pane.clone();
        tokio::spawn(async move {
            loop {
                let line = if resized_pane.load(Ordering::SeqCst) {
                    "DRAWN-AT-120-COLUMNS\n"
                } else {
                    "DRAWN-AT-80-COLUMNS\n"
                };
                let mut file = tokio::fs::OpenOptions::new()
                    .append(true)
                    .open(&log)
                    .await
                    .unwrap();
                file.write_all(line.as_bytes()).await.unwrap();
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
    };

    let response = h
        .router
        .clone()
        .oneshot(get(&format!("/v1/sessions/{}/logs/stream", session.id)))
        .await
        .unwrap();
    let mut body = response.into_body();
    let (name, _) = parse(&next_sse_message(&mut body).await);
    assert_eq!(name, "resize");
    let (name, _) = parse(&next_sse_message(&mut body).await);
    assert_eq!(name, "snapshot");

    // Somebody attached with a wider terminal, mid-output.
    h.stub_pane_screen("120-column screen\n");
    h.stub_pane_geometry(120, 40, 0, 39);
    resized_pane.store(true, Ordering::SeqCst);

    let mut resized = false;
    for _ in 0..20 {
        let (name, _) = parse(&next_sse_message(&mut body).await);
        if name == "resize" {
            resized = true;
            break;
        }
    }
    assert!(resized, "the resize is reported");

    let (name, payload) = parse(&next_sse_message(&mut body).await);
    assert_eq!(
        name, "snapshot",
        "a resize is followed by a screen taken at the new grid, not by the \
         output that was waiting to go out at the old one"
    );
    assert_eq!(payload["chunk"], "120-column screen\u{1b}[40;1H");

    // The tail moved past the dropped bytes with it, so nothing drawn at the
    // old grid arrives with what the pane writes next either.
    for _ in 0..4 {
        let (name, payload) = parse(&next_sse_message(&mut body).await);
        let chunk = payload["chunk"].as_str().unwrap_or("");
        assert!(
            !chunk.contains("DRAWN-AT-80-COLUMNS"),
            "output drawn at the old grid must not be replayed at the new one: {name} {chunk:?}"
        );
    }

    writer.abort();
}

/// A capture can fail — a pane that is still there but cannot be read — and
/// the resize is known by then. Everything the pane writes from that moment is
/// drawn at a grid the client has not been given, so none of it may go out
/// until there is a screen to go with it. Nothing is committed either: the log
/// tail stays where it was, so the retry that succeeds still covers the bytes
/// the failed attempt would have skipped.
#[tokio::test]
async fn output_while_the_resized_pane_cannot_be_captured_waits_for_the_new_screen() {
    let h = harness_with_stub_tmux().await;
    let session = h.session("ariadne-capture-fails").await;
    h.stub_pane("80-column screen\n");
    h.write_console_log(&session.id, "");

    let response = h
        .router
        .clone()
        .oneshot(get(&format!("/v1/sessions/{}/logs/stream", session.id)))
        .await
        .unwrap();
    let mut body = response.into_body();
    let (name, _) = parse(&next_sse_message(&mut body).await);
    assert_eq!(name, "resize");
    let (name, _) = parse(&next_sse_message(&mut body).await);
    assert_eq!(name, "snapshot");

    // The pane is resized and stops being capturable at the same time. No
    // output yet, so nothing can be in flight and the stream has nothing to
    // say until it has a screen.
    h.stub_capture_fails(true);
    h.stub_pane_geometry(120, 40, 0, 39);
    tokio::time::sleep(Duration::from_millis(1_200)).await;

    // Now the pane draws — at 120 columns, which the client knows nothing of.
    let mut log = tokio::fs::OpenOptions::new()
        .append(true)
        .open(h.console_log(&session.id))
        .await
        .unwrap();
    log.write_all(b"DRAWN-AT-120-COLUMNS\n").await.unwrap();
    log.flush().await.unwrap();

    let held = sse_message_within(&mut body, Duration::from_millis(900)).await;
    assert!(
        held.is_none(),
        "output drawn at the new grid must not be sent at the old one: {held:?}"
    );

    // The pane can be read again: a grid, then the screen that goes with it.
    h.stub_pane_screen("120-column screen\n");
    h.stub_capture_fails(false);

    let (name, payload) = parse(&next_sse_message(&mut body).await);
    assert_eq!(name, "resize", "the recovery reports the grid first");
    assert_eq!(payload["cols"], 120);
    let (name, payload) = parse(&next_sse_message(&mut body).await);
    assert_eq!(
        name, "snapshot",
        "and the screen taken at it immediately after"
    );
    assert_eq!(payload["chunk"], "120-column screen\u{1b}[40;1H");

    // What the pane wrote while it could not be captured is part of that
    // screen now, not something still owed to the client.
    log.write_all(b"AFTER-THE-RECOVERY\n").await.unwrap();
    log.flush().await.unwrap();
    let (name, payload) = parse(&next_sse_message(&mut body).await);
    assert_eq!(name, "delta");
    let chunk = payload["chunk"].as_str().unwrap();
    assert!(chunk.contains("AFTER-THE-RECOVERY"), "chunk: {chunk:?}");
    assert!(
        !chunk.contains("DRAWN-AT-120-COLUMNS"),
        "the replacement screen covers what came before it: {chunk:?}"
    );
}

/// A pane that stops answering has not promised to stop changing. Output read
/// after a measurement failed may have been drawn at a grid the client has
/// never been given — the failure and a resize can be the same moment — so it
/// is withheld exactly as a detected resize's is, rather than sent at the last
/// grid anyone happened to confirm.
#[tokio::test]
async fn output_is_withheld_once_the_pane_stops_answering() {
    let h = harness_with_stub_tmux().await;
    let session = h.session("ariadne-unanswering").await;
    h.stub_pane("80-column screen\n");
    h.write_console_log(&session.id, "");

    let mut body = h
        .router
        .clone()
        .oneshot(get(&format!("/v1/sessions/{}/logs/stream", session.id)))
        .await
        .unwrap()
        .into_body();
    let (name, payload) = parse(&next_sse_message(&mut body).await);
    assert_eq!(name, "resize");
    assert_eq!(payload["cols"], 80);
    let (name, _) = parse(&next_sse_message(&mut body).await);
    assert_eq!(name, "snapshot");

    // The pane is resized and stops answering in the same breath, so the
    // daemon has no way to learn about the resize: the failure is all it sees.
    h.stub_pane_screen("120-column screen\n");
    h.stub_pane_geometry(120, 40, 0, 39);
    h.stub_measure_fails(true);
    // Long enough for the follower to have taken that in.
    tokio::time::sleep(Duration::from_millis(1_400)).await;

    let mut log = tokio::fs::OpenOptions::new()
        .append(true)
        .open(h.console_log(&session.id))
        .await
        .unwrap();
    log.write_all(b"DRAWN-AT-AN-UNKNOWN-GRID\n").await.unwrap();
    log.flush().await.unwrap();

    let held = sse_message_within(&mut body, Duration::from_millis(700)).await;
    assert!(
        held.is_none(),
        "output whose grid cannot be confirmed must not be sent at the old one: {held:?}"
    );

    // The pane answers again, and the answer is the grid it had quietly moved
    // to; the screen replaces what was withheld.
    h.stub_measure_fails(false);
    let (name, payload) = parse(&next_sse_message(&mut body).await);
    assert_eq!(name, "resize");
    assert_eq!(
        (payload["cols"].as_u64(), payload["rows"].as_u64()),
        (Some(120), Some(40))
    );
    let (name, payload) = parse(&next_sse_message(&mut body).await);
    assert_eq!(name, "snapshot");
    assert_eq!(payload["chunk"], "120-column screen\u{1b}[40;1H");
}

/// A daemon that cannot run `tmux` at all has learned nothing about the pane.
/// Neither the failed measurement nor the failed confirmation is an answer, so
/// the session keeps its status and the stream keeps its silence — the one
/// thing that must not happen is `end`, which retires the viewer for good.
#[tokio::test]
async fn tmux_being_unreachable_does_not_end_a_session() {
    let h = harness_with_stub_tmux().await;
    let session = h.session("ariadne-no-tmux").await;
    h.stub_pane("a screen behind a broken tmux\n");
    h.write_console_log(&session.id, "console output\n");
    h.stub_tmux_reachable(false);

    let mut body = h
        .router
        .clone()
        .oneshot(get(&format!("/v1/sessions/{}/logs/stream", session.id)))
        .await
        .unwrap()
        .into_body();
    for _ in 0..3 {
        match next_sse(&mut body, Duration::from_millis(500)).await {
            Sse::Silent => {}
            Sse::Closed => break,
            Sse::Message(message) => {
                panic!("nothing was learned about the pane, yet it sent {message:?}")
            }
        }
    }
    assert!(
        h.store
            .get_session(&session.id)
            .await
            .unwrap()
            .status()
            .is_live(),
        "a session is not over because tmux could not be run"
    );

    // tmux comes back: the same connection produces the screen it owed.
    h.stub_tmux_reachable(true);
    let (name, payload) = parse(&next_sse_message(&mut body).await);
    assert_eq!(name, "resize");
    assert_eq!(payload["cols"], 80);
    let (name, payload) = parse(&next_sse_message(&mut body).await);
    assert_eq!(name, "snapshot");
    assert_eq!(
        payload["chunk"],
        "a screen behind a broken tmux\u{1b}[24;1H"
    );
}

/// The first screen a client is given is as much a screen-and-its-grid as any
/// later one: a pane that resizes during that very first capture must not be
/// announced at the size it was measured at beforehand. Opening goes through
/// the same coherent capture as a resize does, so this is that guarantee at
/// the one moment the client has nothing to fall back on.
#[tokio::test]
async fn a_pane_that_resizes_during_the_opening_capture_is_reported_at_the_grid_it_reached() {
    let h = harness_with_stub_tmux().await;
    let session = h.session("ariadne-resized-on-open").await;
    h.stub_pane("132-column screen\n");
    // Armed before the first request: the pane reads 80×24, and reading it
    // turns it into a 132×43 one.
    h.stub_resize_on_capture(132, 43, 0, 42);

    let mut body = h
        .router
        .clone()
        .oneshot(get(&format!("/v1/sessions/{}/logs/stream", session.id)))
        .await
        .unwrap()
        .into_body();

    let (name, payload) = parse(&next_sse_message(&mut body).await);
    assert_eq!(name, "resize");
    assert_eq!(
        (payload["cols"].as_u64(), payload["rows"].as_u64()),
        (Some(132), Some(43)),
        "the opening grid is the captured screen's, not the one measured before it"
    );
    let (name, payload) = parse(&next_sse_message(&mut body).await);
    assert_eq!(name, "snapshot");
    assert_eq!(payload["chunk"], "132-column screen\u{1b}[43;1H");
}

/// A pane that cannot be *measured* is not a pane that is gone either, and the
/// difference is the same one `end` turns into a dead viewer. tmux is asked
/// outright — `has-session` — before anything is concluded, at the opening
/// and while resynchronising alike.
#[tokio::test]
async fn a_pane_that_cannot_be_measured_is_not_reported_as_a_finished_session() {
    let h = harness_with_stub_tmux().await;
    let session = h.session("ariadne-unmeasurable").await;
    h.stub_pane("a screen nobody can measure\n");
    h.write_console_log(&session.id, "console output\n");
    h.stub_measure_fails(true);

    // Opening: the session is live and tmux still has it, so there is nothing
    // to declare — least of all that it is over.
    let mut body = h
        .router
        .clone()
        .oneshot(get(&format!("/v1/sessions/{}/logs/stream", session.id)))
        .await
        .unwrap()
        .into_body();
    match next_sse(&mut body, Duration::from_millis(700)).await {
        Sse::Silent => {}
        other => panic!("expected silence while the pane cannot be measured, got {other:?}"),
    }

    h.stub_measure_fails(false);
    let (name, payload) = parse(&next_sse_message(&mut body).await);
    assert_eq!(name, "resize");
    assert_eq!(payload["cols"], 80);
    let (name, payload) = parse(&next_sse_message(&mut body).await);
    assert_eq!(name, "snapshot");
    assert_eq!(payload["chunk"], "a screen nobody can measure\u{1b}[24;1H");

    // And again mid-stream: a resize sends the follower looking for a screen,
    // and the measurements it needs start failing while it looks.
    h.stub_pane_screen("120-column screen\n");
    h.stub_pane_geometry(120, 40, 0, 39);
    tokio::time::sleep(Duration::from_millis(1_100)).await;
    h.stub_measure_fails(true);

    for _ in 0..3 {
        match next_sse(&mut body, Duration::from_millis(400)).await {
            Sse::Silent => {}
            Sse::Message(message) => {
                let (name, _) = parse(&message);
                assert_ne!(name, "end", "a live session must not be declared over");
            }
            Sse::Closed => break,
        }
    }

    h.stub_measure_fails(false);
    // Whether the connection above survived the wait or gave way to a fresh
    // one, what the client ends up with is the new grid and its screen.
    let mut body = h
        .router
        .clone()
        .oneshot(get(&format!("/v1/sessions/{}/logs/stream", session.id)))
        .await
        .unwrap()
        .into_body();
    let (name, payload) = parse(&next_sse_message(&mut body).await);
    assert_eq!(name, "resize");
    assert_eq!(
        (payload["cols"].as_u64(), payload["rows"].as_u64()),
        (Some(120), Some(40))
    );
    let (name, payload) = parse(&next_sse_message(&mut body).await);
    assert_eq!(name, "snapshot");
    assert_eq!(payload["chunk"], "120-column screen\u{1b}[40;1H");
}

/// A pane that cannot be read is not a session that has ended, and the two
/// must not be confused: `end` tells the client to stop asking, so a stream
/// that says it about a live session leaves a viewer stuck on a dead terminal
/// while the agent works on. It closes instead — silently, and as often as it
/// takes — so that every reconnect is another chance at a coherent screen.
#[tokio::test]
async fn a_pane_that_cannot_be_captured_is_not_reported_as_a_finished_session() {
    let h = harness_with_stub_tmux().await;
    let session = h.session("ariadne-unreadable").await;
    h.stub_pane("a screen nobody can read\n");
    h.write_console_log(&session.id, "console output\n");
    h.stub_capture_fails(true);

    // First connection: nothing to say, and it says nothing — no `end`, and
    // no console log served as though the session were over.
    let mut body = h
        .router
        .clone()
        .oneshot(get(&format!("/v1/sessions/{}/logs/stream", session.id)))
        .await
        .unwrap()
        .into_body();
    let mut closed = false;
    for _ in 0..10 {
        match next_sse(&mut body, Duration::from_millis(1_500)).await {
            Sse::Closed => {
                closed = true;
                break;
            }
            Sse::Silent => {}
            Sse::Message(message) => panic!("nothing coherent to send, yet it sent {message:?}"),
        }
    }
    assert!(
        closed,
        "a stream with no screen to serve gives way to a fresh connection"
    );

    // Reconnecting while the pane still cannot be read: same again, and still
    // no `end` — the client is free to keep trying.
    let mut body = h
        .router
        .clone()
        .oneshot(get(&format!("/v1/sessions/{}/logs/stream", session.id)))
        .await
        .unwrap()
        .into_body();
    match next_sse(&mut body, Duration::from_millis(700)).await {
        Sse::Silent => {}
        other => panic!("expected silence while the pane cannot be read, got {other:?}"),
    }

    // It can be read again: the connection that is already open recovers on
    // its own, screen and grid together.
    h.stub_capture_fails(false);
    let (name, payload) = parse(&next_sse_message(&mut body).await);
    assert_eq!(name, "resize");
    assert_eq!(payload["cols"], 80);
    let (name, payload) = parse(&next_sse_message(&mut body).await);
    assert_eq!(name, "snapshot");
    assert_eq!(payload["chunk"], "a screen nobody can read\u{1b}[24;1H");
}

/// A capture and the grid it is reported at have to describe the same screen.
/// The stub resizes the pane *while* it is being captured, which is the window
/// between measuring and reading that a stream cannot otherwise see: the
/// capture that comes back is the new screen, and reporting it at the grid
/// measured beforehand would be the original corruption with extra steps.
#[tokio::test]
async fn a_pane_that_resizes_while_it_is_read_is_not_reported_at_the_grid_it_left() {
    let h = harness_with_stub_tmux().await;
    let session = h.session("ariadne-resized-mid-capture").await;
    h.stub_pane("80-column screen\n");

    let mut body = h
        .router
        .clone()
        .oneshot(get(&format!("/v1/sessions/{}/logs/stream", session.id)))
        .await
        .unwrap()
        .into_body();
    let (name, payload) = parse(&next_sse_message(&mut body).await);
    assert_eq!(name, "resize");
    assert_eq!(payload["cols"], 80);
    let (name, _) = parse(&next_sse_message(&mut body).await);
    assert_eq!(name, "snapshot");

    // The pane is 120 columns now, and the next capture of it turns it into a
    // 132-column one — a second resize landing inside the read.
    h.stub_pane_screen("132-column screen\n");
    h.stub_pane_geometry(120, 40, 0, 39);
    h.stub_resize_on_capture(132, 43, 0, 42);

    let mut sizes = Vec::new();
    let mut snapshot = None;
    for _ in 0..12 {
        let (name, payload) = parse(&next_sse_message(&mut body).await);
        match name.as_str() {
            "resize" => sizes.push((payload["cols"].as_u64(), payload["rows"].as_u64())),
            "snapshot" => {
                snapshot = payload["chunk"].as_str().map(str::to_owned);
                break;
            }
            _ => {}
        }
    }
    assert_eq!(
        sizes,
        vec![(Some(132), Some(43))],
        "only the grid the captured screen was actually drawn at is reported"
    );
    assert_eq!(
        snapshot.as_deref(),
        Some("132-column screen\u{1b}[43;1H"),
        "and the screen that goes with it, with that grid's cursor"
    );
}

/// A session that has ended has no pane left to measure, and its console log
/// is raw terminal bytes that only wrap correctly at the width they were
/// written at. The last size it was seen at is what it is served at — a
/// history view is where this matters most, since that is all such a session
/// will ever be.
#[tokio::test]
async fn a_finished_session_is_served_at_the_grid_it_was_last_seen_at() {
    let h = harness().await;
    let session = h.dead_session().await;
    h.write_console_log(&session.id, "output from a 120-column pane\n");
    h.launcher.record_pane_size(&session.id, 120, 40).await;

    let response = h
        .router
        .clone()
        .oneshot(get(&format!("/v1/sessions/{}/logs/stream", session.id)))
        .await
        .unwrap();
    let mut body = response.into_body();

    let (name, payload) = parse(&next_sse_message(&mut body).await);
    assert_eq!(name, "resize", "a finished log has a width too");
    assert_eq!(payload["cols"], 120);
    assert_eq!(payload["rows"], 40);

    let (name, payload) = parse(&next_sse_message(&mut body).await);
    assert_eq!(name, "snapshot");
    assert_eq!(
        payload["chunk"], "output from a 120-column pane\n",
        "the console log is replayed as written, cursor sequence and all"
    );
}

/// Nothing was ever recorded — the session ended before anyone watched it —
/// so there is no grid to report and the client falls back to its own default.
#[tokio::test]
async fn a_finished_session_never_measured_reports_no_grid() {
    let h = harness().await;
    let session = h.dead_session().await;
    h.write_console_log(&session.id, "unmeasured output\n");

    let response = h
        .router
        .clone()
        .oneshot(get(&format!("/v1/sessions/{}/logs/stream", session.id)))
        .await
        .unwrap();
    let mut body = response.into_body();

    let (name, _) = parse(&next_sse_message(&mut body).await);
    assert_eq!(
        name, "snapshot",
        "no size was ever known, so none is claimed"
    );
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

    let (name, payload) = parse(&next_sse_message(&mut body).await);
    assert_eq!(name, "resize", "the pane's grid comes first");
    assert!(
        payload["cols"].as_u64().is_some_and(|c| c > 0),
        "a real pane reports a real grid: {payload}"
    );

    let (name, _) = parse(&next_sse_message(&mut body).await);
    assert_eq!(name, "snapshot", "then the pane snapshot");

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
