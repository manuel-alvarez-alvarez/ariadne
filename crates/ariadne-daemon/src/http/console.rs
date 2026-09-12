//! Ariadne's own console for an agent session.
//!
//! There is nothing to capture here that is not already an agent event: the
//! ACP runtime (`crate::acp`) reports what its agent does on the one
//! ingestion path, so the console is a session-scoped view of
//! that record — every event kind the runtime reports flows through
//! unchanged, so a thought, a message, a tool call, a plan, a permission
//! request or a turn's status all show up exactly as the runtime names them,
//! with no fixed list to fall behind it.
//!
//! The one thing the record does not hold is the turn in flight. The runtime
//! streams that live — a chunk of message or thought text, a tool call's
//! progress — to the console alone: never stored, never on the domain bus.
//! The console stream carries both, and a snapshot taken mid-turn ends on
//! the text so far.

use std::collections::VecDeque;
use std::convert::Infallible;
use std::future::Future;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, Sse};
use futures_util::stream::{once, unfold};
use futures_util::{Stream, StreamExt};
use tokio::sync::broadcast;
use tokio::sync::broadcast::error::{RecvError, TryRecvError};

use ariadne_api::events::AgentEventDto;
use ariadne_api::sessions::ConsoleInputRequest;
use ariadne_api::stream::{DomainEvent, ResyncDto};
use ariadne_store::{Store, StoreError};

use super::AppState;
use super::convert::event_dto;
use super::error::{ApiError, ApiResult, Json};
use super::sse;
use crate::bus::BusEvent;

/// A session's stored and live events as one stream, in the order the daemon
/// gave them ids.
///
/// The stored events come off the domain bus and the live ones off the
/// runtime's console channel, and the two are not ordered against each
/// other: a turn that ends at once has its last chunks and its stored whole
/// ready together, and whichever channel is read first wins. Every event
/// has its id from one monotonic generator, so whenever both channels hold
/// something, what they hold is sorted by id before it goes out.
///
/// A stored event reaches the bus late: the store enqueues it, and the bus
/// publishes it once it has loaded what the event carries. A live event
/// taken after it can therefore be here first, with a higher id. The store
/// is the arbiter: an event's id is taken and the event committed under one
/// lock, the same lock a live event takes its id under, so by the time a
/// live event exists every stored event with a lower id is in the store.
/// Before a live event goes out, the stored events of the session above the
/// watermark — the highest stored id sent so far — and below the live
/// event's id are read from the store and sent first; a copy of them the
/// bus delivers later is at or below the watermark, and is dropped. The
/// bus delivers one session's stored events in id order, because the store
/// enqueues them under that lock and one task publishes them, so the
/// watermark is the whole of the bookkeeping.
pub(super) struct Merge {
    store: Store,
    stored: broadcast::Receiver<BusEvent>,
    live: broadcast::Receiver<AgentEventDto>,
    session_id: String,
    /// The highest stored id sent so far: the snapshot's last event at
    /// first, so a stored event committed while the snapshot was read, on
    /// both the snapshot and the bus, is sent once.
    watermark: Option<String>,
    queue: VecDeque<Queued>,
    /// How many live events the channel dropped while the console opened:
    /// the first thing `next` says, as a resync, since the snapshot went
    /// out short of them.
    lagged: Option<u64>,
}

/// One event waiting to go out, and which channel it came off.
enum Queued {
    Stored(AgentEventDto),
    Live(AgentEventDto),
}

impl Merge {
    pub(super) fn new(
        store: Store,
        stored: broadcast::Receiver<BusEvent>,
        live: broadcast::Receiver<AgentEventDto>,
        session_id: String,
        last_id: Option<String>,
    ) -> Self {
        Self {
            store,
            stored,
            live,
            session_id,
            watermark: last_id,
            queue: VecDeque::new(),
            lagged: None,
        }
    }

