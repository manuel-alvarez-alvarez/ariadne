//! Hermetic tests for the ACP adapter: the planned launch and the launch file
//! it writes into the run dir, with no agent process involved.

use std::path::PathBuf;

use ariadne_core::Seat;
use ariadne_daemon::agents::{SpawnCtx, plan_resume, plan_spawn};

fn ctx_with_flags(run_dir: PathBuf, extra_flags: Vec<String>) -> SpawnCtx {
    SpawnCtx {
        session_id: "01sessionxxxxxxxxxxxxxxxxx".into(),
        launch_id: "01launchxxxxxxxxxxxxxxxxxx".into(),
        goal_id: Some("01goalxxxxxxxxxxxxxxxxxxxx".into()),
        task_id: Some("01taskxxxxxxxxxxxxxxxxxxxx".into()),
        pull_request_id: None,
        seat: Seat::Agent,
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

/// A pull request session works for no goal and no task (026): its MCP
/// server is told the request it watches, beside the seat, and nothing of a
/// goal or a task.
#[test]
fn a_pull_request_session_tells_its_mcp_server_the_request_and_no_goal() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = SpawnCtx {
        goal_id: None,
        task_id: None,
        pull_request_id: Some("01prxxxxxxxxxxxxxxxxxxxxxx".into()),
        ..ctx_with_flags(dir.path().into(), Vec::new())
    };
    plan_spawn(&ctx).unwrap();
    let config = launch_file(dir.path());
    let env: Vec<(String, String)> = config["mcpServers"][0]["env"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| {
            (
                v["name"].as_str().unwrap().to_string(),
                v["value"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    let named = |name: &str| env.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str());
    assert_eq!(
        named("ARIADNE_PULL_REQUEST_ID"),
        Some("01prxxxxxxxxxxxxxxxxxxxxxx")
    );
    assert_eq!(named("ARIADNE_SEAT"), Some("agent"));
    assert_eq!(named("ARIADNE_GOAL_ID"), None);
    assert_eq!(named("ARIADNE_TASK_ID"), None);
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
    assert_eq!(env["ARIADNE_SEAT"], "agent");
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

/// A launch links the run dir's `skills` folder onto the shared home copy,
/// so a document read through it is the text the home copy holds. macOS and
/// Linux always get a symlink; the supported outcome on Windows (a symlink,
/// or the copy fallback) is proven separately, since the two cannot both
/// hold in one process.
#[test]
fn a_launch_links_the_run_dirs_skills_folder_to_the_home_copy() {
    let dir = tempfile::tempdir().unwrap();
    let skills_dir = dir.path().join("home-skills");
    std::fs::create_dir_all(skills_dir.join("coding")).unwrap();
    std::fs::write(
        skills_dir.join("coding").join("SKILL.md"),
        "the coding skill",
    )
    .unwrap();

    let run_dir = dir.path().join("run");
    let root = ariadne_daemon::agents::link_skills(&run_dir, &skills_dir).unwrap();

    assert_eq!(root, run_dir.join("skills"));
    #[cfg(unix)]
    {
        assert!(
            std::fs::symlink_metadata(&root).unwrap().is_symlink(),
            "on macOS and Linux the run dir's skills folder is always a symlink"
        );
        assert_eq!(
            std::fs::read_link(&root).unwrap(),
            std::fs::canonicalize(&skills_dir).unwrap(),
            "the link's target is skills_dir resolved absolute"
        );
    }
    assert_eq!(
        std::fs::read_to_string(root.join("coding").join("SKILL.md")).unwrap(),
        "the coding skill",
        "a document read through the link is the text under skills_dir"
    );
}

/// A relative `skills_dir`, as `config.toml` may give it, still links to its
/// real target: a symlink's relative target is read against the link's own
/// directory, so writing the configured value in unresolved would point the
/// run dir's `skills` folder at itself rather than at the home copy.
#[test]
fn a_relative_skills_dir_still_links_to_its_real_target() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir_all(home.join("shared-skills").join("coding")).unwrap();
    std::fs::write(
        home.join("shared-skills").join("coding").join("SKILL.md"),
        "the coding skill",
    )
    .unwrap();
    // Nextest runs every test in its own process, so changing the working
    // directory here reaches no other test.
    std::env::set_current_dir(&home).unwrap();

    let run_dir = dir.path().join("elsewhere").join("run");
    let root = ariadne_daemon::agents::link_skills(&run_dir, std::path::Path::new("shared-skills"))
        .unwrap();

    assert_eq!(
        std::fs::read_to_string(root.join("coding").join("SKILL.md")).unwrap(),
        "the coding skill",
        "a relative skills_dir still resolves to its real target"
    );
}

/// A second launch of the same session relinks rather than erroring on the
/// entry the first launch left, and never reaches through a stale link to
/// touch the home copy it pointed at.
#[test]
fn a_second_launch_replaces_the_link_and_leaves_the_home_copy_whole() {
    let dir = tempfile::tempdir().unwrap();
    let first_skills = dir.path().join("first-skills");
    let second_skills = dir.path().join("second-skills");
    for (skills, text) in [(&first_skills, "first"), (&second_skills, "second")] {
        std::fs::create_dir_all(skills.join("coding")).unwrap();
        std::fs::write(skills.join("coding").join("SKILL.md"), text).unwrap();
    }

    let run_dir = dir.path().join("run");
    ariadne_daemon::agents::link_skills(&run_dir, &first_skills).unwrap();
    let root = ariadne_daemon::agents::link_skills(&run_dir, &second_skills).unwrap();

    #[cfg(unix)]
    assert_eq!(
        std::fs::read_link(&root).unwrap(),
        std::fs::canonicalize(&second_skills).unwrap(),
        "the link now points at the second launch's target"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("coding").join("SKILL.md")).unwrap(),
        "second",
        "the second launch's target replaced the first's, on every OS"
    );
    assert_eq!(
        std::fs::read_to_string(first_skills.join("coding").join("SKILL.md")).unwrap(),
        "first",
        "the home copy the first launch pointed at is untouched"
    );
}

/// A stale regular file at the run dir's `skills` path is cleared before
/// relinking.
#[test]
fn a_stale_file_at_the_skills_path_is_cleared_before_relinking() {
    let dir = tempfile::tempdir().unwrap();
    let skills_dir = dir.path().join("home-skills");
    std::fs::create_dir_all(skills_dir.join("coding")).unwrap();
    std::fs::write(
        skills_dir.join("coding").join("SKILL.md"),
        "the coding skill",
    )
    .unwrap();

    let run_dir = dir.path().join("run");
    std::fs::create_dir_all(&run_dir).unwrap();
    std::fs::write(run_dir.join("skills"), "stale").unwrap();

    let root = ariadne_daemon::agents::link_skills(&run_dir, &skills_dir).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("coding").join("SKILL.md")).unwrap(),
        "the coding skill"
    );
}

/// A stale real directory at the run dir's `skills` path — what the Windows
/// copy fallback leaves behind — is cleared before relinking too.
#[test]
fn a_stale_directory_at_the_skills_path_is_cleared_before_relinking() {
    let dir = tempfile::tempdir().unwrap();
    let skills_dir = dir.path().join("home-skills");
    std::fs::create_dir_all(skills_dir.join("coding")).unwrap();
    std::fs::write(
        skills_dir.join("coding").join("SKILL.md"),
        "the coding skill",
    )
    .unwrap();

    let run_dir = dir.path().join("run");
    std::fs::create_dir_all(run_dir.join("skills").join("leftover")).unwrap();
    std::fs::write(
        run_dir.join("skills").join("leftover").join("SKILL.md"),
        "stale",
    )
    .unwrap();

    let root = ariadne_daemon::agents::link_skills(&run_dir, &skills_dir).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("coding").join("SKILL.md")).unwrap(),
        "the coding skill"
    );
}

