//! A session's trajectory over HTTP (040): the snapshot, its stream, the
//! pages of older turns and of a turn's steps, the detail of one call, and
//! the bytes of one content block.
//!
//! Every subscriber of a session shares one [`Hub`]: one console merge
//! (`super::console::Merge`) is its one ordered input, one [`Fold`] turns it
//! into ops, and one broadcast hands each revision to every subscriber. So
//! two subscribers receive the same `rev` sequence, and a subscriber that
//! falls behind the broadcast is told to resync.

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::{Arc, Mutex, Weak};

use axum::extract::{Path, Query, State};
use axum::response::sse::{Event, Sse};
use futures_util::Stream;
use futures_util::stream::{StreamExt, once, unfold};
use tokio::sync::{Notify, broadcast, mpsc};

use ariadne_api::events::AgentEventDto;
use ariadne_api::stream::{DomainEvent, ResyncDto};
use ariadne_api::trajectory::{
    MAIN_LANE, ToolCallDetailDto, TrajectoryItemQuery, TrajectoryOpDto, TrajectoryQuery,
    TrajectorySnapshotDto, TrajectoryStepsDto, TrajectoryStepsQuery, TrajectoryStreamQuery,
    TrajectoryUpdateDto,
};
use ariadne_store::StoreError;

use super::AppState;
use super::console::Merge;
use super::convert::session_dto_of;
use super::error::{ApiError, ApiResult, Json};
use super::sse;
use crate::trajectory::detail::{blob_of, call_events, detail_kinds, detail_of, opens_a_step};
use crate::trajectory::read::{READ_EVENTS, Read, read_page, read_steps, read_turns};
use crate::trajectory::{Fold, session_state, step_id};

/// How many turns a snapshot carries when the query names no `limit`: a
/// screen and a half of closed turns, each a line, above the running one.
const DEFAULT_TURNS: usize = 20;
/// The most turns one answer carries.
const MAX_TURNS: usize = 50;
/// How many turns a hub keeps while it follows a session. Twice the most a
/// snapshot carries, so a long day does not grow the hub without end; the
/// turns that hold attention stay beside them.
const KEPT_TURNS: usize = 2 * MAX_TURNS;
/// The default and the most steps a page of the steps route carries.
const DEFAULT_STEPS: usize = 50;
const MAX_STEPS: usize = 200;
/// How many revisions a subscriber may fall behind before it is told to
/// resync. A turn's chunks are a revision each, and a busy turn streams
/// hundreds a second.
const CAPACITY: usize = 1024;
/// How many times an open is tried again while the live channel lags under
/// it, as `GET /console` does.
const OPEN_TRIES: usize = 3;

/// The place of one session's hub: locked while the hub opens, so two
/// subscribers that come at once open it once.
type Slot = Arc<tokio::sync::Mutex<Weak<Hub>>>;

/// Every session's hub, by session id.
#[derive(Clone, Default)]
pub struct Trajectories {
    slots: Arc<Mutex<HashMap<String, Slot>>>,
}

struct Hub {
    /// An async lock: one event, its apply and the refill it calls for, is
    /// one revision under it, and a subscriber that joins meanwhile waits for
    /// the whole revision (040, rule 16).
    state: tokio::sync::Mutex<HubState>,
    frames: broadcast::Sender<Frame>,
    /// A subscriber left: the hub ends once nobody follows it.
    left: Notify,
}

struct HubState {
    fold: Fold,
    rev: u64,
    ended: bool,
    /// The attention rows the hub loaded to fill its window: each is loaded
    /// once, so a row the fold cannot fill costs one read and no more.
    filled: std::collections::HashSet<String>,
}

#[derive(Clone, Debug)]
enum Frame {
    Update(Arc<TrajectoryUpdateDto>),
    /// The hub cannot give the session whole from here.
    Resync(u64),
}

