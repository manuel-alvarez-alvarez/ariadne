//! Integration tests for `GET /v1/sessions/{id}/logs/stream`.
//!
//! No tmux needed: every `tmux` here is a stub script. A session pointing at a
//! name no stub admits to is exactly the "session already over" path — the one
//! whose framing and lifecycle the acceptance criteria pin down. Following a
//! live pane is the tailing logic, unit-tested in `log::console`; the one test that
//! drives a real pane is `#[ignore]`d and asks for real tmux.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::{StatusCode, header};
use tokio::io::AsyncWriteExt;

use ariadne_core::SessionStatus;
use ariadne_daemon::log::MAX_CHUNK;
use ariadne_daemon::tmux::TmuxSpawn;
use ariadne_store::AgentSession;

use common::{
    Harness, Sse, TIMEOUT, Tmux, expect_sse, get, harness, next_sse, next_sse_message, parse_sse,
    sse_is_closed, sse_message_within,
};

/// A harness whose `tmux` denies every session it is asked about.
///
/// That is what a real `tmux` answers for a session that has ended — and,
/// unlike a real `tmux`, it answers the same on a machine with none installed,
/// where "cannot ask" is a third thing the stream treats as "still there".
async fn gone_tmux() -> Harness {
    harness().tmux(Tmux::Gone).await
}

/// A session whose tmux is not (and never was) running.
async fn dead_session(h: &Harness) -> AgentSession {
    h.lone_session("ariadne-test-no-such-session").await
}

/// Read deltas until `wanted` bytes of them have arrived, checking the frame
/// cap on every one: what came out, and how many frames it took.
async fn drained(body: &mut Body, wanted: usize) -> (String, usize) {
    let mut received = String::new();
    let mut frames = 0;
    while received.len() < wanted {
        let payload = expect_sse(body, "delta").await;
        let chunk = payload["chunk"].as_str().unwrap();
        assert!(
            chunk.len() <= MAX_CHUNK,
            "no frame is bigger than the cap: {}",
            chunk.len()
        );
        received.push_str(chunk);
        frames += 1;
    }
    (received, frames)
}

/// The log stream of a session, open and ready to be read message by message.
async fn stream(h: &Harness, session: &AgentSession) -> Body {
    h.stream(get(&format!("/v1/sessions/{}/logs/stream", session.id)))
        .await
}

#[tokio::test]
async fn an_exited_session_yields_its_full_log_then_ends() {
    let h = gone_tmux().await;
    let session = dead_session(&h).await;
    // Raw terminal output: escape sequences, newlines, carriage returns and
    // A multi-byte glyph — none of which SSE framing tolerates unencoded.
    let console = "\u{1b}[2J\u{1b}[Hbuilding…\r\n│ done │\n\u{7}";
    h.write_console_log(&session.id, console);

    let response = h
        .response(get(&format!("/v1/sessions/{}/logs/stream", session.id)))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some("text/event-stream")
    );
    let mut body = response.into_body();

    let payload = expect_sse(&mut body, "snapshot").await;
    assert_eq!(
        payload["chunk"], console,
        "the snapshot round-trips the console log byte for byte"
    );

    let payload = expect_sse(&mut body, "end").await;
    assert_eq!(payload["session_id"], session.id);

    sse_is_closed(&mut body).await;

    // Nothing was ever piped for this one: the client still gets a
    // Well-formed (empty) snapshot and a clean end rather than a hanging.
    // Connection.
    let unpiped = h.lone_session("ariadne-test-never-piped").await;
    let mut body = stream(&h, &unpiped).await;
    let payload = expect_sse(&mut body, "snapshot").await;
    assert_eq!(payload["chunk"], "");
    expect_sse(&mut body, "end").await;
}

