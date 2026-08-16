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
/// How long a resized pane is given to yield a screen at its new grid before
/// the connection is dropped for a fresh one. Output is withheld throughout,
/// so this is also how long a viewer may sit on a frozen terminal.
const RESYNC_TIMEOUT: Duration = Duration::from_secs(3);

/// Where one connection stands.
#[derive(Clone, Copy)]
enum Phase {
    /// Following the console log for new output.
    Following,
    /// The screen the client should be holding has not been captured yet: the
    /// stream has just opened, or the pane changed shape under it. Nothing
    /// goes out meanwhile — every byte the pane writes from here belongs to a
    /// grid the client does not have, and the bytes already read belong to one
    /// it is about to lose. `since` bounds the wait.
    Resynchronising { since: Instant },
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
    /// Ready to go out, in order: the `resize` and `snapshot` pair that opens
    /// the stream, and the one that starts it over after a pane resize.
    queue: VecDeque<Event>,
    /// Last grid reported to this client, so only changes are sent.
    size: Option<SessionPaneSize>,
    checked_alive_at: Instant,
}

/// What one liveness tick found.
enum Pane {
    /// Still running, and drawing on this screen.
    Alive(PaneGeometry),
    /// There, but not saying where: tmux still has the session, and the
    /// measurement did not come back. Nothing may be concluded from it — least
    /// of all that the session is over.
    Unreadable,
    /// Confirmed absent, or the session is over. No more output is possible.
    Gone,
}

/// What one attempt at a coherent screen came to.
enum Resync {
    /// The `resize` and the `snapshot` it describes are queued.
    Done,
    /// Nothing happened and nothing was committed: the pane could not be read,
    /// or changed shape while it was being read.
    Retry,
    /// The pane is gone; there is no screen to be had.
    Gone,
}

impl Follower {
    /// Where the pane stands: one `tmux` call for both questions a tick has.
    ///
    /// Measuring the pane doubles as the liveness check — `display-message`
    /// fails on a session that is not there, exactly as `has-session` would —
    /// so the grid comes free with the answer instead of forking a second
    /// process for it. The stored status is asked first, and without forking
    /// anything: tmux names are per (task, role), so a live pane under this
    /// session's name need not be this session's, and the row is what says
    /// whether *this* one is still running.
    ///
    /// A measurement that fails is not an answer, though, and must not be
    /// read as one — `end` stops a client from ever asking again, so it is
    /// owed a second opinion. That is the one case that pays for the second
    /// fork: `has-session` says whether the pane is actually gone or merely
    /// did not answer.
    async fn poll(&mut self) -> Pane {
        self.checked_alive_at = Instant::now();
        let live = self
            .state
            .store
            .get_session(&self.session_id)
            .await
            .is_ok_and(|s| s.status().is_live());
        if !live {
            return Pane::Gone;
        }
        match self
            .state
            .launcher
            .tmux
            .pane_geometry(&self.tmux_session)
            .await
        {
            Ok(geometry) => Pane::Alive(geometry),
            Err(e) => {
                if self
                    .state
                    .launcher
                    .tmux
                    .has_session(&self.tmux_session)
                    .await
                {
                    warn!(session = %self.session_id, error = %e, "measuring the pane failed");
                    Pane::Unreadable
                } else {
                    Pane::Gone
                }
            }
        }
    }

    fn due_for_liveness_check(&self) -> bool {
        self.checked_alive_at.elapsed() >= LIVENESS
    }

