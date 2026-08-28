//! What the scheduler notices about agents that stopped working.
//!
//! One clock — how long since the session was last heard from at all — and
//! one timeline on it: a nudge at five minutes, the user at fifteen, and at
//! forty-five the pane killed and the agent put back on its feet. Every role
//! is under it, since every role can go quiet: the planner of a goal still
//! being planned, the reviewers a round is waiting on, and the engineer —
//! which is the only one whose task carries a flag of its own next to the
//! session's. A pane that disappears while its work is still going says so
//! too, rather than ending quietly.
//!
//! What the nudge is, the pane decides: an idle agent is told to get on with
//! the work, a pane whose composer is still holding the instruction it was
//! launched with gets the Enter that submits it, and an agent in the middle
//! of a turn is left alone until the thresholds behind the nudge.
//!
//! None of it waits on the keystrokes themselves: typing into a pane settles
//! for a second or two, and a pass with three agents to nudge sends all three
//! at once rather than one after another. A message that reached an agent
//! counts for the same pass as the nudge would have — it says the same thing
//! and better — so nothing tells an agent to get on with what it was asked to
//! do a moment ago.
//!
//! The scheduler is started after the seeding rather than with the harness, so
//! that the pass a test asks for is the first one over the state it just
//! wrote. The clock is moved by backdating the database columns it is read
//! from (`last_activity_at` and `launched_at`), since the store only ever
//! stamps them "now" and a threshold is minutes away.

mod common;

use std::ops::Deref;
use std::time::Duration;

use tokio::sync::mpsc::UnboundedSender;

use ariadne_core::{
    Actor, AttentionReason, AuthorRole, GoalStatus, ReviewVerdict, Role, SessionStatus, TaskStatus,
};
use ariadne_daemon::scheduler::{self, SchedEvent};
use ariadne_store::{AgentSession, Goal, NewMessage, NewReview, Recipient, ReviewerSlot, Task};

use common::{Harness, eventually, harness};

/// The watchdog's timeline, as `scheduler.rs` has it: one nudge, then the
/// user, then the pane killed and the agent put back on its feet.
const NUDGE_SECS: i64 = 300;
const FLAG_SECS: i64 = 900;
const RELAUNCH_SECS: i64 = 2_700;
/// How long a test waits for a reconciliation to reach the store. Generous
/// because some of what is waited on is not the daemon thinking: a nudge no
/// composer will let go of spends several seconds of widening backoff before
/// anybody hears about it, and every test here runs beside the others.
const TIMEOUT: Duration = Duration::from_secs(30);


/// One daemon, one goal, and the agents a test puts under it.
///
/// Everything a test writes goes through the harness it derefs to; what this
/// adds is the goal and task the watchdog is watched over, and a scheduler
/// started only once the seeding is done.
struct World {
    h: Harness,
    goal: Goal,
    task: Task,
    engineer: String,
    reviewer: String,
}

impl Deref for World {
    type Target = Harness;
    fn deref(&self) -> &Harness {
        &self.h
    }
}

impl World {
    /// An active goal with one task on it.
    async fn active() -> World {
        World::build(harness().await, 1).await
    }

    /// The same on a goal that wants two approvals: a round one verdict does
    /// not close is where a reviewer sits with its work done.
    async fn needing(approvals: i64) -> World {
        World::build(harness().await, approvals).await
    }

    /// A daemon that cannot start anything: `cli_bin` names no executable, so
    /// every fresh session dies at the launch.
    ///
    /// What a vanished pane leaves behind is only itself visible while nothing
    /// has replaced it — a successful replacement is supposed to clear the
    /// flag — so the tests about what the sweep concluded run where no
    /// replacement can happen, and the one about the replacement runs where it
    /// can.
    async fn cannot_spawn() -> World {
        World::build(harness().cannot_spawn().await, 1).await
    }

    async fn build(h: Harness, approvals: i64) -> World {
        let cast = h.cast_needing(approvals).await;
        let goal = h.activate(&cast.goal).await;
        World {
            h,
            goal,
            task: cast.task,
            engineer: cast.engineer.id,
            reviewer: cast.reviewer.id,
        }
    }

    /// Another task on the same goal, with the same agents behind it.
    async fn extra_task(&self, title: &str) -> Task {
        let repo = self.store.list_goal_repositories(&self.goal.id).await.unwrap()[0].clone();
        self.store
            .create_task(ariadne_store::NewTask {
                goal_id: self.goal.id.clone(),
                repo_id: repo.id,
                title: title.into(),
                description: "do things".into(),
                engineer_profile_id: self.engineer.clone(),
                pin: None,
                reviewers: vec![ReviewerSlot::of(&self.reviewer)],
                depends_on: vec![],
            })
            .await
            .unwrap()
    }

    /// The engineer of a task walked to `status`, in a pane the stub answers
    /// for: the opening most of these tests share.
    async fn engineer_on(&self, task: &Task, status: TaskStatus) -> AgentSession {
        self.advance(task, status).await;
        let session = self
            .session(&self.goal, Some(task), Role::Engineer, &self.engineer)
            .await;
        self.pane_exists(&session);
        session
    }

    /// The scheduler, started over everything seeded so far. Its first tick is
    /// immediate, so a test that only needs the sweep need send nothing.
    fn scheduler(&self) -> Sched {
        Sched(scheduler::start(
            self.store.clone(),
            self.launcher.clone(),
            false,
        ))
    }
}

/// The scheduler's event channel, addressed the way the HTTP layer addresses
/// it.
struct Sched(UnboundedSender<SchedEvent>);

impl Sched {
    fn task(&self, task: &Task) {
        self.0
            .send(SchedEvent::TaskChanged(task.id.clone()))
            .unwrap();
    }

    fn goal(&self, goal: &Goal) {
        self.0
            .send(SchedEvent::GoalChanged(goal.id.clone()))
            .unwrap();
    }

    fn message(&self, message_id: &str) {
        self.0
            .send(SchedEvent::MessagePosted(message_id.to_string()))
            .unwrap();
    }
}

/// The engineer's resume template, as its profile has it: the words the daemon
/// is about to type into the pane, which is what a composer that never lets go
/// keeps showing.
const RESUME: &str = r#"Pick "task" up again: your worktree is on"#;

// -- the timeline -----------------------------------------------------------

/// A planner has no task to flag, so its own session is where a goal that
/// stopped being planned says so.
#[tokio::test]
async fn a_planner_idle_past_the_threshold_is_raised_on_its_session() {
    let h = harness().await;
    let (goal, planner) = h.planning_goal().await;
    let session = h.session(&goal, None, Role::Planner, &planner.id).await;
    h.pane_exists(&session);
    h.idle_for(&session, NUDGE_SECS + 60).await;

    // One pass per threshold: the nudge, and then the escalation behind it.
    let sched = Sched(scheduler::start(
        h.store.clone(),
        h.launcher.clone(),
        false,
    ));
    sched.goal(&goal);
    eventually(TIMEOUT, "the planner to be nudged", async || {
        h.keystrokes(&session) > 0
    })
    .await;
    h.idle_for(&session, FLAG_SECS + 60).await;
    sched.goal(&goal);
    eventually(TIMEOUT, "the planner to be raised", async || {
        h.attention(&session).await == Some(AttentionReason::Stalled)
    })
    .await;
}

