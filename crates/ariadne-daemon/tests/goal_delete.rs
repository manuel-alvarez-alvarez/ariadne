//! Deleting a goal: `DELETE /v1/goals/{id}`.
//!
//! The contract is that only a finished goal can go — an active one still owns
//! tmux sessions and worktrees that cancelling is what tears down — that what
//! goes takes its tasks and messages with it, and that the deletion reaches the
//! domain-event stream so clients stop showing what no longer exists.

mod common;

use axum::http::StatusCode;

use ariadne_api::goals::GoalDto;
use ariadne_api::messages::MessageDto;
use ariadne_api::repositories::RepositoryDto;
use ariadne_api::stream::DomainEvent;
use ariadne_api::tasks::TaskDto;
use ariadne_core::Role;
use ariadne_store::AgentSession;

use common::{Harness, delete, get, harness, next_event, post, post_json};

/// A goal in `planning` on a freshly registered repository. Nothing here
/// spawns an agent; the repository exists because a goal needs one.
async fn goal(h: &Harness, name: &str) -> GoalDto {
    let repo = h.git_repo(name);
    let registered: RepositoryDto = h
        .json(
            post_json(
                "/v1/repositories",
                serde_json::json!({"path": repo.display().to_string(),
                                   "base_branch": "main"}),
            ),
            StatusCode::CREATED,
        )
        .await;
    h.json(
        post_json(
            "/v1/goals",
            serde_json::json!({"title": "Ship it", "repository_ids": [registered.id],
                               "planner_profile": "Planner"}),
        ),
        StatusCode::CREATED,
    )
    .await
}

async fn task_in(h: &Harness, goal: &GoalDto) -> TaskDto {
    h.json(
        post_json(
            &format!("/v1/goals/{}/tasks", goal.id),
            serde_json::json!({"title": "Do the thing", "engineer_profile": "Engineer",
                               "reviewers": [{"profile": "Reviewer"}]}),
        ),
        StatusCode::CREATED,
    )
    .await
}

async fn cancel(h: &Harness, goal: &GoalDto) -> GoalDto {
    h.json(
        post(&format!("/v1/goals/{}/cancel", goal.id)),
        StatusCode::OK,
    )
    .await
}

/// A live session on a goal, with a pane the stub tmux answers for.
async fn live_session(h: &Harness, goal: &GoalDto) -> AgentSession {
    let planner = h.profile("leftover planner", Role::Planner).await;
    let goal = h.store.get_goal(&goal.id).await.unwrap();
    let session = h
        .session_named(
            &goal,
            None,
            Role::Planner,
            &planner.id,
            &ariadne_daemon::tmux::session_name(&goal.id, None, "pla", None),
        )
        .await;
    h.pane_exists(&session);
    session
}