    /// Open a session's console: the snapshot it starts with, and the merge
    /// that follows it.
    ///
    /// Both channels are subscribed already, the live one with the text so
    /// far read under the runtime's turn lock (`subscribe_console`). The
    /// stored events are listed after that, so a live event sent between the
    /// two reads is waiting on the live channel with an id below the last
    /// stored one — the last chunk of a turn that ended in between, or the
    /// progress of a call that ended. Sent after the snapshot it would come
    /// after that turn's stop, and a console would draw its text again below
    /// it. So what is waiting on the live channel is read now: an event
    /// below the snapshot's last stored id joins the snapshot in its place,
    /// and one above it is the first the merge sends. Nothing joins late: a
    /// live event takes its id and is sent under the store's event lock, so
    /// every live event below the last stored id was on the channel before
    /// that event was committed.
    ///
    /// A live channel that dropped events while the stored ones were listed
    /// — a turn streaming more chunks than the channel holds, in that moment
    /// — has lost text the snapshot cannot show. The snapshot goes out as it
    /// is, and the merge's first word is a resync, as for any lag: the
    /// client opens again, and reads the text so far afresh.
    pub(super) async fn open(
        store: Store,
        stored: broadcast::Receiver<BusEvent>,
        mut live: broadcast::Receiver<AgentEventDto>,
        session_id: String,
        so_far: Vec<AgentEventDto>,
    ) -> Result<(Vec<AgentEventDto>, Self), StoreError> {
        let recorded = store.list_session_events(&session_id).await?;
        let last_id = recorded.last().map(|event| event.id.clone());
        let mut waiting = Vec::new();
        let mut lagged = None;
        loop {
            match live.try_recv() {
                Ok(dto) => waiting.push(dto),
                Err(TryRecvError::Lagged(missed)) => {
                    lagged = Some(lagged.unwrap_or(0) + missed);
                }
                Err(TryRecvError::Empty | TryRecvError::Closed) => break,
            }
        }
        let mut merge = Self::new(store, stored, live, session_id, last_id);
        merge.lagged = lagged;
        let (below, above): (Vec<AgentEventDto>, Vec<AgentEventDto>) = waiting
            .into_iter()
            .filter(|dto| dto.session_id.as_deref() == Some(merge.session_id.as_str()))
            .partition(|dto| merge.behind_watermark(&dto.id));
        merge.queue.extend(above.into_iter().map(Queued::Live));
        let so_far = so_far.into_iter().chain(below).collect();
        Ok((snapshot_of(recorded, so_far), merge))
    }

    /// The next event of the session. `Err` is the cue to start over from a
    /// fresh snapshot, because the order cannot be kept from here: a channel
    /// dropped events under a receiver that fell behind, or the store could
    /// not be read for the stored events a live event overtook. `None` is a
    /// channel closing, which is the daemon going away.
    pub(super) async fn next(&mut self) -> Option<Result<AgentEventDto, Resync>> {
        if let Some(missed) = self.lagged.take() {
            return Some(Err(Resync::Lagged(missed)));
        }
        loop {
            match self.queue.pop_front() {
                Some(Queued::Stored(dto)) => {
                    if self.behind_watermark(&dto.id) {
                        continue;
                    }
                    self.watermark = Some(dto.id.clone());
                    return Some(Ok(dto));
                }
                Some(Queued::Live(dto)) => {
                    // A live event that cannot be placed is not sent: the
                    // stored events below it would follow it, out of order,
                    // and the client would draw the turn wrong for good. A
                    // fresh snapshot, read from the store, puts them right.
                    let overtaken = match self.overtaken_by(&dto.id).await {
                        Ok(overtaken) => overtaken,
                        Err(error) => {
                            tracing::warn!(
                                session = %self.session_id, error = %error,
                                "reading the stored events a live event overtook failed"
                            );
                            return Some(Err(Resync::Unread));
                        }
                    };
                    if overtaken.is_empty() {
                        return Some(Ok(dto));
                    }
                    self.queue.push_front(Queued::Live(dto));
                    for stored in overtaken.into_iter().rev() {
                        self.queue.push_front(Queued::Stored(stored));
                    }
                    continue;
                }
                None => {}
            }
            let mut batch = Vec::new();
            tokio::select! {
                biased;
                stored = self.stored.recv() => match stored {
                    Ok(event) => batch.extend(self.stored_dto(event)),
                    Err(RecvError::Lagged(missed)) => return Some(Err(Resync::Lagged(missed))),
                    Err(RecvError::Closed) => return None,
                },
                live = self.live.recv() => match live {
                    Ok(dto) => batch.extend(self.live_dto(dto)),
                    Err(RecvError::Lagged(missed)) => return Some(Err(Resync::Lagged(missed))),
                    Err(RecvError::Closed) => return None,
                },
            }
            // Whatever else is already waiting on either channel was
            // published before this moment too, and is ordered with it.
            loop {
                match self.stored.try_recv() {
                    Ok(event) => batch.extend(self.stored_dto(event)),
                    Err(TryRecvError::Lagged(missed)) => {
                        return Some(Err(Resync::Lagged(missed)));
                    }
                    Err(TryRecvError::Empty | TryRecvError::Closed) => break,
                }
            }
            loop {
                match self.live.try_recv() {
                    Ok(dto) => batch.extend(self.live_dto(dto)),
                    Err(TryRecvError::Lagged(missed)) => {
                        return Some(Err(Resync::Lagged(missed)));
                    }
                    Err(TryRecvError::Empty | TryRecvError::Closed) => break,
                }
            }
            batch.sort_by(|a, b| a.id().cmp(b.id()));
            self.queue.extend(batch);
        }
    }

