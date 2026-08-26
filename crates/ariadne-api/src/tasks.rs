//! Task DTOs.

use ariadne_core::{AgentKind, TaskStatus};
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
    pub engineer_profile_id: String,
    /// Name of the engineer's profile, the way a message addresses it; None
    /// only if that profile is gone.
    pub engineer_profile_name: Option<String>,
    /// Name of the planner profile of the task's goal, which takes part in
    /// every task thread without being a field of the task.
    pub planner_profile_name: Option<String>,
    /// Agent CLI the engineer runs on: pinned when the task was created, from
    /// the agent chosen for it at creation or on an edit or, where none was,
    /// from the engineer profile. Editing the profile afterwards leaves it
    /// alone. None = auto, resolved at spawn time to the first installed CLI.
    pub agent_kind: Option<AgentKind>,
    /// Model the engineer runs on, pinned alongside `agent_kind`. None = the
    /// agent CLI's own default model.
    pub model: Option<String>,
    /// Reviewer slots in planner-assigned order, each carrying its own pin.
    pub reviewers: Vec<TaskReviewerDto>,
    /// Ids of tasks that must merge before this one starts.
    pub depends_on: Vec<String>,
    pub branch: String,
    pub worktree_path: Option<String>,
    pub review_round: i64,
    /// Set when the agent went idle without advancing the task.
    pub stalled: bool,
    pub merge_commit: Option<String>,
    /// URL of the pull or merge request the task was published as, once its
    /// engineer has reported one; None for a task landed directly.
    pub pr_url: Option<String>,
    /// What the agents of this task have spent between them.
    pub usage: TaskUsageDto,
    pub created_at: String,
    pub updated_at: String,
}

/// What a task cost, by who spent it: its engineer, its reviewers one entry
/// each, and the total of every session on the task.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct TaskUsageDto {
    /// Every session on the task summed, whatever its role.
    pub total: TokenUsageDto,
    /// The engineer's own, across every run of it.
    pub engineer: TokenUsageDto,
    /// One entry per reviewer profile that has a session on the task, every
    /// review round of it summed, ordered like `reviewers`. A reviewer whose
    /// session has yet to report anything is listed with zeros; one that has
    /// never been spawned is not listed at all.
    pub reviewers: Vec<ProfileUsageDto>,
}

/// What one profile spent on a task, named the way a reader addresses it.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ProfileUsageDto {
    pub profile_id: String,
    /// The profile's name; None only if that profile is gone.
    pub profile_name: Option<String>,
    pub usage: TokenUsageDto,
}

/// One reviewer slot of a task: which profile reviews it, and what that
/// reviewer was pinned to run on when the slot was assigned — the profile's
/// own agent and model, or the agent chosen for the slot. Pinned the same way
/// the engineer is, and read the same way: what a reviewer of this task runs
/// on, not what its profile says today.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TaskReviewerDto {
    pub profile_id: String,
    /// Name of the reviewer's profile, the way a message addresses it; None
    /// only if that profile is gone.
    pub profile_name: Option<String>,
    /// None = auto, resolved at spawn time to the first installed CLI.
    pub agent_kind: Option<AgentKind>,
    /// None = the agent CLI's own default model.
    pub model: Option<String>,
}

/// One reviewer of a task: the profile that reviews, and what it is to run on.
///
/// The agent CLI is the choice and the model narrows it: an `agent_kind` alone
/// runs that CLI on its own default model, an `agent_kind` with a `model` pins
/// both, and a `model` with no `agent_kind` is refused — nothing here derives
/// one from the other. Omitted, the slot takes the profile's own agent and
/// model as they stand when it is assigned.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ReviewerAssignment {
    /// Reviewer profile id or unique name.
    pub profile: String,
    /// Agent CLI this reviewer runs on; omitted = the profile's own.
    #[serde(default)]
    pub agent_kind: Option<AgentKind>,
    /// Model it runs on there; omitted (or "default") = the CLI's own default
    /// model. Free text, handed to the CLI as typed.
    #[serde(default)]
    pub model: Option<String>,
}

impl ReviewerAssignment {
    /// A reviewer on whatever its profile is on.
    pub fn of(profile: impl Into<String>) -> Self {
        Self {
            profile: profile.into(),
            agent_kind: None,
            model: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateTaskRequest {
    pub title: String,
    #[serde(default)]
    pub description: String,
    /// Id of one of the goal's repositories; may be omitted when the goal
    /// works in exactly one.
    pub repo_id: Option<String>,
    /// Engineer profile id or unique name.
    pub engineer_profile: String,
    /// Agent CLI the engineer runs on; omitted = the engineer profile's own
    /// agent and model. Resolved the way [`ReviewerAssignment::agent_kind`]
    /// is.
    #[serde(default)]
    pub agent_kind: Option<AgentKind>,
    /// Model the engineer runs on there; omitted (or "default") = that CLI's
    /// own default model. A model without an `agent_kind` is refused.
    #[serde(default)]
    pub model: Option<String>,
    /// The reviewers of the task, in review order. At least one.
    pub reviewers: Vec<ReviewerAssignment>,
    /// Task ids this task depends on.
    #[serde(default)]
    pub depends_on: Vec<String>,
}

/// Partial update; only allowed while the task is pending/ready.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct UpdateTaskRequest {
    pub title: Option<String>,
    pub description: Option<String>,
    /// Agent CLI the engineer runs on: absent leaves the task's pins alone,
    /// "default" (or the empty string) puts them back on the engineer
    /// profile's agent and model as they stand now, and a kind pins that kind
    /// — together with `model` where one is given. The same convention
    /// `UpdateProfileRequest::agent_kind` takes for its "auto".
    pub agent_kind: Option<String>,
    /// Model the engineer runs on there; "default" (or the empty string) is
    /// the agent CLI's own default model. A model with no `agent_kind` beside
    /// it is refused, as it is on a creation.
    pub model: Option<String>,
    /// The whole reviewer list, replaced: each slot is cut afresh and pinned
    /// to its own agent and model or, where it names none, to its profile's.
    pub reviewers: Option<Vec<ReviewerAssignment>>,
    pub depends_on: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TransitionRequest {
    pub to: TaskStatus,
    pub reason: Option<String>,
    /// Required when `to` is `merged`.
    pub merge_commit: Option<String>,
}

/// The engineer reporting the pull or merge request it opened for a task, so
/// the user has somewhere to go and read it: taken off `gh pr create`'s output
/// rather than out of the conversation.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
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
