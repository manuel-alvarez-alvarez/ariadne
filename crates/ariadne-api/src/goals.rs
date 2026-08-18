//! Goal DTOs.

use ariadne_core::GoalStatus;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::repositories::RepositoryDto;

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
    /// The registered repositories the goal works in, as they stand now: a
    /// goal references them, so an edit to one shows up here.
    pub repos: Vec<RepositoryDto>,
    pub created_at: String,
    pub updated_at: String,
}

/// Body of `POST /v1/goals/{id}/finalize`: planning ends, execution starts.
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
}
