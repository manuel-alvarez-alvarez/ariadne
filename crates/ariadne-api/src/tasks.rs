//! Task DTOs.

use ariadne_core::TaskStatus;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::usage::TokenUsageDto;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TaskDto {
    /// Present for worker-owned remote tasks. Null for local and legacy tasks.
    #[serde(default)]
    pub execution_context: Option<TaskExecutionContext>,
    /// The column of its goal's workflow the task is in while it is
    /// `in_progress`; null before its first column and once it has ended.
    pub step: Option<String>,
    pub id: String,
    pub goal_id: Option<String>,
    #[serde(default)]
    pub workflow: Option<String>,
    #[serde(default)]
    pub steps: Vec<crate::workflows::WorkflowStepDto>,
    /// The pull request operation the task runs (029, 032), or null for an
    /// ordinary task. Such a task belongs to no goal.
    #[serde(default)]
    pub operation: Option<ariadne_core::workflow::PrOperation>,
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
    /// it has; null for a task landed directly. On a review or babysit task
    /// (one with an `operation`) that opened none, the request it works on, in
    /// the response only.
    pub pr_url: Option<String>,
    /// Why a `failed` or `cancelled` task ended — an agent's own `fail_task`
    /// reason, a dependency that never landed, a cancelled goal. Null for
    /// every other status, and for an ending nobody gave a reason for.
    pub reason: Option<String>,
    /// What the agents of this task have spent between them.
    pub usage: TaskUsageDto,
    pub created_at: String,
    pub updated_at: String,
    /// The host the task runs on: `local` for a task on the goal's own
    /// daemon, else the name of a host (`GET /v1/hosts`).
    #[serde(default = "local_host")]
    #[schema(example = "local")]
    pub host: String,
    /// True while source progress is stale after host or event-stream loss.
    /// The source keeps the last acknowledged worker state.
    /// Direct local and worker reads retain their established semantics.
    #[serde(default)]
    pub host_unreachable: bool,
}

/// The host of a task that names none: the goal's own daemon.
fn local_host() -> String {
    LOCAL_HOST.into()
}

/// The name of the goal's own daemon as a task's host.
pub const LOCAL_HOST: &str = "local";

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
    /// The id of the workflow column this agent works; empty for the agent
    /// of a task with no workflow.
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
    /// is refused. Omitted only for the one agent of a task with no workflow.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<String>,
    /// The names of the skills this agent loads, in the order they reach it.
    /// Omitted or empty = the column's own skills, or none for a task with
    /// no workflow. A name no skill answers
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
            step: Some(step.into()),
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
    /// The host to run the task on, a `ready` host of `GET /v1/hosts`.
    /// Omitted or `local` = this daemon. New remote tasks become worker drafts
    /// during planning. The source authorizes execution after finalization and dependencies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(example = "worker")]
    pub host: Option<String>,
    /// On a linked goal only: the id of the far task on the goal's daemon
    /// that this task runs. The goal holds one task for each. A repeated
    /// create answers `200` with the task the goal already holds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin_task_id: Option<String>,
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
    /// List tasks without a goal.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub no_goal: bool,
    /// Filter by goal id.
    pub goal: Option<String>,
    /// Filter by status.
    pub status: Option<TaskStatus>,
    /// The task that runs this far task of another daemon (037), in whatever
    /// goal and status: one task at most.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin_task_id: Option<String>,
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

/// Durable source identity. Connection aliases and tunnel addresses are not identities.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceGoalReference {
    pub daemon_id: String,
    pub goal_id: String,
    pub title: String,
    pub description: String,
}

/// Saved workflow columns; catalog edits do not change these columns.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TaskWorkflowContext {
    pub name: String,
    pub steps: Vec<crate::workflows::WorkflowStepDto>,
}

/// Worker context sufficient for task inspection without a local goal.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TaskExecutionContext {
    /// Null for a task that has no goal.
    pub source_goal: Option<SourceGoalReference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// Source identity for goal-less work. Goal work uses source_goal.daemon_id.
    pub source_daemon_id: Option<String>,
    /// Persistent daemon ID of the worker that owns execution.
    pub execution_owner: String,
    pub workflow: TaskWorkflowContext,
    /// Draft edits advance this revision. Authorization must name this revision.
    pub revision: i64,
    pub authorized: bool,
    /// The source-authorized revision. It remains stable through task retries.
    pub authorized_revision: Option<i64>,
}

/// Repeatable creation of a blocked worker draft. Use PATCH /v1/tasks/{id} to edit it.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateRemoteTaskRequest {
    pub repo_id: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    /// Null for a task that has no goal.
    pub source_goal: Option<SourceGoalReference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// Source identity for goal-less work. Goal work uses source_goal.daemon_id.
    pub source_daemon_id: Option<String>,
    pub workflow: TaskWorkflowContext,
    pub agents: Vec<AgentAssignment>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AuthorizeRemoteTaskRequest {
    pub revision: i64,
}

/// Explicit protocol negotiation, independent of application version.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RemoteTaskProtocol {
    /// Negotiated optional operations. New source references require source-dependency-controls-v1.
    #[serde(default)]
    pub capabilities: Vec<String>,
    pub protocol: String,
    pub daemon_id: String,
}

/// Fail one unauthorized draft revision after a source dependency failure.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct FailRemoteDependencyRequest {
    pub revision: i64,
    pub operation_id: String,
    pub reason: String,
}

/// Retry the identified dependency failure into a newer blocked draft revision.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RetryRemoteDependencyRequest {
    pub revision: i64,
    pub operation_id: String,
}

/// Both fenced source dependency operations are required for new source references.
pub const SOURCE_DEPENDENCY_CONTROLS: &str = "source-dependency-controls-v1";

/// Create user-owned local work with its own saved columns.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateStandaloneTaskRequest {
    pub title: String,
    #[serde(default)]
    pub description: String,
    pub repo_id: String,
    /// The workflow the task runs. Omitted or null = no workflow: `agents`
    /// holds exactly one agent with no `step`, and its step calls end the task.
    #[serde(default)]
    pub workflow: Option<String>,
    pub agents: Vec<AgentAssignment>,
    pub host: Option<String>,
}