    /// Put the client on the pane's current screen: a `resize`, then a
    /// `snapshot` taken in it, queued in that order. This is how a stream
    /// opens and how it recovers from a resize — the same operation, because
    /// the guarantee it makes is the same one.
    ///
    /// A resize cannot be spliced into the byte stream. The output waiting to
    /// go out straddles the moment the pane changed shape — some of it drawn
    /// at the old grid, the rest at the new one — and nothing in the stream
    /// says where the boundary is, so no ordering of "these bytes" against
    /// "this new size" is right for all of them. Since a resized pane is
    /// redrawn by whatever is running in it anyway, the whole screen is
    /// captured again and the pending bytes are dropped along with the size
    /// they were meant for: the snapshot supersedes them, scrollback included.
    ///
    /// The pane is measured on both sides of the capture, and a screen whose
    /// grid changed under it is thrown away rather than reported at either:
    /// the whole point of the exercise is that a screen and the grid it is
    /// described by have to be the same screen's.
    async fn resynchronise(&mut self) -> Resync {
        let before = match self.poll().await {
            Pane::Alive(geometry) => geometry,
            Pane::Unreadable => return Resync::Retry,
            Pane::Gone => return Resync::Gone,
        };
        // Where the log stands *before* the capture, so that whatever the pane
        // writes in between is sent twice rather than not at all. The tail
        // only moves there once there is a screen to have replaced it: an
        // attempt that comes to nothing must leave the follower exactly as it
        // found it, or the bytes it skipped are lost and the client is left
        // drawing the new grid's output at the old one.
        let end = self.log.end_offset().await;
        let capture = match self
            .state
            .launcher
            .tmux
            .capture_pane(&self.tmux_session, SNAPSHOT_LINES)
            .await
        {
            Ok(capture) => capture,
            Err(e) => {
                warn!(session = %self.session_id, error = %e, "capturing the resized pane failed");
                return Resync::Retry;
            }
        };
        let after = match self.poll().await {
            Pane::Alive(geometry) => geometry,
            Pane::Unreadable => return Resync::Retry,
            Pane::Gone => return Resync::Gone,
        };
        if after != before {
            // Resized again while it was being read: this capture describes
            // neither grid for certain, so nothing is claimed about it.
            return Resync::Retry;
        }

        self.log.skip_to(end);
        let size = SessionPaneSize {
            cols: after.cols,
            rows: after.rows,
        };
        self.size = Some(size);
        self.state
            .launcher
            .record_pane_size(&self.session_id, after.cols, after.rows)
            .await;
        self.queue.push_back(resize_event(size));
        self.queue
            .push_back(chunk_event("snapshot", &as_screen(capture, after)));
        Resync::Done
    }

    /// Serve the console log and have done: what a session with no pane left
    /// amounts to, whether it was already over when the request arrived or
    /// went while a screen was being fetched for it.
    ///
    /// The log is raw terminal bytes wrapped at whatever width they were
    /// written at, which tmux can no longer be asked for — hence the size
    /// recorded while the session lived (see `Launcher::record_pane_size`).
    /// Without it a history-only session would be replayed at the client's
    /// default and wrap wrongly for its whole length.
    async fn finish_with_log(&mut self) {
        if let Some((cols, rows)) = self.state.launcher.last_pane_size(&self.session_id).await {
            let size = SessionPaneSize { cols, rows };
            if self.size != Some(size) {
                self.size = Some(size);
                self.queue.push_back(resize_event(size));
            }
        }
        self.log.rewind();
        let log = self.log.drain().await;
        self.queue.push_back(chunk_event("snapshot", &log));
        self.phase = Phase::Ending;
    }

    /// Has the pane drawn on a grid the client has not been told about?
    fn regridded(&self, geometry: PaneGeometry) -> bool {
        self.size
            != Some(SessionPaneSize {
                cols: geometry.cols,
                rows: geometry.rows,
            })
    }
}

/// Follow a session's terminal output.
///
/// The stream opens with a `resize` event (`SessionPaneSize`) carrying the
/// grid the output is drawn at: the snapshot is wrapped at that width and
/// every later repaint is addressed in it. A live pane is measured; a
/// finished one is reported at the last size it was seen at, if it ever was.
///
/// Then a `snapshot` event carrying the scrollback the `/logs` endpoint would
/// return — as the pane's screen rather than as text: it ends where the pane's
/// cursor is (see [`as_screen`]), so the repaints that follow land where they
/// were addressed. Then a `delta` event per burst of new output. Both payloads
/// are a `SessionLogChunk`: raw terminal bytes, escape sequences and all, are
/// JSON-encoded so they cannot break SSE's line framing.
///
/// A pane resized under the stream — by `ariadne attach`, say — sends a
/// `resize` and a *fresh* `snapshot` rather than continuing with deltas: the
/// output in flight straddles the change and belongs to neither grid, so the
/// client starts over at the new one. `snapshot` therefore means "replace
/// everything you have", whenever it arrives. Nothing is sent in between: a
/// delta drawn at a grid the client does not have is the corruption this is
/// all here to avoid. If no coherent screen can be had — the pane cannot be
/// read, or keeps changing shape while it is — the connection is closed
/// *without* an `end`, at the opening as much as later on: the session is not
/// over, and a fresh connection is the shortest way back to a grid and a
/// screen that agree. Only a pane confirmed gone ends a stream.
///
/// When the session ends — or if it was already over when the request arrived
/// — the remaining output is flushed, a final `end` event (`SessionLogEnd`)
/// is sent and the connection closes. There is no replay and no
/// `Last-Event-ID`: reconnecting starts again from a fresh snapshot.
#[utoipa::path(get, path = "/v1/sessions/{id}/logs/stream", tag = "sessions",
    params(("id" = String, Path, description = "session id")),
    responses((status = 200,
        description = "SSE stream of terminal output (text/event-stream). A `resize` event \
                       with the grid the output is drawn at (`{\"cols\": 80, \"rows\": 24}`, \
                       SessionPaneSize), then a `snapshot` event with the current scrollback \
                       and a `delta` event per burst of new output — both \
                       `{\"chunk\": \"...\"}` (SessionLogChunk). A pane resized under the \
                       stream sends a new `resize` followed by a fresh `snapshot`, which \
                       replaces everything sent so far. A final `end` event (SessionLogEnd) \
                       closes the stream when the session is over.",
        content_type = "text/event-stream", body = SessionLogChunk),
        (status = 404)))]
