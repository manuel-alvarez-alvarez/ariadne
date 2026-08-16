//! Domain-event bus.
//!
//! The store announces every committed write ([`Change`]); one pump task
//! fattens each change into a [`DomainEvent`] carrying the full DTO and
//! broadcasts it to the SSE subscribers of `/v1/events/stream`. Because the
//! hook sits in the repository layer, HTTP handlers, the scheduler and the
//! launcher all feed the bus without knowing it exists.
//!
//! Delivery is best-effort and history-free: a subscriber that cannot keep up
//! is lagged by the broadcast channel and must resync over REST.

use tokio::sync::{broadcast, mpsc};
use tracing::{debug, warn};

use ariadne_api::goals::GoalDto;
use ariadne_api::sessions::SessionDto;
use ariadne_api::stream::{DeletedDto, DomainEvent, TaskUpdatedDto};
use ariadne_store::{AgentSession, Change, Goal, Result, Store, Task};

use crate::http::convert::{
    event_dto, goal_dto, message_dto, profile_dto, review_dto, session_dto, task_dto,
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
}

impl BusEvent {
    /// Does this event pass the `goal`/`task` stream filters? An event with no
    /// such association (profiles) is filtered out by either filter.
    pub fn matches(&self, goal: Option<&str>, task: Option<&str>) -> bool {
        goal.is_none_or(|g| self.goal_id.as_deref() == Some(g))
            && task.is_none_or(|t| self.task_id.as_deref() == Some(t))
    }
}

/// Fan-out handle held by [`AppState`](crate::http::AppState).
#[derive(Clone)]
pub struct EventBus {
    tx: broadcast::Sender<BusEvent>,
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl EventBus {
    pub fn new() -> Self {
        Self {
            tx: broadcast::Sender::new(CAPACITY),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<BusEvent> {
        self.tx.subscribe()
    }

    /// Broadcast an event; dropped when nobody is listening.
    pub fn publish(&self, event: BusEvent) {
        let _ = self.tx.send(event);
    }
}

/// Install the store change hook and start the pump. Call once at startup,
/// before anything writes.
pub fn start(store: Store) -> EventBus {
    let bus = EventBus::new();
    match store.watch_changes() {
        Some(rx) => {
            tokio::spawn(pump(store, rx, bus.clone()));
        }
        None => warn!("store change hook already installed; no domain events will be published"),
    }
    bus
}

/// Fatten changes into domain events, in commit order.
async fn pump(store: Store, mut rx: mpsc::UnboundedReceiver<Change>, bus: EventBus) {
    while let Some(change) = rx.recv().await {
        match fatten(&store, change).await {
            Ok(event) => {
                debug!(kind = event.event.kind(), "publishing domain event");
                bus.publish(event);
            }
            Err(e) => warn!(error = %e, "dropping domain event: loading its payload failed"),
        }
    }
}

/// Load whatever the DTO needs beyond the changed row, and tag the event with
/// its goal/task for filtering.
async fn fatten(store: &Store, change: Change) -> Result<BusEvent> {
    let event = match change {
        Change::GoalCreated(goal) => goal_event(store, goal, DomainEvent::GoalCreated).await?,
        Change::GoalUpdated(goal) => goal_event(store, goal, DomainEvent::GoalUpdated).await?,
        Change::TaskCreated(task) => {
            let (dto, keys) = task_dto_of(store, task).await?;
            BusEvent {
                event: DomainEvent::TaskCreated(dto),
                goal_id: Some(keys.0),
                task_id: Some(keys.1),
            }
        }
        Change::TaskUpdated { task, transition } => {
            let (dto, keys) = task_dto_of(store, task).await?;
            BusEvent {
                event: DomainEvent::TaskUpdated(TaskUpdatedDto {
                    task: dto,
                    transition: transition.map(transition_dto),
                }),
                goal_id: Some(keys.0),
                task_id: Some(keys.1),
            }
        }
        Change::MessageCreated(message) => BusEvent {
            goal_id: Some(message.goal_id.clone()),
            task_id: message.task_id.clone(),
            event: DomainEvent::MessageCreated(message_dto(message)),
        },
        Change::ReviewCreated(review) => {
            // A review carries no goal id of its own; resolve it via the task
            // so a `goal`-filtered stream still sees verdicts.
            let goal_id = store.get_task(&review.task_id).await?.goal_id;
            BusEvent {
                goal_id: Some(goal_id),
                task_id: Some(review.task_id.clone()),
                event: DomainEvent::ReviewCreated(review_dto(review)),
            }
        }
        Change::SessionCreated(session) => session_event(session, DomainEvent::SessionCreated),
        Change::SessionUpdated(session) => session_event(session, DomainEvent::SessionUpdated),
        Change::AgentEventCreated(agent_event) => {
            let goal_id = match &agent_event.session_id {
                Some(id) => Some(store.get_session(id).await?.goal_id),
                None => None,
            };
            BusEvent {
                goal_id,
                task_id: agent_event.task_id.clone(),
                event: DomainEvent::AgentEvent(event_dto(agent_event)),
            }
        }
        Change::ProfileCreated(profile) => {
            unscoped(DomainEvent::ProfileCreated(profile_dto(profile)))
        }
        Change::ProfileUpdated(profile) => {
            unscoped(DomainEvent::ProfileUpdated(profile_dto(profile)))
        }
        Change::ProfileDeleted(id) => unscoped(DomainEvent::ProfileDeleted(DeletedDto { id })),
    };
    Ok(event)
}

async fn goal_event(
    store: &Store,
    goal: Goal,
    wrap: fn(GoalDto) -> DomainEvent,
) -> Result<BusEvent> {
    let repos = store.list_goal_repos(&goal.id).await?;
    let goal_id = goal.id.clone();
    Ok(BusEvent {
        event: wrap(goal_dto(goal, repos)),
        goal_id: Some(goal_id),
        task_id: None,
    })
}

/// Task DTO plus its `(goal_id, task_id)` routing keys.
async fn task_dto_of(
    store: &Store,
    task: Task,
) -> Result<(ariadne_api::tasks::TaskDto, (String, String))> {
    let reviewers = store.list_task_reviewers(&task.id).await?;
    let deps = store.list_task_dependencies(&task.id).await?;
    let keys = (task.goal_id.clone(), task.id.clone());
    Ok((task_dto(task, reviewers, deps), keys))
}

fn session_event(session: AgentSession, wrap: fn(SessionDto) -> DomainEvent) -> BusEvent {
    BusEvent {
        goal_id: Some(session.goal_id.clone()),
        task_id: session.task_id.clone(),
        event: wrap(session_dto(session)),
    }
}

/// An event that belongs to no goal or task (profiles are global).
fn unscoped(event: DomainEvent) -> BusEvent {
    BusEvent {
        event,
        goal_id: None,
        task_id: None,
    }
}
