//! What the daemon stores of a turn, whole: each message with its content
//! blocks and identity, a running tool call's progress, a finished call's
//! input and raw output, and the end of each call a turn leaves open (021,
//! rules 6 and 7).
//!
//! The agent is the scriptable stub (`common::acp`). A held turn waits on
//! its `wait_for` file, so a test can read the daemon while a call is open.

use crate::common;

use axum::body::Body;
use axum::http::StatusCode;
use serde_json::{Value, json};

use ariadne_core::SessionStatus;
use ariadne_daemon::timeouts::Timeouts;

use common::acp::{StubAcpAgent, registry_home, script, stub_acp_agent};
use common::{Harness, RUNS_OUT, TIMEOUT, eventually, expect_sse, get, harness, post, post_json};

/// A session on the stub, idle after its first turn, whose second turn is
/// `turn`, started from the console.
struct Turned {
    dir: tempfile::TempDir,
    stub: StubAcpAgent,
    h: Harness,
    session: ariadne_store::AgentSession,
}

/// The turn the stub runs second: `updates`, then a hold on the `release`
/// file where `held` asks for one.
fn second_turn(dir: &std::path::Path, updates: Value, held: bool) -> Value {
    let mut scripted = script();
    let first = scripted["prompts"][0].clone();
    let mut turn = json!({"updates": updates, "stop_reason": "end_turn"});
    if held {
        turn["wait_for"] = json!(dir.join("release").display().to_string());
    }
    scripted["prompts"] = json!([first, turn]);
    scripted
}

impl Turned {
    async fn start(updates: Value, held: bool, timeouts: Timeouts) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let stub = stub_acp_agent(dir.path(), second_turn(dir.path(), updates, held));
        let h = harness()
            .home(registry_home(&stub))
            .timeouts(timeouts)
            .await;
        h.git_repo("repo");
        let cast = h.cast_pinned("stub:test-model").await;
        let session = h.start_agent(&cast.task, "develop").await;
        eventually(TIMEOUT, "the first turn to end", || async {
            h.session_status(&session).await == SessionStatus::Idle
        })
        .await;
        Self {
            dir,
            stub,
            h,
            session,
        }
    }

    /// Start the second turn from the console.
    async fn begin(&self) {
        let (status, _) = self
            .h
            .send(post_json(
                &format!("/v1/sessions/{}/console/input", self.session.id),
                json!({"text": "go on"}),
            ))
            .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
    }

    /// Let a held turn end.
    fn release(&self) {
        std::fs::write(self.dir.path().join("release"), "go").unwrap();
    }

    /// Wait until the second turn has stored its `stop`.
    async fn ended(&self) {
        eventually(TIMEOUT, "the second turn to end", || async {
            self.stored()
                .await
                .iter()
                .filter(|e| e["kind"] == "stop")
                .count()
                == 2
        })
        .await;
    }

    /// Every stored event of the session, as `GET /v1/events` lists it.
    async fn stored(&self) -> Vec<Value> {
        self.h
            .get(&format!("/v1/events?session={}", self.session.id))
            .await
    }

    async fn snapshot(&self) -> Vec<Value> {
        self.h
            .get(&format!("/v1/sessions/{}/console", self.session.id))
            .await
    }

    async fn console_stream(&self) -> Body {
        self.h
            .stream(get(&format!(
                "/v1/sessions/{}/console/stream",
                self.session.id
            )))
            .await
    }
}

/// The timeouts of these tests: the harness's own short waits on an ending
/// agent, and `tool_progress` as given.
fn with_tool_progress(tool_progress: std::time::Duration) -> Timeouts {
    Timeouts {
        cancel_grace: RUNS_OUT,
        turn_end_grace: RUNS_OUT,
        tool_progress,
        ..Timeouts::default()
    }
}

fn chunk(kind: &str, content: Value, message_id: Option<&str>) -> Value {
    let mut update = json!({"sessionUpdate": kind, "content": content});
    if let Some(id) = message_id {
        update["messageId"] = json!(id);
    }
    update
}

fn text(text: &str) -> Value {
    json!({"type": "text", "text": text})
}

fn of_kind<'a>(events: &'a [Value], kind: &str) -> Vec<&'a Value> {
    events.iter().filter(|e| e["kind"] == kind).collect()
}

fn kinds(events: &[Value]) -> Vec<&str> {
    events.iter().map(|e| e["kind"].as_str().unwrap()).collect()
}

