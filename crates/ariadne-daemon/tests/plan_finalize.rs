//! Integration tests for the one end of a plan: the orchestrator finalizes it.
//!
//! The plan is the tasks it wrote, and `finalize` is what hands them out: the
//! goal goes from `planning` straight to `active` and every task the plan
//! holds starts. Nobody else may make that call — a user session is refused —
//! and it is made once.
//!
//! Mostly no agent is started — the rows are seeded through the store and
//! the endpoints are exercised — except for the two tests about what the
//! scheduler makes of a finalized goal, which want a real scheduler and the
//! stub ACP agent.

mod common;

use std::time::Duration;

use axum::http::StatusCode;

use ariadne_api::error::ErrorBody;
use ariadne_api::goals::GoalDto;
use ariadne_core::{GoalStatus, SessionStatus, TaskStatus};
use ariadne_daemon::attention::work_is_active;

use common::{Cast, Harness, as_session, eventually, harness, post_json};

/// How long a test waits for the scheduler to come round to what it was told.
const TIMEOUT: Duration = Duration::from_secs(30);

fn finalize_uri(cast: &Cast) -> String {
    format!("/v1/goals/{}/finalize", cast.goal.id)
}

/// A live orchestrator session on the goal, which is what an orchestrator's
/// calls come in as.
async fn orchestrator_session(h: &Harness, cast: &Cast) -> ariadne_store::AgentSession {
    h.orchestrator_session(&cast.goal).await
}

/// The plan finalized by its orchestrator, as the MCP tool finalizes it.
async fn finalize(h: &Harness, cast: &Cast, session_id: &str) -> GoalDto {
    h.json(
        as_session(&finalize_uri(cast), session_id, serde_json::json!({})),
        StatusCode::OK,
    )
    .await
}

async fn goal_status(h: &Harness, cast: &Cast) -> GoalStatus {
    h.store.get_goal(&cast.goal.id).await.unwrap().status()
}

/// Finalizing is what starts the work: the goal goes active and its tasks are
/// handed out.
#[tokio::test]
async fn the_orchestrator_finalizes_the_plan_and_its_tasks_start() {
    let h = harness().scheduler().await;
    let cast = h.cast().await;
    let orchestrator = orchestrator_session(&h, &cast).await;

    let goal = finalize(&h, &cast, &orchestrator.id).await;

    assert_eq!(goal.status, GoalStatus::Active);
    eventually(TIMEOUT, "the plan's task to reach an author", async || {
        matches!(
            h.status(&cast.task.id).await,
            TaskStatus::Ready | TaskStatus::InProgress
        )
    })
    .await;
}

