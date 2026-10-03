//! Integration tests for the `attention` stats family (023): How much did it need me?

use crate::common;

use ariadne_api::stats::AttentionStatsDto;
use ariadne_core::{AttentionReason, PermissionMode};
use serde_json::json;

use common::acp::{registry_home, script, stub_acp_agent};
use common::{TIMEOUT, eventually, harness, post_json};

/// `GET /v1/stats/attention` answers 200 with the family's DTO, and the route is
/// in the API document under the `stats` tag.
#[tokio::test]
async fn the_attention_stat_answers_and_is_in_the_api_document() {
    let h = harness().await;
    let stats: AttentionStatsDto = h.get("/v1/stats/attention").await;
    assert_eq!(stats.permissions.total, 0);
    assert_eq!(stats.flags.len(), 5);
    let doc: serde_json::Value = h.get("/api-docs/openapi.json").await;
    assert_eq!(
        doc["paths"]["/v1/stats/attention"]["get"]["tags"][0],
        "stats"
    );
}

/// A console reply and a clear both appear in the family response.
#[tokio::test]
async fn the_attention_stat_counts_a_console_reply_and_a_cleared_flag() {
    let mut scripted = script();
    scripted["prompts"] = json!([{
        "permission": {
            "toolCall": {"toolCallId": "call-1", "name": "Write", "kind": "write"},
            "options": [{"optionId": "yes", "name": "Allow", "kind": "allow_once"}],
        },
    }]);
    let agent_dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(agent_dir.path(), scripted);
    let h = harness().home(registry_home(&stub)).await;
    h.git_repo("repo");
    let cast = h.cast().await;
    h.set_permission_mode(&cast.repo, PermissionMode::Ask).await;
    let session = h.launcher.spawn_author(&cast.task.id).await.unwrap();
    eventually(TIMEOUT, "the permission request", || async {
        h.store
            .count_session_events(&session.id, "permission_request")
            .await
            .unwrap()
            == 1
    })
    .await;
    h.send(post_json(
        &format!("/v1/sessions/{}/console/input", session.id),
        json!({"text": "yes"}),
    ))
    .await;
    h.store
        .set_session_attention(&session.id, AttentionReason::WaitingInput)
        .await
        .unwrap();
    h.store.clear_agent_attention(&session.id).await.unwrap();
    eventually(TIMEOUT, "the attention facts", || async {
        h.facts("permission").await.len() == 1 && h.facts("attention").await.len() == 1
    })
    .await;

    let stats: AttentionStatsDto = h.get("/v1/stats/attention").await;
    assert_eq!(stats.permissions.by_decider[0].decided_by, "console");
    assert_eq!(stats.permissions.by_decider[0].allowed, 1);
    assert_eq!(
        stats
            .flags
            .iter()
            .find(|row| row.reason == "waiting_input")
            .unwrap()
            .raised,
        1
    );
}
