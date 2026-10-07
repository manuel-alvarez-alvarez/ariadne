//! Normalized forge data and the single path from a forge URL to a ledger row.
use super::PullRequestRef;
use ariadne_store::{ForgeIntegration, NewPullRequest, PullRequest, Store};

#[derive(Debug, Clone)]
pub struct ForgePullRequest {
    pub number: i64,
    pub url: String,
    pub title: String,
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
        })
        .await
        .map_err(|error| error.to_string())
}
