//! Reading a live tmux pane coherently, as a stream of SSE frames.
//!
//! Everything here keeps one promise for
//! [`logs_stream`](super::session_logs::logs_stream): what the client draws
//! and the grid it draws it at are always the same screen's. A pane that
//! resizes, or stops answering, invalidates the bytes in flight and the ones
//! about to be read alike, so the follower stops sending and goes and fetches
//! a screen and its measurement together.

use std::collections::VecDeque;
use std::convert::Infallible;
use std::time::{Duration, Instant};

use axum::response::sse::Event;
use futures_util::Stream;
use futures_util::stream::unfold;
use tracing::warn;

use ariadne_api::sessions::{SessionLogChunk, SessionLogEnd, SessionPaneSize};

use super::AppState;
use super::sse;
use crate::log::{LogTail, LogWatch};
use crate::tmux::PaneGeometry;

/// How long a resynchronisation that came to nothing waits before trying
/// again: a frozen terminal for the viewer, against two or three `tmux`
/// forks per attempt.
const RETRY: Duration = Duration::from_millis(300);
/// How often the session is checked for still being alive. It forks a `tmux`
/// process and hits the store, so it is coarse — and it doubles as the
/// ceiling on how long output waits when the watcher misses a write or could
/// not be established at all (see [`LogWatch::changed`]).
const LIVENESS: Duration = Duration::from_secs(1);
/// Pane lines in the opening snapshot — the same window as `/logs`.
const SNAPSHOT_LINES: u32 = 1000;
/// How long a resized pane is given to yield a screen at its new grid before
/// the connection is dropped for a fresh one — the longest a viewer sits on
/// a frozen terminal, since output is withheld throughout.
const RESYNC_TIMEOUT: Duration = Duration::from_secs(3);

/// Where one connection stands.
#[derive(Clone, Copy)]
enum Phase {
    /// Following the console log for new output.
    Following,
    /// The screen the client should be holding has not been captured yet: the
    /// stream has just opened, or the pane changed shape or stopped
    /// answering. Nothing goes out meanwhile — every byte from here belongs
    /// to a grid nobody has confirmed. `since` bounds the wait.
    Resynchronising { since: Instant },
    /// The last output is out; `end` comes next.
    Ending,
    /// `end` is out; the stream closes.
    Done,
}

impl Phase {
    fn resynchronise() -> Self {
        Self::Resynchronising {
            since: Instant::now(),
        }
    }
}

/// One connection's view of a session's terminal.
struct Follower {
    state: AppState,
    session_id: String,
    tmux_session: String,
    log: LogTail,
    /// What says the log has been written to, so the tail is read when there
    /// is something to read rather than on a timer.
    watch: LogWatch,
    phase: Phase,
    /// Ready to go out, in order: a `resize` and the `snapshot` it describes.
    queue: VecDeque<Event>,
    /// Last grid reported to this client, so only changes are sent.
    size: Option<SessionPaneSize>,
    checked_alive_at: Instant,
}

/// What one attempt at a coherent screen came to.
enum Resync {
    /// The `resize` and the `snapshot` it describes are queued.
    Done,
    /// Nothing happened and nothing was committed: the pane could not be read,
    /// or changed shape while it was being read. Nothing may be concluded
    /// from it — least of all that the session is over.
    Retry,
    /// The pane is confirmed gone, or the session is over: no more output is
    /// possible and there is no screen to be had.
    Gone,
}

impl Follower {
    /// Where the pane stands: one `tmux` call for both questions a tick has,
    /// since `display-message` fails on a session that is not there exactly as
    /// `has-session` would. The stored status is asked first, and forks
    /// nothing: tmux names are per (task, role), so a live pane under this
    /// session's name need not be this session's.
    ///
    /// Nothing that merely failed counts as an answer. `Gone` stops a client
    /// from ever asking again, so only two things earn it: a row that says the
    /// session is not running, and a `has-session` that ran and said no.
    async fn poll(&mut self) -> Result<PaneGeometry, Resync> {
        self.checked_alive_at = Instant::now();
        match self.state.store.get_session(&self.session_id).await {
            Ok(session) if !session.status().is_live() => return Err(Resync::Gone),
            Ok(_) => {}
            Err(e) => {
                warn!(session = %self.session_id, error = %e, "reading the session row failed");
                return Err(Resync::Retry);
            }
        }
        let tmux = &self.state.launcher.tmux;
        let e = match tmux.pane_geometry(&self.tmux_session).await {
            Ok(geometry) => return Ok(geometry),
            Err(e) => e,
        };
        // The one case that pays for a second fork: `has-session` says
        // whether the pane is actually gone or merely did not answer.
        match tmux.has_session_checked(&self.tmux_session).await {
            Ok(false) => Err(Resync::Gone),
            Ok(true) => {
                warn!(session = %self.session_id, error = %e, "measuring the pane failed");
                Err(Resync::Retry)
            }
            Err(check) => {
                warn!(session = %self.session_id, error = %e, check = %check, "cannot reach tmux");
                Err(Resync::Retry)
            }
        }
    }

