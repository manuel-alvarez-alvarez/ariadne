//! Live session-log streaming: the push counterpart of the `/logs` snapshot.

use std::convert::Infallible;
use std::time::{Duration, Instant};

use axum::extract::{Path, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use futures_util::Stream;
use futures_util::stream::unfold;
use tracing::warn;

use ariadne_api::sessions::{SessionLogChunk, SessionLogEnd};

use super::AppState;
use super::error::ApiResult;
use crate::logtail::LogTail;

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
    /// Emitted on the first poll, before anything is tailed.
    snapshot: Option<String>,
    checked_alive_at: Instant,
}

impl Follower {
    /// Is more output still possible? tmux is the fast signal — the stored
    /// status only catches up on the scheduler's liveness sweep — while the
    /// status catches a tmux name taken over by a revived session.
    async fn alive(&mut self) -> bool {
        self.checked_alive_at = Instant::now();
        self.state
            .launcher
            .tmux
            .has_session(&self.tmux_session)
            .await
            && self
                .state
                .store
                .get_session(&self.session_id)
                .await
                .is_ok_and(|s| s.status().is_live())
    }

    fn due_for_liveness_check(&self) -> bool {
        self.checked_alive_at.elapsed() >= LIVENESS
    }
}

/// Follow a session's terminal output.
///
/// The first message is a `snapshot` event carrying the scrollback the
/// `/logs` endpoint would return; every later `delta` event carries only what
/// has been written since. Both payloads are a `SessionLogChunk`: raw
/// terminal bytes, escape sequences and all, are JSON-encoded so they cannot
/// break SSE's line framing.
///
/// When the session ends — or if it was already over when the request arrived
/// — the remaining output is flushed, a final `end` event (`SessionLogEnd`)
/// is sent and the connection closes. There is no replay and no
/// `Last-Event-ID`: reconnecting starts again from a fresh snapshot.
#[utoipa::path(get, path = "/v1/sessions/{id}/logs/stream", tag = "sessions",
    params(("id" = String, Path, description = "session id")),
    responses((status = 200,
        description = "SSE stream of terminal output (text/event-stream). One `snapshot` \
                       event with the current scrollback, then a `delta` event per burst of \
                       new output — both `{\"chunk\": \"...\"}` (SessionLogChunk) — and a \
                       final `end` event (SessionLogEnd) when the session is over, after \
                       which the stream closes.",
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
    // status is what says whether *this* session is still running.
    let mut alive =
        session.status().is_live() && state.launcher.tmux.has_session(&session.tmux_session).await;
    let snapshot = if alive {
        match state
            .launcher
            .tmux
            .capture_pane(&session.tmux_session, SNAPSHOT_LINES)
            .await
        {
            Ok(pane) => pane,
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
        snapshot: Some(snapshot),
        checked_alive_at: Instant::now(),
    };
    let events = unfold(follower, |mut f| async move {
        if let Some(snapshot) = f.snapshot.take() {
            return Some((Ok(chunk_event("snapshot", &snapshot)), f));
        }
        match f.phase {
            Phase::Following => loop {
                tokio::time::sleep(POLL).await;
                let mut new = f.log.read_new().await;
                // Checked even when there is output to send: a pane that
                // writes on every poll must not keep a finished session's
                // stream open forever.
                if f.due_for_liveness_check() && !f.alive().await {
                    // Whatever the session wrote on its way out, half-written
                    // characters included.
                    new.push_str(&f.log.drain().await);
                    if new.is_empty() {
                        f.phase = Phase::Done;
                        return Some((Ok(end_event(&f.session_id)), f));
                    }
                    f.phase = Phase::Ending;
                    return Some((Ok(chunk_event("delta", &new)), f));
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

fn end_event(session_id: &str) -> Event {
    Event::default().event("end").data(
        serde_json::json!(SessionLogEnd {
            session_id: session_id.to_owned(),
        })
        .to_string(),
    )
}
