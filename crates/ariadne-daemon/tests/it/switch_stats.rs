//! Integration tests for the switch family of stats (023): the `switch` fact
//! `Launcher::switch_session` writes next to its `session.switched` event,
//! and `GET /v1/stats/switches` over it.

use crate::common;

use axum::http::StatusCode;
use serde_json::{Value, json};

use ariadne_api::sessions::SessionDto;
use ariadne_api::stats::SwitchStatsResponse;
use ariadne_core::models::ModelRank;
use ariadne_core::{SessionStatus, TaskStatus};
use ariadne_daemon::scheduler::{self, SchedEvent};
use ariadne_store::{AgentPin, AgentSession};

use common::acp::{discovery_accepted, option, script, stub_acp_agent};
use common::{Harness, TIMEOUT, eventually, harness, post_json};

/// The old pin, and the one a manual switch below moves to.
const FROM: &str = "stub:a";
const TO: &str = "other:b";
const SAME_TO: &str = "stub:b";

/// Switch `session_id` to `model` over the endpoint.
async fn switch(h: &Harness, session_id: &str, model: &str) -> SessionDto {
    h.json(
        post_json(
            &format!("/v1/sessions/{session_id}/switch"),
            json!({ "model": model }),
        ),
        StatusCode::OK,
    )
    .await
}

/// Wait until `session` has run its first turn whole.
async fn first_turn_done(h: &Harness, session: &AgentSession) {
    eventually(TIMEOUT, "the first turn to end", || async {
        h.session_status(session).await == SessionStatus::Idle
    })
    .await;
}

/// A cross-agent switch leaves one `switch` fact on the model left, naming
/// the model entered, `reason=requested` and `automatic=false`.
#[tokio::test]
async fn a_manual_switch_writes_one_switch_fact_with_the_model_left_and_entered() {
    let h = harness().second_agent().await;
    h.git_repo("repo");
    let cast = h.cast_pinned(FROM, 1).await;
    let old = h.launcher.spawn_author(&cast.task.id).await.unwrap();
    first_turn_done(&h, &old).await;

    switch(&h, &old.id, TO).await;

    let facts = h.facts("switch").await;
    assert_eq!(facts.len(), 1, "{facts:?}");
    let fact = &facts[0];
    assert_eq!(fact.model.as_deref(), Some(FROM));
    assert_eq!(fact.data["to_model"], TO);
    assert_eq!(fact.data["reason"], "requested");
    assert_eq!(fact.data["automatic"], false);
    assert_eq!(fact.data["same_agent"], false);
}

/// A same-agent switch leaves the same one fact, and names the model the
/// session left, not the one its row was just moved onto.
#[tokio::test]
async fn a_same_agent_switch_writes_the_fact_before_the_pin_moves() {
    let h = harness().second_agent().await;
    h.git_repo("repo");
    let cast = h.cast_pinned(FROM, 1).await;
    let old = h.launcher.spawn_author(&cast.task.id).await.unwrap();
    first_turn_done(&h, &old).await;

    switch(&h, &old.id, SAME_TO).await;

    let facts = h.facts("switch").await;
    assert_eq!(facts.len(), 1, "{facts:?}");
    let fact = &facts[0];
    assert_eq!(
        fact.model.as_deref(),
        Some(FROM),
        "the model left, not the one the row holds now"
    );
    assert_eq!(fact.data["to_model"], SAME_TO);
    assert_eq!(fact.data["same_agent"], true);
}

fn codex_error() -> Value {
    json!({
        "code": -32603,
        "message": "You've hit your usage limit. Try again at 7:00 PM",
        "data": {"message": "limit", "codexErrorInfo": "usageLimitExceeded"}
    })
}

fn catalog_script(models: &[&str]) -> Value {
    let mut value = script();
    value["config_options"] = json!([
        {
            "id": "model-id", "name": "model-id", "category": "model",
            "type": "select", "currentValue": models[0],
            "options": models.iter().map(|model| json!({"value": model, "name": model})).collect::<Vec<_>>()
        },
        option("effort-id", "thought_level", "low"),
    ]);
    value
}

