//! Model catalog DTOs.

use ariadne_core::AgentKind;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// One thing an agent can be pinned to, as served by `GET /v1/models`: an
/// agent CLI on a model of it (`claude_code:claude-fable-5`), or an agent CLI
/// on its own, which is that CLI on its own default model.
///
/// The id is what a request writes as its `model`, whole. `agent_kind` is the
/// same fact taken apart, so a picker can group the catalog by CLI without
/// parsing anything.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ModelDto {
    #[schema(example = "claude_code:claude-fable-5")]
    pub id: String,
    /// The agent CLI this entry runs on.
    pub agent_kind: AgentKind,
    /// One-line capability summary (absent for discovered opencode models).
    pub description: Option<String>,
    /// The reasoning efforts this entry can be run at, cheapest first; empty
    /// where the model takes none, or where nothing knows what it takes.
    pub efforts: Vec<String>,
    /// What the agent CLI runs this model at when no effort is passed.
    #[schema(example = "high")]
    pub default_effort: Option<String>,
}