    /// The stored events of the session the bus has not delivered yet, with
    /// ids below a live event's: committed before the live event took its
    /// id, and read from the store so they go out ahead of it.
    async fn overtaken_by(&self, live_id: &str) -> Result<Vec<AgentEventDto>, StoreError> {
        let mut overtaken: Vec<AgentEventDto> = self
            .store
            .list_session_events_after(&self.session_id, self.watermark.as_deref())
            .await?
            .into_iter()
            .map(event_dto)
            .filter(|dto| dto.id.as_str() < live_id)
            .collect();
        overtaken.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(overtaken)
    }

    /// Whether a stored event has been sent already, off the snapshot or off
    /// the store ahead of a live event.
    fn behind_watermark(&self, id: &str) -> bool {
        self.watermark
            .as_deref()
            .is_some_and(|watermark| id <= watermark)
    }

    /// A stored event of this session that has not been sent already.
    fn stored_dto(&self, event: BusEvent) -> Option<Queued> {
        match event.event {
            DomainEvent::AgentEvent(dto)
                if dto.session_id.as_deref() == Some(self.session_id.as_str())
                    && !self.behind_watermark(&dto.id) =>
            {
                Some(Queued::Stored(dto))
            }
            _ => None,
        }
    }

    fn live_dto(&self, dto: AgentEventDto) -> Option<Queued> {
        (dto.session_id.as_deref() == Some(self.session_id.as_str())).then_some(Queued::Live(dto))
    }
}

impl Queued {
    fn id(&self) -> &str {
        match self {
            Self::Stored(dto) | Self::Live(dto) => &dto.id,
        }
    }
}

/// Why a merge stops: the client's cue to start over from a fresh snapshot,
/// because the order promised cannot be kept from here.
#[derive(Debug)]
pub(super) enum Resync {
    /// A channel dropped this many events under a receiver that fell behind.
    Lagged(u64),
    /// The stored events a live event overtook could not be read from the
    /// store, so the live event cannot be sent in its place. The error is
    /// logged where the read failed.
    Unread,
}

impl Resync {
    /// What the client is told it lost: the events a channel dropped. Where
    /// the store could not be read nothing was dropped, and the count is
    /// none.
    pub(super) fn missed(&self) -> u64 {
        match self {
            Self::Lagged(missed) => *missed,
            Self::Unread => 0,
        }
    }
}

/// Open a session's console from the daemon's state: the snapshot `GET
/// /console` answers and the stream and the terminal open with, and the
/// merge the two follow it with. Subscribed to both channels before the
/// snapshot is read, so nothing committed in between is missed; the text so
/// far is read first, the stored events after, and what the live channel
/// holds by then joins the snapshot in its place (`Merge::open`).
pub(super) async fn open_console(
    state: &AppState,
    id: String,
) -> Result<(Vec<AgentEventDto>, Merge), StoreError> {
    let stored = state.events.subscribe();
    let (live, so_far) = state.launcher.acp.subscribe_console(&id).await;
    Merge::open(state.store.clone(), stored, live, id, so_far).await
}

/// How many times `GET /console` opens the console again while the live
/// channel lags under the open.
const SNAPSHOT_TRIES: usize = 3;

/// The whole snapshot, for a reader with no stream to be told resync on.
///
/// A snapshot opened while the live channel lagged is short of the dropped
/// events. The stream's cue for that — a resync, and a fresh open — is not
/// there for `GET /console`, so it opens again, a few times over; `None` is
/// a channel that lagged every time, and the caller refuses rather than
/// answer short.
async fn whole_snapshot<F, Fut>(mut open: F) -> Result<Option<Vec<AgentEventDto>>, StoreError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<(Vec<AgentEventDto>, Merge), StoreError>>,
{
    for _ in 0..SNAPSHOT_TRIES {
        let (snapshot, merge) = open().await?;
        if merge.lagged.is_none() {
            return Ok(Some(snapshot));
        }
    }
    Ok(None)
}

/// The session's events so far, in order: the whole transcript a console
/// opens on. While a turn runs, one `agent_thought_chunk` and one
/// `agent_message_chunk` holding the text so far follow the stored events.
#[utoipa::path(get, path = "/v1/sessions/{id}/console", tag = "sessions",
    params(("id" = String, Path, description = "session id")),
    responses((status = 200, body = [AgentEventDto]), (status = 404),
        (status = 503, description = "the console streamed faster than a snapshot could be \
                                      read, every time it was tried; ask again")))]
