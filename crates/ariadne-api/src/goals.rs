//! Goal DTOs.

use ariadne_core::GoalStatus;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct GoalRepoDto {
    pub id: String,
    pub path: String,
    pub base_branch: String,
}

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
    pub repos: Vec<GoalRepoDto>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RepoSpec {
    /// Absolute path to an existing git repository.
    #[schema(example = "/home/me/projects/webapp")]
    pub path: String,
    /// Base branch tasks merge into; defaults to the repo's current branch.
    pub base_branch: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateGoalRequest {
    pub title: String,
    #[serde(default)]
    pub description: String,
    pub repos: Vec<RepoSpec>,
    /// Planner profile id or unique name.
    pub planner_profile: String,
    /// Max tasks the planner may create (default: unbounded).
    pub max_tasks: Option<i64>,
    /// Approvals required to merge a task (default 1).
    pub required_approvals: Option<i64>,
}
