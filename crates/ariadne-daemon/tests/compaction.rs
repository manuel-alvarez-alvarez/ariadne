//! The compaction every agent session is owed at a hand-off, and how it is
//! paid.
//!
//! Three hand-offs owe one: a review requested owes the engineer's, a verdict
//! given owes the reviewer's, a plan finalized owes the planner's. Paying it
//! is typing the CLI's `/compact` into a pane that is free — the turn ended,
//! nothing being typed, no dialog up — and then leaving that pane alone until
//! the CLI reports the compaction done, or the wait for that runs out. A
//! resume that becomes due meanwhile goes out after it, once.
//!
//! The scheduler is started after the seeding, as in the watchdog tests, so
//! the pass a test asks for is the first one over the state it wrote.

mod common;

use std::ops::Deref;
use std::time::Duration;

use tokio::sync::mpsc::UnboundedSender;

use ariadne_core::{ReviewVerdict, Role, SessionStatus, TaskStatus};
use ariadne_daemon::scheduler::{self, SchedEvent};
use ariadne_store::{AgentSession, EventFilter, Goal, NewReview, Task};

use common::{Harness, HarnessBuilder, eventually, harness};

const TIMEOUT: Duration = Duration::from_secs(30);

/// What Claude Code reports when a compaction it was asked for is over: the
/// same `SessionStart` a resume fires, with the source that tells them apart.
fn compacted() -> serde_json::Value {
    serde_json::json!({
        "session_id": "5f3b1c8e-1234-4a2b-9d0e-0123456789ab",
        "cwd": "/tmp/wt",
        "hook_event_name": "SessionStart",
        "source": "compact",
    })
}

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
    async fn active() -> World {
        World::build(harness(), 1).await
    }

    async fn needing(approvals: i64) -> World {
        World::build(harness(), approvals).await
    }

    async fn build(builder: HarnessBuilder, approvals: i64) -> World {
        let h = builder.await;
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

    /// The engineer of the task, at its prompt in a pane the stub answers
    /// for, with a conversation to resume: what a task under review has.
    async fn idle_engineer(&self) -> AgentSession {
        let session = self
            .session(&self.goal, Some(&self.task), Role::Engineer, &self.engineer)
            .await;
        self.make_resumable(&self.task, &session).await;
        self.pane_exists(&session);
        self.set_status(&session, SessionStatus::Idle).await;
        session
    }

    /// A reviewer of the task at its prompt, with its verdict in for the
    /// round the task is on now — going under review opened it.
    async fn reviewer_that_voted(&self, verdict: ReviewVerdict) -> AgentSession {
        let session = self
            .session(&self.goal, Some(&self.task), Role::Reviewer, &self.reviewer)
            .await;
        self.pane_exists(&session);
        self.set_status(&session, SessionStatus::Idle).await;
        let round = self
            .store
            .get_task(&self.task.id)
            .await
            .unwrap()
            .review_round;
        self.store
            .create_review(NewReview {
                task_id: self.task.id.clone(),
                round,
                reviewer_profile_id: self.reviewer.clone(),
                session_id: Some(session.id.clone()),
                verdict,
                body: Some("Looks fine.".into()),
            })
            .await
            .unwrap();
        session
    }

    fn scheduler(&self) -> Sched {
        Sched(scheduler::start(
            self.store.clone(),
            self.launcher.clone(),
            false,
        ))
    }
}

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

impl Harness {
    async fn owes_compaction(&self, session: &AgentSession) -> bool {
        self.store
            .get_session(&session.id)
            .await
            .unwrap()
            .compact_owed_at
            .is_some()
    }

    /// The kinds of the daemon's own compaction events in this session's
    /// log, in order, with the reason of each failed one.
    async fn compaction_events(&self, session: &AgentSession) -> Vec<String> {
        self.store
            .list_events(EventFilter {
                session_id: Some(session.id.clone()),
                task_id: None,
                limit: 50,
                after: None,
            })
            .await
            .unwrap()
            .into_iter()
            .filter(|e| e.kind.starts_with("compaction"))
            .map(|e| {
                let payload: serde_json::Value = serde_json::from_str(&e.payload).unwrap();
                match payload.get("reason").and_then(|r| r.as_str()) {
                    Some(reason) => format!("{}:{reason}", e.kind),
                    None => e.kind,
                }
            })
            .collect()
    }

