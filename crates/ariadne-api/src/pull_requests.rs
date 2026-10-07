//! Pull request ledger wire types (026).
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PullRequestDto {
    pub id: String,
    pub repository_id: String,
    pub number: i64,
    pub url: String,
    pub title: String,
    pub author_login: String,
    pub tracked_by: String,
    pub state: String,
    pub draft: bool,
    pub head_branch: String,
    pub head_sha: String,
    pub head_repo: Option<String>,
    pub base_branch: String,
    pub checks: String,
    pub review_decision: String,
    pub unanswered_comments: i64,
    pub origin_task_id: Option<String>,
    pub opened_at: String,
    pub role: String,
    pub ready: bool,
    pub last_seen_at: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, IntoParams)]
pub struct PullRequestListQuery {
    pub repo: Option<String>,
    pub role: Option<String>,
    /// Open by default; `all` includes closed and merged requests.
    pub state: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AddPullRequestRequest {
    pub repository_id: Option<String>,
    pub number: Option<i64>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PullRequestMatchDto {
    pub number: i64,
    pub url: String,
    pub title: String,
    pub author_login: String,
    pub role: String,
    pub tracked: bool,
}
