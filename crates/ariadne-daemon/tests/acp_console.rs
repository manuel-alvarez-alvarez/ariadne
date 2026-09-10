//! Integration tests for the ACP session console: the transcript snapshot,
//! its live stream, and posting input as a `session/prompt`.
//!
//! The agent is the scriptable stub from test support (`common::acp`), the
//! same one `acp_runtime.rs` drives the runtime itself against.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::json;

use ariadne_core::{AgentKind, SessionStatus};
use ariadne_store::AgentPin;

use common::acp::{script, stub_acp_agent};
use common::{Cast, Harness, TIMEOUT, eventually, expect_sse, get, harness, post_json};

/// A task whose author runs on `acp`, in a real repo, with an effort pinned —
/// the same fixture `acp_runtime.rs` casts.
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

/// Spawn the task's author against the stub and wait for the initial turn to
/// end.
async fn spawned_idle(h: &Harness, cast: &Cast) -> ariadne_store::AgentSession {
    let session = h.launcher.spawn_author(&cast.task.id).await.unwrap();
    eventually(TIMEOUT, "the prompt round trip to end", || async {
        h.session_status(&session).await == SessionStatus::Idle
    })
    .await;
    session
}

fn post_console_input(session_id: &str, text: &str) -> Request<Body> {
    post_json(
        &format!("/v1/sessions/{session_id}/console/input"),
        json!({ "text": text }),
    )
}

/// The console stream opens with a snapshot of everything the session has
/// done so far, then delivers a later turn's events as deltas.
#[tokio::test]
async fn the_console_stream_gives_the_snapshot_then_deltas() {
    let agent_dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(agent_dir.path(), script());
    let h = harness().acp_bin(&stub.bin).await;
    let cast = acp_cast(&h).await;
    let session = spawned_idle(&h, &cast).await;

    let mut body = h
        .stream(get(&format!("/v1/sessions/{}/console/stream", session.id)))
        .await;

    let snapshot = expect_sse(&mut body, "snapshot").await;
    let kinds: Vec<String> = snapshot
        .as_array()
        .expect("snapshot is an array of events")
        .iter()
        .map(|e| e["kind"].as_str().unwrap().to_string())
        .collect();
    for kind in [
        "session_start",
        "user_prompt_submit",
        "pre_tool_use",
        "stop",
    ] {
        assert!(
            kinds.iter().any(|k| k == kind),
            "{kind} missing from the snapshot: {kinds:?}"
        );
    }

    // A second turn, run by posting input, arrives as a delta rather than
    // requiring a fresh connection.
    let (status, _) = h.send(post_console_input(&session.id, "keep going")).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let delta = expect_sse(&mut body, "event").await;
    assert_eq!(delta["kind"], "user_prompt_submit");
    assert!(
        delta["payload"]["prompt"]
            .as_str()
            .unwrap()
            .contains("keep going"),
        "{delta}"
    );
}

/// Posted text becomes a `session/prompt` reaching the agent; text posted
/// while a turn is still running queues behind it and is sent only once that
/// turn ends.
#[tokio::test]
async fn posted_input_reaches_the_agent_and_queues_behind_a_running_turn() {
    let agent_dir = tempfile::tempdir().unwrap();
    let wait_for = agent_dir.path().join("resume-turn");
    let mut scripted = script();
    let first_turn = scripted["prompts"][0].clone();
    scripted["prompts"] = json!([
        first_turn,
        {"wait_for": wait_for.display().to_string(), "updates": [], "stop_reason": "end_turn"},
        {"updates": [], "stop_reason": "end_turn"},
    ]);
    let stub = stub_acp_agent(agent_dir.path(), scripted);
    let h = harness().acp_bin(&stub.bin).await;
    let cast = acp_cast(&h).await;
    let session = spawned_idle(&h, &cast).await;

    // Starts the second turn, which the stub pauses partway through.
    let (status, _) = h
        .send(post_console_input(
            &session.id,
            "run the paused instruction",
        ))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let reached = agent_dir.path().join("resume-turn.reached");
    eventually(TIMEOUT, "the paused turn to be reached", || async {
        reached.exists()
    })
    .await;

    // Posted while that turn is still open: queued, not sent.
    let (status, _) = h
        .send(post_console_input(
            &session.id,
            "run the queued instruction",
        ))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        stub.calls_of("session/prompt").len(),
        2,
        "the queued input must not reach the agent while a turn is running"
    );

    // Let the paused turn finish; the queued one follows it.
    std::fs::write(&wait_for, "go").unwrap();
    eventually(TIMEOUT, "the queued turn to run", || async {
        stub.calls_of("session/prompt").len() == 3
    })
    .await;

    let prompts = stub.calls_of("session/prompt");
    assert!(
        prompts[1]["prompt"][0]["text"]
            .as_str()
            .unwrap()
            .contains("run the paused instruction")
    );
    assert!(
        prompts[2]["prompt"][0]["text"]
            .as_str()
            .unwrap()
            .contains("run the queued instruction"),
        "the queued prompt arrives only after the turn it waited behind"
    );
}

/// A permission request, and the daemon's reply to it, appear in the console
/// stream — the same events `acp_runtime.rs` proves reach the store.
#[tokio::test]
async fn a_permission_request_appears_in_the_console_stream() {
    let mut scripted = script();
    scripted["prompts"] = json!([{
        "permission": {
            "toolCall": {"toolCallId": "call-1", "title": "Write",
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

    let mut body = h
        .stream(get(&format!("/v1/sessions/{}/console/stream", session.id)))
        .await;
    let snapshot = expect_sse(&mut body, "snapshot").await;
    let kinds: Vec<String> = snapshot
        .as_array()
        .expect("snapshot is an array of events")
        .iter()
        .map(|e| e["kind"].as_str().unwrap().to_string())
        .collect();
    for kind in ["permission_request", "permission.replied"] {
        assert!(
            kinds.iter().any(|k| k == kind),
            "{kind} missing from the console: {kinds:?}"
        );
    }
}
