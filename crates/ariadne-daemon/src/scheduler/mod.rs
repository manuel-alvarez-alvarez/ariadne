//! Scheduler: an event-driven reconciliation loop.
//!
//! HTTP handlers send [`SchedEvent`]s after writes; a periodic tick reconciles
//! everything so crashes, missed events and dead agent processes self-heal.
//! Every rule is idempotent — read state, compare desired, act — which is why
//! a pass that arrives late does what the state says now rather than replaying
//! what it missed.
//!
//! The rules are a module each: `goals` and `tasks` for what the two entities
//! want, `sweeps` for the two passes that see every session whatever it
//! belongs to, and `quiet` for the watchdog over an agent that stopped
//! reporting. What the scheduler says to an agent goes out through
//! [`Scheduler::hand_prompt`].

mod coalesce;
mod goals;
mod messages;
mod quiet;
mod sweeps;
mod tasks;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use ariadne_core::{GoalStatus, Seat};
use ariadne_store::{AgentSession, SessionFilter, Store, TaskFilter};

use crate::launcher::Launcher;
use crate::sleep::SleepInhibitor;
use crate::timeouts::Timeouts;

use coalesce::Coalesced;
use quiet::Quiet;

/// Events that wake the scheduler for a scoped reconciliation.
pub enum SchedEvent {
    /// A task's status, reviews or deps changed.
    TaskChanged(String),
    /// A goal was created, finalized or cancelled.
    GoalChanged(String),
    /// An agent session reported activity.
    SessionEvent(String),
    /// Test support: answered the moment this event is dequeued, which is
    /// only once every event sent before it has been reconciled to
    /// completion — the loop below awaits each one fully before it asks the
    /// channel for the next, and takes every session wake still folded into
    /// an open window before it answers. A test racing a fixed sleep against
    /// the scheduler's own pace can wait on this instead: sent right after
    /// the event under test, its answer proves that one already ran.
    Flush(tokio::sync::oneshot::Sender<()>),
}

/// Test support: block until every event sent on `tx` before this call has
/// been fully reconciled — see [`SchedEvent::Flush`].
pub async fn flush_for_test(tx: &mpsc::UnboundedSender<SchedEvent>) {
    let (done, wait) = tokio::sync::oneshot::channel();
    tx.send(SchedEvent::Flush(done))
        .expect("the scheduler is still running");
    wait.await.expect("the scheduler answered the flush");
}

/// How long a session that is starting is given to get an agent process
/// before the liveness sweep concludes there is none.
///
/// A row is put into `starting` before its agent exists: the launcher writes
/// it and then spawns, and a resume from the API takes the old agent down
/// before the new one is up. A sweep landing in that window would retire a
/// session on its way up and raise `disconnected` on it — an alarm that
/// clears itself the moment the agent's first event arrives, having flashed
/// on the strip and over SSE in the meantime. Long enough for a launch to
/// come up, short enough that one which never will is still noticed within a
/// tick or two.
pub const START_GRACE_SECS: i64 = 30;
/// Spawn attempts before the daemon stops trying: per task, after which it is
/// failed, and per goal, after which its orchestrator is left alone.
pub const SPAWN_RETRY_BUDGET: u32 = 3;
/// How long a session may report nothing before it is nudged: told to get on
/// with the work in front of it.
///
/// Long enough that a slow start or a long tool call is not read as a stuck
/// one — three minutes. What makes the short clock safe is that the nudge is
/// not sent on it alone: an agent in the middle of a turn is left exactly
/// where it is (see [`quiet`]). So the only agent this reaches is one idle
/// with the work still in front of it, and that is not a `cargo build`.
pub const QUIET_NUDGE_SECS: i64 = 180;
/// And before the silence is raised for the user, whom the nudge did not
/// spare.
///
/// Ten minutes. Unlike the nudge this one is spent whatever the agent is
/// doing, so it has to clear the longest wait an agent is *told* to take: the
/// landing briefing sends an author to `sleep` at most five minutes at a time
/// while it waits for a pull request to be merged, and this is twice that. A flag
/// raised over a tool call that ran longer still is not the end of anything —
/// the agent's next event takes it down again.
pub const QUIET_FLAG_SECS: i64 = 600;
/// And before the agent is killed and put back on its feet: the flag
/// plus enough of a wait for a person to have looked at it first — twenty
/// minutes of one, after which nobody is coming and the agent has been silent
/// for half an hour.
pub const QUIET_RELAUNCH_SECS: i64 = 1_800;
/// The watchdog is one timeline, and the order of its thresholds is what
/// makes it one: nudged before the user is told, told before an agent is
/// killed under them. The flag has a floor of its own — it is spent whatever
/// the agent is doing, so it has to clear the longest wait an agent is *told*
/// to take, which is the five-minute `sleep` the landing briefing sends an
/// author to while a pull request waits to be merged. Checked here rather
/// than in a test, so that a number edited into the wrong order does not
/// build.
const _: () = assert!(
    QUIET_NUDGE_SECS < QUIET_FLAG_SECS,
    "an agent is nudged before the user is told about it"
);
const _: () = assert!(
    QUIET_FLAG_SECS < QUIET_RELAUNCH_SECS,
    "and told before the agent is killed and put back on its feet"
);
const _: () = assert!(
    QUIET_FLAG_SECS >= 600,
    "the flag stays clear of a five-minute sleep, with margin"
);

