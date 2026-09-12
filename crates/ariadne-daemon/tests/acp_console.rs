//! Integration tests for the ACP session console: the transcript snapshot,
//! its live stream — the chunks and tool call progress a running turn
//! streams, and the whole text stored once it ends — posting input as a
//! `session/prompt`, and cancelling a turn.
//!
//! The agent is the scriptable stub from test support (`common::acp`), the
//! same one `acp_runtime.rs` drives the runtime itself against.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::json;

use ariadne_api::events::AgentEventDto;
use ariadne_api::goals::GoalDto;
use ariadne_api::sessions::SessionDto;
use ariadne_api::tasks::TaskDto;
use ariadne_api::usage::TokenUsageDto;
use ariadne_core::{Actor, AttentionReason, PermissionMode, Seat, SessionStatus, TaskStatus};
use ariadne_store::{AgentPin, NewTask, NewTaskAgent, Store};

use ariadne_api::stream::{DeletedDto, DomainEvent};
use ariadne_daemon::bus::{BusEvent, EventBus};
use ariadne_daemon::http::{self, AppState};

use common::acp::{StubAcpAgent, registry_home, script, stub_acp_agent};
use common::{
    Cast, Harness, TIMEOUT, eventually, expect_sse, get, harness, next_sse_message, parse_sse,
    post, post_json, sse_is_closed,
};

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

fn tokens(input_tokens: u64, cached_input_tokens: u64, output_tokens: u64) -> TokenUsageDto {
    TokenUsageDto {
        input_tokens,
        cached_input_tokens,
        output_tokens,
    }
}

/// ACP's standard usage is cumulative for a launch, so the latest response
/// replaces the earlier one and its cached tokens remain part of input.
#[tokio::test]
async fn standard_prompt_usage_replaces_launch_totals_and_rolls_up() {
    let agent_dir = tempfile::tempdir().unwrap();
    let mut scripted = script();
    scripted["prompts"] = json!([
        {"updates": [], "usage": {"totalTokens": 100, "inputTokens": 10,
          "cachedReadTokens": 20, "cachedWriteTokens": 30, "outputTokens": 40}},
        {"updates": [], "usage": {"totalTokens": 600, "inputTokens": 100,
          "cachedReadTokens": 200, "cachedWriteTokens": 300, "outputTokens": 400}},
    ]);
    let stub = stub_acp_agent(agent_dir.path(), scripted);
    let h = harness().home(registry_home(&stub)).await;
    let cast = acp_cast(&h).await;
    let session = spawned_idle(&h, &cast).await;

    let session_usage: SessionDto = h.get(&format!("/v1/sessions/{}", session.id)).await;
    assert_eq!(session_usage.usage, tokens(60, 50, 40));

    let (status, _) = h
        .send(post_console_input(&session.id, "report later totals"))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    eventually(TIMEOUT, "the second prompt to end", || async {
        stub.calls_of("session/prompt").len() == 2
            && h.session_status(&session).await == SessionStatus::Idle
    })
    .await;

    let session_usage: SessionDto = h.get(&format!("/v1/sessions/{}", session.id)).await;
    assert_eq!(session_usage.usage, tokens(600, 500, 400));
    let task: TaskDto = h.get(&format!("/v1/tasks/{}", cast.task.id)).await;
    assert_eq!(task.usage.total, tokens(600, 500, 400));
    let goal: GoalDto = h.get(&format!("/v1/goals/{}", cast.goal.id)).await;
    assert_eq!(goal.usage.total, tokens(600, 500, 400));
}

/// The adapters' quota report includes subagent use, so it takes precedence
/// over the standard response where the two disagree.
#[tokio::test]
async fn quota_prompt_usage_takes_precedence_over_standard_usage() {
    let agent_dir = tempfile::tempdir().unwrap();
    let mut scripted = script();
    scripted["prompts"] = json!([{
        "updates": [],
        "usage": {"inputTokens": 100, "cachedReadTokens": 200,
                  "cachedWriteTokens": 300, "outputTokens": 400},
        "quota": {"inputTokens": 4, "cachedInputTokens": 5,
                  "cachedWriteTokens": 6, "outputTokens": 7},
    }]);
    let stub = stub_acp_agent(agent_dir.path(), scripted);
    let h = harness().home(registry_home(&stub)).await;
    let cast = acp_cast(&h).await;
    let session = spawned_idle(&h, &cast).await;

    let session: SessionDto = h.get(&format!("/v1/sessions/{}", session.id)).await;
    assert_eq!(session.usage, tokens(15, 11, 7));
}

