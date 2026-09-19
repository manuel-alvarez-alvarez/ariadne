//! Integration tests for memory: the repository scope and the global one.

use crate::common;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};

use ariadne_api::SESSION_HEADER;
use ariadne_api::stream::DomainEvent;
use ariadne_core::Seat;

use common::{delete, get, harness, next_event, post_json, test_pin};

fn get_as_session(uri: &str, session_id: &str) -> Request<Body> {
    Request::builder()
        .uri(uri)
        .header(SESSION_HEADER, session_id)
        .body(Body::empty())
        .unwrap()
}

fn delete_as_session(uri: &str, session_id: &str) -> Request<Body> {
    Request::builder()
        .method(Method::DELETE)
        .uri(uri)
        .header(SESSION_HEADER, session_id)
        .body(Body::empty())
        .unwrap()
}

/// A memory saved the way an agent saves one: for a repository of its work.
fn save_as_session(session_id: &str, body: serde_json::Value) -> Request<Body> {
    common::as_session("/v1/memories", session_id, body)
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
            save_as_session(
                &saving.id,
                serde_json::json!({
                    "text": "Run the parser fixture before changing token rules.",
                    "repository_id": repo.id,
                    "expires_at": "2099-01-01T00:00:00Z"
                }),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert_eq!(created["repository_id"], repo.id);
    assert_eq!(created["source_session_id"], saving.id);
    assert_eq!(created["source_task_id"], first.id);
    assert_eq!(created["source_goal_id"], goal.id);

    h.store.delete_goal(&goal.id).await.unwrap();

    let found: Vec<serde_json::Value> = h
        .json(
            get_as_session("/v1/memories/search?q=parser", &searching.id),
            StatusCode::OK,
        )
        .await;
    assert_eq!(found.len(), 1);
    assert_eq!(found[0]["id"], created["id"]);
}

#[tokio::test]
async fn a_user_saves_a_global_memory_that_any_repository_reads() {
    let h = harness().await;
    let (goal, repo) = h.goal().await;
    let task = h.task_on(&goal, &repo, "task", 0, test_pin()).await;
    let author = h.store.task_author(&task.id).await.unwrap();
    let session = h
        .session(&goal, Some(&task), Seat::Author, &author.id)
        .await;

    let created: serde_json::Value = h
        .json(
            post_json(
                "/v1/memories",
                serde_json::json!({"text": "Write every commit subject in the imperative."}),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert!(created["repository_id"].is_null(), "{created}");
    assert!(created["source_session_id"].is_null(), "{created}");
    assert!(created["source_goal_id"].is_null(), "{created}");

    let found: Vec<serde_json::Value> = h
        .json(
            get_as_session("/v1/memories/search?q=imperative", &session.id),
            StatusCode::OK,
        )
        .await;
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0]["id"], created["id"]);

    let listed: Vec<serde_json::Value> = h
        .json(get_as_session("/v1/memories", &session.id), StatusCode::OK)
        .await;
    assert_eq!(listed.len(), 1, "{listed:?}");
}

#[tokio::test]
async fn an_agent_session_cannot_save_a_global_memory() {
    let h = harness().await;
    let (goal, repo) = h.goal().await;
    let task = h.task_on(&goal, &repo, "task", 0, test_pin()).await;
    let author = h.store.task_author(&task.id).await.unwrap();
    let session = h
        .session(&goal, Some(&task), Seat::Author, &author.id)
        .await;

    h.error(
        save_as_session(
            &session.id,
            serde_json::json!({"text": "No session writes this."}),
        ),
        StatusCode::FORBIDDEN,
    )
    .await;

    let listed: Vec<serde_json::Value> = h.json(get("/v1/memories"), StatusCode::OK).await;
    assert!(listed.is_empty(), "{listed:?}");
}

#[tokio::test]
async fn an_agent_session_never_reads_another_repositorys_memory() {
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
        save_as_session(
            &saving.id,
            serde_json::json!({
                "text": "Use the hidden blue fixture.",
                "repository_id": first_repo.id,
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
            get_as_session("/v1/memories/search?q=blue", &searching.id),
            StatusCode::OK,
        )
        .await;
    assert!(found.is_empty(), "{found:?}");

    let listed: Vec<serde_json::Value> = h
        .json(
            get_as_session("/v1/memories", &searching.id),
            StatusCode::OK,
        )
        .await;
    assert!(listed.is_empty(), "{listed:?}");

    h.error(
        get_as_session(
            &format!("/v1/memories/search?q=blue&repository={}", first_repo.id),
            &searching.id,
        ),
        StatusCode::FORBIDDEN,
    )
    .await;
}

#[tokio::test]
async fn a_memory_without_an_expiry_stays_in_the_list_and_the_search() {
    let h = harness().await;
    let (goal, repo) = h.goal().await;
    let task = h.task_on(&goal, &repo, "task", 0, test_pin()).await;
    let author = h.store.task_author(&task.id).await.unwrap();
    let session = h
        .session(&goal, Some(&task), Seat::Author, &author.id)
        .await;

    let created: serde_json::Value = h
        .json(
            save_as_session(
                &session.id,
                serde_json::json!({
                    "text": "The parser fixture holds for every branch.",
                    "repository_id": repo.id
                }),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert!(created["expires_at"].is_null(), "{created}");

    let listed: Vec<serde_json::Value> = h
        .json(get_as_session("/v1/memories", &session.id), StatusCode::OK)
        .await;
    assert_eq!(listed.len(), 1, "{listed:?}");

    let found: Vec<serde_json::Value> = h
        .json(
            get_as_session("/v1/memories/search?q=fixture", &session.id),
            StatusCode::OK,
        )
        .await;
    assert_eq!(found.len(), 1, "{found:?}");
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
        save_as_session(
            &session.id,
            serde_json::json!({
                "text": "This stale trap must stay hidden.",
                "repository_id": repo.id,
                "expires_at": "2000-01-01T00:00:00Z"
            }),
        ),
        StatusCode::CREATED,
    )
    .await;

    let listed: Vec<serde_json::Value> = h.json(get("/v1/memories"), StatusCode::OK).await;
    assert!(listed.is_empty(), "{listed:?}");

    let found: Vec<serde_json::Value> = h
        .json(get("/v1/memories/search?q=stale"), StatusCode::OK)
        .await;
    assert!(found.is_empty(), "{found:?}");
}

#[tokio::test]
async fn the_repository_scope_hides_the_global_memories() {
    let h = harness().await;
    let (_goal, repo) = h.goal().await;
    h.json::<serde_json::Value>(
        post_json(
            "/v1/memories",
            serde_json::json!({"text": "A global note.", "repository_id": repo.id}),
        ),
        StatusCode::CREATED,
    )
    .await;
    let global: serde_json::Value = h
        .json(
            post_json(
                "/v1/memories",
                serde_json::json!({"text": "A note for every repository."}),
            ),
            StatusCode::CREATED,
        )
        .await;

    let scoped: Vec<serde_json::Value> = h
        .json(
            get(&format!(
                "/v1/memories?repository={}&scope=repository",
                repo.id
            )),
            StatusCode::OK,
        )
        .await;
    assert_eq!(scoped.len(), 1, "{scoped:?}");
    assert_eq!(scoped[0]["repository_id"], repo.id);

    let both: Vec<serde_json::Value> = h
        .json(
            get(&format!("/v1/memories?repository={}", repo.id)),
            StatusCode::OK,
        )
        .await;
    assert_eq!(both.len(), 2, "{both:?}");

    let only_global: Vec<serde_json::Value> = h
        .json(get("/v1/memories?scope=global"), StatusCode::OK)
        .await;
    assert_eq!(only_global.len(), 1, "{only_global:?}");
    assert_eq!(only_global[0]["id"], global["id"]);

    h.error(
        get("/v1/memories?scope=repository"),
        StatusCode::BAD_REQUEST,
    )
    .await;
}

#[tokio::test]
async fn deleting_a_repository_removes_its_memories_and_keeps_the_global_ones() {
    let h = harness().await;
    let repo = h.repository(&h.at("lone-repo")).await;
    h.json::<serde_json::Value>(
        post_json(
            "/v1/memories",
            serde_json::json!({"text": "A note about this checkout.", "repository_id": repo.id}),
        ),
        StatusCode::CREATED,
    )
    .await;
    let global: serde_json::Value = h
        .json(
            post_json(
                "/v1/memories",
                serde_json::json!({"text": "A note that outlives any checkout."}),
            ),
            StatusCode::CREATED,
        )
        .await;

    h.store.delete_repository(&repo.id).await.unwrap();

    let listed: Vec<serde_json::Value> = h.json(get("/v1/memories"), StatusCode::OK).await;
    assert_eq!(listed.len(), 1, "{listed:?}");
    assert_eq!(listed[0]["id"], global["id"]);
}

#[tokio::test]
async fn delete_removes_a_memory() {
    let h = harness().await;
    let (goal, repo) = h.goal().await;
    let task = h.task_on(&goal, &repo, "task", 0, test_pin()).await;
    let author = h.store.task_author(&task.id).await.unwrap();
    let session = h
        .session(&goal, Some(&task), Seat::Author, &author.id)
        .await;
    let created: serde_json::Value = h
        .json(
            save_as_session(
                &session.id,
                serde_json::json!({
                    "text": "Delete this entry.",
                    "repository_id": repo.id,
                    "expires_at": "2099-01-01T00:00:00Z"
                }),
            ),
            StatusCode::CREATED,
        )
        .await;
    let id = created["id"].as_str().unwrap().to_string();

    let (status, _) = h
        .send(delete_as_session(
            &format!("/v1/memories/{id}"),
            &session.id,
        ))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let listed: Vec<serde_json::Value> = h.json(get("/v1/memories"), StatusCode::OK).await;
    assert!(listed.is_empty(), "{listed:?}");
}

#[tokio::test]
async fn a_session_deletes_no_global_memory() {
    let h = harness().await;
    let (goal, repo) = h.goal().await;
    let task = h.task_on(&goal, &repo, "task", 0, test_pin()).await;
    let author = h.store.task_author(&task.id).await.unwrap();
    let session = h
        .session(&goal, Some(&task), Seat::Author, &author.id)
        .await;
    let created: serde_json::Value = h
        .json(
            post_json(
                "/v1/memories",
                serde_json::json!({"text": "Only the user removes this."}),
            ),
            StatusCode::CREATED,
        )
        .await;
    let id = created["id"].as_str().unwrap().to_string();

    h.error(
        delete_as_session(&format!("/v1/memories/{id}"), &session.id),
        StatusCode::FORBIDDEN,
    )
    .await;

    let (status, _) = h.send(delete(&format!("/v1/memories/{id}"))).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn the_creation_and_the_deletion_events_carry_the_scope() {
    let h = harness().await;
    let mut events = h.bus.subscribe();
    let (_goal, repo) = h.goal().await;

    let of_repository: serde_json::Value = h
        .json(
            post_json(
                "/v1/memories",
                serde_json::json!({"text": "A fact of this repository.", "repository_id": repo.id}),
            ),
            StatusCode::CREATED,
        )
        .await;
    let event = next_event(&mut events, |event| event.event.kind() == "memory_created").await;
    let DomainEvent::MemoryCreated(memory) = event.event else {
        unreachable!("matched memory_created")
    };
    assert_eq!(memory.id, of_repository["id"]);
    assert_eq!(memory.repository_id.as_deref(), Some(repo.id.as_str()));
    assert_eq!(memory.text, "A fact of this repository.");

    let global: serde_json::Value = h
        .json(
            post_json(
                "/v1/memories",
                serde_json::json!({"text": "A fact of every repository."}),
            ),
            StatusCode::CREATED,
        )
        .await;
    let event = next_event(&mut events, |event| event.event.kind() == "memory_created").await;
    let DomainEvent::MemoryCreated(memory) = event.event else {
        unreachable!("matched memory_created")
    };
    assert_eq!(memory.id, global["id"]);
    assert_eq!(memory.repository_id, None);

    for (created, repository_id) in [(&of_repository, Some(repo.id.clone())), (&global, None)] {
        let id = created["id"].as_str().unwrap();
        let (status, _) = h.send(delete(&format!("/v1/memories/{id}"))).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let event = next_event(&mut events, |event| event.event.kind() == "memory_deleted").await;
        let DomainEvent::MemoryDeleted(deleted) = event.event else {
            unreachable!("matched memory_deleted")
        };
        assert_eq!(deleted.id, id);
        assert_eq!(deleted.repository_id, repository_id);
    }
}

#[tokio::test]
async fn every_memory_endpoint_is_in_the_openapi_document() {
    let h = harness().await;
    let document: serde_json::Value = h.json(get("/api-docs/openapi.json"), StatusCode::OK).await;
    for (path, method) in [
        ("/v1/memories", "post"),
        ("/v1/memories", "get"),
        ("/v1/memories/search", "get"),
        ("/v1/memories/{id}", "delete"),
    ] {
        assert!(
            document["paths"][path].get(method).is_some(),
            "no {method} {path}"
        );
    }
    for gone in [
        "/v1/repositories/{repository_id}/memories",
        "/v1/repositories/{repository_id}/memories/search",
        "/v1/repositories/{repository_id}/memories/{id}",
    ] {
        assert!(document["paths"].get(gone).is_none(), "still {gone}");
    }
}
