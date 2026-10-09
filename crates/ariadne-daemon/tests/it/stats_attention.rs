//! Integration tests for the `attention` stats family (023): How much did it need me?

use crate::common;

use ariadne_api::stats::AttentionStatsDto;
use ariadne_core::{AttentionReason, PermissionMode, TaskStatus};
use serde_json::json;

use common::acp::{discovery_settled, registry_home, script, stub_acp_agent};
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

/// A console reply and a clear both appear in the family response. The
/// permission is asked by the agent of a task's first column, started by the
/// scheduler as the daemon starts it.
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
    let h = harness()
        .home(registry_home(&stub))
        .discover_agents()
        .scheduler()
        .await;
    discovery_settled(&h, &stub).await;
    h.git_repo("repo");
    let cast = h.cast().await;
    h.set_permission_mode(&cast.repo, PermissionMode::Ask).await;
    h.activate(&cast.goal).await;
    h.advance(&cast.task, TaskStatus::InProgress).await;
    h.notify(&cast.task.id);
    // The column's agent runs on the registry stub, whose prompts the
    // harness does not read: the session is waited for by its row.
    eventually(TIMEOUT, "the column agent to start", || async {
        h.running_session(&cast.task.id, ariadne_core::Seat::Agent)
            .await
            .is_some()
    })
    .await;
    let session = h
        .running_session(&cast.task.id, ariadne_core::Seat::Agent)
        .await
        .unwrap();
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
    // The console answer takes the permission flag down, which is a clear
    // with a fact of its own; the cleared question is the second.
    eventually(TIMEOUT, "the attention facts", || async {
        h.facts("permission").await.len() == 1
            && h.facts("attention")
                .await
                .iter()
                .any(|fact| fact.data["reason"] == "waiting_input")
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

/// A console permission and a cleared question each count as an
/// intervention, and the response carries the `interventions` object.
#[tokio::test]
async fn the_attention_stat_answers_the_interventions_object() {
    use ariadne_api::stats::AttentionInterventionsDto;
    use ariadne_store::NewStatFact;

    let h = harness().await;
    let fact = |kind: &str, data: serde_json::Value| NewStatFact {
        kind: kind.into(),
        repo_id: Some("repo".into()),
        goal_id: None,
        task_id: None,
        session_id: None,
        launch_id: None,
        seat: None,
        model: None,
        effort: None,
        skills: vec![],
        data,
    };
    for f in [
        fact(
            "permission",
            json!({"decided_by": "console", "wait_ms": 4_000}),
        ),
        fact(
            "attention",
            json!({"reason": "waiting_input", "wait_secs": 20}),
        ),
        fact(
            "attention",
            json!({"reason": "waiting_permission", "wait_secs": 9_000}),
        ),
    ] {
        h.store.record_fact(f).await.unwrap();
    }

    let stats: AttentionStatsDto = h.get("/v1/stats/attention?repo=repo").await;
    assert_eq!(
        stats.interventions,
        AttentionInterventionsDto {
            permissions: 1,
            questions: 1,
            stalls: 0,
            total: 2,
            person_secs: 24.0,
        }
    );
}
