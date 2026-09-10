//! Integration tests for ACP agent discovery and its cached API catalog.

mod common;

use axum::http::StatusCode;
use serde_json::{Value, json};

use common::acp::{option, script, stub_acp_agent};
use common::{Harness, harness, post};

fn home_with_agent(id: &str, bin: &str) -> std::path::PathBuf {
    let home = std::path::Path::new(bin).parent().unwrap().join("home");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(
        home.join("config.toml"),
        format!("[[acp_agents]]\nid = {id:?}\ncommand = [{bin:?}]\n"),
    )
    .unwrap();
    home
}

async fn harness_with_agent(id: &str, agent: &common::acp::StubAcpAgent) -> Harness {
    let home = home_with_agent(id, &agent.bin);
    harness().home(home).discover_agents().await
}

fn choice(value: &str, name: &str) -> Value {
    json!({"value": value, "name": name})
}

/// The registry keeps every shipped command and extends it with configured agents.
#[tokio::test]
async fn the_api_lists_the_three_known_agents_and_one_user_agent() {
    let dir = tempfile::tempdir().unwrap();
    let mut setup = script();
    setup["capabilities"]["sessionCapabilities"]["list"] = json!({});
    let agent = stub_acp_agent(dir.path(), setup);
    let h = harness_with_agent("test-agent", &agent).await;

    let agents: Vec<Value> = h.get("/v1/acp-agents").await;
    let ids: Vec<&str> = agents.iter().filter_map(|a| a["id"].as_str()).collect();
    assert!(ids.contains(&"claude-code-acp"), "{ids:?}");
    assert!(ids.contains(&"codex-acp"), "{ids:?}");
    assert!(ids.contains(&"opencode-acp"), "{ids:?}");
    assert!(ids.contains(&"test-agent"), "{ids:?}");
    let custom = agents
        .iter()
        .find(|agent| agent["id"] == "test-agent")
        .unwrap();
    assert_eq!(custom["status"], "ready");
    assert_eq!(custom["capabilities"]["session_list"], true);
    assert_eq!(custom["capabilities"]["session_load"], true);
    assert_eq!(custom["degraded"], json!([]));

    let doc: Value = h.get("/api-docs/openapi.json").await;
    assert!(doc["paths"]["/v1/acp-agents"]["get"].is_object());
    assert!(doc["paths"]["/v1/acp-agents/refresh"]["post"].is_object());
    assert!(doc["components"]["schemas"]["AcpAgentDto"].is_object());
}

