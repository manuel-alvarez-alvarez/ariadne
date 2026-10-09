//! Task DTOs.

use ariadne_core::TaskStatus;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::usage::TokenUsageDto;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TaskDto {
    /// The column of its goal's workflow the task is in while it is
    /// `in_progress`; null before its first column and once it has ended.
    pub step: Option<String>,
    pub id: String,
    pub goal_id: String,
    /// Id of the repository the task works in, one of its goal's.
    pub repo_id: String,
    pub title: String,
    pub description: String,
    pub status: TaskStatus,
    /// The agents staffed on the task, one per column of its goal's
    /// workflow, in the order the orchestrator listed them. What each one can
    /// do is the skills it carries.
    pub agents: Vec<TaskAgentDto>,
    /// Ids of tasks that must finish before this one starts.
    pub depends_on: Vec<String>,
    /// The branch every column works on, in the one worktree the task has.
    pub branch: String,
    pub worktree_path: Option<String>,
    /// Set when the agent went idle without advancing the task.
    pub stalled: bool,
    /// The commit the task landed as, where its last column reported one.
    pub merge_commit: Option<String>,
    /// URL of the pull or merge request the task's `pr` column opened, once
    /// it has; null for a task landed directly.
    pub pr_url: Option<String>,
    /// Why a `failed` or `cancelled` task ended — an agent's own `fail_task`
    /// reason, a dependency that never landed, a cancelled goal. Null for
    /// every other status, and for an ending nobody gave a reason for.
    pub reason: Option<String>,
    /// What the agents of this task have spent between them.
    pub usage: TaskUsageDto,
    pub created_at: String,
    pub updated_at: String,
}

/// What a task cost, by who spent it: one entry per agent that has a session
/// on the task, and the total of every session on it.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct TaskUsageDto {
    /// Every session on the task summed, whatever its column.
    pub total: TokenUsageDto,
    /// One entry per agent that has a session on the task, every run of it
    /// summed, in column order. An agent whose session has yet to report
    /// anything is listed with zeros; one that has never been spawned is not
    /// listed at all.
    #[serde(default)]
    #[schema(required = true)]
    pub agents: Vec<AgentUsageDto>,
}

/// What one staffed agent spent on a task, named the way a reader addresses
/// it: an agent has no name of its own, so its column and its skills are what
/// identify it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct AgentUsageDto {
    /// The column the agent works; null for an agent the task no longer
    /// staffs.
    pub step: Option<String>,
    pub agent_id: String,
    /// The skills the agent loads; empty only if the agent is gone.
    pub skills: Vec<String>,
    pub usage: TokenUsageDto,
}

/// One agent staffed on a task: the column it works, what it knows, and what
/// it runs on.
///
/// The agent has no identity of its own. `step` says only which column of
/// the workflow it works; the skills are what it can do. What it runs on was
/// sized by the orchestrator when it staffed the task, or chosen by the user
/// since — either way it is what this agent runs on, and nothing behind it
/// changes that.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TaskAgentDto {
    /// The id of the workflow column this agent works.
    pub step: String,
    pub id: String,
    /// The skills this agent loads, in the order they reach it.
    #[schema(example = json!(["coding", "documentation"]))]
    pub skills: Vec<String>,
    /// What this agent runs on, `<agent>:<model>`.
    #[schema(example = "codex-acp:o3")]
    pub model: String,
    /// The reasoning effort that model is run at. None = whatever the agent
    /// runs it at on its own.
    #[schema(example = "high")]
    pub effort: Option<String>,
    /// What the orchestrator told this agent beyond the task itself. None =
    /// the task is the whole of it.
    pub brief: Option<String>,
    /// This agent's live session, the id `POST /v1/sessions/{id}/switch`
    /// takes. None while it carries no live session.
    pub session_id: Option<String>,
}

