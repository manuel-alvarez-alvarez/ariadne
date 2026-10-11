use serde_json::{Value, json};

use ariadne_core::models::ModelRank;
use ariadne_core::{AttentionReason, Seat, SessionStatus};
use ariadne_daemon::scheduler::{self, SPAWN_RETRY_BUDGET, SchedEvent};
use ariadne_store::{AgentPin, AgentSession, SessionFilter, Task};

use crate::common::acp::{StubAcpAgent, discovery_accepted, option, script, stub_acp_agent};
use crate::common::{Harness, TIMEOUT, eventually, eventually_some, harness};

/// Start the agent of the task's first column and hand it a first prompt, the
/// way the scheduler briefs a column's agent once it is up: the launch itself
/// carries none, and the exhausted stub errors on its first turn.
async fn started(h: &Harness, task: &Task) -> AgentSession {
    let task = h.store.get_task(&task.id).await.unwrap();
    let develop = h.store.list_task_agents(&task.id).await.unwrap().remove(0);
    let session = h.launcher.start_step_agent(&task, &develop).await.unwrap();
    h.launcher
        .acp
        .send_prompt(&session.id, "Begin the task.".into())
        .unwrap();
    session
}

/// The world the exhaustion tests run in: the develop column's agent, whose
/// first turn exhausted, and the orchestrator that is told of each switch.
struct World {
    h: Harness,
    agent: AgentSession,
    orchestrator: AgentSession,
    scheduler: tokio::sync::mpsc::UnboundedSender<SchedEvent>,
    control: StubAcpAgent,
    target: StubAcpAgent,
    _root: tempfile::TempDir,
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
    // An in-place model switch keeps the session id and resumes it on the
    // new pin rather than opening a fresh one; without this, the resume is
    // of a session the stub never heard of, and it refuses with a plain
    // protocol error instead of running the turn the switched model owes.
    value["stored_sessions"] = json!(["stub-session"]);
    value
}

fn agent_script(error: Value, models: &[&str]) -> Value {
    prompt_script(json!({"error": error}), models)
}

fn prompt_script(prompt: Value, models: &[&str]) -> Value {
    let mut value = catalog_script(models);
    // Two turns, not one: an in-place switch keeps the same stub process and
    // its queued prompts, so the model that takes over is given the retried
    // prompt out of the same list. One entry would let that retry succeed,
    // which recovers the session instead of exhausting it again.
    value["prompts"] = json!([prompt.clone(), prompt]);
    value
}

async fn world(error: Value, auto_switch: bool, agents: &[&str]) -> World {
    world_with_prompt(json!({"error": error}), auto_switch, agents).await
}

async fn world_with_prompt(prompt: Value, auto_switch: bool, agents: &[&str]) -> World {
    let root = tempfile::tempdir().unwrap();
    let exhausted_dir = root.path().join("exhausted");
    let healthy_dir = root.path().join("healthy");
    let control_dir = root.path().join("control");
    for dir in [&exhausted_dir, &healthy_dir, &control_dir] {
        std::fs::create_dir_all(dir).unwrap();
    }
    let exhausted = stub_acp_agent(
        &exhausted_dir,
        prompt_script(prompt, &["old", "same", "frontier", "fast"]),
    );
    let healthy = stub_acp_agent(
        &healthy_dir,
        catalog_script(&["old-model", "same", "frontier", "fast"]),
    );
    let control = stub_acp_agent(&control_dir, script());
    let home = root.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let mut config = format!(
        "auto_switch = {auto_switch}\n[[acp_agents]]\nid = \"control\"\ncommand = [{:?}]\n[[acp_agents]]\nid = \"codex\"\ncommand = [{:?}]\n",
        control.bin, exhausted.bin
    );
    for agent in agents {
        config.push_str(&format!(
            "[[acp_agents]]\nid = {agent:?}\ncommand = [{:?}]\n",
            healthy.bin
        ));
    }
    std::fs::write(home.join("config.toml"), config).unwrap();
    let h = harness().home(home).discover_agents().await;
    discovery_accepted(&h, &control, "control").await;
    discovery_accepted(&h, &exhausted, "codex").await;
    for agent in agents {
        discovery_accepted(&h, &healthy, agent).await;
    }
    h.git_repo("repo");
    let repo = h.repository(&h.at("repo")).await;
    let goal = h
        .goal_on(
            &repo,
            AgentPin {
                model: "control:old-model".into(),
                effort: None,
            },
        )
        .await;
    let task = h
        .task_on(
            &goal,
            &repo,
            "task",
            AgentPin {
                model: "codex:old".into(),
                effort: None,
            },
        )
        .await;
    let goal = h.activate(&goal).await;
    h.advance(&task, ariadne_core::TaskStatus::InProgress).await;
    let orchestrator = h.launcher.spawn_orchestrator(&goal.id).await.unwrap();
    eventually(TIMEOUT, "the orchestrator to become idle", || async {
        h.session_status(&orchestrator).await == SessionStatus::Idle
    })
    .await;
    let agent = started(&h, &task).await;
    eventually(TIMEOUT, "the failed agent to end", || async {
        h.session_status(&agent).await == SessionStatus::Exited
    })
    .await;
    let scheduler = scheduler::start(h.store.clone(), h.launcher.clone(), false, h.timeouts);
    World {
        h,
        agent,
        orchestrator,
        scheduler,
        control,
        target: healthy,
        _root: root,
    }
}

