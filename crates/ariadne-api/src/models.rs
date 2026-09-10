//! Model catalog DTOs.

use ariadne_core::{AgentKind, ModelTier};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// One thing an agent can be pinned to, as served by `GET /v1/models`: an
/// agent CLI on a model of it (`claude_code:claude-fable-5`). Every entry
/// names both halves — there is no bare-CLI entry, because a model is
/// required wherever an agent is pinned.
///
/// The id is what a request writes as its `model`, whole. `agent_id` is its
/// registry prefix. `agent_kind` keeps the native adapter family, and is
/// `acp` for a discovered entry. The rest is what an orchestrator sizes a task
/// from: what this model is, what it costs and how fast it answers next to
/// every other entry, the work it is and is not the choice for, and what each
/// of its efforts buys.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ModelDto {
    #[schema(example = "claude_code:claude-fable-5")]
    pub id: String,
    /// Stable registry agent id. Native catalog entries use their agent kind.
    #[serde(default)]
    pub agent_id: String,
    /// The native adapter family this entry runs through.
    pub agent_kind: AgentKind,
    /// One line about the model, which is what a picker shows beside the id.
    pub description: Option<String>,
    /// The capability class this entry belongs to, or `unknown` where nothing
    /// says — a model discovered at runtime that nothing has been written
    /// about.
    pub tier: ModelTier,
    /// What it costs to run: 1 (free) to 5 (frontier), ranked across the whole
    /// catalog so entries of different agent CLIs compare. `null` where
    /// nothing knows.
    #[schema(example = 3, minimum = 1, maximum = 5)]
    pub cost: Option<u8>,
    /// How fast it answers: 1 (thinks for minutes) to 5 (near-instant),
    /// ranked the same way. `null` where nothing knows.
    #[schema(example = 4, minimum = 1, maximum = 5)]
    pub speed: Option<u8>,
    /// Task shapes this entry is the right choice for; empty where nothing
    /// knows.
    #[schema(example = json!(["well-specified single-file fixes"]))]
    pub best_for: Vec<String>,
    /// Task shapes it is the wrong choice for; empty where nothing knows.
    #[schema(example = json!(["cross-subsystem design"]))]
    pub avoid_for: Vec<String>,
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
/// both `:` and, for the ids opencode discovers, `/`
/// (`opencode:anthropic/claude-sonnet-4`) — which is a path of its own, not a
/// segment of one.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SetModelEnabledRequest {
    /// The entry, as `GET /v1/models` spells its `id`.
    #[schema(example = "claude_code:claude-fable-5")]
    pub id: String,
    /// What it becomes.
    pub enabled: bool,
}

/// One reasoning effort an entry can be run at: the name it is passed by, and
/// what spending it buys.
///
/// At most one effort of a model is the `default`: what its agent CLI runs it
/// at when a task pins no effort at all. None of them are where the CLI has no
/// default to name.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct EffortDto {
    #[schema(example = "high")]
    pub id: String,
    /// What spending this effort buys — the same on every model of one agent
    /// CLI. `null` where nothing knows, which is where the effort was
    /// discovered rather than curated.
    pub description: Option<String>,
    /// Whether this is what the agent CLI runs the model at when none is
    /// passed.
    pub default: bool,
}
