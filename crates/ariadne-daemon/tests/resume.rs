//! Resuming an agent keeps its session row.
//!
//! A task bounced back by its reviewers is the same author, in the same
//! conversation, in the same worktree — so it stays one session however many
//! rounds it takes, rather than growing a sibling row per round. The same
//! holds for each reviewer: one session for the whole review.
//!
//! The agents are the harness's stub, and what each was launched with is read
//! from the session's launch file. `git` is real — a reviewer's worktree has
//! to actually move to the branch tip between rounds.

mod common;

use std::path::PathBuf;

use ariadne_api::stream::DomainEvent;
use ariadne_core::{GoalStatus, PromptKind, Seat, SessionStatus, TaskStatus};
use ariadne_daemon::agents::prompts;
use ariadne_store::{AgentSession, Task};

use common::{Cast, Harness, TIMEOUT, eventually, harness, next_event, sh};

/// A task with an author session that has already run once: a worktree on
/// disk and no agent left running.
///
/// Its repository is not a git repo, so a fresh spawn cannot get off the
/// ground here — which is what the fallback tests lean on.
async fn author_session(h: &Harness) -> (Cast, AgentSession) {
    let cast = h.cast().await;
    let session = h
        .session(&cast.goal, Some(&cast.task), Seat::Author, &cast.author.id)
        .await;
    h.store
        .set_task_worktree(&cast.task.id, session.worktree_path.as_deref())
        .await
        .unwrap();
    let task = h.store.get_task(&cast.task.id).await.unwrap();
    (Cast { task, ..cast }, session)
}

/// A task under review for real: a repo on disk with a commit on the task
/// branch, its agents carrying `model` at the moment it was created — so
/// that is what the task and the reviewer slot are pinned to.
async fn under_review(h: &Harness, model: &str) -> Cast {
    let repo_path = h.git_repo("repo");
    let cast = h.cast_pinned(model, 1).await;
    sh(&repo_path, &format!("git branch {}", cast.task.branch));
    h.advance(&cast.task, TaskStatus::UnderReview).await;
    let task = h.store.get_task(&cast.task.id).await.unwrap();
    Cast { task, ..cast }
}