/// tmux session names are per (task, role), so another session can hold the
/// name of one that is over. Asking for the finished session's logs must
/// yield *its* console log, never the pane the live one is now drawing.
#[tokio::test]
async fn an_exited_session_ignores_the_pane_that_took_over_its_name() {
    let h = harness().await;
    let session = h.lone_session("ariadne-reused-name").await;
    h.set_status(&session, SessionStatus::Exited).await;
    h.write_console_log(&session.id, "the old session's output\n");
    // Its successor is live under the very same tmux name.
    h.stub_pane("the successor's pane\n");

    let mut body = stream(&h, &session).await;

    let payload = expect_sse(&mut body, "snapshot").await;
    assert_eq!(
        payload["chunk"], "the old session's output\n",
        "an exited session serves its own console log, not the live pane"
    );

    // A session that is already over ends at once.
    expect_sse(&mut body, "end").await;
    sse_is_closed(&mut body).await;
}

/// The stream must not be kept alive by its own traffic: a pane writing on
/// every poll still has to notice that the session behind it is finished.
#[tokio::test]
async fn a_terminal_status_ends_the_stream_even_while_output_keeps_coming() {
    let h = harness().await;
    let session = h.lone_session("ariadne-chatty").await;
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

    let mut body = stream(&h, &session).await;

    expect_sse(&mut body, "resize").await;
    let payload = expect_sse(&mut body, "snapshot").await;
    assert_eq!(payload["chunk"], "pane snapshot\u{1b}[24;1H");
    // The pane is producing output.
    expect_sse(&mut body, "delta").await;

    // The process is gone as far as the daemon is concerned — the pane in
    // Tmux (the stub still has one) is no longer this session's.
    h.set_status(&session, SessionStatus::Failed).await;

    // Output is shipped as it is written now, so a pane writing every 50ms
    // Gets a delta every 50ms: the end is a liveness tick away, not a poll.
    let mut last = String::new();
    for _ in 0..60 {
        let (name, _) = parse_sse(&next_sse_message(&mut body).await);
        last = name;
        if last == "end" {
            break;
        }
    }
    assert_eq!(last, "end", "a terminal status ends the stream");
    sse_is_closed(&mut body).await;
}

/// The snapshot is wrapped at the pane's width and everything after it is
/// addressed in the pane's grid, so the grid has to arrive first — a viewer
/// that renders those bytes at any other size draws every repaint on the
/// wrong row, and one that renders them at the wrong row does the same.
#[tokio::test]
async fn a_live_stream_opens_with_the_grid_the_pane_draws_against() {
    let h = harness().await;
    let session = h.lone_session("ariadne-sized").await;
    h.stub_pane("pane snapshot\n");
    h.pane_geometry(100, 30, 4, 9);

    let mut body = stream(&h, &session).await;

    // The grid comes before anything drawn in it.
    let payload = expect_sse(&mut body, "resize").await;
    assert_eq!(payload["cols"], 100);
    assert_eq!(payload["rows"], 30);

    let payload = expect_sse(&mut body, "snapshot").await;
    assert_eq!(
        payload["chunk"], "pane snapshot\u{1b}[10;5H",
        "the capture is a screen: its last row is the pane's last row, and it \
         leaves the cursor where the pane has it"
    );

    // The trailing newline and the cursor are the difference between a copy of
    // the screen and the screen itself: without them the repaints that follow
    // are addressed a row too high, on top of output that is still there.
    // Three rows, the cursor at the start of the second: what a TUI holding
    // its prompt above a status line looks like.
    let cursor = h.lone_session("ariadne-cursor").await;
    h.stub_pane("first\nsecond\nthird\n");
    h.pane_geometry(80, 3, 0, 1);
    let mut body = stream(&h, &cursor).await;
    expect_sse(&mut body, "resize").await;
    let payload = expect_sse(&mut body, "snapshot").await;
    assert_eq!(payload["chunk"], "first\nsecond\nthird\u{1b}[2;1H");
}

