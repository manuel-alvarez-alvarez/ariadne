//! Model catalog DTOs.

use ariadne_core::AgentKind;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// One model an agent CLI can run, as served by `GET /v1/models`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ModelDto {
    #[schema(example = "claude-fable-5")]
    pub id: String,
    /// The agent CLI this model belongs to.
    pub agent_kind: AgentKind,
    /// One-line capability summary (absent for discovered opencode models).
    pub description: Option<String>,
}