/// A reviewer the round is still waiting on is watched the same way.
#[tokio::test]
async fn a_reviewer_idle_past_the_threshold_is_raised_on_its_session() {
    let w = World::active().await;
    w.advance(&w.task, TaskStatus::UnderReview).await;
    let session = w
        .session(&w.goal, Some(&w.task), Role::Reviewer, &w.reviewer)
        .await;
    w.pane_exists(&session);
    w.idle_for(&session, NUDGE_SECS + 60).await;

    let sched = w.scheduler();
    sched.task(&w.task);
    eventually(TIMEOUT, "the reviewer to be nudged", async || {
        w.keystrokes(&session) > 0
    })
    .await;
    w.idle_for(&session, FLAG_SECS + 60).await;
    sched.task(&w.task);
    eventually(TIMEOUT, "the reviewer to be raised", async || {
        w.attention(&session).await == Some(AttentionReason::Stalled)
    })
    .await;
}

/// The engineer keeps its task-level flag, and now says it on its session too.
#[tokio::test]
async fn an_engineer_stall_flags_the_task_and_its_session() {
    let w = World::active().await;
    let session = w.engineer_on(&w.task, TaskStatus::InProgress).await;
    w.idle_for(&session, NUDGE_SECS + 60).await;

    let sched = w.scheduler();
    sched.task(&w.task);
    eventually(TIMEOUT, "the engineer to be nudged", async || {
        w.keystrokes(&session) > 0
    })
    .await;
    w.idle_for(&session, FLAG_SECS + 60).await;
    sched.task(&w.task);
    eventually(TIMEOUT, "the task to be flagged", async || {
        w.store.get_task(&w.task.id).await.unwrap().is_stalled()
    })
    .await;
    assert_eq!(
        w.attention(&session).await,
        Some(AttentionReason::Stalled),
        "and the session carries the reason as well"
    );
}

/// One nudge per situation, however many passes see the same silence. The
/// agent has been told; what follows a nudge nobody acts on is the user, not
/// another copy of the same words.
#[tokio::test]
async fn an_idle_agent_is_nudged_once_for_the_situation_it_went_quiet_in() {
    let w = World::active().await;
    let session = w.engineer_on(&w.task, TaskStatus::InProgress).await;
    w.idle_for(&session, NUDGE_SECS + 60).await;

    let sched = w.scheduler();
    sched.task(&w.task);
    eventually(TIMEOUT, "the engineer to be nudged", async || {
        w.keystrokes(&session) > 0
    })
    .await;
    // The delivery settles a paste and an Enter before the scheduler hears
    // anything; nothing is counted until it has.
    tokio::time::sleep(Duration::from_secs(2)).await;
    let nudged = w.keystrokes(&session);

    // A second task's agent, quiet in the same way from now on: its nudge is
    // what says the passes the first one went through are over.
    let control_task = w.extra_task("control").await;
    let control = w.engineer_on(&control_task, TaskStatus::InProgress).await;
    w.idle_for(&control, NUDGE_SECS + 60).await;
    // And the first one is as quiet as ever, still in the situation it was
    // nudged for.
    w.idle_for(&session, NUDGE_SECS + 120).await;
    for _ in 0..2 {
        sched.task(&w.task);
        sched.task(&control_task);
        eventually(TIMEOUT, "the other agent to be nudged", async || {
            w.keystrokes(&control) > 0
        })
        .await;
    }

    assert_eq!(
        w.keystrokes(&session),
        nudged,
        "nothing more was typed at an agent that has already been nudged"
    );
    assert_eq!(
        w.attention(&session).await,
        None,
        "and the escalation behind the nudge is the next threshold's, not this pass's"
    );
}

/// A nudge that does not leave the composer is not a nudge. The pane keeps
/// showing it however many Enters follow, so the session is raised for the
/// user rather than counted as told — the flag says the agent is not moving,
/// which is exactly what a message it never received leaves behind, and the
/// task it is not moving on says so with it.
#[tokio::test]
async fn a_nudge_that_never_submits_raises_the_session() {
    let w = World::active().await;
    let session = w.engineer_on(&w.task, TaskStatus::InProgress).await;
    // Past the nudge threshold and nowhere near the flag one: the only route
    // to a raised session here is the delivery that could not be confirmed.
    w.idle_for(&session, NUDGE_SECS + 60).await;
    w.composer_keeps(RESUME);

    let sched = w.scheduler();
    sched.task(&w.task);

    eventually(TIMEOUT, "the session to be raised", async || {
        w.attention(&session).await == Some(AttentionReason::Stalled)
    })
    .await;
    assert!(
        w.keystrokes(&session) > 2,
        "the paste was followed by more than one Enter"
    );
    assert!(
        w.store.get_task(&w.task.id).await.unwrap().is_stalled(),
        "and the task says what its agent's flag says: a stall is recorded once"
    );
}

/// An agent waiting on a person is blocked, not stalled. Typing into it would
/// answer whatever it is waiting on — a permission prompt takes Enter for a
/// yes — so it is left alone, flag and all.
#[tokio::test]
async fn a_session_waiting_on_a_person_is_never_nudged() {
    let w = World::active().await;
    let session = w.engineer_on(&w.task, TaskStatus::InProgress).await;
    w.idle_for(&session, NUDGE_SECS + 60).await;
    w.raise(&session, AttentionReason::WaitingPermission).await;
    // A second task, idle in exactly the same way but blocked on nothing: its
    // nudge is what says the pass the blocked one went through is over.
    let control_task = w.extra_task("control").await;
    let control = w.engineer_on(&control_task, TaskStatus::InProgress).await;
    w.idle_for(&control, NUDGE_SECS + 60).await;

    let sched = w.scheduler();
    sched.task(&w.task);
    sched.task(&control_task);
    eventually(TIMEOUT, "the unblocked engineer to be nudged", async || {
        w.keystrokes(&control) > 0
    })
    .await;

    assert_eq!(
        w.keystrokes(&session),
        0,
        "no keystroke is sent into a pane that is asking the user something"
    );
    assert_eq!(
        w.attention(&session).await,
        Some(AttentionReason::WaitingPermission),
        "and the reason it is waiting is not overwritten with a stall"
    );
    assert!(
        !w.store.get_task(&w.task.id).await.unwrap().is_stalled(),
        "nor is the task escalated behind it"
    );
}

/// The notification Claude fires a minute after every turn is not a person
/// being waited for.
///
/// An engineer that ended its turn mid-task is waiting for the daemon's nudge,
/// and nothing tells the two apart: the hook is registered with no matcher, so
/// the same `idle_prompt` arrives whether the agent asked something or simply
/// stopped. Reading it as a wait on the user put the session behind the
/// watchdog's skip list, where it was never nudged, never raised and never
/// relaunched.
#[tokio::test]
async fn a_claude_agent_idle_at_its_prompt_is_nudged_like_any_other() {
    let w = World::active().await;
    let session = w.engineer_on(&w.task, TaskStatus::InProgress).await;
    w.ingest(
        &session,
        "notification",
        serde_json::json!({
            "session_id": "5f3b1c8e-1234-4a2b-9d0e-0123456789ab",
            "cwd": "/tmp/wt",
            "hook_event_name": "Notification",
            "message": "Claude is waiting for your input",
            "notification_type": "idle_prompt",
        }),
    )
    .await;
    assert_eq!(
        w.attention(&session).await,
        None,
        "an agent sitting at its prompt is asking nobody for anything"
    );

    // And the silence behind that notification is measured like any other.
    w.idle_for(&session, NUDGE_SECS + 60).await;
    let sched = w.scheduler();
    sched.task(&w.task);
    eventually(TIMEOUT, "the engineer to be nudged", async || {
        w.keystrokes(&session) > 0
    })
    .await;
}

