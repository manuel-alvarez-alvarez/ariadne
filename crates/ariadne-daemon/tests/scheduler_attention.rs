//! What the scheduler notices about agents that stopped working.
//!
//! One clock — how long since the session was last heard from at all — and
//! one timeline on it: a nudge, then the user, then the agent killed and put
//! back on its feet. The three thresholds are the scheduler's own,
//! read from it rather than copied, so a test says "past the nudge" and means
//! whatever that is today. Every seat
//! is under it, since every seat can go quiet: the orchestrator of a goal still
//! being planned, the reviewers a round is waiting on, and the author —
//! which is the only one whose task carries a flag of its own next to the
//! session's. An agent that dies while its work is still going says so too,
//! rather than ending quietly.
//!
//! Only an idle agent is nudged, told to get on with the work as a prompt;
//! an agent in the middle of a turn is left alone until the thresholds
//! behind the nudge. The agents are the harness's stub, running under the
//! sessions a test seeds, and a nudge is read back from the prompts it was
//! sent.
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
    Actor, AttentionReason, GoalStatus, MessageKind, Seat, SessionStatus, TaskStatus,
};
// The watchdog's timeline and the orchestrator's budget come from the scheduler
// rather than being written down again here, so that moving a threshold moves
// the tests with it instead of leaving them backdating clocks past a number
// nothing uses any more.
use ariadne_daemon::scheduler::{
    self, QUIET_FLAG_SECS as FLAG_SECS, QUIET_NUDGE_SECS as NUDGE_SECS,
    QUIET_RELAUNCH_SECS as RELAUNCH_SECS, START_GRACE_SECS, SchedEvent,
};
use ariadne_store::{AgentSession, Goal, NewTaskAgent, SessionFilter, Task};

use common::{Harness, eventually, harness, test_pin};

/// The budget the goal's orchestrator spends: how many attempts starting one is
/// worth, as the scheduler has it.
const SPAWN_RETRY_BUDGET: usize = ariadne_daemon::scheduler::SPAWN_RETRY_BUDGET as usize;
/// How long a test waits for a reconciliation to reach the store. Generous
/// because some of what is waited on is not the daemon thinking: a stub agent
/// is a process to start and talk to, and every test here runs beside the
/// others.
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
    author: String,
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

    /// The same with two reviewers on the task: a round one verdict does not
    /// close is where a reviewer sits with its work done.
    async fn reviewed_by(reviewers: usize) -> World {
        World::build(harness().await, reviewers).await
    }

    /// A daemon that cannot start anything: the registry's agent names no
    /// executable, so every fresh session dies at the launch.
    ///
    /// What a vanished agent leaves behind is only itself visible while nothing
    /// has replaced it — a successful replacement is supposed to clear the
    /// flag — so the tests about what the sweep concluded run where no
    /// replacement can happen, and the one about the replacement runs where it
    /// can.
    async fn cannot_spawn() -> World {
        World::build(harness().cannot_spawn().await, 1).await
    }

    async fn build(h: Harness, reviewers: usize) -> World {
        let cast = h.cast_reviewed_by(reviewers).await;
        let goal = h.activate(&cast.goal).await;
        World {
            h,
            goal,
            task: cast.task,
            author: cast.author.id,
            reviewer: cast.reviewer.id,
        }
    }

    /// Another task on the same goal, with the same agents behind it.
    async fn extra_task(&self, title: &str) -> Task {
        let repo = self
            .store
            .list_goal_repositories(&self.goal.id)
            .await
            .unwrap()[0]
            .clone();
        self.store
            .create_task(ariadne_store::NewTask {
                goal_id: self.goal.id.clone(),
                repo_id: repo.id,
                title: title.into(),
                description: "do things".into(),
                agents: vec![
                    NewTaskAgent::new(Seat::Author, ["coding"], test_pin()),
                    NewTaskAgent::new(Seat::Reviewer, ["code-review"], test_pin()),
                ],
                depends_on: vec![],
                landing: None,
                permission_mode: None,
            })
            .await
            .unwrap()
    }

    /// The author of a task walked to `status`, with a stub agent running
    /// under it: the opening most of these tests share.
    async fn author_on(&self, task: &Task, status: TaskStatus) -> AgentSession {
        self.advance(task, status).await;
        let session = self
            .session(&self.goal, Some(task), Seat::Author, &self.author)
            .await;
        self.agent_runs(&session).await;
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
}

/// The author's resume template, as its profile has it: the words the daemon
/// nudges an idle author with.
const RESUME: &str = r#"Continue "task" on"#;

// -- the timeline -----------------------------------------------------------

