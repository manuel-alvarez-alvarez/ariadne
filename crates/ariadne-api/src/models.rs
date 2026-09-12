//! Model catalog DTOs.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// One thing an agent can be pinned to, as served by `GET /v1/models`: a
/// registry agent on a model discovery found it offering
/// (`claude-agent-acp:claude-opus-5`). Every entry names both halves — there
/// is no bare-agent entry, because a model is required wherever an agent is
/// pinned.
///
/// The id is what a request writes as its `model`, whole. `agent_id` is its
/// registry prefix. The rest is what the agent itself said when discovery
/// asked: one line about the model, and the efforts it can be run at.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ModelDto {
    #[schema(example = "claude-agent-acp:claude-opus-5")]
    pub id: String,
    /// Stable registry agent id.
    pub agent_id: String,
    /// One line about the model, which is what a picker shows beside the id.
    pub description: Option<String>,
    /// The reasoning efforts this entry can be run at, cheapest first; empty
    /// where the model takes none, or where nothing knows what it takes.
    pub efforts: Vec<EffortDto>,
    /// Whether an agent can be staffed on this entry. Every model is enabled
    /// until the user turns it off; a disabled one stays in the catalog,
    /// where it is shown as off and refused as a pin.
    pub enabled: bool,
}

/// Body of `PUT /v1/models/enabled`: one model of the catalog, turned on or
/// off.
///
/// The id is a field rather than a path segment because a model id carries
/// `:` and often `/` (`opencode-acp:anthropic/claude-sonnet-4`) — which is a
/// path of its own, not a segment of one.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SetModelEnabledRequest {
    /// The entry, as `GET /v1/models` spells its `id`.
    #[schema(example = "claude-agent-acp:claude-opus-5")]
    pub id: String,
    /// What it becomes.
    pub enabled: bool,
}

/// One reasoning effort an entry can be run at: the name it is passed by, and
/// what spending it buys.
///
/// At most one effort of a model is the `default`: what its agent runs it at
/// when a task pins no effort at all. None of them are where the agent has no
/// default to name.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct EffortDto {
    #[schema(example = "high")]
    pub id: String,
    /// What spending this effort buys, where the agent describes it. `null`
    /// where nothing knows.
    pub description: Option<String>,
    /// Whether this is what the agent runs the model at when none is passed.
    pub default: bool,
}