/// A response without either usage shape keeps usage at zero, but the turn
/// still ends with its terminal event.
#[tokio::test]
async fn a_prompt_without_usage_keeps_zero_totals_and_records_stop() {
    let agent_dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(agent_dir.path(), script());
    let h = harness().home(registry_home(&stub)).await;
    let cast = acp_cast(&h).await;
    let session = spawned_idle(&h, &cast).await;

    let session_usage: SessionDto = h.get(&format!("/v1/sessions/{}", session.id)).await;
    assert_eq!(session_usage.usage, tokens(0, 0, 0));
    let events: Vec<AgentEventDto> = h.get(&format!("/v1/sessions/{}/console", session.id)).await;
    assert!(events.iter().any(|event| event.kind == "stop"));
}

/// A resumed adapter starts its counters again, so its launch source adds to
/// the completed launch instead of replacing it.
#[tokio::test]
async fn resumed_prompt_usage_adds_a_new_launch_total() {
    let agent_dir = tempfile::tempdir().unwrap();
    let mut first = script();
    first["prompts"] = json!([{
        "updates": [],
        "usage": {"inputTokens": 10, "cachedReadTokens": 20,
                  "cachedWriteTokens": 30, "outputTokens": 40},
    }]);
    let stub = stub_acp_agent(agent_dir.path(), first);
    let h = harness().home(registry_home(&stub)).discover_agents().await;
    let cast = acp_cast(&h).await;
    let session = spawned_idle(&h, &cast).await;

    h.launcher.kill_session(&session.id).await.unwrap();
    let mut resumed_script = script();
    resumed_script["stored_sessions"] = json!(["stub-session"]);
    resumed_script["prompts"] = json!([{
        "updates": [],
        "usage": {"inputTokens": 1, "cachedReadTokens": 2,
                  "cachedWriteTokens": 3, "outputTokens": 4},
    }]);
    stub.reprogram(resumed_script);
    let resumed = h
        .launcher
        .resume_author(&cast.task.id, "continue this conversation")
        .await
        .unwrap();
    assert_eq!(resumed.id, session.id);
    eventually(TIMEOUT, "the resumed prompt to end", || async {
        stub.calls_of("session/prompt").len() == 2
            && h.session_status(&resumed).await == SessionStatus::Idle
    })
    .await;

    let session_usage: SessionDto = h.get(&format!("/v1/sessions/{}", resumed.id)).await;
    assert_eq!(session_usage.usage, tokens(66, 55, 44));
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

/// A second turn that says everything a turn can: thought chunks, a plan, a
/// tool call with two updates, message chunks — then holds the turn open on
/// `wait_for` until the test lets go or cancels it.
fn streaming_script(wait_for: &std::path::Path) -> serde_json::Value {
    let mut scripted = script();
    let first_turn = scripted["prompts"][0].clone();
    scripted["prompts"] = json!([
        first_turn,
        {
            "updates": [
                {"sessionUpdate": "agent_thought_chunk", "content": {"type": "text", "text": "Think "}},
                {"sessionUpdate": "agent_thought_chunk", "content": {"type": "text", "text": "first."}},
                {"sessionUpdate": "plan", "entries": [
                    {"content": "List the files", "priority": "high", "status": "pending"},
                ]},
                {"sessionUpdate": "tool_call", "toolCallId": "call-2", "title": "Bash",
                 "kind": "execute", "status": "pending", "rawInput": {"command": "ls"}},
                {"sessionUpdate": "tool_call_update", "toolCallId": "call-2", "status": "in_progress"},
                {"sessionUpdate": "tool_call_update", "toolCallId": "call-2", "status": "completed",
                 "content": [{"type": "content", "content": {"type": "text", "text": "README.md"}}],
                 "rawOutput": {"stdout": "README.md\n"}},
                {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "Half "}},
                {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "done."}},
            ],
            "wait_for": wait_for.display().to_string(),
            "stop_reason": "end_turn",
        },
    ]);
    scripted
}

/// The stub, a session idle after its first turn, and the file that lets the
/// streaming turn end.
struct Streaming {
    _agent_dir: tempfile::TempDir,
    stub: StubAcpAgent,
    h: Harness,
    session: ariadne_store::AgentSession,
    wait_for: std::path::PathBuf,
}

impl Streaming {
    async fn start() -> Self {
        let agent_dir = tempfile::tempdir().unwrap();
        let wait_for = agent_dir.path().join("streaming-turn");
        let stub = stub_acp_agent(agent_dir.path(), streaming_script(&wait_for));
        let h = harness().home(registry_home(&stub)).await;
        let cast = acp_cast(&h).await;
        let session = spawned_idle(&h, &cast).await;
        Self {
            _agent_dir: agent_dir,
            stub,
            h,
            session,
            wait_for,
        }
    }

