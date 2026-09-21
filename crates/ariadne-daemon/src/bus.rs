//! Domain-event bus.
//!
//! The pump sits behind the store's own [`Change`] hook rather than beside the
//! writers, which is why the HTTP handlers, the scheduler and the launcher all
//! feed it without knowing it exists — and why no write can reach the database
//! without reaching a client.
//!
//! Each event carries the whole DTO rather than an id to refetch — but for an
//! agent event, whose payload reaches 1 MB, the stream carries a summary of
//! it — and delivery
//! is history-free: a subscriber that cannot keep up is lagged by the
//! broadcast channel, and the stream tells it to resync over REST rather than
//! leaving it quietly stale (see `http::stream`).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::{broadcast, mpsc, oneshot};
use tracing::{debug, warn};

use ariadne_api::events::{AgentEventDto, AgentEventSummaryDto};
use ariadne_api::goals::GoalDto;
use ariadne_api::sessions::SessionDto;
use ariadne_api::stream::{DeletedDto, DomainEvent, TaskUpdatedDto};
use ariadne_store::{AgentSession, Change, Goal, Result, Store, Task};

use crate::http::convert::{
    event_dto, goal_dto_of, message_dto, repository_dto, session_dto_of, skill_dto, task_dto_of,
    transition_dto,
};

/// Events buffered per subscriber before it is considered too slow.
const CAPACITY: usize = 1024;

/// A domain event plus the keys the stream filters match on. The routing keys
/// live in the envelope rather than being read back out of the payload, so
/// filtering works for DTOs that do not carry them (a review has no goal id).
#[derive(Debug, Clone)]
pub struct BusEvent {
    pub event: DomainEvent,
    pub goal_id: Option<String>,
    pub task_id: Option<String>,
    /// The whole agent event, payload included, where `event` is an
    /// [`DomainEvent::AgentEvent`]. The console alone reads it: the domain
    /// stream sends `event` and never this. Shared, so a subscriber's copy
    /// costs a count and not the payload.
    pub recorded: Option<Arc<AgentEventDto>>,
}

impl BusEvent {
    /// Does this event pass the `goal`/`task` stream filters? An event with no
    /// such association (profiles, repositories) is filtered out by either filter.
    pub(crate) fn matches(&self, goal: Option<&str>, task: Option<&str>) -> bool {
        goal.is_none_or(|g| self.goal_id.as_deref() == Some(g))
            && task.is_none_or(|t| self.task_id.as_deref() == Some(t))
    }
}

/// Asking the pump to publish what it holds: answered once it has.
type Drain = oneshot::Sender<()>;

/// Fan-out handle held by [`AppState`](crate::http::AppState).
#[derive(Clone)]
pub struct EventBus {
    tx: broadcast::Sender<BusEvent>,
    /// Where a drain request reaches the pump. A bus with no pump behind it
    /// — the one a test makes — drops the receiving end where it is made, so
    /// a request finds nobody and the answer is immediate.
    drains: mpsc::UnboundedSender<Drain>,
    /// How many agent events the pump has dropped, per session and in all,
    /// shared by every clone.
    dropped: Dropped,
}

