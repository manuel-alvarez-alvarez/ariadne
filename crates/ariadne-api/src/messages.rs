//! Message DTOs: what one agent said to another.

use ariadne_core::{Actor, MessageKind};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MessageDto {
    pub id: String,
    pub goal_id: String,
    /// The task it is about, or None for a message about the goal itself.
    pub task_id: Option<String>,
    pub kind: MessageKind,
    pub from_actor: Actor,
    /// The staffed agent that sent it, or None for the orchestrator, the
    /// daemon and the user.
    pub from_agent_id: Option<String>,
    pub from_session: Option<String>,
    pub to_actor: Actor,
    /// The staffed agent it is for, or None for the orchestrator.
    pub to_agent_id: Option<String>,
    pub body: String,
    /// When it was handed to the recipient's agent, or None while it is still
    /// waiting for one to be free.
    pub delivered_at: Option<String>,
    pub created_at: String,
}

/// Body of `POST /v1/tasks/{id}/messages` and `POST /v1/goals/{id}/messages`.
///
/// Who it is *from* comes from the session header rather than the body: an
/// agent cannot send a message as somebody else, and a message with no session
/// behind it is the user's.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SendMessageRequest {
    /// Who it is for. `orchestrator` needs no agent id — a goal has one.
    pub to_actor: Actor,
    /// The staffed agent it is for, as `GET /v1/tasks/{id}` lists them.
    /// Required for `agent`, refused for the orchestrator.
    #[serde(default)]
    pub to_agent_id: Option<String>,
    pub body: String,
}

/// Body of `POST /v1/tasks/{id}/posts`: text for the live session of another
/// agent, with no stored row. The sender is the session header's.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PostToSessionRequest {
    /// `orchestrator`, or the column of a staffed agent of the task.
    pub to: String,
    pub body: String,
}

/// Body of `POST /v1/remote-tasks/{id}/posts`: a post of the source
/// orchestrator to a column of a worker-protocol task on this worker.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RemotePostRequest {
    /// The persistent daemon ID of the source that sends the post.
    pub source_daemon_id: String,
    /// The column of a staffed agent of the task.
    pub to: String,
    pub body: String,
}

/// The `source_post` frame of `GET /v1/events/stream?source=…`: a post of a
/// worker agent to the orchestrator of its source goal. The worker sends it
/// on one open stream of that source, live, and never again.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SourcePostDto {
    pub id: String,
    pub task_id: String,
    /// The column of the sender.
    pub column: String,
    pub body: String,
}

/// What `GET /v1/tasks/{id}/messages` and `GET /v1/goals/{id}/messages` narrow
/// by.
#[derive(Debug, Clone, Default, Deserialize, utoipa::IntoParams)]
pub struct MessageListQuery {
    /// Only the messages for this staffed agent.
    pub to_agent_id: Option<String>,
    /// Only the ones that have not reached their agent yet.
    #[serde(default)]
    pub undelivered: bool,
    /// Hand the caller its own undelivered messages: narrow the list to the
    /// calling session's agent, and stamp every row it returns delivered.
    ///
    /// This is a delivery, not a read. It needs a session behind it, and it
    /// is what makes a read of the channel count the same as a prompt: a
    /// message handed over here is never handed over again.
    #[serde(default)]
    pub deliver: bool,
}
