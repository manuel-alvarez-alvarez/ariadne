//! Agent-event DTOs.

use ariadne_core::AgentKind;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AgentEventDto {
    pub id: String,
    pub session_id: Option<String>,
    pub task_id: Option<String>,
    pub agent_kind: Option<String>,
    /// e.g. session_start, post_tool_use, stop, turn_complete
    pub kind: String,
    pub payload: serde_json::Value,
    pub created_at: String,
}

/// Body of `POST /internal/agent-events`, sent by `ariadne agent-event`
/// (hook/notify/plugin side).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct IngestEventRequest {
    /// ARIADNE_SESSION_ID of the reporting agent.
    pub session_id: String,
    pub agent_kind: AgentKind,
    /// Normalized event kind.
    pub kind: String,
    /// Raw hook/notify/plugin payload.
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, IntoParams)]
pub struct EventListQuery {
    /// Filter by session id.
    pub session: Option<String>,
    /// Filter by task id.
    pub task: Option<String>,
}