pub async fn snapshot(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Vec<AgentEventDto>>> {
    state.store.get_session(&id).await?;
    // The same snapshot the stream opens with; the merge it would follow
    // with is dropped unread, once it says the channel did not lag.
    let snapshot = whole_snapshot(|| open_console(&state, id.clone())).await?;
    snapshot.map(Json).ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "console_busy",
            "the console streamed faster than a snapshot could be read; ask again",
        )
    })
}

/// The snapshot a console opens on: the stored events and the running
/// turn's text so far, in id order.
///
/// The two are read one after the other, the text so far first — under the
/// runtime's turn lock — and the stored events after. The text so far takes
/// fresh ids as it is read, so it falls after every event stored before the
/// read and before every event stored after it: a turn that ended between
/// the two reads puts its stored whole after the text, which the console
/// folds into it, and a prompt that began a turn between them comes after
/// the text of the turn before. Read the other way round, a prompt stored
/// between the reads would be missing from the snapshot and arrive later,
/// behind the text of the turn it began. The live events sent between the
/// two reads come with the text so far (`Merge::open`).
fn snapshot_of(
    stored: Vec<ariadne_store::AgentEvent>,
    so_far: Vec<AgentEventDto>,
) -> Vec<AgentEventDto> {
    let mut snapshot: Vec<AgentEventDto> = stored.into_iter().map(event_dto).collect();
    snapshot.extend(so_far);
    snapshot.sort_by(|a, b| a.id.cmp(&b.id));
    snapshot
}

/// Follow a session's console.
///
/// Opens with a `snapshot` event carrying what `GET /console` would return —
/// every event recorded so far, oldest first, then the running turn's text so
/// far — then an `event` per later one, each an `AgentEventDto`. Subscribing
/// happens before the snapshot is read and every later stored event is
/// compared against the snapshot's last id, so nothing committed in between
/// is ever missed or delivered twice; the live events are read under the
/// runtime's own turn lock for the same guarantee.
///
/// There is no replay and no `Last-Event-ID`: reconnecting starts again from a
/// fresh snapshot. A client that falls too far behind gets a final `resync`
/// event and the connection closes, exactly as `/v1/events/stream` does.
#[utoipa::path(get, path = "/v1/sessions/{id}/console/stream", tag = "sessions",
    params(("id" = String, Path, description = "session id")),
    responses((status = 200,
        description = "SSE stream of console events (text/event-stream). A `snapshot` event \
                       carrying every event recorded so far (`[AgentEventDto]`) and the \
                       running turn's text so far, then an `event` per new one \
                       (`AgentEventDto`) — message and thought chunks, tool call progress, \
                       tool calls, plans, permission requests and turn status all arrive \
                       this way, in the vocabulary the ACP runtime reports them in. The \
                       chunks and the progress are live only: they are never stored and \
                       never reach `/v1/events`. A client that falls behind gets a `resync` \
                       event (ResyncDto) and the connection closes.",
        content_type = "text/event-stream", body = AgentEventDto),
        (status = 404)))]
pub async fn stream(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    state.store.get_session(&id).await?;
    // Subscribed before the snapshot is queried: an event committed in
    // between lands in both places, which the id watermark of the merge
    // resolves in the snapshot's favour rather than sending it twice.
    let (snapshot, merge) = open_console(&state, id).await?;
    let opening = once(async move { Ok(sse::json_event("snapshot", snapshot)) });

    // One stream over both channels, in id order (`Merge`). A receiver that
    // fell behind on either, or a merge that cannot read the store, is told
    // to resync, and the connection ends: a transcript with holes in it is
    // not kept open.
    let events = unfold((merge, false), |(mut merge, closing)| async move {
        if closing {
            return None;
        }
        match merge.next().await {
            Some(Ok(dto)) => Some((Ok(sse::json_event("event", dto)), (merge, false))),
            Some(Err(resync)) => Some((
                Ok(sse::identified_event(
                    "resync",
                    ResyncDto {
                        missed: resync.missed(),
                    },
                )),
                (merge, true),
            )),
            None => None,
        }
    });
    Ok(sse::respond(opening.chain(events)))
}

/// Type into a session.
///
/// While a permission request is pending, the text selects that request's
/// option; otherwise it becomes a fresh `session/prompt` — sent at once if
/// the agent is between turns, or queued, in order, behind whichever one is
/// running and sent the moment it ends.
///
/// Both halves of "live" matter: the row's status, because a finished
/// session takes no more input, and the runtime itself, which has no agent to
/// hand a prompt to for a session whose process is gone.
///
/// And it is the user acting on the session, so whatever it was flagged for
/// comes down with the input: a permission answered, a question typed back,
/// a message read. An agent still blocked raises its own again with its next
/// event. The scheduler hears about it as it does about an ingested event, so
/// the quiet clock and the stream follow.
#[utoipa::path(post, path = "/v1/sessions/{id}/console/input", tag = "sessions",
    operation_id = "console_input",
    request_body = ConsoleInputRequest,
    params(("id" = String, Path, description = "session id")),
    responses((status = 204, description = "Permission answer or prompt accepted"),
        (status = 404), (status = 409)))]
