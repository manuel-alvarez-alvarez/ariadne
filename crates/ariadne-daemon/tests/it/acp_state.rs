//! What the daemon keeps of the session state an ACP agent reports (021,
//! rule 24): plans, notices and compactions as stored events, a mode change
//! as `mode_update`, and the display state on the session row — its modes,
//! config options, title, cost and commands.
//!
//! The agent is the scriptable stub (`common::acp`).

use crate::common;

use serde_json::{Value, json};

use ariadne_core::SessionStatus;
use ariadne_store::{AgentSession, EventFilter, SessionCost, SessionDisplayState};

use common::acp::{StubAcpAgent, discovery_settled, registry_home, script, stub_acp_agent};
use common::{Harness, TIMEOUT, eventually, expect_sse, get, harness, post_json};

/// The ACP session modes the stub offers in these tests.
fn modes() -> Value {
    json!({
        "currentModeId": "code",
        "availableModes": [
            {"id": "code", "name": "Code"},
            {"id": "ask", "name": "Ask", "description": "Answer questions only"},
        ],
    })
}

fn text(text: &str) -> Value {
    json!({"type": "text", "text": text})
}

/// A stub whose turns are `turns`, offering [`modes`], on a harness that
/// knows it as the agent `stub`.
async fn started(turns: Value) -> (tempfile::TempDir, StubAcpAgent, Harness, AgentSession) {
    started_on(turns, None).await
}

/// [`started`], where `new_updates` names the updates the stub sends after
/// `session/new` alone, never after a resume. Discovery runs first, since a
/// resume reads what discovery measured of the agent.
async fn started_on(
    turns: Value,
    new_updates: Option<Value>,
) -> (tempfile::TempDir, StubAcpAgent, Harness, AgentSession) {
    let mut scripted = script();
    scripted["modes"] = modes();
    scripted["stored_sessions"] = json!(["stub-session"]);
    scripted["prompts"] = turns;
    let discovered = new_updates.is_some();
    if let Some(updates) = new_updates {
        scripted["new_updates"] = updates;
    }
    let dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(dir.path(), scripted);
    let h = match discovered {
        true => {
            let h = harness().home(registry_home(&stub)).discover_agents().await;
            discovery_settled(&h, &stub).await;
            h
        }
        false => harness().home(registry_home(&stub)).await,
    };
    h.git_repo("repo");
    let cast = h.cast_pinned("stub:test-model").await;
    let session = h.start_agent(&cast.task, "develop").await;
    eventually(TIMEOUT, "the first turn to end", || async {
        h.session_status(&session).await == SessionStatus::Idle
    })
    .await;
    (dir, stub, h, session)
}

fn turn(updates: Value) -> Value {
    json!({"updates": updates, "stop_reason": "end_turn"})
}

/// Every stored event of the session, as kind and payload.
async fn stored(h: &Harness, session_id: &str) -> Vec<(String, Value)> {
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

fn of_kind<'a>(events: &'a [(String, Value)], kind: &str) -> Vec<&'a Value> {
    events
        .iter()
        .filter(|(k, _)| k == kind)
        .map(|(_, payload)| payload)
        .collect()
}

async fn display_state(h: &Harness, session: &AgentSession) -> SessionDisplayState {
    h.store.session_display_state(&session.id).await.unwrap()
}

