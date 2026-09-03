//! Resuming an agent keeps its session row.
//!
//! A task bounced back by its reviewers is the same engineer, in the same
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
use ariadne_core::{AgentKind, GoalStatus, PromptKind, Role, SessionStatus, TaskStatus};
use ariadne_daemon::agents::prompts;
use ariadne_store::{AgentSession, Task};

use common::{Cast, Harness, harness, next_event, sh};

/// A task with an engineer session that has already run once: a worktree on
/// disk and a tmux that is no longer alive.
///
/// Its repository is not a git repo, so a fresh spawn cannot get off the
/// ground here — which is what the fallback tests lean on.
async fn engineer_session(h: &Harness) -> (Cast, AgentSession) {
    let cast = h.cast().await;
    let session = h
        .session(
            &cast.goal,
            Some(&cast.task),
            Role::Engineer,
            &cast.engineer.id,
        )
        .await;
    h.store
        .set_task_worktree(&cast.task.id, session.worktree_path.as_deref())
        .await
        .unwrap();
    let task = h.store.get_task(&cast.task.id).await.unwrap();
    (Cast { task, ..cast }, session)
}

/// A task under review for real: a repo on disk with a commit on the task
/// branch, its profiles carrying `model` at the moment it was created — so
/// that is what the task and the reviewer slot are pinned to.
async fn under_review(h: &Harness, model: Option<&str>) -> Cast {
    let repo_path = h.git_repo("repo");
    let cast = h
        .cast_pinned(Some(AgentKind::ClaudeCode), model, 1)
        .await;
    sh(&repo_path, &format!("git branch {}", cast.task.branch));
    h.advance(&cast.task, TaskStatus::UnderReview).await;
    let task = h.store.get_task(&cast.task.id).await.unwrap();
    Cast { task, ..cast }
}

