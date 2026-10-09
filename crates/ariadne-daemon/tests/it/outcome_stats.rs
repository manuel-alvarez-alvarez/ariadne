//! Integration tests for the stats ledger (023): the `task_ended` fact a
//! task writes once it reaches `finished`, `cancelled` or `failed`, filled
//! from the agent of the column it ended in.

use crate::common;

use ariadne_core::{Actor, TaskStatus};
use chrono::{Duration as ChronoDuration, Utc};

use common::{Harness, harness};

/// Walk a fresh task from `pending` into its first column, with no agent
/// started: the sessions here are rows, and the moves are the ones the
/// daemon itself makes between them.
async fn start(h: &Harness, task_id: &str) {
    h.store
        .transition_task(task_id, TaskStatus::Ready, Actor::Daemon, None, None)
        .await
        .unwrap();
    h.store
        .transition_task(task_id, TaskStatus::InProgress, Actor::Daemon, None, None)
        .await
        .unwrap();
}

async fn transition_at(
    h: &Harness,
    task_id: &str,
    to: TaskStatus,
    actor: Actor,
    reason: Option<&str>,
    at: String,
) {
    h.store
        .transition_task_at(task_id, to, actor, reason, None, &at)
        .await
        .unwrap();
}

/// A task that finishes writes one `task_ended` fact: its status, the column
/// it ended in and that column's agent — its model and its skills — whether
/// its change landed, and a lead time counted from its own creation.
#[tokio::test]
async fn a_finished_task_writes_one_task_ended_fact() {
    let h = harness().await;
    let cast = h.cast().await;
    start(&h, &cast.task.id).await;
    assert_eq!(
        h.store
            .get_task(&cast.task.id)
            .await
            .unwrap()
            .step
            .as_deref(),
        Some("develop"),
        "the daemon put the task in its first column"
    );
    h.store
        .transition_task(
            &cast.task.id,
            TaskStatus::Finished,
            Actor::Agent,
            Some("The change is on the base."),
            Some("abc123"),
        )
        .await
        .unwrap();

    let facts = h.facts("task_ended").await;
    assert_eq!(facts.len(), 1, "{facts:?}");
    let fact = &facts[0];
    assert_eq!(fact.model.as_deref(), Some("stub:test-model"));
    assert_eq!(fact.seat.as_deref(), Some("agent"));
    assert_eq!(fact.skills, ["coding"], "the develop column's own skills");
    assert_eq!(fact.data["status"], "finished");
    assert_eq!(fact.data["step"], "develop");
    assert_eq!(fact.data["landed"], true);
    assert_eq!(fact.data["reason"], "The change is on the base.");
    assert!(fact.data["lead_time_secs"].as_i64().unwrap() >= 0);
    for status in ["pending", "ready", "in_progress"] {
        assert!(
            fact.data["status_secs"][status].is_number(),
            "{status}: {fact:?}"
        );
    }
    for gone in ["landing", "review_requests", "authors", "picked"] {
        assert!(fact.data.get(gone).is_none(), "{gone} is no fact: {fact:?}");
    }
}

/// A task cancelled before it entered a column names no agent: the fact has
/// no seat, no model, no skills and no column, and its change never landed.
#[tokio::test]
async fn a_task_cancelled_before_its_first_column_names_no_agent() {
    let h = harness().await;
    let cast = h.cast().await;
    h.store
        .transition_task(
            &cast.task.id,
            TaskStatus::Cancelled,
            Actor::User,
            Some("Not wanted."),
            None,
        )
        .await
        .unwrap();

    let facts = h.facts("task_ended").await;
    assert_eq!(facts.len(), 1, "{facts:?}");
    let fact = &facts[0];
    assert_eq!(fact.data["status"], "cancelled");
    assert_eq!(fact.seat, None);
    assert_eq!(fact.model, None);
    assert!(fact.skills.is_empty());
    assert!(fact.data.get("step").is_none(), "{fact:?}");
    assert_eq!(fact.data["landed"], false);
}

/// A task retried after it failed, and that fails again, writes a fact for
/// each of the two endings, each with the time the task spent `ready` before
/// that ending.
#[tokio::test]
async fn a_retried_task_that_fails_again_writes_two_facts() {
    let h = harness().await;
    let cast = h.cast().await;
    let clock = Utc::now() - ChronoDuration::seconds(6);
    let at = |seconds| (clock + ChronoDuration::seconds(seconds)).to_rfc3339();
    transition_at(
        &h,
        &cast.task.id,
        TaskStatus::Ready,
        Actor::Daemon,
        None,
        at(0),
    )
    .await;
    transition_at(
        &h,
        &cast.task.id,
        TaskStatus::InProgress,
        Actor::Daemon,
        None,
        at(1),
    )
    .await;
    transition_at(
        &h,
        &cast.task.id,
        TaskStatus::Failed,
        Actor::Agent,
        Some("could not do it"),
        at(2),
    )
    .await;
    transition_at(
        &h,
        &cast.task.id,
        TaskStatus::Ready,
        Actor::User,
        None,
        at(3),
    )
    .await;
    transition_at(
        &h,
        &cast.task.id,
        TaskStatus::InProgress,
        Actor::Daemon,
        None,
        at(4),
    )
    .await;
    transition_at(
        &h,
        &cast.task.id,
        TaskStatus::Failed,
        Actor::Agent,
        Some("still could not do it"),
        at(5),
    )
    .await;

    let facts = h.facts("task_ended").await;
    assert_eq!(facts.len(), 2, "{facts:?}");
    assert!(facts.iter().all(|f| f.data["status"] == "failed"));
    assert!(
        facts.iter().all(|f| f.data["step"] == "develop"),
        "both endings happened in the first column: {facts:?}"
    );
    assert_eq!(facts[0].data["status_secs"]["ready"], 1, "{facts:?}");
    assert_eq!(facts[1].data["status_secs"]["ready"], 2, "{facts:?}");
}