    /// Queue everything the log still holds, then `end`: what a pane confirmed
    /// gone leaves behind, half-written characters included. Long output is
    /// split over as many `delta` events as it takes rather than one frame of
    /// whatever size the session happened to finish at.
    async fn finish_with_last_output(&mut self) {
        loop {
            let chunk = self.log.read_new().await;
            if !chunk.is_empty() {
                self.queue.push_back(chunk_event("delta", &chunk));
            }
            if !self.log.has_backlog() {
                break;
            }
        }
        let last = self.log.drain().await;
        if !last.is_empty() {
            self.queue.push_back(chunk_event("delta", &last));
        }
        self.phase = Phase::Ending;
    }

    /// Put the client on the pane's current screen: a `resize`, then a
    /// `snapshot` taken in it. This is how a stream opens and how it recovers
    /// from a resize — one operation, because the guarantee is the same one.
    ///
    /// A resize cannot be spliced into the byte stream: the output waiting to
    /// go out straddles the moment the pane changed shape, and nothing in it
    /// says where the boundary is. A resized pane is redrawn by whatever runs
    /// in it anyway, so the screen is captured afresh and the pending bytes go
    /// with the size they were meant for. The pane is measured on both sides
    /// of the capture, and a screen whose grid moved under it is thrown away
    /// rather than reported at either.
    async fn resynchronise(&mut self) -> Resync {
        let before = match self.poll().await {
            Ok(grid) => grid,
            Err(no) => return no,
        };
        // Where the log stands *before* the capture, so that whatever the pane
        // writes in between is sent twice rather than not at all. The tail
        // only moves there once there is a screen to have replaced it: an
        // attempt that comes to nothing must leave the follower exactly as it
        // found it.
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
            Ok(grid) => grid,
            Err(no) => return no,
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
        self.state
            .launcher
            .record_pane_size(&self.session_id, after.cols, after.rows)
            .await;
        // Only a grid the client does not already have is worth an event; a
        // pane that merely stopped answering for a moment is the same pane.
        // The screen is sent either way — it is what the withheld bytes were
        // dropped for.
        self.queue_screen(size, chunk_event("snapshot", &as_screen(capture, after)));
        Resync::Done
    }

    /// Serve the console log and have done: what a session with no pane left
    /// amounts to. The log is raw terminal bytes wrapped at whatever width
    /// they were written at, which tmux can no longer be asked for — hence
    /// the size recorded while the session lived (see
    /// `Launcher::record_pane_size`). Without it a history-only session would
    /// be replayed at the client's default and wrap wrongly throughout.
    async fn finish_with_log(&mut self) {
        let size = self.state.launcher.last_pane_size(&self.session_id).await;
        self.log.rewind();
        let log = self.log.drain().await;
        let screen = chunk_event("snapshot", &log);
        match size {
            Some((cols, rows)) => self.queue_screen(SessionPaneSize { cols, rows }, screen),
            None => self.queue.push_back(screen),
        }
        self.phase = Phase::Ending;
    }

    /// Queue a screen, preceded by the grid it is drawn at when that is not
    /// the grid the client already has.
    fn queue_screen(&mut self, size: SessionPaneSize, screen: Event) {
        if self.size != Some(size) {
            self.size = Some(size);
            // The grid the output that follows is drawn for.
            self.queue.push_back(sse::json_event("resize", size));
        }
        self.queue.push_back(screen);
    }

