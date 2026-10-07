//! GitLab issues through `glab`.

use ariadne_api::issues::IssueDto;
use serde::Deserialize;

use super::Gitlab;

#[derive(Deserialize)]
struct Issue {
    iid: i64,
    title: String,
    #[serde(default)]
    description: String,
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
            body: issue.description,
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
    pub(crate) async fn list_open_issues(
        &self,
        repository: &str,
        assignee: Option<&str>,
    ) -> Result<Vec<IssueDto>, String> {
        let mut args = vec!["issue", "list", "-R", repository, "-F", "json"];
        if let Some(login) = assignee {
            args.extend(["--assignee", login]);
        }
        let answer = self.cli.answer(&args).await?;
        serde_json::from_str::<Vec<Issue>>(&answer)
            .map(|issues| issues.into_iter().map(Into::into).collect())
            .map_err(|error| format!("cannot read GitLab issues: {error}"))
    }

    pub(crate) async fn issue(&self, repository: &str, number: i64) -> Result<IssueDto, String> {
        let number = number.to_string();
        let answer = self
            .cli
            .answer(&["issue", "view", &number, "-R", repository, "-F", "json"])
            .await?;
        serde_json::from_str::<Issue>(&answer)
            .map(Into::into)
            .map_err(|error| format!("cannot read GitLab issue: {error}"))
    }
}