/// The reviewer bounces the task back and the author pushes another commit:
/// the task returns to review one commit ahead.
async fn reviewed_again(h: &Harness, task: &Task) -> Task {
    let repo_path = PathBuf::from(&h.store.get_repository(&task.repo_id).await.unwrap().path);
    sh(
        &repo_path,
        &format!(
            "git checkout -q {branch} && echo v2 > file.txt && git add . && \
             git -c user.email=t@t -c user.name=t commit -qm revision && \
             git checkout -q main",
            branch = task.branch
        ),
    );
    for (to, actor) in [
        (TaskStatus::ChangesRequested, ariadne_core::Actor::Daemon),
        (TaskStatus::InProgress, ariadne_core::Actor::Daemon),
        (TaskStatus::UnderReview, ariadne_core::Actor::Author),
    ] {
        h.store
            .transition_task(&task.id, to, actor, None, None)
            .await
            .unwrap();
    }
    h.store.get_task(&task.id).await.unwrap()
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
/// resume goes back to, which the agent names at its session start.
async fn heard(h: &Harness, session: &AgentSession) -> AgentSession {
    eventually(TIMEOUT, "the agent to report its session", || async {
        h.store
            .get_session(&session.id)
            .await
            .unwrap()
            .internal_session_id
            .is_some()
    })
    .await;
    h.store.get_session(&session.id).await.unwrap()
}

/// Which agent and model a session runs on comes off the pin its seat
/// carries — the reviewer slot here — and a profile edited afterwards does not
/// reach it, on any launch path: not the resume that carries a reviewer into
/// round two, and not the fresh session a round with nothing to resume gets.
#[tokio::test]
async fn a_running_reviewer_keeps_the_model_its_session_started_on() {
    let h = harness().await;
    let cast = under_review(&h, "stub:opus").await;
    let (task, reviewer) = (cast.task.clone(), cast.reviewer.id.clone());

    // Nothing to resume yet, so this is the reviewer's first spawn.
    let first = h
        .launcher
        .resume_reviewer(&task.id, &reviewer, "(unused: no session yet)")
        .await
        .unwrap();
    assert_eq!(first.model, "stub:opus");
    assert_eq!(
        launched_model(&h, &first.id),
        "opus",
        "the launch asked for the pinned model"
    );
    heard(&h, &first).await;

    // The agent is re-pinned to another model while the session is alive.
    // The row is not rewritten behind it.
    h.move_agent(&reviewer, "stub:sonnet").await;
    assert_eq!(
        h.store.get_session(&first.id).await.unwrap().model,
        "stub:opus",
        "a re-pin rewrote a running session's model"
    );

    // Round two relaunches the same session, on the same model it was
    // launched with — the agent itself now says sonnet.
    h.launcher.kill_session(&first.id).await.unwrap();
    let task = reviewed_again(&h, &task).await;
    let second = h
        .launcher
        .resume_reviewer(&task.id, &reviewer, "Have another look.")
        .await
        .unwrap();
    assert_eq!(second.id, first.id, "the second review reused the session");
    assert_eq!(second.model, "stub:opus");
    assert_eq!(
        launched_model(&h, &second.id),
        "opus",
        "and that is what the agent was launched with"
    );

    // A round that finds nothing to resume spawns afresh — and a fresh spawn
    // reads the agent as it stands, which is where the re-pin does land.
    h.launcher.kill_session(&second.id).await.unwrap();
    h.forget_session(&second).await;
    let third = h
        .launcher
        .spawn_reviewer(&task.id, &reviewer)
        .await
        .unwrap();
    assert_ne!(third.id, second.id, "a fresh session, not the old one");
    assert_eq!(
        third.model, "stub:sonnet",
        "a fresh session reads the agent's pin as it stands"
    );
}

/// The same for the author, whose pin is the task's: the spawn that starts
/// the work and every resume that carries it through review run on the model
/// the task was created with.
#[tokio::test]
async fn a_resumed_author_stays_on_the_model_its_session_started_on() {
    let h = harness().await;
    let cast = under_review(&h, "stub:opus").await;
    let task = cast.task.clone();

    let first = h.launcher.spawn_author(&task.id).await.unwrap();
    assert_eq!(first.model, "stub:opus");
    heard(&h, &first).await;

    // Re-pinned under a session that is already running: what it was launched
    // with is what every relaunch of it carries.
    h.move_agent(&cast.author.id, "stub:sonnet").await;
    h.launcher.kill_session(&first.id).await.unwrap();
    let resumed = h
        .launcher
        .resume_author(&task.id, "Round 1: please fix things.")
        .await
        .unwrap();
    assert_eq!(resumed.id, first.id, "the resume reused the session");
    assert_eq!(resumed.model, "stub:opus");
    assert_eq!(
        launched_model(&h, &resumed.id),
        "opus",
        "the resume did not re-read the agent's pin"
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
/// The row is the same one on a resume — the same conversation, the same
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
    let cast = under_review(&h, "stub:opus").await;
    let task = cast.task.clone();

    let first = h.launcher.spawn_author(&task.id).await.unwrap();
    let launch = h.launch_id(&first).await.expect("the launch was named");
    assert!(
        mcp_env_of(&h, &first.id).contains(&("ARIADNE_LAUNCH_ID".to_string(), launch.clone())),
        "the agent carries the launch it runs under: {:?}",
        mcp_env_of(&h, &first.id)
    );
    heard(&h, &first).await;

    h.launcher.kill_session(&first.id).await.unwrap();
    let resumed = h
        .launcher
        .resume_author(&task.id, "Round 1: please fix things.")
        .await
        .unwrap();
    assert_eq!(resumed.id, first.id, "the resume reused the session");

    let relaunch = h.launch_id(&resumed).await.expect("the launch was named");
    assert_ne!(relaunch, launch, "a launch of its own");
    assert!(
        mcp_env_of(&h, &resumed.id).contains(&("ARIADNE_LAUNCH_ID".to_string(), relaunch)),
        "the resumed agent carries the new one"
    );
}

/// The changes-requested bounce, twice over: the task panel's Sessions tab
/// must still list one author, live again, on the same conversation.
#[tokio::test]
async fn resuming_the_author_reuses_its_session_across_reviews() {
    let h = harness().await;
    let (cast, first) = h.resumable_author().await;
    let task = cast.task.clone();

    for round in 1..=2 {
        let resumed = h
            .launcher
            .resume_author(&task.id, &format!("Round {round}: please fix things."))
            .await
            .unwrap();
        assert_eq!(resumed.id, first.id, "round {round} reused the session");
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
            "round {round} left more than one author session: {sessions:?}"
        );
        // Each relaunch resumed the stored conversation rather than starting
        // one, and carried its instruction as the next prompt.
        let launch = h.launch_file(&resumed.id).expect("a launch file");
        assert_eq!(
            launch.resume_session_id.as_deref(),
            Some("uuid-1234"),
            "round {round}"
        );
        assert_eq!(
            launch.initial_prompt.as_deref(),
            Some(format!("Round {round}: please fix things.").as_str()),
            "round {round} carried its instruction"
        );
    }
}

/// What an agent is told has no size limit on its way there: a briefing of a
/// hundred kilobytes reaches the agent whole, as the prompt of the turn it
/// is resumed into.
#[tokio::test]
async fn a_briefing_of_any_size_reaches_the_agent_whole() {
    let h = harness().await;
    let (cast, first) = h.resumable_author().await;
    let briefing = "B".repeat(100_000);

    h.launcher
        .resume_author(&cast.task.id, &briefing)
        .await
        .unwrap();
    eventually(TIMEOUT, "the briefing to reach the agent", || async {
        h.prompts_to(&first)
            .iter()
            .any(|prompt| prompt.ends_with(&briefing))
    })
    .await;
}

/// A reviewer asked to look at a task twice is one reviewer with one memory
/// of it: the second review wakes the session it already has — same row, same
/// conversation — in a worktree moved to the new tip.
#[tokio::test]
async fn a_reviewer_reuses_its_session_across_reviews() {
    let h = harness().await;
    let cast = under_review(&h, "stub:opus").await;
    let (task, reviewer) = (cast.task.clone(), cast.reviewer.id.clone());

    // Nothing to resume, so this is the reviewer's first spawn.
    let first = h
        .launcher
        .resume_reviewer(&task.id, &reviewer, "(unused: no session yet)")
        .await
        .unwrap();
    assert_eq!(first.seat(), Seat::Reviewer);
    let internal = heard(&h, &first)
        .await
        .internal_session_id
        .expect("the agent reported its session");

    // The task leaves review, so the daemon takes the reviewer's agent down;
    // then the author revises and asks for a review again.
    h.launcher.kill_session(&first.id).await.unwrap();
    let task = reviewed_again(&h, &task).await;

    // The briefing is the built-in resume template, rendered — the same path
    // the scheduler takes.
    let template = prompts::template_for(PromptKind::ReviewerResume);
    let second = h
        .launcher
        .resume_reviewer(
            &task.id,
            &reviewer,
            &prompts::reviewer_resume_briefing(template, &task, Some("I rewrote the thing.")),
        )
        .await
        .unwrap();
    assert_eq!(second.id, first.id, "the second review reused the session");
    assert_eq!(
        second.internal_session_id.as_deref(),
        Some(internal.as_str()),
        "on the same agent conversation"
    );
    assert_eq!(second.status(), SessionStatus::Running);
    assert_eq!(second.ended_at, None, "the session is live again");
    let sessions: Vec<AgentSession> = h
        .sessions_of(&task.id)
        .await
        .into_iter()
        .filter(|s| s.seat() == Seat::Reviewer)
        .collect();
    assert_eq!(
        sessions.len(),
        1,
        "two rounds left more than one reviewer session: {sessions:?}"
    );

    // The worktree it wakes up in is the branch as it stands now.
    let worktree = PathBuf::from(second.worktree_path.as_deref().unwrap());
    assert_eq!(
        std::fs::read_to_string(worktree.join("file.txt")).unwrap(),
        "v2\n",
        "the reviewer woke up in the tree it already reviewed"
    );

    let launch = h.launch_file(&second.id).expect("a launch file");
    assert_eq!(
        launch.resume_session_id.as_deref(),
        Some(internal.as_str()),
        "the second review resumed the stored conversation"
    );
    assert!(
        launch
            .initial_prompt
            .as_deref()
            .is_some_and(|prompt| prompt.contains("needs your verdict")),
        "and was told what it is being woken for: {:?}",
        launch.initial_prompt
    );
}

/// A reviewer session that never reported an agent id is no conversation to
/// go back to — an agent reports its own at its session start — so the next
/// round spawns a fresh one rather than failing.
#[tokio::test]
async fn a_reviewer_without_an_agent_id_is_spawned_afresh() {
    let h = harness().await;
    let cast = under_review(&h, "stub:opus").await;
    let (task, reviewer) = (cast.task.clone(), cast.reviewer.id.clone());
    let stillborn = h
        .session(&cast.goal, Some(&task), Seat::Reviewer, &reviewer)
        .await;
    h.set_status(&stillborn, SessionStatus::Exited).await;

    let spawned = h
        .launcher
        .resume_reviewer(&task.id, &reviewer, "(unused: nothing to resume)")
        .await
        .unwrap();
    assert_ne!(spawned.id, stillborn.id, "a fresh session, not that one");
    assert_eq!(spawned.status(), SessionStatus::Running);
    heard(&h, &spawned).await;
    assert_eq!(
        h.session_status(&stillborn).await,
        SessionStatus::Exited,
        "an un-resumable session stays finished"
    );
}

/// The UI's caches are driven by domain events, and a reused row only ever
/// gets updates — so the relaunch has to announce itself as one.
#[tokio::test]
async fn a_relaunch_announces_the_session_as_updated() {
    let h = harness().await;
    let (cast, first) = h.resumable_author().await;
    let task = cast.task.clone();
    let mut rx = h.bus.subscribe();

    h.launcher
        .resume_author(&task.id, "fix things")
        .await
        .unwrap();

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
/// down to the agent and the model, so a profile edited in the meantime does
/// not get to move the conversation somewhere else either.
#[tokio::test]
async fn reviving_a_session_revives_it_in_place() {
    let h = harness().await;
    let cast = under_review(&h, "stub:opus").await;
    let task = cast.task.clone();
    let session = h.launcher.spawn_author(&task.id).await.unwrap();
    heard(&h, &session).await;
    h.launcher.kill_session(&session.id).await.unwrap();

    h.move_agent(&cast.author.id, "stub:sonnet").await;

    let revived = h.launcher.revive_session(&session.id, None).await.unwrap();
    assert_eq!(revived.id, session.id, "the same session, revived");
    assert_eq!(revived.status(), SessionStatus::Running);
    assert_eq!(revived.ended_at, None);
    assert_eq!(revived.worktree_path, session.worktree_path);
    assert_eq!(h.sessions_of(&task.id).await.len(), 1);
    assert_eq!(revived.model, "stub:opus");
    assert_eq!(
        launched_model(&h, &revived.id),
        "opus",
        "the revive re-read the agent's pin"
    );
}

/// Nothing to resume from: an author session that never reported an agent id
/// is not a conversation, so it is left alone and a fresh spawn is what runs
/// (which fails here for want of a git repo — the point is the path taken).
#[tokio::test]
async fn a_session_without_an_agent_id_is_not_revived() {
    let h = harness().await;
    let (cast, first) = author_session(&h).await;
    let task = cast.task.clone();
    h.set_status(&first, SessionStatus::Exited).await;

    assert!(
        h.launcher
            .resume_author(&task.id, "carry on")
            .await
            .is_err(),
        "there is no repo to spawn a fresh author in"
    );
    let after = h.store.get_session(&first.id).await.unwrap();
    assert_eq!(
        after.status(),
        SessionStatus::Exited,
        "an un-resumable session stays finished"
    );
    assert_eq!(h.sessions_of(&task.id).await.len(), 1);
}

/// A finished goal has nothing left for an agent to come back to, and the
/// scheduler kills what is live under one — so a revive here would put a
/// session up for the next tick to take straight down. Refused instead, and
/// the session stays as it ended.
#[tokio::test]
async fn a_session_of_a_finished_goal_is_not_revived() {
    for finished in [GoalStatus::Completed, GoalStatus::Cancelled] {
        let h = harness().await;
        let (_cast, session) = h.resumable_author().await;
        h.store
            .set_goal_status(&session.goal_id, finished)
            .await
            .unwrap();

        let error = h
            .launcher
            .revive_session(&session.id, None)
            .await
            .expect_err("a finished goal revives nothing")
            .to_string();
        assert!(
            error.contains(finished.as_str()),
            "the refusal says what the goal is: {error}"
        );
        let after = h.store.get_session(&session.id).await.unwrap();
        assert_eq!(after.status(), SessionStatus::Exited);
    }
}
