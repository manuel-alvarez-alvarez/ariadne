//! Hermetic tests for the ACP adapter: the planned launch and the launch file
//! it writes into the run dir, with no agent process involved.

use std::path::PathBuf;

use ariadne_core::Seat;
use ariadne_daemon::agents::{SpawnCtx, plan_resume, plan_spawn};

fn ctx_with_flags(run_dir: PathBuf, extra_flags: Vec<String>) -> SpawnCtx {
    SpawnCtx {
        session_id: "01sessionxxxxxxxxxxxxxxxxx".into(),
        launch_id: "01launchxxxxxxxxxxxxxxxxxx".into(),
        goal_id: "01goalxxxxxxxxxxxxxxxxxxxx".into(),
        task_id: Some("01taskxxxxxxxxxxxxxxxxxxxx".into()),
        seat: Seat::Author,
        run_dir,
        cwd: PathBuf::from("/tmp/worktree"),
        socket_path: PathBuf::from("/tmp/ariadne.sock"),
        cli_bin: "/usr/local/bin/ariadne".into(),
        system_prompt: "SYSTEM PROMPT".into(),
        skills_dir: None,
        initial_prompt: "DO THE TASK".into(),
        model: "test-model".into(),
        effort: None,
        extra_flags,
    }
}

/// The context a session pinned to `model` at `effort` assembles: both are
/// frozen on the session row, so every launch of it carries the same pair.
fn ctx_with_pin(run_dir: PathBuf, model: &str, effort: Option<&str>) -> SpawnCtx {
    SpawnCtx {
        model: model.to_string(),
        effort: effort.map(str::to_string),
        ..ctx_with_flags(run_dir, vec!["--extra".into()])
    }
}

/// The launch file as the run dir holds it.
fn launch_file(dir: &std::path::Path) -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(dir.join("acp.json")).unwrap()).unwrap()
}

/// A spawn briefs a new agent session: the system prompt, the briefing as its
/// first prompt, the model and effort pins, and the ariadne MCP server — all
/// in the launch file, which the plan carries too, with the configured flags
/// behind the registry command.
#[test]
fn a_spawn_plans_a_new_session_briefed_and_pinned() {
    let dir = tempfile::tempdir().unwrap();
    let pinned = ctx_with_pin(dir.path().into(), "test-model", Some("xhigh"));
    let plan = plan_spawn(&pinned).unwrap();

    assert_eq!(plan.args, ["--extra"]);
    assert_eq!(plan.cwd, PathBuf::from("/tmp/worktree"));
    assert!(plan.internal_session_id.is_none());
    assert!(plan.config.resume_session_id.is_none());

    let config = launch_file(dir.path());
    assert_eq!(config["systemPrompt"], "SYSTEM PROMPT");
    assert_eq!(config["initialPrompt"], "DO THE TASK");
    assert_eq!(config["model"], "test-model");
    assert_eq!(config["effort"], "xhigh");
    assert_eq!(config["mcpServers"][0]["name"], "ariadne");
    assert_eq!(config["mcpServers"][0]["command"], "/usr/local/bin/ariadne");
    assert_eq!(
        config["mcpServers"][0]["args"],
        serde_json::json!(["mcp", "serve"])
    );
    assert_eq!(
        serde_json::to_value(&plan.config).unwrap(),
        config,
        "the plan carries what the record says"
    );
}

/// A resume names the agent session it continues, and delivers its
/// instruction once as the next prompt; an empty instruction delivers
/// nothing.
#[test]
fn a_resume_names_its_session_and_delivers_its_instruction_once() {
    let dir = tempfile::tempdir().unwrap();
    let pinned = ctx_with_pin(dir.path().into(), "test-model", None);

    let resumed = plan_resume(&pinned, "acp-session-1", "apply feedback").unwrap();
    assert_eq!(resumed.args, ["--extra"]);
    assert_eq!(
        resumed.internal_session_id.as_deref(),
        Some("acp-session-1")
    );
    let config = launch_file(dir.path());
    assert_eq!(config["resumeSessionId"], "acp-session-1");
    assert_eq!(config["initialPrompt"], "apply feedback");
    assert_eq!(
        config["effort"],
        serde_json::Value::Null,
        "no effort pinned"
    );

    plan_resume(&pinned, "acp-session-1", "").unwrap();
    assert_eq!(
        launch_file(dir.path())["initialPrompt"],
        serde_json::Value::Null
    );
}

/// Every launch carries the Ariadne session context, in the agent's own
/// environment and in the MCP server's.
#[test]
fn every_launch_carries_the_session_context() {
    let dir = tempfile::tempdir().unwrap();
    let plan = plan_spawn(&ctx_with_flags(dir.path().into(), vec![])).unwrap();
    let env: std::collections::HashMap<_, _> = plan.env.into_iter().collect();
    assert_eq!(env["ARIADNE_SESSION_ID"], "01sessionxxxxxxxxxxxxxxxxx");
    assert_eq!(env["ARIADNE_LAUNCH_ID"], "01launchxxxxxxxxxxxxxxxxxx");
    assert_eq!(env["ARIADNE_GOAL_ID"], "01goalxxxxxxxxxxxxxxxxxxxx");
    assert_eq!(env["ARIADNE_TASK_ID"], "01taskxxxxxxxxxxxxxxxxxxxx");
    assert_eq!(env["ARIADNE_SEAT"], "author");
    assert_eq!(env["ARIADNE_SOCKET"], "/tmp/ariadne.sock");

    let mcp: Vec<(String, String)> = plan.config.mcp_servers[0]
        .env
        .iter()
        .map(|variable| (variable.name.clone(), variable.value.clone()))
        .collect();
    assert!(mcp.contains(&(
        "ARIADNE_SESSION_ID".into(),
        "01sessionxxxxxxxxxxxxxxxxx".into()
    )));
}

/// The configured flags reach the launch once, spawn and resume alike, and
/// the adapter adds no flag of its own: an agent the user left without flags
/// is launched with its registry command alone.
#[test]
fn the_configured_flags_are_passed_once_and_the_adapter_adds_none() {
    let dir = tempfile::tempdir().unwrap();
    let bare = ctx_with_flags(dir.path().into(), vec![]);
    assert!(plan_spawn(&bare).unwrap().args.is_empty());
    assert!(plan_resume(&bare, "id-1", "go on").unwrap().args.is_empty());

    let configured = ctx_with_flags(dir.path().into(), vec!["--sandbox=off".into()]);
    assert_eq!(plan_spawn(&configured).unwrap().args, ["--sandbox=off"]);
    assert_eq!(
        plan_resume(&configured, "id-1", "go on").unwrap().args,
        ["--sandbox=off"]
    );
}

/// A skill the agent no longer loads is gone from the run dir, not left there
/// for it to read.
#[test]
fn a_dropped_skill_leaves_nothing_behind() {
    let dir = tempfile::tempdir().unwrap();
    let both = [
        ("coding".to_string(), "one".to_string()),
        ("testing".to_string(), "two".to_string()),
    ];
    ariadne_daemon::agents::write_skills(dir.path(), &both).unwrap();
    ariadne_daemon::agents::write_skills(dir.path(), &both[..1]).unwrap();
    assert!(dir.path().join("skills").join("coding").exists());
    assert!(!dir.path().join("skills").join("testing").exists());
}
