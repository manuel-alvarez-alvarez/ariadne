//! Pull request ledger wire types (026).
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

/// A pull request as the forge holds it now, read live (026), with what
/// Ariadne keeps of it where Ariadne works on it: the request a task opened,
/// one that asks for the user's review on a repository with a review pin,
/// or one of the user's they asked Ariadne to review.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PullRequestDto {
    /// Ariadne's id of the request while it works on it; null on a request
    /// nobody works on, which the forge alone holds.
    pub id: Option<String>,
    pub repository_id: String,
    pub number: i64,
    pub url: String,
    pub title: String,
    /// The request's description.
    #[serde(default)]
    pub body: String,
    pub author_login: String,
    /// `open`, `merged` or `closed`.
    pub state: String,
    pub draft: bool,
    pub head_branch: String,
    pub head_sha: String,
    pub head_repo: Option<String>,
    pub base_branch: String,
    /// The rolled-up checks: `pending`, `success`, `failure` or `none`.
    pub checks: String,
    pub review_decision: String,
    pub opened_at: String,
    /// When the forge last saw the request move.
    pub updated_at: String,
    /// `author` for a request of the user's, `reviewer` for any other.
    pub role: String,
    /// Whether the request asks for the user's review (029).
    #[serde(default)]
    pub review_requested: bool,
    /// The threads that wait on the user's login. Read where Ariadne works
    /// on the request, or where the request is read on its own; 0 on a row
    /// of a list nobody works on.
    #[serde(default)]
    pub unanswered_comments: i64,
    /// The checks that failed on the head, read as `unanswered_comments` is.
    #[serde(default)]
    pub failed_checks: Vec<FailedCheckDto>,
    /// Whether the base branch has commits the head does not.
    #[serde(default)]
    pub behind_base: bool,
    /// The task that opened the request: its author keeps it (005).
    #[serde(default)]
    pub origin_task_id: Option<String>,
    /// Whether the request's session reported it ready to merge.
    #[serde(default)]
    pub ready: bool,
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
    /// The newest session on this request, if any: its review session
    /// (029), or the author of the task that opened it (005).
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

/// One comment on a request, as the forge holds it now, read live (026),
/// with the marks Ariadne keeps of it.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PullRequestCommentDto {
    /// The forge's id of the comment, `rc-<n>`, `ic-<n>`, `rv-<n>` or
    /// `note-<n>`: what `reply` and `resolve` take.
    pub id: String,
    pub pull_request_id: String,
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
    /// A later comment in the thread is by the integration login.
    pub answered: bool,
    /// The forge reports the thread resolved.
    pub resolved: bool,
    /// When the request's session was told of it.
    pub told_at: Option<String>,
    /// An Ariadne review session posted it (029).
    #[serde(default)]
    pub from_review: bool,
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

/// Body of `PUT /v1/repositories/{id}/pull-requests/{number}/ariadne-review`:
/// whether Ariadne
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

/// Query of `GET /v1/pull-requests`: the open requests of the enabled
/// repositories, read live off the forge, or with `task` the request a
/// task opened.
#[derive(Debug, Clone, Default, Serialize, Deserialize, IntoParams)]
pub struct PullRequestListQuery {
    pub repo: Option<String>,
    /// `reviewer` for the requests of others, `author` for the user's own.
    pub role: Option<String>,
    /// The task that opened the request.
    pub task: Option<String>,
    /// True for the requests that ask for the user's review (029), false
    /// for the ones that do not.
    pub requested: Option<bool>,
}

/// An open request a search found that is not the user's own.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PullRequestMatchDto {
    pub number: i64,
    pub url: String,
    pub title: String,
    pub author_login: String,
    /// Whether Ariadne works on it.
    pub tracked: bool,
}
