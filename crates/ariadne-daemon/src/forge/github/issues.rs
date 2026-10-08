//! GitHub issues through `gh api` (028): every page of the open ones, on the
//! integration's own host.

use ariadne_api::issues::IssueDto;
use serde::Deserialize;

use super::Github;
use crate::forge::pulls::split_slug;

#[derive(Deserialize)]
struct Issue {
    number: i64,
    title: String,
    #[serde(default)]
    body: Option<String>,
    html_url: String,
    #[serde(default)]
    labels: Vec<Name>,
    #[serde(default)]
    assignees: Vec<Login>,
    updated_at: String,
    /// Set on a pull request: the issues endpoint lists those too.
    #[serde(default)]
    pull_request: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct Name {
    name: String,
}

#[derive(Deserialize)]
struct Login {
    login: String,
}

impl From<Issue> for IssueDto {
    fn from(issue: Issue) -> Self {
        Self {
            number: issue.number,
            title: issue.title,
            body: issue.body.unwrap_or_default(),
            url: issue.html_url,
            labels: issue.labels.into_iter().map(|label| label.name).collect(),
            assignees: issue.assignees.into_iter().map(|user| user.login).collect(),
            updated_at: issue.updated_at,
        }
    }
}

impl Github {
    /// Every open issue of `repo`, `host/owner/name`, or those assigned to
    /// `assignee`: `gh api --paginate repos/<owner>/<name>/issues`, a hundred
    /// a page, with the pull requests it also lists left out.
    pub(crate) async fn list_open_issues(
        &self,
        repo: &str,
        assignee: Option<&str>,
    ) -> Result<Vec<IssueDto>, String> {
        let (host, owner, name) = split_slug(repo)?;
        let mut path = format!("repos/{owner}/{name}/issues?state=open&per_page=100");
        if let Some(login) = assignee {
            path.push_str(&format!("&assignee={login}"));
        }
        Ok(self
            .pages::<Issue>(host, &path)
            .await?
            .into_iter()
            .filter(|issue| issue.pull_request.is_none())
            .map(Into::into)
            .collect())
    }

    pub(crate) async fn issue(&self, repo: &str, number: i64) -> Result<IssueDto, String> {
        let (host, owner, name) = split_slug(repo)?;
        self.object::<Issue>(host, &format!("repos/{owner}/{name}/issues/{number}"))
            .await
            .map(Into::into)
    }
}