pub struct Scheduler {
    store: Store,
    launcher: Arc<Launcher>,
    /// Spawn failures per task, and per goal whose orchestrator will not
    /// start, by the id of whichever it is — the two never collide, and what
    /// a failure means is the same either way (in-memory: resets on daemon
    /// restart, which is fine — a restart is exactly when a retry is
    /// warranted).
    spawn_failures: HashMap<String, u32>,
    /// The launch each seat's last death on arrival was already counted for,
    /// keyed like the map above and holding the `launch_id` that died: an
    /// agent that came up and was never heard from is counted once, not once
    /// per pass over the row it left, and a session put back on its feet
    /// under its own id is counted again for the run that died next. Cleared,
    /// like the count it guards, when a launch is heard from.
    dead_launch: HashMap<String, String>,
    /// The launch each seat last gave its budget back for, keyed like the map
    /// above: a launch that was heard from gives the attempts back once, not
    /// on every pass. A task's agents share one budget, so an author that
    /// keeps reporting would otherwise refund, pass after pass, every attempt
    /// its reviewer failed, and a reviewer that cannot be started would be
    /// tried for ever without the task ever failing.
    refunded_launch: HashMap<String, String>,
    /// What the quiet-clock watchdog has done about each session it has had
    /// to act on, by session id (in memory like the map above).
    quiet: HashMap<String, Quiet>,
    /// What each goal's orchestrator was last told its tasks needed, by goal
    /// id, so a situation that has not changed is not sent to it every tick.
    /// In memory like the maps below: a daemon that restarts over a failed
    /// task tells the orchestrator once more, which is the right way round —
    /// a wake too many costs a turn, one too few leaves a goal with nobody
    /// deciding.
    goal_told: HashMap<String, String>,
    /// Tasks whose author has been handed the landing briefing, by task id.
    /// In memory like the maps above: what it prevents is briefing the same
    /// approved task twice while the daemon that approved it is running, and
    /// a daemon that restarts over an approved task wants to say it again.
    landing_briefed: HashSet<String>,
    /// Reviewers already asked to pick a contested task's winner, by (task,
    /// reviewer). In memory like `landing_briefed`, and for the same reason:
    /// a daemon that restarts over an open pick asks once more.
    pick_briefed: HashSet<(String, String)>,
    /// Reviews a live reviewer has already been briefed on, by (reviewer,
    /// review request). A reviewer whose agent survived the last one is
    /// handed the next one's briefing the moment it owes it — once, and in
    /// memory like the sets above: a daemon that restarts over an open review
    /// says it once more.
    ///
    /// The request is the row addressed to that reviewer, not the newest on
    /// the task: an announcement writes one row per reviewer, so the newest
    /// moves while it runs. A briefing sent before that row was written is
    /// stamped under the author it is for, and the row that lands next takes
    /// the stamp over: see `adopt_briefing_sent_before_the_request`.
    review_briefed: HashSet<(String, String)>,
    /// Held while any session is live, so the machine does not idle-sleep
    /// out from under a working agent.
    sleep: SleepInhibitor,
    /// Whether taking that inhibition is wanted at all (`prevent_sleep`).
    prevent_sleep: bool,
}