    /// Start the streaming turn from the console and wait until the stub has
    /// sent everything and holds the turn open.
    async fn begin_turn(&self) {
        let (status, _) = self
            .h
            .send(post_console_input(&self.session.id, "stream it"))
            .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let reached = self._agent_dir.path().join("streaming-turn.reached");
        eventually(TIMEOUT, "the streaming turn to be held open", || async {
            reached.exists()
        })
        .await;
    }

    /// Let the held turn end, and wait for the session to go idle on it.
    async fn end_turn(&self) {
        std::fs::write(&self.wait_for, "go").unwrap();
        eventually(TIMEOUT, "the streaming turn to end", || async {
            self.h.session_status(&self.session).await == SessionStatus::Idle
        })
        .await;
    }

    fn console_stream(&self) -> Request<Body> {
        get(&format!("/v1/sessions/{}/console/stream", self.session.id))
    }

    async fn snapshot(&self) -> Vec<serde_json::Value> {
        self.h
            .get(&format!("/v1/sessions/{}/console", self.session.id))
            .await
    }
}

fn kinds(events: &[serde_json::Value]) -> Vec<String> {
    events
        .iter()
        .map(|e| e["kind"].as_str().unwrap().to_string())
        .collect()
}

/// Read `event` frames off a console stream until one of `kind` arrives,
/// answering every event read on the way, that one last.
async fn events_until(body: &mut Body, kind: &str) -> Vec<serde_json::Value> {
    let mut seen = Vec::new();
    loop {
        let event = expect_sse(body, "event").await;
        let done = event["kind"] == kind;
        seen.push(event);
        if done {
            return seen;
        }
    }
}

fn text_of<'a>(events: &'a [serde_json::Value], kind: &str) -> Vec<&'a str> {
    events
        .iter()
        .filter(|e| e["kind"] == kind)
        .map(|e| e["payload"]["text"].as_str().unwrap())
        .collect()
}

/// A console stream client sees each `agent_message_chunk` as the stub sends
/// it, while the turn is still open.
#[tokio::test]
async fn a_console_stream_client_sees_message_chunks_before_the_turn_ends() {
    let t = Streaming::start().await;
    let mut body = t.h.stream(t.console_stream()).await;
    expect_sse(&mut body, "snapshot").await;

    t.begin_turn().await;
    let seen = events_until(&mut body, "agent_message_chunk").await;
    let seen = [seen, events_until(&mut body, "agent_message_chunk").await].concat();

    assert_eq!(
        text_of(&seen, "agent_message_chunk"),
        ["Half ", "done."],
        "{seen:?}"
    );
    assert!(
        !kinds(&seen).iter().any(|k| k == "stop"),
        "the chunks arrive before the turn ends: {:?}",
        kinds(&seen)
    );
    assert_eq!(t.h.session_status(&t.session).await, SessionStatus::Running);
    t.end_turn().await;
}

/// Thought chunks and a tool call's progress reach the stream live too, the
/// progress carrying the call merged so far.
#[tokio::test]
async fn thought_chunks_and_tool_call_progress_reach_the_console_stream_live() {
    let t = Streaming::start().await;
    let mut body = t.h.stream(t.console_stream()).await;
    expect_sse(&mut body, "snapshot").await;

    t.begin_turn().await;
    let seen = events_until(&mut body, "tool_call_update").await;

    assert_eq!(
        text_of(&seen, "agent_thought_chunk"),
        ["Think ", "first."],
        "{seen:?}"
    );
    let progress = seen.last().unwrap();
    assert_eq!(progress["payload"]["tool_call_id"], "call-2");
    assert_eq!(progress["payload"]["acp"]["status"], "in_progress");
    assert_eq!(progress["payload"]["acp"]["title"], "Bash");
    assert_eq!(t.h.session_status(&t.session).await, SessionStatus::Running);
    t.end_turn().await;
}

