//! Scheduler: an event-driven reconciliation loop.
//!
//! HTTP handlers send [`SchedEvent`]s after writes; a periodic tick reconciles
//! everything so crashes, missed events and dead tmux sessions self-heal.
//! Every rule is idempotent — read state, compare desired, act — which is why
//! a pass that arrives late does what the state says now rather than replaying
//! what it missed.
//!
//! The rules are a module each: `goals` and `tasks` for what the two entities
//! want, `sweeps` for the two passes that see every session whatever it
//! belongs to, `quiet` for the watchdog over an agent that stopped reporting,
//! `compaction` for the compaction every session is owed at a hand-off, and
//! `delivery` for typing into a pane off the loop.

mod compaction;
mod delivery;
mod goals;
mod quiet;
mod sweeps;
mod tasks;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use tracing::{info, warn};

use ariadne_core::{GoalStatus, Role};
use ariadne_store::{SessionFilter, Store, TaskFilter};

use crate::launcher::Launcher;
use crate::sleep::SleepInhibitor;

use compaction::{Compacting, CompactionReport};
use delivery::DeliveryReport;
use quiet::Quiet;

/// Events that wake the scheduler for a scoped reconciliation.
#[derive(Debug, Clone)]
pub enum SchedEvent {
    /// A task's status, reviews or deps changed.
    TaskChanged(String),
    /// A goal was created, finalized or cancelled.
    GoalChanged(String),
    /// An agent session reported activity.
    SessionEvent(String),
}

/// How often the full reconciliation tick runs.
///
/// Not how long a hand-off waits: everything a write can report — a task
/// moving on, a plan finalized, a verdict, an agent's own hook — arrives as a
/// [`SchedEvent`] and is acted on as it lands, in milliseconds. What is left
/// for the tick is the state nothing reports, and
/// the one that costs an agent its turn is a pane that went away without
/// saying so: a killed tmux, a machine that dropped the session. This period
/// is the ceiling on how long the successor of such an agent sits unstarted,
/// so it is short — the pass costs one `tmux display-message` per live
/// session and a handful of indexed reads, which is cheap enough to make five
/// seconds the wait rather than fifteen.
pub const TICK_SECS: u64 = 5;
/// How long a session that is starting is given to get a pane before the
/// liveness sweep concludes there is none.
///
/// A row is put into `starting` before tmux has anything: the launcher writes
/// it and then spawns, and a resume from the API kills the old pane before the
/// new one exists. A sweep landing in that window would retire a session on
/// its way up and raise `disconnected` on it — an alarm that clears itself the
/// moment the agent's first hook arrives, having flashed on the strip and over
/// SSE in the meantime. Long enough for a launch to reach tmux, short enough
/// that one which never will is still noticed within a tick or two.
pub const START_GRACE_SECS: i64 = 30;
/// Spawn attempts before the daemon stops trying: per task, after which it is
/// failed, and per goal, after which its planner is left alone.
pub const SPAWN_RETRY_BUDGET: u32 = 3;
/// How long a session may report nothing before it is nudged: told to get on
/// with the work in front of it, or given the Enter its composer is waiting
/// for.
///
/// Long enough that a slow start or a long tool call is not read as a stuck
/// one — three minutes, where it was five. What makes the shorter clock safe
/// is that the nudge is not sent on it alone: an agent in the middle of a
/// turn has an empty composer, and a pane whose composer is empty is left
/// exactly where it is (see [`quiet`]). So the only pane this reaches sooner
/// is one that is either idle with the work still in front of it or holding
/// an instruction nobody submitted, and neither is a `cargo build`.
pub const QUIET_NUDGE_SECS: i64 = 180;
/// And before the silence is raised for the user, whom the nudge did not
/// spare.
///
/// Ten minutes. Unlike the nudge this one is spent whatever the pane says, so
/// it has to clear the longest wait an agent is *told* to take: the landing
/// briefing sends an engineer to `sleep` at most five minutes at a time while
/// it waits for a pull request to be merged, and this is twice that. A flag
/// raised over a tool call that ran longer still is not the end of anything —
/// the agent's next event takes it down again.
pub const QUIET_FLAG_SECS: i64 = 600;
/// And before the pane is killed and the agent put back on its feet: the flag
/// plus enough of a wait for a person to have looked at it first — twenty
/// minutes of one, after which nobody is coming and the agent has been silent
/// for half an hour.
pub const QUIET_RELAUNCH_SECS: i64 = 1_800;
/// How long a compaction may stay owed without ever being started before the
/// debt is written off: a session that never comes back to its prompt — a
/// turn that runs on, a dialog nobody answers — is not held for it any
/// longer than this, and neither is whatever waits on that session. Ten
/// minutes, the same wait the user is told about a silent agent after.
pub const COMPACTION_OWED_FOR_SECS: i64 = 600;

