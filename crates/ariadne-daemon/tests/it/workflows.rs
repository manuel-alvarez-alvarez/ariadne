//! Integration tests for the workflow routes: CRUD, reset, parse, and the
//! fat events each write emits.

use axum::http::StatusCode;

use ariadne_api::stream::DomainEvent;
use ariadne_api::workflows::{ParsedWorkflowDto, WorkflowDto};

use crate::common;
use common::{delete, get, harness, next_event, post_json, put_json};

const DOC: &str = "workflow api-design\n  build[Build]\n    Do the work.\n    skills: coding\n";

#[tokio::test]
async fn every_route_round_trips_and_emits_one_fat_event_per_write() {
    let h = harness().await;
    let mut rx = h.bus.subscribe();

    let (status, body) = h
        .send(post_json(
            "/v1/workflows",
            serde_json::json!({"name": "api-design", "document": DOC}),
        ))
        .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "{}",
        String::from_utf8_lossy(&body)
    );
    let created: WorkflowDto = serde_json::from_slice(&body).unwrap();
    assert_eq!(created.name, "api-design");
    assert!(!created.builtin);
    assert_eq!(created.steps.len(), 1);
    assert_eq!(created.steps[0].id, "build");
    assert_eq!(created.steps[0].skills, vec!["coding".to_string()]);

    let event = next_event(&mut rx, |e| e.event.kind() == "workflow_created").await;
    let DomainEvent::WorkflowCreated(fat) = event.event else {
        unreachable!("matched on kind above");
    };
    // Fat payload: the whole DTO, not just a name to refetch.
    assert_eq!(fat.name, "api-design");
    assert_eq!(fat.steps.len(), 1);

    let listed: Vec<WorkflowDto> = h.get("/v1/workflows").await;
    assert!(listed.iter().any(|w| w.name == "api-design"));

    let fetched: WorkflowDto = h.get("/v1/workflows/api-design").await;
    assert_eq!(fetched.document, DOC);

    let edited =
        "workflow api-design\n  build[Build]\n    Do it differently.\n    skills: coding\n";
    let updated: WorkflowDto = h
        .json(
            put_json(
                "/v1/workflows/api-design",
                serde_json::json!({"document": edited}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(updated.steps[0].description, "Do it differently.");
    let event = next_event(&mut rx, |e| e.event.kind() == "workflow_updated").await;
    assert!(matches!(event.event, DomainEvent::WorkflowUpdated(_)));

    let (status, _) = h.send(delete("/v1/workflows/api-design")).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let event = next_event(&mut rx, |e| e.event.kind() == "workflow_deleted").await;
    assert!(matches!(event.event, DomainEvent::WorkflowDeleted(_)));

    let (status, _) = h.send(get("/v1/workflows/api-design")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_missing_workflow_is_a_404_naming_workflow_not_found() {
    let h = harness().await;
    let err = h
        .error(get("/v1/workflows/nope"), StatusCode::NOT_FOUND)
        .await;
    assert_eq!(err.error.code, "workflow_not_found");
}

#[tokio::test]
async fn a_taken_name_is_a_409_and_a_built_in_refuses_delete_and_a_user_one_refuses_reset() {
    let h = harness().await;

    let (status, _) = h
        .send(post_json(
            "/v1/workflows",
            serde_json::json!({
                "name": "develop-review-merge",
                "document": "workflow develop-review-merge\n  a[A]\n    Do it.\n    skills: coding\n",
            }),
        ))
        .await;
    assert_eq!(status, StatusCode::CONFLICT);

    let (status, _) = h.send(delete("/v1/workflows/develop-review-merge")).await;
    assert_eq!(status, StatusCode::CONFLICT);

    h.send(post_json(
        "/v1/workflows",
        serde_json::json!({"name": "api-design", "document": DOC}),
    ))
    .await;
    let (status, _) = h
        .send(post_json(
            "/v1/workflows/api-design/reset",
            serde_json::json!({}),
        ))
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn resetting_a_built_in_drops_the_override() {
    let h = harness().await;
    h.send(put_json(
        "/v1/workflows/develop-review-merge",
        serde_json::json!({"document": DOC}),
    ))
    .await;

    let (status, body) = h
        .send(post_json(
            "/v1/workflows/develop-review-merge/reset",
            serde_json::json!({}),
        ))
        .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let reset: WorkflowDto = serde_json::from_slice(&body).unwrap();
    assert_eq!(reset.steps[0].id, "develop");
}

#[tokio::test]
async fn the_parse_route_answers_the_steps_of_a_good_document_and_the_line_of_a_bad_one() {
    let h = harness().await;

    let parsed: ParsedWorkflowDto = h
        .json(
            post_json("/v1/workflows/parse", serde_json::json!({"document": DOC})),
            StatusCode::OK,
        )
        .await;
    assert_eq!(parsed.name, "api-design");
    assert_eq!(parsed.steps[0].id, "build");

    let err = h
        .error(
            post_json(
                "/v1/workflows/parse",
                serde_json::json!({"document": "not a workflow\n"}),
            ),
            StatusCode::BAD_REQUEST,
        )
        .await;
    assert_eq!(err.error.code, "workflow_invalid");
    assert_eq!(err.error.details.unwrap()["line"], 1);
}