/// A resume whose instruction never left the composer: the agent is running,
/// has reported nothing at all, and would sit there for ever. The pane says
/// which it is — the instruction is still drawn in the composer — and what
/// that wants is the Enter a human would press, not another copy of what is
/// already there. If that did not start it either, the user.
#[tokio::test]
async fn a_composer_still_holding_its_instruction_gets_the_enter_alone() {
    let w = World::active().await;
    let session = w.engineer_on(&w.task, TaskStatus::InProgress).await;
    w.launched_ago(&session, NUDGE_SECS + 60).await;
    // The instruction the launch put there, still drawn where it was pasted.
    w.composer_keeps(RESUME);

    let sched = w.scheduler();
    sched.task(&w.task);
    eventually(TIMEOUT, "the stuck composer to be submitted", async || {
        w.keystrokes(&session) > 0
    })
    .await;
    // Whatever else that pass had to say would have been said by now.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        w.keystrokes(&session),
        1,
        "one keystroke: the Enter, and not the whole instruction pasted over \
         the copy already in the composer"
    );

    // Still nothing reported a threshold later, and the keystroke was not the
    // answer: only a person can say why.
    w.launched_ago(&session, FLAG_SECS + 60).await;
    sched.task(&w.task);
    eventually(TIMEOUT, "the agent that never started to be raised", async || {
        w.attention(&session).await == Some(AttentionReason::Stalled)
    })
    .await;
    assert_eq!(
        w.keystrokes(&session),
        1,
        "and the Enter is not pressed again for the same launch"
    );
}

/// An agent in the middle of a turn is left alone at the first threshold: its
/// composer is empty, so there is nothing to submit, and typing into a turn is
/// how work gets interrupted. A turn that never ends is what the thresholds
/// behind the nudge are for.
#[tokio::test]
async fn an_agent_in_the_middle_of_a_turn_is_not_nudged() {
    let w = World::active().await;
    let session = w.engineer_on(&w.task, TaskStatus::InProgress).await;
    w.launched_ago(&session, NUDGE_SECS + 60).await;
    // Nothing written into the stub's composer at all: every look at the pane
    // comes back empty, which is what a TUI drawing a transcript looks like
    // from here.

    // A second task's engineer, idle in the same silence: its nudge is what
    // says the pass the working one went through is over.
    let control_task = w.extra_task("control").await;
    let control = w.engineer_on(&control_task, TaskStatus::InProgress).await;
    w.idle_for(&control, NUDGE_SECS + 60).await;

    let sched = w.scheduler();
    sched.task(&w.task);
    sched.task(&control_task);
    eventually(TIMEOUT, "the idle engineer to be nudged", async || {
        w.keystrokes(&control) > 0
    })
    .await;

    assert_eq!(
        w.keystrokes(&session),
        0,
        "nothing is typed into an agent that is working"
    );
    assert_eq!(
        w.attention(&session).await,
        None,
        "nor is it raised for the user this early"
    );
}

/// An agent that reported an error is already asking for the user by name.
/// OpenCode reports a failed turn as `session.error` and the ingest leaves the
/// session running with the error raised — which is not a composer anybody has
/// to submit, and not a reason the user is better off hearing as a stall.
#[tokio::test]
async fn an_agent_that_reported_an_error_is_left_alone() {
    let w = World::active().await;
    let errored = w.engineer_on(&w.task, TaskStatus::InProgress).await;
    w.launched_ago(&errored, RELAUNCH_SECS + 60).await;
    w.composer_keeps(RESUME);
    w.raise(&errored, AttentionReason::AgentError).await;
    // And an agent whose silence nothing explains, whose flag says the passes
    // are over.
    let control_task = w.extra_task("control").await;
    let control = w.engineer_on(&control_task, TaskStatus::InProgress).await;
    w.launched_ago(&control, FLAG_SECS + 60).await;

    let launched = w.launched_at(&errored).await;
    let sched = w.scheduler();
    sched.task(&w.task);
    sched.task(&control_task);
    eventually(TIMEOUT, "the silent agent to be raised", async || {
        w.attention(&control).await == Some(AttentionReason::Stalled)
    })
    .await;

    assert_eq!(
        w.keystrokes(&errored),
        0,
        "nothing is typed into an agent whose turn failed"
    );
    assert_eq!(
        w.attention(&errored).await,
        Some(AttentionReason::AgentError),
        "what it reported is not overwritten with a stall"
    );
    assert_eq!(
        w.launched_at(&errored).await,
        launched,
        "and its pane is not killed out from under the failure"
    );
}

// -- the sweep --------------------------------------------------------------

/// A pane that vanished while its work was still going is not a session that
/// finished: it is an agent the user has lost, and it says so until something
/// puts it back — whatever the agent happened to be asking when it went, since
/// what the user has to know is that the work lost its agent.
///
/// A planner, so that nothing but the sweep is under test: the goal's own
/// reconciliation cannot start a replacement here (the repository is not a git
/// repository) and would have nothing to say about attention if it could.
#[tokio::test]
async fn a_vanished_pane_with_work_still_active_is_flagged_disconnected() {
    let h = harness().cannot_spawn().await;
    let (goal, planner) = h.planning_goal().await;
    // Live in the database, gone as far as tmux is concerned: never added to
    // the stub's list of panes.
    let session = h.session(&goal, None, Role::Planner, &planner.id).await;
    // And a second one that was sitting on a dialog that died with it.
    let on_a_prompt = h
        .session_named(&goal, None, Role::Planner, &planner.id, "vanished-prompt")
        .await;
    h.raise(&on_a_prompt, AttentionReason::WaitingPermission).await;

    // The sweep runs on the tick, and the first tick is immediate.
    let sched = Sched(scheduler::start(
        h.store.clone(),
        h.launcher.clone(),
        false,
    ));
    for vanished in [&session, &on_a_prompt] {
        eventually(TIMEOUT, "the vanished session to be swept", async || {
            h.attention(vanished).await == Some(AttentionReason::Disconnected)
        })
        .await;
        assert_eq!(
            h.session_status(vanished).await,
            SessionStatus::Exited,
            "the session is retired as well as raised"
        );
    }

    // And it stays raised: a session that ended needing attention keeps the
    // reason until it is resumed or replaced.
    sched.goal(&goal);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        h.attention(&session).await,
        Some(AttentionReason::Disconnected),
        "the flag outlives the session's own status"
    );
}

