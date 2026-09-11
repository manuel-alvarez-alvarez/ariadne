//! The daemon drives every agent itself: a child process on piped stdio,
//! spoken to over ACP version 1.
//!
//! The agent is the scriptable stub from test support (`common::acp`),
//! registered as the registry agent `stub`: every JSON-RPC message the daemon
//! sends lands in its log, which is where the handshake, the pins and the
//! prompt are asserted. `git` is real — spawning an author creates its
//! worktree.

mod common;

use serde_json::{Value, json};

use ariadne_core::SessionStatus;
use ariadne_store::{AgentPin, EventFilter};

use common::acp::{
    StubAcpAgent, discovery_settled, pid_is_alive, registry_home, script, stub_acp_agent,
};
use common::{Cast, Harness, TIMEOUT, eventually, harness, sh};

/// A task whose author runs on the registry agent `stub`, in a real repo,
/// with an effort pinned so the launch has one to set.
async fn acp_cast(h: &Harness) -> Cast {
    let cast = registry_cast(h).await;
    h.store
        .set_agent_pin(&cast.author.id, &registry_pin(Some("high")))
        .await
        .unwrap();
    cast
}

/// A harness whose registry knows the stub as the agent `stub`: the home
/// carries an `[[acp_agents]]` entry for it, and discovery has probed it.
/// The probe's own traffic is dropped from the stub's log, so a test reads
/// only what the daemon sent its agents.
async fn registry_harness(stub: &StubAcpAgent) -> Harness {
    let h = harness().home(registry_home(stub)).discover_agents().await;
    discovery_settled(&h, stub).await;
    h
}

/// The same, with a real scheduler behind the router.
async fn scheduled_registry_harness(stub: &StubAcpAgent) -> Harness {
    let h = harness()
        .home(registry_home(stub))
        .scheduler()
        .discover_agents()
        .await;
    discovery_settled(&h, stub).await;
    h
}

/// The pin a registry-launched seat carries: the catalog id of a discovered
/// model, `<agent-id>:<model>`.
fn registry_pin(effort: Option<&str>) -> AgentPin {
    AgentPin {
        model: "stub:test-model".into(),
        effort: effort.map(str::to_string),
    }
}

/// A task whose agents run on the registry agent `stub`, in a real repo.
async fn registry_cast(h: &Harness) -> Cast {
    h.git_repo("repo");
    h.cast_pinned("stub:test-model", 1).await
}

/// The kinds of every event this session put in the store, in order.
async fn event_kinds(h: &Harness, session_id: &str) -> Vec<String> {
    h.store
        .list_events(EventFilter {
            session_id: Some(session_id.to_string()),
            ..Default::default()
        })
        .await
        .unwrap()
        .into_iter()
        .map(|event| event.kind)
        .collect()
}

/// Spawn the task's author against the stub and wait for the turn to end.
async fn spawned_idle(h: &Harness, cast: &Cast) -> ariadne_store::AgentSession {
    let session = h.launcher.spawn_author(&cast.task.id).await.unwrap();
    eventually(TIMEOUT, "the prompt round trip to end", || async {
        h.session_status(&session).await == SessionStatus::Idle
    })
    .await;
    session
}

/// Launch, handshake, model and effort pins, the briefing as the first
/// prompt, the events in the store, the captured agent session id — and the
/// agent a child of the daemon, on its own stdio.
#[tokio::test]
async fn an_acp_author_runs_on_daemon_stdio() {
    let agent_dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(agent_dir.path(), script());
    let h = harness().home(registry_home(&stub)).await;
    let cast = acp_cast(&h).await;

    let session = spawned_idle(&h, &cast).await;

    // The handshake, in order, and nothing else.
    assert_eq!(
        stub.methods(),
        [
            "initialize",
            "session/new",
            "session/set_config_option",
            "session/set_config_option",
            "session/prompt",
        ]
    );
    assert_eq!(stub.calls_of("initialize")[0]["protocolVersion"], 1);

    // The session opens in the author's worktree with the Ariadne MCP server.
    let new = &stub.calls_of("session/new")[0];
    assert!(
        new["cwd"].as_str().unwrap().ends_with("-eng"),
        "{}",
        new["cwd"]
    );
    assert_eq!(new["mcpServers"][0]["args"], json!(["mcp", "serve"]));

    // The model pin, then the effort pin the seat carries.
    let pins = stub.calls_of("session/set_config_option");
    assert_eq!(pins[0]["configId"], "model-id");
    assert_eq!(pins[0]["value"], "test-model");
    assert_eq!(pins[1]["configId"], "effort-id");
    assert_eq!(pins[1]["value"], "high");

    // The briefing rides the first prompt, behind the system prompt.
    let prompt = &stub.calls_of("session/prompt")[0];
    let text = prompt["prompt"][0]["text"].as_str().unwrap();
    assert!(text.contains("do things"), "{text}");

    // The turn is on the record through the ingestion path, and the agent's
    // own session id is on the row.
    let kinds = event_kinds(&h, &session.id).await;
    for kind in [
        "session_start",
        "user_prompt_submit",
        "pre_tool_use",
        "post_tool_use",
        "stop",
    ] {
        assert!(kinds.iter().any(|k| k == kind), "{kind} missing: {kinds:?}");
    }
    let row = h.store.get_session(&session.id).await.unwrap();
    assert_eq!(row.internal_session_id.as_deref(), Some("stub-session"));

    // The agent sits at its prompt waiting for the next turn.
    assert!(stub.process_is_alive());
    assert!(h.launcher.acp.is_running(&session.id));
}