/// What the pump could not publish, counted for the consoles that lost it.
///
/// A session of its own for each count, because a console follows one
/// session: a hole in another's transcript is none of its business, and
/// ending its stream for one would cost it the turn it is reading. The total
/// is there so that a console can tell that nothing at all was dropped
/// without taking the lock, which is the ordinary case — a drop needs the
/// store to fail, so the map is empty in a run that goes well.
#[derive(Clone, Default)]
struct Dropped {
    total: Arc<AtomicU64>,
    per_session: Arc<Mutex<HashMap<String, u64>>>,
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl EventBus {
    pub(crate) fn new() -> Self {
        Self::with_capacity(CAPACITY)
    }

    /// A bus buffering `capacity` events per subscriber. Tests use a small
    /// one to exercise the lag path without publishing a thousand events.
    pub fn with_capacity(capacity: usize) -> Self {
        Self::behind_pump(capacity, mpsc::unbounded_channel().0)
    }

    fn behind_pump(capacity: usize, drains: mpsc::UnboundedSender<Drain>) -> Self {
        Self {
            tx: broadcast::Sender::new(capacity),
            drains,
            dropped: Dropped::default(),
        }
    }

    /// A bus whose drain requests a test answers itself, with the receiver
    /// the pump would hold. What a test stands in for the pump with where
    /// the order of the two matters: it sees the request, does what the pump
    /// would do, and answers when it chooses.
    #[cfg(test)]
    pub(crate) fn drained_by_hand() -> (Self, mpsc::UnboundedReceiver<oneshot::Sender<()>>) {
        let (drains, requests) = mpsc::unbounded_channel();
        (Self::behind_pump(CAPACITY, drains), requests)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<BusEvent> {
        self.tx.subscribe()
    }

    /// Broadcast an event; dropped when nobody is listening.
    pub fn publish(&self, event: BusEvent) {
        let _ = self.tx.send(event);
    }

    /// Answers once the pump has published every change it had been handed
    /// when this was called.
    ///
    /// A write reaches the bus late: the store commits it and hands it over,
    /// and the pump publishes it once it has loaded what the event carries.
    /// In between, the write is committed and on no stream — which is what a
    /// console's live events overtake ([`crate::http::console`]). The pump
    /// takes its changes and these requests on the one task, and publishes
    /// every change it holds before it answers one, so an answer says that
    /// whatever was committed before the question is on the bus.
    ///
    /// A bus with no pump behind it answers at once: nothing fattens for it,
    /// so nothing of it is ever pending.
    pub(crate) async fn drained(&self) {
        let (answer, answered) = oneshot::channel();
        if self.drains.send(answer).is_ok() {
            let _ = answered.await;
        }
    }

    /// How many of one session's agent events the pump has dropped.
    ///
    /// An event whose payload the pump cannot load reaches no stream, and
    /// nothing publishes it later: it is in the store and nowhere else. A
    /// console of that session reads this count as it opens and again after
    /// each drain — one that has grown is a hole it cannot fill from the
    /// channels, so it tells the client to start again from a snapshot,
    /// which is read from the store and holds the event
    /// ([`crate::http::console`]). A console of any other session reads its
    /// own count, which the drop left where it was.
    pub(crate) fn dropped(&self, session_id: &str) -> u64 {
        // The lock is taken only where something was dropped at all, so an
        // ordinary console asks the atomic and no more.
        if self.dropped.total.load(Ordering::Relaxed) == 0 {
            return 0;
        }
        self.sessions().get(session_id).copied().unwrap_or(0)
    }

    /// Count one agent event of a session that the pump dropped: its own
    /// accounting, and what a test stands in for the pump with.
    pub(crate) fn count_dropped(&self, session_id: &str) {
        *self.sessions().entry(session_id.to_string()).or_default() += 1;
        // Written last: a console that reads a total above zero finds the
        // session's count already there.
        self.dropped.total.fetch_add(1, Ordering::Relaxed);
    }

    /// The counts, past a lock another thread may have poisoned: a panic
    /// while counting a drop is no reason to lose the counts themselves.
    fn sessions(&self) -> std::sync::MutexGuard<'_, HashMap<String, u64>> {
        self.dropped
            .per_session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Install the store change hook and start the pump. Call once at startup,
/// before anything writes.
pub fn start(store: Store) -> EventBus {
    let (drains, requests) = mpsc::unbounded_channel();
    let bus = EventBus::behind_pump(CAPACITY, drains);
    match store.watch_changes() {
        Some(rx) => {
            tokio::spawn(pump(store, rx, requests, bus.clone()));
        }
        None => warn!("store change hook already installed; no domain events will be published"),
    }
    bus
}

/// Fatten changes into domain events, in commit order, and answer the drain
/// requests of whoever waits for that order ([`EventBus::drained`]).
async fn pump(
    store: Store,
    mut rx: mpsc::UnboundedReceiver<Change>,
    mut requests: mpsc::UnboundedReceiver<Drain>,
    bus: EventBus,
) {
    loop {
        tokio::select! {
            // A request first: it publishes what it drains, so the changes
            // lose nothing by being answered second.
            biased;
            request = requests.recv() => {
                // Every change handed over before the request was made is in
                // the queue by now, so what the queue holds is what the
                // answer promises.
                while let Ok(change) = rx.try_recv() {
                    publish(&store, change, &bus).await;
                }
                match request {
                    Some(answer) => { let _ = answer.send(()); }
                    None => break,
                }
            }
            change = rx.recv() => match change {
                Some(change) => publish(&store, change, &bus).await,
                None => break,
            },
        }
    }
}

/// One change, fattened and published. A payload that cannot be loaded costs
/// the event and nothing else.
async fn publish(store: &Store, change: Change, bus: &EventBus) {
    // An agent event is a session's transcript, and a console follows it on
    // this bus alone: one dropped here leaves a hole no later event fills,
    // so it is counted under the session it belongs to, for that session's
    // consoles to read ([`EventBus::dropped`]). An event of no session is
    // on no console.
    let of_session = match &change {
        Change::AgentEventCreated(event) => event.session_id.clone(),
        _ => None,
    };
    match fatten(store, change).await {
        Ok(event) => {
            debug!(kind = event.event.kind(), "publishing domain event");
            bus.publish(event);
        }
        Err(e) => {
            if let Some(session) = of_session {
                bus.count_dropped(&session);
            }
            warn!(error = %e, "dropping domain event: loading its payload failed");
        }
    }
}

/// Load whatever the DTO needs beyond the changed row, and tag the event with
/// its goal/task for filtering.
async fn fatten(store: &Store, change: Change) -> Result<BusEvent> {
    let event = match change {
        Change::GoalCreated(goal) => goal_event(store, goal, DomainEvent::GoalCreated).await?,
        Change::GoalUpdated(goal) => goal_event(store, goal, DomainEvent::GoalUpdated).await?,
        // Scoped to the goal it removes, so a `goal`-filtered stream learns
        // that what it follows is gone instead of just falling silent.
        Change::GoalDeleted(id) => BusEvent {
            goal_id: Some(id.clone()),
            task_id: None,
            recorded: None,
            event: DomainEvent::GoalDeleted(DeletedDto { id }),
        },
        Change::TaskCreated(task) => {
            let (dto, keys) = task_event_of(store, task).await?;
            BusEvent {
                event: DomainEvent::TaskCreated(dto),
                goal_id: Some(keys.0),
                task_id: Some(keys.1),
                recorded: None,
            }
        }
        Change::TaskUpdated { task, transition } => {
            let (dto, keys) = task_event_of(store, task).await?;
            BusEvent {
                event: DomainEvent::TaskUpdated(TaskUpdatedDto {
                    task: dto,
                    transition: transition.map(transition_dto),
                }),
                goal_id: Some(keys.0),
                task_id: Some(keys.1),
                recorded: None,
            }
        }
        Change::MessageSent(message) => BusEvent {
            goal_id: Some(message.goal_id.clone()),
            task_id: message.task_id.clone(),
            recorded: None,
            event: DomainEvent::MessageSent(message_dto(message)),
        },
        Change::SessionCreated(session) => {
            session_event(store, session, DomainEvent::SessionCreated).await?
        }
        Change::SessionUpdated(session) => {
            session_event(store, session, DomainEvent::SessionUpdated).await?
        }
        Change::AgentEventCreated(agent_event) => {
            let goal_id = match &agent_event.session_id {
                Some(id) => Some(store.get_session(id).await?.goal_id),
                None => None,
            };
            let task_id = agent_event.task_id.clone();
            let recorded = event_dto(agent_event);
            BusEvent {
                goal_id,
                task_id,
                event: DomainEvent::AgentEvent(AgentEventSummaryDto::from(&recorded)),
                recorded: Some(Arc::new(recorded)),
            }
        }
        Change::SkillCreated(skill) => unscoped(DomainEvent::SkillCreated(skill_dto(skill))),
        Change::SkillUpdated(skill) => unscoped(DomainEvent::SkillUpdated(skill_dto(skill))),
        Change::SkillDeleted(name) => unscoped(DomainEvent::SkillDeleted(DeletedDto { id: name })),
        Change::RepositoryCreated(repo) => {
            unscoped(DomainEvent::RepositoryCreated(repository_dto(repo)))
        }
        Change::RepositoryUpdated(repo) => {
            unscoped(DomainEvent::RepositoryUpdated(repository_dto(repo)))
        }
        Change::RepositoryDeleted(id) => {
            unscoped(DomainEvent::RepositoryDeleted(DeletedDto { id }))
        }
    };
    Ok(event)
}

async fn goal_event(
    store: &Store,
    goal: Goal,
    wrap: fn(GoalDto) -> DomainEvent,
) -> Result<BusEvent> {
    let goal_id = goal.id.clone();
    Ok(BusEvent {
        event: wrap(goal_dto_of(store, goal).await?),
        goal_id: Some(goal_id),
        task_id: None,
        recorded: None,
    })
}

/// Task DTO plus its `(goal_id, task_id)` routing keys.
async fn task_event_of(
    store: &Store,
    task: Task,
) -> Result<(ariadne_api::tasks::TaskDto, (String, String))> {
    let keys = (task.goal_id.clone(), task.id.clone());
    Ok((task_dto_of(store, task).await?, keys))
}

async fn session_event(
    store: &Store,
    session: AgentSession,
    wrap: fn(SessionDto) -> DomainEvent,
) -> Result<BusEvent> {
    let (goal_id, task_id) = (session.goal_id.clone(), session.task_id.clone());
    Ok(BusEvent {
        goal_id: Some(goal_id),
        task_id,
        recorded: None,
        event: wrap(session_dto_of(store, session).await?),
    })
}

/// An event that belongs to no goal or task (profiles and repositories are
/// global).
fn unscoped(event: DomainEvent) -> BusEvent {
    BusEvent {
        event,
        goal_id: None,
        task_id: None,
        recorded: None,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use ariadne_store::{AgentPin, NewAgentEvent, NewGoal, NewRepository, NewSession};

    use super::*;

    /// A store of its own, with a session to record events under and the
    /// goal that session belongs to. Nothing pumps it yet.
    async fn probe_store() -> (Store, Goal, String, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("test.db")).await.unwrap();
        let repository = store
            .create_repository(NewRepository {
                path: "/tmp/probe".into(),
                base_branch: "main".into(),
                description: None,
            })
            .await
            .unwrap();
        let goal = store
            .create_goal(NewGoal {
                title: "probe".into(),
                description: String::new(),
                repository_ids: vec![repository.id.clone()],
                pin: AgentPin {
                    model: "stub:test-model".into(),
                    effort: None,
                },
            })
            .await
            .unwrap();
        let session = store
            .create_session(NewSession {
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
        (store, goal, session.id, dir)
    }

    /// The same store, with the pump running behind a bus.
    async fn pumped() -> (Store, EventBus, String, tempfile::TempDir) {
        let (store, _goal, session, dir) = probe_store().await;
        let bus = start(store.clone());
        (store, bus, session, dir)
    }

    /// The pump loads what each event carries before it publishes it, so a
    /// run of commits is on the bus a while after it is in the store. A
    /// drain answers only once the run is published, whole and in commit
    /// order: what the subscriber holds the moment it is answered is every
    /// event committed before it asked.
    #[tokio::test]
    async fn a_drain_answers_once_the_pump_has_published_what_it_was_handed() {
        let (store, bus, session, _dir) = pumped().await;
        let mut subscriber = bus.subscribe();

        let mut committed = Vec::new();
        for _ in 0..50 {
            committed.push(
                store
                    .create_event(NewAgentEvent {
                        session_id: Some(session.clone()),
                        task_id: None,
                        kind: "agent_message".into(),
                        payload: json!({"text": "done"}),
                    })
                    .await
                    .unwrap()
                    .id,
            );
        }

        bus.drained().await;

        // Read without awaiting: an event the pump still held would be
        // missing here rather than late.
        let mut published = Vec::new();
        while let Ok(event) = subscriber.try_recv() {
            if let DomainEvent::AgentEvent(dto) = event.event {
                published.push(dto.id);
            }
        }
        assert_eq!(
            published, committed,
            "the drain answered before the pump had published every commit"
        );
    }

    /// A bus nothing fattens for has nothing pending, and says so rather
    /// than waiting for a pump that will never answer.
    #[tokio::test]
    async fn a_bus_with_no_pump_answers_its_own_drain() {
        EventBus::new().drained().await;
    }

    /// An event whose payload the pump cannot load reaches no stream, and
    /// nothing publishes it later: a console following that session is left
    /// with a hole, so the pump counts it under that session. A goal it
    /// cannot load costs no console anything, and is not counted. Nor is
    /// the drop another session's to hear about.
    #[tokio::test]
    async fn an_agent_event_the_pump_could_not_load_is_counted_under_its_own_session() {
        let (store, goal, session, _dir) = probe_store().await;
        let bus = EventBus::new();
        let event = store
            .create_event(NewAgentEvent {
                session_id: Some(session.clone()),
                task_id: None,
                kind: "agent_message".into(),
                payload: json!({"text": "done"}),
            })
            .await
            .unwrap();

        // Every query fails from here, so the pump can load neither of them.
        store.close().await;
        super::publish(&store, Change::GoalUpdated(goal), &bus).await;
        assert_eq!(
            bus.dropped(&session),
            0,
            "a goal is no session's transcript"
        );

        super::publish(&store, Change::AgentEventCreated(event), &bus).await;
        assert_eq!(
            bus.dropped(&session),
            1,
            "the session's event is counted as lost"
        );
        assert_eq!(
            bus.dropped("elsewhere"),
            0,
            "and no other session is told it lost anything"
        );
    }
}