/// The engineer of an active task with no live session is resumed blind, and
/// when even that cannot get off the ground the session it tried to bring back
/// is the thing the user has to look at.
#[tokio::test]
async fn an_engineer_that_cannot_be_resumed_is_flagged_disconnected() {
    let w = World::cannot_spawn().await;
    w.advance(&w.task, TaskStatus::InProgress).await;
    // Ended, with no agent conversation to resume and no git repository to
    // spawn a fresh one in: the resume attempt cannot succeed.
    let session = w
        .session(&w.goal, Some(&w.task), Role::Engineer, &w.engineer)
        .await;
    w.set_status(&session, SessionStatus::Exited).await;

    let sched = w.scheduler();
    sched.task(&w.task);
    eventually(TIMEOUT, "the failed resume to be raised", async || {
        w.attention(&session).await == Some(AttentionReason::Disconnected)
    })
    .await;
}

/// A pane going away when nobody is waiting on that agent is just a session
/// ending: the engineer of a task under review is waiting on its reviewers and
/// is woken by id when they answer, a reviewer that has voted is finished
/// however long the round runs on, and a cancelled task is owed nothing at
/// all. All three are retired, and none of them raised.
#[tokio::test]
async fn a_vanished_pane_nobody_is_waiting_on_is_not_raised() {
    // Two approvals wanted, one given: the round stays open around a reviewer
    // that has nothing left to do, so the status is not what makes it quiet.
    let w = World::needing(2).await;
    w.advance(&w.task, TaskStatus::UnderReview).await;
    // Entering review opens a round: the verdict belongs to that one.
    let under_review = w.store.get_task(&w.task.id).await.unwrap();

    let engineer = w
        .session(&w.goal, Some(&under_review), Role::Engineer, &w.engineer)
        .await;
    let voted = w
        .session(&w.goal, Some(&under_review), Role::Reviewer, &w.reviewer)
        .await;
    w.store
        .create_review(NewReview {
            task_id: under_review.id.clone(),
            round: under_review.review_round,
            reviewer_profile_id: w.reviewer.clone(),
            session_id: Some(voted.id.clone()),
            verdict: ReviewVerdict::Approve,
            body: None,
        })
        .await
        .unwrap();

    let cancelled_task = w.extra_task("cancelled").await;
    let cancelled = w
        .session(&w.goal, Some(&cancelled_task), Role::Engineer, &w.engineer)
        .await;
    w.store
        .transition_task(
            &cancelled_task.id,
            TaskStatus::Cancelled,
            Actor::User,
            None,
            None,
        )
        .await
        .unwrap();

    let _sched = w.scheduler();
    for gone in [&engineer, &voted, &cancelled] {
        eventually(TIMEOUT, "the vanished session to be retired", async || {
            w.session_status(gone).await == SessionStatus::Exited
        })
        .await;
    }
    // Whatever else that pass had to say about them would have been said now.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        w.store.get_task(&under_review.id).await.unwrap().status(),
        TaskStatus::UnderReview,
        "the round is still open, so the status is not what makes this quiet"
    );
    for gone in [&engineer, &voted, &cancelled] {
        assert_eq!(
            w.attention(gone).await,
            None,
            "nothing is waiting on this agent, so nobody has to be told"
        );
    }
}

/// A replacement is a recovery too: the session a fresh spawn supersedes stops
/// asking for the user, but only once the replacement is actually up.
#[tokio::test]
async fn a_superseded_session_drops_its_attention_when_the_replacement_starts() {
    let h = harness().await;
    let (goal, planner) = h.planning_goal().await;
    // The planner cwd has to exist for the spawn to get off the ground.
    std::fs::create_dir_all(h.dir.path().join("repo")).unwrap();
    let session = h.session(&goal, None, Role::Planner, &planner.id).await;
    h.set_status(&session, SessionStatus::Exited).await;
    h.raise(&session, AttentionReason::Disconnected).await;

    // Nothing live for the goal, so reconciliation starts a new planner.
    let sched = Sched(scheduler::start(
        h.store.clone(),
        h.launcher.clone(),
        false,
    ));
    sched.goal(&goal);
    eventually(TIMEOUT, "the replacement planner to be running", async || {
        h.store
            .list_sessions(ariadne_store::SessionFilter {
                goal_id: Some(goal.id.clone()),
                live_only: true,
                ..Default::default()
            })
            .await
            .unwrap()
            .iter()
            .any(|s| s.id != session.id)
    })
    .await;
    eventually(TIMEOUT, "the superseded session to be let go", async || {
        h.attention(&session).await.is_none()
    })
    .await;
}

/// Resuming an agent is the recovery: whatever it needed the user for goes
/// with the relaunch, so a session that came back drops off the attention
/// list.
#[tokio::test]
async fn resuming_a_session_clears_its_attention() {
    let w = World::active().await;
    let session = w
        .session(&w.goal, Some(&w.task), Role::Engineer, &w.engineer)
        .await;
    w.make_resumable(&w.task, &session).await;
    w.set_status(&session, SessionStatus::Exited).await;
    w.raise(&session, AttentionReason::Disconnected).await;

    let resumed = w
        .launcher
        .resume_engineer(&w.task.id, "Continue where you left off.")
        .await
        .unwrap();

    assert_eq!(resumed.id, session.id, "the same session, put back on air");
    assert_eq!(
        resumed.attention_reason(),
        None,
        "an agent that is running again needs nobody"
    );
    assert_eq!(resumed.attention_since, None);
}

/// A flag raised by an agent event is only ever taken down by another one, and
/// a session sitting on a dialog reports nothing: the sweep is what lets go of
/// an engineer that was blocked on a permission prompt when its task moved on
/// to its reviewers — and only then. An agent the work is still waiting on
/// keeps its flag, down to the moment it went up, since how long it has been
/// stuck is the half of it the user acts on.
#[tokio::test]
async fn the_sweep_lets_go_of_a_blocked_agent_only_once_its_work_moved_on() {
    let w = World::cannot_spawn().await;
    let session = w.engineer_on(&w.task, TaskStatus::InProgress).await;
    w.raise(&session, AttentionReason::WaitingPermission).await;
    let raised_at = w
        .store
        .get_session(&session.id)
        .await
        .unwrap()
        .attention_since;

    // A second engineer, blocked in exactly the same way but on a task that
    // has gone to its reviewers: whatever the prompt was about, it got past it
    // and sent the task for review, and nothing more will ever be reported on
    // that session.
    let handed_over = w.extra_task("under review").await;
    let control = w.engineer_on(&handed_over, TaskStatus::UnderReview).await;
    w.raise(&control, AttentionReason::WaitingPermission).await;

    // The sweep runs on the tick, and the first tick is immediate.
    let _sched = w.scheduler();
    eventually(TIMEOUT, "the finished engineer to be let go", async || {
        w.attention(&control).await.is_none()
    })
    .await;

    let kept = w.store.get_session(&session.id).await.unwrap();
    assert_eq!(
        kept.attention_reason(),
        Some(AttentionReason::WaitingPermission),
        "the work is still this agent's, so what it is waiting on stands"
    );
    assert_eq!(
        kept.attention_since, raised_at,
        "and how long it has been waiting is not reset under it"
    );
}

