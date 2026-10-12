//! An open author request with no live task can still be kept by manual
//! review or babysit asks (029, 032). Its `keep_pass` probe of the forge waits
//! `CLEANUP_RETRY` between reads (026), while a request change bypasses that
//! tick-only wait.

use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::mpsc::UnboundedSender;

use crate::common::forge::{StubForgeCli, answer, enable_integration, seed_live, stub_forge_cli};
use crate::common::{Harness, harness};
use ariadne_daemon::scheduler::{self, SchedEvent};
use ariadne_daemon::timeouts::Timeouts;
use ariadne_store::{NewPullRequest, PullRequestRow};

const URL: &str = "https://github.com/acme/widgets/pull/1";
const HEAD_SHA: &str = "abc123";

/// The stub response for `read_ended`. A failing response proves the error
/// path stores the retry wait independently of an open response.
fn script(pr_view_exit: i32) -> Value {
    let read = json!({"number": 1, "url": URL,
        "title": "Fix widgets", "author": {"login": "me"}, "state": "OPEN", "isDraft": false,
        "headRefName": "fix", "headRefOid": HEAD_SHA,
        "headRepository": {"url": "https://github.com/acme/widgets"},
        "baseRefName": "main", "statusCheckRollup": [], "reviewDecision": "",
        "createdAt": "2026-10-01T00:00:00Z"});
    json!([answer(&["pr", "view"], pr_view_exit, &read.to_string())])
}

/// An author request with no task and manual review and babysit asks. Neither
/// ask has a model, so no request session can issue its own forge reads.
async fn asked_row(h: &Harness, repository_id: &str) -> PullRequestRow {
    let (row, _) = h
        .store
        .upsert_pull_request(NewPullRequest {
            repository_id: repository_id.to_string(),
            number: 1,
            url: URL.to_string(),
            role: "author".to_string(),
            origin_task_id: None,
        })
        .await
        .unwrap();
    let database = sqlx::SqlitePool::connect(&format!("sqlite://{}", h.at("test.db").display()))
        .await
        .unwrap();
    sqlx::query("UPDATE pull_requests SET review_asked = 1, babysit_asked = 1 WHERE id = ?")
        .bind(&row.id)
        .execute(&database)
        .await
        .unwrap();
    database.close().await;
    seed_live(h, &row, "me", "fix", HEAD_SHA, false);
    row
}

/// Make a scheduler and request that count completed `pr view` calls through
/// the stub's `fetches` interface. Each test owns a new scheduler, so its
/// retry map starts empty.
async fn kept_request(
    pr_view_exit: i32,
) -> (
    Harness,
    StubForgeCli,
    PullRequestRow,
    UnboundedSender<SchedEvent>,
) {
    let stub = stub_forge_cli(script(pr_view_exit)).pull_request_views_as_fetches();
    let timeouts = Timeouts {
        full_reconcile: Duration::from_secs(60),
        ..Timeouts::default()
    };
    let h = harness().timeouts(timeouts).forge_cli(&stub).await;
    let repo = h.repository(&h.at("widgets")).await;
    enable_integration(&h, &repo.id, "me").await;
    let row = asked_row(&h, &repo.id).await;
    let sched = scheduler::start(h.store.clone(), h.launcher.clone(), false, timeouts);
    (h, stub, row, sched)
}

/// Wake the scheduler about one request and wait for that pass to end.
async fn pass_over(sched: &UnboundedSender<SchedEvent>, pull_request_id: &str) {
    sched
        .send(SchedEvent::PullRequestChanged(pull_request_id.into()))
        .unwrap();
    scheduler::flush_for_test(sched).await;
}

/// Run the scheduler's full reconcile pass and wait for it to finish.
async fn tick_over(sched: &UnboundedSender<SchedEvent>) {
    sched.send(SchedEvent::FullReconcile).unwrap();
    scheduler::flush_for_test(sched).await;
}

/// An open read waits before later scheduler ticks read the request again.
#[tokio::test]
async fn an_open_kept_request_is_read_once_per_wait() {
    let (_h, stub, _row, sched) = kept_request(0).await;

    tick_over(&sched).await;
    assert_eq!(stub.fetches(), 1, "the first tick reads the request once");

    for _ in 0..3 {
        tick_over(&sched).await;
    }
    assert_eq!(stub.fetches(), 1, "later ticks do not read inside the wait");
}

/// A request change bypasses the tick-only retry wait.
#[tokio::test]
async fn a_changed_open_kept_request_is_read_inside_the_wait() {
    let (_h, stub, row, sched) = kept_request(0).await;

    tick_over(&sched).await;
    assert_eq!(stub.fetches(), 1, "the first tick reads the request once");

    pass_over(&sched, &row.id).await;
    assert_eq!(
        stub.fetches(),
        2,
        "the request change reads the forge at once"
    );
}

/// A failed read starts its own retry wait before later scheduler ticks run.
#[tokio::test]
async fn a_failed_kept_request_read_waits_before_its_retry() {
    let (_h, stub, _row, sched) = kept_request(1).await;

    tick_over(&sched).await;
    assert_eq!(stub.fetches(), 1, "the first tick fails to read once");

    for _ in 0..3 {
        tick_over(&sched).await;
    }
    assert_eq!(
        stub.fetches(),
        1,
        "later ticks do not retry inside the wait"
    );
}