/// One subscriber: the snapshot it opened on, and the revisions after it.
pub(super) struct Subscription {
    hub: Arc<Hub>,
    snapshot: Option<TrajectorySnapshotDto>,
    frames: Option<broadcast::Receiver<Frame>>,
}

impl Drop for Subscription {
    fn drop(&mut self) {
        self.frames.take();
        self.hub.left.notify_one();
    }
}

impl Subscription {
    /// The snapshot the subscription opened on. Taken once.
    pub(super) fn opening(&mut self) -> Option<TrajectorySnapshotDto> {
        self.snapshot.take()
    }

    /// The next revision. `Err` carries how many a receiver that fell behind
    /// missed, and `None` is the hub's end.
    pub(super) async fn revision(&mut self) -> Option<Result<Arc<TrajectoryUpdateDto>, u64>> {
        match self.next().await? {
            Frame::Update(update) => Some(Ok(update)),
            Frame::Resync(missed) => Some(Err(missed)),
        }
    }

    /// The next frame. A receiver that fell behind the broadcast is a
    /// resync, and `None` is the hub's end.
    async fn next(&mut self) -> Option<Frame> {
        next_frame(self.frames.as_mut()?).await
    }
}

/// The next frame of a receiver: what was sent, or a resync where the
/// receiver fell behind and lost frames.
async fn next_frame(frames: &mut broadcast::Receiver<Frame>) -> Option<Frame> {
    match frames.recv().await {
        Ok(frame) => Some(frame),
        Err(broadcast::error::RecvError::Lagged(missed)) => Some(Frame::Resync(missed)),
        Err(broadcast::error::RecvError::Closed) => None,
    }
}

/// What the hub's task reads.
enum Input {
    /// An event, and whether the store holds it.
    Event(Box<AgentEventDto>, bool),
    Resync(u64),
}

impl Trajectories {
    /// Join the session's hub, or open one.
    pub(super) async fn subscribe(
        &self,
        state: &AppState,
        session_id: &str,
        limit: usize,
    ) -> ApiResult<Subscription> {
        let slot = self.slot(session_id);
        let mut hub = slot.lock().await;
        if let Some(kept) = hub.upgrade()
            && let Some(sub) = kept.join(limit).await
        {
            return Ok(sub);
        }
        let opened = Hub::open(state, session_id).await?;
        *hub = Arc::downgrade(&opened);
        opened.join(limit).await.ok_or_else(|| {
            ApiError::new(
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                "trajectory_busy",
                "the session streamed faster than its trajectory could be read; ask again",
            )
        })
    }
}

impl Trajectories {
    /// The slot of a session's hub. A slot stays while a subscriber holds
    /// it, so two subscribers that come at once to a session with no hub
    /// lock one slot and open one hub; a slot nobody holds and whose hub
    /// ended goes.
    fn slot(&self, session_id: &str) -> Slot {
        let mut slots = self.slots.lock().expect("trajectory slots");
        slots.retain(|_, slot| {
            Arc::strong_count(slot) > 1
                || slot.try_lock().map_or(true, |hub| hub.strong_count() > 0)
        });
        slots.entry(session_id.to_string()).or_default().clone()
    }
}

impl Hub {
    /// Open a session's hub: the bounded read, the fold over it, and the
    /// task that follows the merge.
    async fn open(state: &AppState, session_id: &str) -> ApiResult<Arc<Self>> {
        let mut tries = 0;
        loop {
            tries += 1;
            // Subscribed before the row is read, so a change of the row in
            // between is followed.
            let session_rx = state.events.subscribe();
            let row =
                session_dto_of(&state.store, state.store.get_session(session_id).await?).await?;
            let stored = state.events.subscribe();
            let (live, so_far) = state.launcher.acp.subscribe_console(session_id).await;
            let store = state.store.clone();
            let id = session_id.to_string();
            let (read, so_far, merge) = Merge::open_on(
                state.events.clone(),
                stored,
                live,
                session_id.to_string(),
                so_far,
                || async move {
                    let read = read_turns(&store, &id, None, MAX_TURNS, None, READ_EVENTS).await?;
                    let last_id = read.upper.clone();
                    Ok((read, last_id))
                },
            )
            .await?;
            if merge.lagged() && tries < OPEN_TRIES {
                continue;
            }
            let mut fold = Fold::new(session_id, session_state(&row));
            read.fold_into(&mut fold, so_far);
            let (frames, _) = broadcast::channel(CAPACITY);
            let hub = Arc::new(Self {
                state: tokio::sync::Mutex::new(HubState {
                    fold,
                    rev: 0,
                    ended: false,
                    filled: Default::default(),
                }),
                frames,
                left: Notify::new(),
            });
            hub.clone()
                .follow(state.clone(), session_id.to_string(), merge, session_rx);
            return Ok(hub);
        }
    }

