use ariadne_api::permissions::{
    LearnedPermissionDto, LearnedPermissionLevel, LearnedPermissionScope, LearnedPermissionTarget,
    LearnedPermissionsResponse,
};
use ariadne_api::stream::DomainEvent;
use ariadne_store::{NewLearnedPermission, NewRepository};
use axum::http::StatusCode;
use serde_json::json;

use crate::common::{delete, get, harness, next_event, post_json, put_json};

fn choice(repository_id: &str, selected: &str) -> NewLearnedPermission {
    NewLearnedPermission {
        repository_id: repository_id.into(),
        tool_name: "Bash".into(),
        key: r#"{"command":"cargo test"}"#.into(),
        level: "command".into(),
        family: "cargo test".into(),
        risk_tags: vec!["background_process".into()],
        scope: "repository".into(),
        tool_call: json!({"toolCallId": "call-1", "name": "Bash", "title": "cargo test",
                          "rawInput": {"command": "cargo test"}}),
        options: json!([
            {"optionId": "yes", "name": "Allow", "kind": "allow_once"},
            {"optionId": "no", "name": "Reject", "kind": "reject_once"},
        ]),
        selected_option: selected.into(),
        target: "ai".into(),
        output: Some(json!({"label": "ask", "danger": 0.4})),
    }
}

