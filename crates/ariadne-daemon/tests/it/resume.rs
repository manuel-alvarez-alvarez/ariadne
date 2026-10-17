//! Resuming an agent keeps its session row.
//!
//! A task that comes back to a column is the same agent, in the same
//! conversation, in the same worktree — so a column's agent stays one session
//! however many times its column is entered, rather than growing a sibling
//! row per entry. Every column of a task works in the one worktree the task
//! has (030).
//!
//! The agents are the harness's stub, and what each was launched with is read
//! from the session's launch file. `git` is real — a column's first launch
//! cuts the task's worktree from the repository.

use crate::common;

use ariadne_api::stream::DomainEvent;
use ariadne_core::{Actor, GoalStatus, SessionStatus, TaskStatus};
use ariadne_store::AgentSession;
use axum::http::StatusCode;

use common::{Cast, Harness, TIMEOUT, eventually, harness, heard_from, next_event};

/// A task in its first column for real: a repo on disk, the goal active and
/// the task in progress, its agents carrying `model` at the moment it was
/// created — so that is what every column's agent is pinned to.
async fn in_progress(h: &Harness, model: &str) -> Cast {
    h.git_repo("repo");
    let cast = h.cast_pinned(model).await;
    let goal = h.activate(&cast.goal).await;
    h.advance(&cast.task, TaskStatus::InProgress).await;
    let task = h.store.get_task(&cast.task.id).await.unwrap();
    Cast { goal, task, ..cast }
}

/// The same, from the fixture whose first column's agent has already run
/// once ([`Harness::resumable_agent`]): no repo is needed, since the task's
/// worktree is already on disk.
async fn resumable_in_progress(h: &Harness) -> (Cast, AgentSession) {
    let (cast, session) = h.resumable_agent().await;
    let goal = h.activate(&cast.goal).await;
    h.advance(&cast.task, TaskStatus::InProgress).await;
    let task = h.store.get_task(&cast.task.id).await.unwrap();
    (Cast { goal, task, ..cast }, session)
}

/// Start the agent of the task's first column, the way the scheduler does
/// when the column has no live agent.
async fn start_develop(h: &Harness, cast: &Cast) -> AgentSession {
    let task = h.store.get_task(&cast.task.id).await.unwrap();
    // The staffing as it stands now, not as the cast was built: a re-pin
    // between two starts is what some of these tests are about.
    let develop = h.store.get_task_agent(&cast.develop().id).await.unwrap();
    h.launcher.start_step_agent(&task, &develop).await.unwrap()
}

/// The environment a session's last launch gave its MCP server.
fn mcp_env_of(h: &Harness, session_id: &str) -> Vec<(String, String)> {
    h.launch_file(session_id)
        .expect("a launch file")
        .mcp_servers[0]
        .env
        .iter()
        .map(|variable| (variable.name.clone(), variable.value.clone()))
        .collect()
}

/// The model a session's last launch pinned its agent to: the model half of
/// the pin, as the launch file carries it.
fn launched_model(h: &Harness, session_id: &str) -> String {
    h.launch_file(session_id).expect("a launch file").model
}

/// The session once its agent has reported the conversation it runs — what a
/// resume goes back to, which the agent names at its session start — and has
/// been heard from since its launch, so a kill after this is not a launch
/// that died on arrival.
async fn heard(h: &Harness, session: &AgentSession) -> AgentSession {
    eventually(TIMEOUT, "the agent to report its session", || async {
        let row = h.store.get_session(&session.id).await.unwrap();
        row.internal_session_id.is_some() && heard_from(&row)
    })
    .await;
    h.store.get_session(&session.id).await.unwrap()
}

