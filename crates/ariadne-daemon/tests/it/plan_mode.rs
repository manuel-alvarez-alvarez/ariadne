//! The orchestrator plans in its agent's plan mode (021 rule 21).
//!
//! A launch of the orchestrator of a goal in planning sets the agent's `plan`
//! mode before its first prompt, where the agent offers one: a configuration
//! option of category `mode` with the value `plan`, else an ACP session mode
//! with the id `plan`. `finalize_plan` puts the session back in the mode it
//! ran in before. While the goal is in planning, the daemon refuses the
//! agent's own `ExitPlanMode` in every repository permission mode.
//!
//! The agent is the harness's stub, reprogrammed per test to offer a mode,
//! and every message the daemon sent it is read back from its log.

use crate::common;

use axum::http::StatusCode;
use serde_json::{Value, json};

use ariadne_api::events::AgentEventDto;
use ariadne_core::{GoalStatus, PermissionMode, SessionStatus};
use ariadne_store::AgentSession;

use common::acp::{option, script};
use common::{Cast, Harness, TIMEOUT, as_session, eventually, harness};

/// The modes claude-agent-acp offers, in its order.
const CLAUDE_MODES: [&str; 4] = ["default", "acceptEdits", "plan", "auto"];

/// A configuration option of category `mode`, set to `current`, that offers
/// each of `offered`, as claude-agent-acp offers its permission modes.
fn mode_option(current: &str, offered: &[&str]) -> Value {
    let mut mode = option("mode", "mode", current);
    mode["options"] = offered
        .iter()
        .map(|value| json!({"value": value, "name": value}))
        .collect();
    mode
}

/// The stub's script with `extra` added to its configuration options.
fn script_with_options(extra: Vec<Value>) -> Value {
    let mut scripted = script();
    scripted["stored_sessions"] = json!(["stub-session"]);
    scripted["config_options"]
        .as_array_mut()
        .unwrap()
        .extend(extra);
    scripted
}

/// The stub, offering Claude's modes as a configuration option, in `auto`.
fn claude_like() -> Value {
    script_with_options(vec![mode_option("auto", &CLAUDE_MODES)])
}

/// The goal's orchestrator on the harness's stub, through its first turn.
async fn orchestrator(h: &Harness, cast: &Cast) -> AgentSession {
    let session = h.launcher.spawn_orchestrator(&cast.goal.id).await.unwrap();
    idle(h, &session).await;
    session
}

async fn idle(h: &Harness, session: &AgentSession) {
    eventually(TIMEOUT, "the first turn to end", || async {
        h.session_status(session).await == SessionStatus::Idle
    })
    .await;
}

/// A goal in planning with one staffed task, on the stub, and the stub
/// reprogrammed to `scripted`.
async fn planning(h: &Harness, scripted: Value) -> Cast {
    h.agent.reprogram(scripted);
    h.agent.clear_messages();
    h.cast().await
}

/// Every mode value the daemon set through `session/set_config_option`.
fn modes_set(h: &Harness) -> Vec<String> {
    h.agent
        .calls_of("session/set_config_option")
        .into_iter()
        .filter(|call| call["configId"] == "mode")
        .map(|call| call["value"].as_str().unwrap().to_string())
        .collect()
}

/// The methods the agent was sent, in order, without the mode they carried.
fn methods_with_modes(h: &Harness) -> Vec<String> {
    h.agent
        .messages()
        .into_iter()
        .filter_map(|m| {
            let method = m.get("method")?.as_str()?.to_string();
            Some(
                match m.pointer("/params/configId").and_then(Value::as_str) {
                    Some("mode") => format!("{method} mode={}", m["params"]["value"].as_str()?),
                    _ => method,
                },
            )
        })
        .collect()
}

async fn events(h: &Harness, session: &AgentSession) -> Vec<AgentEventDto> {
    h.get(&format!("/v1/events?session={}", session.id)).await
}

/// The orchestrator of a goal in planning is put in plan mode after its
/// pins and before its first prompt.
#[tokio::test]
async fn a_planning_orchestrator_is_put_in_plan_mode_before_its_first_prompt() {
    let h = harness().await;
    let cast = planning(&h, claude_like()).await;

    orchestrator(&h, &cast).await;

    assert_eq!(
        methods_with_modes(&h),
        [
            "initialize",
            "session/new",
            "session/set_config_option",
            "session/set_config_option mode=plan",
            "session/prompt",
        ]
    );
}