/// A dangling symlink at the run dir's `skills` path — its target gone — is
/// removed as a link rather than failing the relink.
#[cfg(unix)]
#[test]
fn a_dangling_symlink_at_the_skills_path_is_cleared_before_relinking() {
    let dir = tempfile::tempdir().unwrap();
    let skills_dir = dir.path().join("home-skills");
    std::fs::create_dir_all(skills_dir.join("coding")).unwrap();
    std::fs::write(
        skills_dir.join("coding").join("SKILL.md"),
        "the coding skill",
    )
    .unwrap();

    let run_dir = dir.path().join("run");
    std::fs::create_dir_all(&run_dir).unwrap();
    std::os::unix::fs::symlink(dir.path().join("nowhere"), run_dir.join("skills")).unwrap();

    let root = ariadne_daemon::agents::link_skills(&run_dir, &skills_dir).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("coding").join("SKILL.md")).unwrap(),
        "the coding skill"
    );
}

/// The copy fallback, exercised directly on every OS: every skill's
/// `SKILL.md` under the home copy reaches the run dir as a real file.
#[test]
fn the_copy_fallback_copies_every_skills_md_into_the_run_dir() {
    let dir = tempfile::tempdir().unwrap();
    let skills_dir = dir.path().join("home-skills");
    std::fs::create_dir_all(skills_dir.join("coding")).unwrap();
    std::fs::write(skills_dir.join("coding").join("SKILL.md"), "one").unwrap();
    std::fs::create_dir_all(skills_dir.join("testing")).unwrap();
    std::fs::write(skills_dir.join("testing").join("SKILL.md"), "two").unwrap();

    let dest = dir.path().join("run").join("skills");
    ariadne_daemon::agents::copy_skills_dir(&skills_dir, &dest).unwrap();

    assert_eq!(
        std::fs::read_to_string(dest.join("coding").join("SKILL.md")).unwrap(),
        "one"
    );
    assert_eq!(
        std::fs::read_to_string(dest.join("testing").join("SKILL.md")).unwrap(),
        "two"
    );
}

/// The copy fallback copies the whole tree, not just each `SKILL.md`: a
/// script a document references beside it reaches the run dir too.
#[test]
fn the_copy_fallback_copies_nested_files_a_skill_document_references() {
    let dir = tempfile::tempdir().unwrap();
    let skills_dir = dir.path().join("home-skills");
    std::fs::create_dir_all(skills_dir.join("coding").join("scripts")).unwrap();
    std::fs::write(
        skills_dir.join("coding").join("SKILL.md"),
        "the coding skill",
    )
    .unwrap();
    std::fs::write(
        skills_dir.join("coding").join("scripts").join("helper.py"),
        "print('help')",
    )
    .unwrap();

    let dest = dir.path().join("run").join("skills");
    ariadne_daemon::agents::copy_skills_dir(&skills_dir, &dest).unwrap();

    assert_eq!(
        std::fs::read_to_string(dest.join("coding").join("scripts").join("helper.py")).unwrap(),
        "print('help')",
        "a nested file the document references reaches the run dir too"
    );
}
