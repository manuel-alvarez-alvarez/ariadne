//! The trajectory of the session state an ACP agent reports (040): plans,
//! notices, compactions, content blocks with bytes, the progress and the end
//! of a call, and the session's display state. Each test drives the stub
//! agent, so the runtime, the store and the fold are all in it, and reads
//! the result through the trajectory routes.

use crate::common;

use axum::body::Body;
use axum::http::StatusCode;
use serde_json::{Value, json};

use ariadne_core::SessionStatus;
use ariadne_daemon::timeouts::Timeouts;

use common::acp::{StubAcpAgent, registry_home, script, stub_acp_agent};
use common::{TIMEOUT, eventually, expect_sse, get, harness, post, post_json};

/// The text the test prompts with, after the stub's first turn.
const PROMPT: &str = "go on";

fn text(text: &str) -> Value {
    json!({"type": "text", "text": text})
}

/// A turn that sends `updates` and ends.
fn turn(updates: Value) -> Value {
    json!({"updates": updates, "stop_reason": "end_turn"})
}

/// A session on the stub, idle after the stub's first turn, whose later
/// turns are the ones the test gave.
struct Session {
    dir: tempfile::TempDir,
    _stub: StubAcpAgent,
    h: common::Harness,
    session: ariadne_store::AgentSession,
}

impl Session {
    async fn start(turns: Vec<Value>) -> Self {
        Self::start_with(turns, Timeouts::default()).await
    }

    /// [`Session::start`], with the daemon's `timeouts`. A turn with
    /// `"held": true` waits on [`Session::release`] once it has said
    /// everything.
    async fn start_with(turns: Vec<Value>, timeouts: Timeouts) -> Self {
        Self::start_on(turns, timeouts, |_| {}).await
    }

    /// [`Session::start_with`], with a `change` of the stub's script.
    async fn start_on(
        turns: Vec<Value>,
        timeouts: Timeouts,
        change: impl FnOnce(&mut Value),
    ) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let release = dir.path().join("release").display().to_string();
        let mut scripted = script();
        let first = scripted["prompts"][0].clone();
        scripted["prompts"] = std::iter::once(first)
            .chain(turns.into_iter().map(|mut turn| {
                if turn.get("held").is_some() {
                    turn["wait_for"] = json!(release);
                }
                turn
            }))
            .collect();
        change(&mut scripted);
        let stub = stub_acp_agent(dir.path(), scripted);
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
            _stub: stub,
            h,
            session,
        }
    }

    /// Let a held turn end.
    fn release(&self) {
        std::fs::write(self.dir.path().join("release"), "go").unwrap();
    }

    /// Prompt the next turn from the console.
    async fn prompt(&self) {
        let (status, _) = self
            .h
            .send(post_json(
                &format!("/v1/sessions/{}/console/input", self.session.id),
                json!({"text": PROMPT}),
            ))
            .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
    }

    /// Every stored event of the session, as `GET /v1/events` lists it.
    async fn stored(&self) -> Vec<Value> {
        self.h
            .get(&format!("/v1/events?session={}", self.session.id))
            .await
    }

    /// Wait until the session has stored `count` stops.
    async fn stops(&self, count: usize) {
        eventually(TIMEOUT, "the turn to end", || async {
            self.h
                .store
                .count_session_events(&self.session.id, "stop")
                .await
                .unwrap()
                == count as u64
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

    /// The steps route of the turn `turn_id`: a fold the daemon builds
    /// again from the store.
    async fn steps(&self, turn_id: &str) -> Vec<Value> {
        let page: Value = self
            .h
            .get(&self.path(&format!("/turns/{turn_id}/steps?limit=200")))
            .await;
        page["steps"].as_array().unwrap().clone()
    }

    /// The id of the newest turn the prompt opened.
    async fn prompted_turn(&self) -> String {
        let snapshot = self.trajectory("").await;
        snapshot["turns"]
            .as_array()
            .unwrap()
            .iter()
            .rfind(|turn| turn["prompt"]["content"][0]["text"] == PROMPT)
            .unwrap_or_else(|| panic!("a prompted turn: {snapshot}"))["id"]
            .as_str()
            .unwrap()
            .to_string()
    }
}

fn of_kind<'a>(events: &'a [Value], kind: &str) -> Vec<&'a Value> {
    events.iter().filter(|e| e["kind"] == kind).collect()
}