pub async fn input(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<ConsoleInputRequest>,
) -> ApiResult<StatusCode> {
    take_input(&state, &id, req.text).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// What `POST /console/input` does, for every console that types into a
/// session: the HTTP handler, and the terminal console the daemon hosts
/// in process (`super::terminal`).
pub(super) async fn take_input(state: &AppState, id: &str, text: String) -> ApiResult<()> {
    let session = state.store.get_session(id).await?;
    if !session.status().is_live() {
        return Err(ApiError::conflict(format!(
            "session {id} is {} and cannot take input",
            session.status
        )));
    }
    state
        .launcher
        .acp
        .send_input(id, text)
        .map_err(|e| ApiError::conflict(e.to_string()))?;
    state.store.clear_session_attention(id).await?;
    state.notify_scheduler_session(id);
    Ok(())
}

/// Cancel the turn a session is running.
///
/// Sends ACP `session/cancel` to the agent while its `session/prompt` is
/// still in flight. The turn then ends as any other does — the text so far
/// stored, then a `stop` whose `stop_reason` is `cancelled`. A session that
/// is not live, one whose agent process is gone, and one between turns all
/// have nothing to cancel, and say so with `409`.
#[utoipa::path(post, path = "/v1/sessions/{id}/console/cancel", tag = "sessions",
    operation_id = "console_cancel",
    params(("id" = String, Path, description = "session id")),
    responses((status = 204, description = "The running turn was told to cancel"),
        (status = 404), (status = 409)))]
