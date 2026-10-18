//! Switching a session to another model or agent:
//! `POST /v1/sessions/{id}/switch` ends the session and starts a new one on
//! the same seat, on the new pin, in a new conversation.
//!
//! The new agent cannot resume the old conversation, so it is briefed as a
//! fresh spawn of its seat is, handed the old session's history, and told
//! what a resume would have told it. A column's agent keeps its column: its
//! successor is briefed with the column's first briefing, and the staffing
//! of that column is what moves. The agents are the harness's stub, and
//! `git` is real: a column's agent works in the task's real worktree.

use crate::common;

use axum::http::StatusCode;
use serde_json::json;

use ariadne_api::sessions::SessionDto;
use ariadne_core::{GoalStatus, PromptKind, Seat, SessionStatus, TaskStatus};
use ariadne_daemon::agents::prompts;
use ariadne_store::{AgentPin, AgentSession, NewAgentEvent};

use common::acp::{pid_is_alive, registry_home, script, stub_acp_agent};
use common::{Cast, Harness, TIMEOUT, eventually, harness, post_json, put_json};

/// The line every handoff opens with.
const HANDOFF: &str = "This is the history of the session you continue.";

/// The old pin, and the one every switch below moves to.
const FROM: &str = "stub:a";
const TO: &str = "other:b";
const SAME_TO: &str = "stub:b";

/// Switch `session` to `model` over the endpoint, and answer the new session.
async fn switch(h: &Harness, session: &AgentSession, model: &str) -> SessionDto {
    h.json(
        post_json(
            &format!("/v1/sessions/{}/switch", session.id),
            json!({ "model": model }),
        ),
        StatusCode::OK,
    )
    .await
}

/// Wait until `session` has run its first turn whole: the briefing went in,
/// and the agent's answer came back.
async fn first_turn_done(h: &Harness, session: &AgentSession) {
    eventually(TIMEOUT, "the first turn to end", || async {
        h.session_status(session).await == SessionStatus::Idle
    })
    .await;
}

/// A task in its first column on a repo on disk, its agents pinned to
/// [`FROM`]: the goal active and the task in progress, as a column's agent
/// is only ever started on.
async fn in_progress(h: &Harness) -> Cast {
    h.git_repo("repo");
    let cast = h.cast_pinned(FROM).await;
    let goal = h.activate(&cast.goal).await;
    h.advance(&cast.task, TaskStatus::InProgress).await;
    let task = h.store.get_task(&cast.task.id).await.unwrap();
    Cast { goal, task, ..cast }
}

/// Start the develop column's agent and hand it the column's first briefing,
/// the way the scheduler does once the agent is up: the launch itself carries
/// no prompt. Answers the session and the briefing it was handed.
async fn briefed_develop(h: &Harness, cast: &Cast) -> (AgentSession, String) {
    let session = h
        .launcher
        .start_step_agent(&cast.task, cast.develop())
        .await
        .unwrap();
    let task = h.store.get_task(&cast.task.id).await.unwrap();
    let steps = h.store.goal_steps(&cast.goal.id).await.unwrap();
    let step = steps.iter().find(|s| s.id == "develop").unwrap();
    let briefing = h
        .launcher
        .step_first_briefing(&task, step, "")
        .await
        .unwrap();
    h.launcher
        .acp
        .send_prompt(&session.id, briefing.clone())
        .unwrap();
    (session, briefing)
}

#[tokio::test]
async fn a_same_agent_switch_keeps_the_row_and_conversation() {
    let h = harness().second_agent().await;
    let cast = in_progress(&h).await;
    let (old, _) = briefed_develop(&h, &cast).await;
    first_turn_done(&h, &old).await;
    let internal = h
        .store
        .get_session(&old.id)
        .await
        .unwrap()
        .internal_session_id;
    let launches = h.agent.launches_for(&old.id).len();

    let changed = switch(&h, &old, SAME_TO).await;

    assert_eq!(changed.id, old.id);
    assert_eq!(changed.switched_from, None);
    assert_eq!(changed.model, SAME_TO);
    assert_eq!(
        h.store
            .get_session(&old.id)
            .await
            .unwrap()
            .internal_session_id,
        internal
    );
    assert_eq!(h.agent.launches_for(&old.id).len(), launches);
    assert_eq!(switched_event(&h, &old).await["to"], json!(old.id));
    assert!(
        h.agent
            .calls_of("session/set_config_option")
            .iter()
            .any(|call| call["value"] == "b")
    );
    assert_eq!(
        h.store
            .get_task_agent(&cast.develop().id)
            .await
            .unwrap()
            .model,
        SAME_TO
    );
}