    /// Wait for the compaction to have gone into the pane and been recorded.
    async fn compaction_typed(&self, session: &AgentSession) {
        eventually(TIMEOUT, "the compaction to be typed", async || {
            self.compaction_events(session).await == vec!["compaction"]
        })
        .await;
    }
}

// -- the three hand-offs ----------------------------------------------------

/// A review requested is the engineer's hand-off: the session owes a
/// compaction, and gets it as soon as it is at its prompt — `/compact` with
/// the engineer's focus, typed and confirmed, and written to its log.
#[tokio::test]
async fn a_review_requested_owes_the_engineer_a_compaction() {
    let w = World::active().await;
    let engineer = w.idle_engineer().await;
    w.advance(&w.task, TaskStatus::UnderReview).await;

    let sched = w.scheduler();
    sched.task(&w.task);
    eventually(TIMEOUT, "the engineer to owe a compaction", async || {
        w.owes_compaction(&engineer).await
    })
    .await;
    w.compaction_typed(&engineer).await;

    let pasted = w.pasted(&engineer);
    assert!(
        pasted.contains("/compact Keep the task and the branch."),
        "the engineer's focus rides on the command: {pasted:?}"
    );
    assert!(
        pasted.contains("Keep the open review points."),
        "{pasted:?}"
    );
    assert!(
        w.enters(&engineer) >= 1,
        "and the command was submitted, not left in the composer"
    );
    assert!(
        w.owes_compaction(&engineer).await,
        "the debt stands until the CLI says the compaction is done"
    );
}

