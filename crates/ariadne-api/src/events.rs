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
    /// The one-line gist of `payload`, built by the daemon from the CLI's own
    /// vocabulary rather than stored: an action and its subject for a tool
    /// call, the agent's own words where it left any, and `…` where nothing
    /// of it can be read.
    pub summary: String,
    pub created_at: String,
}

/// Body of `POST /internal/agent-events`, sent by `ariadne agent-event`
/// (hook/notify/plugin side).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct IngestEventRequest {
    /// ARIADNE_SESSION_ID of the reporting agent.
    pub session_id: String,
    /// ARIADNE_LAUNCH_ID of the agent process reporting: which launch of that
    /// session this is. Absent from an agent started before the daemon began
    /// naming them, which is a report nothing is concluded from.
    #[serde(default)]
    pub launch: Option<String>,
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
