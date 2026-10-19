//! Integration tests for dashboard CRUD and validation.

use crate::common;
use ariadne_api::dashboards::DashboardDto;
use ariadne_api::stream::DomainEvent;
use axum::http::StatusCode;
use common::{delete, get, harness, next_event, post_json, put_json};

fn document(name: &str) -> serde_json::Value {
    serde_json::json!({"kind":"Dashboard","metadata":{"name":name},"spec":{}})
}

#[tokio::test]
async fn dashboard_routes_keep_the_document_and_publish_fat_events() {
    let h = harness().await;
    let mut rx = h.bus.subscribe();
    let created: DashboardDto = h
        .json(
            post_json(
                "/v1/dashboards",
                serde_json::json!({"name":"mine","document":document("mine")}),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert_eq!(created.name, "mine");
    assert!(!created.builtin);
    assert_eq!(created.document, document("mine"));
    let event = next_event(&mut rx, |e| e.event.kind() == "dashboard_created").await;
    assert!(matches!(event.event, DomainEvent::DashboardCreated(_)));
    let updated: DashboardDto = h
        .json(
            put_json(
                "/v1/dashboards/mine",
                serde_json::json!({"document":document("mine")}),
            ),
            StatusCode::OK,
        )
        .await;
    assert!(!updated.document_is_default);
    assert_eq!(
        h.send(delete("/v1/dashboards/mine")).await.0,
        StatusCode::NO_CONTENT
    );
    let event = next_event(&mut rx, |e| e.event.kind() == "dashboard_deleted").await;
    let DomainEvent::DashboardDeleted(deleted) = event.event else {
        unreachable!("matched on kind above");
    };
    assert_eq!(deleted.name, "mine");
}

#[tokio::test]
async fn dashboard_documents_must_be_named_perses_dashboards() {
    let h = harness().await;
    let err = h.error(post_json("/v1/dashboards", serde_json::json!({"name":"mine","document":{"kind":"NotDashboard","metadata":{"name":"other"},"spec":[]}})), StatusCode::UNPROCESSABLE_ENTITY).await;
    assert_eq!(err.error.code, "invalid_request");
    assert!(err.error.message.contains("kind"));
    assert_eq!(
        h.send(get("/v1/dashboards/default")).await.0,
        StatusCode::OK
    );
    let err = h
        .error(delete("/v1/dashboards/default"), StatusCode::CONFLICT)
        .await;
    assert_eq!(err.error.code, "dashboard_builtin");
}

#[tokio::test]
async fn resetting_a_built_in_dashboard_restores_its_shipped_document() {
    let h = harness().await;
    let override_document = document("default");
    let updated: DashboardDto = h
        .json(
            put_json(
                "/v1/dashboards/default",
                serde_json::json!({"document": override_document}),
            ),
            StatusCode::OK,
        )
        .await;
    assert!(!updated.document_is_default);

    let reset: DashboardDto = h
        .json(
            post_json("/v1/dashboards/default/reset", serde_json::json!({})),
            StatusCode::OK,
        )
        .await;
    assert!(reset.document_is_default);
    assert_ne!(reset.document, document("default"));

    let _: DashboardDto = h
        .json(
            post_json(
                "/v1/dashboards",
                serde_json::json!({"name":"mine","document":document("mine")}),
            ),
            StatusCode::CREATED,
        )
        .await;
    let err = h
        .error(
            post_json("/v1/dashboards/mine/reset", serde_json::json!({})),
            StatusCode::CONFLICT,
        )
        .await;
    assert_eq!(err.error.code, "conflict");
}
