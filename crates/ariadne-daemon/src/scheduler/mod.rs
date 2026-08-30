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
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::time::Instant;
use tracing::{info, warn};

use ariadne_core::{GoalStatus, Role};
use ariadne_store::{SessionFilter, Store, TaskFilter};

use crate::launcher::Launcher;
use crate::sleep::SleepInhibitor;

use delivery::{DeliveryReport, Owed};
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
    /// A message was posted into a goal or task conversation, by id: whoever
    /// it addresses is woken with it.
    MessagePosted(String),
}

/// How often the full reconciliation tick runs.
///
/// Not how long a hand-off waits: everything a write can report — a task
/// moving on, a plan finalized, a verdict, a message posted, an agent's own
/// hook — arrives as a [`SchedEvent`] and is acted on as it lands, in
/// milliseconds. What is left for the tick is the state nothing reports, and
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
/// Passes one addressed message is worth before the user is told it never
/// arrived. A tmux that would not take it says nothing about whether the
/// agent is there to hear it, so the message is not spent on the attempt —
/// but neither is it retried for ever, or nobody would ever hear that it is
/// stuck.
///
/// Five of them, because they are no longer one a tick: they are spread over
/// the widening wait below, so the first few are spent in seconds and all
/// five still cover about half a minute of a tmux that will not answer —
/// which is what three of them covered when each one waited fifteen seconds.
pub const DELIVERY_ATTEMPTS: u32 = 5;
/// How long a message waits for the pass after one that changed nothing, and
/// what that wait grows from.
///
/// A pass that found nobody to type into — an addressee whose pane is not up
/// yet, a reviewer whose round has not started — is a pass that tried
/// nothing, and it used to be repeated only when the tick came round. A
/// second is long enough to be no busier than the work it is waiting on and
/// short enough that a message posted to an agent that is a moment from
/// having a pane reaches it while it still means something.
const RETRY_AFTER: Duration = Duration::from_secs(1);
/// The longest a message waits between passes at an addressee that has a
/// session and would not take it: the wait doubles after every one of those
/// up to here, so the passes a message is worth are spread over about half a
/// minute of a tmux that will not answer instead of being spent in seconds.
/// Fifteen seconds is what *every* retry waited before, which makes this the
/// worst case rather than a new one.
const RETRY_AT_MOST: Duration = Duration::from_secs(15);
/// And the longest between passes that found nobody to type into at all — a
/// reviewer whose round has not started, an engineer whose task has not
/// begun. Such a pass reads the store and touches no pane, so it can be made
/// as often as the tick that used to make it, and the message goes to the
/// session that turns up within a tick of it existing rather than within the
/// wait above.
const RETRY_FOR_NOBODY_AT_MOST: Duration = Duration::from_secs(TICK_SECS);
/// And what a message whose addressee is mid-paste waits: the delivery in
/// front of it settles in about half a second, and the composer is free the
/// moment it does. Nothing was tried, so nothing was spent — this is only how
/// long before asking again.
const RETRY_WHILE_TYPING: Duration = Duration::from_millis(250);
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

/// And the retry waits: shortest for a pane that is only mid-paste, longest
/// at the bound, which is itself no shorter than the tick that used to make
/// every retry.
const _: () = assert!(
    RETRY_WHILE_TYPING.as_millis() < RETRY_AFTER.as_millis()
        && RETRY_AFTER.as_secs() <= RETRY_FOR_NOBODY_AT_MOST.as_secs()
        && RETRY_FOR_NOBODY_AT_MOST.as_secs() <= RETRY_AT_MOST.as_secs(),
    "a busy pane is asked again soonest, and no wait outlives the bound"
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
    /// Messages the addressee is *confirmed* to have, by message id. In
    /// memory like the two maps above, and for the same reason: what it
    /// prevents is typing one message into a pane twice, which is only ever
    /// at stake while the daemon that saw it posted is still running.
    delivered: HashSet<String>,
    /// What every message not yet confirmed has spent trying and when it is
    /// next worth a pass, by message id: a delivery tmux would not take is
    /// tried again on a widening wait up to [`DELIVERY_ATTEMPTS`], and one
    /// that has spent them all is given up on — the user has been told, and
    /// nothing is typed for it again.
    owed: HashMap<String, Owed>,
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
        owed: HashMap::new(),
        typing: HashSet::new(),
        delivered_at: HashMap::new(),
        reports,
        sleep: SleepInhibitor::new(),
        prevent_sleep,
    };
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(TICK_SECS));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            // A message that is owed another pass has its own clock, read
            // afresh every time round: a delivery that comes back changes
            // when the next one is due, and the loop must be waiting on the
            // new answer rather than on the one it took before.
            let retry_at = scheduler.next_retry_at();
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
                _ = until(retry_at) => scheduler.retry_deliveries().await,
            }
        }
    });
    tx
}

/// Wait until `at`, or for ever when there is nothing to wait for.
///
/// The retry arm of the loop needs an instant either way, and a branch that
/// never fires is what "no message is owed a pass" looks like in a `select!`
/// — better than waking the loop on a made-up deadline to find nothing to do.
async fn until(at: Option<Instant>) {
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
