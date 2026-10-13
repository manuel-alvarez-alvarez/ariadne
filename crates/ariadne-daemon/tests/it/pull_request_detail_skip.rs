//! A fetch skips the detail read of a request whose list read repeats
//! everything a detail read would also show (026 rule 9, perf): proved here
//! by counting the stub's own `graphql` calls, each of them a detail read
//! and nothing else.
use axum::http::StatusCode;
use serde_json::{Value, json};

use crate::common::forge::{StubForgeCli, answer, stub_forge_cli};
use crate::common::{Harness, TIMEOUT, eventually, harness, post_json, sh};
use ariadne_daemon::forge::poll::Mode;

fn github_pull(head_sha: &str, updated_at: &str) -> Value {
    json!({"number": 1, "url": "https://github.com/acme/widgets/pull/1",
        "title": "Fix widgets", "body": "Fixes widgets.", "author": {"login": "other"},
        "state": "OPEN", "isDraft": false, "headRefName": "fix", "headRefOid": head_sha,
        "headRepository": {"url": "https://github.com/acme/widgets"},
        "baseRefName": "main", "statusCheckRollup": [], "reviewDecision": "",
        "reviewRequests": [{"login": "me"}],
        "createdAt": "2026-10-01T00:00:00Z", "updatedAt": updated_at})
}

/// `pull`, with the list's own check state changed: GitHub returns the
/// same check rollup from the list and a single-request read.
fn with_checks(mut pull: Value, checks: Value) -> Value {
    pull["statusCheckRollup"] = checks;
    pull
}

/// What the stub answers for a detail read of request #1, every part
/// empty, so a comparison before and after a skip has nothing else to
/// explain a difference.
fn details(graphql_exit: i32) -> Vec<Value> {
    let threads =
        json!({"data": {"repository": {"pullRequest": {"reviewThreads": {"nodes": []}}}}});
    vec![
        answer(&["api", "repos/acme/widgets/pulls/1/comments"], 0, "[]"),
        answer(&["api", "repos/acme/widgets/issues/1/comments"], 0, "[]"),
        answer(&["api", "repos/acme/widgets/pulls/1/reviews"], 0, "[]"),
        answer(&["api", "graphql"], graphql_exit, &threads.to_string()),
        answer(
            &["api", "repos/acme/widgets/commits/abc/check-runs"],
            0,
            r#"{"check_runs": []}"#,
        ),
        answer(
            &["api", "repos/acme/widgets/commits/def/check-runs"],
            0,
            r#"{"check_runs": []}"#,
        ),
        answer(
            &["api", "repos/acme/widgets/compare/main...abc"],
            0,
            r#"{"behind_by": 0}"#,
        ),
        answer(
            &["api", "repos/acme/widgets/compare/main...def"],
            0,
            r#"{"behind_by": 0}"#,
        ),
    ]
}