/// Once the turn is over the snapshot holds the whole thought and the whole
/// message once each, then the `stop`, and no chunk at all.
#[tokio::test]
async fn the_snapshot_after_a_turn_holds_the_whole_thought_and_message_once() {
    let t = Streaming::start().await;
    t.begin_turn().await;
    t.end_turn().await;

    let events = t.snapshot().await;
    let kinds = kinds(&events);
    let turn: Vec<&str> = kinds
        .iter()
        .map(String::as_str)
        .filter(|k| ["agent_thought", "agent_message", "stop"].contains(k))
        .collect();
    // The first turn said "done" with no thought; the streaming one both.
    assert_eq!(
        turn,
        [
            "agent_message",
            "stop",
            "agent_thought",
            "agent_message",
            "stop"
        ],
        "{kinds:?}"
    );
    assert_eq!(text_of(&events, "agent_thought"), ["Think first."]);
    assert_eq!(text_of(&events, "agent_message"), ["done", "Half done."]);
    assert!(
        !kinds
            .iter()
            .any(|k| k.ends_with("_chunk") || k == "tool_call_update"),
        "nothing live-only is stored: {kinds:?}"
    );
    let stop = events.iter().rfind(|e| e["kind"] == "stop").unwrap();
    assert_eq!(stop["payload"]["stop_reason"], "end_turn");
    assert!(stop["payload"].get("last_assistant_message").is_none());
    assert!(
        kinds.iter().any(|k| k == "plan"),
        "the plan is stored: {kinds:?}"
    );
}

/// The live-only events reach the console alone: neither the events listing
/// nor the domain stream carries a chunk or a tool call's progress.
#[tokio::test]
async fn the_events_listing_and_the_domain_stream_carry_no_chunk() {
    let t = Streaming::start().await;
    let mut domain =
        t.h.stream(get(&format!(
            "/v1/events/stream?task={}",
            t.session.task_id.as_deref().unwrap()
        )))
        .await;
    expect_sse(&mut domain, "heartbeat").await;

    t.begin_turn().await;
    t.end_turn().await;

    let listed: Vec<serde_json::Value> =
        t.h.get(&format!("/v1/events?session={}", t.session.id))
            .await;
    let listed = kinds(&listed);
    assert!(listed.iter().any(|k| k == "agent_message"), "{listed:?}");
    assert!(
        !listed
            .iter()
            .any(|k| k.ends_with("_chunk") || k == "tool_call_update"),
        "{listed:?}"
    );

    let mut streamed = Vec::new();
    loop {
        let (name, payload) = parse_sse(&common::next_sse_message(&mut domain).await);
        if name != "agent_event" {
            continue;
        }
        let kind = payload["kind"].as_str().unwrap().to_string();
        streamed.push(kind.clone());
        if kind == "stop" {
            break;
        }
    }
    assert!(
        streamed.iter().any(|k| k == "agent_message"),
        "{streamed:?}"
    );
    assert!(
        !streamed
            .iter()
            .any(|k| k.ends_with("_chunk") || k == "tool_call_update"),
        "{streamed:?}"
    );
}

/// `post_tool_use` carries the call as every update left it: the title and
/// input the call opened with, and the status, content and output its last
/// update set.
#[tokio::test]
async fn post_tool_use_carries_the_tool_call_merged_from_every_update() {
    let t = Streaming::start().await;
    t.begin_turn().await;
    t.end_turn().await;

    let events = t.snapshot().await;
    let done = events
        .iter()
        .find(|e| e["kind"] == "post_tool_use" && e["payload"]["acp"]["toolCallId"] == "call-2")
        .expect("the streaming turn's tool call ended");
    assert_eq!(done["payload"]["tool_name"], "Bash");
    assert_eq!(done["payload"]["tool_input"], json!({"command": "ls"}));
    let call = &done["payload"]["acp"];
    assert_eq!(call["title"], "Bash");
    assert_eq!(call["kind"], "execute");
    assert_eq!(call["status"], "completed");
    assert_eq!(call["rawInput"], json!({"command": "ls"}));
    assert_eq!(call["rawOutput"], json!({"stdout": "README.md\n"}));
    assert_eq!(call["content"][0]["content"]["text"], "README.md");
    assert!(call.get("sessionUpdate").is_none(), "{call}");
}

/// Console input is reported as what was typed, and as the console's.
#[tokio::test]
async fn console_input_is_reported_as_its_text_from_the_console() {
    let t = Streaming::start().await;
    t.begin_turn().await;
    t.end_turn().await;

    let events = t.snapshot().await;
    let prompts: Vec<&serde_json::Value> = events
        .iter()
        .filter(|e| e["kind"] == "user_prompt_submit")
        .collect();
    assert_eq!(prompts.len(), 2, "{events:?}");
    assert_eq!(prompts[0]["payload"]["source"], "daemon");
    assert_eq!(prompts[1]["payload"]["text"], "stream it");
    assert_eq!(prompts[1]["payload"]["source"], "console");
    assert!(
        prompts[1]["payload"]["prompt"]
            .as_str()
            .unwrap()
            .ends_with("\n\nstream it"),
        "{}",
        prompts[1]
    );
}

