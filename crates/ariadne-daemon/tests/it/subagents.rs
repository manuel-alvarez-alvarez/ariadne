//! Subagents: child sessions an agent announces on its own session and runs
//! under their own session ids, on the same connection (021).
//!
//! The agent is the scriptable stub from test support (`common::acp`), which
//! sends an update under a child's session id where the update names
//! `as_session`. Three shapes are covered: codex-acp's and claude-agent-acp's
//! `subagent_spawned` and `subagent_state_update`, the draft protocol's
//! `subagent_update`, and an agent that announces nothing, as OpenCode does.

use crate::common;

use serde_json::{Value, json};

use ariadne_core::SessionStatus;
use ariadne_store::{AgentPin, EventFilter};

use common::acp::{registry_home, script, stub_acp_agent};
use common::{Harness, TIMEOUT, eventually, expect_sse, get, harness};

/// One stored event: its kind and its payload.
type Stored = (String, Value);

/// Run the develop column's agent on the stub with one turn of `updates`
/// (and whatever else `turn` names) and answer every event it stored, in
/// order.
async fn stored_turn(turn: Value) -> (Vec<Stored>, Value) {
    let agent_dir = tempfile::tempdir().unwrap();
    let mut scripted = script();
    scripted["prompts"] = json!([turn]);
    let stub = stub_acp_agent(agent_dir.path(), scripted);
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
    eventually(TIMEOUT, "the turn to end", || async {
        h.session_status(&session).await == SessionStatus::Idle
    })
    .await;
    let initialize = stub.calls_of("initialize")[0].clone();
    (events(&h, &session.id).await, initialize)
}

async fn events(h: &Harness, session_id: &str) -> Vec<Stored> {
    h.store
        .list_events(EventFilter {
            session_id: Some(session_id.to_string()),
            ..Default::default()
        })
        .await
        .unwrap()
        .into_iter()
        .map(|event| (event.kind, serde_json::from_str(&event.payload).unwrap()))
        .collect()
}

/// The payloads of every stored event of `kind`.
fn of_kind<'a>(events: &'a [Stored], kind: &str) -> Vec<&'a Value> {
    events
        .iter()
        .filter(|(k, _)| k == kind)
        .map(|(_, payload)| payload)
        .collect()
}

fn chunk(text: &str) -> Value {
    json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": text}})
}

/// `update` sent under the session id `session`.
fn under(session: &str, mut update: Value) -> Value {
    update["as_session"] = json!(session);
    update
}

/// codex-acp's shape: a child announced with `subagent_spawned`, its text
/// and its tool call sent under its own session id, then its end with
/// `subagent_state_update`. Every child event is stored on the parent
/// session, under its own kind, with the child named; the parent's text is
/// its own, never one run with the child's. The daemon tells the agent it
/// understands subagents.
#[tokio::test]
async fn a_codex_child_is_folded_onto_the_parent_session() {
    let child = "019a2b3c-thread";
    let (events, initialize) = stored_turn(json!({"updates": [
        chunk("Spawning an explorer. "),
        {"sessionUpdate": "subagent_spawned", "subagentSessionId": child,
         "name": "explorer", "task": "Delegated task for explorer", "capabilities": {}},
        under(child, chunk("Looked around.")),
        under(child, json!({"sessionUpdate": "tool_call", "toolCallId": "call-1",
                            "title": "Read", "kind": "read", "status": "pending",
                            "rawInput": {"path": "README.md"}})),
        under(child, json!({"sessionUpdate": "tool_call_update", "toolCallId": "call-1",
                            "status": "completed"})),
        {"sessionUpdate": "subagent_state_update", "subagentSessionId": child,
         "state": "completed"},
        chunk("The explorer is done."),
    ]}))
    .await;

    assert_eq!(initialize["clientCapabilities"]["subagents"], json!({}));

    let updates = of_kind(&events, "subagent_update");
    assert_eq!(
        updates,
        [
            &json!({"session_id": "stub-session",
                    "subagent": {"id": child, "title": "explorer",
                                 "description": "Delegated task for explorer"},
                    "state": "running", "capabilities": {"cancel": false}}),
            &json!({"session_id": "stub-session",
                    "subagent": {"id": child, "title": "explorer",
                                 "description": "Delegated task for explorer"},
                    "state": "completed", "capabilities": {"cancel": false}}),
        ]
    );

    let named = json!({"id": child, "title": "explorer"});
    let texts: Vec<(Value, Value)> = of_kind(&events, "agent_message")
        .into_iter()
        .map(|payload| (payload["text"].clone(), payload["subagent"].clone()))
        .collect();
    assert_eq!(
        texts,
        [
            (json!("Spawning an explorer. "), Value::Null),
            (json!("Looked around."), named.clone()),
            (json!("The explorer is done."), Value::Null),
        ]
    );
    for kind in ["pre_tool_use", "post_tool_use"] {
        let calls = of_kind(&events, kind);
        assert_eq!(calls.len(), 1, "{kind}: {events:?}");
        assert_eq!(calls[0]["subagent"], named, "{kind}");
        assert_eq!(calls[0]["session_id"], child, "{kind}");
    }
}