/// One write, not several: the scheduler is already running and can wake on
/// its own — an agent's own trailing events reach it independent of a test's
/// `wake` — so a rank set one row at a time would let that wake land between
/// two of the writes and switch on a table only partly ranked.
async fn rank(h: &Harness, entries: &[(&str, ModelRank)]) {
    let entries: Vec<_> = entries
        .iter()
        .map(|(model, rank)| (*model, Some(*rank)))
        .collect();
    h.store.set_model_ranks(&entries).await.unwrap();
}

async fn wake(world: &World) {
    world
        .scheduler
        .send(SchedEvent::SessionEvent(world.agent.id.clone()))
        .unwrap();
}

/// How many `session.error` events a session carries so far.
async fn count_errors(h: &Harness, session: &AgentSession) -> usize {
    h.store
        .list_session_events(&session.id)
        .await
        .unwrap()
        .into_iter()
        .filter(|event| event.kind == "session.error")
        .count()
}

/// The session the first switch off `old` landed on: read off `old`'s own
/// first `session.switched` event rather than `switched_successor`, which
/// names only the newest `switched_from` row on `old.id` — an in-place
/// switch keeps that id, so a model that exhausts again right away can
/// trigger a second, cascading switch under the very same id before a
/// caller ever gets to look, and `switched_successor` would then name that
/// second switch's target, not the first one this helper is asked for.
async fn successor(h: &Harness, old: &AgentSession) -> AgentSession {
    let event = eventually_some(TIMEOUT, "the exhausted session to switch", || async {
        h.store
            .list_session_events(&old.id)
            .await
            .unwrap()
            .into_iter()
            .find(|event| event.kind == "session.switched")
    })
    .await;
    let payload: Value = serde_json::from_str(&event.payload).unwrap();
    let to = payload["to"].as_str().unwrap();
    h.store.get_session(to).await.unwrap()
}

fn codex_error() -> Value {
    json!({
        "code": -32603,
        "message": "You've hit your usage limit. Try again at 7:00 PM",
        "data": {"message": "limit", "codexErrorInfo": "usageLimitExceeded"}
    })
}

#[tokio::test]
async fn a_codex_exhaustion_switches_to_another_agent_at_the_same_rank() {
    let world = world(codex_error(), true, &["other"]).await;
    rank(
        &world.h,
        &[
            ("codex:old", ModelRank::Balanced),
            ("other:old-model", ModelRank::Balanced),
        ],
    )
    .await;
    wake(&world).await;
    let next = successor(&world.h, &world.agent).await;
    assert_eq!(next.model, "other:old-model");
    assert_eq!(next.effort, None);

    let events = world
        .h
        .store
        .list_session_events(&world.agent.id)
        .await
        .unwrap();
    let error: Value = serde_json::from_str(
        &events
            .iter()
            .find(|event| event.kind == "session.error")
            .unwrap()
            .payload,
    )
    .unwrap();
    assert_eq!(error["error"]["code"], -32603);
    assert_eq!(
        error["error"]["data"]["codexErrorInfo"],
        "usageLimitExceeded"
    );
    assert_eq!(error["error"]["exhausted_reason"], "usageLimitExceeded");
}