/// The kinds the second turn stored, from its prompt to its `stop`.
fn second_turn_kinds(events: &[Value]) -> Vec<&str> {
    let kinds = kinds(events);
    let prompt = kinds
        .iter()
        .rposition(|kind| *kind == "user_prompt_submit")
        .unwrap();
    kinds[prompt..].to_vec()
}

/// A stored message keeps each content block the agent sent, in order, with
/// its `messageId`, an empty or adjacent text block among them; the text
/// field keeps the text alone. A thought and a prompt carry their blocks the
/// same way.
#[tokio::test]
async fn a_stored_message_keeps_each_content_block_in_order_and_its_message_id() {
    let image = json!({"type": "image", "mimeType": "image/png", "data": "iVBORw0KGgo="});
    let t = Turned::start(
        json!([
            chunk("agent_thought_chunk", text("Let me look."), None),
            chunk("agent_message_chunk", text("Look "), Some("msg-1")),
            chunk("agent_message_chunk", text("at this:"), Some("msg-1")),
            chunk("agent_message_chunk", text(""), Some("msg-1")),
            chunk("agent_message_chunk", image.clone(), Some("msg-1")),
            chunk("agent_message_chunk", text("Done."), Some("msg-1")),
        ]),
        false,
        Timeouts::default(),
    )
    .await;
    t.begin().await;
    t.ended().await;

    let events = t.stored().await;
    let message = *of_kind(&events, "agent_message").last().unwrap();
    assert_eq!(
        message["payload"]["content"],
        json!([
            text("Look "),
            text("at this:"),
            text(""),
            image,
            text("Done.")
        ]),
        "{message}"
    );
    assert_eq!(message["payload"]["text"], "Look at this:Done.");
    assert_eq!(message["payload"]["message_id"], "msg-1");
    assert!(
        message["payload"]["item_id"]
            .as_str()
            .is_some_and(|id| !id.is_empty())
    );

    let thought = *of_kind(&events, "agent_thought").last().unwrap();
    assert_eq!(thought["payload"]["content"], json!([text("Let me look.")]));
    assert_eq!(thought["payload"]["text"], "Let me look.");
    assert!(thought["payload"].get("message_id").is_none(), "{thought}");
    assert_ne!(thought["payload"]["item_id"], message["payload"]["item_id"]);

    let prompt = *of_kind(&events, "user_prompt_submit").last().unwrap();
    assert_eq!(prompt["payload"]["content"], json!([text("go on")]));
    assert_eq!(prompt["payload"]["text"], "go on");
    assert!(
        prompt["payload"]["item_id"]
            .as_str()
            .is_some_and(|id| !id.is_empty())
    );
}

/// A content block larger than 1 MB is stored as its type, MIME type and
/// size alone.
#[tokio::test]
async fn a_content_block_over_one_megabyte_is_stored_as_a_placeholder() {
    let data = "A".repeat(1_100_000);
    let t = Turned::start(
        json!([
            chunk("agent_message_chunk", text("Here:"), None),
            chunk(
                "agent_message_chunk",
                json!({"type": "image", "mimeType": "image/png", "data": data}),
                None,
            ),
        ]),
        false,
        Timeouts::default(),
    )
    .await;
    t.begin().await;
    t.ended().await;

    let events = t.stored().await;
    let message = *of_kind(&events, "agent_message").last().unwrap();
    assert_eq!(
        message["payload"]["content"],
        json!([
            text("Here:"),
            {"type": "image", "mime_type": "image/png", "size": 1_100_000, "omitted": true},
        ])
    );
}