/// One agent to staff on a task: the column it works, the skills it loads,
/// and what it is to run on.
///
/// The model is written `<agent>:<model>`: the id of an agent in the ACP
/// registry, and after the `:` one model of it. Both halves are required — a
/// model is required, and no agent default stands in for one — and a string
/// naming no registry agent is refused: nothing here derives one from the
/// other.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AgentAssignment {
    /// The id of the workflow column this agent works, one of the goal's
    /// columns. A task takes one agent per column, and a column named twice
    /// is refused.
    pub step: String,
    /// The names of the skills this agent loads, in the order they reach it.
    /// Omitted or empty = the column's own skills. A name no skill answers
    /// to is refused.
    #[serde(default)]
    #[schema(example = json!(["coding", "documentation"]))]
    pub skills: Vec<String>,
    /// What this agent runs on, `<agent>:<model>`. Required; the empty
    /// string and the word "default" are refused.
    #[schema(example = "codex-acp:o3")]
    pub model: String,
    /// The reasoning effort to run that model at, one of the efforts
    /// `GET /v1/models` lists for it; anything else is refused. Omitted (or
    /// "default") = whatever the agent runs the model at.
    #[serde(default)]
    #[schema(example = "high")]
    pub effort: Option<String>,
    /// What to tell this agent beyond the task itself. Omitted = the task is
    /// the whole of it.
    #[serde(default)]
    pub brief: Option<String>,
}

impl AgentAssignment {
    /// An agent on column `step`, on the named skills, on `model`.
    pub fn new(
        step: impl Into<String>,
        skills: impl IntoIterator<Item = impl Into<String>>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            step: step.into(),
            skills: skills.into_iter().map(Into::into).collect(),
            model: model.into(),
            effort: None,
            brief: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateTaskRequest {
    pub title: String,
    #[serde(default)]
    pub description: String,
    /// Id of one of the goal's repositories; may be omitted when the goal
    /// works in exactly one.
    pub repo_id: Option<String>,
    /// The agents to staff: one per column of the goal's workflow, each with
    /// its own model. A column left out is named when the plan is finalized.
    pub agents: Vec<AgentAssignment>,
    /// Task ids this task depends on.
    #[serde(default)]
    pub depends_on: Vec<String>,
}

/// Partial update; only allowed while the task is pending, ready or failed.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateTaskRequest {
    /// The whole staffing, replaced: every column is staffed afresh, with
    /// the skills and the model it names. The way to staff a column a task
    /// lacks before it is retried.
    pub agents: Option<Vec<AgentAssignment>>,
    pub title: Option<String>,
    pub description: Option<String>,
    /// The whole dependency list, replaced.
    pub depends_on: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TransitionRequest {
    pub to: TaskStatus,
    pub reason: Option<String>,
    /// The commit the task landed as, where `to` is `finished` and a column
    /// reported one.
    pub merge_commit: Option<String>,
}

/// The agent of the `pr` column asking the daemon to open the pull or merge
/// request its task lands by. The daemon runs the forge's own CLI, so this
/// carries only what the agent cannot read off the task or the repository
/// itself.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct OpenPullRequestRequest {
    /// Titled by the repository's own commit conventions.
    pub title: String,
    /// Filled from the repository's own request template.
    pub body: String,
    /// Opens the request as a draft. Defaults to false.
    #[serde(default)]
    pub draft: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TaskTransitionDto {
    /// The column the task left, where the move crossed columns.
    pub from_step: Option<String>,
    /// The column the task entered.
    pub to_step: Option<String>,
    pub id: String,
    pub from_status: String,
    pub to_status: String,
    pub actor: String,
    pub reason: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, IntoParams)]
pub struct TaskListQuery {
    /// Filter by goal id.
    pub goal: Option<String>,
    /// Filter by status.
    pub status: Option<TaskStatus>,
}

/// Body of `POST /v1/tasks/{id}/step/complete`: the current column's agent
/// hands the task to the next column, or finishes it from the last one.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CompleteStepRequest {
    /// What the next column's agent is briefed with.
    pub reason: String,
    /// The commit the task landed as, which the `merged` gate checks.
    pub merge_commit: Option<String>,
}

/// Body of `POST /v1/tasks/{id}/step/fail`: the current column's agent hands
/// the task back to the previous column, or fails it from the first one.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct FailStepRequest {
    /// What the previous column's agent is told to fix.
    pub reason: String,
}
