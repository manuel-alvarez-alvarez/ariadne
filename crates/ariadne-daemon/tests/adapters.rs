//! Hermetic tests for the three agent adapters: SpawnPlan argv/env and the
//! generated run-dir files, no real CLI involved.

use std::path::PathBuf;

use ariadne_core::{AgentKind, Role};
use ariadne_daemon::agents::{SpawnCtx, adapter_for};

/// The context the launcher assembles for `kind`: the flags are the ones the
/// agent config holds — the kind's defaults plus whatever the user added —
/// never anything the adapter puts there itself.
fn ctx(run_dir: PathBuf, kind: AgentKind) -> SpawnCtx {
    let flags = kind
        .default_flags()
        .iter()
        .map(|f| f.to_string())
        .chain(["--extra".to_string()])
        .collect();
    ctx_with_flags(run_dir, flags)
}

fn ctx_with_flags(run_dir: PathBuf, extra_flags: Vec<String>) -> SpawnCtx {
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
        effort: None,
        extra_flags,
    }
}

/// The context a session pinned to `model` at `effort` assembles: both are
/// frozen on the session row, so every launch of it carries the same pair.
fn ctx_with_pin(run_dir: PathBuf, model: Option<&str>, effort: Option<&str>) -> SpawnCtx {
    SpawnCtx {
        model: model.map(str::to_string),
        effort: effort.map(str::to_string),
        ..ctx_with_flags(run_dir, vec!["--extra".into()])
    }
}

#[test]
fn claude_spawn_plan() {
    const KIND: AgentKind = AgentKind::ClaudeCode;
    let dir = tempfile::tempdir().unwrap();
    let plan = adapter_for(AgentKind::ClaudeCode)
        .plan_spawn(&ctx(dir.path().into(), KIND))
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
    for event in [
        "SessionStart",
        "UserPromptSubmit",
        "PreToolUse",
        "PostToolUse",
        "Notification",
        "Stop",
        "SessionEnd",
    ] {
        let cmd = settings["hooks"][event][0]["hooks"][0]["command"]
            .as_str()
            .unwrap();
        assert!(
            cmd.contains("agent-event --kind claude_code"),
            "{event}: {cmd}"
        );
    }

    // Resume keeps the internal id and passes --resume.
    let plan = adapter_for(AgentKind::ClaudeCode)
        .plan_resume(&ctx(dir.path().into(), KIND), "abc-123", "apply feedback")
        .unwrap();
    assert!(
        plan.argv.contains(&"--resume".to_string()) && plan.argv.contains(&"abc-123".to_string())
    );
    assert!(!plan.argv.contains(&"--session-id".to_string()));
}

