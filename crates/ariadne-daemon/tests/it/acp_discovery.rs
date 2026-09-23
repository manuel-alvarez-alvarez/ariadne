//! Integration tests for ACP agent discovery and its cached API catalog.

use crate::common;

use std::time::Duration;

use ariadne_daemon::timeouts::Timeouts;

use ariadne_store::AgentPin;

use axum::http::StatusCode;
use serde_json::{Value, json};

use common::acp::{option, script, stub_acp_agent};
use common::{Harness, HarnessBuilder, TIMEOUT, eventually, harness, post, post_json};

fn home_with_agent(id: &str, bin: &str) -> std::path::PathBuf {
    let home = empty_home(std::path::Path::new(bin).parent().unwrap());
    std::fs::write(
        home.join("config.toml"),
        format!("[[acp_agents]]\nid = {id:?}\ncommand = [{bin:?}]\n"),
    )
    .unwrap();
    home
}

/// A home whose config registers no agent of its own: the registry is
/// whatever the `PATH` holds.
fn empty_home(dir: &std::path::Path) -> std::path::PathBuf {
    let home = dir.join("home");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(home.join("config.toml"), "").unwrap();
    home
}

async fn harness_with_agent(id: &str, agent: &common::acp::StubAcpAgent) -> Harness {
    settled(harness().home(home_with_agent(id, &agent.bin)), id).await
}

