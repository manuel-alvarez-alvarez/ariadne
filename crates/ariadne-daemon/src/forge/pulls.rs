//! Normalized forge data, and the one path from a forge read to the row of
//! a request Ariadne starts to work on.
use super::PullRequestRef;
use ariadne_store::{
    ForgeIntegration, NewPullRequest, NewPullRequestComment, PullRequestRow, Store,
};

/// What a detail fetch reads of one request beyond the list fetch's
/// fields (026): the request as `pull_request` reads it, every comment with
/// its thread, the checks that failed on the head, and whether the head is
/// behind its base.
#[derive(Debug, Clone)]
pub struct ForgeDetails {
    pub pull: ForgePullRequest,
    pub comments: Vec<NewPullRequestComment>,
    pub failed_checks: Vec<FailedCheck>,
    pub behind_base: bool,
}

/// One check that failed on a request's head, as `pull_requests.failed_checks`
/// stores it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FailedCheck {
    pub name: String,
    pub url: String,
    pub conclusion: String,
}

/// One review a reviewer session posts in the name of the integration
/// login (029): its verdict and its inline findings. Its summary is not
/// part of it: the review keeps one summary comment on the request, written
/// with `write_summary` and edited on every round. The forge takes no
/// approval from it: the user gives every approval.
#[derive(Debug, Clone)]
pub struct ReviewDraft {
    /// Ask for changes; else the review is a comment.
    pub request_changes: bool,
    /// The head the review is on.
    pub head_sha: String,
    pub comments: Vec<DraftComment>,
}

/// One inline comment of a [`ReviewDraft`], its body already led by its
/// priority.
#[derive(Debug, Clone)]
pub struct DraftComment {
    pub path: String,
    pub line: i64,
    pub body: String,
}

/// The thread every comment on a request's conversation is in: a comment
/// on the request and a review body are flat on both forges, so a reply to
/// any of them answers the conversation.
pub(crate) const CONVERSATION: &str = "conversation";

/// What every comment an Ariadne review posts ends on (029): an HTML
/// comment, which the forge renders as nothing. It is what tells the
/// review's comments from the user's own, which share the login, and it
/// lives on the forge with the comment: a review stopped and started again,
/// whose row went in between, still knows its own findings.
pub(crate) const REVIEW_MARK: &str = "<!-- ariadne:review -->";

/// What the review's one summary comment ends on beside [`REVIEW_MARK`]: how
/// a review that has no record of its summary finds it again, to edit it
/// rather than post a second.
pub(crate) const SUMMARY_MARK: &str = "<!-- ariadne:review-summary -->";

/// Whether a forge call failed because what it named is not there: `gh`
/// and `glab` both say "404" for it. Anything else — a server error, a lost
/// answer — is no proof the thing is gone.
pub(crate) fn is_missing(error: &str) -> bool {
    error.contains("HTTP 404") || error.contains("404 Not Found")
}

/// `body` signed as a review's, or as its summary's.
pub(crate) fn signed(body: &str, summary: bool) -> String {
    match summary {
        true => format!("{body}\n\n{SUMMARY_MARK}\n{REVIEW_MARK}"),
        false => format!("{body}\n\n{REVIEW_MARK}"),
    }
}

/// `body` without the marks a review signs it with: what a reader is shown.
pub(crate) fn unsigned(body: &str) -> String {
    body.replace(SUMMARY_MARK, "")
        .replace(REVIEW_MARK, "")
        .trim_end()
        .to_string()
}

/// A forge repository as the fetch names it, `host/owner/name`, split into
/// its three parts. On GitLab the owner can be a group path.
pub(crate) fn split_slug(slug: &str) -> Result<(&str, &str, &str), String> {
    let (host, path) = slug
        .split_once('/')
        .ok_or_else(|| format!("{slug} names no forge host"))?;
    let (owner, name) = path
        .rsplit_once('/')
        .ok_or_else(|| format!("{slug} names no owner"))?;
    Ok((host, owner, name))
}

#[derive(Debug, Clone)]
pub struct ForgePullRequest {
    pub number: i64,
    pub url: String,
    pub title: String,
    /// The request's description; empty where it has none.
    pub body: String,
    pub author_login: String,
    pub state: String,
    pub draft: bool,
    pub head_branch: String,
    pub head_sha: String,
    pub head_repo: Option<String>,
    pub base_branch: String,
    pub checks: String,
    pub review_decision: String,
    pub opened_at: String,
    /// When the forge last saw the request move.
    pub updated_at: String,
    /// The commit a merged request landed as; None on an open one.
    pub merge_sha: Option<String>,
}

/// What a repository fetch lists: every open request, and the numbers of
/// the ones that ask for the user's review (029).
#[derive(Debug, Clone, Default)]
pub struct Listed {
    pub open: Vec<ForgePullRequest>,
    pub requested: std::collections::HashSet<i64>,
}

pub(crate) fn role(author: &str, integration: &ForgeIntegration) -> &'static str {
    if integration
        .login
        .as_deref()
        .is_some_and(|login| author.eq_ignore_ascii_case(login))
    {
        "author"
    } else {
        "reviewer"
    }
}

/// Start to work on `pull`, a request of `integration`'s repository: the
/// row that keeps Ariadne's bookkeeping of it, created where none is (026),
/// with the task that opened it where one did. Answers the row, and whether
/// it is new.
pub(crate) async fn start_work(
    store: &Store,
    integration: &ForgeIntegration,
    pull: &ForgePullRequest,
    origin_task_id: Option<String>,
) -> Result<(PullRequestRow, bool), String> {
    let reference = PullRequestRef::parse(&pull.url, integration)
        .filter(|reference| reference.number == pull.number)
        .ok_or_else(|| "the forge returned a pull request outside this repository".to_string())?;
    store
        .upsert_pull_request(NewPullRequest {
            repository_id: reference.repository_id,
            number: reference.number,
            url: pull.url.clone(),
            role: role(&pull.author_login, integration).into(),
            origin_task_id,
        })
        .await
        .map_err(|error| error.to_string())
}