/// Once per hand-off: the passes that follow see the same review round and
/// owe nothing more, and a debt paid is not typed for again.
#[tokio::test]
async fn a_hand_off_is_paid_once_however_many_passes_see_it() {
    let w = World::active().await;
    let engineer = w.idle_engineer().await;
    w.advance(&w.task, TaskStatus::UnderReview).await;

    let sched = w.scheduler();
    sched.task(&w.task);
    w.compaction_typed(&engineer).await;

    // The CLI says it is done; the debt is paid.
    w.ingest(&engineer, "session_start", compacted()).await;
    eventually(TIMEOUT, "the debt to be paid", async || {
        !w.owes_compaction(&engineer).await
    })
    .await;
    assert_eq!(
        w.session_status(&engineer).await,
        SessionStatus::Idle,
        "a compaction reported done leaves the agent at its prompt, not in a turn"
    );

    // Passes over the same round, with the engineer at its prompt all along.
    for _ in 0..3 {
        sched.task(&w.task);
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    assert!(
        !w.owes_compaction(&engineer).await,
        "the same hand-off is not owed twice"
    );
    assert_eq!(
        w.compaction_events(&engineer).await,
        vec!["compaction"],
        "and nothing more was typed for it"
    );
}

/// A verdict given is the reviewer's hand-off. The round stays open here —
/// two approvals wanted, one in — so the reviewer sits at its prompt with
/// its work done, which is exactly when the compaction goes in.
#[tokio::test]
async fn a_verdict_given_owes_the_reviewer_a_compaction() {
    let w = World::needing(2).await;
    w.advance(&w.task, TaskStatus::UnderReview).await;
    let reviewer = w.reviewer_that_voted(ReviewVerdict::Approve).await;

    let sched = w.scheduler();
    sched.task(&w.task);
    eventually(TIMEOUT, "the reviewer to owe a compaction", async || {
        w.owes_compaction(&reviewer).await
    })
    .await;
    w.compaction_typed(&reviewer).await;
    assert!(
        w.pasted(&reviewer)
            .contains("/compact Keep what you checked."),
        "{:?}",
        w.pasted(&reviewer)
    );
}

/// A reviewer whose round has closed is ended — but not before the
/// compaction its verdict earned is done, since its session serves the next
/// round too. The pane is left up through the compaction, and killed on the
/// pass after the CLI reports it over.
#[tokio::test]
async fn a_reviewer_that_voted_is_ended_only_once_its_compaction_is_done() {
    let w = World::active().await;
    let _engineer = w.idle_engineer().await;
    w.advance(&w.task, TaskStatus::UnderReview).await;
    let reviewer = w.reviewer_that_voted(ReviewVerdict::Approve).await;

    let sched = w.scheduler();
    sched.task(&w.task);
    eventually(TIMEOUT, "the round to close", async || {
        w.status(&w.task.id).await == TaskStatus::Approved
    })
    .await;
    w.compaction_typed(&reviewer).await;
    assert!(
        w.pane_is_alive(&reviewer),
        "the reviewer's pane is not killed under its compaction"
    );
    assert_eq!(
        w.session_status(&reviewer).await,
        SessionStatus::Idle,
        "nor is the session retired"
    );

    w.ingest(&reviewer, "session_start", compacted()).await;
    eventually(
        TIMEOUT,
        "the reviewer to be ended after its compaction",
        async || {
            !w.pane_is_alive(&reviewer)
                && w.session_status(&reviewer).await == SessionStatus::Exited
        },
    )
    .await;
}

/// A plan finalized is the planner's hand-off: it owes a compaction, is left
/// up through it, and is let go once it is done.
#[tokio::test]
async fn a_plan_finalized_owes_the_planner_a_compaction_before_it_is_let_go() {
    let h = harness().await;
    let (goal, planner_profile) = h.planning_goal().await;
    let planner = h
        .session(&goal, None, Role::Planner, &planner_profile.id)
        .await;
    h.pane_exists(&planner);
    h.set_status(&planner, SessionStatus::Idle).await;
    let goal = h.activate(&goal).await;

    let sched = Sched(scheduler::start(h.store.clone(), h.launcher.clone(), false));
    sched.goal(&goal);
    eventually(TIMEOUT, "the planner to owe a compaction", async || {
        h.owes_compaction(&planner).await
    })
    .await;
    h.compaction_typed(&planner).await;
    assert!(
        h.pasted(&planner).contains("/compact Keep the goal."),
        "{:?}",
        h.pasted(&planner)
    );
    assert!(
        h.pane_is_alive(&planner),
        "an idle planner is not ended while it owes a compaction"
    );

    h.ingest(&planner, "session_start", compacted()).await;
    eventually(
        TIMEOUT,
        "the planner to be ended after its compaction",
        async || !h.pane_is_alive(&planner),
    )
    .await;
}

// -- when the pane is not free ----------------------------------------------

/// A session in the middle of a turn owes the compaction all the same, and
/// is not typed into for it: the debt waits for its prompt. A second
/// engineer, at its prompt, is the control that says the passes came round.
#[tokio::test]
async fn a_session_mid_turn_owes_the_compaction_and_is_not_typed_into() {
    let w = World::active().await;
    let busy = w.idle_engineer().await;
    w.set_status(&busy, SessionStatus::Running).await;
    w.advance(&w.task, TaskStatus::UnderReview).await;

    let repo = w.store.list_goal_repositories(&w.goal.id).await.unwrap()[0].clone();
    let control_task = w
        .task_on(
            &w.goal,
            &repo,
            "control",
            &w.store.get_profile(&w.engineer).await.unwrap(),
            &[&w.store.get_profile(&w.reviewer).await.unwrap()],
        )
        .await;
    let control = w
        .session(&w.goal, Some(&control_task), Role::Engineer, &w.engineer)
        .await;
    w.pane_exists(&control);
    w.set_status(&control, SessionStatus::Idle).await;
    w.advance(&control_task, TaskStatus::UnderReview).await;

    let sched = w.scheduler();
    sched.task(&w.task);
    sched.task(&control_task);
    w.compaction_typed(&control).await;
    for _ in 0..2 {
        sched.task(&w.task);
        tokio::time::sleep(Duration::from_millis(300)).await;
    }

    assert!(w.owes_compaction(&busy).await, "the debt is noted");
    assert_eq!(
        w.keystrokes(&busy),
        0,
        "and nothing is typed into a pane whose agent is mid-turn"
    );

    // Its turn ends: the next pass pays the debt.
    w.ingest(
        &busy,
        "stop",
        serde_json::json!({"hook_event_name": "Stop"}),
    )
    .await;
    w.compaction_typed(&busy).await;
}

/// A dialog only a person may answer is never typed into: the Enter behind
/// the paste would answer it. Through whole ticks, too: the stale-attention
/// sweep drops the flags of an agent nobody waits on — and nobody waits on
/// the engineer of a task under review — but a dialog on a pane the daemon
/// is about to type into is not stale, so the flag stands with the debt.
#[tokio::test]
async fn a_session_waiting_on_a_person_is_not_typed_into() {
    let w = World::active().await;
    let engineer = w.idle_engineer().await;
    w.advance(&w.task, TaskStatus::UnderReview).await;
    w.raise(&engineer, ariadne_core::AttentionReason::WaitingPermission)
        .await;

    let sched = w.scheduler();
    sched.task(&w.task);
    eventually(TIMEOUT, "the engineer to owe a compaction", async || {
        w.owes_compaction(&engineer).await
    })
    .await;
    // Past the next full tick, which runs every sweep over this session.
    tokio::time::sleep(Duration::from_secs(scheduler::TICK_SECS + 1)).await;
    sched.task(&w.task);
    tokio::time::sleep(Duration::from_millis(300)).await;

    assert!(w.owes_compaction(&engineer).await);
    assert_eq!(w.keystrokes(&engineer), 0);
    assert_eq!(
        w.attention(&engineer).await,
        Some(ariadne_core::AttentionReason::WaitingPermission),
        "the dialog is still flagged: it is what keeps the pane from being typed into"
    );
}

/// The same on a daemon starting cold over the hand-off: nothing has
/// reconciled the task yet, so the debt is not on the row when the first
/// full pass begins. That pass writes it before it judges the flags, so the
/// prompt is not dropped as stale in the same breath and the compaction pass
/// after it finds the dialog still flagged. No task event is sent: the tick
/// is the only thing that runs.
#[tokio::test]
async fn a_cold_start_over_a_hand_off_keeps_the_prompt_and_types_nothing() {
    let w = World::active().await;
    let engineer = w.idle_engineer().await;
    w.advance(&w.task, TaskStatus::UnderReview).await;
    w.raise(&engineer, ariadne_core::AttentionReason::WaitingPermission)
        .await;

    let _sched = w.scheduler();
    eventually(
        TIMEOUT,
        "the first pass to owe the compaction",
        async || w.owes_compaction(&engineer).await,
    )
    .await;
    // Through the next full tick and its compaction pass.
    tokio::time::sleep(Duration::from_secs(scheduler::TICK_SECS + 1)).await;

    assert_eq!(
        w.attention(&engineer).await,
        Some(ariadne_core::AttentionReason::WaitingPermission),
        "the dialog is still flagged"
    );
    assert_eq!(w.keystrokes(&engineer), 0, "and nothing was typed into it");
    assert!(
        w.owes_compaction(&engineer).await,
        "the debt waits for the answer"
    );
}

/// A CLI with little to summarise can report the compaction done while the
/// command's keystrokes are still settling. The debt is paid by the report;
/// the delivery that comes back afterwards stands down rather than opening
/// a wait on a compaction already over, and nothing lingers: the resume that
/// follows goes out at once.
#[tokio::test]
async fn a_compaction_reported_done_before_its_delivery_settles_is_over() {
    let w = World::active().await;
    let engineer = w.idle_engineer().await;
    w.advance(&w.task, TaskStatus::UnderReview).await;
    // A composer that never lets go keeps the delivery busy for a few
    // seconds of Enters, which is the window the done signal lands in.
    w.composer_keeps("/compact Keep the task and the branch.");

    let sched = w.scheduler();
    sched.task(&w.task);
    eventually(TIMEOUT, "the compaction to start going in", async || {
        w.keystrokes(&engineer) > 0
    })
    .await;
    w.ingest(&engineer, "session_start", compacted()).await;
    assert!(
        !w.owes_compaction(&engineer).await,
        "the done signal pays the debt"
    );

    // The delivery settles a few seconds later; whatever the pane read
    // back, the compaction it was about is over.
    eventually(
        TIMEOUT,
        "the delivery to settle and be recorded",
        async || w.compaction_events(&engineer).await == vec!["compaction"],
    )
    .await;
    assert_eq!(
        w.attention(&engineer).await,
        None,
        "a composer read as holding the command is not a stalled agent when the CLI compacted"
    );

    // Nothing lingers: feedback that arrives now reaches the engineer at
    // once, which it would not if the pane were still held for a compaction.
    let round = w.store.get_task(&w.task.id).await.unwrap().review_round;
    w.store
        .create_review(NewReview {
            task_id: w.task.id.clone(),
            round,
            reviewer_profile_id: w.reviewer.clone(),
            session_id: None,
            verdict: ReviewVerdict::RequestChanges,
            body: Some("Rename the flag.".into()),
        })
        .await
        .unwrap();
    sched.task(&w.task);
    eventually(
        TIMEOUT,
        "the engineer to be resumed with the feedback",
        async || w.status(&w.task.id).await == TaskStatus::InProgress,
    )
    .await;
}

// -- the wait, and what waits on it -----------------------------------------

/// A compaction the CLI never reports done is given up on: the debt is
/// written off, saying why, and nothing waits on that pane any longer.
#[tokio::test]
async fn a_compaction_nobody_reports_done_is_written_off() {
    let w = World::build(harness().compaction_timeout(Duration::from_secs(1)), 1).await;
    let engineer = w.idle_engineer().await;
    w.advance(&w.task, TaskStatus::UnderReview).await;

    let sched = w.scheduler();
    sched.task(&w.task);
    w.compaction_typed(&engineer).await;

    eventually(TIMEOUT, "the debt to be written off", async || {
        !w.owes_compaction(&engineer).await
    })
    .await;
    assert_eq!(
        w.compaction_events(&engineer).await,
        vec!["compaction", "compaction_failed:timed_out"]
    );
}

/// Review feedback that becomes due while the engineer is compacting goes
/// out after the compaction, once: the pane is neither killed nor typed
/// into under it, and the resume that carries the feedback is the first
/// thing that happens once the CLI reports the compaction done.
#[tokio::test]
async fn a_resume_due_during_a_compaction_goes_out_after_it() {
    let w = World::active().await;
    let engineer = w.idle_engineer().await;
    w.advance(&w.task, TaskStatus::UnderReview).await;

    let sched = w.scheduler();
    sched.task(&w.task);
    w.compaction_typed(&engineer).await;
    let launches_before = w.tmux_calls_of("new-session").len();

    // The reviewer asks for changes while the compaction runs.
    let round = w.store.get_task(&w.task.id).await.unwrap().review_round;
    w.store
        .create_review(NewReview {
            task_id: w.task.id.clone(),
            round,
            reviewer_profile_id: w.reviewer.clone(),
            session_id: None,
            verdict: ReviewVerdict::RequestChanges,
            body: Some("Rename the flag.".into()),
        })
        .await
        .unwrap();
    sched.task(&w.task);
    eventually(TIMEOUT, "the changes to be requested", async || {
        w.status(&w.task.id).await == TaskStatus::ChangesRequested
    })
    .await;
    for _ in 0..2 {
        sched.task(&w.task);
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    assert!(
        w.pane_is_alive(&engineer),
        "the engineer's pane is not killed under its compaction"
    );
    assert_eq!(
        w.tmux_calls_of("new-session").len(),
        launches_before,
        "and it is not relaunched with the feedback yet"
    );
    assert_eq!(
        w.status(&w.task.id).await,
        TaskStatus::ChangesRequested,
        "the task waits for the pass after the compaction"
    );

    w.ingest(&engineer, "session_start", compacted()).await;
    eventually(
        TIMEOUT,
        "the engineer to be resumed with the feedback",
        async || w.status(&w.task.id).await == TaskStatus::InProgress,
    )
    .await;
    assert_eq!(
        w.tmux_calls_of("new-session").len(),
        launches_before + 1,
        "resumed once, after the compaction"
    );
    assert!(
        w.launched_at(&engineer).await.is_some(),
        "as the same session, relaunched on its compacted conversation"
    );
}