    /// A subscriber: the snapshot at the current revision, and a receiver
    /// of every revision after it. None for a hub that has ended.
    async fn join(self: &Arc<Self>, limit: usize) -> Option<Subscription> {
        let state = self.state.lock().await;
        if state.ended {
            return None;
        }
        Some(Subscription {
            hub: self.clone(),
            snapshot: Some(state.fold.snapshot(state.rev, limit, None)),
            frames: Some(self.frames.subscribe()),
        })
    }

    /// Send `ops` as the next revision, under the lock the caller holds.
    fn publish(&self, state: &mut HubState, ops: Vec<TrajectoryOpDto>) {
        if state.ended || ops.is_empty() {
            return;
        }
        state.fold.prune(KEPT_TURNS);
        state.rev += 1;
        let update = TrajectoryUpdateDto {
            rev: state.rev,
            ops,
        };
        let _ = self.frames.send(Frame::Update(Arc::new(update)));
    }

    /// Apply a change to the fold, and send its ops as the next revision.
    async fn apply(&self, change: impl FnOnce(&mut Fold) -> Vec<TrajectoryOpDto>) {
        let mut state = self.state.lock().await;
        if state.ended {
            return;
        }
        let ops = change(&mut state.fold);
        self.publish(&mut state, ops);
    }

    /// Fold one event, fill the attention window it opened, and send both
    /// as one revision. A load that fails ends the hub with a resync: the
    /// window cannot be given whole from here.
    async fn event(&self, store: &ariadne_store::Store, event: &AgentEventDto, stored: bool) {
        let mut state = self.state.lock().await;
        if state.ended {
            return;
        }
        let mut ops = state.fold.apply(event, stored);
        match refill(&mut state, |rows| async move {
            store.events_by_ids(&rows).await
        })
        .await
        {
            Ok(filled) => ops.extend(filled),
            Err(missed) => {
                state.ended = true;
                let _ = self.frames.send(Frame::Resync(missed));
                return;
            }
        }
        self.publish(&mut state, ops);
    }

    async fn end(&self, frame: Option<Frame>) {
        let mut state = self.state.lock().await;
        state.ended = true;
        if let Some(frame) = frame {
            let _ = self.frames.send(frame);
        }
    }

    /// End the hub if nobody follows it. A subscriber joins under the same
    /// lock, so it either joined before this or finds the hub ended.
    async fn end_if_unwatched(&self) -> bool {
        let mut state = self.state.lock().await;
        if self.frames.receiver_count() == 0 {
            state.ended = true;
        }
        state.ended
    }

