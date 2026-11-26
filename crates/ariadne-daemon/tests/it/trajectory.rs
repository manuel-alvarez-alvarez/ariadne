//! The trajectory routes (039): the snapshot, its stream, and the pages of
//! older turns, of a turn's steps and of a subagent's steps, against the
//! scriptable stub agent.

use crate::common;

use axum::body::Body;
use axum::http::StatusCode;
use serde_json::{Value, json};

use ariadne_core::SessionStatus;
use ariadne_store::AgentPin;

use common::acp::{StubAcpAgent, registry_home, script, stub_acp_agent};
use common::{Harness, TIMEOUT, eventually, expect_sse, get, harness, post_json};

/// The child the second turn's agent starts.
const CHILD: &str = "child-1";

/// A second turn that thinks, streams a message in two chunks, starts a
/// subagent that reads a file, makes a call of its own, says more, and
/// holds on `wait_for` until the test lets it end.
fn held_script(wait_for: &std::path::Path) -> Value {
    let mut scripted = script();
    let first_turn = scripted["prompts"][0].clone();
    scripted["prompts"] = json!([
        first_turn,
        {
            "updates": [
                {"sessionUpdate": "agent_thought_chunk", "content": {"type": "text", "text": "Think."}},
                {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "Half "}},
                {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "done."}},
                {"sessionUpdate": "subagent_spawned", "subagentSessionId": CHILD,
                 "name": "explorer", "task": "Look around", "capabilities": {}},
                {"sessionUpdate": "tool_call", "toolCallId": "c-1", "title": "Read",
                 "kind": "read", "status": "pending", "rawInput": {"path": "README.md"},
                 "as_session": CHILD},
                {"sessionUpdate": "tool_call_update", "toolCallId": "c-1", "status": "completed",
                 "as_session": CHILD},
                {"sessionUpdate": "subagent_state_update", "subagentSessionId": CHILD,
                 "state": "completed"},
                {"sessionUpdate": "tool_call", "toolCallId": "call-2", "title": "Bash",
                 "kind": "execute", "status": "pending", "rawInput": {"command": "ls"}},
                {"sessionUpdate": "tool_call_update", "toolCallId": "call-2", "status": "completed",
                 "content": [{"type": "content", "content": {"type": "text", "text": "README.md"}}]},
                {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "All "}},
                {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "good."}},
            ],
            "wait_for": wait_for.display().to_string(),
            "stop_reason": "end_turn",
        },
    ]);
    scripted
}

/// The stub, a session idle after its first turn, and the file that lets
/// the second turn end.
struct Held {
    _agent_dir: tempfile::TempDir,
    _stub: StubAcpAgent,
    h: Harness,
    session: ariadne_store::AgentSession,
    wait_for: std::path::PathBuf,
}

impl Held {
    async fn start() -> Self {
        let agent_dir = tempfile::tempdir().unwrap();
        let wait_for = agent_dir.path().join("held-turn");
        let stub = stub_acp_agent(agent_dir.path(), held_script(&wait_for));
        let h = harness().home(registry_home(&stub)).await;
        h.git_repo("repo");
        let cast = h.cast_pinned("stub:test-model").await;
        h.store
            .set_agent_pin(
                &cast.develop().id,
                &AgentPin {
                    model: "stub:test-model".into(),
                    effort: None,
                },
            )
            .await
            .unwrap();
        let session = h.start_agent(&cast.task, "develop").await;
        eventually(TIMEOUT, "the first turn to end", || async {
            h.session_status(&session).await == SessionStatus::Idle
        })
        .await;
        Self {
            _agent_dir: agent_dir,
            _stub: stub,
            h,
            session,
            wait_for,
        }
    }

    /// Send the prompt that starts the held turn.
    async fn begin_turn(&self) {
        let (status, _) = self
            .h
            .send(post_json(
                &format!("/v1/sessions/{}/console/input", self.session.id),
                json!({"text": "go on"}),
            ))
            .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
    }

