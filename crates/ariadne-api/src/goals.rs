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
    pub required_approvals: i64,
    pub planner_profile_id: String,
    /// What the planner runs on, `<agent_kind>[:<model>]`: the agent CLI and,
    /// after a `:`, the model of it (`codex`, `claude_code:claude-opus-5`).
    /// Pinned when the goal was created, from the model chosen for it or,
    /// where none was, from the planner profile — editing the profile
    /// afterwards leaves it alone. None = auto: the first installed CLI,
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

/// What a goal cost, by the role that spent it. Grouped by role rather than
/// by profile: a goal's engineers are as many as it has tasks, and what is
/// read at this height is where the tokens went, not which agent went there.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct GoalUsageDto {
    /// Every session of the goal summed, the planner's included.
    pub total: TokenUsageDto,
    /// The planner's sessions, which belong to no task.
    pub planner: TokenUsageDto,
    /// Every engineer session of every task of the goal.
    pub engineers: TokenUsageDto,
    /// Every reviewer session of every task of the goal, all rounds.
    pub reviewers: TokenUsageDto,
}

/// Body of `POST /v1/goals/{id}/finalize`: the planner ends planning and
/// execution starts. The planner's call, not the user's, and it carries
/// nothing — the plan is the tasks it wrote.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct FinalizePlanRequest {}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateGoalRequest {
    pub title: String,
    #[serde(default)]
    pub description: String,
    /// Ids of registered repositories (`POST /v1/repositories`); at least one.
    pub repository_ids: Vec<String>,
    /// Planner profile id or unique name.
    pub planner_profile: String,
    /// Max tasks the planner may create (default: unbounded).
    pub max_tasks: Option<i64>,
    /// Approvals required to merge a task (default 1).
    pub required_approvals: Option<i64>,
    /// What the planner runs on, `<agent_kind>[:<model>]` — the agent CLI and,
    /// after a `:`, the model of it: `codex`, `codex:gpt-5.3-codex`,
    /// `opencode:ollama/llama3:8b`. The model half is free text, handed to
    /// that CLI as typed; an agent CLI on its own runs it on its own default
    /// model, and a string naming no agent CLI is refused. Omitted (or
    /// "default") = the planner profile's own model, as it stands now.
    #[serde(default)]
    #[schema(example = "codex:gpt-5.3-codex")]
    pub model: Option<String>,
    /// The reasoning effort to run that model at, one of the efforts
    /// `GET /v1/models` lists for it; anything else is refused. Omitted (or
    /// "default") = whatever the agent CLI runs the model at, and where
    /// `model` is omitted too, the planner profile's own effort. Named where
    /// `model` is omitted, the goal takes the planner profile's own model at
    /// this effort.
    #[serde(default)]
    #[schema(example = "high")]
    pub effort: Option<String>,
}