/// tmux resizes a session's window to whatever client attaches to it, so the
/// grid can change under a stream that is already running. The redraw that
/// follows is only legible at the new one.
#[tokio::test]
async fn a_pane_resized_under_the_stream_reports_its_new_grid() {
    let h = harness().await;
    let session = h.lone_session("ariadne-resized").await;
    h.stub_pane("pane snapshot\n");

    let mut body = stream(&h, &session).await;

    let payload = expect_sse(&mut body, "resize").await;
    assert_eq!(payload["cols"], 80);
    expect_sse(&mut body, "snapshot").await;

    // Somebody attached with a wider terminal.
    h.pane_draws("the redrawn pane\n");
    h.pane_geometry(120, 40, 0, 39);

    let mut sizes = Vec::new();
    for _ in 0..10 {
        let (name, payload) = parse_sse(&next_sse_message(&mut body).await);
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

    // A resize is followed by the screen it applies to, not by more deltas.
    let payload = expect_sse(&mut body, "snapshot").await;
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
    let h = harness().await;
    let session = h.lone_session("ariadne-straddle").await;
    h.stub_pane("80-column screen\n");
    h.write_console_log(&session.id, "");

    // Writes every 50ms — faster than the stream polls — switching what it
    // Draws the moment the pane changes shape.
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

    let mut body = stream(&h, &session).await;
    expect_sse(&mut body, "resize").await;
    expect_sse(&mut body, "snapshot").await;

    // Somebody attached with a wider terminal, mid-output.
    h.pane_draws("120-column screen\n");
    h.pane_geometry(120, 40, 0, 39);
    resized_pane.store(true, Ordering::SeqCst);

    let mut resized = false;
    for _ in 0..60 {
        let (name, _) = parse_sse(&next_sse_message(&mut body).await);
        if name == "resize" {
            resized = true;
            break;
        }
    }
    assert!(resized, "the resize is reported");

    // A resize is followed by a screen taken at the new grid, not by the
    // Output that was waiting to go out at the old one.
    let payload = expect_sse(&mut body, "snapshot").await;
    assert_eq!(payload["chunk"], "120-column screen\u{1b}[40;1H");

    // The tail moved past the dropped bytes with it, so nothing drawn at the
    // Old grid arrives with what the pane writes next either.
    for _ in 0..4 {
        let (name, payload) = parse_sse(&next_sse_message(&mut body).await);
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
    let h = harness().await;
    let session = h.lone_session("ariadne-capture-fails").await;
    h.stub_pane("80-column screen\n");
    h.write_console_log(&session.id, "");

    let mut body = stream(&h, &session).await;
    expect_sse(&mut body, "resize").await;
    expect_sse(&mut body, "snapshot").await;

    // The pane is resized and stops being capturable at the same time. No
    // Output yet, so nothing can be in flight and the stream has nothing to.
    // Say until it has a screen.
    h.capture_fails(true);
    h.pane_geometry(120, 40, 0, 39);
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
    h.pane_draws("120-column screen\n");
    h.capture_fails(false);

    // The recovery reports the grid first.
    let payload = expect_sse(&mut body, "resize").await;
    assert_eq!(payload["cols"], 120);
    // And the screen taken at it immediately after.
    let payload = expect_sse(&mut body, "snapshot").await;
    assert_eq!(payload["chunk"], "120-column screen\u{1b}[40;1H");

    // What the pane wrote while it could not be captured is part of that
    // Screen now, not something still owed to the client.
    log.write_all(b"AFTER-THE-RECOVERY\n").await.unwrap();
    log.flush().await.unwrap();
    let payload = expect_sse(&mut body, "delta").await;
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
    let h = harness().await;
    let session = h.lone_session("ariadne-unanswering").await;
    h.stub_pane("80-column screen\n");
    h.write_console_log(&session.id, "");

    let mut body = stream(&h, &session).await;
    let payload = expect_sse(&mut body, "resize").await;
    assert_eq!(payload["cols"], 80);
    expect_sse(&mut body, "snapshot").await;

    // The pane is resized and stops answering in the same breath, so the
    // Daemon has no way to learn about the resize: the failure is all it sees.
    h.pane_draws("120-column screen\n");
    h.pane_geometry(120, 40, 0, 39);
    h.measure_fails(true);
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
    // To; the screen replaces what was withheld.
    h.measure_fails(false);
    let payload = expect_sse(&mut body, "resize").await;
    assert_eq!(
        (payload["cols"].as_u64(), payload["rows"].as_u64()),
        (Some(120), Some(40))
    );
    let payload = expect_sse(&mut body, "snapshot").await;
    assert_eq!(payload["chunk"], "120-column screen\u{1b}[40;1H");
}

/// A daemon that cannot run `tmux` at all has learned nothing about the pane.
/// Neither the failed measurement nor the failed confirmation is an answer, so
/// the session keeps its status and the stream keeps its silence — the one
/// thing that must not happen is `end`, which retires the viewer for good.
#[tokio::test]
async fn tmux_being_unreachable_does_not_end_a_session() {
    let h = harness().await;
    let session = h.lone_session("ariadne-no-tmux").await;
    h.stub_pane("a screen behind a broken tmux\n");
    h.write_console_log(&session.id, "console output\n");
    h.tmux_vanishes();

    let mut body = stream(&h, &session).await;
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
        h.session_status(&session).await
            .is_live(),
        "a session is not over because tmux could not be run"
    );

    // Tmux comes back: the same connection produces the screen it owed.
    h.tmux_returns();
    let payload = expect_sse(&mut body, "resize").await;
    assert_eq!(payload["cols"], 80);
    let payload = expect_sse(&mut body, "snapshot").await;
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
    let h = harness().await;
    let session = h.lone_session("ariadne-resized-on-open").await;
    h.stub_pane("132-column screen\n");

    // A connection that cannot get a screen and the grid it was drawn at to
    // agree closes without an `end` and leaves the client to open another —
    // which is what a client does, so this does too, arming the pane again
    // each time so that every attempt faces the same resize mid-capture: the
    // pane reads 80×24, and reading it turns it into a 132×43 one.
    let (payload, mut body) = loop {
        h.pane_geometry(80, 24, 0, 23);
        h.resize_on_capture(132, 43, 0, 42);
        let mut body = stream(&h, &session).await;
        match next_sse(&mut body, TIMEOUT).await {
            Sse::Message(message) => {
                let (name, payload) = parse_sse(&message);
                assert_eq!(name, "resize", "expected the opening grid, got {name}");
                break (payload, body);
            }
            Sse::Closed => continue,
            Sse::Silent => panic!("the stream neither opened nor gave way"),
        }
    };
    assert_eq!(
        (payload["cols"].as_u64(), payload["rows"].as_u64()),
        (Some(132), Some(43)),
        "the opening grid is the captured screen's, not the one measured before it"
    );
    let payload = expect_sse(&mut body, "snapshot").await;
    assert_eq!(payload["chunk"], "132-column screen\u{1b}[43;1H");
}

/// A pane that cannot be *measured* is not a pane that is gone either, and the
/// difference is the same one `end` turns into a dead viewer. tmux is asked
/// outright — `has-session` — before anything is concluded, at the opening
/// and while resynchronising alike.
#[tokio::test]
async fn a_pane_that_cannot_be_measured_is_not_reported_as_a_finished_session() {
    let h = harness().await;
    let session = h.lone_session("ariadne-unmeasurable").await;
    h.stub_pane("a screen nobody can measure\n");
    h.write_console_log(&session.id, "console output\n");
    h.measure_fails(true);

    // Opening: the session is live and tmux still has it, so there is nothing
    // To declare — least of all that it is over.
    let mut body = stream(&h, &session).await;
    match next_sse(&mut body, Duration::from_millis(700)).await {
        Sse::Silent => {}
        other => panic!("expected silence while the pane cannot be measured, got {other:?}"),
    }

    h.measure_fails(false);
    let payload = expect_sse(&mut body, "resize").await;
    assert_eq!(payload["cols"], 80);
    let payload = expect_sse(&mut body, "snapshot").await;
    assert_eq!(payload["chunk"], "a screen nobody can measure\u{1b}[24;1H");

    // And again mid-stream: a resize sends the follower looking for a screen,
    // And the measurements it needs start failing while it looks.
    h.pane_draws("120-column screen\n");
    h.pane_geometry(120, 40, 0, 39);
    tokio::time::sleep(Duration::from_millis(1_100)).await;
    h.measure_fails(true);

    for _ in 0..3 {
        match next_sse(&mut body, Duration::from_millis(400)).await {
            Sse::Silent => {}
            Sse::Message(message) => {
                let (name, _) = parse_sse(&message);
                assert_ne!(name, "end", "a live session must not be declared over");
            }
            Sse::Closed => break,
        }
    }

    h.measure_fails(false);
    // Whether the connection above survived the wait or gave way to a fresh
    // One, what the client ends up with is the new grid and its screen.
    let mut body = stream(&h, &session).await;
    let payload = expect_sse(&mut body, "resize").await;
    assert_eq!(
        (payload["cols"].as_u64(), payload["rows"].as_u64()),
        (Some(120), Some(40))
    );
    let payload = expect_sse(&mut body, "snapshot").await;
    assert_eq!(payload["chunk"], "120-column screen\u{1b}[40;1H");
}

/// A pane that cannot be read is not a session that has ended, and the two
/// must not be confused: `end` tells the client to stop asking, so a stream
/// that says it about a live session leaves a viewer stuck on a dead terminal
/// while the agent works on. It closes instead — silently, and as often as it
/// takes — so that every reconnect is another chance at a coherent screen.
#[tokio::test]
async fn a_pane_that_cannot_be_captured_is_not_reported_as_a_finished_session() {
    let h = harness().await;
    let session = h.lone_session("ariadne-unreadable").await;
    h.stub_pane("a screen nobody can read\n");
    h.write_console_log(&session.id, "console output\n");
    h.capture_fails(true);

    // First connection: nothing to say, and it says nothing — no `end`, and
    // No console log served as though the session were over.
    let mut body = stream(&h, &session).await;
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
    // No `end` — the client is free to keep trying.
    let mut body = stream(&h, &session).await;
    match next_sse(&mut body, Duration::from_millis(700)).await {
        Sse::Silent => {}
        other => panic!("expected silence while the pane cannot be read, got {other:?}"),
    }

    // It can be read again: the connection that is already open recovers on
    // Its own, screen and grid together.
    h.capture_fails(false);
    let payload = expect_sse(&mut body, "resize").await;
    assert_eq!(payload["cols"], 80);
    let payload = expect_sse(&mut body, "snapshot").await;
    assert_eq!(payload["chunk"], "a screen nobody can read\u{1b}[24;1H");
}

/// A capture and the grid it is reported at have to describe the same screen.
/// The stub resizes the pane *while* it is being captured, which is the window
/// between measuring and reading that a stream cannot otherwise see: the
/// capture that comes back is the new screen, and reporting it at the grid
/// measured beforehand would be the original corruption with extra steps.
#[tokio::test]
async fn a_pane_that_resizes_while_it_is_read_is_not_reported_at_the_grid_it_left() {
    let h = harness().await;
    let session = h.lone_session("ariadne-resized-mid-capture").await;
    h.stub_pane("80-column screen\n");

    let mut body = stream(&h, &session).await;
    let payload = expect_sse(&mut body, "resize").await;
    assert_eq!(payload["cols"], 80);
    expect_sse(&mut body, "snapshot").await;

    // The pane is 120 columns now, and the next capture of it turns it into a
    // 132-column one — a second resize landing inside the read.
    h.pane_draws("132-column screen\n");
    h.pane_geometry(120, 40, 0, 39);
    h.resize_on_capture(132, 43, 0, 42);

    let mut sizes = Vec::new();
    let mut snapshot = None;
    for _ in 0..12 {
        let (name, payload) = parse_sse(&next_sse_message(&mut body).await);
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
    let h = gone_tmux().await;
    let session = dead_session(&h).await;
    h.write_console_log(&session.id, "output from a 120-column pane\n");
    h.launcher.record_pane_size(&session.id, 120, 40).await;

    let mut body = stream(&h, &session).await;

    // A finished log has a width too.
    let payload = expect_sse(&mut body, "resize").await;
    assert_eq!(payload["cols"], 120);
    assert_eq!(payload["rows"], 40);

    let payload = expect_sse(&mut body, "snapshot").await;
    assert_eq!(
        payload["chunk"], "output from a 120-column pane\n",
        "the console log is replayed as written, cursor sequence and all"
    );

    // Nothing was ever recorded for this one — it ended before anyone watched
    // It — so there is no grid to report and the client falls back to its own.
    // Default.
    let unmeasured = h.lone_session("ariadne-test-unmeasured").await;
    h.write_console_log(&unmeasured.id, "unmeasured output\n");
    let mut body = stream(&h, &unmeasured).await;
    // No size was ever known, so none is claimed.
    expect_sse(&mut body, "snapshot").await;
}

/// pipe-pane can stop mid-character. Those bytes are part of "whatever
/// remains", so they go out lossily instead of vanishing.
#[tokio::test]
async fn a_half_written_character_still_reaches_the_client() {
    let h = gone_tmux().await;
    let session = dead_session(&h).await;
    let mut console = b"cut off: ".to_vec();
    // Two thirds of a three-byte character.
    console.extend_from_slice(&"│".as_bytes()[..2]);
    h.write_console_log(&session.id, &console);

    let mut body = stream(&h, &session).await;

    let payload = expect_sse(&mut body, "snapshot").await;
    assert_eq!(
        payload["chunk"], "cut off: \u{fffd}",
        "the truncated character is replaced, not dropped"
    );
    expect_sse(&mut body, "end").await;
}

/// A pane's writes, not a timer, are what move output along.
///
/// This is the whole of the change: an echoed keystroke used to wait out
/// whatever was left of a 300 ms poll before it even left the daemon, which
/// is what made typing in the web terminal feel like wading. The write is
/// timed against the frame that carries it, several times over, because the
/// one that lands during a liveness check is the interesting one — a typist
/// hits that case too.
#[tokio::test]
async fn output_reaches_the_client_as_soon_as_it_is_written() {
    let h = harness().await;
    let session = h.lone_session("ariadne-prompt").await;
    h.stub_pane("pane snapshot\n");
    h.write_console_log(&session.id, "");

    let mut body = stream(&h, &session).await;
    expect_sse(&mut body, "resize").await;
    expect_sse(&mut body, "snapshot").await;

    let mut log = tokio::fs::OpenOptions::new()
        .append(true)
        .open(h.console_log(&session.id))
        .await
        .unwrap();
    let mut latencies = Vec::new();
    for i in 0..5 {
        // Long enough apart that each one is a keystroke into a quiet pane
        // Rather than the tail of the burst before it.
        tokio::time::sleep(Duration::from_millis(150)).await;
        let written = Instant::now();
        log.write_all(format!("echo {i}\n").as_bytes())
            .await
            .unwrap();
        log.flush().await.unwrap();

        let (name, payload) = parse_sse(&next_sse_message(&mut body).await);
        latencies.push(written.elapsed());
        assert_eq!(name, "delta");
        assert!(
            payload["chunk"]
                .as_str()
                .is_some_and(|chunk| chunk.contains(&format!("echo {i}"))),
            "the delta carries what was written: {payload}"
        );
    }
    let worst = latencies.iter().max().copied().unwrap();
    assert!(
        worst < Duration::from_millis(50),
        "output must reach the stream in well under the old poll: {latencies:?}"
    );
}

/// One `delta` is written, buffered and parsed whole, so an agent that dumps
/// a file must not turn into a single frame of whatever size it happened to
/// write. The burst is split — and nothing about it is lost or reordered in
/// the splitting, half-written characters included.
#[tokio::test]
async fn a_burst_bigger_than_one_frame_arrives_as_several_deltas() {
    let h = harness().await;
    let session = h.lone_session("ariadne-burst").await;
    h.stub_pane("pane snapshot\n");
    h.write_console_log(&session.id, "");

    let mut body = stream(&h, &session).await;
    expect_sse(&mut body, "resize").await;
    expect_sse(&mut body, "snapshot").await;

    // Two and a bit frames' worth in one write, with a multi-byte character
    // Every few bytes so that the cap is certain to fall inside one.
    let burst = "ab│cd".repeat(MAX_CHUNK * 2 / 5);
    assert!(burst.len() > MAX_CHUNK * 2);
    let mut log = tokio::fs::OpenOptions::new()
        .append(true)
        .open(h.console_log(&session.id))
        .await
        .unwrap();
    log.write_all(burst.as_bytes()).await.unwrap();
    log.flush().await.unwrap();

    let (received, frames) = drained(&mut body, burst.len()).await;
    assert!(frames >= 3, "the burst was split, not sent whole: {frames}");
    assert_eq!(
        received, burst,
        "and the frames concatenate back to what was appended"
    );
}

/// The cap has to hold for output that is not text at all. A pane can emit
/// bytes that are not valid UTF-8 — a binary file catted into it, a corrupted
/// escape sequence — and each one decodes to a three-byte replacement
/// character, so a cap counted in bytes *read* would let a frame out at three
/// times the size it promises.
#[tokio::test]
async fn a_burst_of_invalid_bytes_is_capped_by_what_it_decodes_to() {
    let h = harness().await;
    let session = h.lone_session("ariadne-binary").await;
    h.stub_pane("pane snapshot\n");
    h.write_console_log(&session.id, "");

    let mut body = stream(&h, &session).await;
    expect_sse(&mut body, "resize").await;
    expect_sse(&mut body, "snapshot").await;

    // A cap's worth of bytes, none of them valid on its own.
    let burst = vec![0xffu8; MAX_CHUNK];
    let expected = String::from_utf8_lossy(&burst).into_owned();
    let mut log = tokio::fs::OpenOptions::new()
        .append(true)
        .open(h.console_log(&session.id))
        .await
        .unwrap();
    log.write_all(&burst).await.unwrap();
    log.flush().await.unwrap();

    let (received, frames) = drained(&mut body, expected.len()).await;
    assert!(frames >= 3, "the decoded burst was split: {frames}");
    assert_eq!(
        received, expected,
        "and the frames concatenate back to what the log decodes to"
    );
}

/// The live path end to end: pane output shows up as deltas within a second
/// of being written, and killing the session closes the stream with `end`.
#[tokio::test]
#[ignore = "requires tmux"]
async fn a_live_session_streams_new_output_until_it_is_killed() {
    let h = harness().tmux(Tmux::Real).await;
    let tmux_name = format!("ariadne-test-logstream-{}", std::process::id());
    let session = h.lone_session(&tmux_name).await;
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

    let mut body = stream(&h, &session).await;

    // The pane's grid comes first.
    let payload = expect_sse(&mut body, "resize").await;
    assert!(
        payload["cols"].as_u64().is_some_and(|c| c > 0),
        "a real pane reports a real grid: {payload}"
    );

    // Then the pane snapshot.
    expect_sse(&mut body, "snapshot").await;

    // New output reaches the client as a delta, not as a fresh snapshot.
    let payload = expect_sse(&mut body, "delta").await;
    assert!(
        payload["chunk"].as_str().unwrap().contains("tick"),
        "delta carries the new pane output: {payload}"
    );

    h.launcher.tmux.kill_session(&tmux_name).await.unwrap();

    // Trailing output may still be drained; `end` is what closes the stream.
    let mut last = String::new();
    for _ in 0..10 {
        let (name, _) = parse_sse(&next_sse_message(&mut body).await);
        last = name;
        if last == "end" {
            break;
        }
    }
    assert_eq!(last, "end", "killing the session ends the stream");
    sse_is_closed(&mut body).await;
}

#[tokio::test]
async fn an_unknown_session_yields_the_standard_error_envelope() {
    let h = harness().await;

    let envelope = h
        .error(
            get("/v1/sessions/01ARZ3NDEKTSV4RRFFQ69G5FAV/logs/stream"),
            StatusCode::NOT_FOUND,
        )
        .await;
    assert_eq!(envelope.error.code, "not_found");
    assert!(!envelope.error.message.is_empty());
}