    fn release(&self) {
        std::fs::write(&self.wait_for, "go").unwrap();
    }

    /// Run the held turn to its end.
    async fn run_turn(&self) {
        self.release();
        self.begin_turn().await;
        eventually(TIMEOUT, "the held turn to end", || async {
            let snapshot = self.trajectory("").await;
            snapshot["turns"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|turn| turn["prompt"].is_object())
                .count()
                == 2
                && snapshot["session"]["running_turn_id"].is_null()
        })
        .await;
    }

    fn path(&self, rest: &str) -> String {
        format!("/v1/sessions/{}/trajectory{rest}", self.session.id)
    }

    async fn trajectory(&self, query: &str) -> Value {
        self.h.get(&self.path(query)).await
    }

    async fn stream(&self) -> Body {
        self.h.stream(get(&self.path("/stream"))).await
    }
}

/// Read `update` frames until one closes the turn `turn`, or, with no turn
/// named, any turn but the first; answer each frame read.
async fn updates_until_closed(body: &mut Body) -> Vec<Value> {
    let mut seen = Vec::new();
    loop {
        let update = expect_sse(body, "update").await;
        let closed = update["ops"].as_array().unwrap().iter().any(|op| {
            op["op"] == "turn_upsert"
                && op["turn"]["prompt"]["content"][0]["text"] == "go on"
                && op["turn"]["state"] == "completed"
        });
        seen.push(update);
        if closed {
            return seen;
        }
    }
}

fn ops<'a>(updates: &'a [Value], op: &str) -> Vec<&'a Value> {
    updates
        .iter()
        .flat_map(|update| update["ops"].as_array().unwrap())
        .filter(|candidate| candidate["op"] == op)
        .collect()
}

/// The stream opens on one snapshot and follows it with `update` frames,
/// each one revision on; a message's later chunks arrive as appends onto
/// the step its first chunk opened.
#[tokio::test]
async fn the_stream_sends_a_snapshot_then_updates_and_text_as_appends() {
    let t = Held::start().await;
    let mut body = t.stream().await;
    let snapshot = expect_sse(&mut body, "snapshot").await;
    let rev = snapshot["rev"].as_u64().unwrap();

    t.begin_turn().await;
    let mut seen = Vec::new();
    while !ops(&seen, "text_append")
        .iter()
        .any(|op| op["text"] == "done.")
    {
        seen.push(expect_sse(&mut body, "update").await);
    }
    t.release();
    seen.extend(updates_until_closed(&mut body).await);

    let revs: Vec<u64> = seen.iter().map(|u| u["rev"].as_u64().unwrap()).collect();
    let expected: Vec<u64> = (rev + 1..=rev + revs.len() as u64).collect();
    assert_eq!(
        revs, expected,
        "one revision after another, from the snapshot's"
    );
    let append = ops(&seen, "text_append")
        .into_iter()
        .find(|op| op["text"] == "done.")
        .unwrap()
        .clone();
    let opened = ops(&seen, "item_upsert")
        .into_iter()
        .find(|op| op["step"]["id"] == append["item_id"])
        .expect("the append's step was opened by an upsert");
    assert_eq!(opened["step"]["type"], "message");
    assert_eq!(opened["step"]["streaming"], true);
    assert_eq!(opened["step"]["content"][0]["text"], "Half ");
}

/// Two subscribers of one session share one fold: the same snapshot
/// revision, then the same revisions with the same ops.
#[tokio::test]
async fn two_subscribers_receive_the_same_rev_sequence() {
    let t = Held::start().await;
    let mut first = t.stream().await;
    let mut second = t.stream().await;
    let first_snapshot = expect_sse(&mut first, "snapshot").await;
    let second_snapshot = expect_sse(&mut second, "snapshot").await;
    assert_eq!(first_snapshot["rev"], second_snapshot["rev"]);

    t.release();
    t.begin_turn().await;
    let first_updates = updates_until_closed(&mut first).await;
    let second_updates = updates_until_closed(&mut second).await;

    assert!(!first_updates.is_empty());
    assert_eq!(first_updates, second_updates);
}