pub async fn logs_stream(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    let session = state.store.get_session(&id).await?;
    let log = LogTail::new(
        state
            .launcher
            .cfg
            .run_dir
            .join(&session.id)
            .join("console.log"),
    );

    // Nothing is captured here. A screen and the grid it is described by have
    // to be one screen's, which takes measuring the pane on both sides of the
    // capture — and that is exactly what the resynchronising phase does, for
    // a pane that resized and for one being seen for the first time alike.
    // The stream therefore opens owing the client a screen, and its first act
    // is to go and get one; a session with no pane left falls out of that as
    // `Resync::Gone` and is served its console log.
    let follower = Follower {
        state,
        session_id: session.id,
        tmux_session: session.tmux_session,
        log,
        phase: Phase::Resynchronising {
            since: Instant::now(),
        },
        queue: VecDeque::new(),
        size: None,
        checked_alive_at: Instant::now(),
    };
    let events = unfold(follower, |mut f| async move {
        loop {
            if let Some(event) = f.queue.pop_front() {
                return Some((Ok(event), f));
            }
            match f.phase {
                Phase::Following => {
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
                            // The pane changed shape under us. `new` is
                            // exactly the output that cannot be placed either
                            // side of that, so it is dropped, and nothing
                            // more goes out until there is a screen at the
                            // new grid to replace the client's with.
                            Pane::Alive(geometry) if f.regridded(geometry) => {
                                f.phase = Phase::Resynchronising {
                                    since: Instant::now(),
                                };
                                continue;
                            }
                            // Nothing learned: a pane that did not answer is
                            // still drawing at the grid it last answered with
                            // as far as anyone here knows, and freezing the
                            // stream over an unanswered question would be a
                            // worse guess than carrying on.
                            Pane::Alive(_) | Pane::Unreadable => {}
                        }
                    }
                    if !new.is_empty() {
                        return Some((Ok(chunk_event("delta", &new)), f));
                    }
                }
                Phase::Resynchronising { since } => match f.resynchronise().await {
                    Resync::Done => {
                        f.phase = Phase::Following;
                        continue;
                    }
                    Resync::Gone => {
                        // No pane to take a screen from — it was over before
                        // the stream opened, or went while one was being
                        // fetched. Either way the console log is what is left
                        // of it, and it is coherent at the size it was
                        // written at.
                        f.finish_with_log().await;
                        continue;
                    }
                    Resync::Retry => {
                        if since.elapsed() >= RESYNC_TIMEOUT {
                            // Out of ways to make this connection make sense.
                            // Closing it without an `end` is not a lie about
                            // the session, and the connection the client opens
                            // next starts over from a grid and a screen that
                            // agree — or waits here again, which is the truth
                            // of the matter while tmux cannot be read.
                            warn!(session = %f.session_id, "no coherent screen from the pane; closing the stream to resynchronise");
                            return None;
                        }
                        tokio::time::sleep(POLL).await;
                    }
                },
                Phase::Ending => {
                    f.phase = Phase::Done;
                    return Some((Ok(end_event(&f.session_id)), f));
                }
                Phase::Done => return None,
            }
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