/// An orchestrator has no task to flag, so its own session is where a goal that
/// stopped being planned says so.
#[tokio::test]
async fn an_orchestrator_idle_past_the_threshold_is_raised_on_its_session() {
    let h = harness().await;
    let goal = h.planning_goal().await;
    let session = h.orchestrator_session(&goal).await;
    h.agent_runs(&session).await;
    h.idle_for(&session, NUDGE_SECS + 60).await;

    // One pass per threshold: the nudge, and then the escalation behind it.
    let sched = Sched(scheduler::start(h.store.clone(), h.launcher.clone(), false));
    sched.goal(&goal);
    eventually(TIMEOUT, "the orchestrator to be nudged", async || {
        !h.prompts_to(&session).is_empty()
    })
    .await;
    h.idle_for(&session, FLAG_SECS + 60).await;
    sched.goal(&goal);
    eventually(TIMEOUT, "the orchestrator to be raised", async || {
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
        .session(&w.goal, Some(&w.task), Seat::Reviewer, &w.reviewer)
        .await;
    w.agent_runs(&session).await;
    w.idle_for(&session, NUDGE_SECS + 60).await;

    let sched = w.scheduler();
    sched.task(&w.task);
    eventually(TIMEOUT, "the reviewer to be nudged", async || {
        !w.prompts_to(&session).is_empty()
    })
    .await;
    w.idle_for(&session, FLAG_SECS + 60).await;
    sched.task(&w.task);
    eventually(TIMEOUT, "the reviewer to be raised", async || {
        w.attention(&session).await == Some(AttentionReason::Stalled)
    })
    .await;
}

/// The author keeps its task-level flag, and now says it on its session too.
#[tokio::test]
async fn an_author_stall_flags_the_task_and_its_session() {
    let w = World::active().await;
    let session = w.author_on(&w.task, TaskStatus::InProgress).await;
    w.idle_for(&session, NUDGE_SECS + 60).await;

    let sched = w.scheduler();
    sched.task(&w.task);
    eventually(TIMEOUT, "the author to be nudged", async || {
        !w.prompts_to(&session).is_empty()
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
    let session = w.author_on(&w.task, TaskStatus::InProgress).await;
    w.idle_for(&session, NUDGE_SECS + 60).await;

    let sched = w.scheduler();
    sched.task(&w.task);
    eventually(TIMEOUT, "the author to be nudged", async || {
        !w.prompts_to(&session).is_empty()
    })
    .await;
    // Whatever else that pass had to say would have been said by now.
    tokio::time::sleep(Duration::from_millis(500)).await;
    let nudged = w.prompts_to(&session).len();

    // A second task's agent, quiet in the same way from now on: its nudge is
    // what says the passes the first one went through are over.
    let control_task = w.extra_task("control").await;
    let control = w.author_on(&control_task, TaskStatus::InProgress).await;
    w.idle_for(&control, NUDGE_SECS + 60).await;
    // And the first one is as quiet as ever, still in the situation it was
    // nudged for.
    w.idle_for(&session, NUDGE_SECS + 120).await;
    for _ in 0..2 {
        sched.task(&w.task);
        sched.task(&control_task);
        eventually(TIMEOUT, "the other agent to be nudged", async || {
            !w.prompts_to(&control).is_empty()
        })
        .await;
    }

    assert_eq!(
        w.prompts_to(&session).len(),
        nudged,
        "nothing more was sent to an agent that has already been nudged"
    );
    assert_eq!(
        w.attention(&session).await,
        None,
        "and the escalation behind the nudge is the next threshold's, not this pass's"
    );
}

/// An agent waiting on a person is blocked, not stalled: the answer it waits
/// on is the person's to give, so it is left alone, flag and all.
#[tokio::test]
async fn a_session_waiting_on_a_person_is_never_nudged() {
    let w = World::active().await;
    let session = w.author_on(&w.task, TaskStatus::InProgress).await;
    w.idle_for(&session, NUDGE_SECS + 60).await;
    w.raise(&session, AttentionReason::WaitingPermission).await;
    // A second task, idle in exactly the same way but blocked on nothing: its
    // nudge is what says the pass the blocked one went through is over.
    let control_task = w.extra_task("control").await;
    let control = w.author_on(&control_task, TaskStatus::InProgress).await;
    w.idle_for(&control, NUDGE_SECS + 60).await;

    let sched = w.scheduler();
    sched.task(&w.task);
    sched.task(&control_task);
    eventually(TIMEOUT, "the unblocked author to be nudged", async || {
        !w.prompts_to(&control).is_empty()
    })
    .await;

    assert_eq!(
        w.prompts_to(&session).len(),
        0,
        "nothing is sent to an agent that is asking the user something"
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

/// An agent in the middle of a turn is left alone at the first threshold: a
/// nudge would only queue behind the turn it is in. A turn that never ends is
/// what the thresholds behind the nudge are for.
#[tokio::test]
async fn an_agent_in_the_middle_of_a_turn_is_not_nudged() {
    let w = World::active().await;
    let session = w.author_on(&w.task, TaskStatus::InProgress).await;
    w.launched_ago(&session, NUDGE_SECS + 60).await;

    // A second task's author, idle in the same silence: its nudge is what
    // says the pass the working one went through is over.
    let control_task = w.extra_task("control").await;
    let control = w.author_on(&control_task, TaskStatus::InProgress).await;
    w.idle_for(&control, NUDGE_SECS + 60).await;

    let sched = w.scheduler();
    sched.task(&w.task);
    sched.task(&control_task);
    eventually(TIMEOUT, "the idle author to be nudged", async || {
        !w.prompts_to(&control).is_empty()
    })
    .await;

    assert_eq!(
        w.prompts_to(&session).len(),
        0,
        "nothing is sent to an agent that is working"
    );
    assert_eq!(
        w.attention(&session).await,
        None,
        "nor is it raised for the user this early"
    );
}

/// An agent that reported an error is already asking for the user by name.
/// A failed turn is reported as `session.error`, and the ingest leaves the
/// session running with the error raised — which is not a reason the user is
/// better off hearing as a stall.
#[tokio::test]
async fn an_agent_that_reported_an_error_is_left_alone() {
    let w = World::active().await;
    let errored = w.author_on(&w.task, TaskStatus::InProgress).await;
    w.launched_ago(&errored, RELAUNCH_SECS + 60).await;
    w.raise(&errored, AttentionReason::AgentError).await;
    // And an agent whose silence nothing explains, whose flag says the passes
    // are over.
    let control_task = w.extra_task("control").await;
    let control = w.author_on(&control_task, TaskStatus::InProgress).await;
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
        w.prompts_to(&errored).len(),
        0,
        "nothing is sent to an agent whose turn failed"
    );
    assert_eq!(
        w.attention(&errored).await,
        Some(AttentionReason::AgentError),
        "what it reported is not overwritten with a stall"
    );
    assert_eq!(
        w.launched_at(&errored).await,
        launched,
        "and its agent is not killed out from under the failure"
    );
}

// -- the sweep --------------------------------------------------------------

/// An agent that vanished while its work was still going is not a session
/// that finished: it is an agent the user has lost, and it says so until something
/// puts it back — whatever the agent happened to be asking when it went, since
/// what the user has to know is that the work lost its agent.
///
/// An orchestrator, so that nothing but the sweep is under test: the goal's own
/// reconciliation cannot start a replacement here (the repository is not a git
/// repository) and would have nothing to say about attention if it could.
#[tokio::test]
async fn a_vanished_agent_with_work_still_active_is_flagged_disconnected() {
    let h = harness().cannot_spawn().await;
    let goal = h.planning_goal().await;
    // Launched and running in the database, with no agent process under it.
    // Launched rather than merely written, since a row whose start is still
    // in front of it has no agent yet for reasons that are nobody's alarm —
    // which is the grace window's own test below.
    let session = h.orchestrator_session(&goal).await;
    h.launched_ago(&session, 60).await;
    // And a second one that was sitting on a question that died with it.
    let on_a_prompt = h.orchestrator_session(&goal).await;
    h.launched_ago(&on_a_prompt, 60).await;
    h.raise(&on_a_prompt, AttentionReason::WaitingPermission)
        .await;

    // The sweep runs on the tick, and the first tick is immediate.
    let sched = Sched(scheduler::start(h.store.clone(), h.launcher.clone(), false));
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

/// The author of an active task with no live session is resumed blind, and
/// when even that cannot get off the ground the session it tried to bring back
/// is the thing the user has to look at.
#[tokio::test]
async fn an_author_that_cannot_be_resumed_is_flagged_disconnected() {
    let w = World::cannot_spawn().await;
    w.advance(&w.task, TaskStatus::InProgress).await;
    // Ended, with no agent conversation to resume and no git repository to
    // spawn a fresh one in: the resume attempt cannot succeed.
    let session = w
        .session(&w.goal, Some(&w.task), Seat::Author, &w.author)
        .await;
    w.set_status(&session, SessionStatus::Exited).await;

    let sched = w.scheduler();
    sched.task(&w.task);
    eventually(TIMEOUT, "the failed resume to be raised", async || {
        w.attention(&session).await == Some(AttentionReason::Disconnected)
    })
    .await;
}

/// An agent going away when nobody is waiting on it is just a session
/// ending: the author of a task under review is waiting on its reviewers and
/// is woken by id when they answer, a reviewer that has voted is finished
/// however long the round runs on, and a cancelled task is owed nothing at
/// all. All three are retired, and none of them raised.
#[tokio::test]
async fn a_vanished_agent_nobody_is_waiting_on_is_not_raised() {
    // Two approvals wanted, one given: the round stays open around a reviewer
    // that has nothing left to do, so the status is not what makes it quiet.
    let w = World::reviewed_by(2).await;
    w.advance(&w.task, TaskStatus::UnderReview).await;
    // Entering review opens a round: the verdict belongs to that one.
    let under_review = w.store.get_task(&w.task.id).await.unwrap();

    let author = w
        .session(&w.goal, Some(&under_review), Seat::Author, &w.author)
        .await;
    let voted = w
        .session(&w.goal, Some(&under_review), Seat::Reviewer, &w.reviewer)
        .await;
    w.verdict_from(&under_review, &voted, MessageKind::Approve, "looks right")
        .await;

    let cancelled_task = w.extra_task("cancelled").await;
    let cancelled = w
        .session(&w.goal, Some(&cancelled_task), Seat::Author, &w.author)
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

    // All three had launched and were running when their agents went: a session
    // still starting is inside the sweep's grace window, and left alone
    // whoever it belongs to.
    for gone in [&author, &voted, &cancelled] {
        w.launched_ago(gone, 60).await;
    }

    let _sched = w.scheduler();
    for gone in [&author, &voted, &cancelled] {
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
    for gone in [&author, &voted, &cancelled] {
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
    let goal = h.planning_goal().await;
    // The orchestrator cwd has to exist for the spawn to get off the ground.
    std::fs::create_dir_all(h.dir.path().join("repo")).unwrap();
    let session = h.orchestrator_session(&goal).await;
    h.set_status(&session, SessionStatus::Exited).await;
    h.raise(&session, AttentionReason::Disconnected).await;

    // Nothing live for the goal, so reconciliation starts a new orchestrator.
    let sched = Sched(scheduler::start(h.store.clone(), h.launcher.clone(), false));
    sched.goal(&goal);
    eventually(
        TIMEOUT,
        "the replacement orchestrator to be running",
        async || {
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
        },
    )
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
        .session(&w.goal, Some(&w.task), Seat::Author, &w.author)
        .await;
    w.make_resumable(&w.task, &session).await;
    w.set_status(&session, SessionStatus::Exited).await;
    w.raise(&session, AttentionReason::Disconnected).await;

    let resumed = w
        .launcher
        .resume_author(&w.task.id, "Continue where you left off.")
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
/// a session waiting on an answer reports nothing: the sweep is what lets go of
/// an author that was blocked on a permission prompt when its task moved on
/// to its reviewers — and only then. An agent the work is still waiting on
/// keeps its flag, down to the moment it went up, since how long it has been
/// stuck is the half of it the user acts on.
#[tokio::test]
async fn the_sweep_lets_go_of_a_blocked_agent_only_once_its_work_moved_on() {
    let w = World::cannot_spawn().await;
    let session = w.author_on(&w.task, TaskStatus::InProgress).await;
    w.raise(&session, AttentionReason::WaitingPermission).await;
    let raised_at = w
        .store
        .get_session(&session.id)
        .await
        .unwrap()
        .attention_since;

    // A second author, blocked in exactly the same way but on a task that
    // has gone to its reviewers: whatever the prompt was about, it got past it
    // and sent the task for review, and nothing more will ever be reported on
    // that session.
    let handed_over = w.extra_task("under review").await;
    let control = w.author_on(&handed_over, TaskStatus::UnderReview).await;
    w.raise(&control, AttentionReason::WaitingPermission).await;

    // The sweep runs on the tick, and the first tick is immediate.
    let _sched = w.scheduler();
    eventually(TIMEOUT, "the finished author to be let go", async || {
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

/// A prompt is a question to a live agent: nobody can answer one on a
/// session that has ended, so retiring a session takes what it was waiting on
/// with it. Every seat, and every one of them with its work still owed —
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

    // One goal, walked from planning to active, so every seat is retired in
    // the state its own work is still going in.
    let h = harness().await;
    let cast = h.cast().await;
    let orchestrator_session = h.orchestrator_session(&cast.goal).await;
    assert_eq!(
        retire_on(&h, &orchestrator_session, AttentionReason::WaitingInput).await,
        None,
        "the goal is still being planned, and the orchestrator is still waiting on nobody"
    );

    let goal = h.activate(&cast.goal).await;
    h.advance(&cast.task, TaskStatus::InProgress).await;
    let author_session = h
        .session(&goal, Some(&cast.task), Seat::Author, &cast.author.id)
        .await;
    let review = h
        .task_on(&goal, &cast.repo, "under review", 1, test_pin())
        .await;
    h.advance(&review, TaskStatus::UnderReview).await;
    let reviewer_session = h
        .session(&goal, Some(&review), Seat::Reviewer, &cast.reviewer.id)
        .await;

    assert_eq!(
        retire_on(&h, &author_session, AttentionReason::WaitingPermission).await,
        None,
        "nor is the author of a task still in progress"
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
    let goal = h.planning_goal().await;

    // Written the way an older daemon left them: ended, and still saying they
    // are waiting on somebody.
    let mut sessions = Vec::new();
    for reason in [
        AttentionReason::WaitingInput,
        AttentionReason::AgentError,
        AttentionReason::Stalled,
    ] {
        let session = h.orchestrator_session(&goal).await;
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

/// A session on its way up has no agent yet, and that is not an agent that
/// vanished. The row goes into `starting` before its agent exists — a spawn
/// writes it before it launches, and a resume from the API takes the old
/// agent down before the new one is up — so a sweep landing in that window
/// used to retire a session that was coming back and raise `disconnected` on
/// it: an alarm that took itself down again on the agent's first event,
/// having flashed on the strip and over SSE in between. A start older than
/// the window is a launch that is not coming, and is swept as ever.
#[tokio::test]
async fn a_starting_session_is_swept_only_once_its_grace_window_has_run_out() {
    let w = World::cannot_spawn().await;
    w.advance(&w.task, TaskStatus::InProgress).await;
    // Two authors with no agent between them, and nothing but the age of
    // their start to tell them apart.
    let coming_up = w
        .session(&w.goal, Some(&w.task), Seat::Author, &w.author)
        .await;
    let never_came_up = w
        .session(&w.goal, Some(&w.task), Seat::Author, &w.author)
        .await;
    w.starting_for(&never_came_up, START_GRACE_SECS + 60).await;

    // The sweep runs on the tick, and the first tick is immediate.
    let _sched = w.scheduler();
    eventually(
        TIMEOUT,
        "the launch that never arrived to be swept",
        async || w.attention(&never_came_up).await == Some(AttentionReason::Disconnected),
    )
    .await;
    assert_eq!(
        w.session_status(&never_came_up).await,
        SessionStatus::Exited,
        "a session that has been starting for longer than the window is retired"
    );
    assert_eq!(
        w.session_status(&coming_up).await,
        SessionStatus::Starting,
        "and one whose row was written a moment ago is left where it is"
    );
    assert_eq!(
        w.attention(&coming_up).await,
        None,
        "with nothing raised on it: it has not failed to do anything yet"
    );
}

// -- finished goals ---------------------------------------------------------

/// A finished goal owns nothing live, and the scheduler keeps it that way on
/// every pass rather than only on the way in.
///
/// The kill that runs at the transition is a one-off: a `resume` landing just
/// after it — the UI's button on the orchestrator of a goal that had completed
/// seconds earlier — puts an agent back under a goal with no work left, where
/// it sits for ever holding the machine awake. So the completed arm reconciles
/// like every other one — convergently, rather than re-issuing the kill every
/// tick at a session that has already ended.
#[tokio::test]
async fn a_session_that_outlived_its_completed_goal_is_killed() {
    let h = harness().await;
    let goal = h.planning_goal().await;
    h.store
        .set_goal_status(&goal.id, GoalStatus::Completed)
        .await
        .unwrap();
    // Live under a goal that was already finished, which is what a revive
    // racing the completion leaves behind.
    let session = h.orchestrator_session(&goal).await;
    h.agent_runs(&session).await;

    let sched = Sched(scheduler::start(h.store.clone(), h.launcher.clone(), false));
    sched.goal(&goal);
    eventually(
        TIMEOUT,
        "the leftover orchestrator to be killed",
        async || !h.session_status(&session).await.is_live(),
    )
    .await;

    // And the passes after it do nothing at all. The sends are ordered on one
    // channel, so the last one having been seen means the others have too.
    let prompted = h.prompts_to(&session).len();
    for _ in 0..3 {
        sched.goal(&goal);
    }
    eventually(TIMEOUT, "the passes to have run", async || {
        h.store.get_goal(&goal.id).await.unwrap().status() == GoalStatus::Completed
    })
    .await;
    assert_eq!(
        h.prompts_to(&session).len(),
        prompted,
        "a finished session is sent nothing"
    );
    assert_eq!(h.session_status(&session).await, SessionStatus::Exited);
}

/// A task nothing could be started for is a task nobody is coming back to:
/// the retry budget runs out, and the task itself says what stopped it — its
/// status, and the reason on the transition that ended it.
#[tokio::test]
async fn a_task_that_could_never_be_started_fails_with_the_reason_on_it() {
    let w = World::cannot_spawn().await;
    let sched = w.scheduler();
    eventually(TIMEOUT, "the retry budget to run out", async || {
        sched.task(&w.task);
        w.store.get_task(&w.task.id).await.unwrap().status() == TaskStatus::Failed
    })
    .await;

    // Failed once, however many passes ask about a task that has already
    // ended.
    for _ in 0..3 {
        sched.task(&w.task);
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    let ended: Vec<_> = w
        .store
        .list_task_transitions(&w.task.id)
        .await
        .unwrap()
        .into_iter()
        .filter(|t| t.to_status == TaskStatus::Failed.as_str())
        .collect();
    assert_eq!(ended.len(), 1, "{ended:?}");
    assert_eq!(
        ended[0].reason.as_deref(),
        Some("the agent could not be started"),
        "the task does not say what stopped it"
    );
}

/// Every session this goal has, which on a goal with no tasks is every
/// orchestrator it ever tried to start.
async fn orchestrators(h: &Harness, goal: &Goal) -> Vec<AgentSession> {
    h.store
        .list_sessions(SessionFilter {
            goal_id: Some(goal.id.clone()),
            ..Default::default()
        })
        .await
        .unwrap()
}

/// A goal in planning always wants an orchestrator, and its row goes in
/// before the launch: a spawn that cannot get off the ground — a model the
/// agent does not know, an agent that is not installed — used to leave a
/// fresh "disconnected" session on the strip every tick, for ever. So the
/// attempts are counted the way a task's author's are, and when they run out
/// one row is left carrying the alarm.
///
/// The user's answer to it is that alarm coming down: taking it down is what
/// says somebody has dealt with what stopped it, and the count starts again
/// from there.
#[tokio::test]
async fn an_orchestrator_that_can_never_be_started_gives_up_with_one_alarm() {
    let h = harness().cannot_spawn().await;
    let goal = h.planning_goal().await;
    // The orchestrator's cwd has to exist for an attempt to get as far as the
    // launch it cannot perform — and for the user's resume to get that far
    // too.
    std::fs::create_dir_all(h.dir.path().join("repo")).unwrap();

    let sched = Sched(scheduler::start(h.store.clone(), h.launcher.clone(), false));
    for _ in 0..SPAWN_RETRY_BUDGET + 2 {
        sched.goal(&goal);
    }
    eventually(
        TIMEOUT,
        "the orchestrator's spawn budget to run out",
        async || {
            orchestrators(&h, &goal)
                .await
                .iter()
                .any(|s| s.attention_reason() == Some(AttentionReason::Disconnected))
        },
    )
    .await;

    let rows = orchestrators(&h, &goal).await;
    let mut flagged = rows
        .iter()
        .filter(|s| s.attention_reason() == Some(AttentionReason::Disconnected));
    let alarm = flagged.next().expect("a row carrying the alarm").clone();
    assert!(
        flagged.next().is_none(),
        "one goal, one row that says anything: {rows:?}"
    );
    assert!(
        rows.iter().all(|s| !s.status().is_live()),
        "and no orchestrator left running: {rows:?}"
    );

    // And the passes after it try nothing at all: no new row.
    for _ in 0..3 {
        sched.goal(&goal);
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        orchestrators(&h, &goal).await.len(),
        rows.len(),
        "an orchestrator that was given up on is not spawned again"
    );

    // The user's answer: the alarm taken down. An orchestrator that never
    // started has no conversation for a resume to go back to, so the flag
    // coming down is the whole of it — which is what the daemon reads as
    // dealt with.
    h.store.clear_session_attention(&alarm.id).await.unwrap();
    assert_eq!(h.attention(&alarm).await, None, "the alarm is down");

    sched.goal(&goal);
    eventually(
        TIMEOUT,
        "the daemon to try starting one again",
        async || orchestrators(&h, &goal).await.len() > rows.len(),
    )
    .await;
}

/// A launch that works and an agent that runs are not the same thing. An
/// orchestrator whose agent comes up and exits — a protocol it will not
/// speak, a folder it will not open — leaves the seat empty again
/// within a tick, and a goal always wants that seat filled: the daemon spawned
/// one every five seconds for as long as the goal lived, and the user was
/// never told, because the alarm each death raised was cleared by the
/// replacement that went on to die the same way.
///
/// So a death on arrival spends an attempt like a launch that never got off
/// the ground, and it ends where that one ends: one alarm, on one row, and
/// nothing started again. Here every launch works — the agent process is
/// started — and it exits at once, never heard from.
#[tokio::test]
async fn an_orchestrator_that_dies_the_moment_it_starts_is_given_up_on() {
    let h = harness().dying_agent().await;
    let goal = h.planning_goal().await;
    // The cwd of a launch has to exist for the launch to be performed at all.
    std::fs::create_dir_all(h.dir.path().join("repo")).unwrap();

    let sched = Sched(scheduler::start(h.store.clone(), h.launcher.clone(), false));
    sched.goal(&goal);
    // Every death is flagged by the sweep and cleared by the launch that
    // replaces it, so the flag alone says nothing: what is waited for is the
    // budget out — as many launches as it is worth, every one of them over,
    // and the last of them still carrying its alarm.
    eventually(TIMEOUT, "the deaths to run the budget out", async || {
        let rows = orchestrators(&h, &goal).await;
        rows.len() >= SPAWN_RETRY_BUDGET
            && rows.iter().all(|s| !s.status().is_live())
            && rows
                .iter()
                .any(|s| s.attention_reason() == Some(AttentionReason::Disconnected))
    })
    .await;

    let rows = orchestrators(&h, &goal).await;
    assert_eq!(
        rows.iter()
            .filter(|s| s.attention_reason() == Some(AttentionReason::Disconnected))
            .count(),
        1,
        "one goal, one row that says anything: {rows:?}"
    );

    // And a tick later nothing has been started again: given up on, rather
    // than between two launches.
    for _ in 0..3 {
        sched.goal(&goal);
    }
    tokio::time::sleep(Duration::from_secs(scheduler::TICK_SECS + 2)).await;
    let after = orchestrators(&h, &goal).await;
    assert_eq!(
        after.len(),
        rows.len(),
        "an orchestrator that was given up on is not spawned again: {after:?}"
    );
    assert_eq!(
        after
            .iter()
            .filter(|s| s.attention_reason() == Some(AttentionReason::Disconnected))
            .count(),
        1,
        "and the alarm the user answers stands: {after:?}"
    );
}

/// The same for the agent of a task, which has a task to fail rather than an
/// alarm to leave: an author that comes up and is never heard from spends the
/// task's budget, and what the user reads afterwards is the task saying that
/// its agent stopped as soon as it started — not that it could not be started,
/// which is a different thing that has already been ruled out.
#[tokio::test]
async fn a_task_whose_agent_dies_the_moment_it_starts_fails_with_the_reason_on_it() {
    let h = harness().dying_agent().await;
    // A real repository: an author is launched in a worktree of it, and the
    // launch has to work for the death that follows to be the thing under
    // test.
    h.git_repo("repo");
    let cast = h.cast().await;
    let goal = h.activate(&cast.goal).await;

    let sched = Sched(scheduler::start(h.store.clone(), h.launcher.clone(), false));
    sched.goal(&goal);
    sched.task(&cast.task);
    eventually(
        TIMEOUT,
        "the deaths to run the task's budget out",
        async || h.store.get_task(&cast.task.id).await.unwrap().status() == TaskStatus::Failed,
    )
    .await;

    let ended: Vec<_> = h
        .store
        .list_task_transitions(&cast.task.id)
        .await
        .unwrap()
        .into_iter()
        .filter(|t| t.to_status == TaskStatus::Failed.as_str())
        .collect();
    assert_eq!(ended.len(), 1, "{ended:?}");
    assert_eq!(
        ended[0].reason.as_deref(),
        Some("its agent stopped as soon as it started"),
        "the task does not say what stopped it"
    );
}

/// A goal the user cancelled takes its tasks with it, and every one of them
/// records why: a cancelled task is not a task that quietly stopped.
#[tokio::test]
async fn a_cancelled_goal_records_why_on_every_task_it_took_with_it() {
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
        eventually(TIMEOUT, "the task to be cancelled", async || {
            w.store.get_task(&task.id).await.unwrap().status() == TaskStatus::Cancelled
        })
        .await;
        let ended: Vec<_> = w
            .store
            .list_task_transitions(&task.id)
            .await
            .unwrap()
            .into_iter()
            .filter(|t| t.to_status == TaskStatus::Cancelled.as_str())
            .collect();
        assert_eq!(ended.len(), 1, "{ended:?}");
        assert_eq!(ended[0].reason.as_deref(), Some("goal cancelled"));
    }
}

/// A goal whose tasks all landed is not completed by the daemon: whether the
/// goal is *met* is a judgement about the work, and only its orchestrator can
/// make it. What the daemon does is say that there is nothing left running,
/// to the orchestrator's own agent, and `complete_goal` is the answer.
#[tokio::test]
async fn a_goal_whose_tasks_all_landed_wakes_its_orchestrator() {
    let w = World::active().await;
    let orchestrator = w.orchestrator_session(&w.goal).await;
    w.agent_runs(&orchestrator).await;
    w.set_status(&orchestrator, SessionStatus::Idle).await;
    w.advance(&w.task, TaskStatus::UnderReview).await;
    for (status, actor) in [
        (TaskStatus::Approved, Actor::Daemon),
        (TaskStatus::Finished, Actor::Author),
    ] {
        w.store
            .transition_task(
                &w.task.id,
                status,
                actor,
                None,
                (status == TaskStatus::Finished).then_some("cafe1234"),
            )
            .await
            .unwrap();
    }

    let sched = w.scheduler();
    for _ in 0..3 {
        sched.goal(&w.goal);
    }
    eventually(TIMEOUT, "the orchestrator to be woken", async || {
        w.prompted(&orchestrator).contains("Every task is done.")
    })
    .await;
    assert_eq!(
        w.store.get_goal(&w.goal.id).await.unwrap().status(),
        GoalStatus::Active,
        "the daemon does not decide the goal is met"
    );
}

// -- deliveries and relaunches ----------------------------------------------

/// Three agents to nudge in one pass, and the pass hands all three their
/// prompt at once: a delivery is queued by the runtime, never waited on.
#[tokio::test]
async fn a_pass_with_three_agents_to_nudge_does_not_wait_on_the_deliveries() {
    let w = World::active().await;
    let second = w.extra_task("second").await;
    let third = w.extra_task("third").await;
    let mut sessions = Vec::new();
    for task in [&w.task, &second, &third] {
        let session = w.author_on(task, TaskStatus::InProgress).await;
        w.idle_for(&session, NUDGE_SECS + 60).await;
        sessions.push(session);
    }

    // The scheduler's opening reconciliation is the pass: it sees all three.
    let _sched = w.scheduler();
    eventually(TIMEOUT, "all three agents to be nudged", async || {
        sessions.iter().all(|s| !w.prompts_to(s).is_empty())
    })
    .await;
}

/// An agent wedged inside a turn: a model stream that never ends, a
/// subprocess that never returns. The session stays `running` and reports
/// nothing at all.
///
/// The user first, because a person may know what the agent is doing; and if
/// the flag changes nothing, the agent is killed and the same session put
/// back on the conversation it was already having.
#[tokio::test]
async fn an_agent_that_reports_nothing_is_flagged_and_then_relaunched() {
    let w = World::active().await;
    let session = w.author_on(&w.task, TaskStatus::InProgress).await;
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
    // Waited for through the status rather than the stamp: the row is put
    // back into `starting` and stamped before its agent is up, so a read
    // taken on the stamp alone can catch it on its way up.
    eventually(TIMEOUT, "the wedged agent to be relaunched", async || {
        w.launched_at(&session).await != launched
            && w.session_status(&session).await.is_live()
            && w.session_status(&session).await != SessionStatus::Starting
    })
    .await;
    let back = w.store.get_session(&session.id).await.unwrap();
    assert!(back.status().is_live(), "the same session is on air again");
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
    let launch = w.launch_file(&session.id).expect("a launch file");
    assert_eq!(
        launch.resume_session_id.as_deref(),
        Some("uuid-1234"),
        "the relaunch resumes the conversation it was having"
    );
    let resume = launch.initial_prompt.unwrap_or_default();
    assert!(
        resume.contains(&w.task.branch) && resume.contains(RESUME),
        "and carries the resume its seat is picked up with, rendered for this \
         task: {resume}"
    );
}

/// A turn that takes all afternoon is not a stall. What the thresholds
/// measure is silence, not duration: an agent that keeps reporting keeps its
/// own clock reset, however long the work in front of it runs.
#[tokio::test]
async fn a_running_agent_that_keeps_reporting_is_left_alone() {
    let w = World::active().await;
    let session = w.author_on(&w.task, TaskStatus::InProgress).await;
    w.make_resumable(&w.task, &session).await;
    // Launched long before both thresholds, and still saying so.
    w.launched_ago(&session, RELAUNCH_SECS * 2).await;
    w.reports(&session, "pre_tool_use").await;
    w.store.touch_session(&session.id).await.unwrap();
    let launched = w.launched_at(&session).await;
    // A second task whose author really has gone quiet: what it gets says
    // the pass the working one went through is over.
    let control_task = w.extra_task("control").await;
    let control = w.author_on(&control_task, TaskStatus::InProgress).await;
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
/// the answer it needs is a human's, and killing it would throw away the
/// question the user is about to answer.
#[tokio::test]
async fn a_running_agent_waiting_on_a_person_is_never_relaunched() {
    let w = World::active().await;
    let blocked = w.author_on(&w.task, TaskStatus::InProgress).await;
    w.make_resumable(&w.task, &blocked).await;
    w.launched_ago(&blocked, RELAUNCH_SECS + 60).await;
    w.raise(&blocked, AttentionReason::WaitingPermission).await;
    // The other half of the same rule, on a task of its own: an agent that
    // asked the user a question is waiting on the answer, not stuck.
    let asked_task = w.extra_task("asked").await;
    let asked = w.author_on(&asked_task, TaskStatus::InProgress).await;
    w.launched_ago(&asked, RELAUNCH_SECS + 60).await;
    w.raise(&asked, AttentionReason::WaitingInput).await;
    // And a third that is only wedged, whose relaunch says the passes are over.
    let control_task = w.extra_task("control").await;
    let control = w.author_on(&control_task, TaskStatus::InProgress).await;
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
            "and its agent is not killed out from under the question"
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
    let session = w.author_on(&w.task, TaskStatus::InProgress).await;
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
        "and it is not left running under a failed task"
    );
    // A task nobody is coming back to carries why, whichever watchdog gave up
    // on it.
    let ended = w.store.list_task_transitions(&w.task.id).await.unwrap();
    let reason = ended
        .iter()
        .rev()
        .find(|t| t.to_status == TaskStatus::Failed.as_str())
        .and_then(|t| t.reason.clone())
        .expect("the task says why it failed");
    assert!(
        reason.contains("stopped mid-turn after every relaunch"),
        "{reason}"
    );
}

/// The orchestrator outlives the plan. Once the goal it planned is being
/// worked on it stays up — it is what the user talks to about work already
/// running, and what the daemon tells when a task needs a decision — and an
/// orchestrator with nothing to do is neither ended nor nudged nor flagged.
#[tokio::test]
async fn an_idle_orchestrator_stays_up_for_the_whole_goal() {
    let w = World::active().await;
    // The orchestrator's own cwd, which a revive needs to be there.
    std::fs::create_dir_all(w.dir.path().join("repo")).unwrap();
    let orchestrator = w.orchestrator_session(&w.goal).await;
    w.agent_runs(&orchestrator).await;
    w.store
        .set_session_internal_id(&orchestrator.id, "uuid-orchestrator")
        .await
        .unwrap();
    w.set_status(&orchestrator, SessionStatus::Idle).await;

    let _sched = w.scheduler();
    // Whatever the passes and the sweep beside them had to say would have
    // been said by now.
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert_eq!(
        w.session_status(&orchestrator).await,
        SessionStatus::Idle,
        "the orchestrator was let go once its plan was under way"
    );
    assert_eq!(
        w.prompted(&orchestrator),
        "",
        "nothing happened on the goal, and the daemon prompted its agent anyway"
    );
    assert_eq!(
        w.attention(&orchestrator).await,
        None,
        "an orchestrator with nothing to do is not one the user is called for"
    );
}

/// A flag raised for the user is not an agent waiting on one. `waiting_user`
/// says the user has something to do about this task — a request that is
/// theirs to merge — and nothing about whether the agent is working, so it is
/// neither overwritten with a stall nor taken as a reason to leave a wedged
/// agent where it is.
#[tokio::test]
async fn a_wedged_agent_flagged_for_the_user_keeps_the_flag_and_is_relaunched() {
    let w = World::active().await;
    let session = w.author_on(&w.task, TaskStatus::InProgress).await;
    w.make_resumable(&w.task, &session).await;
    w.launched_ago(&session, FLAG_SECS + 60).await;
    w.raise(&session, AttentionReason::WaitingUser).await;
    // A second task's agent, wedged in the same way with nothing raised on
    // it: its flag is what says the pass the first one went through is over.
    let control_task = w.extra_task("control").await;
    let control = w.author_on(&control_task, TaskStatus::InProgress).await;
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
    // the request.
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
    assert!(
        w.session_status(&session).await.is_live(),
        "on the agent that is running again, not on the row it was killed in"
    );
    assert!(
        !w.store.get_task(&w.task.id).await.unwrap().is_stalled(),
        "and what the user is owed is not the task stalling"
    );
}

/// And the flag comes back up for an author that was never carrying it, on
/// the one thing a restart cannot settle: a task published as a request.
///
/// The chain this is about is the whole of an approved task's ending. The
/// author opens the request, tells the user it is theirs to merge, and stops
/// — it has nothing left to do until they do. Its agent going away is read as
/// a disconnect, since landing the change is still the author's turn, and the
/// resume that answers the disconnect wipes the row clean. Nothing in any of
/// that merged anything, so the task must still say who it is waiting for
/// afterwards.
#[tokio::test]
async fn a_published_task_still_says_the_merge_is_the_users_after_its_author_is_resumed() {
    let w = World::active().await;
    w.advance(&w.task, TaskStatus::UnderReview).await;
    // Live in the database with no agent process under it: the agent that
    // stopped.
    let session = w
        .session(&w.goal, Some(&w.task), Seat::Author, &w.author)
        .await;
    w.make_resumable(&w.task, &session).await;
    w.store
        .transition_task(&w.task.id, TaskStatus::Approved, Actor::Daemon, None, None)
        .await
        .unwrap();
    w.store
        .set_task_pull_request(&w.task.id, "https://example.test/pull/1")
        .await
        .unwrap();

    // One pass does all of it: the sweep retires the vanished agent and raises
    // the disconnect, and the task's own reconciliation puts an author back
    // on it.
    let _sched = w.scheduler();
    eventually(
        TIMEOUT,
        "the author to be put back on the task",
        async || {
            // Launched at all, and up now: the row it comes back in is the
            // one that went down, since the conversation was there to resume.
            w.launched_at(&session).await.is_some()
                && w.session_status(&session).await.is_live()
                && w.session_status(&session).await != SessionStatus::Starting
        },
    )
    .await;
    eventually(
        TIMEOUT,
        "the request to still be the user's to merge",
        async || w.attention(&session).await == Some(AttentionReason::WaitingUser),
    )
    .await;
    assert!(
        !w.store.get_task(&w.task.id).await.unwrap().is_stalled(),
        "and what the user is owed is not the task stalling"
    );
}

/// A task that failed is a decision no author can make: retry it, rewrite it,
/// staff it differently or give it up. So the daemon wakes the orchestrator,
/// the agent the user is also talking to, and says which task it is.
///
/// Once per situation, not once per pass: a second task failing is news, and
/// the same one still failed is not.
#[tokio::test]
async fn a_failed_task_wakes_the_orchestrator_once() {
    let w = World::active().await;
    let orchestrator = w.orchestrator_session(&w.goal).await;
    w.agent_runs(&orchestrator).await;
    w.set_status(&orchestrator, SessionStatus::Idle).await;
    w.advance(&w.task, TaskStatus::InProgress).await;
    w.store
        .transition_task(
            &w.task.id,
            TaskStatus::Failed,
            Actor::Author,
            Some("the crate the task names was deleted upstream"),
            None,
        )
        .await
        .unwrap();

    let sched = w.scheduler();
    sched.goal(&w.goal);
    eventually(TIMEOUT, "the orchestrator to be woken", async || {
        w.prompted(&orchestrator).contains("failed")
    })
    .await;
    let woken = w.prompted(&orchestrator);
    assert!(woken.contains(&w.task.title), "{woken}");
    assert!(woken.contains("`list_tasks`"), "{woken}");

    // Every pass after it says the same thing, so nothing is said again.
    let said = w.prompted(&orchestrator).matches("`list_tasks`").count();
    for _ in 0..3 {
        sched.goal(&w.goal);
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    assert_eq!(
        w.prompted(&orchestrator).matches("`list_tasks`").count(),
        said,
        "the orchestrator was woken again for the same situation"
    );
}

/// Work in progress is what the orchestrator delegated, so it is not woken
/// for it: a task being written and a task under review are the delegation
/// working.
#[tokio::test]
async fn a_goal_whose_tasks_are_running_leaves_its_orchestrator_alone() {
    let w = World::active().await;
    let orchestrator = w.orchestrator_session(&w.goal).await;
    w.agent_runs(&orchestrator).await;
    w.set_status(&orchestrator, SessionStatus::Idle).await;
    w.advance(&w.task, TaskStatus::UnderReview).await;

    let sched = w.scheduler();
    for _ in 0..3 {
        sched.goal(&w.goal);
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    assert!(
        !w.prompted(&orchestrator).contains("`list_tasks`"),
        "{:?}",
        w.prompted(&orchestrator)
    );
}