/// The routes read, delete and widen or narrow recorded choices. A person
/// creates none: `POST` takes a whole row, as a daemon that syncs to this one
/// pushes a decision made there (037). `PUT` only ever changes
/// `scope`, and refuses an unknown id or a scope that is neither
/// `repository` nor `all`. Each change publishes its event.
#[tokio::test]
async fn learned_permission_routes_read_and_delete_and_publish_fat_events() {
    let h = harness().await;
    let repo = h
        .store
        .create_repository(NewRepository {
            default_workflow: None,
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
    assert!(openapi["components"]["schemas"]["CreateLearnedPermissionRequest"].is_null());
    assert!(openapi["components"]["schemas"]["UpdateLearnedPermissionRequest"].is_object());
    assert!(
        openapi["components"]["schemas"]["LearnedPermissionDto"]["required"]
            .as_array()
            .unwrap()
            .contains(&json!("output")),
        "output is a required nullable field"
    );
    let dto = &openapi["components"]["schemas"]["LearnedPermissionDto"];
    let reply = &openapi["components"]["schemas"]["PermissionReplyDto"];
    for field in ["learned_id", "learned_level", "learned_key"] {
        assert!(
            reply["properties"][field].is_object(),
            "{field} is absent from the reply"
        );
    }
    for field in ["key", "level", "family", "risk_tags", "scope"] {
        assert!(
            dto["properties"][field].is_object(),
            "{field} is a property"
        );
        assert!(
            dto["required"].as_array().unwrap().contains(&json!(field)),
            "{field} is required"
        );
    }
    assert_eq!(
        openapi["components"]["schemas"]["LearnedPermissionLevel"]["enum"],
        json!(["once", "command", "family"])
    );
    assert_eq!(
        openapi["components"]["schemas"]["LearnedPermissionScope"]["enum"],
        json!(["repository", "all"])
    );

    h.store
        .record_learned_permission(choice(&repo.id, "no"))
        .await
        .unwrap();
    let event = next_event(&mut rx, |event| {
        event.event.kind() == "learned_permission_created"
    })
    .await;
    let DomainEvent::LearnedPermissionCreated(created) = event.event else {
        unreachable!()
    };
    h.store
        .record_learned_permission(choice(&repo.id, "yes"))
        .await
        .unwrap();
    let event = next_event(&mut rx, |event| {
        event.event.kind() == "learned_permission_updated"
    })
    .await;
    let DomainEvent::LearnedPermissionUpdated(updated) = event.event else {
        unreachable!()
    };
    assert_eq!(updated.id, created.id);
    assert_eq!(updated.selected_option, "yes");
    assert_eq!(updated.target, LearnedPermissionTarget::Ai);
    assert_eq!(updated.tool_name, "Bash");
    assert_eq!(updated.key, r#"{"command":"cargo test"}"#);
    assert_eq!(updated.level, LearnedPermissionLevel::Command);
    assert_eq!(updated.family, "cargo test");
    assert_eq!(updated.risk_tags, ["background_process"]);
    assert_eq!(updated.scope, LearnedPermissionScope::Repository);
    assert_eq!(
        updated.tool_call["rawInput"],
        json!({"command": "cargo test"})
    );
    assert_eq!(updated.options.as_array().unwrap().len(), 2);
    assert_eq!(updated.output, Some(json!({"label": "ask", "danger": 0.4})));

    let listed: LearnedPermissionsResponse = h
        .json(
            get(&format!("/v1/permissions/learned?repository={}", repo.id)),
            StatusCode::OK,
        )
        .await;
    assert_eq!(listed.items, vec![updated.clone()]);
    let other: LearnedPermissionsResponse = h
        .json(
            get("/v1/permissions/learned?repository=other"),
            StatusCode::OK,
        )
        .await;
    assert!(other.items.is_empty());
    let shown: LearnedPermissionDto = h
        .json(
            get(&format!("/v1/permissions/learned/{}", updated.id)),
            StatusCode::OK,
        )
        .await;
    assert_eq!(shown, updated);

    let (status, _) = h
        .send(post_json(
            "/v1/permissions/learned",
            json!({"repository_id": repo.id, "tool_name": "Bash", "kind": "execute"}),
        ))
        .await;
    // The form a person once filled is no row: refused, field by field.
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let missing_id = h
        .error(
            put_json(
                "/v1/permissions/learned/unknown-row",
                json!({"scope": "all"}),
            ),
            StatusCode::NOT_FOUND,
        )
        .await;
    assert_eq!(missing_id.error.code, "learned_permission_not_found");

    let bad_scope = h
        .error(
            put_json(
                &format!("/v1/permissions/learned/{}", updated.id),
                json!({"scope": "global"}),
            ),
            StatusCode::UNPROCESSABLE_ENTITY,
        )
        .await;
    assert_eq!(bad_scope.error.code, "invalid_request");

    let widened: LearnedPermissionDto = h
        .json(
            put_json(
                &format!("/v1/permissions/learned/{}", updated.id),
                json!({"scope": "all"}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(widened.id, updated.id);
    assert_eq!(widened.scope, LearnedPermissionScope::All);
    let event = next_event(&mut rx, |event| {
        event.event.kind() == "learned_permission_updated"
    })
    .await;
    let DomainEvent::LearnedPermissionUpdated(published) = event.event else {
        unreachable!()
    };
    assert_eq!(published, widened);

    let (status, _) = h
        .send(delete(&format!("/v1/permissions/learned/{}", updated.id)))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let event = next_event(&mut rx, |event| {
        event.event.kind() == "learned_permission_deleted"
    })
    .await;
    let DomainEvent::LearnedPermissionDeleted(dto) = event.event else {
        unreachable!()
    };
    assert_eq!(dto.id, updated.id);
    let missing = h
        .error(
            get(&format!("/v1/permissions/learned/{}", updated.id)),
            StatusCode::NOT_FOUND,
        )
        .await;
    assert_eq!(missing.error.code, "learned_permission_not_found");
}

/// `POST` writes a whole row as a syncing daemon pushes it (037): a row with
/// the same repository, tool name, level and key keeps its id, and an
/// unknown repository is a 404.
#[tokio::test]
async fn a_pushed_row_is_recorded_whole_and_keeps_the_id_of_its_key() {
    let h = harness().await;
    let repo = h
        .store
        .create_repository(NewRepository {
            default_workflow: None,
            path: "/tmp/learned-push".into(),
            base_branch: "main".into(),
            description: None,
            permission_mode: None,
        })
        .await
        .unwrap();
    let row = |selected: &str, repository_id: &str| {
        json!({
            "repository_id": repository_id,
            "tool_name": "Bash",
            "key": r#"{"command":"cargo test"}"#,
            "level": "command",
            "family": "cargo test",
            "risk_tags": ["background_process"],
            "scope": "all",
            "tool_call": {"name": "Bash", "rawInput": {"command": "cargo test"}},
            "options": [{"optionId": selected, "name": "Allow", "kind": "allow_once"}],
            "selected_option": selected,
            "target": "learn",
            "output": null,
        })
    };
    let first: LearnedPermissionDto = h
        .json(
            post_json("/v1/permissions/learned", row("yes", &repo.id)),
            StatusCode::OK,
        )
        .await;
    assert_eq!(first.repository_id, repo.id);
    assert_eq!(first.scope, LearnedPermissionScope::All);
    assert_eq!(first.target, LearnedPermissionTarget::Learn);
    assert_eq!(first.risk_tags, ["background_process"]);
    let second: LearnedPermissionDto = h
        .json(
            post_json("/v1/permissions/learned", row("always", &repo.id)),
            StatusCode::OK,
        )
        .await;
    assert_eq!(second.id, first.id);
    assert_eq!(second.created_at, first.created_at);
    assert_eq!(second.selected_option, "always");
    let listed: LearnedPermissionsResponse = h
        .get(&format!("/v1/permissions/learned?repository={}", repo.id))
        .await;
    assert_eq!(listed.items, vec![second]);

    let (status, _) = h
        .send(post_json(
            "/v1/permissions/learned",
            row("yes", "no-such-repository"),
        ))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
