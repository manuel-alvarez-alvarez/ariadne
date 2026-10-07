//! Live forge issue routes.

use axum::http::StatusCode;
use serde_json::json;

use ariadne_api::issues::IssueDto;
use ariadne_api::repositories::RepositoryDto;

use crate::common::{
    self,
    forge::{answer, stub_forge_cli},
    get, post_json, put_json, sh,
};

#[tokio::test]
async fn open_issues_are_live_and_assigned_uses_the_forge_login() {
    let issues = json!([
        {"number": 12, "title": "First", "body": "one", "url": "https://github.com/acme/widgets/issues/12", "labels": [{"name": "bug"}], "assignees": [{"login": "octocat"}], "updatedAt": "2026-10-01T00:00:00Z"},
        {"number": 13, "title": "Second", "body": "two", "url": "https://github.com/acme/widgets/issues/13", "labels": [], "assignees": [], "updatedAt": "2026-10-02T00:00:00Z"}
    ]);
    let cli = stub_forge_cli(json!([
        answer(&["auth", "status"], 0, ""),
        answer(&["api", "user"], 0, "octocat\n"),
        answer(&["issue", "list"], 0, &issues.to_string()),
        answer(&["issue", "view", "12"], 0, &issues[0].to_string()),
    ]));
    let h = common::harness().forge_cli(&cli).await;
    let path = h.git_repo("issue-repo");
    sh(
        &path,
        "git remote add origin https://github.com/acme/widgets.git",
    );
    let repository: RepositoryDto = h
        .json(
            post_json(
                "/v1/repositories",
                json!({"path": path, "base_branch": "main"}),
            ),
            StatusCode::CREATED,
        )
        .await;
    let route = format!("/v1/repositories/{}/issues", repository.id);
    let refused = h.error(get(&route), StatusCode::CONFLICT).await;
    assert_eq!(refused.error.code, "conflict");

    let _: RepositoryDto = h
        .json(
            put_json(
                &format!("/v1/repositories/{}", repository.id),
                json!({"forge": {"enabled": true}}),
            ),
            StatusCode::OK,
        )
        .await;
    let listed: Vec<IssueDto> = h.get(&route).await;
    assert_eq!(
        listed.iter().map(|issue| issue.number).collect::<Vec<_>>(),
        [12, 13]
    );
    assert_eq!(listed[0].labels, ["bug"]);
    let one: IssueDto = h.get(&format!("{route}/12")).await;
    assert_eq!(one.body, "one");
    assert!(cli.invocations().iter().any(|call| {
        call.args
            .ends_with(&["--assignee".into(), "octocat".into()])
    }));

    let _: Vec<IssueDto> = h.get(&format!("{route}?assigned=all")).await;
    assert!(cli.invocations().iter().any(|call| {
        call.args.starts_with(&["issue".into(), "list".into()])
            && !call.args.contains(&"--assignee".into())
    }));
}

#[tokio::test]
async fn gitlab_issues_use_glab_json_and_the_group_path() {
    let issue = json!({
        "iid": 7, "title": "Fix pipeline", "description": "Pipeline fails",
        "web_url": "https://gitlab.com/team/widgets/-/issues/7",
        "labels": ["ci"], "assignees": [{"username": "maria"}],
        "updated_at": "2026-10-03T00:00:00Z"
    });
    let listed = json!([issue.clone()]).to_string();
    let cli = stub_forge_cli(json!([
        {"program": "glab", "args": ["auth", "status"], "stdout": ""},
        {"program": "glab", "args": ["api", "user"], "stdout": "{\"username\":\"maria\"}"},
        {"program": "glab", "args": ["issue", "list"], "stdout": listed},
        {"program": "glab", "args": ["issue", "view", "7"], "stdout": issue.to_string()},
    ]));
    let h = common::harness().forge_cli(&cli).await;
    let path = h.git_repo("gitlab-issues");
    sh(
        &path,
        "git remote add origin https://gitlab.com/team/widgets.git",
    );
    let repository: RepositoryDto = h
        .json(
            post_json(
                "/v1/repositories",
                json!({"path": path, "base_branch": "main"}),
            ),
            StatusCode::CREATED,
        )
        .await;
    let _: RepositoryDto = h
        .json(
            put_json(
                &format!("/v1/repositories/{}", repository.id),
                json!({"forge": {"enabled": true}}),
            ),
            StatusCode::OK,
        )
        .await;
    let route = format!("/v1/repositories/{}/issues", repository.id);
    let listed: Vec<IssueDto> = h.get(&route).await;
    assert_eq!(listed[0].url, "https://gitlab.com/team/widgets/-/issues/7");
    assert_eq!(listed[0].assignees, ["maria"]);
    let one: IssueDto = h.get(&format!("{route}/7")).await;
    assert_eq!(one.title, "Fix pipeline");
    assert!(cli.invocations().iter().any(|call| call.program == "glab"
        && call.args.ends_with(&["--assignee".into(), "maria".into()])));
}
