//! Task DTOs.

use ariadne_core::{Landing, Seat, TaskStatus};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::usage::TokenUsageDto;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TaskDto {
    pub id: String,
    pub goal_id: String,
    /// Id of the repository the task works in, one of its goal's.
    pub repo_id: String,
    pub title: String,
    pub description: String,
    pub status: TaskStatus,
    /// The agents staffed on the task: the author first, then the reviewers
    /// in review order. What each one can do is the skills it carries.
    pub agents: Vec<TaskAgentDto>,
    /// Ids of tasks that must merge before this one starts.
    pub depends_on: Vec<String>,
    pub branch: String,
    /// How the task ends: a change on the base branch, a request somebody
    /// else merges, or nothing at all.
    pub landing: Landing,
    pub worktree_path: Option<String>,
    /// Set when the agent went idle without advancing the task.
    pub stalled: bool,
    pub merge_commit: Option<String>,
    /// URL of the pull or merge request the task was published as, once its
    /// author has reported one; None for a task landed directly.
    pub pr_url: Option<String>,
    /// Why a `failed` or `cancelled` task ended — the author's own
    /// `fail_task` reason, a dependency that never landed, a cancelled goal.
    /// None for every other status, and for an ending nobody gave a reason
    /// for.
    pub reason: Option<String>,
    /// What the agents of this task have spent between them.
    pub usage: TaskUsageDto,
    pub created_at: String,
    pub updated_at: String,
}

/// What a task cost, by who spent it: its author, its reviewers one entry
/// each, and the total of every session on the task.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct TaskUsageDto {
    /// Every session on the task summed, whatever its seat.
    pub total: TokenUsageDto,
    /// The author's own, across every run of it.
    pub author: TokenUsageDto,
    /// One entry per reviewer that has a session on the task, every review
    /// round of it summed, in review order. A reviewer whose session has yet
    /// to report anything is listed with zeros; one that has never been
    /// spawned is not listed at all.
    pub reviewers: Vec<AgentUsageDto>,
}

/// What one staffed agent spent on a task, named the way a reader addresses
/// it: an agent has no name of its own, so its skills are what identify it.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AgentUsageDto {
    pub agent_id: String,
    /// The skills the agent loads; empty only if the agent is gone.
    pub skills: Vec<String>,
    pub usage: TokenUsageDto,
}

/// One agent staffed on a task: where it sits, what it knows, and what it
/// runs on.
///
/// The agent has no identity of its own. `seat` says only whether it authors
/// the task or reviews it; the skills are what it can do. What it runs on was
/// sized by the orchestrator when it staffed the task, or chosen by the user
/// since — either way it is what this agent runs on, and nothing behind it
/// changes that.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TaskAgentDto {
    pub id: String,
    /// `author` or `reviewer`.
    pub seat: Seat,
    /// The skills this agent loads, in the order they reach it.
    #[schema(example = json!(["coding", "testing"]))]
    pub skills: Vec<String>,
    /// What this agent runs on, `<agent_kind>[:<model>]`. None = auto: the
    /// first installed CLI, resolved at spawn time, on its own default model.
    #[schema(example = "codex:o3")]
    pub model: Option<String>,
    /// The reasoning effort that model is run at. None = whatever the agent
    /// CLI runs it at on its own.
    #[schema(example = "high")]
    pub effort: Option<String>,
    /// What the orchestrator told this agent beyond the task itself. None =
    /// the task is the whole of it.
    pub brief: Option<String>,
}

/// One agent to staff on a task: where it sits, the skills it loads, and what
/// it is to run on.
///
/// The model is written `<agent_kind>[:<model>]`: the agent CLI on its own
/// runs it on its own default model, an agent with a model after the `:` pins
/// both, and a string naming no agent CLI is refused — nothing here derives
/// one from the other. Omitted, the agent runs on the first installed CLI at
/// spawn time, on that CLI's own default model.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AgentAssignment {
    /// `author` or `reviewer`. A task takes exactly one author.
    pub seat: Seat,
    /// The names of the skills this agent loads, in the order they reach it.
    /// A name no skill answers to is refused.
    #[serde(default)]
    #[schema(example = json!(["coding", "testing"]))]
    pub skills: Vec<String>,
    /// What this agent runs on, `<agent_kind>[:<model>]`; omitted (or
    /// "default") = auto.
    #[serde(default)]
    #[schema(example = "codex:o3")]
    pub model: Option<String>,
    /// The reasoning effort to run that model at, one of the efforts
    /// `GET /v1/models` lists for it; anything else is refused. Omitted (or
    /// "default") = whatever the agent CLI runs the model at.
    #[serde(default)]
    #[schema(example = "high")]
    pub effort: Option<String>,
    /// What to tell this agent beyond the task itself. Omitted = the task is
    /// the whole of it.
    #[serde(default)]
    pub brief: Option<String>,
}

impl AgentAssignment {
    /// An agent in `seat` on the named skills, on auto.
    pub fn new(seat: Seat, skills: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            seat,
            skills: skills.into_iter().map(Into::into).collect(),
            model: None,
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
    /// The agents to staff: exactly one author, then the reviewers in review
    /// order, at least one.
    pub agents: Vec<AgentAssignment>,
    /// Task ids this task depends on.
    #[serde(default)]
    pub depends_on: Vec<String>,
    /// How the task ends. Omitted = the way its repository takes a change.
    #[serde(default)]
    pub landing: Option<Landing>,
}

/// Partial update; only allowed while the task is pending/ready.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateTaskRequest {
    pub title: Option<String>,
    pub description: Option<String>,
    /// What the author runs on, `<agent_kind>[:<model>]`: absent leaves the
    /// author's pins alone, "default" (or the empty string) puts them back on
    /// auto, and anything else pins what it spells.
    #[schema(example = "codex:gpt-5.3-codex")]
    pub model: Option<String>,
    /// The reasoning effort to run the model at: absent leaves it alone,
    /// "default" (or the empty string) puts it back on whatever the agent CLI
    /// runs the model at, and anything else is checked against the model it
    /// will run at — the one this request names, or the task's own where it
    /// names none — and refused where that model does not take it. A `model`
    /// written without an effort runs at the CLI's own default: the effort
    /// belonged to the model that was left behind.
    #[schema(example = "xhigh")]
    pub effort: Option<String>,
    /// The whole reviewer list, replaced: every reviewer is staffed afresh,
    /// with the skills and the model it names.
    pub reviewers: Option<Vec<AgentAssignment>>,
    pub depends_on: Option<Vec<String>>,
    /// How the task ends. Absent leaves it where it is.
    pub landing: Option<Landing>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TransitionRequest {
    pub to: TaskStatus,
    pub reason: Option<String>,
    /// Required when `to` is `finished`, unless the task lands nothing.
    pub merge_commit: Option<String>,
}

/// The author reporting the pull or merge request it opened for a task, so
/// the user has somewhere to go and read it: taken off `gh pr create`'s output
/// and recorded on the task.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RecordPullRequestRequest {
    /// The request's URL, e.g. `https://github.com/owner/repo/pull/12`.
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TaskTransitionDto {
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