#[tokio::test]
async fn a_same_agent_switch_sets_effort_and_clears_the_old_pin() {
    let h = harness().second_agent().await;
    let cast = in_progress(&h).await;
    let (old, _) = briefed_develop(&h, &cast).await;
    first_turn_done(&h, &old).await;
    let uri = format!("/v1/sessions/{}/switch", old.id);
    let with_effort: SessionDto = h
        .json(
            post_json(&uri, json!({"model": SAME_TO, "effort": "high"})),
            StatusCode::OK,
        )
        .await;
    assert_eq!(with_effort.effort.as_deref(), Some("high"));
    assert!(
        h.agent
            .calls_of("session/set_config_option")
            .iter()
            .any(|call| call["value"] == "high")
    );

    let cleared: SessionDto = h
        .json(post_json(&uri, json!({"model": FROM})), StatusCode::OK)
        .await;
    assert_eq!(cleared.effort, None);
    assert_eq!(
        h.store
            .get_task_agent(&cast.develop().id)
            .await
            .unwrap()
            .effort,
        None
    );
}

#[tokio::test]
async fn an_ended_same_agent_session_uses_its_new_pin_on_revival() {
    let h = harness().second_agent().await;
    let cast = in_progress(&h).await;
    let (old, _) = briefed_develop(&h, &cast).await;
    first_turn_done(&h, &old).await;
    h.launcher.kill_session(&old.id).await.unwrap();
    let calls = h.agent.calls_of("session/set_config_option").len();

    let changed = switch(&h, &old, SAME_TO).await;
    assert_eq!(changed.id, old.id);
    assert_eq!(h.agent.calls_of("session/set_config_option").len(), calls);
    h.launcher.revive_session(&old.id, None).await.unwrap();
    eventually(TIMEOUT, "the revived pin", || async {
        h.agent
            .calls_of("session/set_config_option")
            .iter()
            .skip(calls)
            .any(|call| call["value"] == "b")
    })
    .await;
    assert_eq!(h.launch_file(&old.id).unwrap().model, "b");
}

#[tokio::test]
async fn a_refused_same_agent_option_keeps_the_old_pin() {
    let dir = tempfile::tempdir().unwrap();
    let mut scripted = script();
    scripted["reject_config_value"] = json!("b");
    let stub = stub_acp_agent(dir.path(), scripted);
    let h = harness().home(registry_home(&stub)).await;
    let cast = in_progress(&h).await;
    let (old, _) = briefed_develop(&h, &cast).await;
    first_turn_done(&h, &old).await;

    let err = h
        .error(
            post_json(
                &format!("/v1/sessions/{}/switch", old.id),
                json!({"model": SAME_TO}),
            ),
            StatusCode::CONFLICT,
        )
        .await;

    assert!(
        err.error.message.contains("the agent refused the option"),
        "{}",
        err.error.message
    );
    assert_eq!(h.store.get_session(&old.id).await.unwrap().model, FROM);
    assert_eq!(
        h.store
            .get_task_agent(&cast.develop().id)
            .await
            .unwrap()
            .model,
        FROM
    );
}

