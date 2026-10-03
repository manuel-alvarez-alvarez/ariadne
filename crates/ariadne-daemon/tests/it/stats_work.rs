//! Integration tests for the `work` stats family (023): what got done.
//!
//! The `goal_ended` fact itself (`Store::set_goal_status`) and the route's
//! aggregate (`Store::work_stats`) are covered here; `Store::work_stats`'s
//! own counting and bucketing rules have their own tests in
//! `ariadne-store`'s `stats::work`.

use crate::common;

use ariadne_core::{Actor, GoalStatus, TaskStatus};

use common::{harness, test_pin};

/// A goal completed writes one `goal_ended` fact: its status, a lead time
/// counted from its own creation, its task counts and its landing. A second
/// move to the same status writes no second one.
#[tokio::test]
async fn a_completed_goal_writes_one_goal_ended_fact() {
    let h = harness().await;
    let (goal, repo) = h.goal().await;
    let finished = h
        .task_on(&goal, &repo, "finished task", 0, test_pin())
        .await;
    let _pending = h.task_on(&goal, &repo, "pending task", 0, test_pin()).await;
    h.store
        .transition_task(&finished.id, TaskStatus::Ready, Actor::Daemon, None, None)
        .await
        .unwrap();
    h.store
        .transition_task(
            &finished.id,
            TaskStatus::InProgress,
            Actor::Daemon,
            None,
            None,
        )
        .await
        .unwrap();
    h.store
        .transition_task(
            &finished.id,
            TaskStatus::UnderReview,
            Actor::Author,
            None,
            None,
        )
        .await
        .unwrap();
    h.store
        .transition_task(
            &finished.id,
            TaskStatus::Approved,
            Actor::Daemon,
            None,
            None,
        )
        .await
        .unwrap();
    h.store
        .transition_task(
            &finished.id,
            TaskStatus::Finished,
            Actor::Author,
            None,
            Some("abc123"),
        )
        .await
        .unwrap();

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
    assert_eq!(fact.data["landing"], goal.landing);

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

/// `GET /v1/stats/work` answers the totals and the buckets off the ledger,
/// and the route is in the API document under the `stats` tag.
#[tokio::test]
async fn the_work_stat_answers_the_totals_and_the_buckets() {
    let h = harness().await;
    let (goal, repo) = h.goal().await;
    let task = h.task_on(&goal, &repo, "task", 0, test_pin()).await;
    h.store
        .transition_task(&task.id, TaskStatus::Ready, Actor::Daemon, None, None)
        .await
        .unwrap();
    h.store
        .transition_task(&task.id, TaskStatus::InProgress, Actor::Daemon, None, None)
        .await
        .unwrap();
    h.store
        .transition_task(&task.id, TaskStatus::UnderReview, Actor::Author, None, None)
        .await
        .unwrap();
    h.store
        .transition_task(&task.id, TaskStatus::Approved, Actor::Daemon, None, None)
        .await
        .unwrap();
    h.store
        .transition_task(
            &task.id,
            TaskStatus::Finished,
            Actor::Author,
            None,
            Some("abc123"),
        )
        .await
        .unwrap();
    h.store
        .set_goal_status(&goal.id, GoalStatus::Completed)
        .await
        .unwrap();

    let stats: serde_json::Value = h.get("/v1/stats/work").await;
    assert_eq!(stats["totals"]["tasks_finished"], 1);
    assert_eq!(stats["totals"]["goals_completed"], 1);
    assert_eq!(stats["buckets"].as_array().unwrap().len(), 1);
    assert_eq!(stats["buckets"][0]["tasks_finished"], 1);
    assert_eq!(stats["buckets"][0]["goals_completed"], 1);

    let doc: serde_json::Value = h.get("/api-docs/openapi.json").await;
    assert_eq!(doc["paths"]["/v1/stats/work"]["get"]["tags"][0], "stats");
}
