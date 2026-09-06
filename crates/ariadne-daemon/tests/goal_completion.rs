//! The other end of a goal: somebody says it is done.
//!
//! Whether a goal is *met* is a judgement about the work rather than about the
//! rows — every task can be finished and the goal still not be what the user
//! asked for — so the daemon does not make it. The orchestrator does, because
//! it holds the plan, and the user does, because it is their goal and one
//! whose orchestrator will not start is otherwise a goal nothing can close.
//!
//! What the daemon does hold either of them to is the part it can see: a goal
//! with a task still going is not one anybody may declare finished.
//!
//! No tmux and no agent CLI: the rows are seeded through the store, and the
//! calls are the ones the MCP server and the CLI make.

mod common;

use axum::http::StatusCode;

use ariadne_api::error::ErrorBody;
use ariadne_api::goals::GoalDto;
use ariadne_core::{Actor, GoalStatus, Seat, TaskStatus};

use common::{Cast, Harness, as_session, harness, post_json};

fn complete_uri(cast: &Cast) -> String {
    format!("/v1/goals/{}/complete", cast.goal.id)
}

/// Walk the goal's one task all the way to finished.
async fn land(h: &Harness, cast: &Cast) {
    h.advance(&cast.task, TaskStatus::UnderReview).await;
    for (status, actor) in [
        (TaskStatus::Approved, Actor::Daemon),
        (TaskStatus::Finished, Actor::Author),
    ] {
        h.store
            .transition_task(
                &cast.task.id,
                status,
                actor,
                None,
                (status == TaskStatus::Finished).then_some("cafe1234"),
            )
            .await
            .unwrap();
    }
}

/// The orchestrator ends the goal it planned, once its tasks are done.
#[tokio::test]
async fn the_orchestrator_completes_the_goal_it_planned() {
    let h = harness().await;
    let cast = h.active_cast().await;
    let orchestrator = h.orchestrator_session(&cast.goal, "orc").await;
    land(&h, &cast).await;

    let goal: GoalDto = h
        .json(
            as_session(
                &complete_uri(&cast),
                &orchestrator.id,
                serde_json::json!({}),
            ),
            StatusCode::OK,
        )
        .await;

    assert_eq!(goal.status, GoalStatus::Completed);
}

/// And so does the user, from the terminal: a goal whose orchestrator is gone
/// is otherwise one nothing can close.
#[tokio::test]
async fn the_user_completes_a_goal_of_their_own() {
    let h = harness().await;
    let cast = h.active_cast().await;
    land(&h, &cast).await;

    let goal: GoalDto = h
        .json(
            post_json(&complete_uri(&cast), serde_json::json!({})),
            StatusCode::OK,
        )
        .await;

    assert_eq!(goal.status, GoalStatus::Completed);
}

/// A task still going is what the daemon can see, and it is enough to refuse:
/// the message names the tasks, so whoever asked knows what is left.
#[tokio::test]
async fn a_goal_with_a_task_still_going_is_refused_by_name() {
    let h = harness().await;
    let cast = h.active_cast().await;
    let orchestrator = h.orchestrator_session(&cast.goal, "orc").await;
    h.advance(&cast.task, TaskStatus::InProgress).await;

    let envelope: ErrorBody = h
        .json(
            as_session(
                &complete_uri(&cast),
                &orchestrator.id,
                serde_json::json!({}),
            ),
            StatusCode::CONFLICT,
        )
        .await;

    assert!(
        envelope
            .error
            .message
            .starts_with("the goal still has unfinished tasks"),
        "{}",
        envelope.error.message
    );
    assert!(
        envelope.error.message.contains(&cast.task.title),
        "{}",
        envelope.error.message
    );
}

/// A cancelled task counts as done: a goal that ended one of its tasks and
/// landed the rest is still a goal that is over.
#[tokio::test]
async fn a_cancelled_task_is_no_bar_to_completing_the_goal() {
    let h = harness().await;
    let cast = h.active_cast().await;
    h.store
        .transition_task(
            &cast.task.id,
            TaskStatus::Cancelled,
            Actor::User,
            Some("not needed after all"),
            None,
        )
        .await
        .unwrap();

    let goal: GoalDto = h
        .json(
            post_json(&complete_uri(&cast), serde_json::json!({})),
            StatusCode::OK,
        )
        .await;

    assert_eq!(goal.status, GoalStatus::Completed);
}

/// Neither an author nor a reviewer may end the goal: each of them sees one
/// task, and the goal is the plan that holds them all.
#[tokio::test]
async fn an_agent_below_the_orchestrator_may_not_complete_the_goal() {
    let h = harness().await;
    let cast = h.active_cast().await;
    land(&h, &cast).await;

    for (seat, agent) in [
        (Seat::Author, &cast.author.id),
        (Seat::Reviewer, &cast.reviewer.id),
    ] {
        let session = h.session(&cast.goal, Some(&cast.task), seat, agent).await;
        let (status, _) = h
            .send(as_session(
                &complete_uri(&cast),
                &session.id,
                serde_json::json!({}),
            ))
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{seat:?}");
    }
    assert_eq!(
        h.store.get_goal(&cast.goal.id).await.unwrap().status(),
        GoalStatus::Active,
        "a refused call leaves the goal where it was"
    );
}

/// A goal is completed once: a second call on one already over is a conflict,
/// not a no-op.
#[tokio::test]
async fn a_goal_is_completed_only_out_of_active() {
    let h = harness().await;
    let cast = h.active_cast().await;
    land(&h, &cast).await;
    h.json::<GoalDto>(
        post_json(&complete_uri(&cast), serde_json::json!({})),
        StatusCode::OK,
    )
    .await;

    let envelope: ErrorBody = h
        .json(
            post_json(&complete_uri(&cast), serde_json::json!({})),
            StatusCode::CONFLICT,
        )
        .await;

    assert_eq!(envelope.error.message, "goal is completed, expected active");
}