#[tokio::test]
async fn a_same_agent_switch_during_a_turn_precedes_queued_input() {
    let dir = tempfile::tempdir().unwrap();
    let release = dir.path().join("release");
    let mut scripted = script();
    scripted["prompts"][0]["wait_for"] = json!(release.display().to_string());
    let stub = stub_acp_agent(dir.path(), scripted);
    let h = harness().home(registry_home(&stub)).await;
    let cast = in_progress(&h).await;
    let (old, _) = briefed_develop(&h, &cast).await;
    eventually(TIMEOUT, "the held turn", || async {
        release.with_extension("reached").exists()
    })
    .await;

    let changed = switch(&h, &old, SAME_TO).await;
    assert_eq!(changed.id, old.id);
    let (status, _) = h
        .send(post_json(
            &format!("/v1/sessions/{}/console/input", old.id),
            json!({"text": "queued"}),
        ))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    std::fs::write(&release, "go").unwrap();
    eventually(TIMEOUT, "the queued prompt", || async {
        stub.calls_of("session/prompt").len() == 2
    })
    .await;

    let messages = stub.messages();
    let switched = messages
        .iter()
        .rposition(|m| {
            method(m) == Some("session/set_config_option") && m["params"]["value"] == "b"
        })
        .unwrap();
    let queued = messages
        .iter()
        .rposition(|m| method(m) == Some("session/prompt"))
        .unwrap();
    assert!(switched < queued);
    assert_eq!(stub.launches_for(&old.id).len(), 1);
}

/// The first prompt the agent of `session_id` was launched with.
fn first_prompt(h: &Harness, session_id: &str) -> String {
    h.launch_file(session_id)
        .and_then(|launch| launch.initial_prompt)
        .expect("a launch file with a first prompt")
}

/// The `session.switched` event `session` keeps, as JSON.
async fn switched_event(h: &Harness, session: &AgentSession) -> serde_json::Value {
    let events = h.store.list_session_events(&session.id).await.unwrap();
    let event = events
        .iter()
        .find(|e| e.kind == "session.switched")
        .expect("a session.switched event on the old session");
    serde_json::from_str(&event.payload).unwrap()
}

/// `prompt` is `briefing`, a blank line, the handoff and — where the seat has
/// one — a blank line and `resume`, in that order, and the handoff holds what
/// the old session's first turn said.
fn assert_briefing_handoff_resume(prompt: &str, briefing: &str, resume: Option<&str>) {
    let handoff = prompt
        .strip_prefix(&format!("{briefing}\n\n"))
        .unwrap_or_else(|| panic!("the prompt opens with the seat's briefing: {prompt}"));
    let handoff = match resume {
        Some(resume) => handoff
            .strip_suffix(&format!("\n\n{resume}"))
            .unwrap_or_else(|| panic!("the prompt ends with the resume text: {prompt}")),
        None => handoff,
    };
    assert!(handoff.starts_with(HANDOFF), "{handoff}");
    assert!(
        handoff.contains("agent\ndone"),
        "the handoff holds the old agent's answer: {handoff}"
    );
}

/// A column's agent switched from one model to another gets a new row that
/// names the old one, on the same column, in the task's one worktree. The old
/// row is exited and records the switch. The new agent is briefed with the
/// column's first briefing and handed the history — the column's entry is
/// the scheduler's to send, so nothing follows the handoff — and the column's
/// staffing moves to the new model, and only that column's.
#[tokio::test]
async fn a_switched_agent_starts_a_new_session_briefed_on_its_column() {
    let h = harness().second_agent().await;
    let cast = in_progress(&h).await;
    let (old, briefing) = briefed_develop(&h, &cast).await;
    first_turn_done(&h, &old).await;

    let new = switch(&h, &old, TO).await;

    assert_ne!(new.id, old.id);
    assert_eq!(new.switched_from.as_deref(), Some(old.id.as_str()));
    assert_eq!(new.seat, Some(Seat::Agent));
    assert_eq!(new.task_id, old.task_id);
    assert_eq!(new.task_agent_id, old.task_agent_id);
    assert_eq!(new.worktree_path, old.worktree_path);
    assert_eq!(new.model, TO);
    assert!(new.status.is_live());

    assert_eq!(h.session_status(&old).await, SessionStatus::Exited);
    let event = switched_event(&h, &old).await;
    assert_eq!(
        event,
        json!({
            "session_id": old.id,
            "to": new.id,
            "model": TO,
            "effort": null,
            "reason": "requested",
        })
    );

    assert_briefing_handoff_resume(&first_prompt(&h, &new.id), &briefing, None);
    assert_eq!(h.launch_file(&new.id).unwrap().model, "b");

    let develop = h.store.get_task_agent(&cast.develop().id).await.unwrap();
    assert_eq!(develop.model, TO);
    let review = h.store.get_task_agent(&cast.review().id).await.unwrap();
    assert_eq!(
        review.model, FROM,
        "only the switched column's staffing moves"
    );
}