/// Each live chunk of a run carries the `item_id` its stored whole carries,
/// a run of one empty chunk included.
#[tokio::test]
async fn each_live_chunk_and_its_stored_whole_carry_one_item_id() {
    let t = Turned::start(
        json!([
            chunk("agent_thought_chunk", text(""), None),
            chunk("agent_message_chunk", text("One "), None),
            chunk("agent_message_chunk", text("two."), None),
            {"sessionUpdate": "tool_call", "toolCallId": "call-9", "title": "Bash",
             "status": "pending", "rawInput": {"command": "ls"}},
        ]),
        false,
        Timeouts::default(),
    )
    .await;
    let mut body = t.console_stream().await;
    expect_sse(&mut body, "snapshot").await;
    t.begin().await;

    let mut seen = Vec::new();
    while seen
        .last()
        .is_none_or(|e: &Value| e["kind"] != "pre_tool_use")
    {
        seen.push(expect_sse(&mut body, "event").await);
    }
    let chunks = of_kind(&seen, "agent_message_chunk");
    assert_eq!(chunks.len(), 2, "{seen:?}");
    let whole = of_kind(&seen, "agent_message")[0];
    assert_eq!(
        whole["payload"]["content"],
        json!([text("One "), text("two.")])
    );
    let empty = of_kind(&seen, "agent_thought_chunk");
    let thought = of_kind(&seen, "agent_thought");
    assert_eq!((empty.len(), thought.len()), (1, 1), "{seen:?}");
    assert_eq!(thought[0]["payload"]["content"], json!([text("")]));
    assert_eq!(
        empty[0]["payload"]["item_id"],
        thought[0]["payload"]["item_id"]
    );
    assert_ne!(
        thought[0]["payload"]["item_id"],
        whole["payload"]["item_id"]
    );
    for chunk in chunks {
        assert_eq!(
            chunk["payload"]["item_id"], whole["payload"]["item_id"],
            "{chunk}"
        );
    }
    t.ended().await;
}

/// A running call's progress is stored once the interval since its last
/// stored event has passed, and `GET /v1/events` lists it with the call
/// merged so far.
#[tokio::test]
async fn a_running_calls_progress_is_stored_once_its_interval_has_passed() {
    let t = Turned::start(
        json!([
            {"sessionUpdate": "tool_call", "toolCallId": "call-9", "title": "Bash",
             "kind": "execute", "status": "pending", "rawInput": {"command": "cargo build"}},
            {"sessionUpdate": "tool_call_update", "toolCallId": "call-9", "status": "in_progress"},
            {"sessionUpdate": "tool_call_update", "toolCallId": "call-9",
             "content": [{"type": "content", "content": text("Compiling")}]},
            {"sessionUpdate": "tool_call_update", "toolCallId": "call-9", "status": "completed"},
        ]),
        false,
        with_tool_progress(std::time::Duration::ZERO),
    )
    .await;
    t.begin().await;
    t.ended().await;

    let events = t.stored().await;
    assert_eq!(
        second_turn_kinds(&events),
        [
            "user_prompt_submit",
            "pre_tool_use",
            "tool_call_update",
            "tool_call_update",
            "post_tool_use",
            "stop"
        ]
    );
    let progress = of_kind(&events, "tool_call_update");
    assert_eq!(progress[0]["payload"]["tool_call_id"], "call-9");
    assert_eq!(progress[0]["payload"]["session_id"], "stub-session");
    assert_eq!(progress[0]["payload"]["acp"]["status"], "in_progress");
    assert_eq!(progress[0]["payload"]["acp"]["title"], "Bash");
    let later = &progress[1]["payload"]["acp"];
    assert_eq!(later["status"], "in_progress");
    assert_eq!(later["content"][0]["content"]["text"], "Compiling");
}

/// Inside the interval a running call's progress goes live to the console
/// alone, and nothing of it is stored.
#[tokio::test]
async fn a_running_calls_progress_inside_its_interval_is_not_stored() {
    let t = Turned::start(
        json!([
            {"sessionUpdate": "tool_call", "toolCallId": "call-9", "title": "Bash",
             "kind": "execute", "status": "pending", "rawInput": {"command": "cargo build"}},
            {"sessionUpdate": "tool_call_update", "toolCallId": "call-9", "status": "in_progress"},
            {"sessionUpdate": "tool_call_update", "toolCallId": "call-9", "status": "completed"},
        ]),
        false,
        with_tool_progress(TIMEOUT),
    )
    .await;
    let mut body = t.console_stream().await;
    expect_sse(&mut body, "snapshot").await;
    t.begin().await;

    let mut seen = Vec::new();
    while seen
        .last()
        .is_none_or(|e: &Value| e["kind"] != "post_tool_use")
    {
        seen.push(expect_sse(&mut body, "event").await);
    }
    assert_eq!(of_kind(&seen, "tool_call_update").len(), 1, "{seen:?}");
    t.ended().await;
    let events = t.stored().await;
    assert_eq!(
        second_turn_kinds(&events),
        [
            "user_prompt_submit",
            "pre_tool_use",
            "post_tool_use",
            "stop"
        ]
    );
}

