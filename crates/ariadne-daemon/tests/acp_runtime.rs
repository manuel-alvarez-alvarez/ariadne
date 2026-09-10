//! The daemon drives an `acp` author itself: the agent is a child process on
//! piped stdio, spoken to over ACP version 1, and there is no tmux session
//! anywhere in its life.
//!
//! The agent is the scriptable stub from test support (`common::acp`): every
//! JSON-RPC message the daemon sends lands in its log, which is where the
//! handshake, the pins and the prompt are asserted. `git` is real — spawning
//! an author creates its worktree. `tmux` is the recording stub, so a pane
//! created or killed anywhere in the path would show in its command log.

mod common;

use serde_json::{Value, json};

use ariadne_core::{AgentKind, SessionStatus};
use ariadne_store::{AgentPin, EventFilter};

use common::acp::{pid_is_alive, script, stub_acp_agent};
use common::{Cast, Harness, TIMEOUT, eventually, harness};

/// A task whose author runs on `acp`, in a real repo, with an effort pinned
/// so the launch has one to set.
async fn acp_cast(h: &Harness) -> Cast {
    h.git_repo("repo");
    let cast = h.cast_on(AgentKind::Acp).await;
    h.store
        .set_agent_pin(
            &cast.author.id,
            &AgentPin {
                agent_kind: AgentKind::Acp,
                model: "test-model".into(),
                effort: Some("high".into()),
            },
        )
        .await
        .unwrap();
    cast
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
/// prompt, the events in the store, the captured agent session id — and not
/// one tmux call for any of it.
#[tokio::test]
async fn an_acp_author_runs_on_daemon_stdio_with_no_tmux_session() {
    let agent_dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(agent_dir.path(), script());
    let h = harness().acp_bin(&stub.bin).await;
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

    // No pane: tmux was never asked to create one, and the agent sits at its
    // prompt waiting for the next turn.
    assert_eq!(h.tmux_calls_of("new-session"), Vec::<String>::new());
    assert!(stub.process_is_alive());
    assert!(h.launcher.acp.is_running(&session.id));
}

/// Killing the session kills the daemon-owned agent process and retires the
/// row; no pane is asked to die.
#[tokio::test]
async fn killing_an_acp_session_kills_its_agent_process() {
    let agent_dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(agent_dir.path(), script());
    let h = harness().acp_bin(&stub.bin).await;
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
    assert_eq!(h.killed_panes(), Vec::<String>::new());
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
    let h = harness().acp_bin(&stub.bin).await;
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
    assert_eq!(h.tmux_calls_of("new-session"), Vec::<String>::new());
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
    let h = harness().acp_bin(&stub.bin).await;
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
    let h = harness().acp_bin(&stub.bin).await;
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