    /// Follow the session until the merge ends or nobody listens. The merge
    /// runs in a task of its own, because its `next` must not be cancelled
    /// halfway.
    fn follow(
        self: Arc<Self>,
        state: AppState,
        session_id: String,
        mut merge: Merge,
        mut session_rx: broadcast::Receiver<crate::bus::BusEvent>,
    ) {
        let (input, mut inputs) = mpsc::channel(CAPACITY);
        let pump = tokio::spawn(async move {
            loop {
                let next = match merge.next_marked().await {
                    Some(Ok((event, stored))) => Input::Event(Box::new(event), stored),
                    Some(Err(resync)) => Input::Resync(resync.missed()),
                    None => return,
                };
                let last = matches!(next, Input::Resync(_));
                if input.send(next).await.is_err() || last {
                    return;
                }
            }
        });
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    next = inputs.recv() => match next {
                        Some(Input::Event(event, stored)) => {
                            self.event(&state.store, &event, stored).await;
                        }
                        Some(Input::Resync(missed)) => {
                            self.end(Some(Frame::Resync(missed))).await;
                            break;
                        }
                        None => {
                            self.end(None).await;
                            break;
                        }
                    },
                    bus = session_rx.recv() => match bus {
                        Ok(event) => {
                            if let DomainEvent::SessionUpdated(session) = &event.event
                                && session.id == session_id
                            {
                                self.apply(|fold| fold.patch_session(session).into_iter().collect())
                                    .await;
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(_)) => {
                            if let Ok(row) = state.store.get_session(&session_id).await
                                && let Ok(session) = session_dto_of(&state.store, row).await
                            {
                                self.apply(|fold| fold.patch_session(&session).into_iter().collect())
                                    .await;
                            }
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            self.end(None).await;
                            break;
                        }
                    },
                    _ = self.left.notified() => {
                        if self.end_if_unwatched().await {
                            break;
                        }
                    }
                }
            }
            pump.abort();
        });
    }
}

/// The ops that fill a hub's attention window: the rows the fold counts
/// and does not hold, loaded by `load` and filled in. A row counts as filled
/// once its load succeeded, so a failed load leaves every row to load again;
/// the failure is the count of rows that could not be given.
async fn refill<L, Fut>(state: &mut HubState, load: L) -> Result<Vec<TrajectoryOpDto>, u64>
where
    L: FnOnce(Vec<String>) -> Fut,
    Fut: std::future::Future<Output = Result<Vec<ariadne_store::AgentEvent>, StoreError>>,
{
    let missing: Vec<String> = state
        .fold
        .missing_attention()
        .into_iter()
        .filter(|row| !state.filled.contains(row))
        .take(READ_EVENTS)
        .collect();
    if missing.is_empty() {
        return Ok(Vec::new());
    }
    let events = load(missing.clone())
        .await
        .map_err(|_| missing.len() as u64)?;
    state.filled.extend(missing);
    Ok(events
        .into_iter()
        .flat_map(|event| state.fold.fill(&super::convert::event_dto(event)))
        .collect())
}

fn turn_not_found(turn_id: &str) -> ApiError {
    ApiError::new(
        axum::http::StatusCode::NOT_FOUND,
        "not_found",
        format!("turn {turn_id} not found"),
    )
}

fn turns_of(limit: Option<u32>) -> usize {
    limit.map_or(DEFAULT_TURNS, |limit| (limit as usize).clamp(1, MAX_TURNS))
}

/// A fold over a read of the store alone, for the pages: what a page
/// answers is stored, and nothing live joins it.
async fn fold_of(state: &AppState, session_id: &str, read: Read) -> Result<Fold, StoreError> {
    let row = session_dto_of(&state.store, state.store.get_session(session_id).await?).await?;
    let mut fold = Fold::new(session_id, session_state(&row));
    read.fold_into(&mut fold, Vec::new());
    Ok(fold)
}

/// The session's trajectory: the newest turns, oldest first, the running
/// one with its last steps. With `before`, a page of the whole turns older
/// than that turn, at `rev` 0: a page is read from the store and is no
/// revision of the stream. With `subagent`, each turn carries that
/// subagent's steps.
#[utoipa::path(get, path = "/v1/sessions/{id}/trajectory", tag = "sessions",
    operation_id = "trajectory_snapshot",
    params(("id" = String, Path, description = "session id"), TrajectoryQuery),
    responses((status = 200, body = TrajectorySnapshotDto), (status = 404),
        (status = 503, description = "the session streamed faster than its trajectory could \
                                      be read, every time it was tried; ask again")))]
