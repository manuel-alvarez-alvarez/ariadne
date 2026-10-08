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
    /// The request's description, as the forge holds it.
    #[serde(default)]
    pub body: String,
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
    /// Whether the request asks for the user's review (029).
    #[serde(default)]
    pub review_requested: bool,
    /// Whether the user asked Ariadne to review this request of their own
    /// (029).
    #[serde(default)]
    pub review_asked: bool,
    /// The pin the user picked for that review.
    #[serde(default)]
    pub review_model: Option<String>,
    #[serde(default)]
    pub review_effort: Option<String>,
    /// The skills that review loads beside `pr-reviewer`.
    #[serde(default)]
    pub review_skills: Vec<String>,
    /// The newest session on this request, if any: the review session of a
    /// request that asks for the user's review (029), or the author of the
    /// task that opened it (005).
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
    /// The head a reviewer session posted its review on (029).
    #[serde(default)]
    pub reviewed_sha: Option<String>,
}

/// Query of `GET /v1/pull-requests/{id}/diff` (029).
#[derive(Debug, Clone, Default, Serialize, Deserialize, IntoParams)]
pub struct PullRequestDiffQuery {
    /// Read the diff from this sha to the head, not from the base.
    pub since: Option<String>,
}

/// Body of `POST /v1/pull-requests/{id}/reviews` (029): one review, posted
/// in the name of the integration login.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SubmitReviewRequest {
    /// `request_changes` or `comment`. Every other event is refused.
    pub event: String,
    /// The review's summary as it stands now: the daemon writes it into the
    /// one summary comment the review keeps on the request.
    pub body: String,
    #[serde(default)]
    pub comments: Vec<ReviewCommentRequest>,
}

/// One inline comment of a review (029).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ReviewCommentRequest {
    pub path: String,
    /// The line in the new version of the file.
    pub line: i64,
    /// A short title of the defect; the comment opens on `[P0] Title`.
    #[serde(default)]
    pub title: String,
    /// What goes wrong, and how to fix it.
    pub body: String,
    /// `P0`, `P1` or `P2`.
    pub priority: String,
}

/// Body of `PUT /v1/pull-requests/{id}/ariadne-review`: whether Ariadne
/// reviews a request of the user's own (029).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AskReviewRequest {
    pub asked: bool,
    /// The model the review runs on, `agent:model`: required to ask, and
    /// checked against the catalog as any pin is.
    #[serde(default)]
    pub model: Option<String>,
    /// The effort that model runs at; none for the agent's own.
    #[serde(default)]
    pub effort: Option<String>,
    /// The skills the review loads beside `pr-reviewer`, which every review
    /// session loads; skills a task agent is staffed on.
    #[serde(default)]
    pub skills: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, IntoParams)]
pub struct PullRequestListQuery {
    pub repo: Option<String>,
    /// `reviewer` for the requests that ask for the user's review, `author`
    /// for the ones a task opened.
    pub role: Option<String>,
    /// The task that opened the request.
    pub task: Option<String>,
    /// True for the requests that ask for the user's review (029), false
    /// for the ones that do not.
    pub requested: Option<bool>,
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

/// An open request a search found that is not the user's own: a request of
/// the user's own is kept by the task that opened it (005), and is no
/// request to add by hand.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PullRequestMatchDto {
    pub number: i64,
    pub url: String,
    pub title: String,
    pub author_login: String,
    pub tracked: bool,
}
