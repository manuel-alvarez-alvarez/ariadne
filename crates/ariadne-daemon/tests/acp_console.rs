//! Integration tests for the ACP session console: the transcript snapshot,
//! its live stream, and posting input as a `session/prompt`.
//!
//! The agent is the scriptable stub from test support (`common::acp`), the
//! same one `acp_runtime.rs` drives the runtime itself against.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::json;

use ariadne_core::{Actor, AttentionReason, PermissionMode, Seat, SessionStatus, TaskStatus};
use ariadne_store::{AgentPin, NewTask, NewTaskAgent, Store};

use common::acp::{StubAcpAgent, registry_home, script, stub_acp_agent};
use common::{Cast, Harness, TIMEOUT, eventually, expect_sse, get, harness, post_json};

/// A task whose author runs on the registry agent `stub`, in a real repo,
/// with an effort pinned — the same fixture `acp_runtime.rs` casts.
async fn acp_cast(h: &Harness) -> Cast {
    h.git_repo("repo");
    let cast = h.cast_pinned("stub:test-model", 1).await;
    h.store
        .set_agent_pin(
            &cast.author.id,
            &AgentPin {
                model: "stub:test-model".into(),
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

/// A request whose allowing and denying answers are distinct, so a test can
/// prove that only the former becomes a learned approval.
fn permission_script() -> serde_json::Value {
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
    scripted
}

/// A home whose configured ACP permission policy is read as the daemon would
/// read it, before the harness starts the runtime around it, with the stub
/// registered as the agent `stub`.
fn home_with_permission_mode(
    dir: &tempfile::TempDir,
    mode: &str,
    stub: &StubAcpAgent,
) -> std::path::PathBuf {
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(
        home.join("config.toml"),
        format!(
            "permission_mode = \"{mode}\"\n\n[[acp_agents]]\nid = \"stub\"\ncommand = [{:?}]\n",
            stub.bin
        ),
    )
    .unwrap();
    home
}

/// Put a task in the lifecycle state where an author is actively owed work.
async fn ready(h: &Harness, task_id: &str) {
    h.store
        .transition_task(task_id, TaskStatus::Ready, Actor::Daemon, None, None)
        .await
        .unwrap();
}

/// The console stream opens with a snapshot of everything the session has
/// done so far, then delivers a later turn's events as deltas.
#[tokio::test]
async fn the_console_stream_gives_the_snapshot_then_deltas() {
    let agent_dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(agent_dir.path(), script());
    let h = harness().home(registry_home(&stub)).await;
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
    let h = harness().home(registry_home(&stub)).await;
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

/// Ask mode emits the request to the console, raises attention, leaves the
/// turn blocked, and uses posted console text as the selected option id.
#[tokio::test]
async fn ask_raises_attention_and_a_console_answer_unblocks_the_turn() {
    let root = tempfile::tempdir().unwrap();
    let agent_dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(agent_dir.path(), permission_script());
    let h = harness()
        .home(home_with_permission_mode(&root, "auto", &stub))
        .await;
    let cast = acp_cast(&h).await;
    let task = h
        .store
        .create_task(NewTask {
            goal_id: cast.goal.id.clone(),
            repo_id: cast.repo.id.clone(),
            title: "Ask before writing".into(),
            description: "do things".into(),
            agents: vec![
                NewTaskAgent::new(Seat::Author, ["coding"], common::test_pin()),
                NewTaskAgent::new(Seat::Reviewer, ["code-review"], common::test_pin()),
            ],
            depends_on: vec![],
            landing: None,
            permission_mode: Some(PermissionMode::Ask),
        })
        .await
        .unwrap();
    ready(&h, &task.id).await;
    let session = h.launcher.spawn_author(&task.id).await.unwrap();

    eventually(TIMEOUT, "the permission attention to rise", || async {
        h.attention(&session).await == Some(AttentionReason::WaitingPermission)
    })
    .await;
    assert!(
        stub.messages()
            .iter()
            .all(|message| message.get("method").is_some()),
        "the agent must still be waiting for an answer"
    );

    let (status, _) = h.send(post_console_input(&session.id, "yes")).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    eventually(TIMEOUT, "the answered turn to finish", || async {
        h.session_status(&session).await == SessionStatus::Idle
    })
    .await;
    assert_eq!(h.attention(&session).await, None);
    let reply = stub
        .messages()
        .into_iter()
        .find(|message| {
            message.get("id").and_then(serde_json::Value::as_str) == Some("permission-1")
        })
        .expect("the console answer reached the agent");
    assert_eq!(reply["result"]["outcome"]["optionId"], "yes");
}

/// Learn mode asks again after a denial, records an approval under the
/// repository, and a fresh store opened over the database finds that row.
#[tokio::test]
async fn learn_remembers_an_approval_per_repository_across_a_daemon_restart() {
    let root = tempfile::tempdir().unwrap();
    let agent_dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(agent_dir.path(), permission_script());
    let h = harness()
        .home(home_with_permission_mode(&root, "learn", &stub))
        .await;
    let cast = acp_cast(&h).await;
    ready(&h, &cast.task.id).await;

    let denied = h.launcher.spawn_author(&cast.task.id).await.unwrap();
    eventually(TIMEOUT, "the first permission attention", || async {
        h.attention(&denied).await == Some(AttentionReason::WaitingPermission)
    })
    .await;
    let (status, _) = h.send(post_console_input(&denied.id, "no")).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    eventually(TIMEOUT, "the denied turn to finish", || async {
        h.session_status(&denied).await == SessionStatus::Idle
    })
    .await;
    assert!(
        !h.store
            .has_learned_permission(&cast.repo.id, "Write", "write")
            .await
            .unwrap(),
        "a denial is not a learned approval"
    );

    let asked_again = h
        .task_on(&cast.goal, &cast.repo, "Ask again", 1, common::test_pin())
        .await;
    ready(&h, &asked_again.id).await;
    let approved = h.launcher.spawn_author(&asked_again.id).await.unwrap();
    eventually(TIMEOUT, "the second permission attention", || async {
        h.attention(&approved).await == Some(AttentionReason::WaitingPermission)
    })
    .await;
    let (status, _) = h.send(post_console_input(&approved.id, "yes")).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    eventually(TIMEOUT, "the approved turn to finish", || async {
        h.session_status(&approved).await == SessionStatus::Idle
    })
    .await;

    let restarted = Store::open(h.dir.path().join("test.db")).await.unwrap();
    assert!(
        restarted
            .has_learned_permission(&cast.repo.id, "Write", "write")
            .await
            .unwrap(),
        "a daemon restart reads the learned approval from its store"
    );

    let remembered = h
        .task_on(
            &cast.goal,
            &cast.repo,
            "Use approval",
            1,
            common::test_pin(),
        )
        .await;
    ready(&h, &remembered.id).await;
    let session = h.launcher.spawn_author(&remembered.id).await.unwrap();
    eventually(
        TIMEOUT,
        "the remembered permission turn to finish",
        || async { h.session_status(&session).await == SessionStatus::Idle },
    )
    .await;
    assert_eq!(h.attention(&session).await, None);
    let replies: Vec<_> = stub
        .messages()
        .into_iter()
        .filter(|message| {
            message.get("id").and_then(serde_json::Value::as_str) == Some("permission-1")
        })
        .collect();
    assert_eq!(replies.len(), 3, "each stub process got one reply");
    assert_eq!(replies[2]["result"]["outcome"]["optionId"], "yes");
}

/// A permission request, and the daemon's reply to it, appear in the console
/// stream — the same events `acp_runtime.rs` proves reach the store.
#[tokio::test]
async fn a_permission_request_appears_in_the_console_stream() {
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