/// Killing the session kills the daemon-owned agent process and retires the
/// row.
#[tokio::test]
async fn killing_an_acp_session_kills_its_agent_process() {
    let agent_dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(agent_dir.path(), script());
    let h = harness().home(registry_home(&stub)).await;
    let cast = acp_cast(&h).await;
    let session = spawned_idle(&h, &cast).await;
    assert!(stub.process_is_alive());

    h.launcher.kill_session(&session.id).await.unwrap();

    assert_eq!(h.session_status(&session).await, SessionStatus::Exited);
    assert!(!h.launcher.acp.is_running(&session.id));
    eventually(TIMEOUT, "the agent process to die", || async {
        !stub.process_is_alive()
    })
    .await;
}

/// A resumed acp author keeps its conversation: a fresh agent process loads
/// the stored session, hears the instruction on a new prompt, and takes the
/// seat — and the predecessor's exit takes neither the seat nor the row down.
#[tokio::test]
async fn resuming_an_acp_author_replaces_the_agent_and_keeps_the_session() {
    let mut scripted = script();
    scripted["stored_sessions"] = json!(["stub-session"]);
    let agent_dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(agent_dir.path(), scripted);
    let h = registry_harness(&stub).await;
    let cast = acp_cast(&h).await;
    let session = spawned_idle(&h, &cast).await;
    let first_pid = stub.pid().expect("the first agent wrote its pid");

    let resumed = h
        .launcher
        .resume_author(&cast.task.id, "fix it")
        .await
        .unwrap();
    assert_eq!(resumed.id, session.id, "the same session comes back");

    // The predecessor dies and is reaped; the successor runs the round trip.
    eventually(TIMEOUT, "the first agent to be reaped", || async {
        !pid_is_alive(first_pid)
    })
    .await;
    eventually(TIMEOUT, "the resumed turn to end", || async {
        stub.calls_of("session/prompt").len() == 2
            && h.session_status(&session).await == SessionStatus::Idle
    })
    .await;

    // The successor loaded the stored conversation and heard the instruction.
    assert_eq!(
        stub.calls_of("session/resume")[0]["sessionId"],
        "stub-session"
    );
    let prompt = &stub.calls_of("session/prompt")[1];
    let text = prompt["prompt"][0]["text"].as_str().unwrap();
    assert!(text.contains("fix it"), "{text}");

    // The predecessor's end is on the record, and it took nothing with it:
    // the second agent holds the seat and the row stays live.
    eventually(TIMEOUT, "the predecessor's end to be recorded", || async {
        event_kinds(&h, &session.id)
            .await
            .iter()
            .any(|k| k == "session_end")
    })
    .await;
    let second_pid = stub.pid().expect("the second agent wrote its pid");
    assert_ne!(second_pid, first_pid);
    assert!(pid_is_alive(second_pid));
    assert!(h.launcher.acp.is_running(&session.id));
    assert_eq!(h.session_status(&session).await, SessionStatus::Idle);
}

/// In auto mode every permission request is approved: the agent hears the
/// allowing option wherever it stands in the list, and the ask and answer
/// are events.
#[tokio::test]
async fn auto_approves_a_permission_request_with_the_allowing_option() {
    let mut scripted = script();
    scripted["prompts"] = json!([{
        "permission": {
            "toolCall": {"toolCallId": "call-1", "title": "Write", "kind": "write",
                         "rawInput": {"path": "src/main.rs"}},
            "options": [
                {"optionId": "no", "name": "Reject", "kind": "reject_once"},
                {"optionId": "yes", "name": "Allow", "kind": "allow_once"},
            ],
        },
        "updates": [],
        "stop_reason": "end_turn",
    }]);
    let agent_dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(agent_dir.path(), scripted);
    let h = harness().home(registry_home(&stub)).await;
    let cast = acp_cast(&h).await;

    let session = spawned_idle(&h, &cast).await;

    let reply = stub
        .messages()
        .into_iter()
        .find(|m| {
            m.get("method").is_none() && m.get("id").and_then(Value::as_str) == Some("permission-1")
        })
        .expect("a permission reply reached the agent");
    assert_eq!(
        reply["result"]["outcome"],
        json!({"outcome": "selected", "optionId": "yes"})
    );

    let kinds = event_kinds(&h, &session.id).await;
    for kind in ["permission_request", "permission.replied"] {
        assert!(kinds.iter().any(|k| k == kind), "{kind} missing: {kinds:?}");
    }
}

