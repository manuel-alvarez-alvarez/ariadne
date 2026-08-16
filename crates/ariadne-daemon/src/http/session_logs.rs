//! Live session-log streaming: the push counterpart of the `/logs` snapshot.

use std::collections::VecDeque;
use std::convert::Infallible;
use std::time::{Duration, Instant};

use axum::extract::{Path, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use futures_util::Stream;
use futures_util::stream::unfold;
use tracing::warn;

use ariadne_api::sessions::{SessionLogChunk, SessionLogEnd, SessionPaneSize};

use super::AppState;
use super::error::ApiResult;
use crate::logtail::LogTail;
use crate::tmux::PaneGeometry;

/// How often the console log is polled for new output.
const POLL: Duration = Duration::from_millis(300);
/// How often the session is checked for still being alive. Coarser than
/// [`POLL`]: it forks a `tmux` process and hits the store.
const LIVENESS: Duration = Duration::from_secs(1);
/// Pane lines in the opening snapshot — the same window as `/logs`.
const SNAPSHOT_LINES: u32 = 1000;
/// How often a keep-alive comment is sent on a quiet stream.
const KEEP_ALIVE_SECS: u64 = 15;

/// Where one connection stands.
enum Phase {
    /// Following the console log for new output.
    Following,
    /// The last output is out; `end` comes next.
    Ending,
    /// `end` is out; the stream closes.
    Done,
}

/// One connection's view of a session's terminal.
struct Follower {
    state: AppState,
    session_id: String,
    tmux_session: String,
    log: LogTail,
    phase: Phase,
    /// Ready to go out, in order: the opening `resize` and `snapshot`, and a
    /// `delta` that shares a poll with a pane resize.
    queue: VecDeque<Event>,
    /// Last grid reported to this client, so only changes are sent.
    size: Option<SessionPaneSize>,
    checked_alive_at: Instant,
}

/// What one liveness tick found.
enum Pane {
    /// Still running. `resized` carries the grid when it is not the one the
    /// client was last told about.
    Alive { resized: Option<SessionPaneSize> },
    /// No more output is possible.
    Gone,
}

impl Follower {
    /// Where the pane stands: one `tmux` call for both questions a tick has.
    ///
    /// Measuring the pane *is* the liveness check — `display-message` fails on
    /// a session that is not there, exactly as `has-session` would — so the
    /// grid comes free with the answer instead of forking a second process for
    /// it. The stored status is asked too: tmux names are per (task, role), so
    /// a live pane under this session's name need not be this session's, and
    /// the row is what says whether *this* one is still running.
    async fn poll(&mut self) -> Pane {
        self.checked_alive_at = Instant::now();
        let Ok(geometry) = self
            .state
            .launcher
            .tmux
            .pane_geometry(&self.tmux_session)
            .await
        else {
            return Pane::Gone;
        };
        let live = self
            .state
            .store
            .get_session(&self.session_id)
            .await
            .is_ok_and(|s| s.status().is_live());
        if !live {
            return Pane::Gone;
        }
        let size = SessionPaneSize {
            cols: geometry.cols,
            rows: geometry.rows,
        };
        if self.size == Some(size) {
            return Pane::Alive { resized: None };
        }
        self.size = Some(size);
        Pane::Alive {
            resized: Some(size),
        }
    }

    fn due_for_liveness_check(&self) -> bool {
        self.checked_alive_at.elapsed() >= LIVENESS
    }
}

/// Follow a session's terminal output.
///
/// A live pane opens with a `resize` event (`SessionPaneSize`) carrying the
/// grid it draws against: the snapshot is wrapped at that width and every
/// later repaint is addressed in it, so it comes first and is repeated
/// whenever the pane is resized under us — by `ariadne attach`, say.
///
/// Then a `snapshot` event carrying the scrollback the `/logs` endpoint would
/// return — as the pane's screen rather than as text: it ends where the pane's
/// cursor is (see [`as_screen`]), so the repaints that follow land where they
/// were addressed. Then a `delta` event per burst of new output. Both payloads
/// are a `SessionLogChunk`: raw terminal bytes, escape sequences and all, are
/// JSON-encoded so they cannot break SSE's line framing.
///
/// When the session ends — or if it was already over when the request arrived
/// — the remaining output is flushed, a final `end` event (`SessionLogEnd`)
/// is sent and the connection closes. There is no replay and no
/// `Last-Event-ID`: reconnecting starts again from a fresh snapshot.
#[utoipa::path(get, path = "/v1/sessions/{id}/logs/stream", tag = "sessions",
    params(("id" = String, Path, description = "session id")),
    responses((status = 200,
        description = "SSE stream of terminal output (text/event-stream). For a live pane, a \
                       `resize` event with the grid it draws against (`{\"cols\": 80, \
                       \"rows\": 24}`, SessionPaneSize), repeated whenever the pane is \
                       resized. Then one `snapshot` event with the current scrollback and a \
                       `delta` event per burst of new output — both `{\"chunk\": \"...\"}` \
                       (SessionLogChunk) — and a final `end` event (SessionLogEnd) when the \
                       session is over, after which the stream closes.",
        content_type = "text/event-stream", body = SessionLogChunk),
        (status = 404)))]