fn ops<'a>(updates: &'a [Value], op: &str) -> Vec<&'a Value> {
    updates
        .iter()
        .flat_map(|update| update["ops"].as_array().unwrap())
        .filter(|candidate| candidate["op"] == op)
        .collect()
}

/// Read `update` frames until a `turn_upsert` closes the prompted turn.
async fn until_closed(body: &mut Body) -> Vec<Value> {
    let mut seen = Vec::new();
    loop {
        let update = expect_sse(body, "update").await;
        let closed = update["ops"].as_array().unwrap().iter().any(|op| {
            op["op"] == "turn_upsert"
                && op["turn"]["prompt"]["content"][0]["text"] == PROMPT
                && op["turn"]["state"] != "running"
        });
        seen.push(update);
        if closed {
            return seen;
        }
    }
}

/// The newest copy of each step the ops of a stream upserted, by id.
fn newest_steps(updates: &[Value]) -> std::collections::BTreeMap<String, Value> {
    ops(updates, "item_upsert")
        .into_iter()
        .map(|op| {
            (
                op["step"]["id"].as_str().unwrap().to_string(),
                op["step"].clone(),
            )
        })
        .collect()
}

/// The updates of one `plan_id` give one plan that changes in place; a plan
/// of the old `plan` kind is one fixed plan; a `plan_removed` marks its plan
/// removed. Each plan keeps the id of its first stored event, in the stream
/// and in a fold the daemon builds again from the store.
#[tokio::test]
async fn the_updates_of_a_plan_change_it_in_place_and_a_removed_plan_is_marked() {
    let entry = |content: &str, status: &str| json!({"content": content, "priority": "high", "status": status});
    let t = Session::start(vec![
        turn(json!([
            {"sessionUpdate": "plan", "entries": [entry("Old", "pending")]},
            {"sessionUpdate": "plan_update",
             "plan": {"type": "items", "planId": "plan-1", "entries": [entry("Read", "pending")]}},
            {"sessionUpdate": "plan", "entries": [entry("Old", "completed")]},
            {"sessionUpdate": "plan_update",
             "plan": {"type": "items", "planId": "plan-1",
                      "entries": [entry("Read", "completed"), entry("Write", "pending")]}},
            {"sessionUpdate": "plan_update",
             "plan": {"type": "markdown", "planId": "plan-2", "content": "# Plan",
                      "path": "/work/PLAN.md"}},
            {"sessionUpdate": "plan_removed", "planId": "plan-1"},
        ])),
        turn(json!([])),
    ])
    .await;
    let mut body = t.stream().await;
    expect_sse(&mut body, "snapshot").await;
    t.prompt().await;
    let updates = until_closed(&mut body).await;
    drop(body);

    let events = t.stored().await;
    let first_old = of_kind(&events, "plan")[0]["id"].clone();
    let first_items = of_kind(&events, "plan_update")[0]["id"].clone();
    let markdown = of_kind(&events, "plan_update")[2]["id"].clone();
    let warm: std::collections::BTreeMap<String, Value> = ops(&updates, "plan_upsert")
        .into_iter()
        .map(|op| {
            (
                op["plan"]["plan_id"].as_str().unwrap().to_string(),
                op["plan"].clone(),
            )
        })
        .collect();
    let ids: Vec<&String> = warm.keys().collect();
    let mut expected = [&first_old, &first_items, &markdown]
        .map(|id| id.as_str().unwrap().to_string())
        .to_vec();
    expected.sort();
    assert_eq!(ids, expected.iter().collect::<Vec<_>>(), "three plans");

    let old = &warm[first_old.as_str().unwrap()];
    assert_eq!(old["variant"], "entries");
    assert_eq!(old["entries"], json!([entry("Old", "completed")]));
    assert_eq!(old["removed"], false);
    let items = &warm[first_items.as_str().unwrap()];
    assert_eq!(
        items["entries"],
        json!([entry("Read", "completed"), entry("Write", "pending")])
    );
    assert_eq!(items["removed"], true, "plan-1 is removed");
    let doc = &warm[markdown.as_str().unwrap()];
    assert_eq!(doc["variant"], "markdown");
    assert_eq!(doc["markdown"], "# Plan");
    assert_eq!(doc["path"], "/work/PLAN.md");
    assert_eq!(doc["removed"], false);

    // A page before the next turn is a fold of the store alone.
    t.prompt().await;
    t.stops(3).await;
    let next = t.prompted_turn().await;
    let page = t.trajectory(&format!("?before={next}")).await;
    let mut cold: Vec<Value> = page["plans"].as_array().unwrap().clone();
    cold.sort_by_key(|plan| plan["plan_id"].as_str().unwrap().to_string());
    let warm: Vec<Value> = warm.into_values().collect();
    assert_eq!(cold, warm, "a fold built again holds the same plans");
}

