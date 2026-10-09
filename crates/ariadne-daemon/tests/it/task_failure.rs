//! The way out of a task nobody can do: its agent gives it up.
//!
//! `fail_task` is one transition, and the reason it carries is the whole of
//! what the user is told — there is nowhere else it could be said. So the two
//! things this pins are that a column's agent is allowed to make the move at
//! all, and that what it said comes back on the task rather than only in the
//! audit log a person has to go and read.
//!
//! No agent is started: the sessions here are rows, and the calls are the
//! ones the MCP server makes on the agent's behalf.

use crate::common;

use axum::http::StatusCode;

use ariadne_api::tasks::TaskDto;
use ariadne_core::TaskStatus;

use common::{Cast, harness};

fn transitions_uri(cast: &Cast) -> String {
    format!("/v1/tasks/{}/transitions", cast.task.id)
}

/// The agent of the column a task stands in ends the task that cannot be done
/// as written, and the reason it gave is on the task from then on.
#[tokio::test]
async fn a_column_agent_fails_its_own_task_with_the_reason_on_it() {
    const REASON: &str = "the crate the task names was deleted upstream";

    let h = harness().await;
    let cast = h.active_cast().await;
    let develop = h.agent_session(&cast, "develop").await;
    h.advance(&cast.task, TaskStatus::InProgress).await;

    let failed: TaskDto = h
        .json(
            common::as_session(
                &transitions_uri(&cast),
                &develop.id,
                serde_json::json!({"to": "failed", "reason": REASON}),
            ),
            StatusCode::OK,
        )
        .await;

    assert_eq!(failed.status, TaskStatus::Failed);
    assert_eq!(failed.reason.as_deref(), Some(REASON));

    // And a later read says the same: the reason is the task's, not something
    // the answer to one call happened to carry.
    let read: TaskDto = h.get(&format!("/v1/tasks/{}", cast.task.id)).await;
    assert_eq!(read.reason.as_deref(), Some(REASON));
}

/// Giving a task up is its agents' move and the daemon's. The orchestrator
/// holds the plan and has other answers — cancel it, rewrite it — so the
/// state machine refuses it the move by name, and the task is left where it
/// was.
#[tokio::test]
async fn the_orchestrator_may_not_fail_a_task_and_the_task_stays_where_it_was() {
    let h = harness().await;
    let cast = h.active_cast().await;
    h.advance_to(&cast.task, "review").await;
    let orchestrator = h.orchestrator_session(&cast.goal).await;

    let refusal = h
        .error(
            common::as_session(
                &transitions_uri(&cast),
                &orchestrator.id,
                serde_json::json!({"to": "failed", "reason": "I would rather not"}),
            ),
            StatusCode::CONFLICT,
        )
        .await;
    assert!(
        refusal.error.message.contains("orchestrator"),
        "the refusal names who asked: {}",
        refusal.error.message
    );
    let task = h.store.get_task(&cast.task.id).await.unwrap();
    assert_eq!(task.status(), TaskStatus::InProgress);
    assert_eq!(task.step.as_deref(), Some("review"));
}

/// A task that ended with nothing said carries nothing, and one that is still
/// being worked on carries nothing either: `reason` is why an ended task
/// ended, not the last thing anybody wrote about it.
#[tokio::test]
async fn a_task_that_has_not_ended_carries_no_reason() {
    let h = harness().await;
    let cast = h.active_cast().await;

    let live: TaskDto = h.get(&format!("/v1/tasks/{}", cast.task.id)).await;
    assert_eq!(live.reason, None);

    // A move from one column to the next carries a reason, which is the
    // move's — the next column's agent reads it — and not the task's ending.
    h.advance_to(&cast.task, "review").await;
    let moved = h
        .store
        .list_task_transitions(&cast.task.id)
        .await
        .unwrap()
        .pop()
        .unwrap();
    assert!(
        moved.reason.is_some(),
        "the column entry says why the task moved: {moved:?}"
    );
    let reviewed: TaskDto = h.get(&format!("/v1/tasks/{}", cast.task.id)).await;
    assert_eq!(reviewed.reason, None);
}