/// A closed turn is its prompt, its counts and its answer; its steps and
/// the turns older than the newest come in pages.
#[tokio::test]
async fn older_turns_and_the_steps_of_a_turn_come_in_pages() {
    let t = Held::start().await;
    t.run_turn().await;

    let newest = t.trajectory("?limit=1").await;
    let [turn] = newest["turns"].as_array().unwrap().as_slice() else {
        panic!("one turn: {newest}");
    };
    assert_eq!(newest["has_older"], true);
    assert_eq!(turn["state"], "completed");
    assert_eq!(turn["steps"], json!([]), "a closed turn carries no steps");
    assert_eq!(turn["answer"][0]["text"], "All good.");
    assert_eq!(turn["activity"]["tool_calls"], 1);
    assert_eq!(turn["activity"]["subagents"], 1);
    assert_eq!(turn["activity"]["thoughts"], 1);
    let earlier = turn["earlier_steps"].as_u64().unwrap();
    assert!(earlier >= 4, "{turn}");

    let turn_id = turn["id"].as_str().unwrap();
    let older = t.trajectory(&format!("?before={turn_id}&limit=1")).await;
    let [previous] = older["turns"].as_array().unwrap().as_slice() else {
        panic!("one older turn: {older}");
    };
    assert_ne!(previous["id"], turn["id"]);
    assert!(previous["id"].as_str().unwrap() < turn_id);
    assert_eq!(previous["answer"][0]["text"], "done");

    let page: Value =
        t.h.get(&t.path(&format!("/turns/{turn_id}/steps?limit=1")))
            .await;
    let [last] = page["steps"].as_array().unwrap().as_slice() else {
        panic!("one step: {page}");
    };
    assert_eq!(last["type"], "message");
    assert_eq!(last["content"][0]["text"], "All good.");
    assert_eq!(page["earlier_steps"], earlier - 1);
    let before: Value =
        t.h.get(&t.path(&format!(
            "/turns/{turn_id}/steps?limit=1&before={}",
            last["id"].as_str().unwrap()
        )))
        .await;
    assert_eq!(before["steps"][0]["type"], "tool_call");
    assert_eq!(before["earlier_steps"], earlier - 2);
}

/// A subagent is a `subagent` step in its parent's turn, and `?subagent=`
/// answers the steps it took itself.
#[tokio::test]
async fn a_subagent_is_a_step_of_its_parent_and_its_steps_are_a_query_away() {
    let t = Held::start().await;
    t.run_turn().await;

    let snapshot = t.trajectory("").await;
    let [child] = snapshot["subagents"].as_array().unwrap().as_slice() else {
        panic!("one subagent: {snapshot}");
    };
    assert_eq!(child["subagent_id"], CHILD);
    assert_eq!(child["title"], "explorer");
    assert_eq!(child["state"], "completed");
    assert_eq!(child["activity"]["tool_calls"], 1);
    let turn_id = child["turn_id"].as_str().unwrap();
    let page: Value = t.h.get(&t.path(&format!("/turns/{turn_id}/steps"))).await;
    assert!(
        page["steps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|step| step["type"] == "subagent" && step["subagent_id"] == CHILD),
        "the parent's turn holds the subagent step: {page}"
    );

    let own = t.trajectory(&format!("?subagent={CHILD}")).await;
    let [turn] = own["turns"].as_array().unwrap().as_slice() else {
        panic!("the one turn the subagent ran in: {own}");
    };
    let [read] = turn["steps"].as_array().unwrap().as_slice() else {
        panic!("the subagent's one call: {turn}");
    };
    assert_eq!(read["type"], "tool_call");
    assert_eq!(read["lane"], CHILD);
    assert_eq!(read["tool_call_id"], "c-1");
}