/// A notice is a `note` step with its severity, and a compaction is one
/// `compaction` step with its status and its summary. A fold built again
/// from the store gives each the id the stream gave it.
#[tokio::test]
async fn a_notice_is_a_note_and_a_compaction_is_one_step_with_its_status_and_summary() {
    let t = Session::start(vec![turn(json!([
        {"sessionUpdate": "notice", "severity": "warning", "title": "Rate limited",
         "description": "Retrying in a minute"},
        {"sessionUpdate": "notice", "severity": "info", "title": "Indexed"},
        {"sessionUpdate": "compaction_update", "compactionId": "c-1", "status": "in_progress"},
        {"sessionUpdate": "compaction_summary_chunk", "compactionId": "c-1",
         "content": text("Read the parser. ")},
        {"sessionUpdate": "compaction_summary_chunk", "compactionId": "c-1",
         "content": text("Fixed the bug.")},
        {"sessionUpdate": "compaction_update", "compactionId": "c-1", "status": "completed"},
    ]))])
    .await;
    let mut body = t.stream().await;
    expect_sse(&mut body, "snapshot").await;
    t.prompt().await;
    let updates = until_closed(&mut body).await;
    drop(body);

    let warm = newest_steps(&updates);
    let notes: Vec<&Value> = warm
        .values()
        .filter(|step| step["type"] == "note" && step["origin"] == "notice")
        .collect();
    let notes: Vec<(&Value, &Value)> = notes
        .iter()
        .map(|note| (&note["severity"], &note["text"]))
        .collect();
    assert_eq!(
        notes,
        [
            (
                &json!("warning"),
                &json!("Rate limited · Retrying in a minute")
            ),
            (&json!("info"), &json!("Indexed")),
        ]
    );
    let compactions: Vec<&Value> = warm
        .values()
        .filter(|step| step["type"] == "compaction")
        .collect();
    let [compaction] = compactions.as_slice() else {
        panic!("one compaction step: {compactions:?}");
    };
    assert_eq!(compaction["compaction_id"], "c-1");
    assert_eq!(compaction["status"], "completed");
    assert_eq!(
        compaction["summary"],
        json!([text("Read the parser. Fixed the bug.")])
    );

    let turn_id = t.prompted_turn().await;
    let cold: Vec<Value> = t
        .steps(&turn_id)
        .await
        .into_iter()
        .filter(|step| step["type"] == "note" || step["type"] == "compaction")
        .collect();
    let warm: Vec<Value> = warm
        .into_values()
        .filter(|step| step["type"] == "note" || step["type"] == "compaction")
        .collect();
    assert_eq!(cold, warm, "a fold built again holds the same steps");
}

