//! Integration tests for the one end of a plan: the planner finalizes it.
//!
//! The plan is the tasks it wrote, and `finalize` is what hands them out: the
//! goal goes from `planning` straight to `active` and every task the plan
//! holds starts. Nobody else may make that call — a user session is refused —
//! and it is made once.
//!
//! Mostly no tmux and no agent CLI — the rows are seeded through the store and
//! the endpoints are exercised — except for the two tests about what the
//! scheduler makes of a finalized goal, which want a real scheduler and the
//! stub tmux's panes.

mod common;

use std::time::Duration;

use axum::http::StatusCode;

use ariadne_api::error::ErrorBody;
use ariadne_api::goals::GoalDto;
use ariadne_core::{GoalStatus, Role, SessionStatus, TaskStatus};
use ariadne_daemon::attention::work_is_active;

use common::{Cast, Harness, as_session, eventually, harness, post_json};

/// How long a test waits for the scheduler to come round to what it was told.
const TIMEOUT: Duration = Duration::from_secs(30);

fn finalize_uri(cast: &Cast) -> String {
    format!("/v1/goals/{}/finalize", cast.goal.id)
}

/// A live planner session on the goal, which is what a planner's calls come in
/// as.
async fn planner_session(h: &Harness, cast: &Cast) -> ariadne_store::AgentSession {
    h.session(&cast.goal, None, Role::Planner, &cast.planner.id)
        .await
}

/// The plan finalized by its planner, as the MCP tool finalizes it.
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
async fn the_planner_finalizes_the_plan_and_its_tasks_start() {
    let h = harness().scheduler().await;
    let cast = h.cast().await;
    let planner = planner_session(&h, &cast).await;

    let goal = finalize(&h, &cast, &planner.id).await;

    assert_eq!(goal.status, GoalStatus::Active);
    eventually(TIMEOUT, "the plan's task to reach an engineer", async || {
        matches!(
            h.status(&cast.task.id).await,
            TaskStatus::Ready | TaskStatus::InProgress
        )
    })
    .await;
}

/// The planner's call and nobody else's: the user has nothing to press.
#[tokio::test]
async fn only_the_planner_may_finalize_the_plan() {
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
        "only the planner may finalize the plan"
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
    let planner = h.profile("planner", Role::Planner).await;
    let (goal, _repo) = h.goal(&planner).await;
    let session = h.session(&goal, None, Role::Planner, &planner.id).await;

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

    assert_eq!(envelope.error.message, "cannot finalize a plan with no tasks");
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
    let planner = planner_session(&h, &cast).await;
    finalize(&h, &cast, &planner.id).await;

    let envelope: ErrorBody = h
        .json(
            as_session(&finalize_uri(&cast), &planner.id, serde_json::json!({})),
            StatusCode::CONFLICT,
        )
        .await;

    assert_eq!(envelope.error.message, "goal is active, expected planning");
}

/// The plan is the planner's whole job: while the goal is being planned its
/// session is the agent work waits on, and finalizing is what ends that.
#[tokio::test]
async fn a_planner_is_the_agent_work_waits_on_until_it_finalizes() {
    let h = harness().await;
    let cast = h.cast().await;
    let planner = planner_session(&h, &cast).await;

    assert!(work_is_active(&h.store, &planner).await);

    finalize(&h, &cast, &planner.id).await;
    assert!(!work_is_active(&h.store, &planner).await);
}

/// What a reconciliation pass makes of a finalized goal: the plan it was
/// started for is being worked on, so an idle planner under it is let go —
/// once the compaction the finalized plan earned it is done, which the CLI
/// reports as a session start from `compact`.
#[tokio::test]
async fn a_scheduler_pass_ends_the_idle_planner_of_an_active_goal() {
    let h = harness().scheduler().await;
    let cast = h.cast().await;
    let planner = planner_session(&h, &cast).await;
    h.pane_exists(&planner);
    h.set_status(&planner, SessionStatus::Idle).await;
    finalize(&h, &cast, &planner.id).await;

    h.notify_goal(&cast.goal.id);

    eventually(TIMEOUT, "the planner to be told to compact", async || {
        h.pasted(&planner).contains("/compact")
    })
    .await;
    assert!(
        !h.killed_panes().contains(&planner.tmux_session),
        "the planner is not let go under its compaction"
    );
    h.ingest(
        &planner,
        "session_start",
        serde_json::json!({"hook_event_name": "SessionStart", "source": "compact"}),
    )
    .await;
    eventually(TIMEOUT, "the idle planner to be let go", async || {
        h.killed_panes().contains(&planner.tmux_session)
    })
    .await;
}