pub async fn logs_stream(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    let session = state.store.get_session(&id).await?;
    let mut log = LogTail::new(
        state
            .launcher
            .cfg
            .run_dir
            .join(&session.id)
            .join("console.log"),
    );
    // Mark the console log's end *before* capturing the pane: whatever lands
    // in between is then sent twice rather than not at all.
    log.skip_existing().await;

    // Both halves matter: tmux names are per (task, role), so a live pane
    // under this session's name need not be this session's — the row's own
    // status is what says whether *this* session is still running, and asking
    // tmux to measure the pane asks whether there is one at all (see
    // `Follower::poll`).
    //
    // Measured just before the capture: whatever the pane draws in between is
    // in the console log already, and reaches the client as a delta that moves
    // the cursor along with it.
    let geometry = if session.status().is_live() {
        state
            .launcher
            .tmux
            .pane_geometry(&session.tmux_session)
            .await
            .inspect_err(|e| warn!(session = %session.id, error = %e, "measuring the pane failed"))
            .ok()
    } else {
        None
    };
    let mut alive = geometry.is_some();

    let snapshot = if alive {
        match state
            .launcher
            .tmux
            .capture_pane(&session.tmux_session, SNAPSHOT_LINES)
            .await
        {
            Ok(pane) => match geometry {
                Some(geometry) => as_screen(pane, geometry),
                None => pane,
            },
            // The pane went away between the two calls, or tmux is unwell:
            // degrade to the piped console log, as `/logs` does for a
            // finished session, and treat the session as over.
            Err(e) => {
                warn!(session = %session.id, error = %e, "capturing pane failed; falling back to the console log");
                alive = false;
                log.rewind();
                log.drain().await
            }
        }
    } else {
        // Already dead: the full piped log is all there will ever be.
        log.rewind();
        log.drain().await
    };

    let size = alive
        .then_some(geometry)
        .flatten()
        .map(|g| SessionPaneSize {
            cols: g.cols,
            rows: g.rows,
        });

    let mut queue = VecDeque::new();
    if let Some(size) = size {
        queue.push_back(resize_event(size));
    }
    queue.push_back(chunk_event("snapshot", &snapshot));

    let follower = Follower {
        state,
        session_id: session.id,
        tmux_session: session.tmux_session,
        log,
        phase: if alive {
            Phase::Following
        } else {
            Phase::Ending
        },
        queue,
        size,
        checked_alive_at: Instant::now(),
    };
    let events = unfold(follower, |mut f| async move {
        if let Some(event) = f.queue.pop_front() {
            return Some((Ok(event), f));
        }
        match f.phase {
            Phase::Following => loop {
                tokio::time::sleep(POLL).await;
                let mut new = f.log.read_new().await;
                // Checked even when there is output to send: a pane that
                // writes on every poll must not keep a finished session's
                // stream open forever.
                if f.due_for_liveness_check() {
                    match f.poll().await {
                        Pane::Gone => {
                            // Whatever the session wrote on its way out,
                            // half-written characters included.
                            new.push_str(&f.log.drain().await);
                            if new.is_empty() {
                                f.phase = Phase::Done;
                                return Some((Ok(end_event(&f.session_id)), f));
                            }
                            f.phase = Phase::Ending;
                            return Some((Ok(chunk_event("delta", &new)), f));
                        }
                        // A resize is what the output that follows was drawn
                        // for, so it goes out ahead of the poll's own output
                        // rather than behind it.
                        Pane::Alive {
                            resized: Some(size),
                        } => {
                            if !new.is_empty() {
                                f.queue.push_back(chunk_event("delta", &new));
                            }
                            return Some((Ok(resize_event(size)), f));
                        }
                        Pane::Alive { resized: None } => {}
                    }
                }
                if !new.is_empty() {
                    return Some((Ok(chunk_event("delta", &new)), f));
                }
            },
            Phase::Ending => {
                f.phase = Phase::Done;
                Some((Ok(end_event(&f.session_id)), f))
            }
            Phase::Done => None,
        }
    });
    Ok(
        Sse::new(events)
            .keep_alive(KeepAlive::new().interval(Duration::from_secs(KEEP_ALIVE_SECS))),
    )
}

/// A pane capture turned into the screen it was taken from.
///
/// `capture-pane` prints one line per row of the visible pane, every one of
/// them newline-terminated — so writing it out verbatim leaves the cursor on a
/// row *below* the bottom of the screen it describes, with everything shifted
/// up by one. Dropping that last newline lands the last captured row on the
/// terminal's last row, which is where the pane has it.
///
/// The cursor is then put where the pane's is. A capture says what is on the
/// screen but not where the next byte goes, and a TUI's repaints are addressed
/// relative to that: left at the bottom of the screen, every one of them lands
/// however many rows too low.
fn as_screen(capture: String, geometry: PaneGeometry) -> String {
    let mut screen = capture;
    if screen.ends_with('\n') {
        screen.pop();
    }
    // CUP, 1-based, relative to the visible screen — which the last row of the
    // capture is now the bottom of.
    screen.push_str(&format!(
        "\x1b[{};{}H",
        geometry.cursor_y + 1,
        geometry.cursor_x + 1
    ));
    screen
}

/// Terminal output as one JSON-encoded `data:` line: compact JSON escapes the
/// newlines and control bytes SSE framing would otherwise choke on.
fn chunk_event(name: &str, chunk: &str) -> Event {
    Event::default().event(name).data(
        serde_json::json!(SessionLogChunk {
            chunk: chunk.to_owned(),
        })
        .to_string(),
    )
}

/// The grid the output that follows is drawn for.
fn resize_event(size: SessionPaneSize) -> Event {
    Event::default()
        .event("resize")
        .data(serde_json::json!(size).to_string())
}

fn end_event(session_id: &str) -> Event {
    Event::default().event("end").data(
        serde_json::json!(SessionLogEnd {
            session_id: session_id.to_owned(),
        })
        .to_string(),
    )
}