/// A goal whose orchestrator and author are both live: the orchestrator on
/// `other:old-model` (a healthy agent offering a second model to switch onto
/// by hand), the author on `codex:old`, whose agent errors with an
/// exhaustion signal on its first turn.
async fn exhausted_world() -> (Harness, AgentSession, AgentSession, tempfile::TempDir) {
    let root = tempfile::tempdir().unwrap();
    let exhausted_dir = root.path().join("exhausted");
    let healthy_dir = root.path().join("healthy");
    std::fs::create_dir_all(&exhausted_dir).unwrap();
    std::fs::create_dir_all(&healthy_dir).unwrap();
    let mut exhausted_script = catalog_script(&["old"]);
    exhausted_script["prompts"][0] = json!({"error": codex_error()});
    let exhausted = stub_acp_agent(&exhausted_dir, exhausted_script);
    let healthy = stub_acp_agent(&healthy_dir, catalog_script(&["old-model", "other-model"]));
    let home = root.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let config = format!(
        "auto_switch = true\n[[acp_agents]]\nid = \"codex\"\ncommand = [{:?}]\n[[acp_agents]]\nid = \"other\"\ncommand = [{:?}]\n",
        exhausted.bin, healthy.bin
    );
    std::fs::write(home.join("config.toml"), config).unwrap();
    let h = harness().home(home).discover_agents().await;
    discovery_accepted(&h, &exhausted, "codex").await;
    discovery_accepted(&h, &healthy, "other").await;
    h.git_repo("repo");
    let repo = h.repository(&h.at("repo")).await;
    let goal = h
        .goal_on(
            &repo,
            AgentPin {
                model: "other:old-model".into(),
                effort: None,
            },
        )
        .await;
    let task = h
        .task_on(
            &goal,
            &repo,
            "task",
            1,
            AgentPin {
                model: "codex:old".into(),
                effort: None,
            },
        )
        .await;
    let goal = h.activate(&goal).await;
    h.advance(&task, TaskStatus::InProgress).await;
    let orchestrator = h.launcher.spawn_orchestrator(&goal.id).await.unwrap();
    eventually(TIMEOUT, "the orchestrator to become idle", || async {
        h.session_status(&orchestrator).await == SessionStatus::Idle
    })
    .await;
    let author = h.launcher.spawn_author(&task.id).await.unwrap();
    eventually(TIMEOUT, "the failed author to end", || async {
        h.session_status(&author).await == SessionStatus::Exited
    })
    .await;
    h.store
        .set_model_rank("codex:old", Some(ModelRank::Balanced))
        .await
        .unwrap();
    h.store
        .set_model_rank("other:old-model", Some(ModelRank::Balanced))
        .await
        .unwrap();
    (h, orchestrator, author, root)
}

/// An exhausted session that auto-switches leaves one `switch` fact,
/// `reason=exhausted` and `automatic=true`.
#[tokio::test]
async fn an_exhausted_session_that_auto_switches_writes_one_switch_fact() {
    let (h, _orchestrator, author, _root) = exhausted_world().await;
    let scheduler = scheduler::start(h.store.clone(), h.launcher.clone(), false, h.timeouts);
    scheduler
        .send(SchedEvent::SessionEvent(author.id.clone()))
        .unwrap();

    eventually(TIMEOUT, "the exhausted session to switch", || async {
        h.store
            .switched_successor(&author.id)
            .await
            .unwrap()
            .is_some()
    })
    .await;

    let facts = h.facts("switch").await;
    assert_eq!(facts.len(), 1, "{facts:?}");
    let fact = &facts[0];
    assert_eq!(fact.model.as_deref(), Some("codex:old"));
    assert_eq!(fact.data["to_model"], "other:old-model");
    assert_eq!(fact.data["reason"], "exhausted");
    assert_eq!(fact.data["automatic"], true);
}

/// `GET /v1/stats/switches` returns both a manual switch and an exhausted
/// auto-switch, each in the row of the model it left; the model the
/// automatic switch arrived on counts the arrival.
#[tokio::test]
async fn the_switches_stat_returns_both_in_the_row_of_the_model_left() {
    let (h, orchestrator, author, _root) = exhausted_world().await;
    switch(&h, &orchestrator.id, "other:other-model").await;

    let scheduler = scheduler::start(h.store.clone(), h.launcher.clone(), false, h.timeouts);
    scheduler
        .send(SchedEvent::SessionEvent(author.id.clone()))
        .unwrap();
    eventually(TIMEOUT, "the exhausted session to switch", || async {
        h.store
            .switched_successor(&author.id)
            .await
            .unwrap()
            .is_some()
    })
    .await;

    let stats: SwitchStatsResponse = h.get("/v1/stats/switches").await;
    let manual = stats
        .items
        .iter()
        .find(|row| row.model == "other:old-model")
        .expect("a row for the model the manual switch left");
    assert_eq!(manual.switches, 1);
    assert_eq!(manual.exhaustions, 0);
    assert_eq!(manual.automatic_share, 0.0);
    assert_eq!(manual.arrivals, 1, "the exhausted switch arrived here too");

    let exhausted = stats
        .items
        .iter()
        .find(|row| row.model == "codex:old")
        .expect("a row for the model the exhausted switch left");
    assert_eq!(exhausted.switches, 1);
    assert_eq!(exhausted.exhaustions, 1);
    assert_eq!(exhausted.automatic_share, 1.0);
}
