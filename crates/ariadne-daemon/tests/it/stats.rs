//! Integration tests for the stats ledger (023): the `session_ended` fact a
//! session writes as a run of it ends, the `permission` fact, and what every
//! stats route shares.

use crate::common;

use axum::http::StatusCode;
use serde_json::json;

use ariadne_core::{PermissionMode, Seat, SessionStatus, TaskStatus, TokenUsage};
use ariadne_store::AgentSession;

use common::acp::{discovery_settled, registry_home, script, stub_acp_agent};
use common::{Cast, Harness, TIMEOUT, eventually, get, harness, post, post_json, test_pin};

const LAUNCH: &str = "01launchonexxxxxxxxxxxxxxx";

/// A running session of a column's agent on a task of its own, its launch
/// named, having taken two turns and spent 1000 tokens in, 800 of them
/// cached, and 100 out.
async fn worked_session(h: &Harness) -> (Cast, AgentSession) {
    h.git_repo("repo");
    let cast = h.cast().await;
    let session = h
        .session(
            &cast.goal,
            Some(&cast.task),
            Seat::Agent,
            &cast.develop().id,
        )
        .await;
    h.store
        .set_session_launch(&session.id, LAUNCH)
        .await
        .unwrap();
    h.set_status(&session, SessionStatus::Running).await;
    h.reports(&session, "stop").await;
    h.reports(&session, "stop").await;
    h.store
        .upsert_session_usage(
            &session.id,
            "transcript",
            TokenUsage {
                input_tokens: 1000,
                cached_input_tokens: 800,
                output_tokens: 100,
            },
        )
        .await
        .unwrap();
    (cast, session)
}

/// A run that ends writes one `session_ended` fact: the session's launch,
/// seat, model and skills, and its status, turns and tokens. The same end
/// reported twice writes no second fact.
#[tokio::test]
async fn a_session_that_ends_writes_one_session_ended_fact() {
    let h = harness().await;
    let (_cast, session) = worked_session(&h).await;
    h.ingest_from(&session, LAUNCH, "session_end", json!({}))
        .await;
    h.ingest_from(&session, LAUNCH, "session_end", json!({}))
        .await;

    let facts = h.facts("session_ended").await;
    assert_eq!(facts.len(), 1, "{facts:?}");
    let fact = &facts[0];
    assert_eq!(fact.session_id.as_deref(), Some(session.id.as_str()));
    assert_eq!(fact.launch_id.as_deref(), Some(LAUNCH));
    assert_eq!(fact.seat.as_deref(), Some("agent"));
    assert_eq!(fact.model.as_deref(), Some("stub:test-model"));
    assert_eq!(fact.skills, ["coding"]);
    assert_eq!(fact.data["status"], "exited");
    assert_eq!(fact.data["attention_reason"], serde_json::Value::Null);
    assert_eq!(fact.data["turns"], 2);
    assert_eq!(fact.data["input_tokens"], 1000);
    assert_eq!(fact.data["cached_input_tokens"], 800);
    assert_eq!(fact.data["output_tokens"], 100);
    assert!(fact.data["lifetime_secs"].as_i64().unwrap() >= 0);
}

/// A session restarted under its own id ends once per run, so each launch
/// writes a fact of its own.
#[tokio::test]
async fn a_restarted_session_writes_a_fact_for_each_run() {
    let h = harness().await;
    let (_cast, session) = worked_session(&h).await;
    h.ingest_from(&session, LAUNCH, "session_end", json!({}))
        .await;
    h.store.restart_session(&session.id, None).await.unwrap();
    h.store
        .set_session_launch(&session.id, "01launchtwoxxxxxxxxxxxxxxx")
        .await
        .unwrap();
    h.set_status(&session, SessionStatus::Running).await;
    h.ingest_from(
        &session,
        "01launchtwoxxxxxxxxxxxxxxx",
        "session_end",
        json!({}),
    )
    .await;

    let launches: Vec<_> = h
        .facts("session_ended")
        .await
        .into_iter()
        .map(|fact| fact.launch_id.unwrap())
        .collect();
    assert_eq!(launches, [LAUNCH, "01launchtwoxxxxxxxxxxxxxxx"]);
}

