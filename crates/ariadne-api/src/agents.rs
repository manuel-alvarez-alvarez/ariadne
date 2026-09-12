//! ACP agent registry and agent configuration DTOs.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// One ACP agent known to the daemon and its latest discovery result.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AcpAgentDto {
    /// Stable id used as the model-id prefix.
    pub id: String,
    /// Program followed by its arguments.
    pub command: Vec<String>,
    /// Whether Ariadne supplied this entry.
    pub builtin: bool,
    pub status: AcpAgentStatus,
    pub capabilities: AcpCapabilitiesDto,
    /// One flag for every optional capability that is absent.
    pub degraded: Vec<AcpDegradation>,
    /// Why discovery rejected this agent.
    pub rejection_reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AcpAgentStatus {
    Ready,
    Rejected,
}

/// Required and optional ACP capabilities measured by discovery.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct AcpCapabilitiesDto {
    pub stdio: bool,
    pub protocol_v1: bool,
    pub session_new: bool,
    pub model: bool,
    pub thought_level: bool,
    pub session_list: bool,
    pub session_load: bool,
}

/// An optional ACP capability missing from an otherwise usable agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AcpDegradation {
    NoEfforts,
    NoAdoption,
    NoRestartResume,
}

/// How one registry agent is launched, shared by every session that runs on
/// it.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AgentConfigDto {
    /// The id of the agent in the ACP registry (`GET /v1/acp-agents`).
    pub agent_id: String,
    /// Argv flags appended to the agent's registry command on every spawn and
    /// resume.
    pub extra_flags: Vec<String>,
    /// What Ariadne ships for this agent: what restoring the defaults writes
    /// back — a client resets by sending these back as `extra_flags`. An ACP
    /// agent ships with no flags, so this is empty.
    pub default_flags: Vec<String>,
}

/// Body of `PUT /v1/agents/{id}`: the whole new flag list, empty included.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateAgentConfigRequest {
    pub extra_flags: Vec<String>,
}
