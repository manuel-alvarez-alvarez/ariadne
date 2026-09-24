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
//!
//! A console opens on the newest page of the record rather than on all of
//! it, and follows the two channels from there without reading the store
//! again ([`SNAPSHOT_EVENTS`], [`Merge`]).

use std::collections::{HashMap, VecDeque};
use std::convert::Infallible;
use std::future::Future;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, Sse};
use futures_util::stream::{once, unfold};
use futures_util::{Stream, StreamExt};
use serde_json::Value;
use tokio::sync::broadcast;
use tokio::sync::broadcast::error::{RecvError, TryRecvError};

use ariadne_api::events::AgentEventDto;
use ariadne_api::sessions::ConsoleInputRequest;
use ariadne_api::stream::ResyncDto;
use ariadne_store::{AgentEvent, EventFilter, EventOrder, Store, StoreError};

use super::AppState;
use super::convert::event_dto;
use super::error::{ApiError, ApiResult, Json};
use super::sse;
use crate::bus::{BusEvent, EventBus};

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
/// A stored event reaches the bus late: the store commits it and hands it to
/// the bus's pump, which publishes it once it has loaded what the event
/// carries. A live event taken after it can therefore be here first, with a
/// higher id. Two things put that right, and neither reads the store:
///
/// - The store is the arbiter of the ids: an event's id is taken and the
///   event handed over under one lock, the same lock a live event takes its
///   id under. So by the time a live event is here, every stored event with
///   a lower id is with the pump, and everything committed after it has a
///   higher id.
/// - The pump answers [`EventBus::drained`] only once it has published
///   everything it was handed before the question. A live event waits for
///   one answer, and whatever the bus then holds joins it and is sorted by
///   id: the stored events below it go out first.
///
/// So one answer settles a whole batch of live events, however many chunks
/// a turn streams, and the store is read once — for the snapshot the console
/// opened on. A copy of a stored event that snapshot already held is at or
/// below the watermark, the highest stored id sent so far, and is dropped;
/// the pump publishes one session's stored events in id order, so the
/// watermark is the whole of the bookkeeping.
///
/// The pump drops an event whose payload it cannot load, and no drain can
/// answer for one that will never be published. It counts them
/// ([`EventBus::dropped`]), and a count that has grown since the console
/// opened is a resync: the client opens again on a snapshot, which is read
/// from the store and holds the event. The count is read after the drain
/// and before anything goes out, since the drain is where the events below
/// a live one are published, and so where one of them is dropped.
pub(super) struct Merge {
    /// The bus the stored events come off, and the pump behind it that a
    /// live event waits for.
    events: EventBus,
    stored: broadcast::Receiver<BusEvent>,
    live: broadcast::Receiver<AgentEventDto>,
    session_id: String,
    /// The highest stored id sent so far: the snapshot's last event at
    /// first, so a stored event committed while the snapshot was read, on
    /// both the snapshot and the bus, is sent once.
    watermark: Option<String>,
    queue: VecDeque<Queued>,
    /// Whether the queue has waited for the pump since the last live event
    /// joined it. A live event goes out only once it has.
    settled: bool,
    /// What the bus had dropped when this console opened. Anything it drops
    /// after that is an event of the session the stream can never give.
    dropped: u64,
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
        events: EventBus,
        stored: broadcast::Receiver<BusEvent>,
        live: broadcast::Receiver<AgentEventDto>,
        session_id: String,
        last_id: Option<String>,
    ) -> Self {
        Self {
            dropped: events.dropped(&session_id),
            events,
            stored,
            live,
            session_id,
            watermark: last_id,
            queue: VecDeque::new(),
            settled: true,
            lagged: None,
        }
    }

    /// Open a session's console: the snapshot it starts with, and the merge
    /// that follows it.
    ///
    /// Both channels are subscribed already, the live one with the text so
    /// far read under the runtime's turn lock (`subscribe_console`). The
    /// stored events are read after that, so a live event sent between the
    /// two reads is waiting on the live channel with an id below the last
    /// stored one — the last chunk of a turn that ended in between, or the
    /// progress of a call that ended. Sent after the snapshot it would come
    /// after that turn's stop, and a console would draw its text again below
    /// it. So what is waiting on the live channel is read now: an event
    /// below the snapshot's last stored id joins the snapshot in its place,
    /// and one above it is the first the merge sends, once it has waited for
    /// the pump as any live event does. Nothing joins late: a live event
    /// takes its id and is sent under the store's event lock, so every live
    /// event below the last stored id was on the channel before that event
    /// was committed.
    ///
    /// A live channel that dropped events while the stored ones were read —
    /// a turn streaming more chunks than the channel holds, in that moment —
    /// has lost text the snapshot cannot show. The snapshot goes out as it
    /// is, and the merge's first word is a resync, as for any lag: the
    /// client opens again, and reads the text so far afresh.
    pub(super) async fn open(
        store: &Store,
        events: EventBus,
        stored: broadcast::Receiver<BusEvent>,
        mut live: broadcast::Receiver<AgentEventDto>,
        session_id: String,
        so_far: Vec<AgentEventDto>,
    ) -> Result<(Vec<AgentEventDto>, Self), StoreError> {
        // Read before the page is: an event the pump drops from here on is
        // one the page cannot hold either, and only a later open shows it.
        let dropped = events.dropped(&session_id);
        let recorded = newest_page(store, &session_id).await?;
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
        let mut merge = Self::new(events, stored, live, session_id, last_id);
        merge.dropped = dropped;
        merge.lagged = lagged;
        let (below, above): (Vec<AgentEventDto>, Vec<AgentEventDto>) = waiting
            .into_iter()
            .filter(|dto| dto.session_id.as_deref() == Some(merge.session_id.as_str()))
            .partition(|dto| merge.behind_watermark(&dto.id));
        merge.settled = above.is_empty();
        merge.queue.extend(above.into_iter().map(Queued::Live));
        let so_far = so_far.into_iter().chain(below).collect();
        Ok((snapshot_of(recorded, so_far), merge))
    }

    /// The next event of the session. `Err` is the cue to start over from a
    /// fresh snapshot, because the session cannot be given whole from here:
    /// a channel dropped events under a receiver that fell behind, or the
    /// bus dropped an event it could not load. `None` is a channel closing,
    /// which is the daemon going away.
    pub(super) async fn next(&mut self) -> Option<Result<AgentEventDto, Resync>> {
        if let Some(missed) = self.lagged.take() {
            return Some(Err(Resync::missing(missed)));
        }
        loop {
            if !self.settled
                && let Err(resync) = self.settle().await
            {
                return Some(Err(resync));
            }
            // An event the bus dropped is in the store and on no channel, so
            // no drain can answer for it and nothing later brings it. Read
            // after the drain, because the drain is where the pump publishes
            // the events below the live one, and so where it drops one; read
            // before anything goes out, because what goes out next would go
            // over the hole. A drop after this was committed after the live
            // event took its id, so it is above it and the next loop catches
            // it in its turn.
            let undelivered = self
                .events
                .dropped(&self.session_id)
                .saturating_sub(self.dropped);
            if undelivered > 0 {
                return Some(Err(Resync::missing(undelivered)));
            }
            while let Some(queued) = self.queue.pop_front() {
                match queued {
                    Queued::Stored(dto) => {
                        if self.behind_watermark(&dto.id) {
                            continue;
                        }
                        self.watermark = Some(dto.id.clone());
                        return Some(Ok(dto));
                    }
                    Queued::Live(dto) => return Some(Ok(dto)),
                }
            }
            match self.fill().await {
                Some(Ok(())) => {}
                Some(Err(resync)) => return Some(Err(resync)),
                None => return None,
            }
        }
    }

    /// Wait for whichever channel speaks first, and take everything both
    /// hold by then: what is waiting was published before this moment too,
    /// and is ordered with it. `None` is a channel closing.
    async fn fill(&mut self) -> Option<Result<(), Resync>> {
        let mut batch = Vec::new();
        tokio::select! {
            biased;
            stored = self.stored.recv() => match stored {
                Ok(event) => batch.extend(self.stored_dto(event)),
                Err(RecvError::Lagged(missed)) => return Some(Err(Resync::missing(missed))),
                Err(RecvError::Closed) => return None,
            },
            live = self.live.recv() => match live {
                Ok(dto) => batch.extend(self.live_dto(dto)),
                Err(RecvError::Lagged(missed)) => return Some(Err(Resync::missing(missed))),
                Err(RecvError::Closed) => return None,
            },
        }
        if let Err(resync) = self.take_stored(&mut batch) {
            return Some(Err(resync));
        }
        if let Err(resync) = self.take_live(&mut batch) {
            return Some(Err(resync));
        }
        // A live event in hand has not waited for the pump yet, so the
        // stored events below it may still be with it.
        self.settled = !batch.iter().any(|queued| matches!(queued, Queued::Live(_)));
        batch.sort_by(|a, b| a.id().cmp(b.id()));
        self.queue.extend(batch);
        Some(Ok(()))
    }

    /// The queue, settled against the pump.
    ///
    /// A live event in it was taken after every stored event with a lower id
    /// was committed, and a stored event is on the bus a moment after it is
    /// committed. So the pump is asked for what it holds, and what it
    /// publishes is sorted into the queue, ahead of the live event it
    /// overtook.
    async fn settle(&mut self) -> Result<(), Resync> {
        self.settled = true;
        self.events.drained().await;
        let mut batch: Vec<Queued> = self.queue.drain(..).collect();
        self.take_stored(&mut batch)?;
        batch.sort_by(|a, b| a.id().cmp(b.id()));
        self.queue = batch.into();
        Ok(())
    }

    /// The stored events of the session waiting on the bus, into the batch.
    fn take_stored(&mut self, batch: &mut Vec<Queued>) -> Result<(), Resync> {
        loop {
            match self.stored.try_recv() {
                Ok(event) => batch.extend(self.stored_dto(event)),
                Err(TryRecvError::Lagged(missed)) => return Err(Resync::missing(missed)),
                Err(TryRecvError::Empty | TryRecvError::Closed) => return Ok(()),
            }
        }
    }

    /// The live events of the session waiting on the runtime's channel, into
    /// the batch.
    fn take_live(&mut self, batch: &mut Vec<Queued>) -> Result<(), Resync> {
        loop {
            match self.live.try_recv() {
                Ok(dto) => batch.extend(self.live_dto(dto)),
                Err(TryRecvError::Lagged(missed)) => return Err(Resync::missing(missed)),
                Err(TryRecvError::Empty | TryRecvError::Closed) => return Ok(()),
            }
        }
    }

    /// Whether a stored event has been sent already, off the snapshot or off
    /// the bus.
    fn behind_watermark(&self, id: &str) -> bool {
        self.watermark
            .as_deref()
            .is_some_and(|watermark| id <= watermark)
    }

    /// A stored event of this session that has not been sent already.
    fn stored_dto(&self, event: BusEvent) -> Option<Queued> {
        let dto = event.recorded?;
        (dto.session_id.as_deref() == Some(self.session_id.as_str())
            && !self.behind_watermark(&dto.id))
        .then(|| Queued::Stored(Arc::unwrap_or_clone(dto)))
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

/// Why a merge stops: the events it cannot give, which leave a hole in the
/// transcript. A channel dropped them under a receiver that fell behind, or
/// the bus dropped an event whose payload it could not load. Either way it
/// is the client's cue to start over from a fresh snapshot.
#[derive(Debug)]
pub(super) struct Resync {
    missed: u64,
}

impl Resync {
    fn missing(missed: u64) -> Self {
        Self { missed }
    }

    /// What the client is told it lost.
    pub(super) fn missed(&self) -> u64 {
        self.missed
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
    Merge::open(&state.store, state.events.clone(), stored, live, id, so_far).await
}

/// How many of a session's stored events a console opens on: the newest
/// page of them, and the most `Store::list_events` gives for one read.
///
/// A session that has run for hours holds thousands of events, and a tool
/// end averages ten kilobytes, so its whole transcript is tens of megabytes
/// to read out of the store and to send. A console opens on the recent past
/// instead. A client that wants what is further back walks it with `GET
/// /v1/events` (`session`, `order=desc` and `before`, 012).
const SNAPSHOT_EVENTS: i64 = 200;

/// The newest page of a session's stored events, oldest first.
async fn newest_page(store: &Store, session_id: &str) -> Result<Vec<AgentEvent>, StoreError> {
    let mut page = store
        .list_events(EventFilter {
            session_id: Some(session_id.to_string()),
            order: EventOrder::Desc,
            limit: SNAPSHOT_EVENTS,
            ..EventFilter::default()
        })
        .await?;
    page.reverse();
    Ok(page)
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

/// The session's newest events, in order: the transcript a console opens on.
/// It is a page of at most two hundred, the recent past rather than every
/// turn a session ever ran; `GET /v1/events` walks back from it. While a
/// turn runs, one `agent_thought_chunk` or `agent_message_chunk` holding the
/// text so far follows the stored events.
#[utoipa::path(get, path = "/v1/sessions/{id}/console", tag = "sessions",
    params(("id" = String, Path, description = "session id")),
    responses((status = 200, body = [AgentEventDto]), (status = 404),
        (status = 503, description = "the console streamed faster than a snapshot could be \
                                      read, every time it was tried; ask again")))]
pub(super) async fn snapshot(
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

/// The snapshot a console opens on: the page of stored events and the
/// running turn's text so far, in id order.
///
/// The two are read one after the other, the text so far first — under the
/// runtime's turn lock — and the stored events after. The text so far takes
/// fresh ids as it is read, so it falls after every event stored before the
/// read and before every event stored after it: a run of text that ended
/// between the two reads puts its stored whole after the text, which the
/// console folds into it, and a prompt that began a turn between them comes
/// after the text of the turn before. Read the other way round, a prompt stored
/// between the reads would be missing from the snapshot and arrive later,
/// behind the text of the turn it began. The live events sent between the
/// two reads come with the text so far (`Merge::open`).
fn snapshot_of(stored: Vec<AgentEvent>, so_far: Vec<AgentEventDto>) -> Vec<AgentEventDto> {
    let mut snapshot = whole_calls(stored.into_iter().map(event_dto).collect());
    snapshot.extend(so_far);
    snapshot.sort_by(|a, b| a.id.cmp(&b.id));
    snapshot
}

/// The page, less the end of any tool call whose start it cut off.
///
/// A call is two stored events: the `pre_tool_use` that opens it, carrying
/// what the agent asked for, and the `post_tool_use` that ends it, carrying
/// what came back. A console folds the two into one block, and the input is
/// on the first of them alone. So an end whose start is below the page would
/// draw as a call nobody made, with no input to show: the page keeps a call
/// whole, or not at all.
///
/// The page is walked in order and each start is consumed by the first end
/// that matches it, because a name is all that pairs the two where an event
/// carries no call id: an end cut off from its start would otherwise take
/// the start of a later call of the same tool, and survive on it.
fn whole_calls(page: Vec<AgentEventDto>) -> Vec<AgentEventDto> {
    let mut open: HashMap<String, usize> = HashMap::new();
    page.into_iter()
        .filter(|event| match event.kind.as_str() {
            "pre_tool_use" => {
                *open.entry(call_of(event)).or_default() += 1;
                true
            }
            "post_tool_use" => match open.get_mut(&call_of(event)) {
                Some(started) if *started > 0 => {
                    *started -= 1;
                    true
                }
                _ => false,
            },
            _ => true,
        })
        .collect()
}

/// What pairs a call's end with its start: the ACP call id, and the tool's
/// name where an event carries no id, as the transcript pairs them. An event
/// with neither is one call, and pairs with another of its kind.
fn call_of(event: &AgentEventDto) -> String {
    event
        .payload
        .pointer("/acp/toolCallId")
        .and_then(Value::as_str)
        .or_else(|| event.payload.get("tool_call_id").and_then(Value::as_str))
        .or_else(|| event.payload.get("tool_name").and_then(Value::as_str))
        .unwrap_or_default()
        .to_string()
}

/// Follow a session's console.
///
/// Opens with a `snapshot` event carrying what `GET /console` would return —
/// the newest page of stored events, oldest first, then the running turn's
/// text so far — then an `event` per later one, each an `AgentEventDto`.
/// Subscribing happens before the snapshot is read and every later stored
/// event is compared against the snapshot's last id, so nothing committed in
/// between is ever missed or delivered twice; the live events are read under
/// the runtime's own turn lock for the same guarantee.
///
/// There is no replay and no `Last-Event-ID`: reconnecting starts again from a
/// fresh snapshot. A client that falls too far behind gets a final `resync`
/// event and the connection closes, exactly as `/v1/events/stream` does.
#[utoipa::path(get, path = "/v1/sessions/{id}/console/stream", tag = "sessions",
    params(("id" = String, Path, description = "session id")),
    responses((status = 200,
        description = "SSE stream of console events (text/event-stream). A `snapshot` event \
                       carrying the newest page of recorded events (`[AgentEventDto]`) and the \
                       running turn's text so far, then an `event` per new one \
                       (`AgentEventDto`) — message and thought chunks, tool call progress, \
                       tool calls, plans, permission requests and turn status all arrive \
                       this way, in the vocabulary the ACP runtime reports them in. The \
                       chunks and the progress are live only: they are never stored and \
                       never reach `/v1/events`. A client that falls behind gets a `resync` \
                       event (ResyncDto) and the connection closes.",
        content_type = "text/event-stream", body = AgentEventDto),
        (status = 404)))]
