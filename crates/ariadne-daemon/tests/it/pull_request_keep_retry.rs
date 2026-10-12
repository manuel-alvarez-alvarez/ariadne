//! An author request with no live task to keep it, asked for by hand (032),
//! stays open past that ask: `keep_pass` probes the forge with `read_ended`
//! to tell whether it ended. That probe waits `CLEANUP_RETRY`
//! (`crate::scheduler::pull_requests`) between tries, the same as a failed
//! cleanup, rather than running on every scheduler tick (026).
//!
//! The row here asks for both a review and a babysit, neither on a model:
//! `wants_session` and `wants_babysit_session` are both false, so neither
//! ever starts a session — the review's own `done` needs no review asked,
//! and the babysit's own needs role `reviewer`, and neither holds. With no
//! session ever up, nothing but `keep_pass`'s own probe, and a direct wake
//! of the row, ever reads the forge, which keeps this test's count of
//! `gh pr view` calls free of anything but the probe under test.
use std::time::Duration;

use serde_json::{Value, json};

use crate::common::forge::{StubForgeCli, answer, enable_integration, seed_live, stub_forge_cli};
use crate::common::{Harness, QUIET, TIMEOUT, eventually, harness};
use ariadne_daemon::scheduler::SchedEvent;
use ariadne_daemon::timeouts::Timeouts;
use ariadne_store::{NewPullRequest, PullRequestRow};

/// How often the scheduler's full reconcile tick runs, sped up so several
/// ticks land well inside [`QUIET`] and well inside the 60-second
/// cleanup-retry wait.
const SWEEPS_SOON: Duration = Duration::from_millis(100);

const URL: &str = "https://github.com/acme/widgets/pull/1";
const HEAD_SHA: &str = "abc123";

/// What the stub forge answers for the request's own read: `pr view` exits
/// with `pr_view_exit`, so a test can make the scheduler's own probe of it
/// fail.
fn script(pr_view_exit: i32) -> Value {
    let read = json!({"number": 1, "url": URL,
        "title": "Fix widgets", "author": {"login": "me"}, "state": "OPEN", "isDraft": false,
        "headRefName": "fix", "headRefOid": HEAD_SHA,
        "headRepository": {"url": "https://github.com/acme/widgets"},
        "baseRefName": "main", "statusCheckRollup": [], "reviewDecision": "",
        "createdAt": "2026-10-01T00:00:00Z"});
    json!([answer(&["pr", "view"], pr_view_exit, &read.to_string())])
}

/// How many times the scheduler has read the request with `gh pr view`.
fn pr_view_calls(stub: &StubForgeCli) -> usize {
    stub.invocations()
        .iter()
        .filter(|i| {
            i.args.first().map(String::as_str) == Some("pr")
                && i.args.get(1).map(String::as_str) == Some("view")
        })
        .count()
}

/// A row Ariadne works on, asked for a review and a babysit by hand but on
/// no model, open, with nothing to keep it a task's: the one row
/// `keep_pass`'s own probe for its end ever touches.
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

/// Wake the scheduler about one request and wait for that pass to end.
async fn pass_over(h: &Harness, pull_request_id: &str) {
    h.sched
        .as_ref()
        .expect("this harness has a scheduler")
        .send(SchedEvent::PullRequestChanged(pull_request_id.into()))
        .unwrap();
    h.flush_scheduler().await;
}

/// An author request of mine, opened by no task, asked for a review and a
/// babysit by hand on no model: `keep_pass`'s own probe for its end costs
/// one `gh pr view` call, then none for the wait, across several scheduler
/// ticks; a change of the request still reads at once, inside the wait; and
/// a failed probe costs one call the same way, not one a tick.
#[tokio::test]
async fn an_open_kept_request_with_no_task_is_read_once_per_wait() {
    let stub = stub_forge_cli(script(0));
    let timeouts = Timeouts {
        full_reconcile: SWEEPS_SOON,
        ..Timeouts::default()
    };
    let h = harness()
        .scheduler()
        .timeouts(timeouts)
        .forge_cli(&stub)
        .await;
    let repo = h.repository(&h.at("widgets")).await;
    enable_integration(&h, &repo.id, "me").await;
    let row = asked_row(&h, &repo.id).await;

    // The scheduler's own tick finds the new row on its own, with no event
    // of this test's to wake it: that first read is the probe under test,
    // so nothing here wakes it directly ahead of it.
    eventually(
        TIMEOUT,
        "the scheduler's own first read of the request",
        async || pr_view_calls(&stub) >= 1,
    )
    .await;
    let baseline = pr_view_calls(&stub);
    assert_eq!(baseline, 1, "the tick reads the request once");

    tokio::time::sleep(QUIET).await;
    assert_eq!(
        pr_view_calls(&stub),
        baseline,
        "an open read waits before the scheduler reads the request again"
    );

    // A change of the request reads the forge at once, inside the wait.
    pass_over(&h, &row.id).await;
    assert_eq!(
        pr_view_calls(&stub),
        baseline + 1,
        "a change of the request is read at once"
    );

    // A failed read still costs one call, then none for the wait.
    stub.reprogram(script(1));
    pass_over(&h, &row.id).await;
    let after_failure = pr_view_calls(&stub);
    assert_eq!(
        after_failure,
        baseline + 2,
        "the failing read itself still runs once"
    );

    tokio::time::sleep(QUIET).await;
    assert_eq!(
        pr_view_calls(&stub),
        after_failure,
        "a failed read waits before the scheduler tries it again"
    );
}
