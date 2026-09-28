use ariadne_api::permissions::{
    LearnedPermissionDto, LearnedPermissionSource, LearnedPermissionsResponse,
};
use ariadne_api::stream::DomainEvent;
use ariadne_store::NewRepository;
use axum::http::StatusCode;

use crate::common::{delete, get, harness, next_event, post_json, put_json};

#[tokio::test]
async fn learned_permission_routes_validate_crud_and_publish_fat_events() {
    let h = harness().await;
    let repo = h
        .store
        .create_repository(NewRepository {
            path: "/tmp/learned-api".into(),
            base_branch: "main".into(),
            description: None,
            permission_mode: None,
        })
        .await
        .unwrap();
    let mut rx = h.bus.subscribe();
    let openapi: serde_json::Value = h.get("/api-docs/openapi.json").await;
    assert!(openapi["paths"]["/v1/permissions/learned"]["get"].is_object());
    assert!(openapi["paths"]["/v1/permissions/learned"]["post"].is_object());
    assert!(openapi["paths"]["/v1/permissions/learned/{id}"]["put"].is_object());
    assert!(openapi["paths"]["/v1/permissions/learned/{id}"]["delete"].is_object());
    let event = openapi["components"]["schemas"]["DomainEvent"]["oneOf"]
        .as_array()
        .unwrap();
    assert!(
        event
            .iter()
            .any(|schema| schema.to_string().contains("learned_permission_created"))
    );
    let created: LearnedPermissionDto = h
        .json(
            post_json(
                "/v1/permissions/learned",
                serde_json::json!({
                    "repository_id": repo.id, "tool_name": "Bash", "kind": "execute"
                }),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert_eq!(created.source, LearnedPermissionSource::Manual);
    assert!(created.tool_call.is_none());
    let event = next_event(&mut rx, |event| {
        event.event.kind() == "learned_permission_created"
    })
    .await;
    let DomainEvent::LearnedPermissionCreated(dto) = event.event else {
        unreachable!()
    };
    assert_eq!(dto, created);

    let listed: LearnedPermissionsResponse = h
        .json(
            get(&format!(
                "/v1/permissions/learned?repository={}",
                created.repository_id
            )),
            StatusCode::OK,
        )
        .await;
    assert_eq!(listed.items, vec![created.clone()]);
    let shown: LearnedPermissionDto = h
        .json(
            get(&format!("/v1/permissions/learned/{}", created.id)),
            StatusCode::OK,
        )
        .await;
    assert_eq!(shown, created);

    let edited: LearnedPermissionDto = h
        .json(
            put_json(
                &format!("/v1/permissions/learned/{}", created.id),
                serde_json::json!({"tool_name":"Shell"}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(edited.tool_name, "Shell");
    let event = next_event(&mut rx, |event| {
        event.event.kind() == "learned_permission_updated"
    })
    .await;
    assert!(matches!(
        event.event,
        DomainEvent::LearnedPermissionUpdated(_)
    ));

    let duplicate = h
        .error(
            post_json(
                "/v1/permissions/learned",
                serde_json::json!({
                    "repository_id": edited.repository_id, "tool_name": "Shell", "kind": "execute"
                }),
            ),
            StatusCode::CONFLICT,
        )
        .await;
    assert_eq!(duplicate.error.code, "learned_permission_exists");
    let invalid = h
        .error(
            post_json(
                "/v1/permissions/learned",
                serde_json::json!({
                    "repository_id": edited.repository_id, "tool_name": "", "kind": "execute"
                }),
            ),
            StatusCode::UNPROCESSABLE_ENTITY,
        )
        .await;
    assert_eq!(invalid.error.code, "invalid_request");
    let missing_repo = h
        .error(
            post_json(
                "/v1/permissions/learned",
                serde_json::json!({
                    "repository_id": "missing", "tool_name": "Bash", "kind": "execute"
                }),
            ),
            StatusCode::NOT_FOUND,
        )
        .await;
    assert_eq!(missing_repo.error.code, "repository_not_found");

    let (status, _) = h
        .send(delete(&format!("/v1/permissions/learned/{}", created.id)))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let event = next_event(&mut rx, |event| {
        event.event.kind() == "learned_permission_deleted"
    })
    .await;
    let DomainEvent::LearnedPermissionDeleted(dto) = event.event else {
        unreachable!()
    };
    assert_eq!(dto.id, created.id);
    let missing = h
        .error(
            get(&format!("/v1/permissions/learned/{}", created.id)),
            StatusCode::NOT_FOUND,
        )
        .await;
    assert_eq!(missing.error.code, "learned_permission_not_found");
}
