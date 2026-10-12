//! `GET /v1/pull-requests` and its search answer from each repository's last
//! fetch alone (026): neither calls the forge, so repeated reads after one
//! fetch add no `pr list` call to the stub's own count, and a repository with
//! no fetch yet answers empty rather than live. `GET /v1/pull-requests/{id}`
//! answers the same way, from the held read; the panel route,
//! `GET /v1/repositories/{id}/pull-requests/{number}`, still reads live.
use crate::common::forge::{StubForgeCli, answer, stub_forge_cli};
use crate::common::{Harness, TIMEOUT, eventually, get, harness, post_json, put_json, sh};
use ariadne_daemon::forge::poll::Mode;
use axum::http::StatusCode;
use serde_json::{Value, json};

fn github_pull(number: i64, author: &str) -> Value {
    json!({"number": number, "url": format!("https://github.com/acme/widgets/pull/{number}"),
        "title": "Fix widgets", "body": "Fixes widgets.", "author": {"login": author},
        "state": "OPEN", "isDraft": false, "headRefName": "fix", "headRefOid": "abc",
        "headRepository": {"url": "https://github.com/acme/widgets"},
        "baseRefName": "main", "statusCheckRollup": [], "reviewDecision": "",
        "createdAt": "2026-10-01T00:00:00Z", "updatedAt": "2026-10-02T00:00:00Z"})
}

/// What a detail read asks the forge for one request, each answered empty.
fn details(number: i64) -> Vec<Value> {
    let threads =
        json!({"data": {"repository": {"pullRequest": {"reviewThreads": {"nodes": []}}}}});
    vec![
        answer(
            &[
                "api",
                &format!("repos/acme/widgets/pulls/{number}/comments"),
            ],
            0,
            "[]",
        ),
        answer(
            &[
                "api",
                &format!("repos/acme/widgets/issues/{number}/comments"),
            ],
            0,
            "[]",
        ),
        answer(
            &["api", &format!("repos/acme/widgets/pulls/{number}/reviews")],
            0,
            "[]",
        ),
        answer(&["api", "graphql"], 0, &threads.to_string()),
        answer(
            &["api", "repos/acme/widgets/commits/abc/check-runs"],
            0,
            r#"{"check_runs": []}"#,
        ),
        answer(
            &["api", "repos/acme/widgets/compare/main...abc"],
            0,
            r#"{"behind_by": 0}"#,
        ),
    ]
}

/// The stub with `pull` as the repository's one open request, which also
/// asks for my review: the first fetch starts work on it, giving it a row,
/// and reads its details whole, the same read `pr view` and the panel route
/// answer with.
fn script(pull: &Value) -> Value {
    let number = pull["number"].as_i64().unwrap();
    let mut answers = vec![
        answer(&["auth", "status"], 0, ""),
        answer(&["api", "user"], 0, "me"),
        answer(&["pr", "list"], 0, &json!([pull]).to_string()),
        answer(&["pr", "view"], 0, &pull.to_string()),
        answer(
            &["api", "repos/acme/widgets/issues?state=open&per_page=100"],
            0,
            "[]",
        ),
    ];
    answers.extend(details(number));
    json!(answers)
}

async fn enabled_repository(h: &Harness) -> String {
    let path = h.git_repo("enabled");
    sh(
        &path,
        "git remote add origin https://github.com/acme/widgets.git",
    );
    let repo: Value = h
        .json(
            post_json(
                "/v1/repositories",
                json!({"path": path, "forge": {"enabled": true}}),
            ),
            StatusCode::CREATED,
        )
        .await;
    repo["id"].as_str().unwrap().into()
}

fn list_calls(stub: &StubForgeCli) -> usize {
    stub.invocations()
        .iter()
        .filter(|i| i.args.starts_with(&["pr".into(), "list".into()]))
        .count()
}

/// Two reads of the list and the search after one fetch run no further
/// `pr list` call: the fetch itself is what runs the lists, and the routes
/// only join its held read to the rows (026).
#[tokio::test]
async fn two_reads_after_one_fetch_run_no_further_pr_list_call() {
    let pull = github_pull(1, "other");
    let stub = stub_forge_cli(script(&pull));
    let h = harness().forge_cli(&stub).await;
    let id = enabled_repository(&h).await;
    eventually(TIMEOUT, "the first fetch", async || stub.fetches() >= 1).await;
    let held = h.launcher.live.list(&id).expect("the fetch's held list");
    assert!(
        chrono::DateTime::parse_from_rfc3339(&held.fetched_at).is_ok(),
        "the held list records when its fetch ran"
    );
    let after_the_fetch = list_calls(&stub);
    assert!(after_the_fetch > 0, "the fetch itself runs the lists");

    let first: Vec<Value> = h.get("/v1/pull-requests").await;
    let second: Vec<Value> = h.get("/v1/pull-requests").await;
    assert_eq!(first.len(), 1);
    assert_eq!(second, first);
    assert_eq!(
        list_calls(&stub),
        after_the_fetch,
        "two reads of the list run no further pr list call"
    );

    let matches: Vec<Value> = h
        .get(&format!(
            "/v1/repositories/{id}/pull-requests/search?q=widgets"
        ))
        .await;
    let matches_again: Vec<Value> = h
        .get(&format!(
            "/v1/repositories/{id}/pull-requests/search?q=widgets"
        ))
        .await;
    assert_eq!(matches.len(), 1);
    assert_eq!(matches_again, matches);
    assert_eq!(
        list_calls(&stub),
        after_the_fetch,
        "the search answers from the held list, with no pr list call of its own"
    );
}