/// A message with an image gives an `image` block with a blob URL under the
/// stored message, and never the bytes.
#[tokio::test]
async fn a_message_with_an_image_gives_an_image_block_with_a_blob_url() {
    let image = json!({"type": "image", "mimeType": "image/png", "data": "iVBORw0KGgo="});
    let t = Session::start(vec![turn(json!([
        {"sessionUpdate": "agent_message_chunk", "content": text("Look: "), "messageId": "msg-1"},
        {"sessionUpdate": "agent_message_chunk", "content": image, "messageId": "msg-1"},
    ]))])
    .await;
    t.prompt().await;
    t.stops(2).await;

    let events = t.stored().await;
    let message = of_kind(&events, "agent_message").last().unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let blob = format!("/v1/sessions/{}/trajectory/blobs/{message}/1", t.session.id);
    let expected = json!([
        text("Look: "),
        {"type": "image", "mime_type": "image/png", "blob": blob, "uri": null},
    ]);
    let turn_id = t.prompted_turn().await;
    let steps = t.steps(&turn_id).await;
    let step = steps
        .iter()
        .find(|step| step["type"] == "message")
        .unwrap_or_else(|| panic!("a message step: {steps:?}"));
    assert_eq!(step["content"], expected);
    let snapshot = t.trajectory("").await;
    let answer = &snapshot["turns"]
        .as_array()
        .unwrap()
        .iter()
        .find(|turn| turn["id"] == turn_id.as_str())
        .unwrap()["answer"];
    assert_eq!(answer, &expected, "the answer is the same blocks");
    assert!(
        !snapshot.to_string().contains("iVBORw0KGgo"),
        "no bytes in the trajectory: {snapshot}"
    );
}

/// After a reconnect, a running call shows the status and the preview of
/// its newest stored progress, and a running compaction its newest update:
/// a new open reads both from the store.
#[tokio::test]
async fn a_new_open_shows_the_newest_stored_progress_of_a_running_call() {
    let progress = |n: u32| {
        json!({"sessionUpdate": "tool_call_update", "toolCallId": "call-7",
               "status": "in_progress",
               "content": [{"type": "content", "content": text(&format!("step {n}"))}]})
    };
    let mut held = turn(json!([
        {"sessionUpdate": "tool_call", "toolCallId": "call-7", "title": "Bash",
         "kind": "execute", "status": "pending", "rawInput": {"command": "cargo build"}},
        progress(1),
        progress(2),
        {"sessionUpdate": "compaction_update", "compactionId": "c-2", "status": "in_progress"},
        {"sessionUpdate": "compaction_update", "compactionId": "c-2", "status": "completed",
         "summary": [text("Kept.")]},
    ]));
    held["held"] = json!(true);
    // Every update of the call is stored: its interval runs out at once.
    let t = Session::start_with(
        vec![held],
        Timeouts {
            tool_progress: std::time::Duration::ZERO,
            ..Timeouts::default()
        },
    )
    .await;
    t.prompt().await;
    eventually(
        TIMEOUT,
        "the progress and the compaction stored",
        || async {
            let events = t.stored().await;
            of_kind(&events, "tool_call_update").len() == 2
                && of_kind(&events, "compaction_update").len() == 2
        },
    )
    .await;

    let snapshot = t.trajectory("").await;
    let running = snapshot["turns"]
        .as_array()
        .unwrap()
        .iter()
        .find(|turn| turn["state"] == "running")
        .unwrap_or_else(|| panic!("the held turn runs: {snapshot}"));
    let steps = running["steps"].as_array().unwrap();
    let call = steps
        .iter()
        .find(|step| step["type"] == "tool_call")
        .unwrap_or_else(|| panic!("the call: {running}"));
    assert_eq!(call["status"], "running");
    assert_eq!(call["preview"], "step 2");
    assert_eq!(call["subject"], "cargo build");
    let compaction = steps
        .iter()
        .find(|step| step["type"] == "compaction")
        .unwrap_or_else(|| panic!("the compaction: {running}"));
    assert_eq!(compaction["status"], "completed");
    assert_eq!(compaction["summary"], json!([text("Kept.")]));
    t.release();
    t.stops(2).await;
}

/// The newest turn of a page, whole.
fn last_turn(page: &Value) -> &Value {
    page["turns"].as_array().unwrap().last().unwrap()
}

