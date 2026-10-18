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

/// The open issues are read live, every page of them, on the
/// integration's own host: an enterprise host is named to `gh api`, so the
/// read never lands on github.com. A pull request the issues endpoint lists
/// too is left out, and `assigned=me` asks for the forge login's.
#[tokio::test]
async fn open_issues_are_live_on_every_page_of_the_integrations_host() {
    let issue = |number: i64, title: &str| {
        json!({"number": number, "title": title, "body": null,
            "html_url": format!("https://github.company.test/acme/widgets/issues/{number}"),
            "labels": [{"name": "bug"}], "assignees": [{"login": "octocat"}],
            "updated_at": "2026-10-01T00:00:00Z"})
    };
    let first: Vec<_> = (1..=100).map(|n| issue(n, "Paged")).collect();
    let mut second = vec![issue(101, "Last")];
    second.push(json!({"number": 102, "title": "A request", "body": "",
        "html_url": "https://github.company.test/acme/widgets/pull/102",
        "labels": [], "assignees": [], "updated_at": "2026-10-01T00:00:00Z",
        "pull_request": {"url": "x"}}));
    // `gh api --paginate` prints one JSON list per page, back to back.
    let pages = format!("{}{}", json!(first), json!(second));
    let list = "repos/acme/widgets/issues?state=open&per_page=100";
    let mine = format!("{list}&assignee=octocat");
    let cli = stub_forge_cli(json!([
        answer(&["auth", "status"], 0, ""),
        answer(&["api", "user"], 0, "octocat\n"),
        answer(&["api", &mine], 0, &pages),
        answer(&["api", list], 0, &pages),
        answer(
            &["api", "repos/acme/widgets/issues/12"],
            0,
            &issue(12, "One").to_string()
        ),
    ]));
    let h = common::harness().forge_cli(&cli).await;
    let path = h.git_repo("issue-repo");
    sh(
        &path,
        "git remote add origin https://github.company.test/acme/widgets.git",
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
    assert_eq!(listed.len(), 101, "every page, and no pull request");
    assert_eq!(listed[100].title, "Last");
    assert_eq!(listed[0].labels, ["bug"]);
    assert_eq!(listed[0].body, "");
    let one: IssueDto = h.get(&format!("{route}/12")).await;
    assert_eq!(one.title, "One");
    let ran = |path: &str| {
        cli.invocations().iter().any(|call| {
            call.args
                == [
                    "api",
                    path,
                    "--hostname",
                    "github.company.test",
                    "--paginate",
                ]
        })
    };
    assert!(ran(&mine), "{:?}", cli.invocations());

    let _: Vec<IssueDto> = h.get(&format!("{route}?assigned=all")).await;
    assert!(ran(list), "{:?}", cli.invocations());
    assert!(
        cli.invocations()
            .iter()
            .all(|call| !call.args.starts_with(&["issue".into()])),
        "no `gh issue` command, which reads one page"
    );
}
