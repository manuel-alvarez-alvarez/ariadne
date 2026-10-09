//! Integration tests for the one end of a plan: the orchestrator finalizes it.
//!
//! The plan is the tasks it wrote, and `finalize` is what hands them out: the
//! goal goes from `planning` straight to `active` and every task the plan
//! holds starts. Nobody else may make that call — a user session is refused —
//! and it is made once. Every task runs through every column of the goal's
//! workflow, so a plan with a column nobody staffs is refused by that column's
//! name until the orchestrator staffs it.
//!
//! Mostly no agent is started — the rows are seeded through the store and
//! the endpoints are exercised — except for the tests about what the
//! scheduler makes of a finalized goal, which want a real scheduler and the
//! stub ACP agent.

use crate::common;

use std::time::Duration;

use axum::http::StatusCode;
use serde_json::{Value, json};

use ariadne_api::error::ErrorBody;
use ariadne_api::goals::GoalDto;
use ariadne_api::tasks::TaskDto;
use ariadne_core::{GoalStatus, Seat, SessionStatus, TaskStatus};
use ariadne_daemon::attention::work_is_active;
use ariadne_store::{NewTask, NewTaskAgent};

use common::{
    Cast, Harness, as_session, eventually, harness, patch_json, post_json, put_json, test_pin,
};

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

/// The ids of a goal's columns, as its DTO lists them.
fn step_ids(goal: &Value) -> Vec<&str> {
    goal["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap())
        .collect()
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
    eventually(
        TIMEOUT,
        "the plan's task to reach its first column",
        async || {
            matches!(
                h.status(&cast.task.id).await,
                TaskStatus::Ready | TaskStatus::InProgress
            )
        },
    )
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
        "an agent was started on a task the user has not seen"
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

/// A goal created with no workflow of its own runs its first repository's
/// default, and its columns are that workflow's: the workflow is settled at
/// the goal before planning starts, so the orchestrator never has to ask.
#[tokio::test]
async fn a_goal_created_without_a_workflow_runs_its_first_repositorys_default() {
    let h = harness().await;
    let repo = h.repository(&h.at("repo")).await;
    let updated: Value = h
        .json(
            put_json(
                &format!("/v1/repositories/{}", repo.id),
                json!({"default_workflow": "develop-review-pr"}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(updated["default_workflow"], "develop-review-pr");

    let goal: Value = h
        .json(
            post_json(
                "/v1/goals",
                json!({
                    "title": "Ship the board", "description": "Land it through a request.",
                    "repository_ids": [repo.id], "model": "stub:test-model"
                }),
            ),
            StatusCode::CREATED,
        )
        .await;

    assert_eq!(goal["workflow"], "develop-review-pr");
    assert_eq!(step_ids(&goal), ["develop", "review", "pr"]);
    // Snapshotted at creation: a later read says the same, and so does the
    // row behind it.
    let read: Value = h
        .get(&format!("/v1/goals/{}", goal["id"].as_str().unwrap()))
        .await;
    assert_eq!(step_ids(&read), ["develop", "review", "pr"]);
    assert_eq!(
        h.store
            .get_goal(goal["id"].as_str().unwrap())
            .await
            .unwrap()
            .workflow,
        "develop-review-pr"
    );
}

/// Every task runs through every column, so a task with a column nobody
/// staffs would stop there: the plan is refused with the column's name, and
/// is accepted once the orchestrator has staffed it through `update_task`.
#[tokio::test]
async fn finalize_refuses_a_task_with_an_unstaffed_column_by_name_until_it_is_staffed() {
    let h = harness().await;
    let (goal, repo) = h.goal().await;
    // Staffed on the first two columns and not on the third: what a plan
    // written column by column, or a task moved onto a workflow, looks like.
    let task = h
        .store
        .create_task(NewTask {
            goal_id: goal.id.clone(),
            repo_id: repo.id.clone(),
            title: "Build the board".into(),
            description: "Render it from the store.".into(),
            agents: vec![
                NewTaskAgent::new(String::from("develop"), Vec::<String>::new(), test_pin()),
                NewTaskAgent::new(String::from("review"), Vec::<String>::new(), test_pin()),
            ],
            depends_on: vec![],
        })
        .await
        .unwrap();
    let orchestrator = h.orchestrator_session(&goal).await;
    let finalize_uri = format!("/v1/goals/{}/finalize", goal.id);

    let envelope: ErrorBody = h
        .json(
            as_session(&finalize_uri, &orchestrator.id, json!({})),
            StatusCode::CONFLICT,
        )
        .await;
    assert!(
        envelope
            .error
            .message
            .contains("has no agent on column merge"),
        "the refusal names the column: {}",
        envelope.error.message
    );
    assert!(
        envelope.error.message.contains(&task.title),
        "and the task: {}",
        envelope.error.message
    );
    assert_eq!(
        h.store.get_goal(&goal.id).await.unwrap().status(),
        GoalStatus::Planning,
        "a refused call leaves the goal where it was"
    );
    assert_eq!(h.status(&task.id).await, TaskStatus::Pending);

    // The whole staffing again, the missing column included.
    let staffed: TaskDto = h
        .json(
            patch_json(
                &format!("/v1/tasks/{}", task.id),
                json!({"agents": [
                    {"step": "develop", "model": "stub:test-model"},
                    {"step": "review", "model": "stub:test-model"},
                    {"step": "merge", "model": "stub:test-model"},
                ]}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(
        staffed
            .agents
            .iter()
            .map(|a| a.step.as_str())
            .collect::<Vec<_>>(),
        ["develop", "review", "merge"]
    );

    let finalized: GoalDto = h
        .json(
            as_session(&finalize_uri, &orchestrator.id, json!({})),
            StatusCode::OK,
        )
        .await;
    assert_eq!(finalized.status, GoalStatus::Active);
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
///
/// The plan's task really starts, in a git repository of its own, and the
/// passes below run over a goal whose work is under way: what the daemon says
/// to an orchestrator is what its tasks need, and a task nothing could start
/// needs it very much. Without the repository the first column's agent cannot
/// be launched, so every pass over that task spends an attempt, and after
/// three of them the task fails and the daemon rightly says so — which is a
/// prompt this assertion reads as the defect it is watching for.
#[tokio::test]
async fn a_scheduler_pass_keeps_the_orchestrator_of_an_active_goal_and_types_nothing() {
    let h = harness().scheduler().await;
    h.git_repo("repo");
    let cast = h.cast().await;
    let orchestrator = orchestrator_session(&h, &cast).await;
    h.agent_runs(&orchestrator).await;
    h.set_status(&orchestrator, SessionStatus::Idle).await;
    finalize(&h, &cast, &orchestrator.id).await;
    eventually(TIMEOUT, "the plan's task to be under way", async || {
        h.status(&cast.task.id).await == TaskStatus::InProgress
            && h.running_session(&cast.task.id, Seat::Agent)
                .await
                .is_some()
    })
    .await;

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