/// Start the scheduler; returns the event sender for the HTTP layer.
pub fn start(
    store: Store,
    launcher: Arc<Launcher>,
    prevent_sleep: bool,
    timeouts: Timeouts,
) -> mpsc::UnboundedSender<SchedEvent> {
    let (tx, mut rx) = mpsc::unbounded_channel();
    // The ACP runtime reports agent events itself; give it the waker the
    // HTTP handlers poke after a write.
    launcher.acp.connect_scheduler(tx.clone());
    let mut scheduler = Scheduler {
        store,
        launcher,
        spawn_failures: HashMap::new(),
        dead_launch: HashMap::new(),
        refunded_launch: HashMap::new(),
        quiet: HashMap::new(),
        goal_told: HashMap::new(),
        landing_briefed: HashSet::new(),
        pick_briefed: HashSet::new(),
        review_briefed: HashSet::new(),
        sleep: SleepInhibitor::new(),
        prevent_sleep,
    };
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(timeouts.full_reconcile);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        // An agent reports far more often than its task changes, so its wakes
        // are folded per session rather than acted on one by one.
        let mut wakes = Coalesced::new(timeouts.session_wake);
        loop {
            tokio::select! {
                event = rx.recv() => match event {
                    Some(SchedEvent::TaskChanged(id)) => scheduler.reconcile(Target::Task(&id)).await,
                    Some(SchedEvent::GoalChanged(id)) => scheduler.reconcile(Target::Goal(&id)).await,
                    Some(SchedEvent::SessionEvent(id)) => {
                        if wakes.wake(&id, tokio::time::Instant::now()) {
                            scheduler.reconcile_session(&id).await;
                            wakes.reconciled(&id, tokio::time::Instant::now());
                        }
                    }
                    Some(SchedEvent::Flush(done)) => {
                        for id in wakes.take_all() {
                            scheduler.reconcile_session(&id).await;
                            wakes.reconciled(&id, tokio::time::Instant::now());
                        }
                        let _ = done.send(());
                    }
                    None => break, // daemon shutting down
                },
                _ = window_end(wakes.next_window_end()) => {
                    for id in wakes.due(tokio::time::Instant::now()) {
                        scheduler.reconcile_session(&id).await;
                        wakes.reconciled(&id, tokio::time::Instant::now());
                    }
                }
                _ = tick.tick() => scheduler.reconcile_all().await,
            }
        }
    });
    tx
}

/// Wait for the earliest open window to end, or for ever where no session is
/// inside one: a loop with nothing folded waits only on its channel and its
/// tick.
async fn window_end(at: Option<tokio::time::Instant>) {
    match at {
        Some(at) => tokio::time::sleep_until(at).await,
        None => std::future::pending().await,
    }
}