#[tokio::test]
async fn a_switched_agent_receives_an_older_user_correction_before_recent_noise() {
    let h = harness().second_agent().await;
    let cast = in_progress(&h).await;
    let (old, _) = briefed_develop(&h, &cast).await;
    first_turn_done(&h, &old).await;
    h.store
        .create_event(NewAgentEvent {
            session_id: Some(old.id.clone()),
            task_id: old.task_id.clone(),
            kind: "user_prompt_submit".into(),
            payload: json!({"text": "Correction: preserve the release blocker", "source": "console"}),
        })
        .await
        .unwrap();
    h.store
        .create_event(NewAgentEvent {
            session_id: Some(old.id.clone()),
            task_id: old.task_id.clone(),
            kind: "agent_message".into(),
            payload: json!({"text": "recent routine output ".repeat(13_000)}),
        })
        .await
        .unwrap();

    let new = switch(&h, &old, TO).await;
    let prompt = first_prompt(&h, &new.id);

    assert!(
        prompt.contains("Correction: preserve the release blocker"),
        "{prompt}"
    );
    assert!(!prompt.contains("recent routine output"), "{prompt}");
}

/// An orchestrator switches the same way, and the goal's pin is what moves:
/// the orchestrator's next spawn runs on the new model.
#[tokio::test]
async fn a_switched_orchestrator_moves_the_goals_pin() {
    let h = harness().second_agent().await;
    let repo = h.repository(&h.at("repo")).await;
    let goal = h
        .goal_on(
            &repo,
            AgentPin {
                model: FROM.into(),
                effort: None,
            },
        )
        .await;
    let old = h.launcher.spawn_orchestrator(&goal.id).await.unwrap();
    first_turn_done(&h, &old).await;
    let briefing = first_prompt(&h, &old.id);

    let new = switch(&h, &old, TO).await;

    assert_eq!(new.switched_from.as_deref(), Some(old.id.as_str()));
    assert_eq!(new.seat, Some(Seat::Orchestrator));
    assert_eq!(new.goal_id.as_deref(), Some(goal.id.as_str()));
    assert_eq!(h.session_status(&old).await, SessionStatus::Exited);
    assert_eq!(switched_event(&h, &old).await["to"], json!(new.id));

    let resume =
        prompts::template_for(PromptKind::OrchestratorResume).replace("{goal_title}", &goal.title);
    assert_briefing_handoff_resume(&first_prompt(&h, &new.id), &briefing, Some(&resume));

    let goal = h.store.get_goal(&goal.id).await.unwrap();
    assert_eq!(goal.model, TO);
}

/// A loose session has no briefing and no resume text: its successor gets
/// the history alone, in the same directory, and no seat pin moves.
#[tokio::test]
async fn a_switched_loose_session_gets_the_handoff_alone() {
    let h = harness().second_agent().await;
    let work = h.at("work");
    std::fs::create_dir(&work).unwrap();
    let old: SessionDto = h
        .json(
            post_json(
                "/v1/sessions",
                json!({ "model": FROM, "working_directory": work.to_str().unwrap() }),
            ),
            StatusCode::OK,
        )
        .await;
    let old = h.store.get_session(&old.id).await.unwrap();
    let (status, _) = h
        .send(post_json(
            &format!("/v1/sessions/{}/console/input", old.id),
            json!({"text": "Tidy the release notes"}),
        ))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    first_turn_done(&h, &old).await;

    let new = switch(&h, &old, TO).await;

    assert_eq!(new.switched_from.as_deref(), Some(old.id.as_str()));
    assert_eq!(new.seat, None);
    assert_eq!(new.goal_id, None);
    assert_eq!(new.worktree_path.as_deref(), work.to_str());
    assert_eq!(new.model, TO);
    assert_eq!(h.session_status(&old).await, SessionStatus::Exited);
    assert_eq!(switched_event(&h, &old).await["to"], json!(new.id));

    let new_row = h.store.get_session(&new.id).await.unwrap();
    eventually(TIMEOUT, "the new agent's first prompt", || async {
        !h.prompts_to(&new_row).is_empty()
    })
    .await;
    // A loose session has no system prompt, so the runtime's blank line
    // before the prompt is all that precedes it.
    let prompt = h.prompts_to(&new_row).remove(0);
    assert!(
        prompt.trim_start().starts_with(HANDOFF),
        "the handoff alone: {prompt}"
    );
    assert!(prompt.contains("Tidy the release notes"), "{prompt}");
    assert!(prompt.contains("agent\ndone"), "{prompt}");
}

