//! The daemon that receives a linked goal and runs its tasks.

use std::path::Path;

use axum::http::StatusCode;
use serde_json::{Value, json};

use ariadne_api::error::ErrorBody;
use ariadne_api::sessions::SessionPageDto;
use ariadne_api::tasks::TaskDto;
use ariadne_core::{GoalStatus, TaskStatus};
use ariadne_daemon::timeouts::Timeouts;

use crate::common::{
    Harness, TIMEOUT, eventually_some, get, harness, patch_json, post_json, sh, test_pin,
    with_forge_enabled,
};

const ORIGIN: &str = "01arz3ndektsv4rrffq69g5fav";

async fn create_goal(h: &Harness, repo_id: &str, linked: bool) -> Value {
    let mut body = json!({
        "title": "Ship it",
        "repository_ids": [repo_id],
        "model": test_pin().model,
    });
    if linked {
        body["linked"] = json!({"origin_goal_id": ORIGIN});
    }
    h.json(post_json("/v1/goals", body), StatusCode::CREATED)
        .await
}

fn goal_id(goal: &Value) -> &str {
    goal["id"].as_str().unwrap()
}

fn task_body(goal: &Value) -> Value {
    let agents: Vec<_> = goal["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|step| json!({"step": step["id"], "model": test_pin().model}))
        .collect();
    json!({"title": "Implement it", "agents": agents})
}

async fn post_task(h: &Harness, goal: &Value) -> TaskDto {
    h.json(
        post_json(
            &format!("/v1/goals/{}/tasks", goal_id(goal)),
            task_body(goal),
        ),
        StatusCode::CREATED,
    )
    .await
}

async fn running_task(h: &Harness, task: &TaskDto) -> TaskDto {
    eventually_some(
        TIMEOUT,
        "the first column's session and worktree",
        async || {
            let task: TaskDto = h.get(&format!("/v1/tasks/{}", task.id)).await;
            (task.status == TaskStatus::InProgress
                && task.worktree_path.is_some()
                && task.agents.iter().any(|a| a.session_id.is_some()))
            .then_some(task)
        },
    )
    .await
}

#[tokio::test]
async fn a_linked_goal_keeps_its_origin_id_in_the_goal_api() {
    let h = harness().await;
    let repo = h.repository(&h.at("repo")).await;
    let goal = create_goal(&h, &repo.id, true).await;
    assert_eq!(goal["linked"], json!({"origin_goal_id": ORIGIN}));
    let read: Value = h.get(&format!("/v1/goals/{}", goal_id(&goal))).await;
    assert_eq!(read["linked"], goal["linked"]);

    let ordinary = create_goal(&h, &repo.id, false).await;
    assert_eq!(ordinary.get("linked"), Some(&Value::Null));
}

#[tokio::test]
async fn a_linked_goal_starts_active_without_an_orchestrator() {
    let h = harness().scheduler().await;
    let repo = h.repository(&h.git_repo("repo")).await;
    let goal = create_goal(&h, &repo.id, true).await;
    assert_eq!(goal["status"], GoalStatus::Active.as_str());
    assert_eq!(goal["orchestrated"], false);

    h.notify_goal(goal_id(&goal));
    h.flush_scheduler().await;
    let page: SessionPageDto = h
        .get(&format!("/v1/sessions?goal={}&all=true", goal_id(&goal)))
        .await;
    assert_eq!(page.total, 0);
}

#[tokio::test]
async fn a_task_post_without_a_session_starts_on_a_linked_goal() {
    let h = harness()
        .scheduler()
        .timeouts(Timeouts {
            full_reconcile: TIMEOUT * 2,
            ..Timeouts::default()
        })
        .await;
    let repo = h.repository(&h.git_repo("repo")).await;
    with_forge_enabled(&h, &repo, false).await;
    let goal = create_goal(&h, &repo.id, true).await;
    h.flush_scheduler().await;
    let task = post_task(&h, &goal).await;
    h.flush_scheduler().await;

    let started: TaskDto = h.get(&format!("/v1/tasks/{}", task.id)).await;
    assert_eq!(started.status, TaskStatus::InProgress);
    assert_eq!(started.step.as_deref(), Some("develop"));
    assert!(started.agents.iter().any(|a| a.session_id.is_some()));
}

#[tokio::test]
async fn a_linked_goal_refuses_task_dependencies() {
    let h = harness().await;
    let repo = h.repository(&h.at("repo")).await;
    let goal = create_goal(&h, &repo.id, true).await;
    let first = post_task(&h, &goal).await;
    let mut body = task_body(&goal);
    body["depends_on"] = json!([first.id]);
    let err: ErrorBody = h
        .json(
            post_json(&format!("/v1/goals/{}/tasks", goal_id(&goal)), body),
            StatusCode::BAD_REQUEST,
        )
        .await;
    assert!(err.error.message.contains("linked goal"));

    let tasks: Vec<TaskDto> = h.get(&format!("/v1/tasks?goal={}", goal_id(&goal))).await;
    assert_eq!(tasks.len(), 1);
}

#[tokio::test]
async fn a_linked_task_refuses_dependencies_on_edit() {
    let h = harness().await;
    let repo = h.repository(&h.at("repo")).await;
    let goal = create_goal(&h, &repo.id, true).await;
    let first = post_task(&h, &goal).await;
    let task = post_task(&h, &goal).await;
    let uri = format!("/v1/tasks/{}", task.id);
    let err: ErrorBody = h
        .json(
            patch_json(&uri, json!({"title": "Renamed", "depends_on": [first.id]})),
            StatusCode::BAD_REQUEST,
        )
        .await;
    assert!(err.error.message.contains("linked goal"));
    let read: TaskDto = h.get(&uri).await;
    assert_eq!(read.title, task.title);
    assert!(read.depends_on.is_empty());
}

#[tokio::test]
async fn a_linked_task_starts_at_the_fetched_remote_base() {
    let h = harness().scheduler().await;
    let checkout = h.git_repo("repo");
    let repo = h.repository(&checkout).await;
    with_forge_enabled(&h, &repo, false).await;
    // The other host moves the shared base without updating any local base ref.
    let other_host = h.git_repo("other");
    let commit = sh(&other_host, "git rev-parse next");
    sh(
        &other_host,
        &format!("git push '{}' next:main", h.at("remote.git").display()),
    );

    let goal = create_goal(&h, &repo.id, true).await;
    let task = running_task(&h, &post_task(&h, &goal).await).await;
    let worktree = Path::new(task.worktree_path.as_deref().unwrap());
    assert_eq!(sh(worktree, "git rev-parse HEAD"), commit);
}

#[tokio::test]
async fn a_linked_task_compares_its_work_against_the_fetched_remote_base() {
    let h = harness().scheduler().await;
    let checkout = h.git_repo("repo");
    let repo = h.repository(&checkout).await;
    with_forge_enabled(&h, &repo, false).await;
    let other_host = h.git_repo("other");
    sh(
        &other_host,
        &format!("git push '{}' next:main", h.at("remote.git").display()),
    );
    let goal = create_goal(&h, &repo.id, true).await;
    let task = running_task(&h, &post_task(&h, &goal).await).await;
    let worktree = Path::new(task.worktree_path.as_deref().unwrap());

    // No task commit exists: the upstream commit is neither a task commit nor in the diff.
    let (status, body) = h.send(get(&format!("/v1/tasks/{}/diff", task.id))).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.is_empty(), "{}", String::from_utf8_lossy(&body));
    sh(
        worktree,
        "echo mine > mine.txt && git add . && git -c user.email=t@t -c user.name=t commit -qm mine",
    );
    let (_, body) = h.send(get(&format!("/v1/tasks/{}/diff", task.id))).await;
    let diff = String::from_utf8(body).unwrap();
    assert!(diff.contains("+++ b/mine.txt"), "{diff}");
    assert_eq!(diff.matches("+++ b/").count(), 1, "{diff}");
}

#[tokio::test]
async fn the_openapi_schema_describes_linked_goals() {
    let h = harness().await;
    let doc: Value = h.get("/api-docs/openapi.json").await;
    let schemas = &doc["components"]["schemas"];
    for name in ["CreateGoalRequest", "GoalDto"] {
        let linked = &schemas[name]["properties"]["linked"];
        assert!(linked.is_object(), "{name} needs a linked property");
        assert!(
            linked
                .to_string()
                .contains("#/components/schemas/LinkedGoalDto")
        );
    }
    assert_eq!(
        schemas["LinkedGoalDto"]["properties"]["origin_goal_id"]["type"],
        "string"
    );
}