/// The plan is written into Ariadne before the user agrees to it, and none of
/// it starts there.
///
/// The orchestrator creates every task while the goal is still in `planning`,
/// so the user reads and edits the real tasks rather than a description of
/// them, and says yes to what is already there. What the yes buys is
/// `finalize_plan` and nothing else.
///
/// So the wait here is on the scheduler *having reconciled this goal* — the
/// orchestrator it keeps up is the proof of that — and on the task not having
/// moved with it. Notifying the task directly is the path `create_task` takes,
/// and the one that would start a task the user has not seen.
#[tokio::test]
async fn the_tasks_of_a_plan_wait_for_the_yes_that_finalizes_it() {
    let h = harness().scheduler().await;
    let cast = h.cast().await;

    // A pass over the goal, and as many over the task as a plan being written
    // sends: every one of them finds a goal still in planning.
    for _ in 0..3 {
        h.notify_goal(&cast.goal.id);
        h.notify(&cast.task.id);
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    eventually(TIMEOUT, "the goal's orchestrator to be up", async || {
        !h.sessions_of_goal(&cast.goal.id).await.is_empty()
    })
    .await;

    assert_eq!(
        h.status(&cast.task.id).await,
        TaskStatus::Pending,
        "a task of a plan nobody has agreed to was started"
    );
    assert!(
        h.sessions_of(&cast.task.id).await.is_empty(),
        "an agent was staffed on a task the user has not seen"
    );

    // And the same task, on the same passes, once the plan is agreed.
    let orchestrator = orchestrator_session(&h, &cast).await;
    finalize(&h, &cast, &orchestrator.id).await;

    eventually(TIMEOUT, "the agreed plan's task to start", async || {
        h.status(&cast.task.id).await != TaskStatus::Pending
    })
    .await;
}

/// The orchestrator's call and nobody else's: the user has nothing to press.
#[tokio::test]
async fn only_the_orchestrator_may_finalize_the_plan() {
    let h = harness().await;
    let cast = h.cast().await;

    let envelope: ErrorBody = h
        .json(
            post_json(&finalize_uri(&cast), serde_json::json!({})),
            StatusCode::FORBIDDEN,
        )
        .await;

    assert_eq!(
        envelope.error.message,
        "only the orchestrator may finalize the plan"
    );
    assert_eq!(goal_status(&h, &cast).await, GoalStatus::Planning);
    assert_eq!(
        h.status(&cast.task.id).await,
        TaskStatus::Pending,
        "and a refused call starts nothing"
    );
}

/// A plan is a set of tasks, so a goal with none of them is nothing to
/// finalize.
#[tokio::test]
async fn a_plan_with_no_tasks_cannot_be_finalized() {
    let h = harness().await;
    let (goal, _repo) = h.goal().await;
    let session = h.orchestrator_session(&goal).await;

    let envelope: ErrorBody = h
        .json(
            as_session(
                &format!("/v1/goals/{}/finalize", goal.id),
                &session.id,
                serde_json::json!({}),
            ),
            StatusCode::CONFLICT,
        )
        .await;

    assert_eq!(
        envelope.error.message,
        "cannot finalize a plan with no tasks"
    );
    assert_eq!(
        h.store.get_goal(&goal.id).await.unwrap().status(),
        GoalStatus::Planning,
        "a refused call leaves the goal where it was"
    );
}

/// Planning ends once: a second call on a goal already being worked on is a
/// conflict, not a second round of hand-outs.
#[tokio::test]
async fn a_plan_is_finalized_only_out_of_planning() {
    let h = harness().await;
    let cast = h.cast().await;
    let orchestrator = orchestrator_session(&h, &cast).await;
    finalize(&h, &cast, &orchestrator.id).await;

    let envelope: ErrorBody = h
        .json(
            as_session(
                &finalize_uri(&cast),
                &orchestrator.id,
                serde_json::json!({}),
            ),
            StatusCode::CONFLICT,
        )
        .await;

    assert_eq!(envelope.error.message, "goal is active, expected planning");
}

/// The orchestrator is the agent work waits on for the whole goal, not only
/// while the plan is being written: it is what the user talks to about work
/// already running, and what the daemon tells when a task needs a decision.
/// The goal ending is what ends that.
#[tokio::test]
async fn an_orchestrator_is_the_agent_work_waits_on_until_the_goal_is_over() {
    let h = harness().await;
    let cast = h.cast().await;
    let orchestrator = orchestrator_session(&h, &cast).await;

    assert!(work_is_active(&h.store, &orchestrator).await);

    finalize(&h, &cast, &orchestrator.id).await;
    assert!(
        work_is_active(&h.store, &orchestrator).await,
        "the plan is a hand-off, not an ending"
    );

    h.store
        .set_goal_status(&cast.goal.id, GoalStatus::Completed)
        .await
        .unwrap();
    assert!(!work_is_active(&h.store, &orchestrator).await);
}

/// What a reconciliation pass makes of a finalized goal: the orchestrator is
/// left up, because the goal is not over — and left alone, because a goal
/// under way has nothing to say to it.
///
/// The hand-off used to be a message: the daemon asked the orchestrator to
/// compact the conversation that wrote the plan. It asks nothing now, and an
/// idle agent that nothing has happened on is sent nothing.
#[tokio::test]
async fn a_scheduler_pass_keeps_the_orchestrator_of_an_active_goal_and_types_nothing() {
    let h = harness().scheduler().await;
    let cast = h.cast().await;
    let orchestrator = orchestrator_session(&h, &cast).await;
    h.agent_runs(&orchestrator).await;
    h.set_status(&orchestrator, SessionStatus::Idle).await;
    finalize(&h, &cast, &orchestrator.id).await;

    for _ in 0..3 {
        h.notify_goal(&cast.goal.id);
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }

    assert!(
        h.agent_is_running(&orchestrator),
        "the orchestrator was let go once its plan was under way"
    );
    assert_eq!(
        h.prompts_to(&orchestrator),
        Vec::<String>::new(),
        "the daemon prompted an agent that had nothing waiting for it"
    );
}