/// A cancelled goal deletes, tasks and messages with it, and the stream says so.
#[tokio::test]
async fn deleting_a_finished_goal_takes_its_children_and_reaches_the_stream() {
    let h = harness().await;
    let goal = goal(&h, "repo").await;
    let task = task_in(&h, &goal).await;
    let _: MessageDto = h
        .json(
            post_json(
                &format!("/v1/goals/{}/messages", goal.id),
                serde_json::json!({"body": "how is it going?"}),
            ),
            StatusCode::CREATED,
        )
        .await;
    let goal = cancel(&h, &goal).await;

    let mut rx = h.bus.subscribe();
    let (status, body) = h.send(delete(&format!("/v1/goals/{}", goal.id))).await;
    assert_eq!(
        status,
        StatusCode::NO_CONTENT,
        "{}",
        String::from_utf8_lossy(&body)
    );

    // Nothing is left to refetch, so the event carries the id alone — scoped
    // to the goal, for a stream that is following just this one.
    let event = next_event(&mut rx, |e| e.event.kind() == "goal_deleted").await;
    assert_eq!(event.goal_id.as_deref(), Some(goal.id.as_str()));
    assert_eq!(event.task_id, None);
    let DomainEvent::GoalDeleted(gone) = event.event else {
        unreachable!("matched on kind above");
    };
    assert_eq!(gone.id, goal.id);

    h.error(
        get(&format!("/v1/goals/{}", goal.id)),
        StatusCode::NOT_FOUND,
    )
    .await;
    let goals: Vec<GoalDto> = h.get("/v1/goals").await;
    assert!(goals.is_empty(), "the goal is gone from the list too");

    // ON DELETE CASCADE: the task went with it, and so did the thread.
    h.error(
        get(&format!("/v1/tasks/{}", task.id)),
        StatusCode::NOT_FOUND,
    )
    .await;
    let tasks: Vec<TaskDto> = h.get("/v1/tasks").await;
    assert!(tasks.is_empty(), "no task outlives its goal");
    h.error(
        get(&format!("/v1/goals/{}/messages", goal.id)),
        StatusCode::NOT_FOUND,
    )
    .await;

    // The repository the goal referenced is untouched, and free again.
    let repos: Vec<RepositoryDto> = h.get("/v1/repositories").await;
    assert_eq!(repos.len(), 1);
    let (status, _) = h
        .send(delete(&format!("/v1/repositories/{}", repos[0].id)))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "nothing holds it any more");
}

/// A goal that has not finished owns sessions and worktrees only cancelling
/// tears down, so deleting it is refused — and nothing is dropped on the way.
#[tokio::test]
async fn an_unfinished_goal_is_refused_and_keeps_everything() {
    let h = harness().await;
    let goal = goal(&h, "repo").await;
    let task = task_in(&h, &goal).await;

    let err = h
        .error(
            delete(&format!("/v1/goals/{}", goal.id)),
            StatusCode::CONFLICT,
        )
        .await;
    assert_eq!(err.error.code, "conflict");
    assert!(
        err.error.message.contains("planning") && err.error.message.contains("cancel"),
        "the refusal says what the goal is and what to do about it: {}",
        err.error.message
    );

    let still_there: GoalDto = h.get(&format!("/v1/goals/{}", goal.id)).await;
    assert_eq!(still_there.id, goal.id);
    let _: TaskDto = h.get(&format!("/v1/tasks/{}", task.id)).await;

    // Cancelling is the way through, and then it goes.
    cancel(&h, &goal).await;
    let (status, _) = h.send(delete(&format!("/v1/goals/{}", goal.id))).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

/// An id nobody has is a 404, as it is on every other goal endpoint.
#[tokio::test]
async fn deleting_an_unknown_goal_is_a_404() {
    let h = harness().await;
    let err = h
        .error(delete("/v1/goals/01nosuchgoal"), StatusCode::NOT_FOUND)
        .await;
    assert!(
        err.error.message.contains("01nosuchgoal"),
        "the refusal names the id: {}",
        err.error.message
    );
}

/// The delete is what makes an orphan permanent: the rows cascade away, and a
/// pane that outlived them is no longer anything the daemon can name, let
/// alone reap. A finished goal is not supposed to own one — but if it does,
/// the pane goes before the rows do.
#[tokio::test]
async fn deleting_a_goal_takes_down_a_session_that_outlived_it() {
    let h = harness().await;
    let goal = goal(&h, "repo").await;
    let goal = cancel(&h, &goal).await;
    let session = live_session(&h, &goal).await;

    let (status, body) = h.send(delete(&format!("/v1/goals/{}", goal.id))).await;
    assert_eq!(
        status,
        StatusCode::NO_CONTENT,
        "{}",
        String::from_utf8_lossy(&body)
    );
    assert_eq!(
        h.killed_panes(),
        vec![session.tmux_session],
        "the pane was killed before its row was deleted"
    );
    h.error(
        get(&format!("/v1/sessions/{}", session.id)),
        StatusCode::NOT_FOUND,
    )
    .await;
}
