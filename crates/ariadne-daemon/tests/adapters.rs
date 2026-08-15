//! Hermetic tests for the three agent adapters: SpawnPlan argv/env and the
//! generated run-dir files, no real CLI involved.

use std::path::PathBuf;

use ariadne_core::{AgentKind, Role};
use ariadne_daemon::agents::{SpawnCtx, adapter_for};

fn ctx(run_dir: PathBuf) -> SpawnCtx {
    SpawnCtx {
        session_id: "01sessionxxxxxxxxxxxxxxxxx".into(),
        goal_id: "01goalxxxxxxxxxxxxxxxxxxxx".into(),
        task_id: Some("01taskxxxxxxxxxxxxxxxxxxxx".into()),
        role: Role::Engineer,
        run_dir,
        cwd: PathBuf::from("/tmp/worktree"),
        socket_path: PathBuf::from("/tmp/ariadne.sock"),
        cli_bin: "/usr/local/bin/ariadne".into(),
        system_prompt: "SYSTEM PROMPT".into(),
        initial_prompt: "DO THE TASK".into(),
        model: Some("test-model".into()),
        extra_flags: vec!["--extra".into()],
    }
}

#[test]
fn claude_spawn_plan() {
    let dir = tempfile::tempdir().unwrap();
    let plan = adapter_for(AgentKind::ClaudeCode)
        .plan_spawn(&ctx(dir.path().into()))
        .unwrap();

    assert_eq!(plan.argv[0], "claude");
    assert!(
        plan.argv
            .contains(&"--dangerously-skip-permissions".to_string())
    );
    assert!(plan.argv.contains(&"--session-id".to_string()));
    assert!(
        plan.argv.contains(&"--model".to_string()) && plan.argv.contains(&"test-model".to_string())
    );
    assert!(plan.argv.contains(&"--extra".to_string()));
    assert_eq!(plan.argv.last().unwrap(), "DO THE TASK");
    // We chose the uuid up front.
    let uuid = plan
        .internal_session_id
        .expect("claude session id chosen at spawn");
    assert_eq!(uuid.len(), 36);

    // Generated files.
    let prompt = std::fs::read_to_string(dir.path().join("system-prompt.md")).unwrap();
    assert_eq!(prompt, "SYSTEM PROMPT");
    let mcp: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.path().join("mcp.json")).unwrap())
            .unwrap();
    assert_eq!(
        mcp["mcpServers"]["ariadne"]["command"],
        "/usr/local/bin/ariadne"
    );
    assert_eq!(
        mcp["mcpServers"]["ariadne"]["env"]["ARIADNE_SESSION_ID"],
        "01sessionxxxxxxxxxxxxxxxxx"
    );
    let settings: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.path().join("settings.json")).unwrap())
            .unwrap();
    for event in ["SessionStart", "PostToolUse", "Stop", "SessionEnd"] {
        let cmd = settings["hooks"][event][0]["hooks"][0]["command"]
            .as_str()
            .unwrap();
        assert!(cmd.contains("agent-event --kind claude"), "{event}: {cmd}");
    }

    // Resume keeps the internal id and passes --resume.
    let plan = adapter_for(AgentKind::ClaudeCode)
        .plan_resume(&ctx(dir.path().into()), "abc-123", "apply feedback")
        .unwrap();
    assert!(
        plan.argv.contains(&"--resume".to_string()) && plan.argv.contains(&"abc-123".to_string())
    );
    assert!(!plan.argv.contains(&"--session-id".to_string()));
}

#[test]
fn codex_spawn_plan() {
    let dir = tempfile::tempdir().unwrap();
    let plan = adapter_for(AgentKind::Codex)
        .plan_spawn(&ctx(dir.path().into()))
        .unwrap();

    assert_eq!(plan.argv[0], "codex");
    assert!(
        plan.argv
            .contains(&"--dangerously-bypass-approvals-and-sandbox".to_string())
    );
    let joined = plan.argv.join(" ");
    assert!(joined.contains(
        r#"notify=["/usr/local/bin/ariadne","agent-event","--kind","codex","--argv-json"]"#
    ));
    assert!(joined.contains(r#"mcp_servers.ariadne.command="/usr/local/bin/ariadne""#));
    assert!(joined.contains(r#"ARIADNE_SESSION_ID = "01sessionxxxxxxxxxxxxxxxxx""#));
    // System prompt prepended to the user prompt (no append flag in codex).
    let last = plan.argv.last().unwrap();
    assert!(last.contains("SYSTEM PROMPT") && last.contains("DO THE TASK"));
    assert!(
        plan.internal_session_id.is_none(),
        "thread id arrives via notify"
    );

    // Resume re-passes the bypass flag (codex does not inherit it).
    let plan = adapter_for(AgentKind::Codex)
        .plan_resume(&ctx(dir.path().into()), "thread-1", "merge now")
        .unwrap();
    assert_eq!(
        &plan.argv[1..3],
        ["resume".to_string(), "thread-1".to_string()]
    );
    assert!(
        plan.argv
            .contains(&"--dangerously-bypass-approvals-and-sandbox".to_string())
    );
}

#[test]
fn opencode_spawn_plan() {
    let dir = tempfile::tempdir().unwrap();
    let plan = adapter_for(AgentKind::Opencode)
        .plan_spawn(&ctx(dir.path().into()))
        .unwrap();

    assert_eq!(plan.argv[0], "opencode");
    assert!(
        plan.argv.contains(&"--agent".to_string()) && plan.argv.contains(&"ariadne".to_string())
    );

    // Config injected via env, not flags.
    let config_path = plan
        .env
        .iter()
        .find(|(k, _)| k == "OPENCODE_CONFIG")
        .map(|(_, v)| v.clone())
        .expect("OPENCODE_CONFIG set");
    let config: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&config_path).unwrap()).unwrap();
    assert_eq!(config["permission"]["*"], "allow");
    assert_eq!(config["agent"]["ariadne"]["prompt"], "SYSTEM PROMPT");
    // Model without provider prefix is skipped (opencode wants provider/model).
    assert!(config["agent"]["ariadne"].get("model").is_none());
    assert_eq!(
        config["mcp"]["ariadne"]["command"][0],
        "/usr/local/bin/ariadne"
    );
    assert!(
        config["plugin"][0]
            .as_str()
            .unwrap()
            .contains("ariadne-events.js")
    );

    // Resume goes through the TUI with --session (stays attachable).
    let plan = adapter_for(AgentKind::Opencode)
        .plan_resume(&ctx(dir.path().into()), "ses_1", "apply feedback")
        .unwrap();
    let joined = plan.argv.join(" ");
    assert!(joined.contains("--session ses_1") && joined.contains("--prompt apply feedback"));
}

#[test]
fn base_env_carries_session_context() {
    let dir = tempfile::tempdir().unwrap();
    let plan = adapter_for(AgentKind::ClaudeCode)
        .plan_spawn(&ctx(dir.path().into()))
        .unwrap();
    let env: std::collections::HashMap<_, _> = plan.env.into_iter().collect();
    assert_eq!(env["ARIADNE_SESSION_ID"], "01sessionxxxxxxxxxxxxxxxxx");
    assert_eq!(env["ARIADNE_GOAL_ID"], "01goalxxxxxxxxxxxxxxxxxxxx");
    assert_eq!(env["ARIADNE_TASK_ID"], "01taskxxxxxxxxxxxxxxxxxxxx");
    assert_eq!(env["ARIADNE_ROLE"], "engineer");
    assert_eq!(env["ARIADNE_SOCKET"], "/tmp/ariadne.sock");
}
