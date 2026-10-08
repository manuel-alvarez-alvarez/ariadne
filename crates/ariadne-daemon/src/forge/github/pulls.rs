//! Pull requests through the GitHub CLI.
use super::Github;
use crate::forge::pulls::ForgePullRequest;
use serde::Deserialize;

const FIELDS: &str = "number,url,title,body,author,state,isDraft,headRefName,headRefOid,headRepository,headRepositoryOwner,baseRefName,statusCheckRollup,reviewDecision,createdAt,updatedAt,mergeCommit";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Pull {
    number: i64,
    url: String,
    title: String,
    #[serde(default)]
    body: Option<String>,
    author: Login,
    state: String,
    is_draft: bool,
    head_ref_name: String,
    head_ref_oid: String,
    head_repository: Option<Repository>,
    head_repository_owner: Option<Login>,
    base_ref_name: String,
    status_check_rollup: Option<Vec<Check>>,
    review_decision: Option<String>,
    created_at: String,
    /// Absent from an older read: the opening time stands for it.
    #[serde(default)]
    updated_at: Option<String>,
    /// The commit a merged request landed as.
    #[serde(default)]
    merge_commit: Option<Commit>,
}
#[derive(Deserialize)]
struct Commit {
    oid: String,
}
#[derive(Deserialize)]
struct Login {
    login: String,
}
#[derive(Deserialize)]
struct Repository {
    name: Option<String>,
    url: Option<String>,
}
#[derive(Deserialize)]
struct Check {
    status: Option<String>,
    conclusion: Option<String>,
    state: Option<String>,
}

impl Pull {
    fn normalized(self) -> ForgePullRequest {
        let checks = self.status_check_rollup.unwrap_or_default();
        let check_state = if checks.is_empty() {
            "none"
        } else if checks.iter().any(|c| {
            matches!(
                c.conclusion.as_deref().or(c.state.as_deref()),
                Some(
                    "FAILURE"
                        | "ERROR"
                        | "TIMED_OUT"
                        | "CANCELLED"
                        | "ACTION_REQUIRED"
                        | "STARTUP_FAILURE"
                )
            )
        }) {
            "failure"
        } else if checks.iter().any(|c| {
            matches!(
                c.status.as_deref(),
                Some("QUEUED" | "IN_PROGRESS" | "WAITING" | "PENDING" | "REQUESTED")
            ) || c.state.as_deref() == Some("PENDING")
        }) {
            "pending"
        } else {
            "success"
        };
        let head_repo = self.head_repository.and_then(|repo| {
            repo.url
                .map(|url| format!("{}.git", url.trim_end_matches(".git").trim_end_matches('/')))
                .or_else(|| {
                    let host = reqwest::Url::parse(&self.url).ok()?.host_str()?.to_owned();
                    Some(format!(
                        "https://{host}/{}/{}.git",
                        self.head_repository_owner?.login, repo.name?
                    ))
                })
        });
        ForgePullRequest {
            number: self.number,
            url: self.url,
            title: self.title,
            body: self.body.unwrap_or_default(),
            author_login: self.author.login,
            state: self.state.to_lowercase(),
            draft: self.is_draft,
            head_branch: self.head_ref_name,
            head_sha: self.head_ref_oid,
            head_repo,
            base_branch: self.base_ref_name,
            checks: check_state.into(),
            review_decision: match self.review_decision.as_deref() {
                Some("APPROVED") => "approved",
                Some("CHANGES_REQUESTED") => "changes_requested",
                Some("REVIEW_REQUIRED") => "review_required",
                _ => "none",
            }
            .into(),
            updated_at: self.updated_at.unwrap_or_else(|| self.created_at.clone()),
            opened_at: self.created_at,
            merge_sha: self
                .merge_commit
                .filter(|_| self.state == "MERGED")
                .map(|commit| commit.oid),
        }
    }
}
impl Github {
    async fn pulls(&self, repo: &str, filter: &[&str]) -> Result<Vec<ForgePullRequest>, String> {
        // Increase the limit until the complete result fits. `gh pr list` has
        // no page flag; its limit drives the client's GraphQL pagination.
        let mut limit = 100;
        loop {
            let size = limit.to_string();
            let mut args = vec![
                "pr", "list", "--repo", repo, "--state", "open", "--json", FIELDS, "--limit", &size,
            ];
            args.extend_from_slice(filter);
            let output = self.cli.answer(&args).await?;
            let rows: Vec<Pull> = serde_json::from_str(&output)
                .map_err(|e| format!("cannot read GitHub pull requests: {e}"))?;
            if rows.len() < limit {
                return Ok(rows.into_iter().map(Pull::normalized).collect());
            }
            limit *= 2;
        }
    }
    pub(crate) async fn list_open_pull_requests(
        &self,
        repo: &str,
        login: &str,
    ) -> Result<crate::forge::pulls::Listed, String> {
        let mut open = self.pulls(repo, &[]).await?;
        let requested = self
            .pulls(repo, &["--search", &format!("review-requested:{login}")])
            .await?;
        let numbers = requested.iter().map(|p| p.number).collect();
        // A request the open list missed while it paged is still open.
        open.extend(requested);
        open.sort_by_key(|p| p.number);
        open.dedup_by_key(|p| p.number);
        Ok(crate::forge::pulls::Listed {
            open,
            requested: numbers,
        })
    }
    pub(crate) async fn search_pull_requests(
        &self,
        repo: &str,
        query: &str,
    ) -> Result<Vec<ForgePullRequest>, String> {
        let mut rows = self.pulls(repo, &["--search", query]).await?;
        // Forge free-text search does not match every author or request number.
        // Complete those matches from the repository's open requests.
        rows.extend(self.pulls(repo, &[]).await?);
        let query = query.trim().trim_start_matches('#').to_lowercase();
        rows.retain(|p| {
            p.title.to_lowercase().contains(&query)
                || p.author_login.to_lowercase().contains(&query)
                || p.number.to_string() == query
        });
        rows.sort_by_key(|p| p.number);
        rows.dedup_by_key(|p| p.number);
        Ok(rows)
    }
    pub(crate) async fn pull_request(
        &self,
        repo: &str,
        number: i64,
    ) -> Result<ForgePullRequest, String> {
        let output = self
            .cli
            .answer(&[
                "pr",
                "view",
                &number.to_string(),
                "--repo",
                repo,
                "--json",
                FIELDS,
            ])
            .await?;
        serde_json::from_str::<Pull>(&output)
            .map(Pull::normalized)
            .map_err(|e| format!("cannot read GitHub pull request: {e}"))
    }
}