pub(super) async fn snapshot(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<TrajectoryQuery>,
) -> ApiResult<Json<TrajectorySnapshotDto>> {
    state.store.get_session(&id).await?;
    let limit = turns_of(query.limit);
    let subagent = query.subagent.as_deref();
    let read = match (query.before.as_deref(), subagent) {
        (None, None) => {
            let mut sub = state.trajectories.subscribe(&state, &id, limit).await?;
            return Ok(Json(
                sub.snapshot
                    .take()
                    .expect("a subscription opens on a snapshot"),
            ));
        }
        (Some(before), _) => {
            read_page(&state.store, &id, before, limit, subagent, READ_EVENTS).await?
        }
        (None, Some(_)) => {
            read_turns(&state.store, &id, None, limit, subagent, READ_EVENTS).await?
        }
    };
    let fold = fold_of(&state, &id, read).await?;
    Ok(Json(fold.snapshot(0, limit, subagent)))
}

/// Follow a session's trajectory.
///
/// Opens with a `snapshot` (`TrajectorySnapshotDto`), then an `update` per
/// revision (`TrajectoryUpdateDto`), each `rev` one more than the one
/// before. Streamed text arrives as `text_append` ops. A client that falls
/// behind gets a `resync` (`ResyncDto`) and the connection closes; a
/// reconnect opens on a new snapshot.
#[utoipa::path(get, path = "/v1/sessions/{id}/trajectory/stream", tag = "sessions",
    operation_id = "trajectory_stream",
    params(("id" = String, Path, description = "session id"), TrajectoryStreamQuery),
    responses((status = 200,
        description = "SSE stream of the trajectory (text/event-stream): a `snapshot` event \
                       (TrajectorySnapshotDto), then an `update` event per revision \
                       (TrajectoryUpdateDto). A client that falls behind gets a `resync` event \
                       (ResyncDto) and the connection closes.",
        content_type = "text/event-stream", body = TrajectoryUpdateDto),
        (status = 404)))]
pub(super) async fn stream(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<TrajectoryStreamQuery>,
) -> ApiResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    state.store.get_session(&id).await?;
    let mut sub = state
        .trajectories
        .subscribe(&state, &id, turns_of(query.limit))
        .await?;
    let opening = sub
        .snapshot
        .take()
        .expect("a subscription opens on a snapshot");
    let opening = once(async move { Ok(sse::json_event("snapshot", opening)) });
    let updates = unfold((sub, false), |(mut sub, closing)| async move {
        if closing {
            return None;
        }
        match sub.next().await? {
            Frame::Update(update) => Some((Ok(sse::json_event("update", &*update)), (sub, false))),
            Frame::Resync(missed) => Some((
                Ok(sse::identified_event("resync", ResyncDto { missed })),
                (sub, true),
            )),
        }
    });
    Ok(sse::respond(opening.chain(updates)))
}

/// One page of the steps of a turn, oldest first: the `limit` steps before
/// the step `before`, or the turn's last ones, and how many come before
/// them. With `subagent`, that subagent's steps in the turn.
#[utoipa::path(get, path = "/v1/sessions/{id}/trajectory/turns/{turn_id}/steps", tag = "sessions",
    operation_id = "trajectory_steps",
    params(("id" = String, Path, description = "session id"),
        ("turn_id" = String, Path, description = "turn id"), TrajectoryStepsQuery),
    responses((status = 200, body = TrajectoryStepsDto), (status = 404)))]
