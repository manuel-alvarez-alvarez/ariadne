//! Task DTOs.

use ariadne_core::TaskStatus;
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
    /// What the engineer runs on, `<agent_kind>[:<model>]`: the agent CLI and,
    /// after a `:`, the model of it (`codex`, `claude_code:claude-opus-5`).
    /// Pinned when the task was created, from the model chosen for it at
    /// creation or on an edit or, where none was, from the engineer profile —
    /// editing the profile afterwards leaves it alone. None = auto: the first
    /// installed CLI, resolved at spawn time, on its own default model.
    #[schema(example = "claude_code:claude-opus-5")]
    pub model: Option<String>,
    /// The reasoning effort that model is run at, pinned like `model`. None =
    /// whatever the agent CLI runs it at on its own.
    #[schema(example = "xhigh")]
    pub effort: Option<String>,
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
/// own model, or the one chosen for the slot. Pinned the same way the engineer
/// is, and read the same way: what a reviewer of this task runs on, not what
/// its profile says today.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TaskReviewerDto {
    pub profile_id: String,
    /// Name of the reviewer's profile, the way a message addresses it; None
    /// only if that profile is gone.
    pub profile_name: Option<String>,
    /// What this reviewer runs on, `<agent_kind>[:<model>]`. None = auto: the
    /// first installed CLI, resolved at spawn time, on its own default model.
    #[schema(example = "codex:o3")]
    pub model: Option<String>,
    /// The reasoning effort that model is run at, pinned like `model`. None =
    /// whatever the agent CLI runs it at on its own.
    #[schema(example = "high")]
    pub effort: Option<String>,
}

/// One reviewer of a task: the profile that reviews, and what it is to run on.
///
/// The model is written `<agent_kind>[:<model>]`: the agent CLI on its own
/// runs it on its own default model, an agent with a model after the `:` pins
/// both, and a string naming no agent CLI is refused — nothing here derives
/// one from the other. Omitted, the slot takes the profile's own model as it
/// stands when the slot is assigned.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ReviewerAssignment {
    /// Reviewer profile id or unique name.
    pub profile: String,
    /// What this reviewer runs on, `<agent_kind>[:<model>]`; omitted (or
    /// "default") = the profile's own.
    #[serde(default)]
    #[schema(example = "codex:o3")]
    pub model: Option<String>,
    /// The reasoning effort to run that model at, one of the efforts
    /// `GET /v1/models` lists for it; anything else is refused. Omitted (or
    /// "default") = whatever the agent CLI runs the model at, and where
    /// `model` is omitted too, the profile's own effort. Named where `model`
    /// is omitted, the slot takes the profile's own model at this effort.
    #[serde(default)]
    #[schema(example = "high")]
    pub effort: Option<String>,
}

impl ReviewerAssignment {
    /// A reviewer on whatever its profile is on.
    pub fn of(profile: impl Into<String>) -> Self {
        Self {
            profile: profile.into(),
            model: None,
            effort: None,
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
    /// What the engineer runs on, `<agent_kind>[:<model>]`; omitted (or
    /// "default") = the engineer profile's own model. Resolved the way
    /// [`ReviewerAssignment::model`] is.
    #[serde(default)]
    #[schema(example = "codex:gpt-5.3-codex")]
    pub model: Option<String>,
    /// The reasoning effort to run that model at, resolved and refused the
    /// way [`ReviewerAssignment::effort`] is.
    #[serde(default)]
    #[schema(example = "xhigh")]
    pub effort: Option<String>,
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
    /// What the engineer runs on, `<agent_kind>[:<model>]`: absent leaves the
    /// task's pins alone, "default" (or the empty string) puts them back on
    /// the engineer profile's own model as it stands now, and anything else
    /// pins what it spells. The same clearing word
    /// [`crate::profiles::UpdateProfileRequest::model`] takes.
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
    /// The whole reviewer list, replaced: each slot is cut afresh and pinned
    /// to the model it names or, where it names none, to its profile's.
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
