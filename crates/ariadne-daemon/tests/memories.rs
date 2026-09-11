//! Integration tests for repository memory.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};

use ariadne_api::SESSION_HEADER;
use ariadne_api::stream::DomainEvent;
use ariadne_core::Seat;

use common::{as_session, delete, get, harness, next_event, test_pin};

fn get_as_session(uri: &str, session_id: &str) -> Request<Body> {
    Request::builder()
        .uri(uri)
        .header(SESSION_HEADER, session_id)
        .body(Body::empty())
        .unwrap()
}

#[tokio::test]
async fn another_session_of_the_same_repository_finds_an_authors_memory() {
    let h = harness().await;
    let (goal, repo) = h.goal().await;
    let first = h.task_on(&goal, &repo, "first", 0, test_pin()).await;
    let second_goal = h.goal_on(&repo, test_pin()).await;
    let second = h
        .task_on(&second_goal, &repo, "second", 0, test_pin())
        .await;
    let first_author = h.store.task_author(&first.id).await.unwrap();
    let second_author = h.store.task_author(&second.id).await.unwrap();
    let saving = h
        .session(&goal, Some(&first), Seat::Author, &first_author.id)
        .await;
    let searching = h
        .session(&second_goal, Some(&second), Seat::Author, &second_author.id)
        .await;

    let created: serde_json::Value = h
        .json(
            as_session(
                &format!("/v1/repositories/{}/memories", repo.id),
                &saving.id,
                serde_json::json!({
                    "text": "Run the parser fixture before changing token rules.",
                    "expires_at": "2099-01-01T00:00:00Z"
                }),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert_eq!(created["source_session_id"], saving.id);
    assert_eq!(created["source_task_id"], first.id);
    assert_eq!(created["source_goal_id"], goal.id);

    h.store.delete_goal(&goal.id).await.unwrap();

    let found: Vec<serde_json::Value> = h
        .json(
            get_as_session(
                &format!("/v1/repositories/{}/memories/search?q=parser", repo.id),
                &searching.id,
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(found.len(), 1);
    assert_eq!(found[0]["id"], created["id"]);
}

#[tokio::test]
async fn another_repository_does_not_find_the_memory() {
    let h = harness().await;
    let (first_goal, first_repo) = h.goal().await;
    let first_task = h
        .task_on(&first_goal, &first_repo, "first", 0, test_pin())
        .await;
    let first_author = h.store.task_author(&first_task.id).await.unwrap();
    let saving = h
        .session(
            &first_goal,
            Some(&first_task),
            Seat::Author,
            &first_author.id,
        )
        .await;
    h.json::<serde_json::Value>(
        as_session(
            &format!("/v1/repositories/{}/memories", first_repo.id),
            &saving.id,
            serde_json::json!({
                "text": "Use the hidden blue fixture.",
                "expires_at": "2099-01-01T00:00:00Z"
            }),
        ),
        StatusCode::CREATED,
    )
    .await;

    let other_repo = h.repository(&h.at("other-repo")).await;
    let other_goal = h.goal_on(&other_repo, test_pin()).await;
    let other_task = h
        .task_on(&other_goal, &other_repo, "other", 0, test_pin())
        .await;
    let other_author = h.store.task_author(&other_task.id).await.unwrap();
    let searching = h
        .session(
            &other_goal,
            Some(&other_task),
            Seat::Author,
            &other_author.id,
        )
        .await;

    let found: Vec<serde_json::Value> = h
        .json(
            get_as_session(
                &format!("/v1/repositories/{}/memories/search?q=blue", other_repo.id),
                &searching.id,
            ),
            StatusCode::OK,
        )
        .await;
    assert!(found.is_empty(), "{found:?}");

    h.error(
        get_as_session(
            &format!("/v1/repositories/{}/memories/search?q=blue", first_repo.id),
            &searching.id,
        ),
        StatusCode::FORBIDDEN,
    )
    .await;
}

#[tokio::test]
async fn an_expired_memory_never_returns_from_list_or_search() {
    let h = harness().await;
    let (goal, repo) = h.goal().await;
    let task = h.task_on(&goal, &repo, "task", 0, test_pin()).await;
    let author = h.store.task_author(&task.id).await.unwrap();
    let session = h
        .session(&goal, Some(&task), Seat::Author, &author.id)
        .await;

    h.json::<serde_json::Value>(
        as_session(
            &format!("/v1/repositories/{}/memories", repo.id),
            &session.id,
            serde_json::json!({
                "text": "This stale trap must stay hidden.",
                "expires_at": "2000-01-01T00:00:00Z"
            }),
        ),
        StatusCode::CREATED,
    )
    .await;

    let listed: Vec<serde_json::Value> = h
        .json(
            get(&format!("/v1/repositories/{}/memories", repo.id)),
            StatusCode::OK,
        )
        .await;
    assert!(listed.is_empty(), "{listed:?}");

    let found: Vec<serde_json::Value> = h
        .json(
            get(&format!(
                "/v1/repositories/{}/memories/search?q=stale",
                repo.id
            )),
            StatusCode::OK,
        )
        .await;
    assert!(found.is_empty(), "{found:?}");
}

#[tokio::test]
async fn delete_removes_a_memory() {
    let h = harness().await;
    let mut events = h.bus.subscribe();
    let (goal, repo) = h.goal().await;
    let task = h.task_on(&goal, &repo, "task", 0, test_pin()).await;
    let author = h.store.task_author(&task.id).await.unwrap();
    let session = h
        .session(&goal, Some(&task), Seat::Author, &author.id)
        .await;
    let created: serde_json::Value = h
        .json(
            as_session(
                &format!("/v1/repositories/{}/memories", repo.id),
                &session.id,
                serde_json::json!({
                    "text": "Delete this entry.",
                    "expires_at": "2099-01-01T00:00:00Z"
                }),
            ),
            StatusCode::CREATED,
        )
        .await;
    let event = next_event(&mut events, |event| event.event.kind() == "memory_created").await;
    let DomainEvent::MemoryCreated(memory) = event.event else {
        unreachable!("matched memory_created")
    };
    assert_eq!(memory.id, created["id"]);
    assert_eq!(memory.text, "Delete this entry.");

    let (status, _) = h
        .send(delete(&format!(
            "/v1/repositories/{}/memories/{}",
            repo.id,
            created["id"].as_str().unwrap()
        )))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let event = next_event(&mut events, |event| event.event.kind() == "memory_deleted").await;
    let DomainEvent::MemoryDeleted(memory) = event.event else {
        unreachable!("matched memory_deleted")
    };
    assert_eq!(memory.id, created["id"]);

    let listed: Vec<serde_json::Value> = h
        .json(
            get(&format!("/v1/repositories/{}/memories", repo.id)),
            StatusCode::OK,
        )
        .await;
    assert!(listed.is_empty(), "{listed:?}");
}

#[tokio::test]
async fn every_memory_endpoint_is_in_the_openapi_document() {
    let h = harness().await;
    let document: serde_json::Value = h.json(get("/api-docs/openapi.json"), StatusCode::OK).await;
    for path in [
        "/v1/repositories/{repository_id}/memories",
        "/v1/repositories/{repository_id}/memories/search",
        "/v1/repositories/{repository_id}/memories/{id}",
    ] {
        assert!(document["paths"].get(path).is_some(), "no {path}");
    }
}