/// What one pass of reconciliation is about.
///
/// The two are one pass with a different subject — read the entity, work out
/// what it wants, act — so they share the one entry point rather than a
/// logging wrapper each.
#[derive(Clone, Copy)]
enum Target<'a> {
    Goal(&'a str),
    Task(&'a str),
}

impl Scheduler {
    /// Hand `text` to a session's agent as a `session/prompt` (021): sent at
    /// once when the agent is between turns, and queued in order behind
    /// whichever one runs. Answers whether the runtime took it.
    ///
    /// One it refused — no agent runs for the session — gives a nudge spent
    /// on it back, so the next pass over this session sends it again.
    fn hand_prompt(&mut self, session: &AgentSession, text: String) -> bool {
        match self.launcher.acp.send_prompt(&session.id, text) {
            Ok(()) => true,
            Err(e) => {
                warn!(session = %session.id, error = %format!("{e:#}"), "handing the agent a prompt failed");
                if let Some(done) = self.quiet.get_mut(&session.id) {
                    done.nudged = false;
                }
                false
            }
        }
    }

    /// One reconciliation, with nowhere to hand an error: the event loop and
    /// the tick are the only callers, and what they do about a failure is say
    /// so — and, for a task, count it against the spawn-retry budget, since a
    /// task whose agent will not start is what that budget is for. A goal
    /// spends the same budget on its orchestrator, but counts it where the
    /// spawn fails rather than here: a goal reconciliation has other ways to
    /// fail — a store that would not answer, a nudge that went nowhere — and
    /// none of them says anything about whether an orchestrator can be
    /// started.
    async fn reconcile(&mut self, target: Target<'_>) {
        let failed = match target {
            Target::Goal(id) => self.reconcile_goal(id).await.err(),
            Target::Task(id) => self.reconcile_task(id).await.err(),
        };
        let Some(e) = failed else { return };
        match target {
            Target::Goal(id) => {
                warn!(goal = %id, error = %format!("{e:#}"), "goal reconciliation failed")
            }
            Target::Task(id) => {
                warn!(task = %id, error = %format!("{e:#}"), "task reconciliation failed");
                // A store that would not answer says nothing about whether an
                // agent can be started, so it is waited out rather than
                // counted. Under load the write pool hands out a timeout
                // instead of a connection, and every task's reconciliation
                // fails together for as long as that lasts: counted as spawn
                // attempts, three ticks of it fail a task whose agent was
                // never asked for.
                if !unanswered(&e) {
                    self.record_spawn_failure(id, "the agent could not be started")
                        .await;
                }
            }
        }
    }

    /// Kill the live sessions a filter names — all of them, or only those of
    /// one seat.
    ///
    /// Failures are logged and otherwise swallowed: reconciliation carries on
    /// for the rest of the sessions, and the next tick asks again — but a
    /// session that will not die has to be visible, or the only symptom is a
    /// machine that never sleeps.
    async fn kill_sessions(&self, filter: SessionFilter, seat: Option<Seat>, why: &str) {
        let sessions = match self.store.list_sessions(filter).await {
            Ok(sessions) => sessions,
            Err(e) => {
                warn!(error = %e, why, "listing the sessions to kill failed");
                return;
            }
        };
        for session in sessions {
            if seat.is_some_and(|wanted| session.seat() != wanted) {
                continue;
            }
            info!(session = %session.id, seat = %session.seat, why, "killing session");
            if let Err(e) = self.launcher.kill_session(&session.id).await {
                warn!(session = %session.id, error = %e, "killing the session failed");
            }
        }
    }

    async fn reconcile_all(&mut self) {
        // The sweep is the one place that sees every session, so its count
        // decides whether the machine stays awake. `None` means the store did
        // not answer: leave the inhibition as it is rather than guess.
        if let Some(live) = self.liveness_sweep().await {
            self.sleep.set_active(self.prevent_sleep && live > 0);
        }
        self.reconcile_entities().await;
        self.stale_attention_sweep().await;
    }

    /// Every goal, and every task of the active ones, reconciled in turn.
    async fn reconcile_entities(&mut self) {
        let goals = match self.store.list_goals(&[]).await {
            Ok(goals) => goals,
            Err(e) => {
                warn!(error = %e, "reconcile: listing goals failed");
                return;
            }
        };
        for goal in goals {
            self.reconcile(Target::Goal(&goal.id)).await;
            if goal.status() == GoalStatus::Active {
                let tasks = match self
                    .store
                    .list_tasks(TaskFilter {
                        goal_id: Some(goal.id.clone()),
                        status: None,
                    })
                    .await
                {
                    Ok(tasks) => tasks,
                    Err(e) => {
                        warn!(goal = %goal.id, error = %e, "reconcile: listing tasks failed");
                        continue;
                    }
                };
                // Tasks with live sessions are reconciled even when
                // terminal, so a crash between merge/cancel and cleanup
                // still converges on the next tick.
                let live_task_ids: std::collections::HashSet<String> = self
                    .store
                    .list_sessions(SessionFilter {
                        goal_id: Some(goal.id.clone()),
                        live_only: true,
                        ..Default::default()
                    })
                    .await
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|s| s.task_id)
                    .collect();
                for task in tasks {
                    if !task.status().is_terminal() || live_task_ids.contains(&task.id) {
                        self.reconcile(Target::Task(&task.id)).await;
                    }
                }
            }
        }
    }

    /// One pass over what the session's own wakes are about: its task, or its
    /// goal where it belongs to no task.
    ///
    /// The line it logs is how often that pass runs, which is what the
    /// coalescing of the wakes (`coalesce`) is measured by: an agent's events
    /// are the busiest thing the daemon hears, and a pass per event is what
    /// read the store out of connections.
    async fn reconcile_session(&mut self, session_id: &str) {
        debug!(session = %session_id, "reconciling what a session's wake is about");
        let Ok(session) = self.store.get_session(session_id).await else {
            return;
        };
        match &session.task_id {
            Some(task) => self.reconcile(Target::Task(task)).await,
            None => self.reconcile(Target::Goal(&session.goal_id)).await,
        }
    }
}