/// A client that opens the console mid-turn reads the text so far in its
/// snapshot: one thought chunk and one message chunk, after the stored
/// events, on the stream and on the plain snapshot alike.
#[tokio::test]
async fn a_stream_opened_mid_turn_gets_the_text_so_far_in_its_snapshot() {
    let t = Streaming::start().await;
    t.begin_turn().await;

    let mut body = t.h.stream(t.console_stream()).await;
    let snapshot = expect_sse(&mut body, "snapshot").await;
    let events = snapshot.as_array().unwrap().clone();
    let opened_on = kinds(&events);
    let tail = &opened_on[opened_on.len() - 2..];
    assert_eq!(
        tail,
        ["agent_thought_chunk", "agent_message_chunk"],
        "{opened_on:?}"
    );
    assert_eq!(text_of(&events, "agent_thought_chunk"), ["Think first."]);
    assert_eq!(text_of(&events, "agent_message_chunk"), ["Half done."]);

    let plain = t.snapshot().await;
    assert_eq!(text_of(&plain, "agent_message_chunk"), ["Half done."]);

    t.end_turn().await;
    let after = kinds(&t.snapshot().await);
    assert!(
        !after.iter().any(|k| k.ends_with("_chunk")),
        "the partial text goes once the turn is stored: {after:?}"
    );
}

/// Cancelling a running turn sends `session/cancel` to the agent and ends
/// the turn as `cancelled`, the text so far stored.
#[tokio::test]
async fn cancelling_a_running_turn_ends_it_as_cancelled() {
    let t = Streaming::start().await;
    t.begin_turn().await;

    let (status, _) =
        t.h.send(post(&format!(
            "/v1/sessions/{}/console/cancel",
            t.session.id
        )))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    eventually(TIMEOUT, "the cancelled turn to end", || async {
        t.h.session_status(&t.session).await == SessionStatus::Idle
    })
    .await;

    assert_eq!(t.stub.calls_of("session/cancel").len(), 1);
    let events = t.snapshot().await;
    let stop = events.iter().rfind(|e| e["kind"] == "stop").unwrap();
    assert_eq!(stop["payload"]["stop_reason"], "cancelled");
    assert_eq!(text_of(&events, "agent_message"), ["done", "Half done."]);
}

/// With no turn running there is nothing to cancel, and the call says so.
#[tokio::test]
async fn cancel_with_no_turn_running_is_refused() {
    let t = Streaming::start().await;

    let error =
        t.h.error(
            post(&format!("/v1/sessions/{}/console/cancel", t.session.id)),
            StatusCode::CONFLICT,
        )
        .await;
    assert_eq!(error.error.code, "conflict");
    assert!(t.stub.calls_of("session/cancel").is_empty());
}

/// The cancel endpoint is in the OpenAPI document, beside the console's
/// other three.
#[tokio::test]
async fn the_cancel_endpoint_is_in_the_openapi_document() {
    let h = harness().await;
    let doc: serde_json::Value = h.get("/api-docs/openapi.json").await;
    assert!(doc["paths"]["/v1/sessions/{id}/console/cancel"]["post"].is_object());
    assert!(doc["paths"]["/v1/sessions/{id}/console/input"]["post"].is_object());
}

/// A console client that falls behind on the stored events is told to
/// resync, and the connection closes right after — the live half does not
/// keep it open on a transcript with holes in it.
#[tokio::test]
async fn a_lagged_console_client_gets_a_resync_and_the_stream_ends() {
    let t = Streaming::start().await;
    // A tiny bus, so a handful of unread events overflows this subscriber.
    let bus = EventBus::with_capacity(2);
    let router = http::router(AppState {
        events: bus.clone(),
        ..t.h.state.clone()
    });

    let response = tower::ServiceExt::oneshot(router, t.console_stream())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let mut body = response.into_body();

    // Nothing reads the body yet, so these pile up in the subscriber's buffer.
    for i in 0..8 {
        bus.publish(BusEvent {
            event: DomainEvent::SkillDeleted(DeletedDto {
                id: format!("skill-{i}"),
            }),
            goal_id: None,
            task_id: None,
        });
    }

    expect_sse(&mut body, "snapshot").await;
    let message = next_sse_message(&mut body).await;
    assert!(
        message.contains("event: resync"),
        "a lagged client is told to resync, got: {message:?}"
    );
    sse_is_closed(&mut body).await;
}