/// A prompt is a dialog on the agent's pane: nobody can answer one on a
/// session that has ended, so retiring a session takes what it was waiting on
/// with it. Every role, and every one of them with its work still owed —
/// which is exactly when nothing else would take the flag down.
#[tokio::test]
async fn a_prompt_flag_does_not_outlive_the_session_it_was_raised_on() {
    /// Flag a session, retire it, and say what it is left carrying.
    async fn retire_on(
        h: &Harness,
        session: &AgentSession,
        reason: AttentionReason,
    ) -> Option<AttentionReason> {
        h.raise(session, reason).await;
        h.set_status(session, SessionStatus::Exited).await;
        let ended = h.store.get_session(&session.id).await.unwrap();
        assert_eq!(ended.attention_since, None, "and the clock under it");
        ended.attention_reason()
    }

    // One goal, walked from planning to active, so every role is retired in
    // the state its own work is still going in.
    let h = harness().await;
    let cast = h.cast().await;
    let planner_session = h
        .session(&cast.goal, None, Role::Planner, &cast.planner.id)
        .await;
    assert_eq!(
        retire_on(&h, &planner_session, AttentionReason::WaitingInput).await,
        None,
        "the goal is still being planned, and the planner is still waiting on nobody"
    );

    let goal = h.activate(&cast.goal).await;
    h.advance(&cast.task, TaskStatus::InProgress).await;
    let engineer_session = h
        .session(&goal, Some(&cast.task), Role::Engineer, &cast.engineer.id)
        .await;
    let review = h
        .task_on(
            &goal,
            &cast.repo,
            "under review",
            &cast.engineer,
            &[&cast.reviewer],
        )
        .await;
    h.advance(&review, TaskStatus::UnderReview).await;
    let reviewer_session = h
        .session(&goal, Some(&review), Role::Reviewer, &cast.reviewer.id)
        .await;

    assert_eq!(
        retire_on(&h, &engineer_session, AttentionReason::WaitingPermission).await,
        None,
        "nor is the engineer of a task still in progress"
    );
    assert_eq!(
        retire_on(&h, &reviewer_session, AttentionReason::WaitingPermission).await,
        None,
        "nor the reviewer of a round it has not voted in"
    );
}

/// Rows that were already stale when the daemon started are healed by the
/// first sweep — and only the ones that are nonsense: a session that ended
/// reporting an error, or having stalled, ended carrying something true.
#[tokio::test]
async fn a_stale_prompt_flag_from_before_the_daemon_started_is_swept_up() {
    let h = harness().cannot_spawn().await;
    let (goal, planner) = h.planning_goal().await;

    // Written the way an older daemon left them: ended, and still saying they
    // are waiting on somebody.
    let mut sessions = Vec::new();
    for reason in [
        AttentionReason::WaitingInput,
        AttentionReason::AgentError,
        AttentionReason::Stalled,
    ] {
        let session = h.session(&goal, None, Role::Planner, &planner.id).await;
        h.set_status(&session, SessionStatus::Exited).await;
        h.stale_attention(&session, reason).await;
        sessions.push(session);
    }

    // The sweep runs on the tick, and the first tick is immediate.
    let _sched = scheduler::start(h.store.clone(), h.launcher.clone(), false);
    eventually(TIMEOUT, "the stale prompt flag to be dropped", async || {
        h.attention(&sessions[0]).await.is_none()
    })
    .await;
    assert_eq!(
        h.attention(&sessions[1]).await,
        Some(AttentionReason::AgentError),
        "an error the agent reported before it died is still worth reading"
    );
    assert_eq!(
        h.attention(&sessions[2]).await,
        Some(AttentionReason::Stalled),
        "and so is the stall it ended in"
    );
}

// -- finished goals ---------------------------------------------------------

/// A finished goal owns nothing live, and the scheduler keeps it that way on
/// every pass rather than only on the way in.
///
/// The kill that runs at the transition is a one-off: a `resume` landing just
/// after it — the UI's button on the planner of a goal that had completed
/// seconds earlier — puts an agent back under a goal with no work left, where
/// it sits for ever holding the machine awake. So the completed arm reconciles
/// like every other one — convergently, rather than re-issuing the kill every
/// tick at a session that has already ended.
#[tokio::test]
async fn a_session_that_outlived_its_completed_goal_is_killed() {
    let h = harness().await;
    let (goal, planner) = h.planning_goal().await;
    h.store
        .set_goal_status(&goal.id, GoalStatus::Completed)
        .await
        .unwrap();
    // Live under a goal that was already finished, which is what a revive
    // racing the completion leaves behind.
    let session = h.session(&goal, None, Role::Planner, &planner.id).await;
    h.pane_exists(&session);

    let sched = Sched(scheduler::start(
        h.store.clone(),
        h.launcher.clone(),
        false,
    ));
    sched.goal(&goal);
    eventually(TIMEOUT, "the leftover planner to be killed", async || {
        !h.session_status(&session).await
            .is_live()
    })
    .await;

    // And the passes after it do nothing at all. The sends are ordered on one
    // channel, so the last one having been seen means the others have too.
    let keystrokes = h.keystrokes(&session);
    for _ in 0..3 {
        sched.goal(&goal);
    }
    eventually(TIMEOUT, "the passes to have run", async || {
        h.store.get_goal(&goal.id).await.unwrap().status() == GoalStatus::Completed
    })
    .await;
    assert_eq!(
        h.keystrokes(&session),
        keystrokes,
        "a finished session is not typed into"
    );
    assert_eq!(
        h.session_status(&session).await,
        SessionStatus::Exited
    );
}

/// A task nothing could be started for is a task nobody is coming back to:
/// the retry budget runs out, and the user is told once, in the task's own
/// thread, what stopped it.
#[tokio::test]
async fn a_task_that_could_never_be_started_tells_the_user_it_failed() {
    let w = World::cannot_spawn().await;
    let sched = w.scheduler();
    eventually(TIMEOUT, "the retry budget to run out", async || {
        sched.task(&w.task);
        w.store.get_task(&w.task.id).await.unwrap().status() == TaskStatus::Failed
    })
    .await;

    // Said once, however many passes ask about a task that has already ended.
    for _ in 0..3 {
        sched.task(&w.task);
    }
    eventually(TIMEOUT, "the failure to reach the user", async || {
        w.user_messages(&w.task).await.len() == 1
    })
    .await;
    let told = w.user_messages(&w.task).await;
    assert_eq!(told.len(), 1, "{told:?}");
    assert_eq!(told[0].author_role(), AuthorRole::System);
    assert!(told[0].body.contains(&w.task.title), "{}", told[0].body);
    assert!(
        told[0].body.contains("the agent could not be started"),
        "the notice does not say what stopped it: {}",
        told[0].body
    );
}

/// A goal the user cancelled takes its tasks with it, and every one of them
/// says so where it happened: a cancelled task is not a task that quietly
/// stopped.
#[tokio::test]
async fn a_cancelled_goal_tells_the_user_of_every_task_it_took_with_it() {
    let w = World::cannot_spawn().await;
    let second = w.extra_task("the other one").await;
    w.store
        .set_goal_status(&w.goal.id, GoalStatus::Cancelled)
        .await
        .unwrap();

    let sched = w.scheduler();
    for _ in 0..3 {
        sched.goal(&w.goal);
    }
    for task in [&w.task, &second] {
        eventually(TIMEOUT, "the task to be cancelled and said so", async || {
            w.store.get_task(&task.id).await.unwrap().status() == TaskStatus::Cancelled
                && !w.user_messages(task).await.is_empty()
        })
        .await;
        let told = w.user_messages(task).await;
        assert_eq!(told.len(), 1, "{told:?}");
        assert!(told[0].body.contains(&task.title), "{}", told[0].body);
        assert!(told[0].body.contains("goal cancelled"), "{}", told[0].body);
    }
}