/// An agent that dies mid-turn is reaped — its pid is gone, where an unreaped
/// zombie would still answer — and the session ends on the record: the error,
/// the end, and a row no longer live.
#[tokio::test]
async fn a_dead_acp_agent_is_reaped_and_its_session_retired() {
    let mut scripted = script();
    scripted["prompts"] = json!([{"exit": 0}]);
    let agent_dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(agent_dir.path(), scripted);
    let h = harness().home(registry_home(&stub)).await;
    let cast = acp_cast(&h).await;

    let session = h.launcher.spawn_author(&cast.task.id).await.unwrap();

    eventually(TIMEOUT, "the session to retire", || async {
        h.session_status(&session).await == SessionStatus::Exited
    })
    .await;
    eventually(TIMEOUT, "the agent process to be reaped", || async {
        !stub.process_is_alive()
    })
    .await;
    assert!(!h.launcher.acp.is_running(&session.id));
    let kinds = event_kinds(&h, &session.id).await;
    for kind in ["session.error", "session_end"] {
        assert!(kinds.iter().any(|k| k == kind), "{kind} missing: {kinds:?}");
    }
}

/// An orchestrator seat runs on the agent its pin names in the registry:
/// the registry command is spawned, the bare model half is pinned, and the
/// session opens with the ariadne MCP server.
#[tokio::test]
async fn an_orchestrator_runs_on_the_registry_agent() {
    let agent_dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(agent_dir.path(), script());
    let h = registry_harness(&stub).await;
    let repo_path = h.git_repo("repo");
    let repo = h.repository(&repo_path).await;
    let goal = h.goal_on(&repo, registry_pin(Some("high"))).await;

    let session = h.launcher.spawn_orchestrator(&goal.id).await.unwrap();
    eventually(TIMEOUT, "the orchestrator's first turn to end", || async {
        h.session_status(&session).await == SessionStatus::Idle
    })
    .await;

    // The registry split: the pin's model half without the agent id, and the
    // effort beside it, taken from the seat's pin.
    let pins = stub.calls_of("session/set_config_option");
    assert_eq!(pins[0]["configId"], "model-id");
    assert_eq!(pins[0]["value"], "test-model");
    assert_eq!(pins[1]["value"], "high");

    // The orchestrator's session opens in the repo with the ariadne MCP
    // server, which is the seat's whole tool surface.
    let new = &stub.calls_of("session/new")[0];
    assert_eq!(new["cwd"].as_str().unwrap(), repo_path.to_str().unwrap());
    assert_eq!(new["mcpServers"][0]["name"], "ariadne");
    assert_eq!(new["mcpServers"][0]["args"], json!(["mcp", "serve"]));

    // The briefing rides the first prompt.
    let prompt = &stub.calls_of("session/prompt")[0];
    let text = prompt["prompt"][0]["text"].as_str().unwrap();
    assert!(text.contains("Ship the UI"), "{text}");
    assert!(h.launcher.acp.is_running(&session.id));
}

/// A reviewer seat runs on the registry agent the same way: a detached
/// worktree as its cwd, the ariadne MCP server, and the briefing as the
/// first prompt.
#[tokio::test]
async fn a_reviewer_runs_on_the_registry_agent() {
    let agent_dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(agent_dir.path(), script());
    let h = registry_harness(&stub).await;
    let cast = registry_cast(&h).await;
    // Something on the task branch to review.
    sh(&h.at("repo"), &format!("git branch {}", cast.task.branch));

    let session = h
        .launcher
        .spawn_reviewer(&cast.task.id, &cast.reviewer.id)
        .await
        .unwrap();
    eventually(TIMEOUT, "the reviewer's first turn to end", || async {
        h.session_status(&session).await == SessionStatus::Idle
    })
    .await;

    let new = &stub.calls_of("session/new")[0];
    assert!(
        new["cwd"].as_str().unwrap().contains("-rev-"),
        "{}",
        new["cwd"]
    );
    assert_eq!(new["mcpServers"][0]["args"], json!(["mcp", "serve"]));
    assert_eq!(
        stub.calls_of("session/set_config_option")[0]["value"],
        "test-model"
    );
    let prompt = &stub.calls_of("session/prompt")[0];
    let text = prompt["prompt"][0]["text"].as_str().unwrap();
    assert!(text.contains("do things"), "{text}");
    assert!(h.launcher.acp.is_running(&session.id));
}

