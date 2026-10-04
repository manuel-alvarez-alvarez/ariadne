//! Integration tests for the `tools` stats family (023): What do the agents do?

use crate::common;

use axum::http::StatusCode;
use serde_json::json;

use ariadne_api::stats::ToolStatsDto;

use common::acp::{registry_home, script, stub_acp_agent};
use common::{TIMEOUT, eventually, get, harness};

/// `GET /v1/stats/tools` answers 200 with the family's DTO, and the route is
/// in the API document under the `stats` tag.
#[tokio::test]
async fn the_tools_stat_answers_and_is_in_the_api_document() {
    let h = harness().await;
    let stats: ToolStatsDto = h.get("/v1/stats/tools").await;
    assert_eq!(stats, ToolStatsDto::default());
    let doc: serde_json::Value = h.get("/api-docs/openapi.json").await;
    assert_eq!(doc["paths"]["/v1/stats/tools"]["get"]["tags"][0], "stats");
}

/// A tool call named by the agent's own `name`, or by its
/// `_meta.claudeCode.toolName` where it sends no `name`, writes a `tool_call`
/// fact under that name, with its ACP `kind`; a call with a title alone is
/// named by the title.
#[tokio::test]
async fn a_tool_call_fact_is_named_by_its_stable_name_and_kind() {
    let mut scripted = script();
    scripted["prompts"] = json!([{
        "updates": [
            {"sessionUpdate": "tool_call", "toolCallId": "call-1", "name": "Bash",
             "title": "Run a shell command", "kind": "execute", "status": "pending",
             "rawInput": {"command": "ls"}},
            {"sessionUpdate": "tool_call_update", "toolCallId": "call-1", "status": "completed"},
            {"sessionUpdate": "tool_call", "toolCallId": "call-2",
             "_meta": {"claudeCode": {"toolName": "Read"}}, "title": "Reading a file",
             "kind": "read", "status": "pending"},
            {"sessionUpdate": "tool_call_update", "toolCallId": "call-2", "status": "completed"},
            {"sessionUpdate": "tool_call", "toolCallId": "call-3", "title": "git show HEAD",
             "kind": "execute", "status": "pending"},
            {"sessionUpdate": "tool_call_update", "toolCallId": "call-3", "status": "completed"},
            {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "done"}},
        ],
        "stop_reason": "end_turn",
    }]);
    let agent_dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(agent_dir.path(), scripted);
    let h = harness().home(registry_home(&stub)).await;
    h.git_repo("repo");
    let cast = h.cast().await;
    h.launcher.spawn_author(&cast.task.id).await.unwrap();
    eventually(TIMEOUT, "the three tool facts", || async {
        h.facts("tool_call").await.len() == 3
    })
    .await;
    let facts = h.facts("tool_call").await;
    assert_eq!(facts[0].data["tool_name"], "Bash", "named by /name");
    assert_eq!(facts[0].data["kind"], "execute");
    assert_eq!(
        facts[1].data["tool_name"], "Read",
        "named by /_meta/claudeCode/toolName where there is no /name"
    );
    assert_eq!(facts[1].data["kind"], "read");
    assert_eq!(
        facts[2].data["tool_name"], "git show HEAD",
        "named by /title where there is neither"
    );
    assert_eq!(facts[2].data["kind"], "execute");
}

/// `GET /v1/stats/tools?limit=1` keeps the one tool with the most calls in
/// `top` and sums the rest into `other`; `by_kind` groups every call by its
/// kind regardless of the limit.
#[tokio::test]
async fn limit_keeps_the_top_tools_and_sums_the_rest_into_other() {
    let mut scripted = script();
    let mut updates = Vec::new();
    for call_id in ["call-1", "call-2", "call-3"] {
        updates.push(json!({"sessionUpdate": "tool_call", "toolCallId": call_id,
            "title": "Bash", "kind": "execute", "status": "pending"}));
        updates.push(
            json!({"sessionUpdate": "tool_call_update", "toolCallId": call_id, "status": "completed"}),
        );
    }
    updates.push(json!({"sessionUpdate": "tool_call", "toolCallId": "call-4",
        "title": "Read", "kind": "read", "status": "pending"}));
    updates.push(
        json!({"sessionUpdate": "tool_call_update", "toolCallId": "call-4", "status": "completed"}),
    );
    updates.push(json!({"sessionUpdate": "tool_call", "toolCallId": "call-5",
        "title": "Edit", "kind": "edit", "status": "pending"}));
    updates.push(
        json!({"sessionUpdate": "tool_call_update", "toolCallId": "call-5", "status": "completed"}),
    );
    updates.push(json!({"sessionUpdate": "agent_message_chunk",
        "content": {"type": "text", "text": "done"}}));
    scripted["prompts"] = json!([{"updates": updates, "stop_reason": "end_turn"}]);
    let agent_dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(agent_dir.path(), scripted);
    let h = harness().home(registry_home(&stub)).await;
    h.git_repo("repo");
    let cast = h.cast().await;
    h.launcher.spawn_author(&cast.task.id).await.unwrap();
    eventually(TIMEOUT, "the five tool facts", || async {
        h.facts("tool_call").await.len() == 5
    })
    .await;

    let stats: ToolStatsDto = h.get("/v1/stats/tools?limit=1").await;
    assert_eq!(stats.calls, 5);
    assert_eq!(stats.errors, 0);
    assert_eq!(stats.tools, 3);
    assert_eq!(stats.top.len(), 1, "{:?}", stats.top);
    assert_eq!(stats.top[0].tool_name, "Bash");
    assert_eq!(stats.top[0].kind, "execute");
    assert_eq!(stats.top[0].calls, 3);
    assert_eq!(stats.top[0].errors, 0);
    assert_eq!(
        stats.other,
        ariadne_api::stats::OtherToolsDto {
            tools: 2,
            calls: 2,
            errors: 0,
        }
    );
    assert_eq!(stats.by_kind.len(), 3, "{:?}", stats.by_kind);
    assert_eq!(stats.by_kind[0].kind, "execute");
    assert_eq!(stats.by_kind[0].calls, 3);
}

/// `limit` outside 1 to 100 is refused with a 400 naming it.
#[tokio::test]
async fn a_limit_outside_1_to_100_is_refused() {
    let h = harness().await;
    let error = h
        .error(get("/v1/stats/tools?limit=0"), StatusCode::BAD_REQUEST)
        .await;
    assert_eq!(error.error.code, "invalid_request");
    assert!(error.error.message.contains('0'), "{error:?}");
}
