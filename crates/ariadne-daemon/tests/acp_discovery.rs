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
    assert!(ids.contains(&"claude-agent-acp"), "{ids:?}");
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

/// Version one and session creation are both required.
#[tokio::test]
async fn every_required_acp_capability_is_enforced() {
    let cases = [
        ("wrong-version", json!({"protocol_version": 2}), "version 1"),
        (
            "no-new",
            json!({"unsupported_methods": ["session/new"]}),
            "session/new",
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

/// Discovery never prompts — a prompt would be a real model turn, billed on
/// every probe — so an agent that refuses `session/prompt` is still ready.
#[tokio::test]
async fn discovery_sends_no_prompt() {
    let dir = tempfile::tempdir().unwrap();
    let mut setup = script();
    setup["unsupported_methods"] = json!(["session/prompt"]);
    let agent = stub_acp_agent(dir.path(), setup);
    let h = harness_with_agent("unprompted", &agent).await;

    let agents: Vec<Value> = h.get("/v1/acp-agents").await;
    let ready = agents.iter().find(|a| a["id"] == "unprompted").unwrap();
    assert_eq!(ready["status"], "ready", "{ready}");
    let methods = agent.methods();
    assert!(methods.contains(&"session/new".to_string()), "{methods:?}");
    assert!(
        !methods.contains(&"session/prompt".to_string()),
        "{methods:?}"
    );
}

/// A probe that runs out its time is rejected with every capability it had
/// already shown, not reported as an agent that showed nothing.
#[tokio::test]
async fn a_timed_out_probe_keeps_what_it_measured() {
    let dir = tempfile::tempdir().unwrap();
    let mut setup = script();
    setup["silent_methods"] = json!(["session/new"]);
    let agent = stub_acp_agent(dir.path(), setup);
    let h = harness()
        .home(home_with_agent("silent", &agent.bin))
        .discover_agents()
        .await;

    // Under full-suite load even `initialize` can run out the probe's time,
    // so probe again until a snapshot shows the agent got past it.
    eventually(TIMEOUT, "a probe that reaches session/new", || async {
        let agents = h.launcher.registry.agents().await;
        let silent = agents.iter().find(|a| a.id == "silent").unwrap();
        let reached = silent.capabilities.protocol_v1;
        if reached {
            assert_eq!(
                silent.rejection_reason.as_deref(),
                Some("discovery timed out")
            );
            assert!(silent.capabilities.stdio);
            assert!(!silent.capabilities.session_new);
        } else {
            h.launcher.registry.refresh().await;
        }
        reached
    })
    .await;
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

/// Run discovery as a daemon start does, again until the agent is ready — a
/// probe under full-suite load can run out its time.
async fn start_discovery(h: &Harness, id: &str) {
    eventually(TIMEOUT, "discovery to accept the agent", || async {
        h.launcher.registry.discover().await.iter().any(|agent| {
            agent.id == id && agent.status == ariadne_api::agents::AcpAgentStatus::Ready
        })
    })
    .await;
}

fn has(methods: &[String], method: &str) -> bool {
    methods.iter().any(|m| m == method)
}

/// A daemon start reads an agent's catalog once per agent version: the store
/// keeps it, a start at the same version opens no session, a new version is
/// read again, and so is every version on an explicit refresh. The session a
/// read opens is closed where the agent can close one.
#[tokio::test]
async fn a_catalog_is_read_once_per_agent_version() {
    let dir = tempfile::tempdir().unwrap();
    let mut setup = script();
    setup["agent_info"] = json!({"name": "stub", "version": "1.0"});
    setup["capabilities"]["sessionCapabilities"]["close"] = json!({});
    let agent = stub_acp_agent(dir.path(), setup.clone());
    let h = harness_with_agent("versioned", &agent).await;
    let methods = agent.methods();
    assert!(has(&methods, "session/new"), "{methods:?}");
    assert!(has(&methods, "session/close"), "{methods:?}");
    let kept = h.store.list_acp_catalogs().await.unwrap();
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].agent_id, "versioned");
    assert_eq!(kept[0].version, "1.0");
    assert_eq!(kept[0].command(), vec![agent.bin.clone()]);

    agent.clear_messages();
    start_discovery(&h, "versioned").await;
    let methods = agent.methods();
    assert!(has(&methods, "initialize"), "{methods:?}");
    assert!(!has(&methods, "session/new"), "{methods:?}");
    let models: Vec<Value> = h.get("/v1/models").await;
    let model = models
        .iter()
        .find(|model| model["id"] == "versioned:old-model")
        .unwrap_or_else(|| panic!("{models:#?}"));
    assert_eq!(model["efforts"][0]["id"], "low");
    let agents: Vec<Value> = h.get("/v1/acp-agents").await;
    let kept_agent = agents.iter().find(|a| a["id"] == "versioned").unwrap();
    assert_eq!(kept_agent["capabilities"]["session_new"], true);
    assert_eq!(kept_agent["capabilities"]["thought_level"], true);

    setup["agent_info"]["version"] = json!("2.0");
    stub_acp_agent(dir.path(), setup.clone());
    agent.clear_messages();
    start_discovery(&h, "versioned").await;
    assert!(has(&agent.methods(), "session/new"));
    assert_eq!(h.store.list_acp_catalogs().await.unwrap()[0].version, "2.0");

    setup["config_options"] = json!([option("model-id", "model", "new-model")]);
    stub_acp_agent(dir.path(), setup);
    let _: Vec<Value> = h.json(post("/v1/acp-agents/refresh"), StatusCode::OK).await;
    let models: Vec<Value> = h.get("/v1/models").await;
    assert!(
        models
            .iter()
            .any(|model| model["id"] == "versioned:new-model"),
        "{models:#?}"
    );
}

/// An agent that reports no version gives nothing to keep its catalog under,
/// so every start reads it again; one that cannot close a session is not
/// asked to.
#[tokio::test]
async fn an_agent_without_a_version_is_read_on_every_start() {
    let dir = tempfile::tempdir().unwrap();
    let agent = stub_acp_agent(dir.path(), script());
    let h = harness_with_agent("unversioned", &agent).await;
    assert!(h.store.list_acp_catalogs().await.unwrap().is_empty());

    agent.clear_messages();
    start_discovery(&h, "unversioned").await;
    let methods = agent.methods();
    assert!(has(&methods, "session/new"), "{methods:?}");
    assert!(!has(&methods, "session/close"), "{methods:?}");
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
            "[[acp_agents]]\nid = \"claude-agent-acp\"\ncommand = [{bin:?}]\n\n\
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
    let duplicate = taken("claude-agent-acp", false);
    assert_eq!(duplicate["status"], "rejected", "{duplicate}");
    assert!(
        duplicate["rejection_reason"]
            .as_str()
            .unwrap()
            .contains("already another agent's"),
        "{duplicate}"
    );
    assert_eq!(
        h.launcher.registry.command_of("claude-agent-acp"),
        Some(vec!["claude-agent-acp".to_string()]),
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
