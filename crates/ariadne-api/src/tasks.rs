//! Task DTOs.

use ariadne_core::TaskStatus;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

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
    /// Reviewer profile ids in planner-assigned order.
    pub reviewer_profile_ids: Vec<String>,
    /// Ids of tasks that must merge before this one starts.
    pub depends_on: Vec<String>,
    pub branch: String,
    pub worktree_path: Option<String>,
    pub review_round: i64,
    /// Set when the agent went idle without advancing the task.
    pub stalled: bool,
    pub merge_commit: Option<String>,
    pub created_at: String,
    pub updated_at: String,
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
    /// Reviewer profile ids or names, in review order. At least one.
    pub reviewer_profiles: Vec<String>,
    /// Task ids this task depends on.
    #[serde(default)]
    pub depends_on: Vec<String>,
}

/// Partial update; only allowed while the task is pending/ready.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct UpdateTaskRequest {
    pub title: Option<String>,
    pub description: Option<String>,
    pub reviewer_profiles: Option<Vec<String>>,
    pub depends_on: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TransitionRequest {
    pub to: TaskStatus,
    pub reason: Option<String>,
    /// Required when `to` is `merged`.
    pub merge_commit: Option<String>,
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
