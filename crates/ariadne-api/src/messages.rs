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
    /// When it reached the recipient's pane, or None while it is still
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
    pub kind: MessageKind,
    /// Who it is for. `orchestrator` needs no agent id — a goal has one.
    pub to_actor: Actor,
    /// The staffed agent it is for, as `GET /v1/tasks/{id}` lists them.
    /// Required for `author` and `reviewer`, refused for the orchestrator.
    #[serde(default)]
    pub to_agent_id: Option<String>,
    pub body: String,
}

/// What `GET /v1/tasks/{id}/messages` and `GET /v1/goals/{id}/messages` narrow
/// by.
#[derive(Debug, Clone, Default, Deserialize, utoipa::IntoParams)]
pub struct MessageListQuery {
    /// Only the messages for this staffed agent.
    pub to_agent_id: Option<String>,
    /// Only the ones that have not reached a pane yet.
    #[serde(default)]
    pub undelivered: bool,
}