/// A call that its turn leaves open ends as `interrupted` with a turn that
/// ends, and as `cancelled` with a turn that is cancelled.
#[tokio::test]
async fn an_abandoned_call_ends_interrupted_with_its_turn() {
    let t = Session::start(vec![turn(json!([
        {"sessionUpdate": "tool_call", "toolCallId": "call-8", "title": "Bash",
         "kind": "execute", "status": "pending", "rawInput": {"command": "ls"}},
        {"sessionUpdate": "tool_call_update", "toolCallId": "call-8", "status": "in_progress"},
    ]))])
    .await;
    t.prompt().await;
    t.stops(2).await;
    assert_eq!(
        of_kind(&t.stored().await, "tool_call_abandoned")[0]["payload"]["reason"],
        "turn_ended"
    );

    let turn_id = t.prompted_turn().await;
    let steps = t.steps(&turn_id).await;
    let call = steps
        .iter()
        .find(|step| step["type"] == "tool_call")
        .unwrap_or_else(|| panic!("the call: {steps:?}"));
    assert_eq!(call["tool_call_id"], "call-8");
    assert_eq!(call["status"], "interrupted");
    assert!(call["ended_at"].is_string(), "{call}");
}

#[tokio::test]
async fn an_abandoned_call_ends_cancelled_with_a_cancelled_turn() {
    let t = Session::start(vec![json!({"held": true, "updates": [
        {"sessionUpdate": "tool_call", "toolCallId": "call-9", "title": "Bash",
         "kind": "execute", "status": "pending", "rawInput": {"command": "sleep 60"}},
    ]})])
    .await;
    t.prompt().await;
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
    t.stops(2).await;
    assert_eq!(
        of_kind(&t.stored().await, "tool_call_abandoned")[0]["payload"]["reason"],
        "cancelled"
    );

    let turn_id = t.prompted_turn().await;
    let steps = t.steps(&turn_id).await;
    let call = steps
        .iter()
        .find(|step| step["type"] == "tool_call")
        .unwrap_or_else(|| panic!("the call: {steps:?}"));
    assert_eq!(call["status"], "cancelled");
    let snapshot = t.trajectory("").await;
    let closed = last_turn(&snapshot);
    assert_eq!(closed["state"], "cancelled");
    assert_eq!(closed["activity"]["cancelled"], 1);
}

/// The fields of a session's state that its agent reports.
fn reported(session: &Value) -> Value {
    json!({
        "mode": session["mode"],
        "config_options": session["config_options"],
        "title": session["title"],
        "cost": session["cost"],
        "commands": session["commands"],
    })
}