/// The stub with `pull` as the one open request, asking for my review, and
/// its detail read's own `graphql` call exiting `graphql_exit`.
fn script(pull: Value, graphql_exit: i32) -> Value {
    let mut answers = vec![
        answer(&["auth", "status"], 0, ""),
        answer(&["api", "user"], 0, "me"),
        answer(&["pr", "list"], 0, &json!([pull.clone()]).to_string()),
        answer(&["pr", "view"], 0, &pull.to_string()),
        answer(
            &["api", "repos/acme/widgets/issues?state=open&per_page=100"],
            0,
            "[]",
        ),
    ];
    answers.extend(details(graphql_exit));
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

/// How many detail reads the stub has actually run: its `graphql` call is
/// asked only from inside `ForgeClient::details`, never from a list read or
/// a single read of the request, so a count of it is a count of detail
/// reads, skipped or not.
fn detail_reads(stub: &StubForgeCli) -> usize {
    stub.invocations()
        .iter()
        .filter(|i| i.args.get(1).is_some_and(|a| a == "graphql"))
        .count()
}

/// Two fetches of a request whose list read repeats every field run one
/// detail read, not two: the second fetch finds nothing `Live::unmoved`
/// would not already call unmoved, and keeps the held details instead of
/// asking the forge again.
#[tokio::test]
async fn two_fetches_of_an_unmoved_request_run_one_detail_read() {
    let pull = github_pull("abc", "2026-10-01T00:00:00Z");
    let stub = stub_forge_cli(script(pull, 0));
    let h = harness().forge_cli(&stub).await;
    let id = enabled_repository(&h).await;
    h.state.forge_poll.set_mode(&id, Mode::WakeOnly);
    eventually(TIMEOUT, "the first detail read", async || {
        detail_reads(&stub) == 1
    })
    .await;
    eventually(TIMEOUT, "the first fetch to finish", async || {
        stub.fetches() >= 1
    })
    .await;

    h.state.forge_poll.wake(&id);
    eventually(TIMEOUT, "the second fetch to finish", async || {
        stub.fetches() >= 2
    })
    .await;
    assert_eq!(
        detail_reads(&stub),
        1,
        "the unmoved request's second fetch skipped the detail read"
    );
}

/// A fetch whose list read moves its head, update time, or check state runs
/// the detail read again. Each field is changed on its own between fetches.
#[tokio::test]
async fn a_fetch_whose_list_read_field_moved_runs_the_detail_read() {
    let pull = github_pull("abc", "2026-10-01T00:00:00Z");
    let stub = stub_forge_cli(script(pull, 0));
    let h = harness().forge_cli(&stub).await;
    let id = enabled_repository(&h).await;
    h.state.forge_poll.set_mode(&id, Mode::WakeOnly);
    eventually(TIMEOUT, "the first detail read", async || {
        detail_reads(&stub) == 1
    })
    .await;
    eventually(TIMEOUT, "the first fetch to finish", async || {
        stub.fetches() >= 1
    })
    .await;

    let moved = github_pull("def", "2026-10-01T00:00:00Z");
    stub.reprogram(script(moved, 0));
    h.state.forge_poll.wake(&id);
    eventually(TIMEOUT, "the second detail read", async || {
        detail_reads(&stub) == 2
    })
    .await;
    eventually(TIMEOUT, "the second fetch to finish", async || {
        stub.fetches() >= 2
    })
    .await;

    let moved = github_pull("def", "2026-10-02T00:00:00Z");
    stub.reprogram(script(moved, 0));
    h.state.forge_poll.wake(&id);
    eventually(TIMEOUT, "the third detail read", async || {
        detail_reads(&stub) == 3
    })
    .await;
    eventually(TIMEOUT, "the third fetch to finish", async || {
        stub.fetches() >= 3
    })
    .await;

    let moved = with_checks(
        github_pull("def", "2026-10-02T00:00:00Z"),
        json!([{"status": "COMPLETED", "conclusion": "FAILURE"}]),
    );
    stub.reprogram(script(moved, 0));
    h.state.forge_poll.wake(&id);
    eventually(TIMEOUT, "the fourth detail read", async || {
        detail_reads(&stub) == 4
    })
    .await;
}

/// A fetch after Ariadne posted a comment on the request runs the detail
/// read again, even where the list read repeats every other field: the
/// posted comment is not yet read back by a detail read
/// (`Live::unmoved`), so the held details cannot be trusted as current.
#[tokio::test]
async fn a_fetch_after_ariadne_posts_a_comment_runs_the_detail_read() {
    let pull = github_pull("abc", "2026-10-01T00:00:00Z");
    let stub = stub_forge_cli(script(pull, 0));
    let h = harness().forge_cli(&stub).await;
    let id = enabled_repository(&h).await;
    h.state.forge_poll.set_mode(&id, Mode::WakeOnly);
    eventually(TIMEOUT, "the first detail read", async || {
        detail_reads(&stub) == 1
    })
    .await;
    eventually(TIMEOUT, "the first fetch to finish", async || {
        stub.fetches() >= 1
    })
    .await;

    let row = h
        .store
        .list_pull_requests(ariadne_store::PullRequestFilter::default())
        .await
        .unwrap()
        .into_iter()
        .next()
        .expect("the request that asks for my review has a row");
    h.launcher.live.add_comments(
        &row.id,
        &[ariadne_store::NewPullRequestComment {
            forge_id: "ic-9".into(),
            thread_id: "conversation".into(),
            kind: "issue_comment".into(),
            author_login: "me".into(),
            author_is_bot: false,
            body: "Looks good".into(),
            path: None,
            line: None,
            in_reply_to: None,
            created_at: "2026-10-03T00:00:00Z".into(),
            resolved: false,
            from_review: false,
        }],
    );

    h.state.forge_poll.wake(&id);
    eventually(TIMEOUT, "the second detail read", async || {
        detail_reads(&stub) == 2
    })
    .await;
}

/// A row whose last detail read failed runs the detail read again, even
/// where the list read repeats every field: `evidence_ok` false is never
/// trusted as unmoved (`Live::unmoved`).
#[tokio::test]
async fn a_fetch_after_a_failed_detail_read_runs_the_detail_read_again() {
    let pull = github_pull("abc", "2026-10-01T00:00:00Z");
    let stub = stub_forge_cli(script(pull.clone(), 1));
    let h = harness().forge_cli(&stub).await;
    let id = enabled_repository(&h).await;
    h.state.forge_poll.set_mode(&id, Mode::WakeOnly);
    eventually(TIMEOUT, "the first, failing detail read", async || {
        detail_reads(&stub) == 1
    })
    .await;
    eventually(TIMEOUT, "the first fetch to finish", async || {
        stub.fetches() >= 1
    })
    .await;

    stub.reprogram(script(pull, 0));
    h.state.forge_poll.wake(&id);
    eventually(TIMEOUT, "the second detail read", async || {
        detail_reads(&stub) == 2
    })
    .await;
}