/// The interval starts again at each stored update: right after one is
/// stored, the next update goes live alone, and a later one is stored once a
/// whole interval has passed. The stub sends one update each time the test
/// asks, until a second one is stored.
#[tokio::test]
async fn the_interval_starts_again_at_each_stored_update() {
    let trigger = tempfile::tempdir().unwrap();
    let file = trigger.path().join("next");
    let progress: Vec<Value> = (0..2000)
        .map(|n| {
            json!({"sessionUpdate": "tool_call_update", "toolCallId": "call-9",
                   "status": "in_progress",
                   "content": [{"type": "content", "content": text(&format!("step {n}"))}]})
        })
        .collect();
    let dir = tempfile::tempdir().unwrap();
    let mut scripted = second_turn(
        dir.path(),
        json!([{"sessionUpdate": "tool_call", "toolCallId": "call-9", "title": "Bash",
                "status": "pending", "rawInput": {"command": "cargo build"}}]),
        true,
    );
    scripted["prompts"][1]["updates_on_trigger"] =
        json!({"file": file.display().to_string(), "updates": progress});
    let stub = stub_acp_agent(dir.path(), scripted);
    let h = harness()
        .home(registry_home(&stub))
        .timeouts(with_tool_progress(RUNS_OUT))
        .await;
    h.git_repo("repo");
    let cast = h.cast_pinned("stub:test-model").await;
    let session = h.start_agent(&cast.task, "develop").await;
    eventually(TIMEOUT, "the first turn to end", || async {
        h.session_status(&session).await == SessionStatus::Idle
    })
    .await;
    let t = Turned {
        dir,
        stub,
        h,
        session,
    };
    t.begin().await;
    let steps = |events: &[Value]| -> Vec<u64> {
        of_kind(events, "tool_call_update")
            .iter()
            .map(|e| {
                let text = e["payload"]["acp"]["content"][0]["content"]["text"]
                    .as_str()
                    .unwrap();
                text.trim_start_matches("step ").parse().unwrap()
            })
            .collect()
    };
    eventually(TIMEOUT, "two stored updates", || async {
        if !file.exists() {
            std::fs::write(&file, "1").unwrap();
        }
        steps(&t.stored().await).len() >= 2
    })
    .await;

    let stored = steps(&t.stored().await);
    assert_eq!(stored.len(), 2, "{stored:?}");
    assert!(
        stored[1] > stored[0] + 1,
        "the update after a stored one is inside the new interval: {stored:?}"
    );
    t.release();
    t.ended().await;
}

/// A turn that ends with calls still open stores one `tool_call_abandoned`
/// for each, in the order they opened, before its `stop`. Neither moves the
/// session off the idle its `stop` set.
#[tokio::test]
async fn a_turn_that_ends_with_open_calls_abandons_each() {
    let t = Turned::start(
        json!([
            {"sessionUpdate": "tool_call", "toolCallId": "call-8", "title": "Bash",
             "status": "pending", "rawInput": {"command": "ls"}},
            {"sessionUpdate": "tool_call", "toolCallId": "call-9", "title": "Read",
             "status": "pending", "rawInput": {"path": "README.md"}},
            {"sessionUpdate": "tool_call_update", "toolCallId": "call-8", "status": "in_progress"},
        ]),
        false,
        Timeouts::default(),
    )
    .await;
    t.begin().await;
    t.ended().await;

    let events = t.stored().await;
    assert_eq!(
        second_turn_kinds(&events),
        [
            "user_prompt_submit",
            "pre_tool_use",
            "pre_tool_use",
            "tool_call_abandoned",
            "tool_call_abandoned",
            "stop"
        ]
    );
    let abandoned = of_kind(&events, "tool_call_abandoned");
    assert_eq!(abandoned[0]["payload"]["tool_call_id"], "call-8");
    assert_eq!(abandoned[0]["payload"]["reason"], "turn_ended");
    assert_eq!(abandoned[0]["payload"]["session_id"], "stub-session");
    assert_eq!(abandoned[0]["payload"]["acp"]["status"], "in_progress");
    assert_eq!(abandoned[0]["payload"]["acp"]["title"], "Bash");
    assert_eq!(abandoned[1]["payload"]["tool_call_id"], "call-9");
    assert_eq!(abandoned[1]["payload"]["reason"], "turn_ended");
    eventually(TIMEOUT, "the session to go idle on its stop", || async {
        t.h.session_status(&t.session).await == SessionStatus::Idle
    })
    .await;
}

