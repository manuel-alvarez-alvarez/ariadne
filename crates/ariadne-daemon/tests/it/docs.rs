//! The embedded API documentation and the schema it loads (012).
use crate::common::{get, harness};
use axum::http::{StatusCode, header};
use serde_json::Value;

#[tokio::test]
async fn swagger_serves_its_page_assets_and_the_daemons_schema() {
    let h = harness().await;
    let redirect = h.response(get("/docs")).await;
    assert_eq!(redirect.status(), StatusCode::SEE_OTHER);
    assert_eq!(redirect.headers()[header::LOCATION], "/docs/");

    let (status, body) = h.send(get("/docs/")).await;
    assert_eq!(status, StatusCode::OK);
    let html = String::from_utf8(body).unwrap();
    for asset in [
        "swagger-ui.css",
        "swagger-ui-bundle.js",
        "swagger-ui-standalone-preset.js",
        "swagger-initializer.js",
    ] {
        assert!(html.contains(asset), "the page loads {asset}");
        let (status, body) = h.send(get(&format!("/docs/{asset}"))).await;
        assert_eq!(status, StatusCode::OK, "the daemon serves {asset}");
        assert!(!body.is_empty(), "{asset} is not empty");
        if asset == "swagger-initializer.js" {
            let script = String::from_utf8(body).unwrap();
            assert!(script.contains("/api-docs/openapi.json"));
        }
    }

    let doc: Value = h.get("/api-docs/openapi.json").await;
    assert!(doc["paths"]["/v1/health"]["get"].is_object());
    assert!(doc["components"]["schemas"]["HealthResponse"].is_object());
}

/// Every trajectory type is in the daemon's schema, the same in the
/// committed `ui/openapi.json`, and generated into `ui/src/api/schema.d.ts`:
/// every step kind and every op, those a later task produces included.
#[tokio::test]
async fn the_committed_api_pair_holds_every_trajectory_type() {
    let h = harness().await;
    let doc: Value = h.get("/api-docs/openapi.json").await;
    let ui = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui");
    let committed: Value =
        serde_json::from_str(&std::fs::read_to_string(ui.join("openapi.json")).unwrap()).unwrap();
    let generated = std::fs::read_to_string(ui.join("src/api/schema.d.ts")).unwrap();

    for name in [
        "TrajectorySnapshotDto",
        "TrajectoryUpdateDto",
        "TrajectoryOpDto",
        "TrajectoryStepsDto",
        "SessionStateDto",
        "ModeStateDto",
        "ConfigOptionDto",
        "ContextWindowDto",
        "CostDto",
        "CommandDto",
        "TurnDto",
        "PromptDto",
        "TurnState",
        "ActivityDto",
        "StepDto",
        "MessageStepDto",
        "ThoughtStepDto",
        "ToolCallStepDto",
        "ToolCallStatus",
        "DiffStatDto",
        "PermissionStepDto",
        "QuestionStepDto",
        "NoteStepDto",
        "ErrorStepDto",
        "SubagentStepDto",
        "CompactionStepDto",
        "PlanDto",
        "PlanVariant",
        "ContentBlockDto",
        "ToolCallDetailDto",
        "ToolDetailDto",
        "CommandDetailDto",
        "DiffDetailDto",
        "DiffFileDto",
        "FileDetailDto",
        "SearchDetailDto",
        "FetchDetailDto",
        "GenericDetailDto",
        "RawToolDto",
    ] {
        let schema = &doc["components"]["schemas"][name];
        assert!(schema.is_object(), "the daemon serves {name}");
        assert_eq!(
            &committed["components"]["schemas"][name], schema,
            "ui/openapi.json holds {name} as the daemon serves it"
        );
        assert!(
            generated.contains(&format!("        {name}: ")),
            "schema.d.ts holds {name}"
        );
    }
    let kinds = |name: &str, tag: &str| -> Vec<String> {
        doc["components"]["schemas"][name]["oneOf"]
            .as_array()
            .unwrap()
            .iter()
            .map(|variant| {
                let tagged = variant["allOf"].get(1).unwrap_or(variant);
                tagged["properties"][tag]["enum"][0]
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect()
    };
    assert_eq!(
        kinds("StepDto", "type"),
        [
            "message",
            "thought",
            "tool_call",
            "permission",
            "question",
            "note",
            "error",
            "subagent",
            "compaction"
        ]
    );
    assert_eq!(
        kinds("TrajectoryOpDto", "op"),
        [
            "turn_upsert",
            "item_upsert",
            "text_append",
            "item_remove",
            "plan_upsert",
            "subagent_upsert",
            "session_patch"
        ]
    );
    assert_eq!(
        kinds("ToolDetailDto", "type"),
        ["command", "diff", "file", "search", "fetch", "generic"]
    );
    for path in [
        "/v1/sessions/{id}/trajectory/items/{item_id}",
        "/v1/sessions/{id}/trajectory/blobs/{event_id}/{index}",
        "/v1/sessions/{id}/trajectory",
        "/v1/sessions/{id}/trajectory/stream",
        "/v1/sessions/{id}/trajectory/turns/{turn_id}/steps",
    ] {
        assert_eq!(
            &committed["paths"][path], &doc["paths"][path],
            "ui/openapi.json holds {path}"
        );
        assert!(doc["paths"][path]["get"].is_object(), "{path}");
    }
}