/// A daemon that ran discovery, probing again until `id` answers for itself
/// — accepted, or rejected for a reason of its own. A probe under full-suite
/// load can run out its timeout, and no test reads a timed-out snapshot.
async fn settled(builder: HarnessBuilder, id: &str) -> Harness {
    let h = builder.discover_agents().await;
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

/// The registry is the agents of the shipped index the daemon's `PATH`
/// holds, and the configured agents after them. A `binary` entry is run by
/// the command the index gives it — `goose` with `acp` behind it — and every
/// entry says where it came from.
#[tokio::test]
async fn the_api_lists_an_installed_index_agent_and_one_user_agent() {
    let dir = tempfile::tempdir().unwrap();
    let mut setup = script();
    setup["capabilities"]["sessionCapabilities"]["list"] = json!({});
    let agent = stub_acp_agent(dir.path(), setup);
    let path = agent.path_with(&["goose"]);
    let h = settled(
        harness()
            .home(home_with_agent("test-agent", &agent.bin))
            .agents_on_path(path),
        "goose",
    )
    .await;

    let agents: Vec<Value> = h.get("/v1/acp-agents").await;
    let ids: Vec<&str> = agents.iter().filter_map(|a| a["id"].as_str()).collect();
    assert_eq!(ids, ["goose", "test-agent"], "{agents:#?}");
    let goose = &agents[0];
    assert_eq!(goose["command"], json!(["goose", "acp"]), "{goose}");
    assert_eq!(goose["source"], "registry", "{goose}");
    assert_eq!(goose["status"], "ready", "{goose}");
    let custom = &agents[1];
    assert_eq!(custom["source"], "config", "{custom}");
    assert_eq!(custom["status"], "ready");
    assert_eq!(custom["capabilities"]["session_list"], true);
    assert_eq!(custom["capabilities"]["session_load"], true);
    assert_eq!(custom["degraded"], json!([]));

    let doc: Value = h.get("/api-docs/openapi.json").await;
    assert!(doc["paths"]["/v1/acp-agents"]["get"].is_object());
    assert!(doc["paths"]["/v1/acp-agents/refresh"]["post"].is_object());
    assert!(doc["components"]["schemas"]["AcpAgentDto"].is_object());
}

/// Ariadne installs nothing: an agent of the index that no name of it finds
/// on the `PATH` is no agent of this daemon, and a `PATH` holding none of
/// them leaves a daemon with only the agents its config names — here, none.
#[tokio::test]
async fn an_empty_path_registers_no_agent() {
    let dir = tempfile::tempdir().unwrap();
    let h = harness()
        .home(empty_home(dir.path()))
        .discover_agents()
        .await;

    let agents: Vec<Value> = h.get("/v1/acp-agents").await;
    assert_eq!(agents, Vec::<Value>::new(), "{agents:#?}");
    let models: Vec<Value> = h.get("/v1/models").await;
    assert_eq!(models, Vec::<Value>::new(), "{models:#?}");
}

/// A relative `PATH` entry names a directory of whoever started the daemon,
/// and an empty entry — the whole of an empty `PATH` — names that directory
/// too. An agent is not taken from there, however plainly the name is there
/// to be found: the probe runs in a directory of its own, and would look for
/// that file somewhere else again.
#[tokio::test]
async fn a_relative_path_entry_registers_no_agent() {
    let dir = tempfile::tempdir().unwrap();
    let agent = stub_acp_agent(dir.path(), script());
    let installed = agent.path_with(&["goose"]);
    let relative = from_this_directory(std::path::Path::new(&installed));
    assert!(
        ariadne_core::probe::is_executable(&std::path::Path::new(&relative).join("goose")),
        "the relative entry names the stub: {relative:?}"
    );

    let h = harness()
        .home(empty_home(dir.path()))
        .agents_on_path(relative)
        .discover_agents()
        .await;

    let agents: Vec<Value> = h.get("/v1/acp-agents").await;
    assert_eq!(agents, Vec::<Value>::new(), "{agents:#?}");
}

/// Only the entry is dropped, never the search: an agent named by a relative
/// entry and by an absolute one behind it is taken from the absolute one, as
/// a `PATH` search takes the first directory that holds what it looks for.
#[tokio::test]
async fn an_absolute_entry_behind_a_relative_one_is_still_searched() {
    let dir = tempfile::tempdir().unwrap();
    let agent = stub_acp_agent(dir.path(), script());
    let installed = agent.path_with(&["goose"]);
    // The same `goose`, under a relative entry and an absolute one.
    let relative = from_this_directory(std::path::Path::new(&installed));
    let path = std::env::join_paths([relative, installed]).unwrap();
    let h = settled(
        harness().home(empty_home(dir.path())).agents_on_path(path),
        "goose",
    )
    .await;

    let agents: Vec<Value> = h.get("/v1/acp-agents").await;
    let ids: Vec<&str> = agents.iter().filter_map(|a| a["id"].as_str()).collect();
    assert_eq!(ids, ["goose"], "{agents:#?}");
    assert_eq!(agents[0]["command"], json!(["goose", "acp"]));
    assert_eq!(agents[0]["status"], "ready", "{:#?}", agents[0]);
}

/// `dir`, written from the directory this test runs in: the same directory,
/// named relatively.
fn from_this_directory(dir: &std::path::Path) -> std::ffi::OsString {
    let here = std::env::current_dir().unwrap();
    // One step up for every component below the root, then down again.
    let mut path: std::path::PathBuf = here.components().skip(1).map(|_| "..").collect();
    path.push(dir.strip_prefix("/").unwrap());
    path.into_os_string()
}

/// An `npx` entry is installed under the name of its package as often as
/// under its own id, so both are tried. `claude-acp` is the package
/// `@agentclientprotocol/claude-agent-acp`, whose command is
/// `claude-agent-acp`, and the agent takes the registry id all the same.
#[tokio::test]
async fn an_npx_agent_is_found_under_the_name_of_its_package() {
    let dir = tempfile::tempdir().unwrap();
    let agent = stub_acp_agent(dir.path(), script());
    let path = agent.path_with(&["claude-agent-acp"]);
    let h = settled(
        harness().home(empty_home(dir.path())).agents_on_path(path),
        "claude-acp",
    )
    .await;

    let agents: Vec<Value> = h.get("/v1/acp-agents").await;
    let ids: Vec<&str> = agents.iter().filter_map(|a| a["id"].as_str()).collect();
    assert_eq!(ids, ["claude-acp"], "{agents:#?}");
    assert_eq!(agents[0]["command"], json!(["claude-agent-acp"]));
    assert_eq!(agents[0]["status"], "ready", "{:#?}", agents[0]);
    assert_eq!(
        h.launcher.registry.command_of("claude-acp"),
        Some(vec!["claude-agent-acp".to_string()])
    );
}

/// The index decides the command: a package entry is looked up under the
/// entry id first and under the package name after it — without its
/// `@scope/` and without the version it is pinned to — and the
/// distribution's arguments follow whichever name answered.
#[tokio::test]
async fn the_index_maps_an_entry_to_the_first_name_the_path_holds() {
    let dir = tempfile::tempdir().unwrap();
    let agent = stub_acp_agent(dir.path(), script());
    let path = agent.path_with(&["packaged-cli", "under-its-id", "id-cli"]);
    let index = json!({
        "version": "1.0.0",
        "agents": [
            {
                "id": "packaged",
                "distribution": {"uvx": {"package": "packaged-cli==0.1.44", "args": ["acp"]}}
            },
            {
                "id": "under-its-id",
                "distribution": {"npx": {"package": "@scope/id-cli@2.0.0", "args": ["--acp"]}}
            },
            {
                "id": "absent",
                "distribution": {"npx": {"package": "absent-cli@1.0.0", "args": ["--acp"]}}
            }
        ]
    });
    let h = settled(
        harness()
            .home(empty_home(dir.path()))
            .agents_on_path(path)
            .acp_index(index.to_string()),
        "packaged",
    )
    .await;

    let agents: Vec<Value> = h.get("/v1/acp-agents").await;
    let ids: Vec<&str> = agents.iter().filter_map(|a| a["id"].as_str()).collect();
    assert_eq!(ids, ["packaged", "under-its-id"], "{agents:#?}");
    assert_eq!(agents[0]["command"], json!(["packaged-cli", "acp"]));
    assert_eq!(agents[1]["command"], json!(["under-its-id", "--acp"]));
}

/// The id rule is the id's, not the config's: an index entry whose id could
/// never be pinned — it carries the catalog delimiter, or it is empty — is
/// refused where it stands, as a configured entry of that id is. It is
/// listed with the reason, it is never started, and it resolves to nothing.
#[tokio::test]
async fn an_index_id_that_cannot_be_pinned_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let agent = stub_acp_agent(dir.path(), script());
    let path = agent.path_with(&["odd-agent", "nameless-agent"]);
    let index = json!({
        "version": "1.0.0",
        "agents": [
            {"id": "my:agent", "distribution": {"npx": {"package": "odd-agent@1.0.0"}}},
            {"id": "", "distribution": {"npx": {"package": "nameless-agent@1.0.0"}}}
        ]
    });
    let h = settled(
        harness()
            .home(empty_home(dir.path()))
            .agents_on_path(path)
            .acp_index(index.to_string()),
        "my:agent",
    )
    .await;

    let agents: Vec<Value> = h.get("/v1/acp-agents").await;
    let entry = |id: &str| {
        agents
            .iter()
            .find(|a| a["id"] == id)
            .unwrap_or_else(|| panic!("no {id:?} in {agents:#?}"))
    };
    let delimiter = entry("my:agent");
    assert_eq!(delimiter["status"], "rejected", "{delimiter}");
    assert!(
        delimiter["rejection_reason"]
            .as_str()
            .unwrap()
            .contains("contains `:`"),
        "{delimiter}"
    );
    let nameless = entry("");
    assert_eq!(nameless["status"], "rejected", "{nameless}");
    assert!(
        nameless["rejection_reason"]
            .as_str()
            .unwrap()
            .contains("the id is empty"),
        "{nameless}"
    );

    assert!(h.launcher.registry.command_of("my:agent").is_none());
    assert!(h.launcher.registry.command_of("").is_none());
    assert_eq!(
        agent.methods(),
        Vec::<String>::new(),
        "a refused entry is never started"
    );
}

