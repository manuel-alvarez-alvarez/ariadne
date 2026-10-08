//! Normalized forge data and the single path from a forge URL to a ledger row.
use super::PullRequestRef;
use ariadne_store::{ForgeIntegration, NewPullRequest, NewPullRequestComment, PullRequest, Store};

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

pub(crate) async fn record(
    store: &Store,
    integration: &ForgeIntegration,
    pull: ForgePullRequest,
    tracked_by: &str,
    origin_task_id: Option<String>,
    existing_id: Option<String>,
) -> Result<(PullRequest, bool), String> {
    let reference = PullRequestRef::parse(&pull.url, integration)
        .filter(|reference| reference.number == pull.number)
        .ok_or_else(|| "the forge returned a pull request outside this repository".to_string())?;
    store
        .upsert_pull_request(NewPullRequest {
            existing_id,
            repository_id: reference.repository_id,
            number: reference.number,
            url: pull.url,
            title: pull.title,
            body: pull.body,
            author_login: pull.author_login,
            tracked_by: tracked_by.into(),
            state: pull.state,
            draft: pull.draft,
            head_branch: pull.head_branch,
            head_sha: pull.head_sha,
            head_repo: pull.head_repo,
            base_branch: pull.base_branch,
            checks: pull.checks,
            review_decision: pull.review_decision,
            origin_task_id,
            opened_at: pull.opened_at,
            merge_sha: pull.merge_sha,
        })
        .await
        .map_err(|error| error.to_string())
}