/// A console permission answer writes a fact. The permission is asked by the
/// agent of a task's first column, started by the scheduler as the daemon
/// starts it.
#[tokio::test]
async fn a_console_permission_answer_writes_a_fact() {
    let mut scripted = script();
    scripted["prompts"] = json!([{
        "permission": {
            "toolCall": {"toolCallId": "call-2", "name": "Write", "kind": "write"},
            "options": [
                {"optionId": "no", "name": "Reject", "kind": "reject_once"},
                {"optionId": "yes", "name": "Allow", "kind": "allow_once"},
            ],
        },
    }]);
    let agent_dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(agent_dir.path(), scripted);
    let h = harness()
        .home(registry_home(&stub))
        .discover_agents()
        .scheduler()
        .await;
    discovery_settled(&h, &stub).await;
    h.git_repo("repo");
    // The repository asks before the goal exists: the scheduler starts the
    // goal's orchestrator on the same stub as soon as the goal is created,
    // and one launched in `auto` answers the script's permission itself,
    // with a second fact.
    let repo = h.repository(&h.at("repo")).await;
    h.set_permission_mode(&repo, PermissionMode::Ask).await;
    let goal = h.goal_running(&repo, test_pin(), None).await;
    let task = h.task_on(&goal, &repo, "task", test_pin()).await;
    h.activate(&goal).await;
    h.advance(&task, TaskStatus::InProgress).await;
    h.notify(&task.id);
    // The column's agent runs on the registry stub, whose prompts the
    // harness does not read: the session is waited for by its row.
    eventually(TIMEOUT, "the column agent to start", || async {
        h.running_session(&task.id, ariadne_core::Seat::Agent)
            .await
            .is_some()
    })
    .await;
    let session = h
        .running_session(&task.id, ariadne_core::Seat::Agent)
        .await
        .unwrap();
    eventually(TIMEOUT, "the permission request", || async {
        h.store
            .count_session_events(&session.id, "permission_request")
            .await
            .unwrap()
            == 1
    })
    .await;
    let (status, body) = h
        .send(post_json(
            &format!("/v1/sessions/{}/console/input", session.id),
            json!({"text": "yes"}),
        ))
        .await;
    assert_eq!(
        status,
        StatusCode::NO_CONTENT,
        "the console answer was refused: {}",
        String::from_utf8_lossy(&body)
    );
    eventually(TIMEOUT, "the permission fact", || async {
        h.facts("permission").await.len() == 1
    })
    .await;
    let permission = &h.facts("permission").await[0];
    assert_eq!(permission.data["decided_by"], "console");
    assert_eq!(permission.data["answer"], "allow");
}

/// A `since` that is neither a moment nor a span is refused with a 400
/// naming what it got.
#[tokio::test]
async fn a_bad_since_is_refused() {
    let h = harness().await;
    let error = h
        .error(
            get("/v1/stats/work?since=yesterday"),
            StatusCode::BAD_REQUEST,
        )
        .await;
    assert_eq!(error.error.code, "invalid_request");
    assert!(error.error.message.contains("yesterday"), "{error:?}");
}

/// The families the stats had before the five answer no more.
#[tokio::test]
async fn the_old_families_are_gone() {
    let h = harness().await;
    for family in ["reviews", "switches", "outcomes", "tools"] {
        let (status, _) = h.send(get(&format!("/v1/stats/{family}"))).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{family}");
    }
}

/// A stub whose one prompt turn is held open until `release` exists, and
/// which can load the conversation a resumable agent left behind.
fn held_turn(release: &std::path::Path) -> serde_json::Value {
    let mut scripted = script();
    scripted["stored_sessions"] = json!(["uuid-1234"]);
    scripted["prompts"] = json!([{
        "updates": [],
        "usage": {"totalTokens": 1500, "inputTokens": 1000, "cachedReadTokens": 400,
                  "outputTokens": 100},
        "wait_for": release.display().to_string(),
    }]);
    scripted
}