/// A repository with no fetch yet answers its list empty, with 200, rather
/// than reading the forge live; once the fetch lands, the list holds its
/// request. A repository whose integration is off still answers 409 (026).
#[tokio::test]
async fn a_repository_with_no_fetch_yet_lists_empty_and_a_disabled_one_is_refused() {
    let stub = stub_forge_cli(json!([]));
    let h = harness().forge_cli(&stub).await;
    let gate = h.dir.path().join("release-list");
    let mut gated = script(&github_pull(1, "other"));
    for entry in gated.as_array_mut().unwrap() {
        if entry["args"] == json!(["pr", "list"]) {
            entry["wait_for"] = json!(gate);
        }
    }
    stub.reprogram(gated);
    let id = enabled_repository(&h).await;

    eventually(
        TIMEOUT,
        "the fetch's list call to be in flight",
        async || {
            stub.invocations()
                .iter()
                .any(|i| i.args.starts_with(&["pr".into(), "list".into()]))
        },
    )
    .await;
    let rows: Vec<Value> = h.get(&format!("/v1/pull-requests?repo={id}")).await;
    assert_eq!(
        rows,
        Vec::<Value>::new(),
        "no fetch has landed yet, so the list is empty rather than live"
    );

    std::fs::write(&gate, "").unwrap();
    eventually(TIMEOUT, "the first fetch to land", async || {
        stub.fetches() >= 1
    })
    .await;
    let rows: Vec<Value> = h.get(&format!("/v1/pull-requests?repo={id}")).await;
    assert_eq!(rows.len(), 1, "the fetch filled the held list");

    let _: Value = h
        .json(
            put_json(
                &format!("/v1/repositories/{id}"),
                json!({"forge": {"enabled": false}}),
            ),
            StatusCode::OK,
        )
        .await;
    h.error(
        get(&format!("/v1/pull-requests?repo={id}")),
        StatusCode::CONFLICT,
    )
    .await;
}

/// `GET /v1/pull-requests/{id}` answers the held read, not a fresh
/// `details()` call: it still answers once every detail-reading call is
/// broken, since it never makes one.
#[tokio::test]
async fn the_single_request_route_answers_the_held_read_with_no_details_call() {
    let pull = github_pull(2, "other");
    let stub = stub_forge_cli(script(&pull));
    let h = harness().forge_cli(&stub).await;
    let id = enabled_repository(&h).await;
    h.state.forge_poll.set_mode(&id, Mode::WakeOnly);

    let mut row = None;
    eventually(TIMEOUT, "the review request's row", async || {
        row = h
            .store
            .list_pull_requests(ariadne_store::PullRequestFilter::default())
            .await
            .unwrap()
            .pop();
        row.is_some()
    })
    .await;
    let row = row.unwrap();
    eventually(TIMEOUT, "the row's first detail read", async || {
        h.launcher
            .live
            .get(&row.id)
            .is_some_and(|l| l.details.is_some())
    })
    .await;
    eventually(TIMEOUT, "the first fetch to finish", async || {
        stub.fetches() >= 1
    })
    .await;

    // Every call a live detail read would make now fails.
    let mut broken = script(&pull);
    for entry in broken.as_array_mut().unwrap() {
        if entry["args"][0] == "api" || entry["args"] == json!(["pr", "view"]) {
            entry["exit"] = json!(1);
            entry["stdout"] = json!("boom");
        }
    }
    stub.reprogram(broken);
    let calls_before = stub.invocations().len();

    let dto: Value = h.get(&format!("/v1/pull-requests/{}", row.id)).await;
    assert_eq!(
        dto["number"], 2,
        "the held read answers; a live one would have failed"
    );
    assert_eq!(
        stub.invocations().len(),
        calls_before,
        "the held route ran no detail read"
    );
}

/// `GET /v1/repositories/{id}/pull-requests/{number}` still reads the forge
/// now, unlike the list, the search and the single-request route: the panel
/// sees a change the held list has not caught up with yet (026).
#[tokio::test]
async fn the_panel_route_still_reads_live() {
    let stub = stub_forge_cli(script(&github_pull(1, "other")));
    let h = harness().forge_cli(&stub).await;
    let id = enabled_repository(&h).await;
    eventually(TIMEOUT, "the first fetch", async || stub.fetches() >= 1).await;

    let mut renamed = github_pull(1, "other");
    renamed["title"] = json!("Fix widgets, renamed");
    stub.reprogram(script(&renamed));

    let dto: Value = h
        .get(&format!("/v1/repositories/{id}/pull-requests/1"))
        .await;
    assert_eq!(
        dto["title"], "Fix widgets, renamed",
        "the panel reads the forge now, not the repository's held list"
    );
}