/// An agent with no mode option, but an ACP session mode `plan`, is put in
/// it with `session/set_mode`.
#[tokio::test]
async fn an_agent_offering_a_plan_session_mode_is_put_in_it() {
    let h = harness().await;
    let mut scripted = script_with_options(Vec::new());
    scripted["modes"] = json!({
        "currentModeId": "default",
        "availableModes": [
            {"id": "default", "name": "Default"},
            {"id": "plan", "name": "Plan"},
        ],
    });
    let cast = planning(&h, scripted).await;

    orchestrator(&h, &cast).await;

    let methods = h.agent.methods();
    let set = methods.iter().position(|m| m == "session/set_mode");
    let prompt = methods.iter().position(|m| m == "session/prompt");
    assert!(set.is_some() && set < prompt, "{methods:?}");
    assert_eq!(h.agent.calls_of("session/set_mode")[0]["modeId"], "plan");
}

/// An agent that offers no `plan` mode runs as it always has: no mode call,
/// and nothing recorded about one. A mode option without `plan` is no plan
/// mode either.
#[tokio::test]
async fn an_agent_without_a_plan_mode_gets_no_mode_change() {
    let h = harness().await;
    let cast = planning(
        &h,
        script_with_options(vec![mode_option("auto", &["default", "auto"])]),
    )
    .await;

    let session = orchestrator(&h, &cast).await;

    assert!(modes_set(&h).is_empty(), "{:?}", h.agent.methods());
    assert!(h.agent.calls_of("session/set_mode").is_empty());
    assert!(
        events(&h, &session)
            .await
            .iter()
            .all(|event| event.kind != "session.pin_fallback")
    );
}

/// A refused mode call does not stop the launch: the session runs its turn
/// in the agent's own mode, and an event names the mode and the reason.
#[tokio::test]
async fn a_refused_plan_mode_runs_the_session_in_the_agents_own_mode() {
    let h = harness().await;
    let mut scripted = claude_like();
    scripted["reject_config_value"] = json!("plan");
    let cast = planning(&h, scripted).await;

    let session = orchestrator(&h, &cast).await;

    assert_eq!(h.agent.calls_of("session/prompt").len(), 1);
    let fallback = events(&h, &session)
        .await
        .into_iter()
        .find(|event| event.kind == "session.pin_fallback")
        .expect("the refused mode was recorded")
        .payload;
    assert_eq!(fallback["option"], "mode");
    assert_eq!(fallback["asked"], "plan");
    assert_eq!(fallback["running"], "auto");
    assert!(
        fallback["error"]
            .as_str()
            .unwrap()
            .contains("the agent refused the option"),
        "{fallback}"
    );
}

/// A resumed planning orchestrator is put in plan mode again.
#[tokio::test]
async fn a_resumed_planning_orchestrator_is_put_in_plan_mode_again() {
    let h = harness().await;
    let cast = planning(&h, claude_like()).await;
    let session = orchestrator(&h, &cast).await;
    h.launcher.kill_session(&session.id).await.unwrap();

    h.launcher.revive_session(&session.id, None).await.unwrap();

    eventually(TIMEOUT, "the resumed session in plan mode", || async {
        modes_set(&h).len() == 2
    })
    .await;
    assert_eq!(h.agent.calls_of("session/resume").len(), 1);
    assert_eq!(modes_set(&h), ["plan", "plan"]);
}

/// An orchestrator switched to another agent while the goal is in planning
/// starts in plan mode too.
#[tokio::test]
async fn a_planning_orchestrator_switched_to_another_agent_starts_in_plan_mode() {
    let h = harness().second_agent().await;
    let cast = planning(&h, claude_like()).await;
    let old = orchestrator(&h, &cast).await;

    let new: ariadne_api::sessions::SessionDto = h
        .json(
            common::post_json(
                &format!("/v1/sessions/{}/switch", old.id),
                json!({"model": "other:b"}),
            ),
            StatusCode::OK,
        )
        .await;

    let new = h.store.get_session(&new.id).await.unwrap();
    idle(&h, &new).await;
    assert_eq!(modes_set(&h), ["plan", "plan"]);
}

/// `finalize_plan` puts the live orchestrator back in the mode it ran in
/// before the daemon set `plan`.
#[tokio::test]
async fn finalizing_the_plan_puts_the_orchestrator_back_in_its_own_mode() {
    let h = harness().await;
    let cast = planning(&h, claude_like()).await;
    let session = orchestrator(&h, &cast).await;

    h.json::<Value>(
        as_session(
            &format!("/v1/goals/{}/finalize", cast.goal.id),
            &session.id,
            json!({}),
        ),
        StatusCode::OK,
    )
    .await;

    eventually(TIMEOUT, "the orchestrator out of plan mode", || async {
        modes_set(&h) == ["plan", "auto"]
    })
    .await;
}