/// Which agent and model a session runs on comes off the pin its seat
/// carries — the column's agent here — and a session freezes that pin at
/// its first launch: a staffing edited afterwards does not reach a running
/// session, nor a relaunch of it on the same pin. An edit that moves the
/// pin reaches the next entry of the column instead: the agent gets a
/// session of its own on the new pin, and the old session stays as history.
#[tokio::test]
async fn a_relaunched_agent_stays_on_the_model_its_session_started_on() {
    let h = harness().await;
    let cast = in_progress(&h, "stub:opus").await;
    let develop = cast.develop().id.clone();

    let first = start_develop(&h, &cast).await;
    assert_eq!(first.model, "stub:opus");
    assert_eq!(
        launched_model(&h, &first.id),
        "opus",
        "the launch asked for the pinned model"
    );
    heard(&h, &first).await;
    // Its entry briefing, which a live reconcile sends right behind the
    // launch: what makes its conversation one a relaunch may resume.
    h.reports(&first, "user_prompt_submit").await;

    // The column is entered again on the same pin: the same session, on the
    // conversation it was having.
    h.launcher.kill_session(&first.id).await.unwrap();
    let second = start_develop(&h, &cast).await;
    assert_eq!(second.id, first.id, "the relaunch reused the session");
    assert_eq!(second.model, "stub:opus");
    heard(&h, &second).await;

    // The agent is re-pinned to another model while the session is alive.
    // The row is not rewritten behind it.
    h.move_agent(&develop, "stub:sonnet").await;
    assert_eq!(
        h.store.get_session(&second.id).await.unwrap().model,
        "stub:opus",
        "a re-pin rewrote a running session's model"
    );

    // The next entry of the column is on the new pin: a session of its own,
    // launched on sonnet, beside the one that ran on opus.
    h.launcher.kill_session(&second.id).await.unwrap();
    let third = start_develop(&h, &cast).await;
    assert_ne!(third.id, second.id, "a new pin gets a session of its own");
    assert_eq!(third.model, "stub:sonnet");
    assert_eq!(launched_model(&h, &third.id), "sonnet");
    let kept = h.store.get_session(&second.id).await.unwrap();
    assert_eq!(
        kept.model, "stub:opus",
        "the old session is history, as it was"
    );
    assert!(kept.internal_session_id.is_some());
    assert_eq!(h.sessions_of(&cast.task.id).await.len(), 2);
}

