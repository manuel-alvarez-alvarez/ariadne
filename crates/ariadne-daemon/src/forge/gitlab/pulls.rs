//! Merge requests through the GitLab CLI.
use super::Gitlab;
use crate::forge::pulls::ForgePullRequest;
use serde::Deserialize;

#[derive(Deserialize)]
struct Pull {
    iid: i64,
    project_id: i64,
    source_project_id: Option<i64>,
    web_url: String,
    title: String,
    #[serde(default)]
    description: Option<String>,
    author: User,
    state: String,
    #[serde(default)]
    draft: bool,
    source_branch: String,
    target_branch: String,
    sha: String,
    #[serde(default)]
    source_project: Option<Project>,
    #[serde(default)]
    head_pipeline: Option<Pipeline>,
    #[serde(default)]
    detailed_merge_status: String,
    created_at: String,
    #[serde(default)]
    updated_at: Option<String>,
    /// The merge commit, or the squash commit of a squash merge; GitLab
    /// names neither on a fast-forward merge, which lands the head itself.
    #[serde(default)]
    merge_commit_sha: Option<String>,
    #[serde(default)]
    squash_commit_sha: Option<String>,
}
#[derive(Deserialize)]
struct User {
    username: String,
}
#[derive(Deserialize)]
struct Project {
    http_url_to_repo: String,
}
#[derive(Deserialize)]
struct Pipeline {
    status: String,
}
#[derive(Deserialize)]
struct Approvals {
    #[serde(default)]
    approvals_left: i64,
    approved: Option<bool>,
    #[serde(default)]
    approved_by: Vec<serde_json::Value>,
}
impl Pull {
    fn normalized(self) -> ForgePullRequest {
        let merge_sha = (self.state == "merged").then(|| {
            self.merge_commit_sha
                .clone()
                .or_else(|| self.squash_commit_sha.clone())
                .unwrap_or_else(|| self.sha.clone())
        });
        ForgePullRequest {
            number: self.iid,
            url: self.web_url,
            title: self.title,
            body: self.description.unwrap_or_default(),
            author_login: self.author.username,
            state: if self.state == "opened" {
                "open".into()
            } else {
                self.state
            },
            draft: self.draft,
            head_branch: self.source_branch,
            head_sha: self.sha,
            head_repo: self.source_project.map(|p| p.http_url_to_repo),
            base_branch: self.target_branch,
            checks: match self.head_pipeline.as_ref().map(|p| p.status.as_str()) {
                None => "none",
                Some("success" | "skipped") => "success",
                Some("failed" | "canceled") => "failure",
                Some(_) => "pending",
            }
            .into(),
            review_decision: match self.detailed_merge_status.as_str() {
                "not_approved" => "review_required",
                "requested_changes" => "changes_requested",
                _ => "none",
            }
            .into(),
            updated_at: self.updated_at.unwrap_or_else(|| self.created_at.clone()),
            opened_at: self.created_at,
            merge_sha,
        }
    }
}
impl Gitlab {
    async fn pulls(&self, repo: &str, filter: &[&str]) -> Result<Vec<ForgePullRequest>, String> {
        let project = super::details::repo_url(repo)?;
        let mut all = Vec::new();
        for page in 1.. {
            let page = page.to_string();
            let mut args = vec![
                "mr",
                "list",
                "-R",
                &project,
                "-F",
                "json",
                "--per-page",
                "100",
                "--page",
                &page,
            ];
            args.extend_from_slice(filter);
            let output = self.cli.answer(&args).await?;
            let rows: Vec<Pull> = serde_json::from_str(&output)
                .map_err(|e| format!("cannot read GitLab merge requests: {e}"))?;
            let last = rows.len() < 100;
            for pull in rows {
                // The list omits the head pipeline on some GitLab versions.
                all.push(self.pull_request(repo, pull.iid).await?);
            }
            if last {
                break;
            }
        }
        Ok(all)
    }
    pub(crate) async fn list_open_pull_requests(
        &self,
        repo: &str,
        login: &str,
    ) -> Result<crate::forge::pulls::Listed, String> {
        let mut open = self.pulls(repo, &[]).await?;
        let requested = self.pulls(repo, &["--reviewer", login]).await?;
        let numbers = requested.iter().map(|p| p.number).collect();
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
        let project = super::details::repo_url(repo)?;
        let output = self
            .cli
            .answer(&[
                "mr",
                "view",
                &number.to_string(),
                "-R",
                &project,
                "-F",
                "json",
            ])
            .await?;
        let pull: Pull = serde_json::from_str(&output)
            .map_err(|e| format!("cannot read GitLab merge request: {e}"))?;
        let host = reqwest::Url::parse(&pull.web_url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_owned))
            .ok_or("the merge request has no forge host")?;
        let project_id = pull.project_id;
        let source_project_id = pull.source_project_id;
        let mut result = pull.normalized();
        if result.head_repo.is_none()
            && let Some(source) = source_project_id
        {
            let output = self
                .cli
                .answer(&["api", &format!("projects/{source}"), "--hostname", &host])
                .await?;
            let project: Project = serde_json::from_str(&output)
                .map_err(|e| format!("cannot read GitLab head repository: {e}"))?;
            result.head_repo = Some(project.http_url_to_repo);
        }
        let output = self
            .cli
            .answer(&[
                "api",
                &format!("projects/{project_id}/merge_requests/{number}/approvals"),
                "--hostname",
                &host,
            ])
            .await?;
        let approvals: Approvals = serde_json::from_str(&output)
            .map_err(|e| format!("cannot read GitLab approvals: {e}"))?;
        if result.review_decision != "changes_requested" {
            result.review_decision = if approvals.approvals_left > 0 {
                "review_required"
            } else if !approvals.approved_by.is_empty() && approvals.approved.unwrap_or(true) {
                "approved"
            } else {
                "none"
            }
            .into();
        }
        Ok(result)
    }
}