/// Runtime-compatible option-name fallbacks enter models and efforts in the catalog.
#[tokio::test]
async fn model_and_effort_name_fallbacks_enter_the_discovered_catalog() {
    let dir = tempfile::tempdir().unwrap();
    let mut setup = script();
    setup["config_options"] = json!([
        {
            "id": "model", "name": "Model", "category": "mode",
            "type": "select", "currentValue": "small",
            "options": [choice("small", "Small"), choice("large", "Large")]
        },
        {
            "id": "reasoning", "name": "Effort", "category": "mode",
            "type": "select", "currentValue": "high",
            "options": [choice("low", "Low"), choice("high", "High")]
        }
    ]);
    let agent = stub_acp_agent(dir.path(), setup);
    let h = harness_with_agent("test-agent", &agent).await;

    let models: Vec<Value> = h.get("/v1/models").await;
    let found: Vec<&Value> = models
        .iter()
        .filter(|model| model["agent_id"] == "test-agent")
        .collect();
    assert_eq!(
        found
            .iter()
            .map(|m| m["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["test-agent:small", "test-agent:large"]
    );
    for model in found {
        assert_eq!(model["agent_kind"], "acp");
        assert_eq!(model["efforts"][0]["id"], "low");
        assert_eq!(model["efforts"][1]["id"], "high");
        assert_eq!(model["efforts"][1]["default"], true);
    }
}

/// A missing model option rejects the agent and explains the rejection through doctor.
#[tokio::test]
async fn an_agent_without_a_model_option_is_rejected_and_doctor_shows_why() {
    let dir = tempfile::tempdir().unwrap();
    let mut setup = script();
    setup["config_options"] = json!([option("effort-id", "thought_level", "low")]);
    let agent = stub_acp_agent(dir.path(), setup);
    let h = harness_with_agent("no-model", &agent).await;

    let agents: Vec<Value> = h.get("/v1/acp-agents").await;
    let rejected = agents.iter().find(|a| a["id"] == "no-model").unwrap();
    assert_eq!(rejected["status"], "rejected");
    assert!(
        rejected["rejection_reason"]
            .as_str()
            .unwrap()
            .contains("model")
    );

    let doctor: Value = h.get("/v1/doctor").await;
    let rejected = doctor["acp_agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == "no-model")
        .unwrap();
    assert!(
        rejected["rejection_reason"]
            .as_str()
            .unwrap()
            .contains("model")
    );
}

/// Every missing optional capability has its own degradation flag.
#[tokio::test]
async fn every_optional_capability_gap_sets_its_degraded_flag() {
    let dir = tempfile::tempdir().unwrap();
    let mut setup = script();
    setup["capabilities"] = json!({"loadSession": {}});
    setup["config_options"] = json!([{
        "id": "model-id", "name": "Model", "category": "model",
        "type": "select", "currentValue": "small",
        "options": [choice("small", "Small")]
    }]);
    let agent = stub_acp_agent(dir.path(), setup);
    let h = harness_with_agent("degraded", &agent).await;

    let agents: Vec<Value> = h.get("/v1/acp-agents").await;
    let degraded = agents.iter().find(|a| a["id"] == "degraded").unwrap();
    assert_eq!(degraded["status"], "ready");
    assert_eq!(degraded["capabilities"]["thought_level"], false);
    assert_eq!(degraded["capabilities"]["session_list"], false);
    assert_eq!(degraded["capabilities"]["session_load"], false);
    assert_eq!(
        degraded["degraded"],
        json!(["no_efforts", "no_adoption", "no_restart_resume"])
    );
}

/// Version one, session creation, and prompting are all required.
#[tokio::test]
async fn every_required_acp_capability_is_enforced() {
    let cases = [
        ("wrong-version", json!({"protocol_version": 2}), "version 1"),
        (
            "no-new",
            json!({"unsupported_methods": ["session/new"]}),
            "session/new",
        ),
        (
            "no-prompt",
            json!({"unsupported_methods": ["session/prompt"]}),
            "session/prompt",
        ),
    ];
    for (id, changes, reason) in cases {
        let dir = tempfile::tempdir().unwrap();
        let mut setup = script();
        for (key, value) in changes.as_object().unwrap() {
            setup[key] = value.clone();
        }
        let agent = stub_acp_agent(dir.path(), setup);
        let h = harness_with_agent(id, &agent).await;
        let agents: Vec<Value> = h.get("/v1/acp-agents").await;
        let rejected = agents.iter().find(|agent| agent["id"] == id).unwrap();
        assert_eq!(rejected["status"], "rejected", "{id}: {rejected}");
        assert!(
            rejected["rejection_reason"]
                .as_str()
                .unwrap()
                .contains(reason),
            "{id}: {rejected}"
        );
    }
}

/// Refresh replaces the cached models with a new probe result.
#[tokio::test]
async fn discovery_refreshes_on_demand() {
    let dir = tempfile::tempdir().unwrap();
    let agent = stub_acp_agent(dir.path(), script());
    let h = harness_with_agent("refreshable", &agent).await;
    let initial: Vec<Value> = h.get("/v1/models").await;
    assert!(
        initial
            .iter()
            .any(|model| model["id"] == "refreshable:old-model")
    );

    let mut changed = script();
    changed["config_options"] = json!([{
        "id": "model-id", "name": "Model", "category": "model",
        "type": "select", "currentValue": "new-model", "options": []
    }]);
    let _replacement = stub_acp_agent(dir.path(), changed);
    assert!(std::path::Path::new(&_replacement.bin).exists());
    let refreshed_agents: Vec<Value> = h.json(post("/v1/acp-agents/refresh"), StatusCode::OK).await;
    assert_eq!(
        refreshed_agents
            .iter()
            .find(|agent| agent["id"] == "refreshable")
            .unwrap()["status"],
        "ready",
        "{refreshed_agents:#?}"
    );

    let refreshed: Vec<Value> = h.get("/v1/models").await;
    assert!(
        refreshed
            .iter()
            .any(|model| model["id"] == "refreshable:new-model"),
        "{refreshed:#?}"
    );
    assert!(
        !refreshed
            .iter()
            .any(|model| model["id"] == "refreshable:old-model")
    );
}
