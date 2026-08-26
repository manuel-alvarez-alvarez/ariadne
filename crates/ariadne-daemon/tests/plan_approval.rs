//! Integration tests for the two ends of a plan: the planner submits it, the
//! user approves it.
//!
//! Between the two the goal waits in `plan_ready`, which is a status where
//! nothing at all runs. The planner has said the plan is finished, the user
//! has the tasks in front of them to read and edit, and the only call that
//! starts an engineer is the user's own `finalize`. A plan sent back for
//! changes never leaves `plan_ready`: the planner reworks it there and submits
//! again.
//!
//! Mostly no tmux and no agent CLI — the rows are seeded through the store and
//! the endpoints are exercised — except for the two tests about what a
//! scheduler pass makes of a goal in each of the two statuses, which want a
//! real scheduler and the stub tmux's panes.

mod common;

use std::time::Duration;

use axum::http::StatusCode;

use ariadne_api::error::ErrorBody;
use ariadne_api::goals::GoalDto;
use ariadne_api::tasks::TaskDto;
use ariadne_core::{AuthorRole, GoalStatus, Role, SessionStatus, TaskStatus};
use ariadne_daemon::attention::work_is_active;
use ariadne_store::Recipient;

use common::{Cast, Harness, as_session, eventually, harness, patch_json, post_json};

/// How long a test waits for the scheduler to come round to what it was told.
const TIMEOUT: Duration = Duration::from_secs(30);

fn submit_uri(cast: &Cast) -> String {
    format!("/v1/goals/{}/submit", cast.goal.id)
}

fn finalize_uri(cast: &Cast) -> String {
    format!("/v1/goals/{}/finalize", cast.goal.id)
}

/// A live planner session on the goal, which is what a planner's calls come in
/// as.
async fn planner_session(h: &Harness, cast: &Cast) -> ariadne_store::AgentSession {
    h.session(&cast.goal, None, Role::Planner, &cast.planner.id)
        .await
}

/// The plan submitted by its planner, as the MCP tool submits it.
async fn submit(h: &Harness, cast: &Cast, session_id: &str, summary: &str) -> GoalDto {
    h.json(
        as_session(
            &submit_uri(cast),
            session_id,
            serde_json::json!({"summary": summary}),
        ),
        StatusCode::OK,
    )
    .await
}

/// The plan approved by the user, from the terminal or the UI.
async fn finalize(h: &Harness, cast: &Cast, summary: &str) -> GoalDto {
    h.json(
        post_json(&finalize_uri(cast), serde_json::json!({"summary": summary})),
        StatusCode::OK,
    )
    .await
}

async fn goal_status(h: &Harness, cast: &Cast) -> GoalStatus {
    h.store.get_goal(&cast.goal.id).await.unwrap().status()
}

/// Submitting hands the plan over and starts nothing: the goal waits in
/// `plan_ready`, the user is told so in the goal thread, and the tasks are
/// exactly where the planner left them.
#[tokio::test]
async fn a_submitted_plan_waits_for_the_user() {
    let h = harness().await;
    let cast = h.cast().await;
    let planner = planner_session(&h, &cast).await;

    let goal = submit(&h, &cast, &planner.id, "three tasks, ui last").await;

    assert_eq!(goal.status, GoalStatus::PlanReady);
    assert_eq!(
        h.status(&cast.task.id).await,
        TaskStatus::Pending,
        "submitting a plan starts nothing"
    );

    let thread = h.goal_thread(&cast.goal).await;
    let [message] = thread.as_slice() else {
        panic!("one message in the thread, got {thread:?}");
    };
    assert_eq!(
        message.body,
        "Plan submitted for approval: three tasks, ui last"
    );
    assert_eq!(
        message.recipient(),
        Some(Recipient::User),
        "the plan is handed to the user, so the message is addressed to them"
    );
    assert_eq!(message.author_role, AuthorRole::Planner.as_str());
}

/// A plan is a set of tasks, so a goal with none of them is nothing to submit.
#[tokio::test]
async fn a_plan_with_no_tasks_cannot_be_submitted() {
    let h = harness().await;
    let planner = h.profile("planner", Role::Planner).await;
    let (goal, _repo) = h.goal(&planner).await;
    let session = h.session(&goal, None, Role::Planner, &planner.id).await;

    let envelope: ErrorBody = h
        .json(
            as_session(
                &format!("/v1/goals/{}/submit", goal.id),
                &session.id,
                serde_json::json!({"summary": "nothing yet"}),
            ),
            StatusCode::CONFLICT,
        )
        .await;
    assert_eq!(envelope.error.message, "cannot submit a plan with no tasks");
    assert_eq!(
        h.store.get_goal(&goal.id).await.unwrap().status(),
        GoalStatus::Planning,
        "a refused submission leaves the goal where it was"
    );
}

/// The planner submits; approving is the user's, and nobody else's.
#[tokio::test]
async fn the_planner_may_not_approve_its_own_plan() {
    let h = harness().await;
    let cast = h.cast().await;
    let planner = planner_session(&h, &cast).await;
    submit(&h, &cast, &planner.id, "ready").await;

    let envelope: ErrorBody = h
        .json(
            as_session(
                &finalize_uri(&cast),
                &planner.id,
                serde_json::json!({"summary": "and off we go"}),
            ),
            StatusCode::FORBIDDEN,
        )
        .await;
    assert_eq!(
        envelope.error.message,
        "only the user may finalize the plan"
    );
    assert_eq!(goal_status(&h, &cast).await, GoalStatus::PlanReady);
}