/// The retry of a failed task after a staffing edit runs the column on the
/// edited pin: the orchestrator fixes a model or an effort with `update_task`
/// before it retries, and the retry starts a session on that pin rather than
/// resuming the conversation the old pin was having. That conversation stays
/// on its own row, with the id the agent gave it.
#[tokio::test]
async fn a_retry_after_a_staffing_edit_starts_a_new_session_on_the_new_pin() {
    let h = harness().await;
    let cast = in_progress(&h, "stub:opus").await;
    let first = start_develop(&h, &cast).await;
    let first = heard(&h, &first).await;
    let conversation = first
        .internal_session_id
        .clone()
        .expect("a conversation id");
    assert_eq!(launched_model(&h, &first.id), "opus");
    assert_eq!(h.launch_file(&first.id).unwrap().effort, None);

    // The task fails, the staffing is edited, and the task is retried.
    h.launcher.kill_session(&first.id).await.unwrap();
    h.store
        .transition_task(
            &cast.task.id,
            TaskStatus::Failed,
            Actor::Agent,
            Some("the input is missing"),
            None,
        )
        .await
        .unwrap();
    let _: serde_json::Value = h
        .json(
            crate::common::patch_json(
                &format!("/v1/tasks/{}", cast.task.id),
                serde_json::json!({"agents": [
                    {"step": "develop", "model": "stub:sonnet", "effort": "high"},
                    {"step": "review", "model": "stub:opus"},
                    {"step": "merge", "model": "stub:opus"},
                ]}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(
        h.store
            .get_task_agent(&cast.develop().id)
            .await
            .unwrap()
            .model,
        "stub:sonnet",
        "the edit kept the agent's row and moved its pin"
    );
    let _: serde_json::Value = h
        .json(
            crate::common::post_json(
                &format!("/v1/tasks/{}/retry", cast.task.id),
                serde_json::json!({}),
            ),
            StatusCode::OK,
        )
        .await;
    h.advance(&cast.task, TaskStatus::InProgress).await;

    let retried = start_develop(&h, &cast).await;
    assert_ne!(retried.id, first.id, "the retry runs a session of its own");
    assert_eq!(retried.model, "stub:sonnet");
    assert_eq!(retried.effort.as_deref(), Some("high"));
    let launch = h.launch_file(&retried.id).expect("a launch file");
    assert_eq!(launch.model, "sonnet", "launched on the edited model");
    assert_eq!(
        launch.effort.as_deref(),
        Some("high"),
        "at the edited effort"
    );
    assert_eq!(
        launch.resume_session_id, None,
        "a fresh conversation, not the old pin's"
    );

    // The history stays: the old row, its conversation id and its pin.
    let kept = h.store.get_session(&first.id).await.unwrap();
    assert_eq!(kept.model, "stub:opus");
    assert_eq!(
        kept.internal_session_id.as_deref(),
        Some(conversation.as_str())
    );
    assert_eq!(
        h.sessions_of(&cast.task.id)
            .await
            .iter()
            .map(|s| s.id.as_str())
            .collect::<Vec<_>>(),
        [first.id.as_str(), retried.id.as_str()]
    );
}

/// And for the orchestrator, whose pin is the goal's: a respawn plans on the
/// agent and model the goal was created with, whatever happened in between.
#[tokio::test]
async fn an_orchestrator_respawn_stays_on_the_goals_pin() {
    let h = harness().await;
    let repo = h.repository(&h.at("repo")).await;
    let goal = h
        .goal_on(
            &repo,
            ariadne_store::AgentPin {
                model: "stub:opus".into(),
                effort: None,
            },
        )
        .await;
    let goal = goal.id;

    let first = h.launcher.spawn_orchestrator(&goal).await.unwrap();
    assert_eq!(first.model, "stub:opus");

    h.launcher.kill_session(&first.id).await.unwrap();

    let second = h.launcher.spawn_orchestrator(&goal).await.unwrap();
    assert_ne!(
        second.id, first.id,
        "an orchestrator respawn is a fresh session"
    );
    assert_eq!(second.model, "stub:opus");
    assert_eq!(
        launched_model(&h, &second.id),
        "opus",
        "the respawn read something other than the goal's pin"
    );
}

/// Every launch of a session reports under a name of its own.
///
/// The row is the same one on a relaunch — the same conversation, the same
/// worktree — so a report carrying its id says nothing about *which* agent
/// sent it. Between the kill and the agent that replaces it, two of them can:
/// the one being torn down still has its exit to report, and on a resumed
/// conversation even its internal id is the same. The launch is what tells
/// them apart, so it is fresh per process, written to the row before the
/// agent is started, and carried by the agent's MCP server in its
/// environment.
#[tokio::test]
async fn every_launch_of_a_session_reports_under_a_new_id() {
    let h = harness().await;
    let cast = in_progress(&h, "stub:opus").await;

    let first = start_develop(&h, &cast).await;
    let launch = h.launch_id(&first).await.expect("the launch was named");
    assert!(
        mcp_env_of(&h, &first.id).contains(&("ARIADNE_LAUNCH_ID".to_string(), launch.clone())),
        "the agent carries the launch it runs under: {:?}",
        mcp_env_of(&h, &first.id)
    );
    heard(&h, &first).await;
    // Its entry briefing, which a live reconcile sends right behind the
    // launch: what makes its conversation one a relaunch may resume.
    h.reports(&first, "user_prompt_submit").await;

    h.launcher.kill_session(&first.id).await.unwrap();
    let resumed = start_develop(&h, &cast).await;
    assert_eq!(resumed.id, first.id, "the relaunch reused the session");

    let relaunch = h.launch_id(&resumed).await.expect("the launch was named");
    assert_ne!(relaunch, launch, "a launch of its own");
    assert!(
        mcp_env_of(&h, &resumed.id).contains(&("ARIADNE_LAUNCH_ID".to_string(), relaunch)),
        "the resumed agent carries the new one"
    );
}

/// `Harness::agent_runs` waits for the launch it just started to settle on
/// `running`, not for any earlier publish that happens to carry a fresh
/// clock or a status the row already had. Two publishes come before that
/// settling one: `set_session_launch`'s own, off the row exactly as it
/// stood before this launch, and `session_start`'s own first publish
/// (`touch_session`, moving the clock alone, before that event's own
/// ingestion writes the status last) — which, for a session idle from an
/// earlier run of this same helper, still carries that same idle status. A
/// second run over the same session, idle from the first, only returns once
/// both are true at once: the clock past what the first run left there, and
/// the status this launch's own report decided.
#[tokio::test]
async fn agent_runs_on_a_reused_idle_session_waits_for_its_own_report() {
    let h = harness().await;
    let cast = h.active_cast().await;
    let session = h.agent_session(&cast, "develop").await;
    h.agent_runs(&session).await;
    h.set_status(&session, SessionStatus::Idle).await;
    let before = h
        .store
        .get_session(&session.id)
        .await
        .unwrap()
        .last_activity_at;

    h.agent_runs(&session).await;

    let row = h.store.get_session(&session.id).await.unwrap();
    assert_ne!(
        row.last_activity_at, before,
        "the second run's own report moved the clock; a wait satisfied by \
         the row's already-idle status would not have"
    );
    assert_eq!(
        row.status(),
        SessionStatus::Running,
        "the second run's own status write landed too; a wait satisfied by \
         its own earlier, clock-only publish (`touch_session`, before that \
         event's ingestion writes the status last) would still read idle here"
    );
}

/// The two writes `ingest_event` makes for one report — the clock alone
/// (`touch_session`), then the status last — are not one write, and
/// `agent_runs` must not settle on the first of them. Proven by driving
/// those same two store calls by hand, with a deliberate gap between them
/// this test controls rather than races, while a real `agent_runs` waits on
/// the very session they name: the fresh agent itself is held back from
/// reporting for real, so only this test's own two writes move anything.
#[tokio::test]
async fn agent_runs_stays_open_through_the_clock_alone_and_settles_on_the_status_write() {
    let h = std::sync::Arc::new(harness().await);
    let cast = h.active_cast().await;
    let session = h.agent_session(&cast, "develop").await;
    h.agent_runs(&session).await;
    h.set_status(&session, SessionStatus::Idle).await;
    let launched_before = h.launch_id(&session).await;

    // Slow enough that the fresh agent's own real session_start cannot land
    // before this test has driven its own two writes by hand.
    let mut slow = common::acp::script();
    slow["start_delay"] = serde_json::json!(30.0);
    h.agent.reprogram(slow);

    let waiting = tokio::spawn({
        let h = h.clone();
        let session = session.clone();
        async move { h.agent_runs(&session).await }
    });

    eventually(TIMEOUT, "the second launch to be named", async || {
        h.launch_id(&session).await != launched_before
    })
    .await;
    let launch_id = h.launch_id(&session).await.unwrap();

    // The clock alone, exactly as `touch_session` would report it.
    h.store.touch_session(&session.id).await.unwrap();
    assert!(
        !waiting.is_finished(),
        "the clock alone is not what the wait settles on"
    );
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert!(
        !waiting.is_finished(),
        "still nothing to settle the wait on without the status write"
    );

    // The status, exactly as that same report's ingestion writes it last.
    h.store
        .set_session_status_if_live(&session.id, SessionStatus::Running, Some(&launch_id))
        .await
        .unwrap();
    tokio::time::timeout(TIMEOUT, waiting)
        .await
        .expect("the wait settles once the status write lands")
        .unwrap();
}

/// The column entered again, twice over: the task panel's Sessions tab must
/// still list one agent for the column, live again, on the same conversation.
/// The launch itself carries no prompt — the scheduler hands the column's
/// briefing to the agent once it is up.
#[tokio::test]
async fn relaunching_a_columns_agent_reuses_its_session_across_entries() {
    let h = harness().await;
    let (cast, first) = resumable_in_progress(&h).await;
    let task = cast.task.clone();

    for entry in 1..=2 {
        let resumed = start_develop(&h, &cast).await;
        assert_eq!(resumed.id, first.id, "entry {entry} reused the session");
        assert_eq!(resumed.status(), SessionStatus::Running);
        assert_eq!(resumed.ended_at, None, "the session is live again");
        assert_eq!(
            resumed.internal_session_id.as_deref(),
            Some("uuid-1234"),
            "on the same agent conversation"
        );
        assert!(resumed.last_activity_at.is_some(), "and is stamped live");
        let sessions = h.sessions_of(&task.id).await;
        assert_eq!(
            sessions.len(),
            1,
            "entry {entry} left more than one session on the column: {sessions:?}"
        );
        // Each relaunch resumed the stored conversation rather than starting
        // one, and carried no prompt of its own.
        let launch = h.launch_file(&resumed.id).expect("a launch file");
        assert_eq!(
            launch.resume_session_id.as_deref(),
            Some("uuid-1234"),
            "entry {entry}"
        );
        assert_eq!(
            launch.initial_prompt, None,
            "entry {entry}: the briefing is the scheduler's to send"
        );
        heard(&h, &resumed).await;
        h.launcher.kill_session(&resumed.id).await.unwrap();
    }
}

/// A relaunch that died before its first prompt — a model pin refused while
/// the account was rate limited — named a conversation the agent never
/// saved. A resume of the column goes to the conversation that holds the
/// work, the older session, and not to that name, which the agent answers
/// with "Resource not found".
#[tokio::test]
async fn a_resume_passes_over_a_launch_that_never_had_a_prompt() {
    let h = harness().await;
    let (cast, worked) = resumable_in_progress(&h).await;
    let dead = h.agent_session(&cast, "develop").await;
    h.store
        .set_session_internal_id(&dead.id, "never-saved")
        .await
        .unwrap();
    h.set_status(&dead, SessionStatus::Exited).await;

    let resumed = start_develop(&h, &cast).await;
    assert_eq!(resumed.id, worked.id, "the session with the work came back");
    let launch = h.launch_file(&resumed.id).expect("a launch file");
    assert_eq!(launch.resume_session_id.as_deref(), Some("uuid-1234"));
}

/// What an agent is told has no size limit on its way there: a briefing of a
/// hundred kilobytes reaches the agent whole, as the prompt of the next turn
/// of the session it was resumed into.
#[tokio::test]
async fn a_briefing_of_any_size_reaches_the_agent_whole() {
    let h = harness().await;
    let (cast, first) = resumable_in_progress(&h).await;
    let briefing = "B".repeat(100_000);

    start_develop(&h, &cast).await;
    h.launcher
        .acp
        .send_prompt(&first.id, briefing.clone())
        .unwrap();
    eventually(TIMEOUT, "the briefing to reach the agent", || async {
        h.prompts_to(&first)
            .iter()
            .any(|prompt| prompt.ends_with(&briefing))
    })
    .await;
}

/// The scheduler puts the current column's agent back on its feet in the
/// session it already has: an agent that went down with the task still in
/// its column is relaunched — same row, same conversation — and nudged to
/// go on, rather than replaced by a sibling.
#[tokio::test]
async fn the_scheduler_relaunches_the_current_columns_agent_in_its_session() {
    let h = harness().scheduler().await;
    h.git_repo("repo");
    let cast = h.active_cast().await;
    let develop = h.step_session(&cast.task, "develop").await;
    let heard_once = heard(&h, &develop).await;
    let launched = heard_once.launched_at.clone();
    let prompts = h.prompts_to(&develop).len();

    h.launcher.kill_session(&develop.id).await.unwrap();
    h.notify(&cast.task.id);

    eventually(TIMEOUT, "the column's agent to be relaunched", || async {
        h.relaunched(&develop, &launched).await
    })
    .await;
    let sessions = h.sessions_of(&cast.task.id).await;
    assert_eq!(sessions.len(), 1, "a relaunch, not a sibling: {sessions:?}");
    assert_eq!(
        sessions[0].internal_session_id, heard_once.internal_session_id,
        "on the same agent conversation"
    );
    assert!(h.prompts_to(&develop).len() > prompts);
    assert!(
        h.prompted(&develop).contains("Continue task at Develop."),
        "the relaunched agent is told to go on: {}",
        h.prompted(&develop)
    );
}

/// A session that never reported an agent id is no conversation to go back
/// to — an agent reports its own at its session start — so the column's
/// next entry spawns a fresh one rather than failing, and a revive of it is
/// refused.
#[tokio::test]
async fn a_session_without_an_agent_id_is_not_resumed_and_a_fresh_one_is_spawned() {
    let h = harness().await;
    let cast = in_progress(&h, "stub:opus").await;
    let stillborn = h.agent_session(&cast, "develop").await;
    h.set_status(&stillborn, SessionStatus::Exited).await;

    assert!(
        h.launcher
            .revive_session(&stillborn.id, None)
            .await
            .is_err(),
        "there is no conversation to revive"
    );
    let spawned = start_develop(&h, &cast).await;
    assert_ne!(spawned.id, stillborn.id, "a fresh session, not that one");
    assert_eq!(spawned.status(), SessionStatus::Running);
    heard(&h, &spawned).await;
    assert_eq!(
        h.session_status(&stillborn).await,
        SessionStatus::Exited,
        "an un-resumable session stays finished"
    );
    assert_eq!(h.sessions_of(&cast.task.id).await.len(), 2);
}

/// The UI's caches are driven by domain events, and a reused row only ever
/// gets updates — so the relaunch has to announce itself as one.
#[tokio::test]
async fn a_relaunch_announces_the_session_as_updated() {
    let h = harness().await;
    let (cast, first) = resumable_in_progress(&h).await;
    let mut rx = h.bus.subscribe();

    start_develop(&h, &cast).await;

    let event = next_event(
        &mut rx,
        |e| matches!(&e.event, DomainEvent::SessionUpdated(s) if s.status.is_live()),
    )
    .await;
    let DomainEvent::SessionUpdated(session) = event.event else {
        unreachable!("filtered above")
    };
    assert_eq!(session.id, first.id);
    assert!(
        !rx.try_recv()
            .is_ok_and(|e| matches!(e.event, DomainEvent::SessionCreated(_))),
        "a relaunch creates nothing"
    );
}

/// Manual resume (the UI's button, `ariadne attach`): the caller gets the very
/// session it named back, live again, not a sibling to go and find — in place
/// down to the agent and the model, so a staffing edited in the meantime does
/// not get to move the conversation somewhere else either.
#[tokio::test]
async fn reviving_a_session_revives_it_in_place() {
    let h = harness().await;
    let cast = in_progress(&h, "stub:opus").await;
    let session = start_develop(&h, &cast).await;
    heard(&h, &session).await;
    h.launcher.kill_session(&session.id).await.unwrap();

    h.move_agent(&cast.develop().id, "stub:sonnet").await;

    let revived = h.launcher.revive_session(&session.id, None).await.unwrap();
    assert_eq!(revived.id, session.id, "the same session, revived");
    assert_eq!(revived.status(), SessionStatus::Running);
    assert_eq!(revived.ended_at, None);
    assert_eq!(revived.worktree_path, session.worktree_path);
    assert_eq!(h.sessions_of(&cast.task.id).await.len(), 1);
    assert_eq!(revived.model, "stub:opus");
    assert_eq!(
        launched_model(&h, &revived.id),
        "opus",
        "the revive re-read the agent's pin"
    );
}

#[tokio::test]
async fn a_session_of_a_completed_goal_revives_and_the_goal_stays_completed() {
    let h = harness().scheduler().await;
    let (cast, session) = h.resumable_agent().await;
    h.store
        .set_goal_status(&cast.goal.id, GoalStatus::Completed)
        .await
        .unwrap();
    h.notify_goal(&cast.goal.id);
    h.flush_scheduler().await;
    let revived = h.launcher.revive_session(&session.id, None).await.unwrap();
    h.notify_goal(&cast.goal.id);
    h.flush_scheduler().await;
    assert_eq!(revived.id, session.id);
    assert!(h.launcher.acp.is_running(&session.id));
    assert_eq!(
        h.store.get_goal(&cast.goal.id).await.unwrap().status(),
        GoalStatus::Completed
    );
}

#[tokio::test]
async fn a_session_with_a_deleted_worktree_revives_in_the_repository_checkout() {
    let h = harness().await;
    let (cast, session) = h.resumable_agent().await;
    let worktree = session.worktree_path.as_deref().unwrap();
    h.git_repo("repo");
    std::fs::remove_dir(worktree).unwrap();
    let revived = h.launcher.revive_session(&session.id, None).await.unwrap();
    eventually(TIMEOUT, "the replacement agent to resume", || async {
        !h.agent.calls_of("session/resume").is_empty()
    })
    .await;
    assert_eq!(revived.id, session.id);
    let call = h.agent.calls_of("session/resume").pop().unwrap();
    assert_eq!(call["cwd"], cast.repo.path);
    assert!(!std::path::Path::new(worktree).exists());
}
