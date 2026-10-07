//! GitHub issues through `gh`.

use ariadne_api::issues::IssueDto;
use serde::Deserialize;

use super::Github;

const FIELDS: &str = "number,title,body,url,labels,assignees,updatedAt";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Issue {
    number: i64,
    title: String,
    body: String,
    url: String,
    labels: Vec<Name>,
    assignees: Vec<Login>,
    updated_at: String,
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
            body: issue.body,
            url: issue.url,
            labels: issue.labels.into_iter().map(|label| label.name).collect(),
            assignees: issue.assignees.into_iter().map(|user| user.login).collect(),
            updated_at: issue.updated_at,
        }
    }
}

impl Github {
    pub(crate) async fn list_open_issues(
        &self,
        repository: &str,
        assignee: Option<&str>,
    ) -> Result<Vec<IssueDto>, String> {
        let mut args = vec![
            "issue", "list", "--repo", repository, "--state", "open", "--json", FIELDS,
        ];
        if let Some(login) = assignee {
            args.extend(["--assignee", login]);
        }
        let answer = self.cli.answer(&args).await?;
        serde_json::from_str::<Vec<Issue>>(&answer)
            .map(|issues| issues.into_iter().map(Into::into).collect())
            .map_err(|error| format!("cannot read GitHub issues: {error}"))
    }

    pub(crate) async fn issue(&self, repository: &str, number: i64) -> Result<IssueDto, String> {
        let number = number.to_string();
        let answer = self
            .cli
            .answer(&[
                "issue", "view", &number, "--repo", repository, "--json", FIELDS,
            ])
            .await?;
        serde_json::from_str::<Issue>(&answer)
            .map(Into::into)
            .map_err(|error| format!("cannot read GitHub issue: {error}"))
    }
}