/// A session switched in the middle of a turn has that turn cancelled and
/// its agent killed and reaped before the new agent starts.
#[tokio::test]
async fn a_session_switched_mid_turn_is_cancelled_and_killed_before_the_new_one_starts() {
    let h = harness().second_agent().await;
    let mut held = script();
    held["stored_sessions"] = json!(["uuid-1234", "stub-session"]);
    held["prompts"][0]["wait_for"] = json!(h.at("never").display().to_string());
    h.agent.reprogram(held);
    let cast = in_progress(&h).await;
    let (old, _) = briefed_develop(&h, &cast).await;
    eventually(TIMEOUT, "the first turn to start", || async {
        !h.prompts_to(&old).is_empty()
    })
    .await;
    let old_pid = h.agent.pid().expect("the old agent's pid");

    let new = switch(&h, &old, TO).await;

    assert!(
        !pid_is_alive(old_pid),
        "the old agent is reaped before the switch answers"
    );
    eventually(TIMEOUT, "the new agent to start", || async {
        h.agent.messages().iter().any(|m| from(m, &new.id))
    })
    .await;
    let messages = h.agent.messages();
    let cancel = messages
        .iter()
        .position(|m| from(m, &old.id) && method(m) == Some("session/cancel"))
        .expect("the old turn was cancelled");
    let first_of_new = messages
        .iter()
        .position(|m| from(m, &new.id))
        .expect("the new agent was started");
    assert!(
        cancel < first_of_new,
        "the cancel went out before the new agent started"
    );
    assert_eq!(h.session_status(&old).await, SessionStatus::Exited);
}

/// Whether the stub logged `message` from the agent of `session_id`.
fn from(message: &serde_json::Value, session_id: &str) -> bool {
    message.get("ariadne_session").and_then(|s| s.as_str()) == Some(session_id)
}

/// The method of a logged message, where it is a request or a notification.
fn method(message: &serde_json::Value) -> Option<&str> {
    message.get("method").and_then(|m| m.as_str())
}

/// The live sessions of the columns' agents on `task_id`.
async fn live_agents(h: &Harness, task_id: &str) -> Vec<AgentSession> {
    h.store
        .list_sessions(ariadne_store::SessionFilter {
            task_id: Some(task_id.to_string()),
            live_only: true,
            ..Default::default()
        })
        .await
        .unwrap()
        .into_iter()
        .filter(|s| s.seat() == Some(Seat::Agent))
        .collect()
}

/// A switch repeated against a session already switched is refused, and
/// starts no second agent on the seat.
#[tokio::test]
async fn a_session_already_switched_is_not_switched_again() {
    let h = harness().second_agent().await;
    let cast = in_progress(&h).await;
    let (old, _) = briefed_develop(&h, &cast).await;
    first_turn_done(&h, &old).await;
    let new = switch(&h, &old, TO).await;

    let err = h
        .error(
            post_json(
                &format!("/v1/sessions/{}/switch", old.id),
                json!({ "model": FROM }),
            ),
            StatusCode::CONFLICT,
        )
        .await;

    assert!(
        err.error.message.contains("already switched") && err.error.message.contains(&new.id),
        "{}",
        err.error.message
    );
    let live: Vec<String> = live_agents(&h, &cast.task.id)
        .await
        .into_iter()
        .map(|s| s.id)
        .collect();
    assert_eq!(live, std::slice::from_ref(&new.id));
    let develop = h.store.get_task_agent(&cast.develop().id).await.unwrap();
    assert_eq!(develop.model, TO, "the refused switch moves no pin");
}