/// A kill lands while a turn is running. The runtime then cancels the turn,
/// and the cancelled turn reports its stop and what it spent only after the
/// kill has returned. The fact of the run counts that last turn and those
/// tokens: it is written once the agent is reaped, not at the kill.
///
/// The run is a column agent's conversation revived with an instruction,
/// which is the one way a session is put back on its feet outside the
/// scheduler.
#[tokio::test]
async fn a_killed_turn_counts_in_its_fact() {
    let agent_dir = tempfile::tempdir().unwrap();
    let release = agent_dir.path().join("release");
    let stub = stub_acp_agent(agent_dir.path(), held_turn(&release));
    let h = harness().home(registry_home(&stub)).discover_agents().await;
    discovery_settled(&h, &stub).await;
    h.git_repo("repo");
    let (_cast, resumable) = h.resumable_agent().await;
    let session = h
        .launcher
        .revive_session(&resumable.id, Some("go on"))
        .await
        .unwrap();
    let reached = release.with_extension("reached");
    eventually(TIMEOUT, "the turn to be held open", || async {
        reached.exists()
    })
    .await;

    let (status, body) = h
        .send(post(&format!("/v1/sessions/{}/kill", session.id)))
        .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    eventually(TIMEOUT, "the fact of the killed run", || async {
        !h.facts("session_ended").await.is_empty()
    })
    .await;

    let facts = h.facts("session_ended").await;
    assert_eq!(facts.len(), 1, "{facts:?}");
    assert_eq!(facts[0].seat.as_deref(), Some("agent"));
    let usage = h.store.session_usage(&session.id).await.unwrap();
    assert!(
        usage.input_tokens > 0,
        "the cancelled turn reported its usage"
    );
    assert_eq!(facts[0].data["turns"], 1, "{facts:?}");
    assert_eq!(facts[0].data["input_tokens"], usage.input_tokens);
    assert_eq!(
        facts[0].data["cached_input_tokens"],
        usage.cached_input_tokens
    );
    assert_eq!(facts[0].data["output_tokens"], usage.output_tokens);
}

/// A resume right after a kill moves the row to a new launch, and the new
/// agent takes a turn of its own, prompted by the resume. The killed launch
/// still gets exactly one fact, and that fact holds only the killed run: its
/// one cancelled turn and what that turn spent, never the resumed turn. The
/// resumed launch waits for that fact before it starts, so the outcome does
/// not depend on timing.
#[tokio::test]
async fn a_resume_after_a_kill_leaves_the_killed_run_its_own_fact() {
    let agent_dir = tempfile::tempdir().unwrap();
    let release = agent_dir.path().join("release");
    let stub = stub_acp_agent(agent_dir.path(), held_turn(&release));
    let h = harness().home(registry_home(&stub)).discover_agents().await;
    discovery_settled(&h, &stub).await;
    h.git_repo("repo");
    let (_cast, resumable) = h.resumable_agent().await;
    let session = h
        .launcher
        .revive_session(&resumable.id, Some("go on"))
        .await
        .unwrap();
    let reached = release.with_extension("reached");
    eventually(TIMEOUT, "the turn to be held open", || async {
        reached.exists()
    })
    .await;
    let killed = h.launch_id(&session).await.expect("a launch");

    // The next agent's turn ends at once and spends far more.
    let mut next = script();
    next["stored_sessions"] = json!(["uuid-1234"]);
    next["prompts"] = json!([{
        "updates": [],
        "usage": {"totalTokens": 9000, "inputTokens": 5000, "cachedReadTokens": 0,
                  "outputTokens": 900},
    }]);
    stub.reprogram(next);
    discovery_settled(&h, &stub).await;
    let row = format!("/v1/sessions/{}", session.id);
    let (status, body) = h.send(post(&format!("{row}/kill"))).await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    h.launcher
        .revive_session(&session.id, Some("go on"))
        .await
        .unwrap();
    eventually(TIMEOUT, "the resumed turn to end", || async {
        h.store
            .count_session_events(&session.id, "stop")
            .await
            .unwrap()
            >= 2
    })
    .await;
    assert_ne!(
        h.launch_id(&session).await.as_deref(),
        Some(killed.as_str())
    );

    let facts: Vec<_> = h
        .facts("session_ended")
        .await
        .into_iter()
        .filter(|fact| fact.launch_id.as_deref() == Some(killed.as_str()))
        .collect();
    assert_eq!(facts.len(), 1, "{facts:?}");
    let data = &facts[0].data;
    assert_eq!(data["status"], "exited");
    assert_eq!(data["turns"], 1, "{data}");
    assert_eq!(data["input_tokens"], 1400, "{data}");
    assert_eq!(data["cached_input_tokens"], 400, "{data}");
    assert_eq!(data["output_tokens"], 100, "{data}");
}