/// The watchdog is one timeline, and the order of its thresholds is what
/// makes it one: nudged before the user is told, told before a pane is killed
/// under them. The flag has a floor of its own — it is spent whatever the
/// pane is doing, so it has to clear the longest wait an agent is *told* to
/// take, which is the five-minute `sleep` the landing briefing sends an
/// engineer to while a pull request waits to be merged. Checked here rather
/// than in a test, so that a number edited into the wrong order does not
/// build.
const _: () = assert!(
    QUIET_NUDGE_SECS < QUIET_FLAG_SECS,
    "an agent is nudged before the user is told about it"
);
const _: () = assert!(
    QUIET_FLAG_SECS < QUIET_RELAUNCH_SECS,
    "and told before its pane is killed and the agent put back on its feet"
);
const _: () = assert!(
    QUIET_FLAG_SECS >= 600,
    "the flag stays clear of a five-minute sleep, with margin"
);

pub struct Scheduler {
    store: Store,
    launcher: Arc<Launcher>,
    /// Spawn failures per task, and per goal whose planner will not start, by
    /// the id of whichever it is — the two never collide, and what a failure
    /// means is the same either way (in-memory: resets on daemon restart,
    /// which is fine — a restart is exactly when a retry is warranted).
    spawn_failures: HashMap<String, u32>,
    /// What the quiet-clock watchdog has done about each session it has had
    /// to act on, by session id (in memory like the map above).
    quiet: HashMap<String, Quiet>,
    /// Tasks whose engineer has been handed the landing briefing, by task id.
    /// In memory like the maps above: what it prevents is briefing the same
    /// approved task twice while the daemon that approved it is running, and
    /// a daemon that restarts over an approved task wants to say it again.
    landing_briefed: HashSet<String>,
    /// Sessions with a delivery going into their pane right now, by session
    /// id: two pastes into one composer at once would interleave into
    /// something neither of them said.
    typing: HashSet<String>,
    /// Where a delivery that ran off the loop reports back to.
    reports: mpsc::UnboundedSender<DeliveryReport>,
    /// The situation — status and round — each session was last found to owe
    /// a compaction for, by session id, so that the passes after the one that
    /// noticed a hand-off do not owe it again for the same one. In memory
    /// like the maps above: a daemon that restarts over a hand-off owes the
    /// compaction once more, and one compaction too many costs a summary
    /// where one too few costs every resume after it.
    compaction_owed_for: HashMap<String, (String, i64)>,
    /// Sessions with a compaction going on in their pane right now, by
    /// session id: typed and confirmed, and not yet reported done. Nothing
    /// else is typed into such a pane, and nothing kills it, until the CLI
    /// says the compaction is over or the wait for that runs out.
    compacting: HashMap<String, Compacting>,
    /// Passes at typing a compaction that tmux refused, by session id, so a
    /// pane that will not take it is not asked every tick for ever.
    compaction_refused: HashMap<String, u32>,
    /// Where a compaction typed off the loop reports back to.
    compaction_reports: mpsc::UnboundedSender<CompactionReport>,
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
) -> mpsc::UnboundedSender<SchedEvent> {
    let (tx, mut rx) = mpsc::unbounded_channel();
    // Deliveries report on a channel of their own rather than on the event
    // one, so the loop still ends when the daemon drops the sender it was
    // given: the scheduler holds this one for as long as it lives.
    let (reports, mut settled) = mpsc::unbounded_channel();
    let (compaction_reports, mut compacted) = mpsc::unbounded_channel();
    let mut scheduler = Scheduler {
        store,
        launcher,
        spawn_failures: HashMap::new(),
        quiet: HashMap::new(),
        landing_briefed: HashSet::new(),
        typing: HashSet::new(),
        reports,
        compaction_owed_for: HashMap::new(),
        compacting: HashMap::new(),
        compaction_refused: HashMap::new(),
        compaction_reports,
        sleep: SleepInhibitor::new(),
        prevent_sleep,
    };
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(TICK_SECS));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                event = rx.recv() => match event {
                    Some(SchedEvent::TaskChanged(id)) => scheduler.reconcile(Target::Task(&id)).await,
                    Some(SchedEvent::GoalChanged(id)) => scheduler.reconcile(Target::Goal(&id)).await,
                    Some(SchedEvent::SessionEvent(id)) => scheduler.reconcile_session(&id).await,
                    None => break, // daemon shutting down
                },
                Some(report) = settled.recv() => scheduler.delivery_settled(report).await,
                Some(report) = compacted.recv() => scheduler.compaction_settled(report).await,
                _ = tick.tick() => scheduler.reconcile_all().await,
            }
        }
    });
    tx
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
    /// One reconciliation, with nowhere to hand an error: the event loop and
    /// the tick are the only callers, and what they do about a failure is say
    /// so — and, for a task, count it against the spawn-retry budget, since a
    /// task whose agent will not start is what that budget is for. A goal
    /// spends the same budget on its planner, but counts it where the spawn
    /// fails rather than here: a goal reconciliation has other ways to fail —
    /// a store that would not answer, a nudge that went nowhere — and none of
    /// them says anything about whether a planner can be started.
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
                self.record_spawn_failure(id).await;
            }
        }
    }

    /// Kill the live sessions a filter names — all of them, or only those of
    /// one role.
    ///
    /// Failures are logged and otherwise swallowed: reconciliation carries on
    /// for the rest of the sessions, and the next tick asks again — but a
    /// session that will not die has to be visible, or the only symptom is a
    /// machine that never sleeps.
    async fn kill_sessions(&self, filter: SessionFilter, role: Option<Role>, why: &str) {
        let sessions = match self.store.list_sessions(filter).await {
            Ok(sessions) => sessions,
            Err(e) => {
                warn!(error = %e, why, "listing the sessions to kill failed");
                return;
            }
        };
        for session in sessions {
            if role.is_some_and(|wanted| session.role() != wanted) {
                continue;
            }
            info!(session = %session.id, role = %session.role, why, "killing session");
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
        // After the goals and tasks, not before: reconciling a hand-off is
        // what writes the compaction a session owes, and the sweeps below
        // read that debt — the attention sweep to leave a prompt standing
        // on a pane the compaction will be typed into, the compaction sweep
        // to know the pane is not free for it. A daemon starting cold over a
        // hand-off nothing has reconciled yet would otherwise drop the
        // prompt as stale in this very pass and type into the dialog on the
        // next.
        self.stale_attention_sweep().await;
        self.compaction_sweep().await;
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

    async fn reconcile_session(&mut self, session_id: &str) {
        let Ok(session) = self.store.get_session(session_id).await else {
            return;
        };
        // What the event may have changed about the session's compaction —
        // a turn that ended, a compaction reported done — before the work
        // that waits on it is reconciled.
        self.settle_compaction(&session).await;
        match &session.task_id {
            Some(task) => self.reconcile(Target::Task(task)).await,
            None => self.reconcile(Target::Goal(&session.goal_id)).await,
        }
    }
}
