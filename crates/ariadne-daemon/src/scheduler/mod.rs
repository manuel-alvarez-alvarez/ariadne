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
//! and `delivery` and `wake` for taking a message to whoever it addresses.

mod delivery;
mod goals;
mod quiet;
mod sweeps;
mod tasks;
mod wake;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use tokio::sync::mpsc;
use tracing::{info, warn};

use ariadne_core::{GoalStatus, Role};
use ariadne_store::{SessionFilter, Store, TaskFilter};

use crate::launcher::Launcher;
use crate::sleep::SleepInhibitor;

use delivery::DeliveryReport;
use quiet::Quiet;

/// Events that wake the scheduler for a scoped reconciliation.
#[derive(Debug, Clone)]
pub enum SchedEvent {
    /// A task's status, reviews or deps changed.
    TaskChanged(String),
    /// A goal was created, submitted for approval, approved or cancelled.
    GoalChanged(String),
    /// An agent session reported activity.
    SessionEvent(String),
    /// A message was posted into a goal or task conversation, by id: whoever
    /// it addresses is woken with it.
    MessagePosted(String),
}

/// How often the full reconciliation tick runs.
const TICK_SECS: u64 = 15;
/// Spawn attempts per task before it is failed.
const SPAWN_RETRY_BUDGET: u32 = 3;
/// Passes one addressed message is worth before the user is told it never
/// arrived. A tmux that would not take it says nothing about whether the
/// agent is there to hear it, so the message is not spent on the attempt —
/// but neither is it retried for ever, or nobody would ever hear that it is
/// stuck.
const DELIVERY_ATTEMPTS: u32 = 3;
/// How long a session may report nothing before it is nudged: told to get on
/// with the work in front of it, or given the Enter its composer is waiting
/// for. Long enough that a slow start or a long tool call is not read as a
/// stuck one.
const QUIET_NUDGE_SECS: i64 = 300;
/// And before the silence is raised for the user, whom the nudge did not
/// spare.
const QUIET_FLAG_SECS: i64 = 900;
/// And before the pane is killed and the agent put back on its feet: the flag
/// plus enough of a wait for a person to have looked at it first.
const QUIET_RELAUNCH_SECS: i64 = 2_700;

pub struct Scheduler {
    store: Store,
    launcher: Arc<Launcher>,
    /// Spawn failures per task (in-memory: resets on daemon restart, which is
    /// fine — a restart is exactly when a retry is warranted).
    spawn_failures: HashMap<String, u32>,
    /// What the quiet-clock watchdog has done about each session it has had
    /// to act on, by session id (in memory like the map above).
    quiet: HashMap<String, Quiet>,
    /// Tasks whose engineer has been handed the landing briefing, by task id.
    /// In memory like the maps above: what it prevents is briefing the same
    /// approved task twice while the daemon that approved it is running, and
    /// a daemon that restarts over an approved task wants to say it again.
    landing_briefed: HashSet<String>,
    /// Messages the addressee is *confirmed* to have, by message id. In
    /// memory like the two maps above, and for the same reason: what it
    /// prevents is typing one message into a pane twice, which is only ever
    /// at stake while the daemon that saw it posted is still running.
    delivered: HashSet<String>,
    /// What every message not yet confirmed has spent trying, by message id:
    /// a delivery tmux would not take is tried again on later ticks up to
    /// [`DELIVERY_ATTEMPTS`], and one that has spent them all is given up on
    /// — the user has been told, and nothing is typed for it again.
    attempts: HashMap<String, u32>,
    /// Sessions with a delivery going into their pane right now, by session
    /// id: two pastes into one composer at once would interleave into
    /// something neither of them said.
    typing: HashSet<String>,
    /// When each session was last confirmed to have been handed something,
    /// by session id. A delivery is a nudge, and a better one — the agent has
    /// just been told what to do — so the watchdog's clock counts from here
    /// as well as from what the agent itself last reported.
    delivered_at: HashMap<String, chrono::DateTime<chrono::Utc>>,
    /// Where a delivery that ran off the loop reports back to.
    reports: mpsc::UnboundedSender<DeliveryReport>,
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
    let mut scheduler = Scheduler {
        store,
        launcher,
        spawn_failures: HashMap::new(),
        quiet: HashMap::new(),
        landing_briefed: HashSet::new(),
        delivered: HashSet::new(),
        attempts: HashMap::new(),
        typing: HashSet::new(),
        delivered_at: HashMap::new(),
        reports,
        sleep: SleepInhibitor::new(),
        prevent_sleep,
    };
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(TICK_SECS));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                event = rx.recv() => match event {
                    Some(SchedEvent::TaskChanged(id)) => scheduler.reconcile(Target::Task(&id)).await,
                    Some(SchedEvent::GoalChanged(id)) => scheduler.reconcile(Target::Goal(&id)).await,
                    Some(SchedEvent::SessionEvent(id)) => scheduler.reconcile_session(&id).await,
                    Some(SchedEvent::MessagePosted(id)) => scheduler.deliver_message(&id).await,
                    None => break, // daemon shutting down
                },
                Some(report) = settled.recv() => scheduler.delivery_settled(report).await,
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
    /// task whose agent will not start is what that budget is for.
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
        self.stale_attention_sweep().await;
        self.retry_deliveries().await;
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
        match &session.task_id {
            Some(task) => self.reconcile(Target::Task(task)).await,
            None => self.reconcile(Target::Goal(&session.goal_id)).await,
        }
    }
}