/// After a daemon restart nothing owns the old child, and a revive reaches
/// the stored conversation through `session/load`: the agent advertises no
/// `session/resume`, and the instruction rides the new prompt.
#[tokio::test]
async fn a_stub_session_resumes_through_session_load_after_a_daemon_restart() {
    let mut scripted = script();
    scripted["capabilities"] = json!({"loadSession": true});
    scripted["stored_sessions"] = json!(["stub-session"]);
    let agent_dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(agent_dir.path(), scripted);
    let h = registry_harness(&stub).await;
    let cast = registry_cast(&h).await;
    let session = spawned_idle(&h, &cast).await;
    h.launcher.kill_session(&session.id).await.unwrap();

    // The restarted daemon: the same store, config and registry, and a fresh
    // runtime that owns no child of the daemon that died.
    let restarted = ariadne_daemon::launcher::Launcher {
        cfg: h.launcher.cfg.clone(),
        store: h.store.clone(),
        git: ariadne_daemon::gitwt::GitManager,
        acp: ariadne_daemon::acp::AcpRuntime::new(h.store.clone()),
        registry: h.launcher.registry.clone(),
        branches: ariadne_daemon::branch::BranchWatchers::new(h.bus.clone()),
    };
    let revived = restarted
        .revive_session(&session.id, Some("carry on"))
        .await
        .unwrap();
    assert_eq!(revived.id, session.id, "the same session comes back");

    eventually(TIMEOUT, "the resumed turn to end", || async {
        stub.calls_of("session/prompt").len() == 2
    })
    .await;
    assert_eq!(
        stub.calls_of("session/load")[0]["sessionId"],
        "stub-session"
    );
    assert!(
        stub.calls_of("session/resume").is_empty(),
        "the agent advertises no session/resume"
    );
    let prompt = &stub.calls_of("session/prompt")[1];
    let text = prompt["prompt"][0]["text"].as_str().unwrap();
    assert!(text.contains("carry on"), "{text}");
    assert!(restarted.acp.is_running(&session.id));
}

/// An agent discovery measured without `session/load` cannot come back into
/// a stored conversation: the resume is refused as not resumable, and no
/// agent process is started for it.
#[tokio::test]
async fn a_session_of_an_agent_without_session_load_is_not_resumable() {
    let mut scripted = script();
    scripted["capabilities"] = json!({});
    let agent_dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(agent_dir.path(), scripted);
    let h = registry_harness(&stub).await;
    let cast = registry_cast(&h).await;
    let session = spawned_idle(&h, &cast).await;
    h.launcher.kill_session(&session.id).await.unwrap();

    let error = h
        .launcher
        .resume_author(&cast.task.id, "fix it")
        .await
        .unwrap_err();
    assert!(format!("{error:#}").contains("not resumable"), "{error:#}");
    assert!(stub.calls_of("session/load").is_empty());
    assert!(!h.launcher.acp.is_running(&session.id));
}

/// A scheduler nudge reaches an idle agent as a `session/prompt`: the resume
/// text arrives as a new turn.
#[tokio::test]
async fn a_scheduler_nudge_arrives_at_the_stub_agent_as_a_prompt() {
    let agent_dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(agent_dir.path(), script());
    let h = scheduled_registry_harness(&stub).await;
    let cast = registry_cast(&h).await;
    h.activate(&cast.goal).await;
    h.notify(&cast.task.id);

    // The scheduler starts the author; its first turn ends idle.
    let idle_author = || async {
        h.sessions_of(&cast.task.id)
            .await
            .into_iter()
            .find(|s| s.seat() == ariadne_core::Seat::Author && s.status() == SessionStatus::Idle)
    };
    eventually(TIMEOUT, "the author's first turn to end", || async {
        idle_author().await.is_some()
    })
    .await;
    let author = idle_author().await.unwrap();

    // Quiet past the nudge threshold, in the situation it went idle in.
    h.launched_ago(&author, 240).await;
    h.set_status(&author, SessionStatus::Idle).await;
    h.notify(&cast.task.id);

    eventually(TIMEOUT, "the nudge to arrive as a prompt", || async {
        stub.calls_of("session/prompt").iter().any(|prompt| {
            prompt["prompt"][0]["text"]
                .as_str()
                .is_some_and(|text| text.contains("Continue \"task\""))
        })
    })
    .await;

    // End the goal and take its agents down, so nothing respawns and no
    // child outlives the test.
    h.store
        .set_goal_status(&cast.goal.id, ariadne_core::GoalStatus::Cancelled)
        .await
        .unwrap();
    for session in h.sessions_of_goal(&cast.goal.id).await {
        let _ = h.launcher.kill_session(&session.id).await;
    }
    eventually(TIMEOUT, "the agents to die", || async {
        !stub.process_is_alive()
    })
    .await;
}