pub(super) async fn steps(
    State(state): State<AppState>,
    Path((id, turn_id)): Path<(String, String)>,
    Query(query): Query<TrajectoryStepsQuery>,
) -> ApiResult<Json<TrajectoryStepsDto>> {
    state.store.get_session(&id).await?;
    let limit = query
        .limit
        .map_or(DEFAULT_STEPS, |limit| (limit as usize).clamp(1, MAX_STEPS));
    let lane = query.subagent.as_deref().unwrap_or(MAIN_LANE);
    let read = read_steps(
        &state.store,
        &id,
        &turn_id,
        lane,
        query.before.as_deref(),
        limit,
        READ_EVENTS,
    )
    .await?
    .ok_or_else(|| turn_not_found(&turn_id))?;
    let page = read.page;
    let earlier = read.earlier;
    let fold = fold_of(&state, &id, read.read).await?;
    let steps = fold
        .held_steps(&turn_id, lane)
        .into_iter()
        .filter(|step| page.iter().any(|id| id == step_id(step)))
        .collect();
    Ok(Json(TrajectoryStepsDto {
        steps,
        earlier_steps: earlier,
    }))
}

/// The full detail of one `tool_call` step, by its kind: the command and
/// its output, the change of each file, the text read, a search's or a
/// fetch's result, or the content blocks of any other call. A running call
/// shows its latest stored progress. With `raw=true`, the raw input and
/// output as well.
#[utoipa::path(get, path = "/v1/sessions/{id}/trajectory/items/{item_id}", tag = "sessions",
    operation_id = "trajectory_item",
    params(("id" = String, Path, description = "session id"),
        ("item_id" = String, Path, description = "the id of a `tool_call` step"),
        TrajectoryItemQuery),
    responses((status = 200, body = ToolCallDetailDto), (status = 404)))]
pub(super) async fn item(
    State(state): State<AppState>,
    Path((id, item_id)): Path<(String, String)>,
    Query(query): Query<TrajectoryItemQuery>,
) -> ApiResult<Json<ToolCallDetailDto>> {
    state.store.get_session(&id).await?;
    let not_found = || {
        ApiError::new(
            axum::http::StatusCode::NOT_FOUND,
            "not_found",
            format!("item {item_id} not found"),
        )
    };
    let upper = state.store.newest_session_event_id(&id, None).await?;
    let Some(upper) = upper else {
        return Err(not_found());
    };
    // The turn of the item opens on the newest prompt at or before it, so
    // the fold that tells whether it opens a step starts there.
    let prompts = state
        .store
        .session_event_heads(&id, None, Some(&item_id), &["user_prompt_submit"])
        .await?;
    let from = prompts.last().map(|prompt| prompt.id.clone());
    let heads = state
        .store
        .session_event_heads(&id, from.as_deref(), Some(&upper), &detail_kinds())
        .await?;
    let at = heads
        .iter()
        .position(|head| head.id == item_id)
        .ok_or_else(not_found)?;
    let row = session_dto_of(&state.store, state.store.get_session(&id).await?).await?;
    if !opens_a_step(Fold::new(&id, session_state(&row)), &heads[..=at], &item_id) {
        return Err(not_found());
    }
    let call = call_events(&heads[at..], &item_id, READ_EVENTS).ok_or_else(not_found)?;
    let events: Vec<AgentEventDto> = state
        .store
        .events_by_ids(&call.load)
        .await?
        .into_iter()
        .map(super::convert::event_dto)
        .collect();
    detail_of(&id, &events, &call, query.raw)
        .map(Json)
        .ok_or_else(not_found)
}

/// The bytes of one image, audio or resource block, with its MIME type: the
/// `blob` URL of a content block names it by its stored event and its place.
#[utoipa::path(get, path = "/v1/sessions/{id}/trajectory/blobs/{event_id}/{index}",
    tag = "sessions", operation_id = "trajectory_blob",
    params(("id" = String, Path, description = "session id"),
        ("event_id" = String, Path, description = "the stored event that holds the block"),
        ("index" = u32, Path, description = "the place of the block in the event")),
    responses((status = 200, description = "the bytes of the block, with its MIME type",
        content_type = "application/octet-stream", body = Vec<u8>), (status = 404)))]
