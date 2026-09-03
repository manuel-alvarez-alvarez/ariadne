//! Goals work in registered repositories, by reference.
//!
//! What a goal is created with is a repository id; what its tasks branch from
//! is whatever that repository says at the time. So the checks here run the
//! whole way through: register a repository over HTTP, create a goal on it,
//! create a task, and see the worktree the launcher makes from the path and
//! base branch the repository holds — including after that base branch moves.
//!
//! No tmux and no agent CLI: `tmux` is a stub that answers "no session" and
//! records what it was told, and the profiles are pinned to a model so that
//! nothing here looks for a coding-agent CLI on `PATH`. `git` is real.

mod common;

use std::path::{Path, PathBuf};

use axum::http::StatusCode;

use ariadne_api::goals::GoalDto;
use ariadne_api::profiles::ProfileDto;
use ariadne_api::repositories::RepositoryDto;
use ariadne_api::tasks::TaskDto;
use ariadne_core::AgentKind;
use ariadne_store::defaults::BUILTIN_PROFILES;

use common::{Harness, delete, harness, post_json, put_json, sh};

/// A daemon whose seeded profiles are pinned to a model.
///
/// They are seeded on "auto", which at spawn time means "the first
/// coding-agent CLI on `PATH`" — and where there is none, as on every CI
/// runner, spawning fails outright. What is under test here is the worktree a
/// spawn cuts, not the agent it starts, so the model is pinned and never
/// looked up.
async fn pinned_harness() -> Harness {
    let h = harness().await;
    for builtin in BUILTIN_PROFILES {
        let _: ProfileDto = h
            .json(
                put_json(
                    &format!("/v1/profiles/{}", builtin.id),
                    serde_json::json!({"model": AgentKind::ClaudeCode.as_str()}),
                ),
                StatusCode::OK,
            )
            .await;
    }
    h
}

async fn register(h: &Harness, path: &Path, base_branch: &str) -> RepositoryDto {
    h.json(
        post_json(
            "/v1/repositories",
            serde_json::json!({"path": path.display().to_string(),
                               "base_branch": base_branch}),
        ),
        StatusCode::CREATED,
    )
    .await
}

async fn goal_on(h: &Harness, repository_ids: Vec<&str>) -> GoalDto {
    h.json(
        post_json(
            "/v1/goals",
            serde_json::json!({"title": "Ship it", "repository_ids": repository_ids,
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

/// Register a repository, create a goal on it, create a task: the engineer's
/// worktree is cut from that repository's checkout and base branch.
#[tokio::test]
async fn a_task_branches_from_the_repository_its_goal_references() {
    let h = pinned_harness().await;
    let repo = h.git_repo("repo");
    let registered = register(&h, &repo, "next").await;
    let goal = goal_on(&h, vec![&registered.id]).await;
    assert_eq!(goal.repos.len(), 1);
    assert_eq!(goal.repos[0].id, registered.id);

    let task = task_in(&h, &goal).await;
    assert_eq!(task.repo_id, registered.id, "the goal has one repository");

    let session = h.launcher.spawn_engineer(&task.id).await.unwrap();
    let worktree = PathBuf::from(session.worktree_path.unwrap());
    assert!(worktree.is_dir(), "the worktree was created");
    assert_eq!(sh(&worktree, "git rev-parse --abbrev-ref HEAD"), task.branch);
    assert_eq!(
        sh(&worktree, "git rev-parse HEAD"),
        sh(&repo, "git rev-parse next"),
        "the branch starts at the repository's base branch"
    );
}

/// The goal reads its repository live: moving the base branch moves what the
/// next task branches from, with nothing to update on the goal.
#[tokio::test]
async fn editing_the_base_branch_moves_what_new_tasks_branch_from() {
    let h = pinned_harness().await;
    let repo = h.git_repo("repo");
    let registered = register(&h, &repo, "next").await;
    let goal = goal_on(&h, vec![&registered.id]).await;

    let edited: RepositoryDto = h
        .json(
            put_json(
                &format!("/v1/repositories/{}", registered.id),
                serde_json::json!({"base_branch": "main"}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(edited.base_branch, "main");
    let goal: GoalDto = h.get(&format!("/v1/goals/{}", goal.id)).await;
    assert_eq!(goal.repos[0].base_branch, "main", "the goal moved with it");

    let task = task_in(&h, &goal).await;
    let session = h.launcher.spawn_engineer(&task.id).await.unwrap();
    let worktree = PathBuf::from(session.worktree_path.unwrap());
    assert_eq!(
        sh(&worktree, "git rev-parse HEAD"),
        sh(&repo, "git rev-parse main"),
        "the new base branch is what the worktree starts from"
    );
}

/// Deleting a repository a goal works in would leave the goal pointing at
/// nothing, so it is refused — and the refusal says who is holding it.
#[tokio::test]
async fn a_repository_in_use_cannot_be_deleted() {
    let h = pinned_harness().await;
    let registered = register(&h, &h.git_repo("repo"), "main").await;
    let goal = goal_on(&h, vec![&registered.id]).await;
    task_in(&h, &goal).await;

    let err = h
        .error(
            delete(&format!("/v1/repositories/{}", registered.id)),
            StatusCode::CONFLICT,
        )
        .await;
    assert!(
        err.error.message.contains("1 goal"),
        "{}",
        err.error.message
    );
    assert!(
        err.error.message.contains("1 task"),
        "{}",
        err.error.message
    );

    // An unheld one still goes.
    let spare = register(&h, &h.git_repo("spare"), "main").await;
    let (status, _) = h
        .send(delete(&format!("/v1/repositories/{}", spare.id)))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

/// A goal is created on repositories that are registered, and nothing else:
/// an id nobody registered is a 404, not a repository invented on the spot.
#[tokio::test]
async fn a_goal_cannot_be_created_on_an_unknown_repository() {
    let h = pinned_harness().await;
    let registered = register(&h, &h.git_repo("repo"), "main").await;

    let err = h
        .error(
            post_json(
                "/v1/goals",
                serde_json::json!({"title": "Ship it", "planner_profile": "Planner",
                                   "repository_ids": [registered.id, "01nosuchrepository"]}),
            ),
            StatusCode::NOT_FOUND,
        )
        .await;
    assert!(
        err.error.message.contains("01nosuchrepository"),
        "the refusal names the id: {}",
        err.error.message
    );

    // And no repository at all is a bad request, as it always was.
    h.error(
        post_json(
            "/v1/goals",
            serde_json::json!({"title": "Ship it", "planner_profile": "Planner",
                               "repository_ids": []}),
        ),
        StatusCode::BAD_REQUEST,
    )
    .await;

    let goals: Vec<GoalDto> = h.get("/v1/goals").await;
    assert!(goals.is_empty(), "neither attempt left a goal behind");
}
