//! The way out of a task nobody can do: the engineer gives it up.
//!
//! `fail_task` is one transition, and the reason it carries is the whole of
//! what the user is told — there is nowhere else it could be said. So the two
//! things this pins are that the engineer is allowed to make the move at all,
//! and that what it said comes back on the task rather than only in the audit
//! log a person has to go and read.
//!
//! No tmux and no agent CLI: the sessions here are rows, and the calls are the
//! ones the MCP server makes on the engineer's behalf.

mod common;

use axum::http::StatusCode;

use ariadne_api::tasks::TaskDto;
use ariadne_core::{Role, TaskStatus};

use common::{Cast, Harness, as_session, harness};

/// An engineer session on the goal's one task, which is what its calls come in
/// as.
async fn engineer_session(h: &Harness, cast: &Cast) -> ariadne_store::AgentSession {
    h.session(
        &cast.goal,
        Some(&cast.task),
        Role::Engineer,
        &cast.engineer.id,
    )
    .await
}

fn transitions_uri(cast: &Cast) -> String {
    format!("/v1/tasks/{}/transitions", cast.task.id)
}

/// The engineer of a task that cannot be done as written ends it, and the
/// reason it gave is on the task from then on.
#[tokio::test]
async fn an_engineer_fails_its_own_task_with_the_reason_on_it() {
    const REASON: &str = "the crate the task names was deleted upstream";

    let h = harness().await;
    let cast = h.active_cast().await;
    let engineer = engineer_session(&h, &cast).await;
    h.advance(&cast.task, TaskStatus::InProgress).await;

    let failed: TaskDto = h
        .json(
            as_session(
                &transitions_uri(&cast),
                &engineer.id,
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

/// Only the engineer that owns the task, though. A reviewer reaching for the
/// move is refused by the state machine, and the task is left where it was.
#[tokio::test]
async fn a_reviewer_may_not_fail_the_task_it_is_reviewing() {
    let h = harness().await;
    let cast = h.active_cast().await;
    h.advance(&cast.task, TaskStatus::UnderReview).await;
    let reviewer = h
        .session(
            &cast.goal,
            Some(&cast.task),
            Role::Reviewer,
            &cast.reviewer.id,
        )
        .await;

    let refusal = h
        .error(
            as_session(
                &transitions_uri(&cast),
                &reviewer.id,
                serde_json::json!({"to": "failed", "reason": "I would rather not"}),
            ),
            StatusCode::CONFLICT,
        )
        .await;
    assert!(
        refusal.error.message.contains("reviewer"),
        "the refusal names who asked: {}",
        refusal.error.message
    );
    assert_eq!(h.status(&cast.task.id).await, TaskStatus::UnderReview);
}

/// A task that ended with nothing said carries nothing, and one that is still
/// being worked on carries nothing either: `reason` is why an ended task
/// ended, not the last thing anybody wrote about it.
#[tokio::test]
async fn a_task_that_has_not_ended_carries_no_reason() {
    let h = harness().await;
    let cast = h.active_cast().await;
    let engineer = engineer_session(&h, &cast).await;

    let live: TaskDto = h.get(&format!("/v1/tasks/{}", cast.task.id)).await;
    assert_eq!(live.reason, None);

    // A review request carries a summary as its reason, which is the round's
    // and not the task's ending.
    h.advance(&cast.task, TaskStatus::InProgress).await;
    let _: TaskDto = h
        .json(
            as_session(
                &transitions_uri(&cast),
                &engineer.id,
                serde_json::json!({"to": "under_review", "reason": "rewrote the parser"}),
            ),
            StatusCode::OK,
        )
        .await;
    let reviewed: TaskDto = h.get(&format!("/v1/tasks/{}", cast.task.id)).await;
    assert_eq!(reviewed.reason, None);
}