/// claude-agent-acp's shape: a child that asks for a permission, a child it
/// starts in turn, announced on its own session, and an end the agent
/// cancelled. The child's question carries the child's name.
#[tokio::test]
async fn a_claude_child_asks_for_permission_under_its_own_name() {
    let (events, _) = stored_turn(json!({
        "updates_first": [
            {"sessionUpdate": "subagent_spawned", "subagentSessionId": "task-1",
             "name": "Explore the repo", "task": "Find the config loader",
             "capabilities": {}},
        ],
        "permission_session": "task-1",
        "permission": {
            "toolCall": {"toolCallId": "call-1", "title": "Bash", "kind": "execute",
                         "rawInput": {"command": "ls"}},
            "options": [
                {"optionId": "no", "name": "Reject", "kind": "reject_once"},
                {"optionId": "yes", "name": "Allow", "kind": "allow_once"},
            ],
        },
        "updates": [
            under("task-1", json!({"sessionUpdate": "subagent_spawned",
                                   "subagentSessionId": "task-2", "name": "Read the tests",
                                   "task": "Read every test", "capabilities": {}})),
            under("task-2", chunk("Read them.")),
            {"sessionUpdate": "subagent_state_update", "subagentSessionId": "task-1",
             "state": "cancelled"},
        ],
    }))
    .await;

    let asked = of_kind(&events, "permission_request");
    assert_eq!(asked.len(), 1, "{events:?}");
    assert_eq!(
        asked[0]["subagent"],
        json!({"id": "task-1", "title": "Explore the repo"})
    );

    let updates = of_kind(&events, "subagent_update");
    let summary: Vec<(Value, Value, Value)> = updates
        .iter()
        .map(|payload| {
            (
                payload["session_id"].clone(),
                payload["subagent"]["id"].clone(),
                payload["state"].clone(),
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            (json!("stub-session"), json!("task-1"), json!("running")),
            (json!("task-1"), json!("task-2"), json!("running")),
            (json!("stub-session"), json!("task-1"), json!("cancelled")),
        ]
    );
    let texts = of_kind(&events, "agent_message");
    assert_eq!(
        texts[0]["subagent"],
        json!({"id": "task-2", "title": "Read the tests"})
    );
}

/// The draft protocol's shape: one `subagent_update` kind whose fields are
/// patches. An omitted field keeps its value, a state is an object, and a
/// child that may be cancelled says so.
#[tokio::test]
async fn a_draft_subagent_update_patches_what_it_names() {
    let (events, _) = stored_turn(json!({"updates": [
        {"sessionUpdate": "subagent_update", "sessionId": "child-9", "title": "Reviewer",
         "description": "Review the diff", "capabilities": {"cancel": {}},
         "state": {"state": "running"}},
        under("child-9", json!({"sessionUpdate": "plan",
                                "entries": [{"content": "Read", "status": "pending"}]})),
        {"sessionUpdate": "subagent_update", "sessionId": "child-9",
         "state": {"state": "idle", "stopReason": "end_turn"}},
        {"sessionUpdate": "subagent_update", "sessionId": "child-9", "state": null},
    ]}))
    .await;

    let updates = of_kind(&events, "subagent_update");
    let subagent = json!({"id": "child-9", "title": "Reviewer", "description": "Review the diff"});
    assert_eq!(
        updates,
        [
            &json!({"session_id": "stub-session", "subagent": subagent,
                    "state": "running", "capabilities": {"cancel": true}}),
            &json!({"session_id": "stub-session", "subagent": subagent,
                    "state": "completed", "capabilities": {"cancel": true}}),
            &json!({"session_id": "stub-session", "subagent": subagent,
                    "state": null, "capabilities": {"cancel": true}}),
        ]
    );
    let plans = of_kind(&events, "plan");
    assert_eq!(
        plans[0]["subagent"],
        json!({"id": "child-9", "title": "Reviewer"})
    );
}

/// An agent that announces nothing, as OpenCode: no `subagent_update` is
/// stored and no event names a child. An update under a session id nobody
/// announced is the parent's own, and an unknown kind is let by.
#[tokio::test]
async fn an_agent_that_announces_nothing_stores_what_it_did_before() {
    let (events, _) = stored_turn(json!({"updates": [
        under("stranger", json!({"sessionUpdate": "plan",
                                 "entries": [{"content": "Read", "status": "pending"}]})),
        {"sessionUpdate": "future_agent_update", "value": "new"},
        chunk("done"),
    ]}))
    .await;

    assert!(of_kind(&events, "subagent_update").is_empty(), "{events:?}");
    assert!(
        events
            .iter()
            .all(|(_, payload)| payload.get("subagent").is_none()),
        "{events:?}"
    );
    let plans = of_kind(&events, "plan");
    assert_eq!(plans.len(), 1, "{events:?}");
    assert_eq!(plans[0]["session_id"], "stranger");
    assert_eq!(of_kind(&events, "agent_message")[0]["text"], "done");
}

/// A child's slash commands reach the console stream under their own kind,
/// `available_commands_update`, with the child named, and never stored, as
/// the parent's are. The parent's list stays the session's own: a snapshot
/// still carries it alone.
#[tokio::test]
async fn a_childs_commands_reach_the_console_named_and_leave_the_parents_alone() {
    let agent_dir = tempfile::tempdir().unwrap();
    let finish = agent_dir.path().join("finish-turn");
    let publish = agent_dir.path().join("publish-child");
    let mut scripted = script();
    scripted["new_updates"] = json!([
        {"sessionUpdate": "available_commands_update", "availableCommands": [
            {"name": "review", "description": "Review the change."}]},
    ]);
    scripted["prompts"] = json!([{
        "wait_for": finish.display().to_string(),
        "updates": [],
        "updates_when": {"file": publish.display().to_string(), "updates": [
            {"sessionUpdate": "subagent_spawned", "subagentSessionId": "c1",
             "name": "explorer", "task": "Look around", "capabilities": {}},
            under("c1", json!({"sessionUpdate": "available_commands_update",
                               "availableCommands": [{"name": "explore", "description": "Explore."}]})),
        ]},
    }]);
    let stub = stub_acp_agent(agent_dir.path(), scripted);
    let h = harness().home(registry_home(&stub)).await;
    h.git_repo("repo");
    let cast = h.cast_pinned("stub:test-model").await;
    let session = h.start_agent(&cast.task, "develop").await;
    eventually(TIMEOUT, "the turn to pause", || async {
        finish.with_extension("reached").exists()
    })
    .await;
    let mut stream = h
        .stream(get(&format!("/v1/sessions/{}/console/stream", session.id)))
        .await;
    expect_sse(&mut stream, "snapshot").await;

    std::fs::write(&publish, "go").unwrap();
    let commands = loop {
        let event = expect_sse(&mut stream, "event").await;
        if event["kind"] == "available_commands_update" {
            break event;
        }
    };
    assert_eq!(
        commands["payload"],
        json!({"session_id": "c1",
               "available_commands": [{"name": "explore", "description": "Explore."}],
               "subagent": {"id": "c1", "title": "explorer"}}),
    );

    let snapshot: Vec<Value> = h.get(&format!("/v1/sessions/{}/console", session.id)).await;
    assert_eq!(
        snapshot
            .iter()
            .filter(|event| event["kind"] == "available_commands_update")
            .map(|event| event["payload"]["available_commands"][0]["name"].clone())
            .collect::<Vec<_>>(),
        [json!("review")],
        "the parent's list is the session's own"
    );
    std::fs::write(&finish, "go").unwrap();
    eventually(TIMEOUT, "the turn to end", || async {
        h.session_status(&session).await == SessionStatus::Idle
    })
    .await;
    assert!(
        events(&h, &session.id)
            .await
            .iter()
            .all(|(kind, _)| kind != "available_commands_update"),
        "commands are never stored"
    );
}