/// A session that already ran in `plan` goes back to `default` once the
/// plan is finalized, and one with no `default` to its first other mode.
#[tokio::test]
async fn a_session_that_was_already_planning_goes_back_to_default() {
    for (offered, after) in [
        (&["acceptEdits", "plan", "default"][..], "default"),
        (&["plan", "acceptEdits", "auto"][..], "acceptEdits"),
    ] {
        let h = harness().await;
        let cast = planning(&h, script_with_options(vec![mode_option("plan", offered)])).await;
        let session = orchestrator(&h, &cast).await;

        h.json::<Value>(
            as_session(
                &format!("/v1/goals/{}/finalize", cast.goal.id),
                &session.id,
                json!({}),
            ),
            StatusCode::OK,
        )
        .await;

        eventually(TIMEOUT, "the orchestrator out of plan mode", || async {
            modes_set(&h) == ["plan", after]
        })
        .await;
    }
}

/// An orchestrator launched once the plan is finalized starts in the
/// agent's normal mode.
#[tokio::test]
async fn an_orchestrator_of_an_active_goal_gets_no_mode_change() {
    let h = harness().await;
    let cast = planning(&h, claude_like()).await;
    h.store
        .set_goal_status(&cast.goal.id, GoalStatus::Active)
        .await
        .unwrap();

    orchestrator(&h, &cast).await;

    assert!(modes_set(&h).is_empty(), "{:?}", h.agent.methods());
}

/// A column's agent never changes mode, even on a goal in planning.
#[tokio::test]
async fn a_column_agent_gets_no_mode_change() {
    let h = harness().await;
    h.git_repo("repo");
    let cast = planning(&h, claude_like()).await;

    let session = h.start_agent(&cast.task, "develop").await;
    idle(&h, &session).await;

    assert!(modes_set(&h).is_empty(), "{:?}", h.agent.methods());
}

/// The options claude-agent-acp offers on `ExitPlanMode`.
fn exit_plan_mode_turn() -> Value {
    json!({
        "permission": {
            "toolCall": {
                "toolCallId": "call-exit",
                "title": "Ready to code?",
                "rawInput": {"plan": "the plan"},
                "_meta": {"claudeCode": {"toolName": "ExitPlanMode"}},
            },
            "options": [
                {"optionId": "exit-plan-auto", "name": "Yes, and use auto mode", "kind": "allow_always"},
                {"optionId": "exit-plan-default", "name": "Yes, manually approve edits", "kind": "allow_once"},
                {"optionId": "reject", "name": "No, keep planning", "kind": "reject_once"},
            ],
        },
        "stop_reason": "end_turn",
    })
}

/// What the daemon answered the stub's one permission request.
fn permission_answer(h: &Harness) -> Value {
    h.agent
        .messages()
        .into_iter()
        .find(|m| m["id"] == "permission-1" && m.get("method").is_none())
        .map(|m| m["result"]["outcome"].clone())
        .unwrap_or_default()
}

/// The orchestrator of `goal` asking for `ExitPlanMode` in its first turn,
/// on a repository in `mode`.
async fn asks_to_exit_plan_mode(
    mode: PermissionMode,
    status: GoalStatus,
) -> (Harness, AgentSession) {
    let h = harness().await;
    let mut scripted = claude_like();
    scripted["prompts"] = json!([exit_plan_mode_turn()]);
    let cast = planning(&h, scripted).await;
    h.set_permission_mode(&cast.repo, mode).await;
    h.store
        .set_goal_status(&cast.goal.id, status)
        .await
        .unwrap();
    let session = orchestrator(&h, &cast).await;
    (h, session)
}

/// While the goal is in planning, the daemon refuses `ExitPlanMode` with
/// the agent's rejecting option, in every repository permission mode, and
/// records the answer as its own.
#[tokio::test]
async fn a_planning_orchestrator_is_refused_exit_plan_mode_in_every_permission_mode() {
    for mode in [
        PermissionMode::Auto,
        PermissionMode::Ask,
        PermissionMode::Learn,
        PermissionMode::Ai,
    ] {
        let (h, session) = asks_to_exit_plan_mode(mode, GoalStatus::Planning).await;

        assert_eq!(
            permission_answer(&h),
            json!({"outcome": "selected", "optionId": "reject"}),
            "{mode:?}"
        );
        let replied = events(&h, &session)
            .await
            .into_iter()
            .find(|event| event.kind == "permission.replied")
            .expect("the answer was recorded")
            .payload;
        assert_eq!(replied["option_id"], "reject", "{mode:?}");
        assert_eq!(replied["decided_by"], "plan", "{mode:?}");
    }
}

/// Once the goal is out of planning, `ExitPlanMode` is answered as any other
/// tool: `auto` allows it.
#[tokio::test]
async fn exit_plan_mode_out_of_planning_is_answered_by_the_permission_mode() {
    let (h, _session) = asks_to_exit_plan_mode(PermissionMode::Auto, GoalStatus::Active).await;

    assert_eq!(
        permission_answer(&h),
        json!({"outcome": "selected", "optionId": "exit-plan-auto"})
    );
}