#[tokio::test]
async fn claude_and_message_signals_classify_while_a_plain_error_does_not() {
    for error in [
        json!({"code": -32603, "message": "stopped", "data": {
            "_meta": {"jetbrains": {"air": {"sessionFailure": {"category": "limit"}}}}
        }}),
        json!({"code": -32603, "message": "RATE LIMIT reached", "data": {}}),
        json!({"code": -32603, "message": "You've hit your session limit", "data": {
            "errorKind": "rate_limit"
        }}),
        json!({"code": -32603, "message": "Internal error", "data": {
            "details": "API error: 429 This request would exceed your account's rate limit."
        }}),
    ] {
        let world = world(error, true, &["other"]).await;
        rank(
            &world.h,
            &[
                ("codex:old", ModelRank::Fast),
                ("other:old-model", ModelRank::Fast),
            ],
        )
        .await;
        wake(&world).await;
        assert_eq!(
            successor(&world.h, &world.agent).await.model,
            "other:old-model"
        );
    }

    let response = world_with_prompt(
        json!({"meta": {
            "jetbrains": {"air": {"sessionFailure": {"category": "limit"}}}
        }}),
        true,
        &["other"],
    )
    .await;
    rank(
        &response.h,
        &[
            ("codex:old", ModelRank::Fast),
            ("other:old-model", ModelRank::Fast),
        ],
    )
    .await;
    wake(&response).await;
    assert_eq!(
        successor(&response.h, &response.agent).await.model,
        "other:old-model"
    );

    let plain = world(
        json!({"code": -32603, "message": "authentication failed", "data": {"kept": true}}),
        true,
        &["other"],
    )
    .await;
    rank(
        &plain.h,
        &[
            ("codex:old", ModelRank::Fast),
            ("other:old-model", ModelRank::Fast),
        ],
    )
    .await;
    wake(&plain).await;
    scheduler::flush_for_test(&plain.scheduler).await;
    assert!(
        plain
            .h
            .store
            .switched_successor(&plain.agent.id)
            .await
            .unwrap()
            .is_none()
    );
    let events = plain
        .h
        .store
        .list_session_events(&plain.agent.id)
        .await
        .unwrap();
    let error: Value = serde_json::from_str(
        &events
            .iter()
            .find(|event| event.kind == "session.error")
            .unwrap()
            .payload,
    )
    .unwrap();
    assert_eq!(error["error"]["exhausted"], Value::Null);
}

#[tokio::test]
async fn the_ladder_uses_same_agent_then_the_rank_above_then_below() {
    for (ranks, expected) in [
        (
            vec![
                ("codex:old", ModelRank::Balanced),
                ("codex:same", ModelRank::Balanced),
                ("other:old-model", ModelRank::Balanced),
            ],
            "other:old-model",
        ),
        (
            vec![
                ("codex:old", ModelRank::Balanced),
                ("codex:same", ModelRank::Balanced),
                ("other:frontier", ModelRank::Frontier),
            ],
            "codex:same",
        ),
        (
            vec![
                ("codex:old", ModelRank::Balanced),
                ("codex:same", ModelRank::Balanced),
            ],
            "codex:same",
        ),
        (
            vec![
                ("codex:old", ModelRank::Balanced),
                ("other:frontier", ModelRank::Frontier),
                ("other:fast", ModelRank::Fast),
            ],
            "other:frontier",
        ),
        (
            vec![
                ("codex:old", ModelRank::Balanced),
                ("other:fast", ModelRank::Fast),
            ],
            "other:fast",
        ),
    ] {
        let world = world(codex_error(), true, &["other"]).await;
        rank(&world.h, &ranks).await;
        wake(&world).await;
        assert_eq!(successor(&world.h, &world.agent).await.model, expected);
    }
}

