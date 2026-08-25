//! Domain events streamed by `GET /v1/events/stream`.
//!
//! Events are *fat*: each one carries the full updated DTO so a client can
//! patch its state without a refetch. There is no replay — a client that
//! (re)connects bootstraps over REST and then follows the stream. Besides the
//! [`DomainEvent`] kinds there is one control event, `resync` ([`ResyncDto`]),
//! sent when a connection has lost events.

use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::events::AgentEventDto;
use crate::goals::GoalDto;
use crate::messages::MessageDto;
use crate::profiles::ProfileDto;
use crate::repositories::RepositoryDto;
use crate::reviews::ReviewDto;
use crate::sessions::SessionDto;
use crate::tasks::{TaskDto, TaskTransitionDto};

/// Payload of `task_updated`: the task as it now stands, plus the audit row
/// when the update was a status transition.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TaskUpdatedDto {
    pub task: TaskDto,
    /// Present when this update was a status transition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transition: Option<TaskTransitionDto>,
}

/// Payload of the deletion events: the id of the gone entity.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DeletedDto {
    pub id: String,
}

/// Payload of the `resync` control event.
///
/// Sent as the last message of a connection that fell too far behind: the
/// daemon dropped `missed` events for it and closes the stream. The client
/// must refetch its REST state before following the stream again (an
/// `EventSource` reconnects on its own).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ResyncDto {
    /// Events this connection lost. Informational: they cannot be recovered.
    pub missed: u64,
}

/// One domain event. Serialized as `{"event": "<kind>", "data": <payload>}`;
/// on the SSE wire the kind becomes the `event:` field and the payload alone
/// the `data:` field.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
pub enum DomainEvent {
    GoalCreated(GoalDto),
    /// Covers status changes: finalize, cancel, completion.
    GoalUpdated(GoalDto),
    /// A terminal goal was deleted, tasks and messages with it.
    GoalDeleted(DeletedDto),
    TaskCreated(TaskDto),
    /// Covers status transitions, edits, stall flags and worktree changes.
    TaskUpdated(TaskUpdatedDto),
    MessageCreated(MessageDto),
    ReviewCreated(ReviewDto),
    SessionCreated(SessionDto),
    /// Covers status changes: kill, resume, exit, activity.
    SessionUpdated(SessionDto),
    /// A raw agent event reported by a hook.
    AgentEvent(AgentEventDto),
    ProfileCreated(ProfileDto),
    ProfileUpdated(ProfileDto),
    ProfileDeleted(DeletedDto),
    RepositoryCreated(RepositoryDto),
    RepositoryUpdated(RepositoryDto),
    RepositoryDeleted(DeletedDto),
}

impl DomainEvent {
    /// SSE `event:` name.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::GoalCreated(_) => "goal_created",
            Self::GoalUpdated(_) => "goal_updated",
            Self::GoalDeleted(_) => "goal_deleted",
            Self::TaskCreated(_) => "task_created",
            Self::TaskUpdated(_) => "task_updated",
            Self::MessageCreated(_) => "message_created",
            Self::ReviewCreated(_) => "review_created",
            Self::SessionCreated(_) => "session_created",
            Self::SessionUpdated(_) => "session_updated",
            Self::AgentEvent(_) => "agent_event",
            Self::ProfileCreated(_) => "profile_created",
            Self::ProfileUpdated(_) => "profile_updated",
            Self::ProfileDeleted(_) => "profile_deleted",
            Self::RepositoryCreated(_) => "repository_created",
            Self::RepositoryUpdated(_) => "repository_updated",
            Self::RepositoryDeleted(_) => "repository_deleted",
        }
    }

    /// SSE `data:` payload — the DTO alone, without the kind wrapper.
    pub fn payload(&self) -> serde_json::Value {
        fn json<T: Serialize>(value: &T) -> serde_json::Value {
            // DTOs are plain structs: serialization cannot fail.
            serde_json::to_value(value).unwrap_or(serde_json::Value::Null)
        }
        match self {
            Self::GoalCreated(g) | Self::GoalUpdated(g) => json(g),
            Self::GoalDeleted(d) => json(d),
            Self::TaskCreated(t) => json(t),
            Self::TaskUpdated(t) => json(t),
            Self::MessageCreated(m) => json(m),
            Self::ReviewCreated(r) => json(r),
            Self::SessionCreated(s) | Self::SessionUpdated(s) => json(s),
            Self::AgentEvent(e) => json(e),
            Self::ProfileCreated(p) | Self::ProfileUpdated(p) => json(p),
            Self::ProfileDeleted(d) => json(d),
            Self::RepositoryCreated(r) | Self::RepositoryUpdated(r) => json(r),
            Self::RepositoryDeleted(d) => json(d),
        }
    }
}

/// Filters of `GET /v1/events/stream`.
#[derive(Debug, Clone, Default, Deserialize, Serialize, IntoParams)]
pub struct EventStreamQuery {
    /// Only events belonging to this goal.
    pub goal: Option<String>,
    /// Only events belonging to this task.
    pub task: Option<String>,
}