#[test]
fn codex_spawn_plan() {
    const KIND: AgentKind = AgentKind::Codex;
    let dir = tempfile::tempdir().unwrap();
    let plan = adapter_for(AgentKind::Codex)
        .plan_spawn(&ctx(dir.path().into(), KIND))
        .unwrap();

    assert_eq!(plan.argv[0], "codex");
    assert!(
        plan.argv
            .contains(&"--dangerously-bypass-approvals-and-sandbox".to_string())
    );
    let joined = plan.argv.join(" ");
    // Events arrive through hooks, not the (turn-only) notify config.
    assert!(!joined.contains("notify"), "{joined}");
    assert_codex_hooks(&plan.argv);
    assert!(joined.contains(r#"mcp_servers.ariadne.command="/usr/local/bin/ariadne""#));
    assert!(joined.contains(r#"ARIADNE_SESSION_ID = "01sessionxxxxxxxxxxxxxxxxx""#));
    // System prompt prepended to the user prompt (no append flag in codex).
    let last = plan.argv.last().unwrap();
    assert!(last.contains("SYSTEM PROMPT") && last.contains("DO THE TASK"));
    assert!(
        plan.internal_session_id.is_none(),
        "the session id arrives with the SessionStart hook"
    );

    // Resume re-passes every config flag (codex does not inherit them).
    let plan = adapter_for(AgentKind::Codex)
        .plan_resume(&ctx(dir.path().into(), KIND), "thread-1", "merge now")
        .unwrap();
    assert_eq!(
        &plan.argv[1..3],
        ["resume".to_string(), "thread-1".to_string()]
    );
    let joined = plan.argv.join(" ");
    assert!(!joined.contains("notify"), "{joined}");
    // Hooks must be re-passed too, and identically: codex trusts the string.
    assert_codex_hooks(&plan.argv);
    assert!(joined.contains("--dangerously-bypass-approvals-and-sandbox"));
    assert!(joined.contains(r#"mcp_servers.ariadne.command="/usr/local/bin/ariadne""#));
    assert!(joined.contains(r#"ARIADNE_SESSION_ID = "01sessionxxxxxxxxxxxxxxxxx""#));
    assert!(plan.argv.contains(&"test-model".to_string()));
    assert!(plan.argv.contains(&"--extra".to_string()));
    assert_eq!(plan.argv.last().unwrap(), "merge now");
}

/// Every hook event declared, spelled exactly as `ariadne setup codex-hooks`
/// spells it — a difference in either half strands sessions at a trust prompt.
fn assert_codex_hooks(argv: &[String]) {
    for flag in ariadne_core::codex_hooks::config_flags("/usr/local/bin/ariadne") {
        assert!(argv.contains(&flag), "missing {flag} in {argv:?}");
    }
    for event in ["SessionStart", "PermissionRequest"] {
        assert!(
            argv.iter()
                .any(|a| a.starts_with(&format!("hooks.{event}="))
                    && a.contains("agent-event --kind codex")),
            "no {event} hook in {argv:?}"
        );
    }
}

#[test]
fn opencode_spawn_plan() {
    const KIND: AgentKind = AgentKind::Opencode;
    let dir = tempfile::tempdir().unwrap();
    let plan = adapter_for(AgentKind::Opencode)
        .plan_spawn(&ctx(dir.path().into(), KIND))
        .unwrap();

    assert_eq!(plan.argv[0], "opencode");
    assert!(
        plan.argv.contains(&"--agent".to_string()) && plan.argv.contains(&"ariadne".to_string())
    );
    // Autonomy rides on the configured flags, like the other two agents.
    assert!(plan.argv.contains(&"--auto".to_string()));
    assert!(plan.argv.contains(&"--extra".to_string()));

    // Nothing to type after a spawn: the briefing rides `--prompt`, which
    // OpenCode honors for a session it creates.
    assert!(plan.post_launch_input.is_none());

    // Config injected via env, not flags.
    let config_path = plan
        .env
        .iter()
        .find(|(k, _)| k == "OPENCODE_CONFIG")
        .map(|(_, v)| v.clone())
        .expect("OPENCODE_CONFIG set");
    let config: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&config_path).unwrap()).unwrap();
    // The denies are the load-bearing part: `--auto` approves whatever still
    // asks, but never what is explicitly denied — and each denied tool hands
    // control to a human.
    assert_eq!(config["permission"]["*"], "allow");
    assert_eq!(config["permission"]["question"], "deny");
    assert_eq!(config["permission"]["plan_enter"], "deny");
    assert_eq!(config["permission"]["plan_exit"], "deny");
    // Every message runs as the ariadne agent, which carries the system
    // prompt: it is the default, and the primaries OpenCode would fall back
    // to (or Tab-cycle onto) are disabled.
    assert_eq!(config["default_agent"], "ariadne");
    assert_eq!(config["agent"]["ariadne"]["prompt"], "SYSTEM PROMPT");
    assert_eq!(config["agent"]["ariadne"]["mode"], "primary");
    assert_eq!(config["agent"]["build"]["disable"], true);
    assert_eq!(config["agent"]["plan"]["disable"], true);
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

    // Resume goes through the TUI with --session (stays attachable). The
    // instruction is NOT on the argv — OpenCode drops --prompt when resuming
    // — it is typed into the TUI by the launcher instead.
    let plan = adapter_for(AgentKind::Opencode)
        .plan_resume(&ctx(dir.path().into(), KIND), "ses_1", "apply feedback")
        .unwrap();
    let joined = plan.argv.join(" ");
    assert!(joined.contains("--session ses_1"));
    assert!(!joined.contains("--prompt"), "{joined}");
    assert_eq!(plan.post_launch_input.as_deref(), Some("apply feedback"));
    assert!(plan.argv.contains(&"--auto".to_string()));

    // An interactive resume (ariadne attach) types nothing.
    let plan = adapter_for(AgentKind::Opencode)
        .plan_resume(&ctx(dir.path().into(), KIND), "ses_1", "")
        .unwrap();
    assert!(plan.post_launch_input.is_none());
}

#[test]
fn base_env_carries_session_context() {
    const KIND: AgentKind = AgentKind::ClaudeCode;
    let dir = tempfile::tempdir().unwrap();
    let plan = adapter_for(AgentKind::ClaudeCode)
        .plan_spawn(&ctx(dir.path().into(), KIND))
        .unwrap();
    let env: std::collections::HashMap<_, _> = plan.env.into_iter().collect();
    assert_eq!(env["ARIADNE_SESSION_ID"], "01sessionxxxxxxxxxxxxxxxxx");
    assert_eq!(env["ARIADNE_GOAL_ID"], "01goalxxxxxxxxxxxxxxxxxxxx");
    assert_eq!(env["ARIADNE_TASK_ID"], "01taskxxxxxxxxxxxxxxxxxxxx");
    assert_eq!(env["ARIADNE_ROLE"], "engineer");
    assert_eq!(env["ARIADNE_SOCKET"], "/tmp/ariadne.sock");
}

/// The permission bypasses live in the agent config, not in the adapters: an
/// agent whose flags the user emptied is launched without one, and whatever
/// the user put there instead is what reaches the argv.
#[test]
fn the_adapters_hardcode_no_bypass_flag() {
    let dir = tempfile::tempdir().unwrap();
    for (kind, bypass) in [
        (AgentKind::ClaudeCode, "--dangerously-skip-permissions"),
        (
            AgentKind::Codex,
            "--dangerously-bypass-approvals-and-sandbox",
        ),
        (AgentKind::Opencode, "--auto"),
    ] {
        let bare = ctx_with_flags(dir.path().into(), vec![]);
        let plan = adapter_for(kind).plan_spawn(&bare).unwrap();
        assert!(
            !plan.argv.contains(&bypass.to_string()),
            "{kind:?}: {plan:?}"
        );
        let plan = adapter_for(kind)
            .plan_resume(&bare, "id-1", "go on")
            .unwrap();
        assert!(
            !plan.argv.contains(&bypass.to_string()),
            "{kind:?}: {plan:?}"
        );

        // What the config does hold is passed on, spawn and resume alike.
        let configured = ctx_with_flags(dir.path().into(), vec!["--sandbox=off".into()]);
        let plan = adapter_for(kind).plan_spawn(&configured).unwrap();
        assert!(plan.argv.contains(&"--sandbox=off".to_string()), "{kind:?}");
        let plan = adapter_for(kind)
            .plan_resume(&configured, "id-1", "go on")
            .unwrap();
        assert!(plan.argv.contains(&"--sandbox=off".to_string()), "{kind:?}");
    }
}

/// Exactly once: the flag comes from the config and from nowhere else, so a
/// claude session cannot end up with it twice over.
#[test]
fn the_configured_flags_are_passed_once() {
    let dir = tempfile::tempdir().unwrap();
    let plan = adapter_for(AgentKind::ClaudeCode)
        .plan_spawn(&ctx(dir.path().into(), AgentKind::ClaudeCode))
        .unwrap();
    let bypasses = plan
        .argv
        .iter()
        .filter(|a| *a == "--dangerously-skip-permissions")
        .count();
    assert_eq!(bypasses, 1, "{:?}", plan.argv);
}

/// Claude takes the effort on its argv, once, right after the model it
/// qualifies — on the session it starts and on a resumed one alike.
#[test]
fn claude_passes_the_effort_after_the_model() {
    let dir = tempfile::tempdir().unwrap();
    let pinned = ctx_with_pin(dir.path().into(), Some("test-model"), Some("xhigh"));
    let adapter = adapter_for(AgentKind::ClaudeCode);
    for plan in [
        adapter.plan_spawn(&pinned).unwrap(),
        adapter
            .plan_resume(&pinned, "abc-123", "apply feedback")
            .unwrap(),
    ] {
        let at = plan
            .argv
            .iter()
            .position(|a| a == "--effort")
            .unwrap_or_else(|| panic!("no --effort in {:?}", plan.argv));
        assert_eq!(
            plan.argv.iter().filter(|a| *a == "--effort").count(),
            1,
            "{:?}",
            plan.argv
        );
        assert_eq!(
            &plan.argv[at - 2..=at + 1],
            ["--model", "test-model", "--effort", "xhigh"]
        );
    }

    // No effort pinned, no flag: the CLI runs the model at its own.
    let bare = ctx_with_pin(dir.path().into(), Some("test-model"), None);
    for plan in [
        adapter.plan_spawn(&bare).unwrap(),
        adapter
            .plan_resume(&bare, "abc-123", "apply feedback")
            .unwrap(),
    ] {
        assert!(
            !plan.argv.contains(&"--effort".to_string()),
            "{:?}",
            plan.argv
        );
    }
}

/// Codex has no effort flag: it takes the config override, and — like every
/// other config flag — it has to be re-passed on resume.
#[test]
fn codex_passes_the_effort_as_a_config_override() {
    let dir = tempfile::tempdir().unwrap();
    let pinned = ctx_with_pin(dir.path().into(), Some("test-model"), Some("xhigh"));
    let adapter = adapter_for(AgentKind::Codex);
    for plan in [
        adapter.plan_spawn(&pinned).unwrap(),
        adapter
            .plan_resume(&pinned, "thread-1", "merge now")
            .unwrap(),
    ] {
        assert!(
            plan.argv
                .windows(2)
                .any(|w| w[0] == "-c" && w[1] == r#"model_reasoning_effort="xhigh""#),
            "{:?}",
            plan.argv
        );
    }

    let bare = ctx_with_pin(dir.path().into(), Some("test-model"), None);
    for plan in [
        adapter.plan_spawn(&bare).unwrap(),
        adapter.plan_resume(&bare, "thread-1", "merge now").unwrap(),
    ] {
        assert!(
            !plan.argv.join(" ").contains("model_reasoning_effort"),
            "{:?}",
            plan.argv
        );
    }
}

/// OpenCode takes the effort as the agent's variant in the generated config,
/// which is what a spawn and a resume both read. It only lands beside a model
/// we write: OpenCode ignores a variant whose model comes from elsewhere.
#[test]
fn opencode_writes_the_effort_as_the_agents_variant() {
    let dir = tempfile::tempdir().unwrap();
    let adapter = adapter_for(AgentKind::Opencode);
    let variant = |plan: &ariadne_daemon::agents::SpawnPlan| {
        let path = plan
            .env
            .iter()
            .find(|(k, _)| k == "OPENCODE_CONFIG")
            .map(|(_, v)| v.clone())
            .expect("OPENCODE_CONFIG set");
        let config: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        config["agent"]["ariadne"]["variant"].clone()
    };

    let pinned = ctx_with_pin(
        dir.path().into(),
        Some("opencode/test-model"),
        Some("xhigh"),
    );
    for plan in [
        adapter.plan_spawn(&pinned).unwrap(),
        adapter
            .plan_resume(&pinned, "ses_1", "apply feedback")
            .unwrap(),
    ] {
        assert_eq!(variant(&plan), "xhigh");
    }

    // No effort pinned: no variant key at all, rather than an empty one.
    let bare = ctx_with_pin(dir.path().into(), Some("opencode/test-model"), None);
    for plan in [
        adapter.plan_spawn(&bare).unwrap(),
        adapter
            .plan_resume(&bare, "ses_1", "apply feedback")
            .unwrap(),
    ] {
        assert_eq!(variant(&plan), serde_json::Value::Null);
    }

    // An effort with no model of the agent's own is dropped, not written: a
    // variant beside a model OpenCode resolves elsewhere is ignored anyway.
    for model in [Some("test-model"), None] {
        let unpinned = ctx_with_pin(dir.path().into(), model, Some("xhigh"));
        let plan = adapter.plan_spawn(&unpinned).unwrap();
        assert_eq!(variant(&plan), serde_json::Value::Null, "{model:?}");
    }
}