#[tokio::test]
async fn an_unranked_model_and_disabled_auto_switch_stay_exhausted() {
    for auto_switch in [true, false] {
        let world = world(codex_error(), auto_switch, &["other"]).await;
        if !auto_switch {
            rank(
                &world.h,
                &[
                    ("codex:old", ModelRank::Fast),
                    ("other:old-model", ModelRank::Fast),
                ],
            )
            .await;
        }
        wake(&world).await;
        scheduler::flush_for_test(&world.scheduler).await;
        assert!(
            world
                .h
                .store
                .switched_successor(&world.agent.id)
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(
            world.h.attention(&world.agent).await,
            Some(AttentionReason::Exhausted),
            "auto_switch={auto_switch} task={:?} session={:?}",
            world
                .h
                .status(world.agent.task_id.as_deref().unwrap())
                .await,
            world.h.session_status(&world.agent).await
        );
    }
}

#[tokio::test]
async fn a_chain_never_revisits_a_model_and_stops_at_the_budget() {
    let world = world(codex_error(), true, &["a", "b", "c", "d"]).await;
    world.target.reprogram(agent_script(
        codex_error(),
        &["old-model", "same", "frontier", "fast"],
    ));
    rank(
        &world.h,
        &[
            ("codex:old", ModelRank::Balanced),
            ("a:old-model", ModelRank::Balanced),
            ("b:old-model", ModelRank::Balanced),
            ("c:old-model", ModelRank::Balanced),
            ("d:old-model", ModelRank::Balanced),
        ],
    )
    .await;
    wake(&world).await;
    eventually(
        TIMEOUT,
        "the automatic switch budget to be spent",
        || async {
            world
                .h
                .store
                .list_sessions(SessionFilter {
                    task_id: world.agent.task_id.clone(),
                    ..Default::default()
                })
                .await
                .unwrap()
                .iter()
                .filter(|session| session.seat() == Some(Seat::Agent))
                .count()
                == SPAWN_RETRY_BUDGET as usize + 1
        },
    )
    .await;
    let sessions = world
        .h
        .store
        .list_sessions(SessionFilter {
            task_id: world.agent.task_id.clone(),
            ..Default::default()
        })
        .await
        .unwrap();
    let agents: Vec<_> = sessions
        .iter()
        .filter(|session| session.seat() == Some(Seat::Agent))
        .collect();
    let models: std::collections::HashSet<_> =
        agents.iter().map(|session| &session.model).collect();
    assert_eq!(models.len(), agents.len());
    let last = agents
        .iter()
        .find(|session| {
            !agents
                .iter()
                .any(|candidate| candidate.switched_from.as_deref() == Some(session.id.as_str()))
        })
        .unwrap();
    eventually(TIMEOUT, "the last exhausted flag", || async {
        world.h.attention(last).await == Some(AttentionReason::Exhausted)
    })
    .await;
    for earlier in agents.iter().filter(|session| session.id != last.id) {
        assert_eq!(world.h.attention(earlier).await, None);
    }
    let notices = world
        .control
        .prompts_for(&world.orchestrator.id)
        .into_iter()
        .filter(|prompt| prompt.contains("switched from model"))
        .count();
    assert_eq!(notices, SPAWN_RETRY_BUDGET as usize);
}

#[tokio::test]
async fn an_in_place_switch_does_not_return_to_the_exhausted_model() {
    let world = world(codex_error(), true, &[]).await;
    // Before the first `wake`: the switch, the resume on the new pin and its
    // own exhaustion can all run inside that one wake's reconcile, with
    // nothing here scheduled in between to catch them apart. Counted from
    // here, the one `session.error` of the first exhaustion (raised while
    // `world` was still building) is on hand to compare against regardless
    // of how much of the rest that reconcile gets through before anything
    // below runs again.
    let errors_so_far = count_errors(&world.h, &world.agent).await;
    rank(
        &world.h,
        &[
            ("codex:old", ModelRank::Balanced),
            ("codex:same", ModelRank::Balanced),
        ],
    )
    .await;
    wake(&world).await;
    assert_eq!(successor(&world.h, &world.agent).await.model, "codex:same");
    // The retried prompt fails the same way on the model that took over
    // (`prompt_script`'s second turn), so the session is exhausted again
    // through the same classification as the first failure, with nothing
    // here having to fake that state.
    //
    // Waited for as a second `session.error`, not as the attention flag
    // turning `Exhausted`: the switch clears the first exhaustion and the
    // scheduler can resume and re-exhaust the session before this ever
    // polls, both inside the reconcile the switch itself ran in, so the
    // flag can read `Exhausted` the whole time without the clear ever
    // being caught in between — and `SessionStatus::Exited` fares no
    // better, true of the *old* exhaustion until that resume runs. A
    // second error row is instead something no poll can catch mid-flight:
    // once written, it stays.
    eventually(TIMEOUT, "the switched model to exhaust in turn", || async {
        count_errors(&world.h, &world.agent).await > errors_so_far
    })
    .await;
    wake(&world).await;
    scheduler::flush_for_test(&world.scheduler).await;
    let session = world.h.store.get_session(&world.agent.id).await.unwrap();
    assert_eq!(session.model, "codex:same");
    assert_eq!(session.attention_reason(), Some(AttentionReason::Exhausted));
    let switches = world
        .h
        .store
        .list_session_events(&world.agent.id)
        .await
        .unwrap()
        .into_iter()
        .filter(|event| event.kind == "session.switched")
        .count();
    assert_eq!(switches, 1);
}
