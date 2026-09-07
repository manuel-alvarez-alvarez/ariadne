//! Resuming an agent keeps its session row.
//!
//! A task bounced back by its reviewers is the same author, in the same
//! conversation, in the same worktree — so it stays one session however many
//! rounds it takes, rather than growing a sibling row per round. The same
//! holds for each reviewer: one session for the whole review.
//!
//! No tmux and no agent CLI needed: `tmux` is the stub script that records the
//! commands the launcher issues, which is also how the console-log wiring is
//! checked without a pane to pipe. What the agent itself was launched with is
//! read from the session's spawn plan, since that is where it travels — tmux
//! is handed `ariadne _spawn <plan>`. `git` is real — a reviewer's worktree
//! has to actually move to the branch tip between rounds.

mod common;

use std::path::PathBuf;

use ariadne_api::stream::DomainEvent;
use ariadne_core::{AgentKind, GoalStatus, PromptKind, Seat, SessionStatus, TaskStatus};
use ariadne_daemon::agents::prompts;
use ariadne_store::{AgentSession, Task};

use common::{Cast, Harness, harness, next_event, sh};

/// A task with an author session that has already run once: a worktree on
/// disk and a tmux that is no longer alive.
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
    let cast = h.cast_pinned(AgentKind::ClaudeCode, model, 1).await;
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

/// The last `new-session` the launcher issued, as the stub recorded it.
fn last_new_session(h: &Harness) -> String {
    h.tmux_calls_of("new-session")
        .pop()
        .expect("the launcher started a tmux session")
}

/// The `pipe-pane` calls, one per launch.
fn pipes(h: &Harness) -> Vec<String> {
    h.tmux_calls_of("pipe-pane")
}

/// The environment the launcher wrote into a session's spawn plan.
fn env_of(h: &Harness, session_id: &str) -> Vec<(String, String)> {
    h.spawn_plan(session_id).expect("a spawn plan").env
}

fn argv_of(h: &Harness, session_id: &str) -> String {
    h.spawn_plan(session_id)
        .expect("a spawn plan")
        .argv
        .join(" ")
}

/// Which agent and model a session runs on comes off the pin its seat
/// carries — the reviewer slot here — and a profile edited afterwards does not
/// reach it, on any launch path: not the resume that carries a reviewer into
/// round two, and not the fresh session a round with nothing to resume gets.
#[tokio::test]
async fn a_running_reviewer_keeps_the_model_its_session_started_on() {
    let h = harness().await;
    let cast = under_review(&h, "opus").await;
    let (task, reviewer) = (cast.task.clone(), cast.reviewer.id.clone());

    // Nothing to resume yet, so this is the reviewer's first spawn.
    let first = h
        .launcher
        .resume_reviewer(&task.id, &reviewer, "(unused: no session yet)")
        .await
        .unwrap();
    assert_eq!(first.model, "opus");
    assert!(
        argv_of(&h, &first.id).contains("--model opus"),
        "the launch asked for the pinned model"
    );

    // The agent is re-pinned to another CLI and another model while the
    // session is alive. The row is not rewritten behind it.
    h.move_agent(&reviewer, AgentKind::Codex, "sonnet").await;
    assert_eq!(
        h.store.get_session(&first.id).await.unwrap().model,
        "opus",
        "a re-pin rewrote a running session's model"
    );

    // Round two relaunches the same session, on the same agent and model it
    // was launched with — the agent itself now says codex/sonnet.
    h.launcher.kill_session(&first.id).await.unwrap();
    let task = reviewed_again(&h, &task).await;
    let second = h
        .launcher
        .resume_reviewer(&task.id, &reviewer, "Have another look.")
        .await
        .unwrap();
    assert_eq!(second.id, first.id, "the second review reused the session");
    assert_eq!(second.agent_kind(), AgentKind::ClaudeCode);
    assert_eq!(second.model, "opus");
    let argv = argv_of(&h, &second.id);
    assert!(
        argv.contains("--model opus"),
        "and that is what the agent was launched with: {argv}"
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
        third.agent_kind(),
        AgentKind::Codex,
        "a fresh session reads the agent's pin as it stands"
    );
    assert_eq!(third.model, "sonnet");
}