pub(super) async fn stream(
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
    // fell behind on either, or a merge whose event the bus dropped, is told
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
pub(super) async fn input(
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
pub(super) async fn cancel(
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

    use std::sync::Arc;

    use ariadne_api::events::{AgentEventDto, AgentEventSummaryDto};
    use ariadne_api::stream::DomainEvent;

    use super::{EventBus, Merge, SNAPSHOT_EVENTS, Store};
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

    /// A store of its own for one test: the console reads its page of stored
    /// events out of it, and the ids of a session's events come from it.
    async fn test_store() -> (Store, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("test.db")).await.unwrap();
        (store, dir)
    }

    fn stored(id: &str, kind: &str) -> BusEvent {
        published(dto(id, kind))
    }

    fn published(dto: AgentEventDto) -> BusEvent {
        BusEvent {
            event: DomainEvent::AgentEvent(AgentEventSummaryDto::from(&dto)),
            goal_id: None,
            task_id: None,
            recorded: Some(Arc::new(dto)),
        }
    }

    /// A turn that ends at once has its last live chunk and its stored whole
    /// waiting together; the merge hands them over in id order, the chunk
    /// first. The next turn's stored prompt and its first live chunk are
    /// waiting together likewise, and the prompt goes first.
    #[tokio::test]
    async fn a_stored_whole_does_not_pass_the_live_chunk_that_preceded_it() {
        let (stored_tx, stored_rx) = broadcast::channel(16);
        let (live_tx, live_rx) = broadcast::channel(16);
        let mut merge = Merge::new(EventBus::new(), stored_rx, live_rx, "session".into(), None);

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
        let (stored_tx, stored_rx) = broadcast::channel(16);
        let (live_tx, live_rx) = broadcast::channel(16);
        let mut merge = Merge::new(
            EventBus::new(),
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

    /// One stored event, as the store keeps it.
    fn row(id: &str, kind: &str, payload: serde_json::Value) -> ariadne_store::AgentEvent {
        ariadne_store::AgentEvent {
            id: id.into(),
            session_id: Some("session".into()),
            task_id: None,
            kind: kind.into(),
            payload: payload.to_string(),
            created_at: "2026-09-12T00:00:00.000Z".into(),
        }
    }

    /// The text so far is read before the stored events and takes fresh ids,
    /// so a turn that ended between the two reads puts its stored whole and
    /// stop after the text, and a prompt that began between them comes after
    /// them all: the snapshot is the two in id order, not the stored events
    /// and then the text.
    #[test]
    fn the_text_so_far_sits_by_id_among_the_stored_events() {
        let snapshot = super::snapshot_of(
            vec![
                row("0001", "user_prompt_submit", json!({})),
                row("0003", "agent_message", json!({})),
                row("0004", "stop", json!({})),
                row("0005", "user_prompt_submit", json!({})),
            ],
            vec![dto("0002", "agent_message_chunk")],
        );

        let ids: Vec<&str> = snapshot.iter().map(|event| event.id.as_str()).collect();
        assert_eq!(ids, ["0001", "0002", "0003", "0004", "0005"]);
    }

    /// A tool call is two stored events, and the page can begin between
    /// them. The end alone carries no input, and the console has no call to
    /// fold it into, so the page drops it. The call the page holds whole
    /// keeps both its events, and the input its start carries.
    #[test]
    fn a_page_that_cut_a_calls_start_off_drops_its_end_and_keeps_a_whole_call() {
        let snapshot = super::snapshot_of(
            vec![
                row(
                    "0001",
                    "post_tool_use",
                    json!({"tool_name": "Read", "acp": {"toolCallId": "call-1"}}),
                ),
                row(
                    "0002",
                    "pre_tool_use",
                    json!({"tool_name": "Write", "tool_input": {"path": "a.rs"},
                           "acp": {"toolCallId": "call-2"}}),
                ),
                row(
                    "0003",
                    "post_tool_use",
                    json!({"tool_name": "Write", "acp": {"toolCallId": "call-2"}}),
                ),
            ],
            Vec::new(),
        );

        let ids: Vec<&str> = snapshot.iter().map(|event| event.id.as_str()).collect();
        assert_eq!(
            ids,
            ["0002", "0003"],
            "the end of the call the page cut in half is not in the snapshot"
        );
        assert_eq!(
            snapshot[0].payload["tool_input"]["path"], "a.rs",
            "the call the page holds whole still shows its input"
        );
    }

    /// An event that carries no call id is paired by the tool's name, and an
    /// agent calls one tool many times. The page consumes each start with
    /// the first end that follows it, so an end the bound cut off cannot
    /// live on the start of a later call of the same tool.
    #[test]
    fn an_end_cut_off_from_its_start_does_not_take_the_start_of_a_later_call() {
        let snapshot = super::snapshot_of(
            vec![
                // The page begins inside a call: its start is below the bound.
                row("0001", "post_tool_use", json!({"tool_name": "Bash"})),
                row(
                    "0002",
                    "pre_tool_use",
                    json!({"tool_name": "Bash", "tool_input": {"command": "ls"}}),
                ),
                row("0003", "post_tool_use", json!({"tool_name": "Bash"})),
                // A call of the same tool that is still running at the bound.
                row(
                    "0004",
                    "pre_tool_use",
                    json!({"tool_name": "Bash", "tool_input": {"command": "cat"}}),
                ),
            ],
            Vec::new(),
        );

        let ids: Vec<&str> = snapshot.iter().map(|event| event.id.as_str()).collect();
        assert_eq!(
            ids,
            ["0002", "0003", "0004"],
            "the orphan end is dropped, and the call that runs on keeps its start"
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
                goal_id: Some(goal.id.clone()),
                task_id: None,
                seat: Some(ariadne_core::Seat::Orchestrator),
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

    /// A stored event of the session on the bus, with the next id: what the
    /// pump publishes once it has loaded what the event carries.
    fn publish(events: &EventBus, session: &str, kind: &str) -> String {
        let dto = live(session, kind);
        let id = dto.id.clone();
        events.publish(published(dto));
        id
    }

    /// A console opens on the newest page of a session's events, however
    /// many it has: the page is the bound, and it ends on the last thing the
    /// session did.
    #[tokio::test]
    async fn opening_a_console_on_a_long_session_reads_a_page_of_the_newest_events() {
        let (store, _dir) = test_store().await;
        let session = session_in(&store).await;
        let recorded = 2_000;
        let mut committed = Vec::new();
        for _ in 0..recorded {
            committed.push(commit(&store, &session, "agent_message").await);
        }
        let (_stored_tx, stored_rx) = broadcast::channel(16);
        let (_live_tx, live_rx) = broadcast::channel(16);

        let (snapshot, _merge) = Merge::open(
            &store,
            EventBus::new(),
            stored_rx,
            live_rx,
            session,
            Vec::new(),
        )
        .await
        .unwrap();

        let ids: Vec<&str> = snapshot.iter().map(|event| event.id.as_str()).collect();
        let newest: Vec<&str> = committed
            .iter()
            .skip(recorded - SNAPSHOT_EVENTS as usize)
            .map(String::as_str)
            .collect();
        assert_eq!(
            ids.len(),
            SNAPSHOT_EVENTS as usize,
            "the snapshot is a page"
        );
        assert_eq!(ids, newest, "and the page is the newest events, in order");
    }

    /// A stored event reaches the bus only after the pump has loaded what it
    /// carries, so a live event taken after a run of commits can be here
    /// first. The merge asks the pump for what it holds before it sends a
    /// live event, so every commit below it goes out first, in order, and
    /// none of them twice.
    #[tokio::test]
    async fn a_live_event_waits_for_the_commits_the_pump_has_not_published_yet() {
        let (store, _dir) = test_store().await;
        let session = session_in(&store).await;
        let events = crate::bus::start(store.clone());
        let stored_rx = events.subscribe();
        let (live_tx, live_rx) = broadcast::channel(16);
        let mut merge = Merge::new(events, stored_rx, live_rx, session.clone(), None);

        let mut committed = Vec::new();
        for _ in 0..50 {
            committed.push(commit(&store, &session, "agent_message").await);
        }
        let chunk = live(&session, "agent_message_chunk");
        live_tx.send(chunk.clone()).unwrap();

        let mut order = Vec::new();
        for _ in 0..=committed.len() {
            order.push(merge.next().await.unwrap().unwrap().id);
        }

        let expected: Vec<String> = committed.iter().cloned().chain([chunk.id]).collect();
        assert_eq!(
            order, expected,
            "the live event goes out after the commits it overtook, each of them once"
        );
    }

    /// Following a turn reads nothing: the merge has no store to read, and
    /// the turn arrives whole on the two channels — its chunks, the call it
    /// made and the stop that ended it — with the store shut behind it.
    #[tokio::test]
    async fn a_turn_follows_with_the_store_shut() {
        let (store, _dir) = test_store().await;
        let session = session_in(&store).await;
        let events = EventBus::new();
        let stored_rx = events.subscribe();
        let (live_tx, live_rx) = broadcast::channel(16);
        let (snapshot, mut merge) = Merge::open(
            &store,
            events.clone(),
            stored_rx,
            live_rx,
            session.clone(),
            Vec::new(),
        )
        .await
        .unwrap();
        assert!(snapshot.is_empty(), "the session has done nothing yet");

        // Every query fails from here: a merge that read the store for a
        // chunk would say resync rather than give the turn.
        store.close().await;

        let text = live(&session, "agent_message_chunk");
        live_tx.send(text.clone()).unwrap();
        let call = publish(&events, &session, "pre_tool_use");
        let progress = live(&session, "tool_call_update");
        live_tx.send(progress.clone()).unwrap();
        let ended = publish(&events, &session, "post_tool_use");
        let more = live(&session, "agent_message_chunk");
        live_tx.send(more.clone()).unwrap();
        let whole = publish(&events, &session, "agent_message");
        let stop = publish(&events, &session, "stop");

        let mut order = Vec::new();
        for _ in 0..7 {
            order.push(merge.next().await.unwrap().unwrap().id);
        }

        assert_eq!(
            order,
            [text.id, call, progress.id, ended, more.id, whole, stop],
            "the turn goes out in the order the daemon gave its events ids"
        );
    }

    /// The bus drops an event whose payload it cannot load, so no drain will
    /// ever answer with it and nothing later brings it. The stream cannot
    /// give the session whole from there: it says resync, with the count of
    /// what was dropped, and the client opens again on a snapshot read from
    /// the store, which holds the event.
    #[tokio::test]
    async fn an_event_the_bus_dropped_is_a_resync_rather_than_a_hole() {
        let (store, _dir) = test_store().await;
        let session = session_in(&store).await;
        let events = EventBus::new();
        let stored_rx = events.subscribe();
        let (live_tx, live_rx) = broadcast::channel(16);
        let (_snapshot, mut merge) = Merge::open(
            &store,
            events.clone(),
            stored_rx,
            live_rx,
            session.clone(),
            Vec::new(),
        )
        .await
        .unwrap();

        // The pump's own accounting, for an event it could not load.
        events.count_dropped(&session);
        live_tx.send(live(&session, "agent_message_chunk")).unwrap();

        let resync = merge
            .next()
            .await
            .expect("a dropped event is a resync, not the end")
            .expect_err("the chunk cannot go out above an event that is missing");
        assert_eq!(resync.missed(), 1, "the event the bus dropped");
    }

    /// The drain is where the pump publishes the events below a live one, so
    /// it is where it drops one of them. The console reads the count after
    /// the drain answers and before the chunk goes out: a drop in that
    /// moment is a resync too, not a chunk over the hole it left.
    #[tokio::test]
    async fn an_event_dropped_while_the_console_waits_for_the_drain_is_a_resync_too() {
        let (store, _dir) = test_store().await;
        let session = session_in(&store).await;
        // The drain is answered here rather than by the pump, so that the
        // drop lands where the pump's own does: after the request, and
        // before the answer that says everything it held is published.
        let (events, mut requests) = EventBus::drained_by_hand();
        let stored_rx = events.subscribe();
        let (live_tx, live_rx) = broadcast::channel(16);
        let (_snapshot, mut merge) = Merge::open(
            &store,
            events.clone(),
            stored_rx,
            live_rx,
            session.clone(),
            Vec::new(),
        )
        .await
        .unwrap();
        live_tx.send(live(&session, "agent_message_chunk")).unwrap();

        // The channel is the order, not the runtime: this waits for the
        // console to ask, counts the drop, and only then answers. The answer
        // going home says the console was still inside the drain as the drop
        // was counted, since what receives it is the drain's own stack.
        let pump = tokio::spawn({
            let (events, session) = (events.clone(), session.clone());
            async move {
                let answer = requests.recv().await.expect("the console asks for a drain");
                events.count_dropped(&session);
                answer
                    .send(())
                    .expect("the console waits for the drain it asked for");
            }
        });

        let resync = merge
            .next()
            .await
            .expect("a dropped event is a resync, not the end")
            .expect_err("the chunk cannot go out above an event dropped under the drain");
        assert_eq!(resync.missed(), 1, "the event the bus dropped");
        pump.await.unwrap();
    }

    /// A console follows one session. An event the bus drops for another
    /// leaves this transcript whole, so the stream goes on: only a drop of
    /// its own session ends it, and the count it reports is its own.
    #[tokio::test]
    async fn a_drop_in_another_session_leaves_this_console_alone() {
        let (store, _dir) = test_store().await;
        let session = session_in(&store).await;
        let events = EventBus::new();
        let stored_rx = events.subscribe();
        let (live_tx, live_rx) = broadcast::channel(16);
        let (_snapshot, mut merge) = Merge::open(
            &store,
            events.clone(),
            stored_rx,
            live_rx,
            session.clone(),
            Vec::new(),
        )
        .await
        .unwrap();

        events.count_dropped("elsewhere");
        events.count_dropped("elsewhere");
        let chunk = live(&session, "agent_message_chunk");
        live_tx.send(chunk.clone()).unwrap();
        let sent = merge
            .next()
            .await
            .expect("another session's loss does not end this stream")
            .expect("nor does it hold this session's chunk back");
        assert_eq!(sent.id, chunk.id, "the chunk goes out as it would have");

        events.count_dropped(&session);
        live_tx.send(live(&session, "agent_message_chunk")).unwrap();
        let resync = merge
            .next()
            .await
            .expect("its own loss is a resync, not the end")
            .expect_err("the chunk cannot go out above an event that is missing");
        assert_eq!(
            resync.missed(),
            1,
            "the one event of this session, not the two of the other"
        );
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
            &store,
            EventBus::new(),
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
        let first = merge
            .next()
            .await
            .expect("the merge sends the next turn's chunk")
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

        let (snapshot, mut merge) = Merge::open(
            &store,
            EventBus::new(),
            stored_rx,
            live_rx,
            session,
            Vec::new(),
        )
        .await
        .unwrap();
        let ids: Vec<&str> = snapshot.iter().map(|event| event.id.as_str()).collect();
        assert_eq!(ids, [whole.as_str(), stop.as_str()]);
        let first = merge
            .next()
            .await
            .expect("the merge sends the live event above the snapshot")
            .unwrap();
        assert_eq!(first.id, after.id);
    }

    /// A console whose stored channel dropped events cannot go on: the
    /// transcript would have a hole in it. It is told how many it missed,
    /// and the stream ends on that.
    #[tokio::test]
    async fn a_console_that_fell_behind_on_the_stored_events_is_told_what_it_missed() {
        let (stored_tx, stored_rx) = broadcast::channel(1);
        let (_live_tx, live_rx) = broadcast::channel(16);
        let mut merge = Merge::new(EventBus::new(), stored_rx, live_rx, "session".into(), None);

        stored_tx.send(stored("0001", "agent_message")).unwrap();
        stored_tx.send(stored("0002", "stop")).unwrap();
        stored_tx
            .send(stored("0003", "user_prompt_submit"))
            .unwrap();

        let resync = merge
            .next()
            .await
            .expect("a lagged receiver is a resync, not the end")
            .expect_err("the events it missed cannot be given");
        assert_eq!(resync.missed(), 2, "the two events the channel dropped");
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

        let (snapshot, mut merge) = Merge::open(
            &store,
            EventBus::new(),
            stored_rx,
            live_rx,
            "session".into(),
            Vec::new(),
        )
        .await
        .unwrap();
        assert!(
            snapshot.is_empty(),
            "nothing is stored, so the chunk the channel still holds is above the snapshot"
        );
        let resync = merge
            .next()
            .await
            .expect("the lag is a resync, not the end")
            .expect_err("the chunks it missed cannot be given");
        assert_eq!(
            resync.missed(),
            2,
            "the two dropped chunks are a resync before the chunk that is left"
        );
    }

    /// `GET /console` has no stream to be told resync on. A snapshot opened
    /// while the live channel lagged is opened again, and one whose channel
    /// lags every time is refused rather than answered short.
    #[tokio::test]
    async fn the_snapshot_get_answers_is_opened_again_under_a_lag_and_refused_when_it_keeps_lagging()
     {
        let opens = std::cell::Cell::new(0usize);
        // An open whose live channel lagged the first `until` times.
        let lagging_until = |until: usize| {
            let opens = &opens;
            move || {
                opens.set(opens.get() + 1);
                let lagged = opens.get() <= until;
                async move {
                    let (_stored_tx, stored_rx) = broadcast::channel(16);
                    let (_live_tx, live_rx) = broadcast::channel(16);
                    let mut merge =
                        Merge::new(EventBus::new(), stored_rx, live_rx, "session".into(), None);
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
