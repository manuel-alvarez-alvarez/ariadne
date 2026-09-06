//! Goal DTOs.

use ariadne_core::GoalStatus;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::repositories::RepositoryDto;
use crate::usage::TokenUsageDto;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct GoalDto {
    pub id: String,
    pub title: String,
    pub description: String,
    pub status: GoalStatus,
    /// None = unbounded.
    pub max_tasks: Option<i64>,
    /// What the orchestrator runs on, `<agent_kind>[:<model>]`: the agent CLI
    /// and, after a `:`, the model of it (`codex`,
    /// `claude_code:claude-opus-5`). None = auto: the first installed CLI,
    /// resolved at spawn time, on its own default model.
    #[schema(example = "claude_code:claude-opus-5")]
    pub model: Option<String>,
    /// The reasoning effort that model is run at, pinned like `model`. None =
    /// whatever the agent CLI runs it at on its own.
    #[schema(example = "high")]
    pub effort: Option<String>,
    /// The registered repositories the goal works in, as they stand now: a
    /// goal references them, so an edit to one shows up here.
    pub repos: Vec<RepositoryDto>,
    /// What the agents of this goal have spent between them.
    pub usage: GoalUsageDto,
    pub created_at: String,
    pub updated_at: String,
}

/// What a goal cost, by the seat that spent it. Grouped by seat rather than
/// by agent: a goal's authors are as many as it has tasks, and what is
/// read at this height is where the tokens went, not which agent went there.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct GoalUsageDto {
    /// Every session of the goal summed, the orchestrator's included.
    pub total: TokenUsageDto,
    /// The orchestrator's sessions, which belong to no task.
    pub orchestrator: TokenUsageDto,
    /// Every author session of every task of the goal.
    pub authors: TokenUsageDto,
    /// Every reviewer session of every task of the goal, all rounds.
    pub reviewers: TokenUsageDto,
}

/// Body of `POST /v1/goals/{id}/finalize`: the orchestrator ends planning and
/// execution starts. The orchestrator's call, not the user's, and it carries
/// nothing — the plan is the tasks it wrote.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct FinalizePlanRequest {}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateGoalRequest {
    pub title: String,
    #[serde(default)]
    pub description: String,
    /// Ids of registered repositories (`POST /v1/repositories`); at least one.
    pub repository_ids: Vec<String>,
    /// Max tasks the orchestrator may create (default: unbounded).
    pub max_tasks: Option<i64>,
    /// What the orchestrator runs on, `<agent_kind>[:<model>]` — the agent
    /// CLI and, after a `:`, the model of it: `codex`, `codex:gpt-5.3-codex`,
    /// `opencode:ollama/llama3:8b`. The model half is free text, handed to
    /// that CLI as typed; an agent CLI on its own runs it on its own default
    /// model, and a string naming no agent CLI is refused. Omitted (or
    /// "default") = auto: the first installed CLI, on its own default model.
    #[serde(default)]
    #[schema(example = "codex:gpt-5.3-codex")]
    pub model: Option<String>,
    /// The reasoning effort to run that model at, one of the efforts `GET
    /// /v1/models` lists for it; anything else is refused. Omitted (or
    /// "default") = whatever the agent CLI runs the model at. An effort is
    /// run at a model, so an effort written where `model` names none is
    /// refused.
    #[serde(default)]
    #[schema(example = "high")]
    pub effort: Option<String>,
}