/// The same for the author, whose pin is the task's: the spawn that starts
/// the work and every resume that carries it through review run on the model
/// the task was created with.
#[tokio::test]
async fn a_resumed_author_stays_on_the_model_its_session_started_on() {
    let h = harness().await;
    let cast = under_review(&h, "opus").await;
    let task = cast.task.clone();

    let first = h.launcher.spawn_author(&task.id).await.unwrap();
    assert_eq!(first.agent_kind(), AgentKind::ClaudeCode);
    assert_eq!(first.model, "opus");

    // Re-pinned under a session that is already running: what it was launched
    // with is what every relaunch of it carries.
    h.move_agent(&cast.author.id, AgentKind::Codex, "sonnet")
        .await;
    h.launcher.kill_session(&first.id).await.unwrap();
    let resumed = h
        .launcher
        .resume_author(&task.id, "Round 1: please fix things.")
        .await
        .unwrap();
    assert_eq!(resumed.id, first.id, "the resume reused the session");
    assert_eq!(resumed.model, "opus");
    let argv = argv_of(&h, &resumed.id);
    assert!(
        argv.contains("--model opus"),
        "the resume re-read the agent's pin: {argv}"
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
                agent_kind: AgentKind::ClaudeCode,
                model: "opus".into(),
                effort: None,
            },
        )
        .await;
    let goal = goal.id;

    let first = h.launcher.spawn_orchestrator(&goal).await.unwrap();
    assert_eq!(first.model, "opus");

    h.launcher.kill_session(&first.id).await.unwrap();

    let second = h.launcher.spawn_orchestrator(&goal).await.unwrap();
    assert_ne!(
        second.id, first.id,
        "an orchestrator respawn is a fresh session"
    );
    assert_eq!(second.agent_kind(), AgentKind::ClaudeCode);
    assert_eq!(second.model, "opus");
    assert!(
        argv_of(&h, &second.id).contains("--model opus"),
        "the respawn read the profile instead of the goal's pin"
    );
}

/// Every launch of a session reports under a name of its own.
///
/// The row is the same one on a resume — the same conversation, the same
/// worktree — so a report carrying its id says nothing about *which* agent
/// sent it. Between the kill and the pane that replaces it, two of them can:
/// the one being torn down has its exit hook still to fire, and on a resumed
/// conversation even its internal id is the same. The launch is what tells
/// them apart, so it is fresh per process, written to the row before tmux is
/// asked for a pane, and carried by the agent in its environment.
#[tokio::test]
async fn every_launch_of_a_session_reports_under_a_new_id() {
    let h = harness().await;
    let cast = under_review(&h, "opus").await;
    let task = cast.task.clone();

    let first = h.launcher.spawn_author(&task.id).await.unwrap();
    let launch = h.launch_id(&first).await.expect("the launch was named");
    assert!(
        env_of(&h, &first.id).contains(&("ARIADNE_LAUNCH_ID".to_string(), launch.clone())),
        "the agent carries the launch it runs under: {:?}",
        env_of(&h, &first.id)
    );

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
        env_of(&h, &resumed.id).contains(&("ARIADNE_LAUNCH_ID".to_string(), relaunch)),
        "the resumed agent carries the new one"
    );
}

/// A pane that outlived the row that owned it costs the next spawn nothing.
///
/// tmux names are derived from the goal, the task and the seat, so a seat has
/// exactly one — and `new-session` refuses a name that is taken *after* the
/// session row has been written. A pane left behind that way would mint a dead
/// row per attempt until the spawn budget ran out and the user was told an
/// agent that could have started would not. Nothing live claims that pane, so
/// the spawn takes its name.
#[tokio::test]
async fn a_pane_left_behind_is_taken_rather_than_spawned_around() {
    let h = harness().await;
    let repo = h.repository(&h.at("repo")).await;
    let goal = h
        .goal_on(
            &repo,
            ariadne_store::AgentPin {
                agent_kind: AgentKind::ClaudeCode,
                model: "opus".into(),
                effort: None,
            },
        )
        .await;

    let first = h.launcher.spawn_orchestrator(&goal.id).await.unwrap();
    // The agent is in its pane; the row under it is not — the database and
    // the machine disagreeing, which is the whole of the situation.
    h.pane_exists(&first);
    h.set_status(&first, SessionStatus::Exited).await;

    let second = h.launcher.spawn_orchestrator(&goal.id).await.unwrap();
    assert_eq!(
        h.killed_panes(),
        vec![first.tmux_session.clone()],
        "the leftover pane was taken"
    );
    assert_eq!(second.tmux_session, first.tmux_session);
    assert_eq!(h.session_status(&second).await, SessionStatus::Running);
    assert_eq!(
        h.sessions_of_goal(&goal.id).await.len(),
        2,
        "one row per orchestrator that was started, and no row for an attempt that was not"
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
            resumed.tmux_session, first.tmux_session,
            "and keeps its tmux name"
        );
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
        // one. The plan is where that is written now, one per launch.
        let argv = argv_of(&h, &resumed.id);
        assert!(argv.contains("--resume uuid-1234"), "round {round}: {argv}");
        assert!(
            argv.contains(&format!("Round {round}: please fix things.")),
            "round {round} carried its instruction: {argv}"
        );
    }

    // Console-log continuity: with the id reused, both runs pipe into the one
    // file, and deliberately append to it — the log stays the whole transcript
    // of the one session, in the order the terminal produced it.
    let expected = format!("cat >> '{}'", h.console_log(&first.id).display());
    let pipes = pipes(&h);
    assert_eq!(pipes.len(), 2, "one pipe-pane per launch: {pipes:?}");
    for pipe in pipes {
        assert!(
            pipe.contains(&expected),
            "a relaunch must append to the session's own console log: {pipe}"
        );
    }
}

