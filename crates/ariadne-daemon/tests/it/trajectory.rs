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

    // The held run's whole is stored only once the turn is let go, so its
    // chunks reach the stream while it streams.
    t.begin_turn().await;
    let mut seen = Vec::new();
    while !ops(&seen, "text_append")
        .iter()
        .any(|op| op["text"] == "good.")
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
        .find(|op| op["text"] == "good.")
        .unwrap()
        .clone();
    let opened = ops(&seen, "item_upsert")
        .into_iter()
        .find(|op| op["step"]["id"] == append["item_id"])
        .expect("the append's step was opened by an upsert");
    assert_eq!(opened["step"]["type"], "message");
    assert_eq!(opened["step"]["streaming"], true);
    assert_eq!(opened["step"]["content"][0]["text"], "All ");
}

/// Two subscribers of one session share one fold: the same snapshot
/// revision, then the same revisions with the same ops.
#[tokio::test]
async fn two_subscribers_receive_the_same_rev_sequence() {
    let t = Held::start().await;
    // Both open at once on a session no one follows yet: one hub opens.
    let (mut first, mut second) = tokio::join!(t.stream(), t.stream());
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

/// The state names the seat, the step, the host, the agent, the model and
/// the effort of the session. A switch to another model of the same agent
/// reaches the stream as a patch.
#[tokio::test]
async fn the_session_state_names_where_it_runs_and_a_switch_patches_it() {
    let t = Held::start().await;
    let snapshot = t.trajectory("").await;
    let state = &snapshot["session"];
    assert_eq!(state["seat"], "agent");
    assert_eq!(state["step"], "develop");
    assert_eq!(state["host"], "local", "the task has no other host");
    assert_eq!(state["agent"], "stub");
    assert_eq!(state["model"], "test-model");
    assert_eq!(
        state["effort"], "low",
        "the effort option's value, for a pin with no effort"
    );

    let mut body = t.stream().await;
    expect_sse(&mut body, "snapshot").await;
    let (status, _) =
        t.h.send(post_json(
            &format!("/v1/sessions/{}/switch", t.session.id),
            json!({"model": "stub:other-model", "effort": "high"}),
        ))
        .await;
    assert_eq!(status, StatusCode::OK);
    loop {
        let update = expect_sse(&mut body, "update").await;
        let patched = ops(&[update], "session_patch")
            .into_iter()
            .map(|op| op["session"].clone())
            .find(|session| session["model"] == "other-model" && session["effort"] == "high");
        if let Some(session) = patched {
            assert_eq!(session["agent"], "stub");
            assert_eq!(session["seat"], "agent");
            assert_eq!(session["step"], "develop");
            assert_eq!(session["host"], "local");
            return;
        }
    }
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

/// A run of text the agent streams has one id across the opens of the
/// stream: its `item_id`, which its stored whole names too. The stream, a
/// reopen and the steps route name it the same, and its text reads whole.
#[tokio::test]
async fn a_streamed_run_keeps_its_id_across_opens_and_ends_on_its_stored_id() {
    let t = Held::start().await;
    let mut body = t.stream().await;
    expect_sse(&mut body, "snapshot").await;
    t.begin_turn().await;
    let mut seen = Vec::new();
    while !ops(&seen, "text_append")
        .iter()
        .any(|op| op["text"] == "good.")
    {
        seen.push(expect_sse(&mut body, "update").await);
    }
    let streaming = ops(&seen, "text_append")
        .into_iter()
        .find(|op| op["text"] == "good.")
        .unwrap()["item_id"]
        .clone();
    drop(body);

    // Opened again, after the hub that streamed the run is gone.
    let mut again = t.stream().await;
    let snapshot = expect_sse(&mut again, "snapshot").await;
    let running = snapshot["turns"]
        .as_array()
        .unwrap()
        .iter()
        .find(|turn| turn["state"] == "running")
        .unwrap_or_else(|| panic!("the held turn runs: {snapshot}"));
    let step = running["steps"].as_array().unwrap().last().unwrap();
    assert_eq!(step["id"], streaming, "the same id on the new open");
    assert_eq!(step["content"][0]["text"], "All good.");

    t.release();
    let updates = updates_until_closed(&mut again).await;
    let closed = ops(&updates, "item_upsert")
        .into_iter()
        .find(|op| op["step"]["type"] == "message" && op["step"]["streaming"] == false)
        .expect("the whole closes the run")["step"]
        .clone();
    assert_eq!(closed["id"], streaming, "the whole closes the same step");
    assert_eq!(
        closed["content"],
        json!([{"type": "text", "text": "All good."}])
    );
    let turn_id = running["id"].as_str().unwrap();
    let page: Value =
        t.h.get(&t.path(&format!("/turns/{turn_id}/steps?limit=1")))
            .await;
    assert_eq!(
        page["steps"][0]["id"], streaming,
        "the steps route names it the same"
    );
    assert_eq!(page["steps"][0]["content"], closed["content"]);
}

/// A cold open of 101 waiting questions holds the newest 100. An answer to
/// one of them is one revision that also brings the 101st in: no client
/// sees, or joins on, a turn of 99 (040, rule 16).
#[tokio::test]
async fn an_answer_and_the_refill_it_calls_for_are_one_revision() {
    let h = harness().await;
    let cast = h.cast().await;
    let session = h.agent_session(&cast, "develop").await;
    let store_event = |kind: &str, payload: Value| ariadne_store::NewAgentEvent {
        session_id: Some(session.id.clone()),
        task_id: Some(cast.task.id.clone()),
        kind: kind.into(),
        payload,
    };
    h.store
        .create_event(store_event("user_prompt_submit", json!({"text": "Ask"})))
        .await
        .unwrap();
    for n in 0..=100 {
        h.store
            .create_event(store_event(
                "question",
                json!({"question": {"id": format!("q-{n}"), "message": "Which?", "schema": {}}}),
            ))
            .await
            .unwrap();
    }
    let mut body = h
        .stream(get(&format!(
            "/v1/sessions/{}/trajectory/stream",
            session.id
        )))
        .await;
    let snapshot = expect_sse(&mut body, "snapshot").await;
    let turn = &snapshot["turns"][0];
    assert_eq!(turn["attention"].as_array().unwrap().len(), 100);
    assert_eq!(turn["attention_more"], 1);

    h.store
        .create_event(store_event(
            "question.resolved",
            json!({"question_id": "q-100", "action": "decline", "closed_by": "user"}),
        ))
        .await
        .unwrap();
    let update = expect_sse(&mut body, "update").await;

    assert_eq!(update["rev"], snapshot["rev"].as_u64().unwrap() + 1);
    let upserts: Vec<&Value> = update["ops"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|op| op["op"] == "turn_upsert")
        .collect();
    let last = upserts.last().expect("the revision updates the turn");
    assert_eq!(last["turn"]["attention"].as_array().unwrap().len(), 100);
    assert_eq!(last["turn"]["attention_more"], 0);
    assert!(
        update["ops"]
            .as_array()
            .unwrap()
            .iter()
            .any(|op| op["op"] == "item_upsert" && op["step"]["request_id"] == "q-0"),
        "the oldest question comes in within the same revision: {update}"
    );
    let joined: Value = h
        .get(&format!("/v1/sessions/{}/trajectory", session.id))
        .await;
    assert_eq!(joined["rev"], update["rev"]);
    assert_eq!(
        joined["turns"][0]["attention"].as_array().unwrap().len(),
        100
    );
}

/// The detail of a call: a running call shows its latest stored progress,
/// an ended one its command, directory, output and exit code, and the raw
/// data comes only with `raw=true`. A progress row of the call names no
/// item (040, rules 49 to 52, 54).
#[tokio::test]
async fn a_call_detail_gives_its_command_its_progress_and_raw_data_on_demand() {
    let h = harness().await;
    let session = h.lone_session("call-detail").await;
    let store_event = |kind: &str, payload: Value| ariadne_store::NewAgentEvent {
        session_id: Some(session.id.clone()),
        task_id: None,
        kind: kind.into(),
        payload,
    };
    h.store
        .create_event(store_event("user_prompt_submit", json!({"text": "Build"})))
        .await
        .unwrap();
    let input = json!({"command": "cargo build\n  --release", "cwd": "/work"});
    let call = json!({"toolCallId": "call-1", "title": "Bash", "kind": "execute",
                      "status": "pending", "rawInput": input});
    let opener = h
        .store
        .create_event(store_event(
            "pre_tool_use",
            json!({"tool_name": "Bash", "tool_input": input, "acp": call}),
        ))
        .await
        .unwrap();
    let mut progress = call.clone();
    progress["status"] = "in_progress".into();
    progress["content"] =
        json!([{"type": "content", "content": {"type": "text", "text": "Compiling a"}}]);
    let progress_row = h
        .store
        .create_event(store_event(
            "tool_call_update",
            json!({"tool_call_id": "call-1", "acp": progress}),
        ))
        .await
        .unwrap();
    let path = format!("/v1/sessions/{}/trajectory/items/{}", session.id, opener.id);

    let running: Value = h.get(&path).await;

    assert_eq!(running["status"], "running");
    assert_eq!(running["detail_rev"], 1);
    assert_eq!(running["detail"]["type"], "command");
    assert_eq!(running["detail"]["output"], "Compiling a");
    assert!(running["detail"]["exit_code"].is_null());

    let raw_output = json!({"stdout": "Compiling a\nFinished", "stderr": "", "exit_code": 101});
    h.store
        .create_event(store_event(
            "post_tool_use",
            json!({"tool_name": "Bash", "tool_input": input,
                   "acp": {"toolCallId": "call-1", "title": "Bash", "kind": "execute",
                           "status": "failed", "content": [], "rawOutput": raw_output}}),
        ))
        .await
        .unwrap();

    let ended: Value = h.get(&path).await;

    assert_eq!(ended["status"], "failed");
    assert_eq!(ended["detail_rev"], 2);
    assert_eq!(
        ended["detail"],
        json!({"type": "command", "command": "cargo build\n  --release", "cwd": "/work",
               "output": "Compiling a\nFinished", "exit_code": 101, "truncated": false})
    );
    assert!(ended.get("raw").is_none(), "no raw data unasked: {ended}");
    let refused: Value = h.get(&format!("{path}?raw=false")).await;
    assert!(
        refused.get("raw").is_none(),
        "no raw data refused: {refused}"
    );
    let raw: Value = h.get(&format!("{path}?raw=true")).await;
    assert_eq!(
        raw["raw"],
        json!({"title": "Bash", "input": input, "output": raw_output})
    );
    let (status, _) = h
        .send(get(&format!(
            "/v1/sessions/{}/trajectory/items/{}",
            session.id, progress_row.id
        )))
        .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "a progress row of an open call opens no step"
    );
}

/// A call's content block with bytes names a blob, and the blob route
/// answers its bytes with its MIME type. An unknown item, an item that is
/// no call, and an unknown blob are each a 404 (040, rules 53, 54).
#[tokio::test]
async fn a_blob_answers_the_bytes_of_its_block_and_unknown_ids_are_not_found() {
    let h = harness().await;
    let session = h.lone_session("call-blob").await;
    let store_event = |kind: &str, payload: Value| ariadne_store::NewAgentEvent {
        session_id: Some(session.id.clone()),
        task_id: None,
        kind: kind.into(),
        payload,
    };
    let prompt = h
        .store
        .create_event(store_event("user_prompt_submit", json!({"text": "Look"})))
        .await
        .unwrap();
    let opener = h
        .store
        .create_event(store_event(
            "pre_tool_use",
            json!({"tool_name": "Screenshot",
                   "acp": {"toolCallId": "call-1", "title": "Screenshot", "kind": "other",
                           "status": "completed",
                           "content": [
                               {"type": "content", "content": {"type": "text", "text": "Taken"}},
                               {"type": "content", "content": {"type": "image",
                                "mimeType": "image/png", "data": "aGVsbG8="}}]}}),
        ))
        .await
        .unwrap();
    let base = format!("/v1/sessions/{}/trajectory", session.id);

    let detail: Value = h.get(&format!("{base}/items/{}", opener.id)).await;

    assert_eq!(detail["detail"]["type"], "generic");
    assert_eq!(detail["detail"]["content"][0]["text"], "Taken");
    let blob = detail["detail"]["content"][1]["blob"].as_str().unwrap();
    assert_eq!(blob, format!("{base}/blobs/{}/1", opener.id));
    let response = h.response(get(blob)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "image/png");
    assert_eq!(
        response.headers()["content-security-policy"],
        "sandbox; default-src 'none'"
    );
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
    let bytes = http_body_util::BodyExt::collect(response.into_body())
        .await
        .unwrap()
        .to_bytes();
    assert_eq!(&bytes[..], b"hello");

    for missing in [
        format!("{base}/items/no-such-item"),
        format!("{base}/items/{}", prompt.id),
        format!("{base}/blobs/{}/0", opener.id),
        format!("{base}/blobs/{}/7", opener.id),
        format!("{base}/blobs/no-such-event/0"),
    ] {
        let (status, _) = h.send(get(&missing)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{missing}");
    }
}

/// A stop that ends a turn over an open call interrupts the call: its step
/// and its detail give the same status and the same `detail_rev`
/// (040, rules 12, 49).
#[tokio::test]
async fn a_call_its_turn_interrupts_has_one_revision_in_its_step_and_its_detail() {
    let h = harness().await;
    let session = h.lone_session("call-interrupted").await;
    let store_event = |kind: &str, payload: Value| ariadne_store::NewAgentEvent {
        session_id: Some(session.id.clone()),
        task_id: None,
        kind: kind.into(),
        payload,
    };
    let prompt = h
        .store
        .create_event(store_event("user_prompt_submit", json!({"text": "Run"})))
        .await
        .unwrap();
    let opener = h
        .store
        .create_event(store_event(
            "pre_tool_use",
            json!({"tool_name": "Bash", "tool_input": {"command": "sleep 9"},
                   "acp": {"toolCallId": "call-1", "title": "Bash", "kind": "execute",
                           "status": "pending", "rawInput": {"command": "sleep 9"}}}),
        ))
        .await
        .unwrap();
    h.store
        .create_event(store_event("stop", json!({"stop_reason": "end_turn"})))
        .await
        .unwrap();
    let base = format!("/v1/sessions/{}/trajectory", session.id);

    let page: Value = h.get(&format!("{base}/turns/{}/steps", prompt.id)).await;
    let detail: Value = h.get(&format!("{base}/items/{}", opener.id)).await;

    let step = page["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|step| step["id"] == opener.id.as_str())
        .unwrap_or_else(|| panic!("the call's step: {page}"));
    assert_eq!(step["status"], "interrupted");
    assert_eq!(detail["status"], "interrupted");
    assert_eq!(
        step["detail_rev"], 1,
        "the end of the turn changed the call"
    );
    assert_eq!(detail["detail_rev"], step["detail_rev"]);
}
