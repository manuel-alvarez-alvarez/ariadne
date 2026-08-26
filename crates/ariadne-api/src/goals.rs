//! Goal DTOs.

use ariadne_core::{AgentKind, GoalStatus};
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
    /// Agent CLI the planner runs on: pinned from the planner profile when
    /// the goal was created, or from the model chosen for the goal instead.
    /// Editing the profile afterwards leaves it alone. None = auto, resolved
    /// at spawn time to the first installed CLI.
    pub agent_kind: Option<AgentKind>,
    /// Model the planner runs on, pinned like `agent_kind`. None = the agent
    /// CLI's own default.
    pub model: Option<String>,
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

/// Body of `POST /v1/goals/{id}/submit`: the planner hands the plan to the
/// user for approval. The goal waits in `plan_ready` and no task starts.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SubmitPlanRequest {
    /// Plan summary, posted to the goal thread addressed to the user.
    pub summary: String,
}

/// Body of `POST /v1/goals/{id}/finalize`: the user approves the plan,
/// planning ends and execution starts. The user's call, not the planner's.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct FinalizePlanRequest {
    /// Plan summary, recorded in the goal thread.
    pub summary: String,
}

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
    /// Model the planner runs on; omitted = the planner profile's own model
    /// and agent CLI. A model names the agent CLI that runs it (claude ids
    /// belong to claude_code, gpt/o-series and codex ids to codex, a
    /// `provider/model` id to opencode), and both are pinned onto the goal; a
    /// model nothing can place, and the empty string, are refused.
    #[serde(default)]
    pub model: Option<String>,
}