/// Two switches of one session at once start one successor: the other is
/// refused.
#[tokio::test]
async fn two_switches_of_one_session_at_once_start_one_successor() {
    let h = harness().second_agent().await;
    let cast = in_progress(&h).await;
    let (old, _) = briefed_develop(&h, &cast).await;
    first_turn_done(&h, &old).await;
    let uri = format!("/v1/sessions/{}/switch", old.id);

    let (first, second) = tokio::join!(
        h.send(post_json(&uri, json!({ "model": TO }))),
        h.send(post_json(&uri, json!({ "model": TO }))),
    );

    let mut statuses = [first.0, second.0];
    statuses.sort();
    assert_eq!(statuses, [StatusCode::OK, StatusCode::CONFLICT]);
    let live = live_agents(&h, &cast.task.id).await;
    assert_eq!(live.len(), 1, "{live:?}");
    assert_eq!(live[0].switched_from.as_deref(), Some(old.id.as_str()));
}

/// A session of a cancelled goal is not switched, and an id that names no
/// session is a 404.
#[tokio::test]
async fn a_session_of_a_cancelled_goal_is_not_switched() {
    let h = harness().second_agent().await;
    let cast = h.cast().await;
    let session = h.agent_session(&cast, "develop").await;
    h.store
        .set_goal_status(&cast.goal.id, GoalStatus::Cancelled)
        .await
        .unwrap();

    let err = h
        .error(
            post_json(
                &format!("/v1/sessions/{}/switch", session.id),
                json!({ "model": TO }),
            ),
            StatusCode::CONFLICT,
        )
        .await;
    assert!(
        err.error.message.contains("cancelled"),
        "{}",
        err.error.message
    );

    h.error(
        post_json(
            "/v1/sessions/no-such-session/switch",
            json!({ "model": TO }),
        ),
        StatusCode::NOT_FOUND,
    )
    .await;
    let develop = h.store.get_task_agent(&cast.develop().id).await.unwrap();
    assert_ne!(develop.model, TO, "a refused switch moves no pin");
}

/// A model the user turned off is refused as a pin on it is, and so is a
/// model whose agent the registry does not hold.
#[tokio::test]
async fn a_switch_to_a_model_that_is_turned_off_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let mut two_models = script();
    two_models["config_options"][0]["options"] = json!([
        {"value": "old-model", "name": "The old one"},
        {"value": "new-model", "name": "The new one"},
    ]);
    let stub = stub_acp_agent(dir.path(), two_models);
    let h = harness().home(registry_home(&stub)).discover_agents().await;
    common::acp::discovery_settled(&h, &stub).await;
    let cast = h.cast().await;
    let session = h.agent_session(&cast, "develop").await;
    let off = "stub:old-model";
    let _: serde_json::Value = h
        .json(
            put_json("/v1/models/enabled", json!({"id": off, "enabled": false})),
            StatusCode::OK,
        )
        .await;
    let uri = format!("/v1/sessions/{}/switch", session.id);

    let err = h
        .error(
            post_json(&uri, json!({ "model": off })),
            StatusCode::BAD_REQUEST,
        )
        .await;
    assert!(
        err.error.message.contains(off) && err.error.message.contains("turned off"),
        "{}",
        err.error.message
    );
    h.error(
        post_json(&uri, json!({ "model": "nobody:model" })),
        StatusCode::BAD_REQUEST,
    )
    .await;
    assert_eq!(
        h.session_status(&session).await,
        SessionStatus::Starting,
        "a refused switch leaves the session alone"
    );
}

#[tokio::test]
async fn the_switch_endpoint_is_in_the_openapi_document() {
    let h = harness().second_agent().await;
    let doc: serde_json::Value = h.get("/api-docs/openapi.json").await;
    let post = &doc["paths"]["/v1/sessions/{id}/switch"]["post"];
    assert_eq!(
        post["requestBody"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/SwitchSessionRequest"
    );
    assert_eq!(
        post["responses"]["200"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/SessionDto"
    );
    assert!(doc["components"]["schemas"]["SessionDto"]["properties"]["switched_from"].is_object());
}