/// And a goal whose tasks all landed ends in its own thread rather than in
/// the killing of its planner.
#[tokio::test]
async fn a_completed_goal_says_so_in_its_thread() {
    let w = World::active().await;
    w.advance(&w.task, TaskStatus::UnderReview).await;
    for (status, actor) in [
        (TaskStatus::Approved, Actor::Daemon),
        (TaskStatus::Merged, Actor::Engineer),
    ] {
        w.store
            .transition_task(
                &w.task.id,
                status,
                actor,
                None,
                (status == TaskStatus::Merged).then_some("cafe1234"),
            )
            .await
            .unwrap();
    }

    let sched = w.scheduler();
    for _ in 0..3 {
        sched.goal(&w.goal);
    }
    eventually(TIMEOUT, "the goal to be completed", async || {
        w.store.get_goal(&w.goal.id).await.unwrap().status() == GoalStatus::Completed
    })
    .await;
    let thread = w
        .store
        .list_goal_messages(&w.goal.id, None, 100)
        .await
        .unwrap();
    assert_eq!(thread.len(), 1, "{thread:?}");
    assert_eq!(thread[0].author_role(), AuthorRole::System);
    assert_eq!(thread[0].recipient(), None, "it wakes nobody");
    assert!(thread[0].body.contains(&w.goal.title), "{}", thread[0].body);
}

// -- deliveries and relaunches ----------------------------------------------

/// Three agents to nudge in one pass, and the pass does not wait on any of
/// them. Every delivery settles a paste and an Enter before it can say
/// whether the composer let go — a second or two each — which the loop used
/// to spend one agent at a time while every other event queued behind it.
#[tokio::test]
async fn a_pass_with_three_agents_to_nudge_does_not_wait_on_the_keystrokes() {
    let w = World::active().await;
    let second = w.extra_task("second").await;
    let third = w.extra_task("third").await;
    let mut sessions = Vec::new();
    for task in [&w.task, &second, &third] {
        let session = w.engineer_on(task, TaskStatus::InProgress).await;
        w.idle_for(&session, NUDGE_SECS + 60).await;
        sessions.push(session);
    }

    // The scheduler's opening reconciliation is the pass: it sees all three.
    let _sched = w.scheduler();

    // What is measured is the pass, not the machine it runs on: how long
    // after the first pane is typed into the last one is. A delivery settles
    // for a second before it can report anything, so three taken in turn put
    // seconds between the first and the last.
    let deadline = std::time::Instant::now() + TIMEOUT;
    let mut first: Option<std::time::Instant> = None;
    let spread = loop {
        let typed = sessions.iter().filter(|s| w.keystrokes(s) > 0).count();
        if typed > 0 && first.is_none() {
            first = Some(std::time::Instant::now());
        }
        if typed == sessions.len() {
            break first.unwrap().elapsed();
        }
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for all three panes to be typed into"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    assert!(
        spread < Duration::from_millis(900),
        "the three nudges went out together, not one after another: {spread:?}"
    );
}

/// A message that reached an agent is a nudge, and a better one: it says what
/// to do rather than asking why nothing is being done. So the pass that would
/// have nudged this session leaves it alone, and the escalation behind the
/// nudge does not happen either — the clock runs from the delivery.
#[tokio::test]
async fn a_delivered_message_stands_in_for_the_stall_nudge() {
    let w = World::active().await;
    let session = w.engineer_on(&w.task, TaskStatus::InProgress).await;
    w.idle_for(&session, 5).await;
    // Another task's agent, idle long enough to be nudged in the same passes:
    // what says a pass really looked at both of them.
    let other = w.extra_task("second").await;
    let canary = w.engineer_on(&other, TaskStatus::InProgress).await;
    w.idle_for(&canary, NUDGE_SECS + 60).await;

    let sched = w.scheduler();
    let message = w.message_to_engineer("Use the other endpoint.").await;
    sched.message(&message);
    eventually(TIMEOUT, "the message to reach the pane", async || {
        w.keystrokes(&session) > 1
    })
    .await;
    // The delivery is confirmed a beat after the Enter, by reading the pane
    // back; nothing here means anything until the scheduler has been told.
    tokio::time::sleep(Duration::from_secs(2)).await;
    let after_delivery = w.keystrokes(&session);

    // And now what the agent's own clock says: nothing since long before the
    // message — long enough to have been nudged, raised and relaunched, had
    // the delivery not been the freshest thing that happened to it.
    w.idle_for(&session, RELAUNCH_SECS + 60).await;
    for _ in 0..2 {
        sched.task(&w.task);
        sched.task(&other);
        eventually(TIMEOUT, "the other agent to be nudged", async || {
            w.keystrokes(&canary) > 0
        })
        .await;
    }

    assert_eq!(
        w.keystrokes(&session),
        after_delivery,
        "nothing was typed at an agent that has just been told what to do"
    );
    assert_eq!(
        w.attention(&session).await,
        None,
        "and it was not raised for the user either"
    );
    assert!(
        !w.store.get_task(&w.task.id).await.unwrap().is_stalled(),
        "nor was its task"
    );
    assert!(
        w.pane_is_alive(&session),
        "and its pane was left where it is"
    );
}

/// An agent wedged inside a turn: a model stream that never ends, a
/// subprocess that never returns, hooks that stopped firing. The session stays
/// `running`, its composer is empty — there is nothing to submit — and it
/// reports nothing at all.
///
/// The user first, because a person may know what the pane is doing; and if
/// the flag changes nothing, the pane is killed and the same session put back
/// on the conversation it was already having.
#[tokio::test]
async fn an_agent_that_reports_nothing_is_flagged_and_then_relaunched() {
    let w = World::active().await;
    let session = w.engineer_on(&w.task, TaskStatus::InProgress).await;
    w.make_resumable(&w.task, &session).await;
    w.launched_ago(&session, FLAG_SECS + 60).await;
    let launched = w.launched_at(&session).await;

    let sched = w.scheduler();
    sched.task(&w.task);
    eventually(TIMEOUT, "the wedged agent to be raised", async || {
        w.attention(&session).await == Some(AttentionReason::Stalled)
    })
    .await;
    assert!(
        w.store.get_task(&w.task.id).await.unwrap().is_stalled(),
        "the task carries the stall of the agent that stopped working"
    );
    assert_eq!(
        w.launched_at(&session).await,
        launched,
        "the flag comes first: nothing is relaunched at that threshold"
    );

    // Still silent a threshold later, and the flag was not the answer.
    w.launched_ago(&session, RELAUNCH_SECS + 60).await;
    let launched = w.launched_at(&session).await;
    sched.task(&w.task);
    eventually(TIMEOUT, "the wedged agent to be relaunched", async || {
        w.launched_at(&session).await != launched
    })
    .await;
    let back = w.store.get_session(&session.id).await.unwrap();
    assert_eq!(
        back.status(),
        SessionStatus::Running,
        "the same session is on air again"
    );
    assert_eq!(
        back.attention_reason(),
        None,
        "an agent that is running again needs nobody"
    );
    assert!(
        !w.store.get_task(&w.task.id).await.unwrap().is_stalled(),
        "and the task's stall goes with it"
    );
    assert_eq!(
        w.sessions_of(&w.task.id).await.len(),
        1,
        "the same session row, not a sibling beside it"
    );
    let argv = w.spawn_argv(&session.id);
    assert!(
        argv.contains("uuid-1234"),
        "the relaunch resumes the conversation it was having: {argv}"
    );
    assert!(
        argv.contains(&w.task.branch) && argv.contains("Pick \"task\" up again"),
        "and carries the resume its role is picked up with, rendered for this \
         task: {argv}"
    );
}