/// A configured entry takes over the id it names: the agent of that id is
/// the user's command, listed once and sourced from the config, and that is
/// the command a launch of it runs.
#[tokio::test]
async fn a_configured_agent_replaces_the_discovered_agent_of_its_id() {
    let dir = tempfile::tempdir().unwrap();
    let agent = stub_acp_agent(dir.path(), script());
    let path = agent.path_with(&["goose"]);
    let home = std::path::Path::new(&agent.bin)
        .parent()
        .unwrap()
        .join("home");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(
        home.join("config.toml"),
        format!(
            "[[acp_agents]]\nid = \"goose\"\ncommand = [{bin:?}, \"--configured\"]\n",
            bin = agent.bin
        ),
    )
    .unwrap();
    let h = settled(harness().home(home).agents_on_path(path), "goose").await;

    let agents: Vec<Value> = h.get("/v1/acp-agents").await;
    let ids: Vec<&str> = agents.iter().filter_map(|a| a["id"].as_str()).collect();
    assert_eq!(ids, ["goose"], "{agents:#?}");
    assert_eq!(agents[0]["source"], "config", "{:#?}", agents[0]);
    assert_eq!(agents[0]["command"], json!([agent.bin, "--configured"]));

    // What a seat pinned to that id is started with is the configured
    // command, arguments and all.
    let repo = h.repository(&h.git_repo("repo")).await;
    let goal = h
        .goal_on(
            &repo,
            AgentPin {
                model: "goose:old-model".into(),
                effort: None,
            },
        )
        .await;
    let session = h.launcher.spawn_orchestrator(&goal.id).await.unwrap();
    eventually(TIMEOUT, "the orchestrator to start", || async {
        !agent.launches_for(&session.id).is_empty()
    })
    .await;
    assert_eq!(
        agent.launches_for(&session.id),
        vec![vec!["--configured".to_string()]]
    );
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
/// already shown, not reported as an agent that showed nothing. The probe's
/// time is shortened from a daemon's 5 s to what an agent under load still
/// comes up in; a probe that does not even get that far is simply retried.
#[tokio::test]
async fn a_timed_out_probe_keeps_what_it_measured() {
    let dir = tempfile::tempdir().unwrap();
    let mut setup = script();
    setup["silent_methods"] = json!(["session/new"]);
    let agent = stub_acp_agent(dir.path(), setup);
    let h = harness()
        .home(home_with_agent("silent", &agent.bin))
        .discover_agents()
        .timeouts(Timeouts {
            probe: Duration::from_secs(1),
            ..Timeouts::default()
        })
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

/// The first holder of a configured id keeps it. Every later entry with the
/// same id is rejected with the reason on it, and nothing resolves its
/// command: `command_of` answers with the first holder's.
#[tokio::test]
async fn a_registry_id_already_taken_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let agent = stub_acp_agent(dir.path(), script());
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(
        home.join("config.toml"),
        format!(
            "[[acp_agents]]\nid = \"twin\"\ncommand = [{bin:?}]\n\n\
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

    // Between two configured entries, the first keeps the id and the
    // second is rejected.
    let agents: Vec<Value> = h.get("/v1/acp-agents").await;
    let twins: Vec<&Value> = agents.iter().filter(|a| a["id"] == "twin").collect();
    assert_eq!(twins.len(), 2, "{agents:#?}");
    assert_eq!(twins[0]["status"], "ready", "{:#?}", twins[0]);
    assert_eq!(twins[1]["status"], "rejected", "{:#?}", twins[1]);
    assert!(
        twins[1]["rejection_reason"]
            .as_str()
            .unwrap()
            .contains("already another agent's"),
        "{:#?}",
        twins[1]
    );
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
