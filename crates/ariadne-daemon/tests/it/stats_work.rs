//! Integration tests for the `work` stats family (023): what got done.
//!
//! The `goal_ended` fact itself (`Store::set_goal_status`) and the route's
//! aggregate (`Store::work_stats`) are covered here; `Store::work_stats`'s
//! own counting and bucketing rules have their own tests in
//! `ariadne-store`'s `stats::work`.

use crate::common;

use ariadne_core::{Actor, GoalStatus, TaskStatus};
use ariadne_store::Task;

use common::{Harness, harness, test_pin};

/// Run a task through its columns to `finished`: the daemon starts it, and
/// its last column reports the commit it landed as, where it landed one.
async fn finish(h: &Harness, task: &Task, merge_commit: Option<&str>) {
    h.store
        .transition_task(&task.id, TaskStatus::Ready, Actor::Daemon, None, None)
        .await
        .unwrap();
    h.store
        .transition_task(&task.id, TaskStatus::InProgress, Actor::Daemon, None, None)
        .await
        .unwrap();
    h.store
        .transition_task(
            &task.id,
            TaskStatus::Finished,
            Actor::Agent,
            None,
            merge_commit,
        )
        .await
        .unwrap();
}

/// A goal completed writes one `goal_ended` fact: its status, a lead time
/// counted from its own creation, its task counts and its workflow. A second
/// move to the same status writes no second one.
#[tokio::test]
async fn a_completed_goal_writes_one_goal_ended_fact() {
    let h = harness().await;
    let (goal, repo) = h.goal().await;
    let finished = h.task_on(&goal, &repo, "finished task", test_pin()).await;
    let _pending = h.task_on(&goal, &repo, "pending task", test_pin()).await;
    finish(&h, &finished, Some("abc123")).await;

    h.store
        .set_goal_status(&goal.id, GoalStatus::Completed)
        .await
        .unwrap();

    let facts = h.facts("goal_ended").await;
    assert_eq!(facts.len(), 1, "{facts:?}");
    let fact = &facts[0];
    assert_eq!(fact.data["status"], "completed");
    assert_eq!(fact.data["tasks"], 2);
    assert_eq!(fact.data["tasks_finished"], 1);
    assert!(fact.data["lead_time_secs"].as_i64().unwrap() >= 0);
    assert_eq!(fact.data["workflow"], goal.workflow);
    assert_eq!(fact.model.as_deref(), Some(goal.model.as_str()));

    // The same status again writes no second fact.
    h.store
        .set_goal_status(&goal.id, GoalStatus::Completed)
        .await
        .unwrap();
    assert_eq!(h.facts("goal_ended").await.len(), 1);
}

/// A goal cancelled writes one `goal_ended` fact too, with its status.
#[tokio::test]
async fn a_cancelled_goal_writes_one_goal_ended_fact() {
    let h = harness().await;
    let (goal, _repo) = h.goal().await;

    h.store
        .set_goal_status(&goal.id, GoalStatus::Cancelled)
        .await
        .unwrap();

    let facts = h.facts("goal_ended").await;
    assert_eq!(facts.len(), 1, "{facts:?}");
    assert_eq!(facts[0].data["status"], "cancelled");
    assert_eq!(facts[0].data["tasks"], 0);
    assert_eq!(facts[0].data["tasks_finished"], 0);
}

/// `GET /v1/stats/work` answers the totals and the buckets off the ledger —
/// a task counts as landed where its `task_ended` fact says its change
/// reached the base — and the route is in the API document under the
/// `stats` tag.
#[tokio::test]
async fn the_work_stat_answers_the_totals_and_the_buckets() {
    let h = harness().await;
    let (goal, repo) = h.goal().await;
    let landed = h.task_on(&goal, &repo, "landed", test_pin()).await;
    let unlanded = h
        .task_on(&goal, &repo, "finished unlanded", test_pin())
        .await;
    finish(&h, &landed, Some("abc123")).await;
    finish(&h, &unlanded, None).await;
    h.store
        .set_goal_status(&goal.id, GoalStatus::Completed)
        .await
        .unwrap();

    let facts = h.facts("task_ended").await;
    assert_eq!(facts.len(), 2, "{facts:?}");
    assert_eq!(facts[0].data["landed"], true);
    assert_eq!(facts[1].data["landed"], false);

    let stats: serde_json::Value = h.get("/v1/stats/work").await;
    assert_eq!(stats["totals"]["tasks_finished"], 2);
    assert_eq!(stats["totals"]["landed"], 1);
    assert_eq!(stats["totals"]["goals_completed"], 1);
    assert_eq!(stats["buckets"].as_array().unwrap().len(), 1);
    assert_eq!(stats["buckets"][0]["tasks_finished"], 2);
    assert_eq!(stats["buckets"][0]["landed"], 1);
    assert_eq!(stats["buckets"][0]["goals_completed"], 1);

    let doc: serde_json::Value = h.get("/api-docs/openapi.json").await;
    assert_eq!(doc["paths"]["/v1/stats/work"]["get"]["tags"][0], "stats");
}