/// The reviewer bounces the task back and the engineer pushes another commit:
/// the task returns to review one round on, one commit ahead.
async fn next_round(h: &Harness, task: &Task) -> Task {
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
        (TaskStatus::UnderReview, ariadne_core::Actor::Engineer),
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

fn argv_of(h: &Harness, session_id: &str) -> String {
    h.spawn_plan(session_id)
        .expect("a spawn plan")
        .argv
        .join(" ")
}

/// Which agent and model a session runs on comes off the pin its role
/// carries — the reviewer slot here — and a profile edited afterwards does not
/// reach it, on any launch path: not the resume that carries a reviewer into
/// round two, and not the fresh session a round with nothing to resume gets.
#[tokio::test]
async fn a_reviewers_pin_outlives_a_profile_edit() {
    let h = harness().await;
    let cast = under_review(&h, Some("opus")).await;
    let (task, reviewer) = (cast.task.clone(), cast.reviewer.id.clone());

    // Nothing to resume yet, so this is the reviewer's first spawn.
    let first = h
        .launcher
        .resume_reviewer(&task.id, &reviewer, "(unused: no session yet)")
        .await
        .unwrap();
    assert_eq!(first.model.as_deref(), Some("opus"));
    assert!(
        argv_of(&h, &first.id)
            .contains("--model opus"),
        "the launch asked for the pinned model"
    );

    // The profile moves to another agent and another model while the session
    // is alive. The row is not rewritten behind it.
    h.move_profile(
        &reviewer, Some(AgentKind::Codex), Some("sonnet"))
        .await;
    assert_eq!(
        h.store
            .get_session(&first.id)
            .await
            .unwrap()
            .model
            .as_deref(),
        Some("opus"),
        "a profile edit rewrote a running session's model"
    );

    // Round two relaunches the same session, on the same agent and model it
    // was pinned to — the profile now says codex/sonnet.
    h.launcher.kill_session(&first.id).await.unwrap();
    let task = next_round(&h, &task).await;
    let second = h
        .launcher
        .resume_reviewer(&task.id, &reviewer, "Round 2: have another look.")
        .await
        .unwrap();
    assert_eq!(second.id, first.id, "round 2 reused the session");
    assert_eq!(second.agent_kind(), AgentKind::ClaudeCode);
    assert_eq!(second.model.as_deref(), Some("opus"));
    let argv = argv_of(&h, &second.id);
    assert!(
        argv.contains("--model opus"),
        "and that is what the agent was launched with: {argv}"
    );

    // A round that finds nothing to resume spawns afresh, and lands on the
    // pin just the same.
    h.launcher.kill_session(&second.id).await.unwrap();
    let third = h
        .launcher
        .spawn_reviewer(&task.id, &reviewer)
        .await
        .unwrap();
    assert_ne!(third.id, second.id, "a fresh session, not the old one");
    assert_eq!(third.agent_kind(), AgentKind::ClaudeCode);
    assert_eq!(third.model.as_deref(), Some("opus"));
    assert!(
        argv_of(&h, &third.id)
            .contains("--model opus"),
        "a fresh session read the profile instead of the pin"
    );
}

/// The same for the engineer, whose pin is the task's: the spawn that starts
/// the work and every resume that carries it through review run on the model
/// the task was created with.
#[tokio::test]
async fn an_engineers_pin_outlives_a_profile_edit() {
    let h = harness().await;
    let cast = under_review(&h, Some("opus")).await;
    let task = cast.task.clone();
    h.move_profile(
        &task.engineer_profile_id,
        Some(AgentKind::Codex),
        Some("sonnet"),
    )
    .await;

    let first = h.launcher.spawn_engineer(&task.id).await.unwrap();
    assert_eq!(first.agent_kind(), AgentKind::ClaudeCode);
    assert_eq!(first.model.as_deref(), Some("opus"));

    h.launcher.kill_session(&first.id).await.unwrap();
    let resumed = h
        .launcher
        .resume_engineer(&task.id, "Round 1: please fix things.")
        .await
        .unwrap();
    assert_eq!(resumed.id, first.id, "the resume reused the session");
    assert_eq!(resumed.model.as_deref(), Some("opus"));
    let argv = argv_of(&h, &resumed.id);
    assert!(
        argv.contains("--model opus"),
        "the resume re-read the profile: {argv}"
    );
}

/// And for the planner, whose pin is the goal's: a respawn after the profile
/// moved still plans on the agent and model the goal was created with.
#[tokio::test]
async fn a_planner_respawn_stays_on_the_goals_pin() {
    let h = harness().await;
    let planner = h
        .profile_on("planner", Role::Planner, Some(AgentKind::ClaudeCode), Some("opus"))
        .await;
    let (goal, _repo) = h.goal(&planner).await;
    let (goal, planner) = (goal.id, planner.id);

    let first = h.launcher.spawn_planner(&goal).await.unwrap();
    assert_eq!(first.model.as_deref(), Some("opus"));

    h.move_profile(
        &planner, Some(AgentKind::Codex), Some("sonnet"))
        .await;
    h.launcher.kill_session(&first.id).await.unwrap();

    let second = h.launcher.spawn_planner(&goal).await.unwrap();
    assert_ne!(second.id, first.id, "a planner respawn is a fresh session");
    assert_eq!(second.agent_kind(), AgentKind::ClaudeCode);
    assert_eq!(second.model.as_deref(), Some("opus"));
    assert!(
        argv_of(&h, &second.id)
            .contains("--model opus"),
        "the respawn read the profile instead of the goal's pin"
    );
}

/// A pin of "no model" is a pin too: the work runs on the agent CLI's own
/// default however the profile is edited afterwards.
#[tokio::test]
async fn a_pin_of_no_model_stays_the_agents_own_default() {
    let h = harness().await;
    let cast = under_review(&h, None).await;
    let (task, reviewer) = (cast.task.clone(), cast.reviewer.id.clone());
    h.move_profile(&reviewer, Some(AgentKind::ClaudeCode), Some("sonnet"))
        .await;

    let session = h
        .launcher
        .spawn_reviewer(&task.id, &reviewer)
        .await
        .unwrap();
    assert_eq!(session.model, None);
    assert!(
        !argv_of(&h, &session.id).contains("--model"),
        "no model was asked for"
    );
}

/// The changes-requested bounce, twice over: the task panel's Sessions tab
/// must still list one engineer, live again, on the same conversation.
#[tokio::test]
async fn resuming_the_engineer_reuses_its_session_across_review_rounds() {
    let h = harness().await;
    let (cast, first) = h.resumable_engineer().await;
    let task = cast.task.clone();

    for round in 1..=2 {
        let resumed = h
            .launcher
            .resume_engineer(&task.id, &format!("Round {round}: please fix things."))
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
            "round {round} left more than one engineer session: {sessions:?}"
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
    let (cast, first) = h.resumable_engineer().await;
    let task = cast.task.clone();
    let briefing = "B".repeat(100_000);

    let session = h
        .launcher
        .resume_engineer(&task.id, &briefing)
        .await
        .unwrap();
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

/// A reviewer that sees a task through two rounds is one reviewer with one
/// memory of it: round two wakes the session it already has — same row, same
/// tmux name, same conversation — in a worktree moved to the new tip, and is
/// told which round it is now judging.
#[tokio::test]
async fn a_reviewer_reuses_its_session_across_review_rounds() {
    let h = harness().await;
    let cast = under_review(&h, None).await;
    let (task, reviewer) = (cast.task.clone(), cast.reviewer.id.clone());

    // Round one: nothing to resume, so this is the reviewer's first spawn.
    let first = h
        .launcher
        .resume_reviewer(&task.id, &reviewer, "(unused: no session yet)")
        .await
        .unwrap();
    assert_eq!(first.role(), Role::Reviewer);
    assert_eq!(first.review_round, Some(1));
    assert!(
        !first.tmux_session.ends_with("-r1"),
        "the round is no part of the session's name: {}",
        first.tmux_session
    );
    let internal = first
        .internal_session_id
        .clone()
        .expect("claude picks its session uuid at spawn");

    // The task leaves review, so the daemon tears the reviewer's tmux down;
    // then the engineer revises and it comes back for round two.
    h.launcher.kill_session(&first.id).await.unwrap();
    let task = next_round(&h, &task).await;
    assert_eq!(task.review_round, 2);

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
    assert_eq!(second.id, first.id, "round 2 reused the session");
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
    assert_eq!(
        second.review_round,
        Some(2),
        "and its row says which round it is on"
    );
    let sessions: Vec<AgentSession> = h
        .sessions_of(&task.id)
        .await
        .into_iter()
        .filter(|s| s.role() == Role::Reviewer)
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
        "round 2 resumed the stored conversation: {argv}"
    );
    assert!(
        argv.contains("Round 2 of"),
        "and was told which round it is reviewing: {argv}"
    );
    // One console log, appended to across both rounds.
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
    let cast = under_review(&h, None).await;
    let (task, reviewer) = (cast.task.clone(), cast.reviewer.id.clone());
    let stillborn = h
        .session(&cast.goal, Some(&task), Role::Reviewer, &reviewer)
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
    let (cast, first) = h.resumable_engineer().await;
    let task = cast.task.clone();
    let mut rx = h.bus.subscribe();

    h.launcher
        .resume_engineer(&task.id, "fix things")
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
    let cast = under_review(&h, Some("opus")).await;
    let task = cast.task.clone();
    let session = h.launcher.spawn_engineer(&task.id).await.unwrap();
    h.launcher.kill_session(&session.id).await.unwrap();

    h.move_profile(
        &task.engineer_profile_id,
        Some(AgentKind::Codex),
        Some("sonnet"),
    )
    .await;

    let revived = h.launcher.revive_session(&session.id, None).await.unwrap();
    assert_eq!(revived.id, session.id, "the same session, revived");
    assert_eq!(revived.status(), SessionStatus::Running);
    assert_eq!(revived.ended_at, None);
    assert_eq!(revived.worktree_path, session.worktree_path);
    assert_eq!(h.sessions_of(&task.id).await.len(), 1);
    assert_eq!(revived.agent_kind(), AgentKind::ClaudeCode);
    assert_eq!(revived.model.as_deref(), Some("opus"));
    let argv = argv_of(&h, &revived.id);
    assert!(
        argv.contains("--model opus"),
        "the revive re-read the profile: {argv}"
    );
}

/// Nothing to resume from: an engineer session that never reported an agent id
/// is not a conversation, so it is left alone and a fresh spawn is what runs
/// (which fails here for want of a git repo — the point is the path taken).
#[tokio::test]
async fn a_session_without_an_agent_id_is_not_revived() {
    let h = harness().await;
    let (cast, first) = engineer_session(&h).await;
    let task = cast.task.clone();
    h.set_status(&first, SessionStatus::Exited).await;

    assert!(
        h.launcher
            .resume_engineer(&task.id, "carry on")
            .await
            .is_err(),
        "there is no repo to spawn a fresh engineer in"
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
        let (_cast, session) = h.resumable_engineer().await;
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