/// The user's approval is what starts the work: the goal goes active and its
/// tasks are handed out.
#[tokio::test]
async fn the_user_approves_a_submitted_plan() {
    let h = harness().scheduler().await;
    let cast = h.cast().await;
    let planner = planner_session(&h, &cast).await;
    submit(&h, &cast, &planner.id, "ready").await;

    let goal = finalize(&h, &cast, "looks right").await;

    assert_eq!(goal.status, GoalStatus::Active);
    eventually(TIMEOUT, "the approved plan's task to start", async || {
        matches!(
            h.status(&cast.task.id).await,
            TaskStatus::Ready | TaskStatus::InProgress
        )
    })
    .await;
}

/// And a plan the planner never handed over is the user's to approve all the
/// same: they have read it in the thread, and waiting for the hand-over would
/// be waiting on an agent for nothing.
#[tokio::test]
async fn the_user_approves_a_plan_that_was_never_submitted() {
    let h = harness().await;
    let cast = h.cast().await;

    let goal = finalize(&h, &cast, "no need to wait").await;

    assert_eq!(goal.status, GoalStatus::Active);
    let bodies: Vec<String> = h
        .goal_thread(&cast.goal)
        .await
        .into_iter()
        .map(|m| m.body)
        .collect();
    assert_eq!(bodies, vec!["Plan finalized: no need to wait"]);
}

/// The user asks for changes, the planner reworks the plan and hands it over
/// again: a second submission is another notice in the thread, and the goal
/// never falls back to `planning` in between.
#[tokio::test]
async fn a_reworked_plan_is_submitted_again() {
    let h = harness().await;
    let cast = h.cast().await;
    let planner = planner_session(&h, &cast).await;

    submit(&h, &cast, &planner.id, "first attempt").await;
    let goal = submit(&h, &cast, &planner.id, "split the ui task in two").await;

    assert_eq!(goal.status, GoalStatus::PlanReady);
    let bodies: Vec<String> = h
        .goal_thread(&cast.goal)
        .await
        .into_iter()
        .map(|m| m.body)
        .collect();
    assert_eq!(
        bodies,
        vec![
            "Plan submitted for approval: first attempt",
            "Plan submitted for approval: split the ui task in two",
        ]
    );
}

/// Reading the plan is one thing and editing it another: a task of a goal
/// waiting for approval is still the user's to rewrite, which is the whole
/// reason the goal waits.
#[tokio::test]
async fn the_user_edits_a_task_while_the_plan_waits() {
    let h = harness().await;
    let cast = h.cast().await;
    let planner = planner_session(&h, &cast).await;
    submit(&h, &cast, &planner.id, "ready").await;

    let edited: TaskDto = h
        .json(
            patch_json(
                &format!("/v1/tasks/{}", cast.task.id),
                serde_json::json!({"title": "the same task, better scoped"}),
            ),
            StatusCode::OK,
        )
        .await;

    assert_eq!(edited.title, "the same task, better scoped");
    assert_eq!(goal_status(&h, &cast).await, GoalStatus::PlanReady);
}

/// A planner whose plan is waiting is still working: the user may send it
/// back, so whatever its pane asks for is asked of somebody.
#[tokio::test]
async fn a_planner_of_a_waiting_plan_is_still_the_agent_work_waits_on() {
    let h = harness().await;
    let cast = h.cast().await;
    let planner = planner_session(&h, &cast).await;
    submit(&h, &cast, &planner.id, "ready").await;

    assert!(work_is_active(&h.store, &planner).await);

    // The approval is what ends it: from there the plan is nobody's to rework.
    finalize(&h, &cast, "approved").await;
    assert!(!work_is_active(&h.store, &planner).await);
}

/// What a reconciliation pass makes of a goal waiting for approval: nothing.
///
/// An idle planner under an *active* goal is let go, since the plan it was
/// started for is being worked on; one under a goal whose plan is still the
/// user's to approve is not, since the user may hand it straight back. The two
/// goals are told to the scheduler in that order, and it works its events
/// through one at a time, so the pane killed on the second is what says the
/// first was reconciled too.
#[tokio::test]
async fn a_scheduler_pass_leaves_a_waiting_planner_alone() {
    let h = harness().scheduler().await;
    let waiting = h.cast().await;
    // A second goal for the same planner, with no task of its own: what the
    // active arm does before it looks at any is let an idle planner go.
    let active = h.goal_on(&waiting.planner, &waiting.repo, 1).await;
    let waiting_planner = planner_session(&h, &waiting).await;
    let active_planner = h
        .session(&active, None, Role::Planner, &waiting.planner.id)
        .await;
    for session in [&waiting_planner, &active_planner] {
        h.pane_exists(session);
        h.set_status(session, SessionStatus::Idle).await;
    }
    submit(&h, &waiting, &waiting_planner.id, "ready").await;
    h.activate(&active).await;

    h.notify_goal(&waiting.goal.id);
    h.notify_goal(&active.id);

    eventually(
        TIMEOUT,
        "the active goal's idle planner to be let go",
        async || h.killed_panes().contains(&active_planner.tmux_session),
    )
    .await;
    assert!(
        h.pane_is_alive(&waiting_planner),
        "the plan is still the user's to send back, so its planner stays"
    );
    assert_eq!(
        h.keystrokes(&waiting_planner),
        0,
        "and nothing is asked of it: no resume nudge goes into a pane whose \
         agent is waiting on the user"
    );
    assert_eq!(
        h.status(&waiting.task.id).await,
        TaskStatus::Pending,
        "nor does any task of the waiting plan start"
    );
}
