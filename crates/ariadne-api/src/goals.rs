//! Goal DTOs.

use ariadne_core::GoalStatus;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::repositories::RepositoryDto;
use crate::usage::TokenUsageDto;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct GoalDto {
    /// The workflow every task of the goal runs on, chosen when the goal was
    /// created.
    pub workflow: String,
    /// The columns of that workflow as they were when the goal was created:
    /// a later edit of the catalog reaches later goals alone.
    #[serde(default)]
    #[schema(required = true)]
    pub steps: Vec<crate::workflows::WorkflowStepDto>,
    pub id: String,
    pub title: String,
    pub description: String,
    pub issue_url: Option<String>,
    pub status: GoalStatus,
    /// Whether this goal has an orchestrator for its lifetime.
    pub orchestrated: bool,
    /// What the orchestrator runs on, `<agent>:<model>`: the registry agent
    /// and, after the `:`, the model of it.
    #[schema(example = "claude-acp:claude-opus-5")]
    pub model: String,
    /// The reasoning effort that model is run at, pinned like `model`. None =
    /// whatever the agent runs it at on its own.
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

/// What a goal cost, by the seat that spent it, and by the agent of every
/// column of every task under it.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct GoalUsageDto {
    /// Every session of the goal summed, the orchestrator's included.
    pub total: TokenUsageDto,
    /// The orchestrator's sessions, which belong to no task.
    pub orchestrator: TokenUsageDto,
    /// One entry per agent of every task of the goal that has a session,
    /// task by task and column by column.
    #[serde(default)]
    #[schema(required = true)]
    pub agents: Vec<crate::tasks::AgentUsageDto>,
}

/// Body of `POST /v1/goals/{id}/finalize`: the orchestrator ends planning and
/// execution starts. The orchestrator's call, not the user's, and it carries
/// nothing — the plan is the tasks it wrote.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct FinalizePlanRequest {}

/// Body of `POST /v1/goals/{id}/complete`: the orchestrator says the goal is
/// done. Its call, not the user's, and it carries nothing — every task being
/// finished or cancelled is the whole of the argument, and the daemon checks
/// that itself.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CompleteGoalRequest {}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateGoalRequest {
    /// The workflow every task of the goal runs on, a name of the catalog
    /// (`GET /v1/workflows`). Omitted = the default of the first repository.
    /// It cannot change once the goal is created.
    #[serde(default)]
    pub workflow: Option<String>,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub issue_url: Option<String>,
    /// Ids of registered repositories (`POST /v1/repositories`); at least one.
    pub repository_ids: Vec<String>,
    /// What the orchestrator runs on, `<agent>:<model>` — the id of an agent
    /// in the ACP registry and, after the `:`, the model of it:
    /// `codex-acp:gpt-5.3-codex`, `opencode:ollama/llama3:8b`. Required —
    /// a model is required, and no agent default stands in for one. The model
    /// half is free text, handed to that agent as typed; a string naming no
    /// registry agent is refused, and so are the empty string and the word
    /// "default".
    #[schema(example = "codex-acp:gpt-5.3-codex")]
    pub model: String,
    /// The reasoning effort to run that model at, one of the efforts `GET
    /// /v1/models` lists for it; anything else is refused. Omitted (or
    /// "default") = whatever the agent runs the model at.
    #[serde(default)]
    #[schema(example = "high")]
    pub effort: Option<String>,
}
