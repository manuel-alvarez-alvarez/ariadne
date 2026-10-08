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
    /// The checks that failed on the head, as the last detail fetch read
    /// them.
    #[serde(default)]
    pub failed_checks: Vec<FailedCheckDto>,
    /// Whether the base branch has commits the head does not.
    #[serde(default)]
    pub behind_base: bool,
    /// The newest session the daemon started on this request, if any.
    #[serde(default)]
    pub session_id: Option<String>,
}

/// One check that failed on a request's head.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct FailedCheckDto {
    pub name: String,
    /// Where the forge shows the check's run; empty where it names none.
    pub url: String,
    /// The forge's own word for how it ended: `failure`, `cancelled` and
    /// the like.
    pub conclusion: String,
}

/// One comment on a request, as the daemon stored it from the forge.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PullRequestCommentDto {
    /// The daemon's own id: what `reply` takes.
    pub id: String,
    pub pull_request_id: String,
    /// The forge's id of the comment.
    pub forge_id: String,
    /// The forge's thread or discussion; the conversation of the request
    /// is one thread.
    pub thread_id: String,
    /// `review_comment`, `issue_comment` or `review`.
    pub kind: String,
    pub author_login: String,
    pub author_is_bot: bool,
    pub body: String,
    pub path: Option<String>,
    pub line: Option<i64>,
    pub in_reply_to: Option<String>,
    pub created_at: String,
    pub fetched_at: String,
    /// A later comment in the thread is by the integration login.
    pub answered: bool,
    /// The forge reports the thread resolved.
    pub resolved: bool,
    pub told_at: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, IntoParams)]
pub struct PullRequestCommentQuery {
    /// Only the threads that wait on an answer from the integration login.
    pub unanswered_only: Option<bool>,
}

/// Body of `POST /v1/pull-requests/{id}/comments/{comment_id}/reply`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ReplyCommentRequest {
    pub body: String,
}

/// Body of `POST /v1/pull-requests/{id}/report`: what the request's session
/// says of it.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ReportPullRequestRequest {
    /// Every required approval and check reads green.
    pub ready: Option<bool>,
    /// `open`, `merged` or `closed`.
    pub state: Option<String>,
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