impl Scheduler {
    /// Count this seat's last launch if it died on arrival, and say whether
    /// the budget for starting another has run out.
    ///
    /// An agent that comes up and dies without a word is not one more launch
    /// away from working, and the seat it left empty is one the daemon wants
    /// filled: the two together are a loop that starts an agent every tick for
    /// as long as the goal lives, and the user is never told, because every
    /// alarm the sweep raises is cleared by the replacement that goes on to
    /// die the same way. So the deaths are counted against the same budget a
    /// spawn that never got off the ground spends, one per launch, and the
    /// caller says what running out of it means for the seat.
    /// `seat` names the seat the launch was for — the goal for an
    /// orchestrator, the task for its author, the session row for one of
    /// several reviewers — and `budget` the count it spends, which for every
    /// agent of a task is the task's own.
    pub(super) fn spent_on_a_dead_launch(
        &mut self,
        seat: &str,
        budget: &str,
        last: &AgentSession,
    ) -> bool {
        // Once per launch, and per launch rather than per row: an author and
        // a reviewer are put back on their feet under the id they already
        // have, so the row says nothing about which run of it died. Every
        // pass over the same dead launch would otherwise spend the budget
        // again, and no relaunch of a session would ever spend it once. The
        // same holds for giving it back: a launch heard from refunds once.
        let launch = last.launch_id.clone().unwrap_or_else(|| last.id.clone());
        if last.heard_from() {
            self.dead_launch.remove(seat);
            if self.refunded_launch.get(seat) != Some(&launch) {
                self.refunded_launch.insert(seat.to_string(), launch);
                self.spawn_failures.remove(budget);
            }
            return false;
        }
        if !last.died_on_arrival() {
            return false;
        }
        if self.dead_launch.get(seat) == Some(&launch) {
            return false;
        }
        self.dead_launch.insert(seat.to_string(), launch);
        true
    }
}

/// Whether an error is the store failing to answer rather than the work
/// failing: a pool that timed out, a statement that could not run.
///
/// Every other [`ariadne_store::StoreError`] is about the data — a row that
/// is not there, a transition the state machine refuses — and is a real
/// answer to the question it was read for.
fn unanswered(error: &anyhow::Error) -> bool {
    matches!(
        error.downcast_ref::<ariadne_store::StoreError>(),
        Some(ariadne_store::StoreError::Db(_))
    )
}

#[cfg(test)]
mod tests {
    use ariadne_core::{TaskStatus, TransitionError};
    use ariadne_store::StoreError;

    use super::unanswered;

    /// The spawn-retry budget is for a task whose agent will not start. A
    /// store that would not answer is the daemon failing to read, not the
    /// agent failing to run, and three ticks of it must not fail a task —
    /// which is how a pool timeout under load failed two tasks whose authors
    /// had already committed.
    #[test]
    fn only_a_store_that_would_not_answer_is_waited_out() {
        let timed_out = anyhow::Error::from(StoreError::Db(sqlx::Error::PoolTimedOut));
        assert!(unanswered(&timed_out), "a pool timeout is not an answer");

        for real in [
            StoreError::NotFound {
                entity: "task",
                id: "gone".into(),
            },
            StoreError::Conflict("already landed".into()),
            StoreError::Invalid("no author".into()),
            StoreError::Transition(TransitionError::IllegalTransition {
                from: TaskStatus::Failed,
                to: TaskStatus::InProgress,
            }),
        ] {
            let error = anyhow::Error::from(real);
            assert!(
                !unanswered(&error),
                "the store answered, and the answer counts: {error}"
            );
        }

        let other = anyhow::anyhow!("git would not spawn");
        assert!(!unanswered(&other), "not a store error at all: {other}");
    }
}