pub async fn cancel(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    cancel_turn(&state, &id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// What `POST /console/cancel` does, for the HTTP handler and the terminal
/// console alike.
pub(super) async fn cancel_turn(state: &AppState, id: &str) -> ApiResult<()> {
    let session = state.store.get_session(id).await?;
    if !session.status().is_live() {
        return Err(ApiError::conflict(format!(
            "session {id} is {} and has no turn to cancel",
            session.status
        )));
    }
    state
        .launcher
        .acp
        .cancel(id)
        .await
        .map_err(|e| ApiError::conflict(e.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tokio::sync::broadcast;

    use ariadne_api::events::AgentEventDto;
    use ariadne_api::stream::DomainEvent;

    use super::{Merge, Resync, Store};
    use crate::bus::BusEvent;

    fn dto(id: &str, kind: &str) -> AgentEventDto {
        AgentEventDto {
            id: id.into(),
            session_id: Some("session".into()),
            task_id: None,
            kind: kind.into(),
            payload: json!({}),
            summary: kind.into(),
            created_at: "2026-09-12T00:00:00.000Z".into(),
        }
    }

    /// A store of its own for one test: the merge reads it for the stored
    /// events a live event overtook.
    async fn test_store() -> (Store, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("test.db")).await.unwrap();
        (store, dir)
    }

    fn stored(id: &str, kind: &str) -> BusEvent {
        BusEvent {
            event: DomainEvent::AgentEvent(dto(id, kind)),
            goal_id: None,
            task_id: None,
        }
    }

    /// A turn that ends at once has its last live chunk and its stored whole
    /// waiting together; the merge hands them over in id order, the chunk
    /// first. The next turn's stored prompt and its first live chunk are
    /// waiting together likewise, and the prompt goes first.
    #[tokio::test]
    async fn a_stored_whole_does_not_pass_the_live_chunk_that_preceded_it() {
        let (store, _dir) = test_store().await;
        let (stored_tx, stored_rx) = broadcast::channel(16);
        let (live_tx, live_rx) = broadcast::channel(16);
        let mut merge = Merge::new(store, stored_rx, live_rx, "session".into(), None);

        live_tx.send(dto("0001", "agent_message_chunk")).unwrap();
        stored_tx.send(stored("0002", "agent_message")).unwrap();
        stored_tx.send(stored("0003", "stop")).unwrap();
        stored_tx
            .send(stored("0004", "user_prompt_submit"))
            .unwrap();
        live_tx.send(dto("0005", "agent_message_chunk")).unwrap();

        let mut order = Vec::new();
        for _ in 0..5 {
            let event = merge.next().await.unwrap().unwrap();
            order.push(event.id);
        }

        assert_eq!(order, ["0001", "0002", "0003", "0004", "0005"]);
    }

    /// Another session's events, and a stored event the snapshot already
    /// held, are not this console's.
    #[tokio::test]
    async fn the_merge_keeps_other_sessions_and_the_snapshots_last_event_out() {
        let (store, _dir) = test_store().await;
        let (stored_tx, stored_rx) = broadcast::channel(16);
        let (live_tx, live_rx) = broadcast::channel(16);
        let mut merge = Merge::new(
            store,
            stored_rx,
            live_rx,
            "session".into(),
            Some("0002".into()),
        );

        stored_tx
            .send(stored("0002", "user_prompt_submit"))
            .unwrap();
        let mut other = dto("0003", "agent_message_chunk");
        other.session_id = Some("elsewhere".into());
        live_tx.send(other).unwrap();
        stored_tx.send(stored("0004", "stop")).unwrap();

        let event = merge.next().await.unwrap().unwrap();
        assert_eq!(event.id, "0004");
    }

    /// The text so far is read before the stored events and takes fresh ids,
    /// so a turn that ended between the two reads puts its stored whole and
    /// stop after the text, and a prompt that began between them comes after
    /// them all: the snapshot is the two in id order, not the stored events
    /// and then the text.
    #[test]
    fn the_text_so_far_sits_by_id_among_the_stored_events() {
        let stored_at = |id: &str, kind: &str| ariadne_store::AgentEvent {
            id: id.into(),
            session_id: Some("session".into()),
            task_id: None,
            kind: kind.into(),
            payload: "{}".into(),
            created_at: "2026-09-12T00:00:00.000Z".into(),
        };
        let snapshot = super::snapshot_of(
            vec![
                stored_at("0001", "user_prompt_submit"),
                stored_at("0003", "agent_message"),
                stored_at("0004", "stop"),
                stored_at("0005", "user_prompt_submit"),
            ],
            vec![dto("0002", "agent_message_chunk")],
        );

        let ids: Vec<&str> = snapshot.iter().map(|event| event.id.as_str()).collect();
        assert_eq!(ids, ["0001", "0002", "0003", "0004", "0005"]);
    }

    /// A stored event reaches the bus only after the bus has loaded what it
    /// carries, so a live event taken after it can arrive first. The store
    /// already holds the stored event, committed under the lock the live
    /// event took its id under: the merge reads it and sends it ahead of
    /// the live event, and drops the copy the bus delivers later.
    #[tokio::test]
    async fn a_live_event_waits_for_the_stored_events_below_it_that_the_bus_has_not_delivered() {
        let (store, _dir) = test_store().await;
        let repository = store
            .create_repository(ariadne_store::NewRepository {
                path: "/tmp/probe".into(),
                base_branch: "main".into(),
                description: None,
            })
            .await
            .unwrap();
        let goal = store
            .create_goal(ariadne_store::NewGoal {
                title: "probe".into(),
                description: String::new(),
                repository_ids: vec![repository.id.clone()],
                pin: ariadne_store::AgentPin {
                    model: "stub:test-model".into(),
                    effort: None,
                },
            })
            .await
            .unwrap();
        let session = store
            .create_session(ariadne_store::NewSession {
                goal_id: goal.id.clone(),
                task_id: None,
                seat: ariadne_core::Seat::Orchestrator,
                task_agent_id: None,
                model: "stub:test-model".into(),
                effort: None,
                worktree_path: None,
            })
            .await
            .unwrap();
        let (stored_tx, stored_rx) = broadcast::channel(16);
        let (live_tx, live_rx) = broadcast::channel(16);
        let mut merge = Merge::new(store.clone(), stored_rx, live_rx, session.id.clone(), None);

        // Stored and committed — and not yet on the bus, which is what the
        // pump's awaited lookup leaves for a moment.
        let whole = store
            .create_event(ariadne_store::NewAgentEvent {
                session_id: Some(session.id.clone()),
                task_id: None,
                kind: "agent_message".into(),
                payload: json!({"text": "done"}),
            })
            .await
            .unwrap();
        let mut chunk = dto(&ariadne_core::id::new_id(), "agent_message_chunk");
        chunk.session_id = Some(session.id.clone());
        live_tx.send(chunk.clone()).unwrap();

        // Nothing is on the bus yet: the two events must come from the live
        // channel and the store, or the second read waits for ever.
        let soon = std::time::Duration::from_secs(2);
        let first = tokio::time::timeout(soon, merge.next())
            .await
            .expect("the stored event is read from the store ahead of the live one")
            .unwrap()
            .unwrap();
        let second = tokio::time::timeout(soon, merge.next())
            .await
            .expect("the live event follows the stored one it overtook")
            .unwrap()
            .unwrap();
        assert_eq!(
            (first.id.as_str(), second.id.as_str()),
            (whole.id.as_str(), chunk.id.as_str()),
            "the stored event goes out before the live one that overtook it"
        );

        // The bus catches up: the copy is not sent again, the next event is.
        let mut late = dto(&whole.id, "agent_message");
        late.session_id = Some(session.id.clone());
        stored_tx
            .send(BusEvent {
                event: DomainEvent::AgentEvent(late),
                goal_id: None,
                task_id: None,
            })
            .unwrap();
        let mut stop = dto(&ariadne_core::id::new_id(), "stop");
        stop.session_id = Some(session.id.clone());
        stored_tx
            .send(BusEvent {
                event: DomainEvent::AgentEvent(stop.clone()),
                goal_id: None,
                task_id: None,
            })
            .unwrap();
        let third = merge.next().await.unwrap().unwrap();
        assert_eq!(
            third.id, stop.id,
            "the bus's copy of the stored event is dropped"
        );
    }

    /// A session of its own in the store, so its stored events take their
    /// ids from the generator the live events take theirs from.
    async fn session_in(store: &Store) -> String {
        let repository = store
            .create_repository(ariadne_store::NewRepository {
                path: "/tmp/probe".into(),
                base_branch: "main".into(),
                description: None,
            })
            .await
            .unwrap();
        let goal = store
            .create_goal(ariadne_store::NewGoal {
                title: "probe".into(),
                description: String::new(),
                repository_ids: vec![repository.id.clone()],
                pin: ariadne_store::AgentPin {
                    model: "stub:test-model".into(),
                    effort: None,
                },
            })
            .await
            .unwrap();
        store
            .create_session(ariadne_store::NewSession {
                goal_id: goal.id.clone(),
                task_id: None,
                seat: ariadne_core::Seat::Orchestrator,
                task_agent_id: None,
                model: "stub:test-model".into(),
                effort: None,
                worktree_path: None,
            })
            .await
            .unwrap()
            .id
    }

    /// A live event of the session, with the next id.
    fn live(session: &str, kind: &str) -> AgentEventDto {
        let mut event = dto(&ariadne_core::id::new_id(), kind);
        event.session_id = Some(session.into());
        event
    }

    /// A stored event of the session, committed and given the next id.
    async fn commit(store: &Store, session: &str, kind: &str) -> String {
        store
            .create_event(ariadne_store::NewAgentEvent {
                session_id: Some(session.into()),
                task_id: None,
                kind: kind.into(),
                payload: json!({}),
            })
            .await
            .unwrap()
            .id
    }

    /// Opening a console reads the text so far, then the stored events. A
    /// chunk sent between the two reads — the last of a turn that ended in
    /// between — waits on the live channel with an id below the turn's
    /// stored whole and stop. It joins the snapshot in its place, and the
    /// merge does not send it again after the stop, where a console would
    /// draw its text a second time.
    #[tokio::test]
    async fn a_live_chunk_sent_between_the_two_reads_joins_the_snapshot_in_its_place() {
        let (store, _dir) = test_store().await;
        let session = session_in(&store).await;
        let (_stored_tx, stored_rx) = broadcast::channel(16);
        let (live_tx, live_rx) = broadcast::channel(16);

        let so_far = live(&session, "agent_message_chunk");
        let between = live(&session, "agent_message_chunk");
        live_tx.send(between.clone()).unwrap();
        let whole = commit(&store, &session, "agent_message").await;
        let stop = commit(&store, &session, "stop").await;

        let (snapshot, mut merge) = Merge::open(
            store,
            stored_rx,
            live_rx,
            session.clone(),
            vec![so_far.clone()],
        )
        .await
        .unwrap();
        let ids: Vec<&str> = snapshot.iter().map(|event| event.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                so_far.id.as_str(),
                between.id.as_str(),
                whole.as_str(),
                stop.as_str()
            ],
            "the chunk sent between the two reads is in the snapshot, below the whole"
        );

        let next_turn = live(&session, "agent_message_chunk");
        live_tx.send(next_turn.clone()).unwrap();
        let first = tokio::time::timeout(std::time::Duration::from_secs(2), merge.next())
            .await
            .expect("the merge sends the next turn's chunk")
            .unwrap()
            .unwrap();
        assert_eq!(
            first.id, next_turn.id,
            "the chunk the snapshot holds is not sent again"
        );
    }

    /// A live event sent after the stored events were read is above the
    /// snapshot's last event: it is not in the snapshot, and it is the first
    /// the merge sends.
    #[tokio::test]
    async fn a_live_event_above_the_snapshots_last_stored_event_follows_it_once() {
        let (store, _dir) = test_store().await;
        let session = session_in(&store).await;
        let (_stored_tx, stored_rx) = broadcast::channel(16);
        let (live_tx, live_rx) = broadcast::channel(16);

        let whole = commit(&store, &session, "agent_message").await;
        let stop = commit(&store, &session, "stop").await;
        let after = live(&session, "agent_message_chunk");
        live_tx.send(after.clone()).unwrap();

        let (snapshot, mut merge) = Merge::open(store, stored_rx, live_rx, session, Vec::new())
            .await
            .unwrap();
        let ids: Vec<&str> = snapshot.iter().map(|event| event.id.as_str()).collect();
        assert_eq!(ids, [whole.as_str(), stop.as_str()]);
        let first = tokio::time::timeout(std::time::Duration::from_secs(2), merge.next())
            .await
            .expect("the merge sends the live event above the snapshot")
            .unwrap()
            .unwrap();
        assert_eq!(first.id, after.id);
    }

    /// A merge that cannot read the store cannot tell which stored events a
    /// live event overtook. It does not send the live event, which the
    /// stored events would then follow out of order: it says resync, and the
    /// client starts over from a snapshot read from the store.
    #[tokio::test]
    async fn a_merge_that_cannot_read_the_store_says_resync_instead_of_sending_a_live_event() {
        let (store, _dir) = test_store().await;
        store.close().await;
        let (_stored_tx, stored_rx) = broadcast::channel(16);
        let (live_tx, live_rx) = broadcast::channel(16);
        let mut merge = Merge::new(store, stored_rx, live_rx, "session".into(), None);

        live_tx.send(dto("0001", "agent_message_chunk")).unwrap();

        let next = merge.next().await;
        assert!(
            matches!(next, Some(Err(Resync::Unread))),
            "the live event is held back and the client is told to resync: {next:?}"
        );
    }

    /// A live channel that dropped events while the console opened has lost
    /// text the snapshot cannot show: the merge says resync first, and the
    /// client opens again.
    #[tokio::test]
    async fn a_live_channel_that_lagged_while_the_console_opened_is_told_to_resync_first() {
        let (store, _dir) = test_store().await;
        let (_stored_tx, stored_rx) = broadcast::channel(16);
        let (live_tx, live_rx) = broadcast::channel(1);
        live_tx.send(dto("0001", "agent_message_chunk")).unwrap();
        live_tx.send(dto("0002", "agent_message_chunk")).unwrap();
        live_tx.send(dto("0003", "agent_message_chunk")).unwrap();

        let (snapshot, mut merge) =
            Merge::open(store, stored_rx, live_rx, "session".into(), Vec::new())
                .await
                .unwrap();
        assert!(
            snapshot.is_empty(),
            "nothing is stored, so the chunk the channel still holds is above the snapshot"
        );
        let next = merge.next().await;
        assert!(
            matches!(next, Some(Err(Resync::Lagged(2)))),
            "the two dropped chunks are a resync before the chunk that is left: {next:?}"
        );
    }

    /// `GET /console` has no stream to be told resync on. A snapshot opened
    /// while the live channel lagged is opened again, and one whose channel
    /// lags every time is refused rather than answered short.
    #[tokio::test]
    async fn the_snapshot_get_answers_is_opened_again_under_a_lag_and_refused_when_it_keeps_lagging()
     {
        let (store, _dir) = test_store().await;
        let opens = std::cell::Cell::new(0usize);
        // An open whose live channel lagged the first `until` times.
        let lagging_until = |until: usize| {
            let store = &store;
            let opens = &opens;
            move || {
                opens.set(opens.get() + 1);
                let lagged = opens.get() <= until;
                let store = store.clone();
                async move {
                    let (_stored_tx, stored_rx) = broadcast::channel(16);
                    let (_live_tx, live_rx) = broadcast::channel(16);
                    let mut merge = Merge::new(store, stored_rx, live_rx, "session".into(), None);
                    merge.lagged = lagged.then_some(1);
                    Ok((vec![dto("0001", "stop")], merge))
                }
            }
        };

        let whole = super::whole_snapshot(lagging_until(2)).await.unwrap();
        assert_eq!(
            whole.map(|snapshot| snapshot.len()),
            Some(1),
            "the third open, with no lag, is the snapshot"
        );
        assert_eq!(opens.get(), 3, "opened again after each lag");

        opens.set(0);
        let none = super::whole_snapshot(lagging_until(usize::MAX))
            .await
            .unwrap();
        assert!(none.is_none(), "a channel that lags every time is refused");
        assert_eq!(opens.get(), super::SNAPSHOT_TRIES, "tried that many times");
    }
}