pub(super) async fn blob(
    State(state): State<AppState>,
    Path((id, event_id, index)): Path<(String, String, usize)>,
) -> ApiResult<axum::response::Response> {
    use axum::response::IntoResponse;
    state.store.get_session(&id).await?;
    let found = state
        .store
        .events_by_ids(std::slice::from_ref(&event_id))
        .await?
        .into_iter()
        .find(|event| event.session_id.as_deref() == Some(id.as_str()))
        .and_then(|event| blob_of(&super::convert::event_dto(event), index));
    let Some((mime, bytes)) = found else {
        return Err(ApiError::new(
            axum::http::StatusCode::NOT_FOUND,
            "not_found",
            format!("blob {event_id}/{index} not found"),
        ));
    };
    let mut response = bytes.into_response();
    if let Ok(value) = mime.parse() {
        response
            .headers_mut()
            .insert(axum::http::header::CONTENT_TYPE, value);
    }
    Ok(response)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use ariadne_api::trajectory::TrajectoryUpdateDto;
    use tokio::sync::broadcast;

    use super::{Frame, HubState, Trajectories, next_frame, refill};
    use crate::trajectory::Fold;

    /// A hub's fold that counts one waiting question it does not hold: what
    /// a cold read past the waiting limit leaves to fill.
    fn hub_lacking_one_question() -> HubState {
        let state = ariadne_api::trajectory::SessionStateDto {
            status: ariadne_core::SessionStatus::Running,
            attention_reason: None,
            running_turn_id: None,
            title: None,
            mode: None,
            config_options: Vec::new(),
            context: None,
            usage: Default::default(),
            cost: None,
            commands: Vec::new(),
        };
        let mut fold = Fold::new("session", state);
        let head = |id: &str, kind: &str, facts: &str| ariadne_store::EventHead {
            id: id.into(),
            kind: kind.into(),
            lane: None,
            facts: facts.into(),
            created_at: "2026-11-28T00:00:00Z".into(),
        };
        fold.apply_head(&head("0001", "user_prompt_submit", "{}"));
        fold.apply_head(&head("0002", "question", r#"{"question":"q-1"}"#));
        HubState {
            fold,
            rev: 0,
            ended: false,
            filled: Default::default(),
        }
    }

    /// A refill whose load fails gives its rows back to load again, and
    /// says how many it could not give; a later load that succeeds fills
    /// them (040, rule 16).
    #[tokio::test]
    async fn a_failed_refill_leaves_its_rows_to_load_again() {
        let mut state = hub_lacking_one_question();

        let failed = refill(&mut state, |_| async {
            Err(ariadne_store::StoreError::Invalid(
                "the store is gone".into(),
            ))
        })
        .await;

        assert_eq!(failed, Err(1));
        assert!(state.filled.is_empty(), "nothing counts as filled");
        let mut asked = Vec::new();
        let _ = refill(&mut state, |rows| {
            asked = rows;
            async { Ok(Vec::new()) }
        })
        .await;
        assert_eq!(asked, ["0002"], "the next refill asks for the row again");
    }

    /// Two subscribers that come at once to a session with no hub get one
    /// slot, so the first to lock it opens the hub and the second joins it.
    #[tokio::test]
    async fn two_cold_subscribers_share_one_slot() {
        let trajectories = Trajectories::default();

        let first = trajectories.slot("session");
        let second = trajectories.slot("session");

        assert!(Arc::ptr_eq(&first, &second));
        let opening = first.lock().await;
        assert!(
            second.try_lock().is_err(),
            "the second waits on the first's open"
        );
        drop(opening);
    }

    /// A subscriber that falls more revisions behind than the broadcast
    /// holds is told to resync, with the count of what it lost.
    #[tokio::test]
    async fn a_subscriber_that_lags_receives_resync() {
        let (frames, mut behind) = broadcast::channel(2);
        for rev in 1..=5 {
            frames
                .send(Frame::Update(Arc::new(TrajectoryUpdateDto {
                    rev,
                    ops: Vec::new(),
                })))
                .unwrap();
        }

        let frame = next_frame(&mut behind).await.unwrap();

        assert!(
            matches!(frame, Frame::Resync(3)),
            "the three revisions it lost are a resync, got {frame:?}"
        );
    }
}