/// A turn that takes all afternoon is not a stall. What the thresholds
/// measure is silence, not duration: an agent that keeps reporting keeps its
/// own clock reset, however long the work in front of it runs.
#[tokio::test]
async fn a_running_agent_that_keeps_reporting_is_left_alone() {
    let w = World::active().await;
    let session = w.engineer_on(&w.task, TaskStatus::InProgress).await;
    w.make_resumable(&w.task, &session).await;
    // Launched long before both thresholds, and still saying so.
    w.launched_ago(&session, RELAUNCH_SECS * 2).await;
    w.reports(&session, "pre_tool_use").await;
    w.store.touch_session(&session.id).await.unwrap();
    let launched = w.launched_at(&session).await;
    // A second task whose engineer really has gone quiet: what it gets says
    // the pass the working one went through is over.
    let control_task = w.extra_task("control").await;
    let control = w.engineer_on(&control_task, TaskStatus::InProgress).await;
    w.launched_ago(&control, FLAG_SECS + 60).await;

    let sched = w.scheduler();
    sched.task(&w.task);
    sched.task(&control_task);
    eventually(TIMEOUT, "the silent agent to be raised", async || {
        w.attention(&control).await == Some(AttentionReason::Stalled)
    })
    .await;

    assert_eq!(
        w.attention(&session).await,
        None,
        "an agent that is reporting is not raised, however long its turn takes"
    );
    assert_eq!(
        w.launched_at(&session).await,
        launched,
        "nor relaunched out from under the work it is doing"
    );
    assert!(
        !w.store.get_task(&w.task.id).await.unwrap().is_stalled(),
        "and its task is not stalled either"
    );
}

/// An agent waiting on a person is blocked, not wedged: it is silent because
/// the answer it needs is a human's, and killing its pane would throw away
/// the dialog the user is about to answer.
#[tokio::test]
async fn a_running_agent_waiting_on_a_person_is_never_relaunched() {
    let w = World::active().await;
    let blocked = w.engineer_on(&w.task, TaskStatus::InProgress).await;
    w.make_resumable(&w.task, &blocked).await;
    w.launched_ago(&blocked, RELAUNCH_SECS + 60).await;
    w.raise(&blocked, AttentionReason::WaitingPermission).await;
    // The other half of the same rule, on a task of its own: an agent that
    // asked the user a question is waiting on the answer, not stuck.
    let asked_task = w.extra_task("asked").await;
    let asked = w.engineer_on(&asked_task, TaskStatus::InProgress).await;
    w.launched_ago(&asked, RELAUNCH_SECS + 60).await;
    w.raise(&asked, AttentionReason::WaitingInput).await;
    // And a third that is only wedged, whose relaunch says the passes are over.
    let control_task = w.extra_task("control").await;
    let control = w.engineer_on(&control_task, TaskStatus::InProgress).await;
    w.launched_ago(&control, FLAG_SECS + 60).await;

    let launched = [w.launched_at(&blocked).await, w.launched_at(&asked).await];
    let sched = w.scheduler();
    for task in [&w.task, &asked_task, &control_task] {
        sched.task(task);
    }
    eventually(TIMEOUT, "the wedged agent to be raised", async || {
        w.attention(&control).await == Some(AttentionReason::Stalled)
    })
    .await;

    for (session, reason, was) in [
        (&blocked, AttentionReason::WaitingPermission, &launched[0]),
        (&asked, AttentionReason::WaitingInput, &launched[1]),
    ] {
        assert_eq!(
            w.attention(session).await,
            Some(reason),
            "what the agent is waiting for is not overwritten with a stall"
        );
        assert_eq!(
            &w.launched_at(session).await,
            was,
            "and its pane is not killed out from under the dialog"
        );
    }
}

/// A relaunch is a remedy, not a loop. An agent that wedges again after every
/// one of them is not one more relaunch away from working, so the same budget
/// a spawn is given bounds this too, and the task ends failed rather than
/// being restarted for ever.
#[tokio::test]
async fn an_agent_that_wedges_after_every_relaunch_fails_its_task() {
    let w = World::active().await;
    let session = w.engineer_on(&w.task, TaskStatus::InProgress).await;
    w.make_resumable(&w.task, &session).await;

    let sched = w.scheduler();
    // Two relaunches out of the budget of three, each one wedging again.
    for relaunch in 1..=2 {
        w.launched_ago(&session, RELAUNCH_SECS + 60).await;
        let launched = w.launched_at(&session).await;
        sched.task(&w.task);
        eventually(TIMEOUT, &format!("relaunch {relaunch}"), async || {
            w.launched_at(&session).await != launched
        })
        .await;
    }
    assert_eq!(
        w.store.get_task(&w.task.id).await.unwrap().status(),
        TaskStatus::InProgress,
        "a task whose agent is being put back on its feet is still going"
    );

    // And it wedges once more.
    w.launched_ago(&session, RELAUNCH_SECS + 60).await;
    let launched = w.launched_at(&session).await;
    sched.task(&w.task);
    eventually(TIMEOUT, "the task to be failed", async || {
        w.store.get_task(&w.task.id).await.unwrap().status() == TaskStatus::Failed
    })
    .await;
    assert_eq!(
        w.launched_at(&session).await,
        launched,
        "the agent is not started a fourth time"
    );
    assert_eq!(
        w.session_status(&session).await,
        SessionStatus::Exited,
        "and it is not left holding a pane under a failed task"
    );
    // A task nobody is coming back to is told to the user, whichever watchdog
    // gave up on it.
    eventually(TIMEOUT, "the failure to reach the user", async || {
        w.user_messages(&w.task).await.len() == 1
    })
    .await;
    let told = w.user_messages(&w.task).await;
    assert!(
        told[0]
            .body
            .contains("stopped mid-turn after every relaunch"),
        "the notice says what stopped it: {}",
        told[0].body
    );
}

