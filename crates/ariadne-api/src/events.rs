//! Agent-event DTOs.

use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AgentEventDto {
    pub id: String,
    pub session_id: Option<String>,
    pub task_id: Option<String>,
    /// e.g. session_start, post_tool_use, stop
    pub kind: String,
    pub payload: serde_json::Value,
    /// The one-line gist of `payload`, built by the daemon from the event's
    /// own vocabulary rather than stored: an action and its subject for a tool
    /// call, the agent's own words where it left any, and `…` where nothing
    /// of it can be read.
    pub summary: String,
    pub created_at: String,
}

/// One event the ACP runtime reports for an agent session, on its way into
/// the store.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct IngestEventRequest {
    /// The Ariadne session the agent runs under.
    pub session_id: String,
    /// Which launch of that session the reporting agent process is. Absent
    /// from a report that names none, which is one nothing is concluded from.
    #[serde(default)]
    pub launch: Option<String>,
    /// Normalized event kind.
    pub kind: String,
    /// The event's payload.
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, IntoParams)]
pub struct EventListQuery {
    /// Filter by session id.
    pub session: Option<String>,
    /// Filter by task id.
    pub task: Option<String>,
}
