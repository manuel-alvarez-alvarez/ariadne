//! Agent-kind configuration DTOs.

use ariadne_core::AgentKind;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// How one agent CLI is launched, shared by every profile that runs on it.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AgentConfigDto {
    pub agent_kind: AgentKind,
    /// Argv flags appended on every spawn and resume of this agent CLI.
    pub extra_flags: Vec<String>,
    /// What Ariadne ships for this agent kind: what `extra_flags` was seeded
    /// with, and what restoring the defaults writes back — a client resets by
    /// sending these back as `extra_flags`.
    pub default_flags: Vec<String>,
}

/// Body of `PUT /v1/agents/{kind}`: the whole new flag list, empty included.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct UpdateAgentConfigRequest {
    pub extra_flags: Vec<String>,
}