/// A cancelled turn abandons its open call as `cancelled`.
#[tokio::test]
async fn a_cancelled_turn_abandons_its_open_call_as_cancelled() {
    let t = Turned::start(
        json!([
            {"sessionUpdate": "tool_call", "toolCallId": "call-9", "title": "Bash",
             "status": "pending", "rawInput": {"command": "sleep 60"}},
        ]),
        true,
        Timeouts::default(),
    )
    .await;
    t.begin().await;
    eventually(TIMEOUT, "the call to open", || async {
        of_kind(&t.stored().await, "pre_tool_use").len() == 2
    })
    .await;

    let (status, _) =
        t.h.send(post(&format!(
            "/v1/sessions/{}/console/cancel",
            t.session.id
        )))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    t.ended().await;

    let events = t.stored().await;
    assert_eq!(
        second_turn_kinds(&events),
        [
            "user_prompt_submit",
            "pre_tool_use",
            "tool_call_abandoned",
            "stop"
        ]
    );
    let abandoned = of_kind(&events, "tool_call_abandoned")[0];
    assert_eq!(abandoned["payload"]["tool_call_id"], "call-9");
    assert_eq!(abandoned["payload"]["reason"], "cancelled");
    assert_eq!(
        abandoned["payload"]["acp"]["rawInput"]["command"],
        "sleep 60"
    );
}

/// An agent that ends mid-turn abandons its open call as `session_ended`,
/// before the session ends on the record.
#[tokio::test]
async fn an_agent_that_ends_mid_call_abandons_it_as_session_ended() {
    let mut scripted = script();
    scripted["prompts"] = json!([{
        "updates": [
            {"sessionUpdate": "tool_call", "toolCallId": "call-9", "title": "Bash",
             "status": "pending", "rawInput": {"command": "ls"}},
        ],
        "exit": 1,
    }]);
    let dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(dir.path(), scripted);
    let h = harness().home(registry_home(&stub)).await;
    h.git_repo("repo");
    let cast = h.cast_pinned("stub:test-model").await;
    let session = h.start_agent(&cast.task, "develop").await;
    eventually(TIMEOUT, "the session to retire", || async {
        h.session_status(&session).await == SessionStatus::Exited
    })
    .await;

    let events: Vec<Value> = h.get(&format!("/v1/events?session={}", session.id)).await;
    let kinds = kinds(&events);
    let at = |kind: &str| kinds.iter().position(|k| *k == kind);
    assert!(at("pre_tool_use") < at("tool_call_abandoned"), "{kinds:?}");
    assert!(at("tool_call_abandoned") < at("session_end"), "{kinds:?}");
    let abandoned = of_kind(&events, "tool_call_abandoned");
    assert_eq!(abandoned.len(), 1, "{kinds:?}");
    assert_eq!(abandoned[0]["payload"]["tool_call_id"], "call-9");
    assert_eq!(abandoned[0]["payload"]["reason"], "session_ended");
}

/// A console that opens during a call reads the open call, merged so far,
/// in its snapshot.
#[tokio::test]
async fn a_console_opened_during_a_call_reads_the_open_call() {
    let t = Turned::start(
        json!([
            {"sessionUpdate": "tool_call", "toolCallId": "call-9", "title": "Bash",
             "status": "pending", "rawInput": {"command": "cargo build"}},
            {"sessionUpdate": "tool_call_update", "toolCallId": "call-9", "status": "in_progress"},
        ]),
        true,
        Timeouts::default(),
    )
    .await;
    t.begin().await;
    let reached = t.dir.path().join("release.reached");
    let open_call = || async {
        let snapshot = t.snapshot().await;
        snapshot
            .iter()
            .find(|e| {
                e["kind"] == "tool_call_update" && e["payload"]["acp"]["status"] == "in_progress"
            })
            .cloned()
    };
    eventually(TIMEOUT, "the open call in the snapshot", || async {
        reached.exists() && open_call().await.is_some()
    })
    .await;

    let call = open_call().await.unwrap();
    assert_eq!(call["payload"]["tool_call_id"], "call-9");
    assert_eq!(call["payload"]["acp"]["title"], "Bash");
    assert_eq!(call["payload"]["acp"]["rawInput"]["command"], "cargo build");

    let mut body = t.console_stream().await;
    let snapshot = expect_sse(&mut body, "snapshot").await;
    assert!(
        snapshot.to_string().contains("\"tool_call_id\":\"call-9\""),
        "{snapshot}"
    );
    t.release();
    t.ended().await;
    assert!(
        !t.snapshot()
            .await
            .iter()
            .any(|e| e["kind"] == "tool_call_update"),
        "a call the turn ended is no longer open"
    );
    let _ = &t.stub;
}
