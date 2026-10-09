//! GitLab issues through `glab api` (028): every page of the open ones, on
//! the integration's own host.

use ariadne_api::issues::IssueDto;
use serde::Deserialize;

use super::Gitlab;
use super::details::project;
use crate::forge::pulls::split_slug;

#[derive(Deserialize)]
struct Issue {
    iid: i64,
    title: String,
    #[serde(default)]
    description: Option<String>,
    web_url: String,
    #[serde(default)]
    labels: Vec<String>,
    #[serde(default)]
    assignees: Vec<User>,
    updated_at: String,
}

#[derive(Deserialize)]
struct User {
    username: String,
}

impl From<Issue> for IssueDto {
    fn from(issue: Issue) -> Self {
        Self {
            number: issue.iid,
            title: issue.title,
            body: issue.description.unwrap_or_default(),
            url: issue.web_url,
            labels: issue.labels,
            assignees: issue
                .assignees
                .into_iter()
                .map(|user| user.username)
                .collect(),
            updated_at: issue.updated_at,
        }
    }
}

impl Gitlab {
    /// Every open issue of `repo`, `host/group/name`, or those assigned to
    /// `assignee`: `glab api --paginate projects/<id>/issues`, a hundred a
    /// page.
    pub(crate) async fn list_open_issues(
        &self,
        repo: &str,
        assignee: Option<&str>,
    ) -> Result<Vec<IssueDto>, String> {
        let (host, owner, name) = split_slug(repo)?;
        let mut path = format!(
            "projects/{}/issues?state=opened&per_page=100",
            project(owner, name)
        );
        if let Some(login) = assignee {
            path.push_str(&format!("&assignee_username={login}"));
        }
        Ok(self
            .pages::<Issue>(host, &path)
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub(crate) async fn issue(&self, repo: &str, number: i64) -> Result<IssueDto, String> {
        let (host, owner, name) = split_slug(repo)?;
        self.object::<Issue>(
            host,
            &format!("projects/{}/issues/{number}", project(owner, name)),
        )
        .await
        .map(Into::into)
    }
}