/// The point of the spawn plan: what an agent is told has no bearing on the
/// size of the command tmux is given.
///
/// A briefing of a hundred kilobytes used to be unlaunchable — tmux hands its
/// server one message, capped near 16KB, so `new-session` answered "command
/// too long" until the spawn ran out of attempts and the task was failed for
/// it. Now tmux gets three words and a path, and the launch itself is in the
/// plan file: argv, environment, working directory, and permissions that keep
/// it to the daemon.
#[tokio::test]
async fn a_launch_hands_tmux_nothing_that_can_outgrow_it() {
    use std::os::unix::fs::PermissionsExt;

    let h = harness().await;
    let (cast, first) = h.resumable_author().await;
    let task = cast.task.clone();
    let briefing = "B".repeat(100_000);

    let session = h.launcher.resume_author(&task.id, &briefing).await.unwrap();
    let worktree = session.worktree_path.clone().unwrap();

    // What tmux was asked to run, in full: the plan file and nothing else.
    let plan_file = h.plan_file(&first.id);
    assert_eq!(
        last_new_session(&h),
        format!(
            "new-session -d -s {} -c {worktree} -- {} _spawn {}",
            session.tmux_session,
            h.launcher.cfg.cli_bin,
            plan_file.display()
        )
    );

    // And the plan is the launch, verbatim: the briefing the adapter built,
    // the environment that used to arrive as `-e` pairs, the working dir.
    let plan = h.spawn_plan(&first.id).expect("a spawn plan");
    assert_eq!(plan.argv[0], "claude");
    assert!(
        plan.argv.iter().any(|arg| arg.ends_with(&briefing)),
        "the briefing rode in the plan: {:?}",
        plan.argv.iter().map(String::len).collect::<Vec<_>>()
    );
    assert!(
        plan.env
            .contains(&("ARIADNE_SESSION_ID".to_string(), first.id.clone())),
        "the session env rode in the plan: {:?}",
        plan.env
    );
    assert_eq!(plan.cwd, PathBuf::from(&worktree));

    // The plan stays behind as the record of how the session was started, and
    // it holds the agent's whole environment: nobody else's to read.
    let mode = std::fs::metadata(&plan_file).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600, "plan mode: {mode:o}");
}

/// A reviewer asked to look at a task twice is one reviewer with one memory
/// of it: the second review wakes the session it already has — same row, same
/// tmux name, same conversation — in a worktree moved to the new tip.
#[tokio::test]
async fn a_reviewer_reuses_its_session_across_reviews() {
    let h = harness().await;
    let cast = under_review(&h, "opus").await;
    let (task, reviewer) = (cast.task.clone(), cast.reviewer.id.clone());

    // Nothing to resume, so this is the reviewer's first spawn.
    let first = h
        .launcher
        .resume_reviewer(&task.id, &reviewer, "(unused: no session yet)")
        .await
        .unwrap();
    assert_eq!(first.seat(), Seat::Reviewer);
    assert!(
        !first.tmux_session.ends_with("-r1"),
        "which review it is on is no part of the session's name: {}",
        first.tmux_session
    );
    let internal = first
        .internal_session_id
        .clone()
        .expect("claude picks its session uuid at spawn");

    // The task leaves review, so the daemon tears the reviewer's tmux down;
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
        second.tmux_session, first.tmux_session,
        "and keeps its tmux name"
    );
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

    let argv = argv_of(&h, &second.id);
    assert!(
        argv.contains(&format!("--resume {internal}")),
        "the second review resumed the stored conversation: {argv}"
    );
    assert!(
        argv.contains("needs your verdict"),
        "and was told what it is being woken for: {argv}"
    );
    // One console log, appended to across both reviews.
    let expected = format!("cat >> '{}'", h.console_log(&first.id).display());
    let pipes = pipes(&h);
    assert_eq!(pipes.len(), 2, "one pipe-pane per launch: {pipes:?}");
    for pipe in pipes {
        assert!(
            pipe.contains(&expected),
            "both rounds pipe into the one console log: {pipe}"
        );
    }
}

/// A reviewer session that never reported an agent id is no conversation to
/// go back to — codex and opencode only report theirs from a hook — so the
/// next round spawns a fresh one rather than failing.
#[tokio::test]
async fn a_reviewer_without_an_agent_id_is_spawned_afresh() {
    let h = harness().await;
    let cast = under_review(&h, "opus").await;
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
    assert!(spawned.internal_session_id.is_some());
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
    let cast = under_review(&h, "opus").await;
    let task = cast.task.clone();
    let session = h.launcher.spawn_author(&task.id).await.unwrap();
    h.launcher.kill_session(&session.id).await.unwrap();

    h.move_agent(&cast.author.id, AgentKind::Codex, "sonnet")
        .await;

    let revived = h.launcher.revive_session(&session.id, None).await.unwrap();
    assert_eq!(revived.id, session.id, "the same session, revived");
    assert_eq!(revived.status(), SessionStatus::Running);
    assert_eq!(revived.ended_at, None);
    assert_eq!(revived.worktree_path, session.worktree_path);
    assert_eq!(h.sessions_of(&task.id).await.len(), 1);
    assert_eq!(revived.agent_kind(), AgentKind::ClaudeCode);
    assert_eq!(revived.model, "opus");
    let argv = argv_of(&h, &revived.id);
    assert!(
        argv.contains("--model opus"),
        "the revive re-read the profile: {argv}"
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
