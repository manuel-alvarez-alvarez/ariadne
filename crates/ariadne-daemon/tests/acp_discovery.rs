//! Integration tests for ACP agent discovery and its cached API catalog.

mod common;

use axum::http::StatusCode;
use serde_json::{Value, json};

use common::acp::{option, script, stub_acp_agent};
use common::{Harness, TIMEOUT, eventually, harness, post, post_json};

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
    let h = harness().home(home).discover_agents().await;
    // A probe under full-suite load can run out its timeout: probe again
    // until the agent itself answers — accepted, or rejected for a reason of
    // its own — so no test reads a timed-out snapshot.
    eventually(TIMEOUT, "the probe to answer", || async {
        let settled =
            h.launcher.registry.agents().await.iter().any(|a| {
                a.id == id && a.rejection_reason.as_deref() != Some("discovery timed out")
            });
        if !settled {
            h.launcher.registry.refresh().await;
        }
        settled
    })
    .await;
    h
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
        assert!(model.get("agent_kind").is_none(), "{model}");
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

/// A registry id is any word without the catalog's delimiter in it: one
/// that happens to spell the name of an agent's own CLI is an agent like any
/// other, probed, resolved, and pinned by its id.
#[tokio::test]
async fn a_registry_id_that_spells_a_cli_name_is_an_agent_like_any_other() {
    let dir = tempfile::tempdir().unwrap();
    let agent = stub_acp_agent(dir.path(), script());
    let h = harness_with_agent("codex", &agent).await;

    let agents: Vec<Value> = h.get("/v1/acp-agents").await;
    let entry = agents.iter().find(|a| a["id"] == "codex").unwrap();
    assert_eq!(entry["status"], "ready", "{entry}");
    assert_eq!(
        h.launcher.registry.command_of("codex"),
        Some(vec![agent.bin.clone()])
    );

    let repo = h.repository(&dir.path().join("plain-repo")).await;
    let goal: Value = h
        .json(
            post_json(
                "/v1/goals",
                json!({
                    "title": "Ship it",
                    "repository_ids": [repo.id],
                    "model": "codex:old-model",
                }),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert_eq!(goal["model"], "codex:old-model");
}

/// The first holder of an id keeps it — built-ins first, then configuration
/// order. Every later entry with the same id is rejected with the reason on
/// it, and nothing resolves its command: `command_of` answers with the
/// first holder's.
#[tokio::test]
async fn a_registry_id_already_taken_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let agent = stub_acp_agent(dir.path(), script());
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(
        home.join("config.toml"),
        format!(
            "[[acp_agents]]\nid = \"claude-code-acp\"\ncommand = [{bin:?}]\n\n\
             [[acp_agents]]\nid = \"twin\"\ncommand = [{bin:?}]\n\n\
             [[acp_agents]]\nid = \"twin\"\ncommand = [{bin:?}, \"second\"]\n",
            bin = agent.bin
        ),
    )
    .unwrap();
    let h = harness().home(home).discover_agents().await;
    // A probe under full-suite load can run out its timeout: probe again
    // until the first twin is accepted, so no assertion reads a timed-out
    // snapshot.
    eventually(TIMEOUT, "discovery to accept the first twin", || async {
        let agents: Vec<Value> = h.get("/v1/acp-agents").await;
        if agents
            .iter()
            .any(|a| a["id"] == "twin" && a["status"] == "ready")
        {
            return true;
        }
        h.launcher.registry.refresh().await;
        false
    })
    .await;

    let agents: Vec<Value> = h.get("/v1/acp-agents").await;
    let taken = |id: &str, builtin: bool| {
        agents
            .iter()
            .find(|a| a["id"] == id && a["builtin"] == builtin)
            .unwrap_or_else(|| panic!("no {id} (builtin: {builtin}) in {agents:#?}"))
            .clone()
    };

    // The configured duplicate of a built-in id is rejected; the built-in
    // keeps the id and its command.
    let duplicate = taken("claude-code-acp", false);
    assert_eq!(duplicate["status"], "rejected", "{duplicate}");
    assert!(
        duplicate["rejection_reason"]
            .as_str()
            .unwrap()
            .contains("already another agent's"),
        "{duplicate}"
    );
    assert_eq!(
        h.launcher.registry.command_of("claude-code-acp"),
        Some(vec!["claude-code-acp".to_string()]),
        "the built-in keeps its id"
    );

    // Between two configured entries, the first keeps the id and the
    // second is rejected.
    let twins: Vec<&Value> = agents.iter().filter(|a| a["id"] == "twin").collect();
    assert_eq!(twins.len(), 2, "{agents:#?}");
    assert_eq!(twins[0]["status"], "ready", "{:#?}", twins[0]);
    assert_eq!(twins[1]["status"], "rejected", "{:#?}", twins[1]);
    assert_eq!(
        h.launcher.registry.command_of("twin"),
        Some(vec![agent.bin.clone()]),
        "the first configured entry keeps its id"
    );
}

/// An id that carries the catalog delimiter could never be split back out
/// of a catalog id, so it is refused at build like a reserved one: listed,
/// rejected with the reason, and never resolved.
#[tokio::test]
async fn a_registry_id_with_the_catalog_delimiter_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let agent = stub_acp_agent(dir.path(), script());
    let h = harness_with_agent("my:agent", &agent).await;

    let agents: Vec<Value> = h.get("/v1/acp-agents").await;
    let entry = agents.iter().find(|a| a["id"] == "my:agent").unwrap();
    assert_eq!(entry["status"], "rejected", "{entry}");
    assert!(
        entry["rejection_reason"]
            .as_str()
            .unwrap()
            .contains("contains `:`"),
        "{entry}"
    );
    assert!(h.launcher.registry.command_of("my:agent").is_none());
    assert!(h.launcher.registry.command_of("my").is_none());
}