/// The planner's work ends with the plan. Once the goal it planned is being
/// worked on, an idle planner is an agent nobody is waiting on holding a pane
/// and the machine's sleep inhibitor open until the last task lands, so it is
/// let go — and being gone is what is expected of it, not a disconnect.
///
/// Gone, not unreachable: a task thread can still address its planner —
/// whose session belongs to the goal rather than to that task — and the
/// message is what brings the session back on the conversation it was
/// having.
#[tokio::test]
async fn an_idle_planner_is_let_go_once_the_goal_leaves_planning() {
    let w = World::active().await;
    // The planner's own cwd, which a revive needs to be there.
    std::fs::create_dir_all(w.dir.path().join("repo")).unwrap();
    let planner = w
        .session(&w.goal, None, Role::Planner, &w.goal.planner_profile_id)
        .await;
    w.pane_exists(&planner);
    w.store
        .set_session_internal_id(&planner.id, "uuid-planner")
        .await
        .unwrap();
    w.set_status(&planner, SessionStatus::Idle).await;

    // The first tick is immediate, and one reconciliation of the goal is all
    // it takes.
    let sched = w.scheduler();
    eventually(TIMEOUT, "the planner to be let go", async || {
        w.session_status(&planner).await == SessionStatus::Exited
    })
    .await;
    // Whatever the sweep beside it had to say would have been said by now.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        w.attention(&planner).await,
        None,
        "a planner that is done is expected to be gone, not reported disconnected"
    );

    // And the engineer of one of its tasks still reaches it, from a thread
    // the planner has no session in: the ended session is revived on the
    // agent conversation it was having, which is the whole reason it is ended
    // rather than kept alive.
    let engineer = w
        .session(&w.goal, Some(&w.task), Role::Engineer, &w.engineer)
        .await;
    let message = w
        .message(
            AuthorRole::Engineer,
            Some(&engineer.id),
            &w.goal.planner_profile_id.clone(),
            "Task three overlaps with task one; which of them owns the store?",
        )
        .await;
    sched.message(&message);

    eventually(TIMEOUT, "the planner to be revived with the message", async || {
        w.session_status(&planner).await != SessionStatus::Exited
    })
    .await;
    let argv = w.spawn_argv(&planner.id);
    assert!(
        argv.contains("uuid-planner"),
        "the same conversation is resumed: {argv}"
    );
    assert!(
        argv.contains("which of them owns the store?"),
        "and it is woken with what was said to it: {argv}"
    );
}

/// A pane with a delivery going into it is not a pane to kill, wedged or not:
/// the paste and the Enters behind it would come back as a message nobody
/// could be given, and the user would be told about a composer that was only
/// ever interrupted. The relaunch waits for the pass after the delivery has
/// settled — and then it happens, because a composer that took a paste says
/// nothing about the turn the agent is stuck in.
#[tokio::test]
async fn a_wedged_agent_is_not_killed_while_a_message_is_going_into_its_pane() {
    let w = World::active().await;
    let session = w.engineer_on(&w.task, TaskStatus::InProgress).await;
    w.make_resumable(&w.task, &session).await;
    // Past the flag and nowhere near the relaunch, so the first pass only
    // raises it: what the relaunch has to wait for is set up after that.
    w.launched_ago(&session, FLAG_SECS + 60).await;

    let sched = w.scheduler();
    sched.task(&w.task);
    eventually(TIMEOUT, "the wedged agent to be raised", async || {
        w.attention(&session).await == Some(AttentionReason::Stalled)
    })
    .await;

    // A composer that never lets go: the delivery spends its whole backoff in
    // the pane, which is the window this is about.
    w.composer_keeps("Use the other endpoint.");
    let message = w.message_to_engineer("Use the other endpoint.").await;
    sched.message(&message);
    eventually(TIMEOUT, "the message to reach the pane", async || {
        w.keystrokes(&session) > 0
    })
    .await;

    // Now it is past the relaunch threshold too, and every pass while the
    // pane is being typed into leaves it exactly where it is.
    w.launched_ago(&session, RELAUNCH_SECS + 60).await;
    let launched = w.launched_at(&session).await;
    for _ in 0..6 {
        sched.task(&w.task);
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(
            w.pane_is_alive(&session),
            "the pane was killed with a delivery going into it"
        );
        assert_eq!(
            w.launched_at(&session).await,
            launched,
            "and the session was relaunched under the delivery"
        );
    }

    // And once the delivery has settled, the wedge is still a wedge.
    eventually(TIMEOUT, "the wedged agent to be relaunched", async || {
        sched.task(&w.task);
        tokio::time::sleep(Duration::from_millis(100)).await;
        w.launched_at(&session).await != launched
    })
    .await;
}

/// A flag raised for the user is not an agent waiting on one. `waiting_user`
/// says the user has something to do about this task — a message written to
/// them, a request that is theirs to merge — and nothing about whether the
/// agent is working, so it is neither overwritten with a stall nor taken as a
/// reason to leave a wedged agent where it is.
#[tokio::test]
async fn a_wedged_agent_flagged_for_the_user_keeps_the_flag_and_is_relaunched() {
    let w = World::active().await;
    let session = w.engineer_on(&w.task, TaskStatus::InProgress).await;
    w.make_resumable(&w.task, &session).await;
    w.launched_ago(&session, FLAG_SECS + 60).await;
    w.raise(&session, AttentionReason::WaitingUser).await;
    // A second task's agent, wedged in the same way with nothing raised on
    // it: its flag is what says the pass the first one went through is over.
    let control_task = w.extra_task("control").await;
    let control = w.engineer_on(&control_task, TaskStatus::InProgress).await;
    w.launched_ago(&control, FLAG_SECS + 60).await;

    let sched = w.scheduler();
    sched.task(&w.task);
    sched.task(&control_task);
    eventually(TIMEOUT, "the other wedged agent to be raised", async || {
        w.attention(&control).await == Some(AttentionReason::Stalled)
    })
    .await;
    assert_eq!(
        w.attention(&session).await,
        Some(AttentionReason::WaitingUser),
        "what the user is owed is not overwritten with a stall"
    );

    // And the silence was measured all the same: a threshold later the agent
    // is put back on its feet like any other — and what the user is owed
    // comes back up with it, since a relaunch is not the user having merged
    // the request or read the message.
    w.launched_ago(&session, RELAUNCH_SECS + 60).await;
    let launched = w.launched_at(&session).await;
    sched.task(&w.task);
    eventually(TIMEOUT, "the wedged agent to be relaunched", async || {
        w.launched_at(&session).await != launched
    })
    .await;
    eventually(
        TIMEOUT,
        "the flag raised for the user to survive the relaunch",
        async || w.attention(&session).await == Some(AttentionReason::WaitingUser),
    )
    .await;
    assert_eq!(
        w.session_status(&session).await,
        SessionStatus::Running,
        "on the agent that is running again, not on the row it was killed in"
    );
    assert!(
        !w.store.get_task(&w.task.id).await.unwrap().is_stalled(),
        "and what the user is owed is not the task stalling"
    );
}

impl World {
    /// A message in the task's thread, addressed to its engineer.
    async fn message_to_engineer(&self, body: &str) -> String {
        self.message(AuthorRole::User, None, &self.engineer.clone(), body)
            .await
    }

    async fn message(
        &self,
        author_role: AuthorRole,
        author_session_id: Option<&str>,
        to: &str,
        body: &str,
    ) -> String {
        self.store
            .create_message(NewMessage {
                goal_id: self.goal.id.clone(),
                task_id: Some(self.task.id.clone()),
                author_role,
                author_session_id: author_session_id.map(str::to_string),
                recipient: Some(Recipient::Profile(to.to_string())),
                body: body.into(),
            })
            .await
            .unwrap()
            .id
    }
}
