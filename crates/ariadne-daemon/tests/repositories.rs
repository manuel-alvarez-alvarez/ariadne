//! Integration tests for the repository endpoints.
//!
//! The contract is that a repository is validated exactly the way a goal's
//! repos are — an absolute path into a real git work tree, on a branch that
//! exists and has a commit — that the same checkout is registered once per
//! base branch, and that every write reaches the domain-event stream.

mod common;

use axum::http::StatusCode;

use ariadne_api::repositories::RepositoryDto;
use ariadne_api::stream::DomainEvent;

use common::{delete, get, harness, next_event, post_json, put_json, sh};

#[tokio::test]
async fn crud_round_trip_emits_a_fat_event_per_write() {
    let h = harness().await;
    let repo = h.git_repo("repo");
    let mut rx = h.bus.subscribe();

    let created: RepositoryDto = h
        .json(
            post_json(
                "/v1/repositories",
                serde_json::json!({"path": repo.display().to_string(), "base_branch": "main",
                                   "description": "the toy repo"}),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert_eq!(created.path, repo.display().to_string());
    assert_eq!(created.base_branch, "main");
    assert_eq!(created.description.as_deref(), Some("the toy repo"));

    // Fat payload: the whole DTO, not just an id to refetch. Repositories are
    // global, so the event belongs to no goal or task.
    let event = next_event(&mut rx, |e| e.event.kind() == "repository_created").await;
    assert_eq!(event.goal_id, None);
    assert_eq!(event.task_id, None);
    let DomainEvent::RepositoryCreated(dto) = event.event else {
        unreachable!("matched on kind above");
    };
    assert_eq!(dto.id, created.id);
    assert_eq!(dto.base_branch, "main");

    // Listed and readable by id.
    let all: Vec<RepositoryDto> = h.json(get("/v1/repositories"), StatusCode::OK).await;
    assert_eq!(all.len(), 1);
    let one: RepositoryDto = h
        .json(
            get(&format!("/v1/repositories/{}", created.id)),
            StatusCode::OK,
        )
        .await;
    assert_eq!(one.id, created.id);

    // Moving the base branch revalidates it; an empty description clears it.
    let edited: RepositoryDto = h
        .json(
            put_json(
                &format!("/v1/repositories/{}", created.id),
                serde_json::json!({"base_branch": "next", "description": ""}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(edited.path, created.path, "the path is left alone");
    assert_eq!(edited.base_branch, "next");
    assert!(edited.description.is_none());
    let event = next_event(&mut rx, |e| e.event.kind() == "repository_updated").await;
    let DomainEvent::RepositoryUpdated(dto) = event.event else {
        unreachable!("matched on kind above");
    };
    assert_eq!(dto.base_branch, "next");

    let (status, _) = h
        .send(delete(&format!("/v1/repositories/{}", created.id)))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let event = next_event(&mut rx, |e| e.event.kind() == "repository_deleted").await;
    let DomainEvent::RepositoryDeleted(gone) = event.event else {
        unreachable!("matched on kind above");
    };
    assert_eq!(gone.id, created.id);

    h.error(
        get(&format!("/v1/repositories/{}", created.id)),
        StatusCode::NOT_FOUND,
    )
    .await;
}

/// Omitting the base branch takes the repo's checked-out one.
#[tokio::test]
async fn base_branch_defaults_to_the_current_branch() {
    let h = harness().await;
    let repo = h.git_repo("repo");
    sh(&repo, "git checkout -q next");

    let created: RepositoryDto = h
        .json(
            post_json(
                "/v1/repositories",
                serde_json::json!({"path": repo.display().to_string()}),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert_eq!(created.base_branch, "next");
    assert!(created.description.is_none());
}

#[tokio::test]
async fn create_refuses_a_path_or_branch_it_cannot_use() {
    let h = harness().await;
    let repo = h.git_repo("repo");

    // Relative path.
    let err = h
        .error(
            post_json(
                "/v1/repositories",
                serde_json::json!({"path": "relative/repo"}),
            ),
            StatusCode::BAD_REQUEST,
        )
        .await;
    assert!(
        err.error.message.contains("must be absolute"),
        "{}",
        err.error.message
    );

    // Absolute, but not a git work tree.
    let not_a_repo = h.dir.path().join("empty");
    std::fs::create_dir_all(&not_a_repo).unwrap();
    h.error(
        post_json(
            "/v1/repositories",
            serde_json::json!({"path": not_a_repo.display().to_string()}),
        ),
        StatusCode::BAD_REQUEST,
    )
    .await;

    // A branch the repo does not have.
    let err = h
        .error(
            post_json(
                "/v1/repositories",
                serde_json::json!({"path": repo.display().to_string(), "base_branch": "nope"}),
            ),
            StatusCode::BAD_REQUEST,
        )
        .await;
    assert!(
        err.error.message.contains("nope"),
        "the refusal names the branch: {}",
        err.error.message
    );
}

/// One registration per checkout and base branch; another branch of the same
/// checkout is a repository of its own.
#[tokio::test]
async fn the_same_path_and_branch_cannot_be_registered_twice() {
    let h = harness().await;
    let repo = h.git_repo("repo");
    let body = serde_json::json!({"path": repo.display().to_string(), "base_branch": "main"});

    let first: RepositoryDto = h
        .json(
            post_json("/v1/repositories", body.clone()),
            StatusCode::CREATED,
        )
        .await;
    h.error(post_json("/v1/repositories", body), StatusCode::CONFLICT)
        .await;

    let other: RepositoryDto = h
        .json(
            post_json(
                "/v1/repositories",
                serde_json::json!({"path": repo.display().to_string(), "base_branch": "next"}),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert_ne!(other.id, first.id);

    // Editing one onto the other's (path, base_branch) conflicts the same way.
    h.error(
        put_json(
            &format!("/v1/repositories/{}", other.id),
            serde_json::json!({"base_branch": "main"}),
        ),
        StatusCode::CONFLICT,
    )
    .await;
}