/// A plan update of each shape, a removed plan, a notice, a compaction and a
/// mode change are stored under their own kinds, with the payload the
/// interface names.
#[tokio::test]
async fn plans_notices_compactions_and_mode_changes_are_stored_under_their_names() {
    let (_dir, _stub, h, session) = started(json!([turn(json!([
        {"sessionUpdate": "plan_update",
         "plan": {"type": "items", "planId": "plan-1",
                  "entries": [{"content": "Read", "priority": "high", "status": "pending"}]}},
        {"sessionUpdate": "plan_update",
         "plan": {"type": "markdown", "planId": "plan-2", "content": "# Plan",
                  "path": "/work/PLAN.md"}},
        {"sessionUpdate": "plan_removed", "planId": "plan-1"},
        {"sessionUpdate": "notice", "severity": "warning", "title": "Rate limited",
         "description": "Retrying in a minute"},
        {"sessionUpdate": "compaction_update", "compactionId": "c-1", "status": "in_progress"},
        {"sessionUpdate": "compaction_summary_chunk", "compactionId": "c-1",
         "content": text("Read the parser. ")},
        {"sessionUpdate": "compaction_summary_chunk", "compactionId": "c-1",
         "content": text("Fixed the bug.")},
        {"sessionUpdate": "compaction_update", "compactionId": "c-1", "status": "completed"},
        {"sessionUpdate": "current_mode_update", "currentModeId": "ask"},
    ]))]))
    .await;

    let events = stored(&h, &session.id).await;
    assert_eq!(
        of_kind(&events, "plan_update"),
        [
            &json!({"session_id": "stub-session",
                    "plan": {"type": "items", "plan_id": "plan-1",
                             "entries": [{"content": "Read", "priority": "high",
                                          "status": "pending"}]}}),
            &json!({"session_id": "stub-session",
                    "plan": {"type": "markdown", "plan_id": "plan-2", "content": "# Plan",
                             "path": "/work/PLAN.md"}}),
        ]
    );
    assert_eq!(
        of_kind(&events, "plan_removed"),
        [&json!({"session_id": "stub-session", "plan_id": "plan-1"})]
    );
    assert_eq!(
        of_kind(&events, "notice"),
        [&json!({"session_id": "stub-session", "severity": "warning",
                 "title": "Rate limited", "description": "Retrying in a minute"})]
    );
    assert_eq!(
        of_kind(&events, "compaction_update"),
        [
            &json!({"session_id": "stub-session", "compaction_id": "c-1",
                    "status": "in_progress", "summary": [], "error": null}),
            &json!({"session_id": "stub-session", "compaction_id": "c-1",
                    "status": "completed",
                    "summary": [text("Read the parser. "), text("Fixed the bug.")],
                    "error": null}),
        ],
        "the stored compaction holds the whole summary"
    );
    assert!(
        of_kind(&events, "compaction_summary_chunk").is_empty(),
        "a summary chunk is never stored"
    );
    assert_eq!(
        of_kind(&events, "mode_update"),
        [&json!({"session_id": "stub-session", "current_mode_id": "ask"})]
    );
}

/// A summary the agent sends whole replaces the chunks before it, and an
/// error is kept with the compaction.
#[tokio::test]
async fn a_whole_compaction_summary_replaces_its_chunks() {
    let (_dir, _stub, h, session) = started(json!([turn(json!([
        {"sessionUpdate": "compaction_summary_chunk", "compactionId": "c-2",
         "content": text("Draft")},
        {"sessionUpdate": "compaction_update", "compactionId": "c-2", "status": "failed",
         "summary": [text("Final")], "error": "context still full"},
    ]))]))
    .await;

    let events = stored(&h, &session.id).await;
    assert_eq!(
        of_kind(&events, "compaction_update"),
        [
            &json!({"session_id": "stub-session", "compaction_id": "c-2",
                 "status": "failed", "summary": [text("Final")],
                 "error": "context still full"})
        ]
    );
}