/// The snapshot's session holds the mode, the config options, the title,
/// the cost and the commands the agent reported; a change arrives as a
/// `session_patch`, and a fold built again holds the same.
#[tokio::test]
async fn the_session_state_holds_what_the_agent_reports_and_changes_by_patch() {
    let options = json!([{"id": "model-id", "name": "Model", "category": "model",
                          "type": "select", "currentValue": "test-model",
                          "options": [{"value": "test-model", "name": "Test model",
                                       "description": "The one model"}]}]);
    let t = Session::start_on(
        vec![turn(json!([
            {"sessionUpdate": "current_mode_update", "currentModeId": "ask"},
            {"sessionUpdate": "config_option_update", "configOptions": options},
            {"sessionUpdate": "session_info_update", "title": "Fix the parser"},
            {"sessionUpdate": "usage_update", "used": 10, "size": 100,
             "cost": {"amount": 0.42, "currency": "USD"}},
            {"sessionUpdate": "available_commands_update",
             "availableCommands": [{"name": "review", "description": "Review the diff",
                                    "input": {"hint": "a path"}}]},
        ]))],
        Timeouts::default(),
        |scripted| {
            scripted["modes"] = json!({
                "currentModeId": "code",
                "availableModes": [
                    {"id": "code", "name": "Code"},
                    {"id": "ask", "name": "Ask", "description": "Answer questions only"},
                ],
            });
        },
    )
    .await;
    let mut body = t.stream().await;
    let snapshot = expect_sse(&mut body, "snapshot").await;
    let opened = &snapshot["session"];
    assert_eq!(opened["mode"]["current_id"], "code", "{opened}");
    let ids: Vec<&Value> = opened["config_options"]
        .as_array()
        .unwrap()
        .iter()
        .map(|option| &option["id"])
        .collect();
    assert_eq!(ids, [&json!("model-id"), &json!("effort-id")]);

    let expected = json!({
        "mode": {"current_id": "ask", "available": [
            {"id": "code", "name": "Code", "description": null},
            {"id": "ask", "name": "Ask", "description": "Answer questions only"},
        ]},
        "config_options": [{"id": "model-id", "name": "Model", "description": null,
                            "category": "model", "current_value": "test-model",
                            "values": [{"value": "test-model", "name": "Test model",
                                        "description": "The one model"}]}],
        "title": "Fix the parser",
        "cost": {"amount": 0.42, "currency": "USD"},
        "commands": [{"name": "review", "description": "Review the diff", "hint": "a path"}],
    });
    t.prompt().await;
    let mut seen = Vec::new();
    while ops(&seen, "session_patch")
        .last()
        .is_none_or(|op| reported(&op["session"]) != expected)
    {
        seen.push(expect_sse(&mut body, "update").await);
    }
    drop(body);
    t.stops(2).await;

    // A page is a fold of the store alone.
    let turn_id = t.prompted_turn().await;
    let page = t.trajectory(&format!("?before={turn_id}")).await;
    assert_eq!(reported(&page["session"]), expected);
}

/// The header of a closed turn — its counts, its steps before the page, its
/// attention and its answer — is exact from a read of the store, with
/// notices, a compaction, a plan and an abandoned call in it. A read of a
/// closed turn loads its prompt and its answer alone, so every other event
/// of it folds as a head, the path a turn of any size takes.
#[tokio::test]
async fn the_header_of_a_closed_turn_stays_exact() {
    const NOTICES: usize = 30;
    let mut updates: Vec<Value> = (0..NOTICES)
        .map(|n| json!({"sessionUpdate": "notice", "severity": "info", "title": format!("n{n}")}))
        .collect();
    updates.extend([
        json!({"sessionUpdate": "compaction_update", "compactionId": "c-1", "status": "in_progress"}),
        json!({"sessionUpdate": "compaction_update", "compactionId": "c-1", "status": "completed"}),
        json!({"sessionUpdate": "plan_update",
               "plan": {"type": "items", "planId": "plan-1", "entries": []}}),
        json!({"sessionUpdate": "tool_call", "toolCallId": "call-5", "title": "Bash",
               "kind": "execute", "status": "pending", "rawInput": {"command": "ls"}}),
        json!({"sessionUpdate": "agent_message_chunk", "content": text("Done.")}),
    ]);
    let t = Session::start(vec![turn(Value::Array(updates))]).await;
    t.prompt().await;
    t.stops(2).await;
    assert!(
        t.h.store
            .count_session_events(&t.session.id, "notice")
            .await
            .unwrap() as usize
            == NOTICES,
        "every notice is stored"
    );

    // The first open of the session: a fold of the store, under its bound.
    let snapshot = t.trajectory("").await;
    let turn = last_turn(&snapshot);
    assert_eq!(turn["prompt"]["content"][0]["text"], PROMPT);
    assert_eq!(turn["state"], "completed");
    assert_eq!(turn["steps"], json!([]));
    assert_eq!(
        turn["earlier_steps"],
        NOTICES + 3,
        "each notice, the compaction, the call and the message"
    );
    assert_eq!(
        turn["activity"],
        json!({"steps": 1, "tool_calls": 1, "files_changed": 0, "failed": 0,
               "cancelled": 0, "thoughts": 0, "subagents": 0})
    );
    assert_eq!(turn["attention"], json!([]));
    assert_eq!(turn["attention_more"], 0);
    assert_eq!(turn["answer"], json!([text("Done.")]));
}