    /// One turn of the following phase: the next burst of output, or nothing
    /// when the phase has moved on.
    ///
    /// Liveness is asked before the wait and even while output is streaming,
    /// or a pane that writes on every wake-up would keep a finished session's
    /// stream open forever. A pane that changed shape and one that will not
    /// say where it is both send the stream back to resynchronising: the grid
    /// the next byte is drawn at is unknown either way, and carrying on would
    /// be guessing that a pane which stopped answering stopped changing too.
    async fn follow(&mut self) -> Option<String> {
        if self.checked_alive_at.elapsed() >= LIVENESS {
            match self.poll().await {
                Err(Resync::Gone) => {
                    self.finish_with_last_output().await;
                    return None;
                }
                Err(_) => {
                    self.phase = Phase::resynchronise();
                    return None;
                }
                Ok(geometry) if self.regridded(geometry) => {
                    self.phase = Phase::resynchronise();
                    return None;
                }
                Ok(_) => {}
            }
        }
        // A read that stopped at the frame cap left the rest of the burst
        // behind it, already written: there is nothing to wait for. Otherwise
        // wait for the pane to write or for the next liveness check to fall
        // due, whichever comes first — a wake-up that never comes costs the
        // output the rest of the interval and nothing more.
        if !self.log.has_backlog() {
            let budget =
                (self.checked_alive_at + LIVENESS).saturating_duration_since(Instant::now());
            self.watch.changed(budget).await;
        }
        Some(self.log.read_new().await).filter(|chunk| !chunk.is_empty())
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

/// The frames one connection to `session`'s pane produces, in order.
pub fn follow(
    state: AppState,
    session: ariadne_store::AgentSession,
) -> impl Stream<Item = Result<Event, Infallible>> {
    let console_log = state
        .launcher
        .cfg
        .run_dir
        .join(&session.id)
        .join("console.log");
    // Nothing is captured here: the stream opens owing the client a screen,
    // and going to get one is what the resynchronising phase does — for a
    // pane seen for the first time as for one that resized.
    let follower = Follower {
        state,
        session_id: session.id,
        tmux_session: session.tmux_session,
        log: LogTail::new(&console_log),
        watch: LogWatch::new(&console_log),
        phase: Phase::resynchronise(),
        queue: VecDeque::new(),
        size: None,
        checked_alive_at: Instant::now(),
    };
    unfold(follower, |mut f| async move {
        loop {
            if let Some(event) = f.queue.pop_front() {
                return Some((Ok(event), f));
            }
            match f.phase {
                Phase::Following => {
                    if let Some(chunk) = f.follow().await {
                        return Some((Ok(chunk_event("delta", &chunk)), f));
                    }
                }
                Phase::Resynchronising { since } => match f.resynchronise().await {
                    Resync::Done => {
                        f.phase = Phase::Following;
                        continue;
                    }
                    // No pane to take a screen from — it was over before the
                    // stream opened, or went while one was being fetched.
                    Resync::Gone => {
                        f.finish_with_log().await;
                        continue;
                    }
                    Resync::Retry => {
                        if since.elapsed() >= RESYNC_TIMEOUT {
                            // Out of ways to make this connection make sense.
                            // Closing it without an `end` is no lie about the
                            // session, and the next one starts over from a
                            // grid and a screen that agree.
                            warn!(session = %f.session_id, "no coherent screen from the pane; closing the stream to resynchronise");
                            return None;
                        }
                        tokio::time::sleep(RETRY).await;
                    }
                },
                Phase::Ending => {
                    f.phase = Phase::Done;
                    let end = SessionLogEnd {
                        session_id: f.session_id.clone(),
                    };
                    return Some((Ok(sse::json_event("end", end)), f));
                }
                Phase::Done => return None,
            }
        }
    })
}

/// A pane capture turned into the screen it was taken from.
///
/// `capture-pane` newline-terminates every row, so writing it out verbatim
/// leaves the cursor a row *below* the screen it describes, everything
/// shifted up by one; dropping that last newline lands the last captured row
/// where the pane has it. The cursor is then put where the pane's is with a
/// CUP — a capture says what is on the screen but not where the next byte
/// goes, and a TUI's repaints are addressed relative to that.
fn as_screen(capture: String, geometry: PaneGeometry) -> String {
    let mut screen = capture;
    if screen.ends_with('\n') {
        screen.pop();
    }
    // 1-based, and relative to the visible screen — whose bottom row the last
    // captured row now is.
    screen.push_str(&format!(
        "\x1b[{};{}H",
        geometry.cursor_y + 1,
        geometry.cursor_x + 1
    ));
    screen
}

/// Terminal output — raw bytes, escape sequences and all — as one `snapshot`
/// or `delta` event.
fn chunk_event(name: &str, chunk: &str) -> Event {
    let chunk = chunk.to_owned();
    sse::json_event(name, SessionLogChunk { chunk })
}