/// Summary chunks and display state changes go out live to the console
/// stream while the turn runs.
#[tokio::test]
async fn summary_chunks_and_display_changes_go_out_live() {
    let first = script()["prompts"][0].clone();
    let (_dir, _stub, h, session) = started(json!([
        first,
        turn(json!([
            {"sessionUpdate": "compaction_summary_chunk", "compactionId": "c-3",
             "content": text("Kept this.")},
            {"sessionUpdate": "session_info_update", "title": "Fix the parser"},
            {"sessionUpdate": "compaction_update", "compactionId": "c-3", "status": "completed"},
        ]))
    ]))
    .await;
    let mut body = h
        .stream(get(&format!("/v1/sessions/{}/console/stream", session.id)))
        .await;
    expect_sse(&mut body, "snapshot").await;
    let (status, _) = h
        .send(post_json(
            &format!("/v1/sessions/{}/console/input", session.id),
            json!({"text": "go on"}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::NO_CONTENT);

    let mut seen: Vec<Value> = Vec::new();
    while seen
        .last()
        .is_none_or(|e: &Value| e["kind"] != "compaction_update")
    {
        seen.push(expect_sse(&mut body, "event").await);
    }
    let live = |kind: &str| -> Vec<Value> {
        seen.iter()
            .filter(|e| e["kind"] == kind)
            .map(|e| e["payload"].clone())
            .collect()
    };
    assert_eq!(
        live("compaction_summary_chunk"),
        [json!({"session_id": "stub-session", "compaction_id": "c-3",
                "content": text("Kept this.")})]
    );
    assert_eq!(
        live("display_state_update"),
        [json!({"session_id": "stub-session", "title": "Fix the parser"})]
    );
}

/// The session row keeps the modes and options the agent opened with, and
/// each later report of a mode, the options, a title, a cost and the
/// commands.
#[tokio::test]
async fn the_session_keeps_the_display_state_its_agent_reports() {
    let options = json!([{"id": "model-id", "name": "Model", "category": "model",
                          "type": "select", "currentValue": "test-model",
                          "options": [{"value": "test-model", "name": "test-model"}]}]);
    let (_dir, _stub, h, session) = started(json!([turn(json!([
        {"sessionUpdate": "current_mode_update", "currentModeId": "ask"},
        {"sessionUpdate": "config_option_update", "configOptions": options},
        {"sessionUpdate": "session_info_update", "title": "Fix the parser"},
        {"sessionUpdate": "usage_update", "used": 10, "size": 100,
         "cost": {"amount": 0.42, "currency": "USD"}},
        {"sessionUpdate": "available_commands_update",
         "availableCommands": [{"name": "review", "description": "Review the diff"}]},
    ]))]))
    .await;

    let state = display_state(&h, &session).await;
    let mode = state.mode.expect("the agent offers modes");
    assert_eq!(mode.current_id.as_deref(), Some("ask"));
    assert_eq!(json!(mode.available), modes()["availableModes"]);
    assert_eq!(json!(state.config_options), options);
    assert_eq!(state.title.as_deref(), Some("Fix the parser"));
    assert_eq!(
        state.cost,
        Some(SessionCost {
            amount: 0.42,
            currency: "USD".into()
        })
    );
    assert_eq!(
        json!(state.commands),
        json!([{"name": "review", "description": "Review the diff"}])
    );
}

/// The modes and options a session opens with are its display state before
/// any update.
#[tokio::test]
async fn a_launch_starts_from_the_modes_and_options_it_opened_with() {
    let (_dir, _stub, h, session) = started(json!([turn(json!([]))])).await;

    let state = display_state(&h, &session).await;
    let mode = state.mode.expect("the agent offers modes");
    assert_eq!(mode.current_id.as_deref(), Some("code"));
    assert_eq!(json!(mode.available), modes()["availableModes"]);
    let ids: Vec<&str> = state
        .config_options
        .iter()
        .map(|option| option["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["model-id", "effort-id"]);
    assert_eq!(state.title, None);
}

/// A child's mode, options, title, cost and commands are its own: none of
/// them changes the session's display state.
#[tokio::test]
async fn a_subagents_updates_leave_the_display_state_alone() {
    let under = |mut update: Value| {
        update["as_session"] = json!("child-1");
        update
    };
    let (_dir, _stub, h, session) = started(json!([turn(json!([
        {"sessionUpdate": "subagent_update", "sessionId": "child-1", "title": "Explorer",
         "state": {"state": "running"}},
        under(json!({"sessionUpdate": "current_mode_update", "currentModeId": "ask"})),
        under(json!({"sessionUpdate": "config_option_update", "configOptions": []})),
        under(json!({"sessionUpdate": "session_info_update", "title": "Child title"})),
        under(json!({"sessionUpdate": "usage_update", "used": 1, "size": 2,
                     "cost": {"amount": 9.0, "currency": "USD"}})),
        under(json!({"sessionUpdate": "available_commands_update",
                     "availableCommands": [{"name": "child", "description": "x"}]})),
    ]))]))
    .await;

    let state = display_state(&h, &session).await;
    assert_eq!(
        state.mode.unwrap().current_id.as_deref(),
        Some("code"),
        "the child's mode is not the session's"
    );
    assert_eq!(state.config_options.len(), 2);
    assert_eq!(state.title, None);
    assert_eq!(state.cost, None);
    assert!(state.commands.is_empty());
}

/// A resumed session starts from what its new launch reports: the title of
/// the earlier launch is gone, and the modes are the resumed answer's. The
/// stub reports the title and the mode after `session/new` alone, so the
/// resumed launch reports neither.
#[tokio::test]
async fn a_new_launch_replaces_the_display_state_of_the_earlier_one() {
    let (_dir, stub, h, session) = started_on(
        json!([turn(json!([])), turn(json!([]))]),
        Some(json!([
            {"sessionUpdate": "session_info_update", "title": "Old title"},
            {"sessionUpdate": "current_mode_update", "currentModeId": "ask"},
        ])),
    )
    .await;
    let first = display_state(&h, &session).await;
    assert_eq!(first.title.as_deref(), Some("Old title"));
    assert_eq!(
        first.mode.unwrap().current_id.as_deref(),
        Some("ask"),
        "a mode reported during setup is newer than the opening answer"
    );

    h.launcher
        .relaunch_session(&session.id, "carry on")
        .await
        .unwrap();
    eventually(TIMEOUT, "the resumed turn to end", || async {
        stub.calls_of("session/prompt").len() == 2
            && h.session_status(&session).await == SessionStatus::Idle
    })
    .await;

    let state = display_state(&h, &session).await;
    assert_eq!(state.title, None, "the earlier launch's title is gone");
    assert_eq!(state.mode.unwrap().current_id.as_deref(), Some("code"));
}

/// A refused model switch leaves the options that the agent reported: the
/// daemon's own copy of the options is not written over them.
#[tokio::test]
async fn a_refused_switch_leaves_the_reported_options() {
    let reported = json!([{"id": "model-id", "name": "Model", "category": "model",
                           "type": "select", "currentValue": "test-model",
                           "options": [{"value": "test-model", "name": "Reported"}]}]);
    let mut scripted = script();
    scripted["reject_config_value"] = json!("b");
    scripted["prompts"] = json!([turn(json!([
        {"sessionUpdate": "config_option_update", "configOptions": reported},
    ]))]);
    let dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(dir.path(), scripted);
    let h = harness().home(registry_home(&stub)).await;
    h.git_repo("repo");
    let cast = h.cast_pinned("stub:test-model").await;
    let session = h.start_agent(&cast.task, "develop").await;
    eventually(TIMEOUT, "the first turn to end", || async {
        h.session_status(&session).await == SessionStatus::Idle
    })
    .await;
    assert_eq!(
        json!(display_state(&h, &session).await.config_options),
        reported
    );

    let (status, _) = h
        .send(post_json(
            &format!("/v1/sessions/{}/switch", session.id),
            json!({"model": "stub:b"}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::CONFLICT);
    assert_eq!(
        stub.calls_of("session/set_config_option")
            .last()
            .map(|call| call["value"].clone()),
        Some(json!("b")),
        "the switch reached the agent"
    );

    assert_eq!(
        json!(display_state(&h, &session).await.config_options),
        reported
    );
}

/// A switch the agent takes writes the options of its answer, even where
/// that answer matches the options the session opened with: the agent
/// reported other options meanwhile, and its answer is newer.
#[tokio::test]
async fn a_switch_writes_the_options_the_agent_answers_with() {
    let reported = json!([{"id": "model-id", "name": "Model", "category": "model",
                           "type": "select", "currentValue": "reported-model",
                           "options": [{"value": "reported-model", "name": "Reported"}]}]);
    let (_dir, stub, h, session) = started(json!([turn(json!([
        {"sessionUpdate": "config_option_update", "configOptions": reported},
    ]))]))
    .await;
    let opened = h.store.get_session(&session.id).await.unwrap().model;
    assert_eq!(
        json!(display_state(&h, &session).await.config_options),
        reported
    );

    let (status, body) = h
        .send(post_json(
            &format!("/v1/sessions/{}/switch", session.id),
            json!({"model": opened}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK, "{body:?}");
    let answered = stub
        .calls_of("session/set_config_option")
        .last()
        .map(|call| call["value"].clone());
    assert_eq!(
        answered,
        Some(json!("test-model")),
        "the switch reached the agent"
    );

    let state = display_state(&h, &session).await;
    let model = state
        .config_options
        .iter()
        .find(|option| option["id"] == "model-id")
        .expect("the agent's answer names the model option");
    assert_eq!(
        model["currentValue"], "test-model",
        "the agent's answer replaces the options it reported before"
    );
}
