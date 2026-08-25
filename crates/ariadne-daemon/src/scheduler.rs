//! Scheduler: event-driven reconciliation loop (docker-style).
//!
//! HTTP handlers send [`SchedEvent`]s after writes; a periodic tick
//! reconciles everything so crashes, missed events and dead tmux sessions
//! self-heal. Every rule is idempotent: read state, compare desired, act.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use tokio::sync::mpsc;
use tracing::{info, warn};

use ariadne_core::{
    Actor, AttentionReason, GoalStatus, PromptKind, ReviewVerdict, Role, SessionStatus, TaskStatus,
};
use ariadne_store::{AgentSession, Message, Recipient, SessionFilter, Store, Task, TaskFilter};

use crate::agents::prompts;
use crate::attention;
use crate::launcher::Launcher;
use crate::notify;
use crate::sleep::SleepInhibitor;

/// Events that wake the scheduler for a scoped reconciliation.
#[derive(Debug, Clone)]
pub enum SchedEvent {
    /// A task's status, reviews or deps changed.
    TaskChanged(String),
    /// A goal was created / cancelled / finalized.
    GoalChanged(String),
    /// An agent session reported activity.
    SessionEvent(String),
    /// A message was posted into a goal or task conversation, by id: whoever
    /// it addresses is woken with it.
    MessagePosted(String),
}

/// What one keystroke delivery came to, reported back to the loop that asked
/// for it.
///
/// Typing into a pane takes seconds — a paste, an Enter, and the pane read
/// back to see whether the composer let go of it — so it happens in a task of
/// its own and the loop hears about it here. A tick that has three agents to
/// nudge waits on none of them.
#[derive(Debug)]
struct DeliveryReport {
    /// The message it carried, or `None` for a stall nudge, which is no
    /// message and nobody's to retry.
    message_id: Option<String>,
    /// The session whose pane it went into.
    session_id: String,
    outcome: DeliveryOutcome,
}

/// How a delivery ended: exactly one of confirmed, worth another pass, or
/// given up on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeliveryOutcome {
    /// The composer let go of it: the agent has it.
    Confirmed,
    /// Typed in and never submitted — the pane is there, and whoever is in
    /// front of it is not listening.
    Unsubmitted,
    /// tmux would not take it at all, which says nothing about the agent.
    Refused,
}

/// What one pass at waking an addressee came to.
enum Wake {
    /// Going into its pane now; the report says how that ended.
    InFlight,
    /// The agent has it: a resumed session comes back to it as its
    /// instruction.
    Delivered,
    /// Nothing to deliver: the agent is being woken for what it said itself,
    /// or it is sitting on a dialog nobody but the user may answer — and it
    /// is there to read the thread once it has been.
    Nothing,
    /// Its pane is busy with another delivery; a later tick tries again
    /// without spending an attempt on it.
    Busy,
    /// This pass could not, with the session to raise for the user once the
    /// attempts are gone — `None` when the addressee has no session at all,
    /// whether it has yet to have one or has lost the one it had.
    Failed(Option<String>),
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

/// What the watchdog has already done about one session's silence.
///
/// One record per session. `situation` is what keeps the nudge to one per
/// situation rather than one per session for ever: an agent whose task moved
/// on — a new status, a new review round — has a fresh reason to get on with
/// it, and so has one that was just put back on its feet.
#[derive(Debug, Default)]
struct Quiet {
    /// The status and round the two steps below were taken in: the task's for
    /// an engineer or a reviewer, the goal's for a planner.
    situation: (String, i64),
    /// Whether the one nudge for that situation has been spent.
    nudged: bool,
    /// Whether the user has been told about it.
    flagged: bool,
    /// Relaunches spent on this session, which no change of situation gives
    /// back. Bounded by [`SPAWN_RETRY_BUDGET`] like a task's spawn attempts
    /// are: an agent that goes quiet again after every relaunch is not one
    /// more relaunch away from working.
    relaunches: u32,
}

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
                    Some(SchedEvent::TaskChanged(id)) => scheduler.reconcile_task_logged(&id).await,
                    Some(SchedEvent::GoalChanged(id)) => scheduler.reconcile_goal_logged(&id).await,
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

impl Scheduler {
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
            self.reconcile_goal_logged(&goal.id).await;
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
                        self.reconcile_task_logged(&task.id).await;
                    }
                }
            }
        }
    }

    /// Mark sessions whose tmux process died as exited, and note the grid the
    /// living ones are drawing at.
    ///
    /// Measuring a pane answers both: `display-message` fails on a session
    /// that is not there. The size is written down because it is only
    /// knowable while the pane exists — a viewer opening the console log of a
    /// session that has ended has no other way to learn what width its bytes
    /// were written at (see `Launcher::record_pane_size`). This sweep is the
    /// only place that sees every session, watched or not.
    ///
    /// Returns how many sessions came out of it still alive, or `None` if the
    /// store could not be listed.
    async fn liveness_sweep(&mut self) -> Option<usize> {
        let Ok(live) = self
            .store
            .list_sessions(SessionFilter {
                live_only: true,
                ..Default::default()
            })
            .await
        else {
            return None;
        };
        let mut alive = 0;
        for session in live {
            match self
                .launcher
                .tmux
                .pane_geometry(&session.tmux_session)
                .await
            {
                Ok(geometry) => {
                    alive += 1;
                    self.launcher
                        .record_pane_size(&session.id, geometry.cols, geometry.rows)
                        .await;
                }
                // Confirmed before acting on it, and only an answer counts as
                // confirmation: marking a session exited ends its work, which
                // is too much to hang on a line of tmux output that failed to
                // parse — or on a `has-session` that never ran. Both leave the
                // session alone for the next sweep to ask again.
                Err(e) => match self
                    .launcher
                    .tmux
                    .has_session_checked(&session.tmux_session)
                    .await
                {
                    Ok(false) => {
                        info!(session = %session.id, tmux = %session.tmux_session, "session process gone, marking exited");
                        let _ = self
                            .store
                            .set_session_status(&session.id, SessionStatus::Exited)
                            .await;
                        // A pane that went away while its work is still going
                        // is not a session that finished: whatever was waiting
                        // on this agent is now waiting on nobody, so it is
                        // raised for the user. The flag outlives the session
                        // row's `exited` status on purpose — it stays up until
                        // the agent is resumed or replaced.
                        if attention::work_is_active(&self.store, &session).await {
                            warn!(session = %session.id, role = %session.role, "agent disconnected with work still active");
                            let _ = self
                                .store
                                .set_session_attention(&session.id, AttentionReason::Disconnected)
                                .await;
                        }
                    }
                    Ok(true) => {
                        alive += 1;
                        warn!(session = %session.id, error = %e, "measuring the pane failed")
                    }
                    // Unknown, so counted as alive: an unreachable tmux is no
                    // reason to let the machine sleep on a working agent.
                    Err(check) => {
                        alive += 1;
                        warn!(session = %session.id, error = %e, check = %check, "cannot reach tmux")
                    }
                },
            }
        }
        Some(alive)
    }

    /// Take down attention nobody can act on any more.
    ///
    /// A flag raised by an agent event is only ever taken down by another
    /// one, and a session sitting on a dialog emits nothing: an engineer
    /// blocked on a permission prompt whose task then goes under review would
    /// keep asking for the user for ever. Whatever put a flag up, it comes
    /// down once the work it was about stopped being this session's — the
    /// same question the sweep above asks before raising one.
    ///
    /// Two ways for that to be true, and a dead agent is the second: a prompt
    /// is a dialog on a pane, so a session that has ended cannot be waiting
    /// on an answer whatever its row still says. Retiring a session clears
    /// the flag as it goes (`set_session_status`); this is what heals the
    /// rows that were already stale when the daemon started, and it is not
    /// the same question as the one above — an exited planner of a goal still
    /// being planned is very much owed, which is what the sweep before this
    /// one raises as `disconnected`.
    async fn stale_attention_sweep(&self) {
        let Ok(flagged) = self
            .store
            .list_sessions(SessionFilter {
                attention_only: true,
                ..Default::default()
            })
            .await
        else {
            return;
        };
        for session in flagged {
            let why = if !session.status().is_live()
                && session.attention_reason().is_some_and(|r| r.is_prompt())
            {
                "the session ended on a prompt nobody can answer"
            } else if !attention::work_is_active(&self.store, &session).await {
                "the work moved on"
            } else {
                continue;
            };
            info!(session = %session.id, role = %session.role, why, "dropping attention");
            let _ = self.store.clear_session_attention(&session.id).await;
        }
    }

    async fn reconcile_session(&mut self, session_id: &str) {
        let Ok(session) = self.store.get_session(session_id).await else {
            return;
        };
        match &session.task_id {
            Some(task) => self.reconcile_task_logged(task).await,
            None => self.reconcile_goal_logged(&session.goal_id).await,
        }
    }

    async fn reconcile_goal_logged(&mut self, goal_id: &str) {
        if let Err(e) = self.reconcile_goal(goal_id).await {
            warn!(goal = %goal_id, error = %format!("{e:#}"), "goal reconciliation failed");
        }
    }

    async fn reconcile_goal(&mut self, goal_id: &str) -> anyhow::Result<()> {
        let goal = self.store.get_goal(goal_id).await?;
        match goal.status() {
            // A goal in planning wants a live planner session.
            GoalStatus::Planning => {
                let planners = self.live_sessions(goal_id, None, Role::Planner).await?;
                if planners.is_empty() {
                    info!(goal = %goal.id, "spawning planner");
                    self.launcher.spawn_planner(goal_id).await?;
                }
                // A planner has no task to flag: its session carries the
                // stall, which is the only place a goal still in planning has
                // to say that nothing is happening.
                for planner in planners {
                    let template = prompts::template_for(
                        &self.store,
                        &planner.profile_id,
                        PromptKind::PlannerResume,
                    )
                    .await;
                    let nudge = prompts::planner_resume_briefing(&template, &goal);
                    self.check_session_quiet(&planner, (goal.status.clone(), 0), &nudge)
                        .await?;
                }
            }
            GoalStatus::Active => {
                self.end_idle_planner(&goal.id).await;
                let tasks = self
                    .store
                    .list_tasks(TaskFilter {
                        goal_id: Some(goal.id.clone()),
                        status: None,
                    })
                    .await?;
                let all_merged = !tasks.is_empty()
                    && tasks
                        .iter()
                        .all(|t| matches!(t.status(), TaskStatus::Merged | TaskStatus::Cancelled))
                    && tasks.iter().any(|t| t.status() == TaskStatus::Merged);
                if all_merged {
                    info!(goal = %goal.id, "all tasks merged, goal completed");
                    let goal = self
                        .store
                        .set_goal_status(&goal.id, GoalStatus::Completed)
                        .await?;
                    // Before the planner is killed with the rest, so that the
                    // thread it was held in says how it ended rather than
                    // simply stopping.
                    if let Err(e) = notify::goal_completed(&self.store, &goal).await {
                        warn!(goal = %goal.id, error = %e, "writing the goal's last message failed");
                    }
                    self.kill_goal_sessions(&goal.id).await;
                }
            }
            // Cancelled: tear everything down; tasks are cancelled on behalf
            // of the user who cancelled the goal.
            GoalStatus::Cancelled => {
                for task in self
                    .store
                    .list_tasks(TaskFilter {
                        goal_id: Some(goal.id.clone()),
                        status: None,
                    })
                    .await?
                {
                    if !task.status().is_terminal() && task.status() != TaskStatus::Failed {
                        let cancelled = self
                            .store
                            .transition_task(
                                &task.id,
                                TaskStatus::Cancelled,
                                Actor::User,
                                Some("goal cancelled"),
                                None,
                            )
                            .await;
                        if let Ok(task) = cancelled {
                            self.announce_ending(&task, Some("goal cancelled")).await;
                        }
                    }
                    let _ = self.launcher.cleanup_task(&task.id, false, false).await;
                }
                self.kill_goal_sessions(&goal.id).await;
            }
            // Nothing left to do, but the teardown is repeated rather than
            // assumed: the kill on the way in is a one-off, and anything that
            // puts a session back on its feet afterwards — a `resume` racing
            // the transition, a kill that did not take — would otherwise keep
            // an agent alive under a finished goal for ever, holding the
            // machine awake with it. Every other arm converges on each tick;
            // so does this one. Sessions are killed at most once in practice,
            // because a killed one is no longer live.
            GoalStatus::Completed => self.kill_goal_sessions(&goal.id).await,
        }
        Ok(())
    }

    /// The planner's work ends with the plan: an idle one is let go once the
    /// goal it planned is being worked on.
    ///
    /// Nothing waits on a planner outside `planning` — `attention::work_is_active`
    /// says so, which is why a planner pane that vanishes under an active
    /// goal is not reported as a disconnect either — and a session nobody
    /// waits on that is left running is an agent holding a pane, a tmux
    /// process and the machine's sleep inhibitor open until the goal
    /// completes.
    ///
    /// Ended, not unreachable: the goal's tasks may still have something to
    /// say to their planner, and a message addressed to an exited session is
    /// what `wake_profile` revives it with — the conversation is in the agent
    /// CLI's own history, not in the pane. What is not ended is a planner
    /// mid-turn: whatever it is writing is finished first, and the next tick
    /// finds it idle.
    async fn end_idle_planner(&self, goal_id: &str) {
        let Ok(sessions) = self
            .store
            .list_sessions(SessionFilter {
                goal_id: Some(goal_id.to_string()),
                live_only: true,
                ..Default::default()
            })
            .await
        else {
            return;
        };
        for planner in sessions
            .iter()
            .filter(|s| s.role() == Role::Planner && s.status() == SessionStatus::Idle)
        {
            info!(goal = %goal_id, session = %planner.id, "the goal is past planning; ending its idle planner");
            if let Err(e) = self.launcher.kill_session(&planner.id).await {
                warn!(goal = %goal_id, session = %planner.id, error = %e, "ending the planner failed");
            }
        }
    }

    /// Kill every live session of a goal, whatever ended it.
    ///
    /// Failures are logged and otherwise swallowed: reconciliation carries on
    /// for the rest of the sessions, and the next tick asks again — but a
    /// session that will not die has to be visible, or the only symptom is a
    /// machine that never sleeps.
    async fn kill_goal_sessions(&self, goal_id: &str) {
        let sessions = match self
            .store
            .list_sessions(SessionFilter {
                goal_id: Some(goal_id.to_string()),
                live_only: true,
                ..Default::default()
            })
            .await
        {
            Ok(sessions) => sessions,
            Err(e) => {
                warn!(goal = %goal_id, error = %e, "listing sessions to kill failed");
                return;
            }
        };
        for session in sessions {
            info!(goal = %goal_id, session = %session.id, role = %session.role, "killing session of a finished goal");
            if let Err(e) = self.launcher.kill_session(&session.id).await {
                warn!(goal = %goal_id, session = %session.id, error = %e, "killing the session failed");
            }
        }
    }

    async fn reconcile_task_logged(&mut self, task_id: &str) {
        if let Err(e) = self.reconcile_task(task_id).await {
            warn!(task = %task_id, error = %format!("{e:#}"), "task reconciliation failed");
            self.record_spawn_failure(task_id).await;
        }
    }

    async fn record_spawn_failure(&mut self, task_id: &str) {
        let failures = self.spawn_failures.entry(task_id.to_string()).or_insert(0);
        *failures += 1;
        if *failures >= SPAWN_RETRY_BUDGET {
            warn!(task = %task_id, failures, "retry budget exhausted, failing task");
            let reason = "the agent could not be started";
            if let Ok(task) = self
                .store
                .transition_task(
                    task_id,
                    TaskStatus::Failed,
                    Actor::Daemon,
                    Some(reason),
                    None,
                )
                .await
            {
                self.announce_ending(&task, Some(reason)).await;
            }
            self.spawn_failures.remove(task_id);
        }
    }

    /// The sessions for this role that are still running — including the ones
    /// tmux would not answer for.
    ///
    /// Their number decides whether to spawn, so an unanswered question has to
    /// count as a session: the sweep leaves such rows alone precisely because
    /// nothing is known about them, and reconciling on the assumption they are
    /// dead is how a tmux outage turns into two agents on one task.
    async fn live_sessions(
        &self,
        goal_id: &str,
        task_id: Option<&str>,
        role: Role,
    ) -> anyhow::Result<Vec<AgentSession>> {
        let sessions = self
            .store
            .list_sessions(SessionFilter {
                goal_id: Some(goal_id.to_string()),
                task_id: task_id.map(str::to_string),
                live_only: true,
                ..Default::default()
            })
            .await?;
        let mut out = Vec::new();
        for s in sessions {
            if s.role() == role
                && self
                    .launcher
                    .tmux
                    .has_session_or_unknown(&s.tmux_session)
                    .await
            {
                out.push(s);
            }
        }
        Ok(out)
    }

    async fn reconcile_task(&mut self, task_id: &str) -> anyhow::Result<()> {
        let task = self.store.get_task(task_id).await?;
        let goal = self.store.get_goal(&task.goal_id).await?;
        if goal.status() != GoalStatus::Active {
            return Ok(());
        }

        // Reviewer sessions only belong to under_review: an agent whose part
        // of the lifecycle has passed is not left running on the task. The
        // engineer's is not one of them — it holds the worktree from the
        // first commit to the merge.
        if task.status() != TaskStatus::UnderReview {
            self.kill_role_sessions(&task, Role::Reviewer).await;
        }
        // A task that has left `approved` — landed, or sent back to the
        // reviewers with a revision — is one whose engineer wants briefing
        // again the next time it is approved.
        if task.status() != TaskStatus::Approved {
            self.landing_briefed.remove(&task.id);
        }

        match task.status() {
            TaskStatus::Pending => {
                if self.store.task_dependencies_merged(&task.id).await? {
                    info!(task = %task.id, "dependencies merged, task ready");
                    self.store
                        .transition_task(&task.id, TaskStatus::Ready, Actor::Daemon, None, None)
                        .await?;
                    // Fall through on the next event/tick.
                    return Box::pin(self.reconcile_task(task_id)).await;
                }
            }
            TaskStatus::Ready => {
                if self
                    .live_sessions(&goal.id, Some(&task.id), Role::Engineer)
                    .await?
                    .is_empty()
                {
                    info!(task = %task.id, "spawning engineer");
                    self.launcher.spawn_engineer(&task.id).await?;
                    self.spawn_failures.remove(&task.id);
                }
                self.store
                    .transition_task(&task.id, TaskStatus::InProgress, Actor::Daemon, None, None)
                    .await?;
            }
            TaskStatus::InProgress => {
                self.check_stall(&task, Role::Engineer).await?;
            }
            TaskStatus::UnderReview => {
                let reviewers = self.store.list_task_reviewers(&task.id).await?;
                let reviews = self
                    .store
                    .list_reviews(&task.id, Some(task.review_round))
                    .await?;

                // Verdicts first: they may close the round.
                let changes_requested = reviews
                    .iter()
                    .any(|r| r.verdict() == ReviewVerdict::RequestChanges);
                let approvals = reviews
                    .iter()
                    .filter(|r| r.verdict() == ReviewVerdict::Approve)
                    .count() as i64;
                if changes_requested {
                    info!(task = %task.id, "changes requested");
                    self.store
                        .transition_task(
                            &task.id,
                            TaskStatus::ChangesRequested,
                            Actor::Daemon,
                            None,
                            None,
                        )
                        .await?;
                    return Box::pin(self.reconcile_task(task_id)).await;
                }
                if approvals >= goal.required_approvals {
                    info!(task = %task.id, approvals, "approval threshold reached");
                    self.store
                        .transition_task(&task.id, TaskStatus::Approved, Actor::Daemon, None, None)
                        .await?;
                    return Box::pin(self.reconcile_task(task_id)).await;
                }

                // Start the reviewers that have no verdict and no live
                // session. A reviewer keeps one session for the whole task,
                // so what runs for round two onwards is that same session
                // resumed — its round is not part of its identity, only of
                // the briefing it is woken with.
                let verdict_by: std::collections::HashSet<_> = reviews
                    .iter()
                    .map(|r| r.reviewer_profile_id.clone())
                    .collect();
                let pending: Vec<String> = reviewers
                    .into_iter()
                    .filter(|p| !verdict_by.contains(p))
                    .collect();
                if pending.is_empty() {
                    return Ok(());
                }
                let summary = self.store.review_summary(&task.id).await?;
                for profile_id in pending {
                    let live = self
                        .store
                        .list_sessions(SessionFilter {
                            task_id: Some(task.id.clone()),
                            live_only: true,
                            ..Default::default()
                        })
                        .await?;
                    // As in `live_sessions`: a pane tmux would not answer for
                    // counts as one, so an outage cannot put a second reviewer
                    // on a round that already has one.
                    let mut running = None;
                    for s in &live {
                        if s.role() == Role::Reviewer
                            && s.profile_id == profile_id
                            && self
                                .launcher
                                .tmux
                                .has_session_or_unknown(&s.tmux_session)
                                .await
                        {
                            running = Some(s.clone());
                            break;
                        }
                    }
                    // One text for either way this reviewer is picked up:
                    // the verdict the round is waiting on and the diff that
                    // may have moved under it are what it is told whether its
                    // session is being started again or merely nudged.
                    let template =
                        prompts::template_for(&self.store, &profile_id, PromptKind::ReviewerResume)
                            .await;
                    let resume =
                        prompts::reviewer_resume_briefing(&template, &task, summary.as_deref());
                    // A reviewer with no verdict yet is the round's only
                    // reason to still be open, so an idle one is watched the
                    // same way an engineer is. Reviewers that already voted
                    // are not in `pending` and are left to sit: waiting for
                    // the others is not a stall. There is no task-level flag
                    // for this — the session's own is the signal.
                    if let Some(reviewer) = running {
                        self.check_session_quiet(
                            &reviewer,
                            (task.status.clone(), task.review_round),
                            &resume,
                        )
                        .await?;
                    } else {
                        info!(task = %task.id, reviewer = %profile_id, round = task.review_round, "starting reviewer");
                        // Resumes the reviewer's earlier session when there is
                        // one, spawns a first for it otherwise.
                        self.launcher
                            .resume_reviewer(&task.id, &profile_id, &resume)
                            .await?;
                        self.spawn_failures.remove(&task.id);
                    }
                }
            }
            TaskStatus::ChangesRequested => {
                let reviews = self
                    .store
                    .list_reviews(&task.id, Some(task.review_round))
                    .await?;
                // Who asked, as the engineer reads it: the reviewer's own name
                // and role, with the id as the fallback for a profile that has
                // since been deleted.
                let mut feedback: Vec<(String, String)> = Vec::new();
                for review in reviews
                    .iter()
                    .filter(|r| r.verdict() == ReviewVerdict::RequestChanges)
                {
                    let who = match self.store.get_profile(&review.reviewer_profile_id).await {
                        Ok(profile) => format!("{} ({})", profile.name, profile.role),
                        Err(_) => format!("reviewer {}", review.reviewer_profile_id),
                    };
                    feedback.push((
                        who,
                        review.body.clone().unwrap_or_else(|| "(no details)".into()),
                    ));
                }
                info!(task = %task.id, "resuming engineer with review feedback");
                let template = prompts::template_for(
                    &self.store,
                    &task.engineer_profile_id,
                    PromptKind::ChangesRequested,
                )
                .await;
                self.launcher
                    .resume_engineer(
                        &task.id,
                        &prompts::changes_requested_briefing(&template, &feedback),
                    )
                    .await?;
                self.spawn_failures.remove(&task.id);
                self.store
                    .transition_task(&task.id, TaskStatus::InProgress, Actor::Daemon, None, None)
                    .await?;
            }
            TaskStatus::Approved => {
                // Landing the change is the engineer's last turn, and the
                // session that wrote it is still there to take it: nothing
                // took the worktree away. What it has not had is the briefing
                // that says the task is approved and how its repository takes
                // it, so that goes out once — and from there the turn is
                // watched like any other.
                if self.landing_briefed.insert(task.id.clone()) {
                    info!(task = %task.id, "approved: briefing the engineer to land it");
                    let landing = self.resume_text(&task, Role::Engineer).await?;
                    self.launcher.resume_engineer(&task.id, &landing).await?;
                    self.spawn_failures.remove(&task.id);
                } else {
                    self.check_stall(&task, Role::Engineer).await?;
                }
            }
            TaskStatus::Merged => {
                // Post-merge cleanup (idempotent), then wake dependents.
                // Worktrees and the branch go by default; set
                // delete_merged_worktrees = false to keep merged work around
                // for inspection.
                self.launcher
                    .cleanup_task(
                        &task.id,
                        self.launcher.cfg.delete_merged_worktrees,
                        self.launcher.cfg.delete_merged_branches,
                    )
                    .await?;
                for dependent in self.dependents_of(&task).await? {
                    Box::pin(self.reconcile_task(&dependent)).await?;
                }
            }
            TaskStatus::Cancelled => {
                // Kill leftover agents; always keep worktrees and branch — a
                // cancelled task may hold uncommitted work worth salvaging.
                self.launcher.cleanup_task(&task.id, false, false).await?;
            }
            TaskStatus::Failed => {}
        }
        Ok(())
    }

    async fn dependents_of(&self, task: &Task) -> anyhow::Result<Vec<String>> {
        let all = self
            .store
            .list_tasks(TaskFilter {
                goal_id: Some(task.goal_id.clone()),
                status: None,
            })
            .await?;
        let mut out = Vec::new();
        for candidate in all {
            if candidate.status() == TaskStatus::Pending
                && self
                    .store
                    .list_task_dependencies(&candidate.id)
                    .await?
                    .contains(&task.id)
            {
                out.push(candidate.id);
            }
        }
        Ok(out)
    }

    async fn kill_role_sessions(&self, task: &Task, role: Role) {
        if let Ok(sessions) = self
            .store
            .list_sessions(SessionFilter {
                task_id: Some(task.id.clone()),
                live_only: true,
                ..Default::default()
            })
            .await
        {
            for session in sessions {
                if session.role() == role {
                    let _ = self.launcher.kill_session(&session.id).await;
                }
            }
        }
    }

    /// The agent a task is waiting on, watched.
    ///
    /// `role` is whose turn it is, which is the engineer's from the first
    /// commit to the merge.
    /// A task with no live session of that role gets one started; one that has
    /// reported nothing for too long goes under [`Self::check_session_quiet`],
    /// which is one nudge per (status, round), then the user, then a relaunch.
    /// The task shows that stall too, but nothing here writes it: the flag on
    /// the session is the record of it, and the task's own column is the
    /// store's projection of that (`sync_task_stall`).
    async fn check_stall(&mut self, task: &Task, role: Role) -> anyhow::Result<()> {
        let sessions = self
            .store
            .list_sessions(SessionFilter {
                task_id: Some(task.id.clone()),
                live_only: true,
                ..Default::default()
            })
            .await?;
        let Some(agent) = sessions.iter().find(|s| s.role() == role) else {
            info!(task = %task.id, role = role.as_str(), "no live session for the role the task is waiting on, starting one");
            if let Err(e) = self.start_role(task, role).await {
                // The task still wants this agent and could not get one: the
                // ended session is the thing the user has to look at.
                self.flag_last_disconnected(task, role).await;
                return Err(e);
            }
            self.spawn_failures.remove(&task.id);
            return Ok(());
        };
        // The same words it would be started again with: an agent that has
        // gone quiet with the work still in front of it and one whose session
        // ended are in the same situation, and there is one text for it.
        let nudge = self.resume_text(task, role).await?;
        self.check_session_quiet(agent, (task.status.clone(), task.review_round), &nudge)
            .await
    }

    /// Put the agent a task is waiting on back on it: its engineer, resumed
    /// where its session merely ended and started afresh where there is none.
    async fn start_role(&mut self, task: &Task, role: Role) -> anyhow::Result<()> {
        debug_assert_eq!(role, Role::Engineer);
        let instruction = self.resume_text(task, role).await?;
        self.launcher
            .resume_engineer(&task.id, &instruction)
            .await?;
        Ok(())
    }

    /// What the agent a task is waiting on is picked up with, whether its
    /// session ended or it merely went quiet: its profile's template for the
    /// situation, rendered.
    ///
    /// Two situations, and the task's status tells them apart. An approved
    /// task is one the engineer is landing, and what it is picked up with is
    /// the landing briefing — the whole procedure, which is what a session
    /// that ended over it has to be given back. Anything earlier is work in
    /// the worktree, and the resume nudge is what that wants.
    async fn resume_text(&self, task: &Task, _role: Role) -> anyhow::Result<String> {
        if task.status() == TaskStatus::Approved {
            let repo = self.store.get_repository(&task.repo_id).await?;
            let template = prompts::template_for(
                &self.store,
                &task.engineer_profile_id,
                PromptKind::LandingInstructions,
            )
            .await;
            return Ok(prompts::landing_briefing(&template, task, &repo));
        }
        let template = prompts::template_for(
            &self.store,
            &task.engineer_profile_id,
            PromptKind::EngineerResume,
        )
        .await;
        Ok(prompts::engineer_resume_briefing(&template, task))
    }

    /// Raise `disconnected` on the session of `role` that was last on this
    /// task, whatever state it ended in. Best effort: this runs while another
    /// failure is being reported, and adds nothing to it if it fails too.
    async fn flag_last_disconnected(&self, task: &Task, role: Role) {
        let Ok(sessions) = self
            .store
            .list_sessions(SessionFilter {
                task_id: Some(task.id.clone()),
                ..Default::default()
            })
            .await
        else {
            return;
        };
        if let Some(previous) = sessions.iter().rev().find(|s| s.role() == role) {
            warn!(task = %task.id, session = %previous.id, role = role.as_str(), "starting the agent failed, flagging its last session disconnected");
            let _ = self
                .store
                .set_session_attention(&previous.id, AttentionReason::Disconnected)
                .await;
        }
    }

    /// One agent that has reported nothing for too long.
    ///
    /// One clock and one timeline, whatever the shape of the silence.
    /// [`Self::last_heard_from`] says when this session was last heard from at
    /// all, and three thresholds are read off it: a nudge at
    /// [`QUIET_NUDGE_SECS`], the user at [`QUIET_FLAG_SECS`], and at
    /// [`QUIET_RELAUNCH_SECS`] the pane killed and the agent put back on its
    /// feet. Each of them is done once for the situation the agent is in, and
    /// a pass that arrives late does what the clock says now rather than going
    /// back for the steps it never had a chance to take.
    ///
    /// What the nudge is, the pane decides. An agent that is idle finished a
    /// turn and stopped with the work still in front of it, so it is told to
    /// get on with it, in the words it would be started again with. A running
    /// one whose composer is still holding an instruction is one that never
    /// submitted it — the Enter a TUI swallowed, or `codex resume <thread>
    /// <instruction>`, which hands the prompt to the composer through argv and
    /// leaves it there for somebody to send — and what that wants is the Enter
    /// a human would press on finding such a pane. A running one whose
    /// composer is empty is inside a turn, and typing into a turn is how work
    /// gets interrupted: it is left alone until the thresholds behind the
    /// nudge, which is where a turn that never ends is answered for.
    ///
    /// `situation` is what the nudge and the flag are spent on — the status
    /// and round the agent went quiet in — so moving on earns fresh ones, and
    /// `resume` is both what it is nudged with and what it is revived with.
    async fn check_session_quiet(
        &mut self,
        session: &AgentSession,
        situation: (String, i64),
        resume: &str,
    ) -> anyhow::Result<()> {
        if !matches!(
            session.status(),
            SessionStatus::Idle | SessionStatus::Running
        ) {
            return Ok(());
        }
        // An agent waiting on a person is blocked, not quiet. Typing into it
        // would answer whatever it is waiting on — a permission prompt takes
        // Enter for a yes — which is the one decision the daemon must not make
        // for it, and killing its pane would throw the dialog away. An agent
        // that reported an error is already asking for the user by name, and
        // overwriting that with a stall would take away the more useful half
        // of what it said.
        if matches!(
            session.attention_reason(),
            Some(
                AttentionReason::WaitingPermission
                    | AttentionReason::WaitingInput
                    | AttentionReason::AgentError
            )
        ) {
            return Ok(());
        }
        let Some(since) = self.last_heard_from(session) else {
            return Ok(());
        };
        let quiet_secs = (chrono::Utc::now() - since).num_seconds();
        if quiet_secs < QUIET_NUDGE_SECS {
            return Ok(());
        }
        // A pane already being typed into is being nudged by that, and is no
        // pane to kill either: the paste and the Enter behind it would come
        // back as a message nobody could be given, and the user would be told
        // about a composer that was only ever interrupted. It waits for the
        // pass after the delivery has settled.
        if self.typing.contains(&session.id) {
            return Ok(());
        }
        let done = self.quiet.entry(session.id.clone()).or_default();
        if done.situation != situation {
            done.situation = situation;
            done.nudged = false;
            done.flagged = false;
        }
        if quiet_secs >= QUIET_RELAUNCH_SECS {
            return self.relaunch_wedged(session, resume).await;
        }
        if quiet_secs >= QUIET_FLAG_SECS {
            // A flag raised for the user is left where it is: what
            // `waiting_user` says — a message written to them, a request that
            // is theirs to merge — is more use to them than "stalled", and it
            // is not the daemon's to take down on the agent's behalf. Nothing
            // is written down either, so a session that is still silent once
            // the user has had what they were owed is raised then. The silence
            // is measured all the same, and the relaunch above still happens.
            if session.attention_reason() == Some(AttentionReason::WaitingUser) {
                return Ok(());
            }
            if std::mem::replace(&mut done.flagged, true) {
                return Ok(());
            }
            warn!(session = %session.id, role = %session.role, quiet_secs, "the agent has reported nothing, flagging for user attention");
            self.store
                .set_session_attention(&session.id, AttentionReason::Stalled)
                .await?;
            return Ok(());
        }
        if done.nudged {
            return Ok(());
        }
        // A running agent is asked before the nudge is spent, so that a turn
        // nobody may interrupt costs it nothing: an empty composer is left
        // where it is, with its nudge still to come if something turns up in
        // there later. An unreachable tmux answers neither way, and is left
        // for the next pass too.
        let enter = session.status() == SessionStatus::Running;
        if enter
            && !self
                .launcher
                .tmux
                .composer_holds(&session.tmux_session, resume)
                .await
                .unwrap_or(false)
        {
            return Ok(());
        }
        self.quiet.entry(session.id.clone()).or_default().nudged = true;
        if enter {
            info!(session = %session.id, role = %session.role, quiet_secs, "the agent's composer is still holding its instruction, pressing Enter into the pane");
            // Spent whether or not tmux took it: a pane that refused the
            // keystroke this pass will refuse the next.
            return self.launcher.tmux.send_enter(&session.tmux_session).await;
        }
        info!(session = %session.id, role = %session.role, quiet_secs, "nudging idle agent");
        // Spent as the delivery goes out, and off the loop: a pane that takes
        // the nudge and will not submit it is raised for the user rather than
        // nudged again, and one tmux would not take at all gives the nudge
        // back — see [`Self::delivery_settled`].
        self.spawn_delivery(session, resume.to_string(), None);
        Ok(())
    }

    /// The one clock: when this session was last heard from at all.
    ///
    /// Three things count, and the latest of them is the answer. What the
    /// agent reported is the plain one — every hook and every plugin event
    /// stamps `last_activity_at`, so an agent that is working keeps its own
    /// clock reset however slowly it works, and a wedged one is exactly the
    /// one that cannot. A confirmed delivery counts because it is a nudge in
    /// its own right, and a better one: an agent told what to do a moment ago
    /// is not asked why it has stopped. And the launch counts because a
    /// session that has reported nothing at all still has to be measured from
    /// something — an instruction left sitting in a composer fires no hook
    /// whatsoever.
    ///
    /// `None` when none of the three is known, which is a session nothing is
    /// concluded about.
    fn last_heard_from(&self, session: &AgentSession) -> Option<chrono::DateTime<chrono::Utc>> {
        let stamped = |at: &Option<String>| {
            at.as_deref()
                .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
                .map(|at| at.with_timezone(&chrono::Utc))
        };
        [
            stamped(&session.last_activity_at),
            stamped(&session.launched_at),
            self.delivered_at.get(&session.id).copied(),
        ]
        .into_iter()
        .flatten()
        .max()
    }

    /// Put a wedged agent back on its feet: the pane killed, and the same
    /// session row relaunched on the agent conversation it was already having.
    ///
    /// The relaunch is spent out of a budget for the same reason a spawn is:
    /// an agent that goes quiet, is put back and goes quiet again is not one
    /// more relaunch away from working, so [`SPAWN_RETRY_BUDGET`] of them is
    /// what there is and a task whose agent will not run is failed rather than
    /// restarted for ever. A planner has no task to fail — its own flag is
    /// what is left, and it stands.
    ///
    /// An engineer is started the way a task with no live engineer is started
    /// — [`Self::start_role`], which renders what it is picked up with, the
    /// landing briefing included where the task is approved. A planner and a
    /// reviewer have no such path: they are revived with the resume the caller
    /// already rendered, through the same `revive_session` a message addressed
    /// to a dead agent takes.
    ///
    /// What the user is owed outlives the relaunch. Putting an agent back on
    /// its feet drops the row's attention (`restart_session`), which is right
    /// for every reason the agent raised for itself and wrong for the one
    /// nobody raised on its behalf: a published request is still theirs to
    /// merge and a message written to them is still unread, however many
    /// times the agent underneath is restarted. So `waiting_user` is put back
    /// — on whatever came back up, since a resume keeps the row and a spawn
    /// that had to start afresh does not, and the flag belongs to the agent
    /// the work is with either way.
    async fn relaunch_wedged(
        &mut self,
        session: &AgentSession,
        revival: &str,
    ) -> anyhow::Result<()> {
        let done = self.quiet.entry(session.id.clone()).or_default();
        done.relaunches += 1;
        // A relaunched agent is a fresh instruction that may be stuck in its
        // own right, so the steps taken for the situation come back with the
        // launch: the clock starts again, and so does the timeline on it.
        done.nudged = false;
        done.flagged = false;
        let spent = done.relaunches;
        if spent >= SPAWN_RETRY_BUDGET {
            warn!(session = %session.id, role = %session.role, relaunches = spent - 1, "the agent went quiet again after every relaunch");
            let Some(task_id) = session.task_id.clone() else {
                return Ok(());
            };
            if let Err(e) = self.launcher.kill_session(&session.id).await {
                warn!(session = %session.id, error = %e, "killing the wedged session failed");
            }
            // Told to the user like every other ending the daemon decides,
            // and by the same call: a task nobody is coming back to is worth
            // a line in its thread.
            let reason = "its agent stopped mid-turn after every relaunch";
            if let Ok(task) = self
                .store
                .transition_task(
                    &task_id,
                    TaskStatus::Failed,
                    Actor::Daemon,
                    Some(reason),
                    None,
                )
                .await
            {
                self.announce_ending(&task, Some(reason)).await;
            }
            return Ok(());
        }
        info!(session = %session.id, role = %session.role, relaunch = spent, "the agent has reported nothing for too long, relaunching it");
        let for_the_user = session.attention_reason() == Some(AttentionReason::WaitingUser);
        self.launcher.kill_session(&session.id).await?;
        if let Some(task_id) = session.task_id.clone()
            && session.role() == Role::Engineer
        {
            let task = self.store.get_task(&task_id).await?;
            self.start_role(&task, session.role()).await?;
        } else {
            self.launcher
                .revive_session(&session.id, Some(revival))
                .await?;
        }
        if for_the_user {
            let back = self.relaunched_session(session).await;
            self.store
                .set_session_attention(&back, AttentionReason::WaitingUser)
                .await?;
        }
        Ok(())
    }

    /// The session the agent came back as: the same row wherever it was
    /// resumed, and the one the role is live in when the relaunch had to
    /// spawn afresh instead. The row that was killed is the answer of last
    /// resort — a relaunch that left nothing running is not a row to lose the
    /// flag over.
    async fn relaunched_session(&self, session: &AgentSession) -> String {
        self.live_sessions(&session.goal_id, session.task_id.as_deref(), session.role())
            .await
            .ok()
            .and_then(|mut live| live.pop())
            .map(|live| live.id)
            .unwrap_or_else(|| session.id.clone())
    }

    /// Take a posted message to whoever it addresses.
    ///
    /// A conversation an agent only reads when it next happens to look is a
    /// conversation nobody can hold: a message with a recipient is carried to
    /// that recipient, and one addressed to the thread wakes nobody, exactly
    /// as every message did before recipients existed.
    async fn deliver_message(&mut self, message_id: &str) {
        if let Err(e) = self.deliver(message_id).await {
            warn!(message = %message_id, error = %format!("{e:#}"), "delivering the message failed");
        }
    }

    /// One pass at an addressed message, which ends in exactly one of three
    /// places: the agent has it, another tick tries again, or the user is
    /// told nobody ever got it.
    ///
    /// What is never spent is the attempt itself. A message struck off before
    /// it was typed — because that is when the daemon happened to look — is a
    /// message nobody ever receives and nobody is told about, so the only
    /// things that stop a message being tried again are a confirmation and
    /// running out of [`DELIVERY_ATTEMPTS`].
    async fn deliver(&mut self, message_id: &str) -> anyhow::Result<()> {
        if self.delivered.contains(message_id) || self.given_up_on(message_id) {
            return Ok(());
        }
        let message = self.store.get_message(message_id).await?;
        let Some(recipient) = message.recipient() else {
            return Ok(());
        };
        match recipient {
            Recipient::User => {
                self.raise_for_user(&message).await?;
                self.delivered.insert(message.id.clone());
                Ok(())
            }
            Recipient::Profile(profile_id) => {
                match self.wake_profile(&message, &profile_id).await? {
                    // The report it comes back with says what became of it.
                    Wake::InFlight => Ok(()),
                    Wake::Delivered => {
                        self.attempts.remove(&message.id);
                        self.delivered.insert(message.id.clone());
                        Ok(())
                    }
                    Wake::Nothing => {
                        self.attempts.remove(&message.id);
                        Ok(())
                    }
                    // Nothing was tried, so nothing was spent: the message
                    // stays on the list the tick works through.
                    Wake::Busy => {
                        self.attempts.entry(message.id.clone()).or_insert(0);
                        Ok(())
                    }
                    Wake::Failed(session_id) => {
                        self.delivery_failed(&message.id, session_id.as_deref())
                            .await
                    }
                }
            }
        }
    }

    /// Whether this message has spent everything it was worth and the user
    /// has been told: nothing is typed for it again.
    fn given_up_on(&self, message_id: &str) -> bool {
        self.attempts
            .get(message_id)
            .is_some_and(|spent| *spent >= DELIVERY_ATTEMPTS)
    }

    /// Every message still owed a delivery, offered another pass.
    ///
    /// This is what makes "tried again on a later tick" true: a tmux that
    /// would not take a message has said nothing about whether the agent is
    /// there to hear it, so the message waits here and every tick asks again
    /// until it goes through or the attempts run out.
    async fn retry_deliveries(&mut self) {
        let owed: Vec<String> = self
            .attempts
            .iter()
            .filter(|(_, spent)| **spent < DELIVERY_ATTEMPTS)
            .map(|(id, _)| id.clone())
            .collect();
        for message_id in owed {
            self.deliver_message(&message_id).await;
        }
    }

    /// Type `text` into a session's pane in a task of its own, which reports
    /// back what came of it.
    ///
    /// Off the loop because [`TmuxManager::send_submitted`] is slow by
    /// design: it lets a paste settle, presses Enter, reads the pane back and
    /// tries again on a widening backoff — seconds of waiting that the
    /// scheduler used to do inline, one agent at a time, while every other
    /// event queued behind it.
    fn spawn_delivery(&mut self, session: &AgentSession, text: String, message_id: Option<String>) {
        self.typing.insert(session.id.clone());
        let tmux = self.launcher.tmux.clone();
        let reports = self.reports.clone();
        let pane = session.tmux_session.clone();
        let session_id = session.id.clone();
        tokio::spawn(async move {
            let outcome = match tmux.send_submitted(&pane, &text).await {
                Ok(true) => DeliveryOutcome::Confirmed,
                Ok(false) => DeliveryOutcome::Unsubmitted,
                Err(e) => {
                    warn!(session = %session_id, error = %format!("{e:#}"), "typing into the agent's pane failed");
                    DeliveryOutcome::Refused
                }
            };
            let _ = reports.send(DeliveryReport {
                message_id,
                session_id,
                outcome,
            });
        });
    }

    /// A delivery has come back: the one place that may call a message
    /// arrived, and the one that decides what an agent that never heard it
    /// costs.
    async fn delivery_settled(&mut self, report: DeliveryReport) {
        self.typing.remove(&report.session_id);
        match report.outcome {
            DeliveryOutcome::Confirmed => {
                // Only a message. A nudge that went in is a nudge spent —
                // what follows one nobody acts on is the user, and a nudge
                // that gave itself back would ask for ever and tell nobody.
                if let Some(message_id) = &report.message_id {
                    info!(message = %message_id, session = %report.session_id, "the addressed agent has the message");
                    self.attempts.remove(message_id);
                    self.delivered.insert(message_id.clone());
                    // This agent has just been told what to do: the quiet
                    // clock starts again here and the nudge that may have
                    // been spent comes back, so that nothing tells it to get
                    // on with what it was asked to do a moment ago.
                    if let Some(done) = self.quiet.get_mut(&report.session_id) {
                        done.nudged = false;
                    }
                    self.delivered_at
                        .insert(report.session_id.clone(), chrono::Utc::now());
                }
            }
            DeliveryOutcome::Unsubmitted => {
                warn!(session = %report.session_id, message = ?report.message_id, "what was typed stayed in the agent's composer, flagging for user attention");
                // Not tried again: a pane that would not submit it this pass
                // will not submit it the next, and a second paste would leave
                // the composer holding the same thing twice.
                if let Some(message_id) = report.message_id {
                    self.attempts.insert(message_id, DELIVERY_ATTEMPTS);
                }
                if let Err(e) = self
                    .store
                    .set_session_attention(&report.session_id, AttentionReason::Stalled)
                    .await
                {
                    warn!(session = %report.session_id, error = %e, "flagging the session failed");
                }
            }
            DeliveryOutcome::Refused => match report.message_id {
                Some(message_id) => {
                    if let Err(e) = self
                        .delivery_failed(&message_id, Some(&report.session_id))
                        .await
                    {
                        warn!(message = %message_id, error = %format!("{e:#}"), "giving up on the message failed");
                    }
                }
                // Nothing was typed, so the nudge is unspent rather than
                // lost: the next pass over this session sends it again.
                None => {
                    if let Some(done) = self.quiet.get_mut(&report.session_id) {
                        done.nudged = false;
                    }
                }
            },
        }
        // The pane is free again, so whatever was waiting for it goes in now
        // rather than at the next tick — unless tmux is the thing that
        // refused, in which case asking it again this second only spends the
        // attempts of everything queued behind it.
        if report.outcome != DeliveryOutcome::Refused {
            self.retry_deliveries().await;
        }
    }

    /// One pass that could not deliver: another tick tries again, and once
    /// the passes are gone the user is told rather than the message being
    /// left with nobody.
    async fn delivery_failed(
        &mut self,
        message_id: &str,
        session_id: Option<&str>,
    ) -> anyhow::Result<()> {
        let spent = {
            let spent = self.attempts.entry(message_id.to_string()).or_insert(0);
            *spent += 1;
            *spent
        };
        if spent < DELIVERY_ATTEMPTS {
            info!(message = %message_id, spent, "the message did not reach its agent; trying again on a later tick");
            return Ok(());
        }
        let message = self.store.get_message(message_id).await?;
        self.give_up(&message, session_id).await
    }

    /// A message that will not be delivered, put where the user will see it:
    /// on the addressee's session — stalled while its pane is still there,
    /// disconnected once it is gone — and, when the addressee has no session
    /// of its own to flag, on the session of whoever wrote it, which is the
    /// pane they are watching for an answer. The message itself stays in the
    /// thread either way; what is raised is that nobody came for it.
    async fn give_up(&self, message: &Message, session_id: Option<&str>) -> anyhow::Result<()> {
        let session = match session_id {
            Some(id) => self.store.get_session(id).await.ok(),
            None => None,
        };
        let Some(session) = session else {
            let Some(author) = &message.author_session_id else {
                warn!(message = %message.id, "the message reached nobody, and there is nobody to tell");
                return Ok(());
            };
            warn!(message = %message.id, session = %author, "the message reached nobody; raising its author for the user");
            self.store
                .set_session_attention(author, AttentionReason::WaitingInput)
                .await?;
            return Ok(());
        };
        let reason = match self
            .launcher
            .tmux
            .has_session_checked(&session.tmux_session)
            .await
        {
            Ok(false) => AttentionReason::Disconnected,
            _ => AttentionReason::Stalled,
        };
        warn!(message = %message.id, session = %session.id, reason = reason.as_str(), "the message never reached the agent, flagging for user attention");
        self.store
            .set_session_attention(&session.id, reason)
            .await?;
        Ok(())
    }

    /// Tell the user a task the daemon itself ended is over, and deliver it
    /// the way the HTTP path delivers its own.
    ///
    /// Best effort on both halves: a notice that cannot be written is not a
    /// reason to leave the transition half-made, and the ending is in the
    /// task's status either way.
    async fn announce_ending(&mut self, task: &Task, reason: Option<&str>) {
        match notify::task_ended(&self.store, task, reason).await {
            Ok(Some(message)) => self.deliver_message(&message.id).await,
            Ok(None) => {}
            Err(e) => {
                warn!(task = %task.id, error = %e, "telling the user the task ended failed")
            }
        }
    }

    /// A message for the human, which no agent is woken for: it goes up the
    /// attention path the UI strip and `ariadne attention` already show, on
    /// the session of the agent that wrote it — the session the user answers
    /// in, and the one place the message can be traced back to.
    ///
    /// This is the only place a message addressed to the user raises
    /// anything, whoever wrote it: what the daemon says to the user — a pull
    /// request opened, an approval, a task that ended — travels as a message
    /// like everything else and goes up here, rather than beside a
    /// `create_message` call with a flag of its own.
    async fn raise_for_user(&self, message: &Message) -> anyhow::Result<()> {
        let session_id = match &message.author_session_id {
            Some(session_id) => session_id.clone(),
            // Written by the daemon rather than by an agent, so there is no
            // author's session to point at: the flag goes on the agent the
            // task is with, which is the row its attention is read from. A
            // task with nothing running, and a notice in a goal's thread,
            // raise nothing — the message waits where it was written.
            None => match self.session_the_task_is_with(message).await? {
                Some(session) => session.id,
                None => {
                    info!(message = %message.id, "message addressed to the user with no session to raise it on; it waits in the thread");
                    return Ok(());
                }
            },
        };
        info!(message = %message.id, session = %session_id, "message addressed to the user, raising it for them");
        self.store
            .set_session_attention(&session_id, AttentionReason::WaitingUser)
            .await?;
        Ok(())
    }

    /// The live session a task's own notices are raised on: its engineer, the
    /// most recent one it has.
    async fn session_the_task_is_with(
        &self,
        message: &Message,
    ) -> anyhow::Result<Option<AgentSession>> {
        let Some(task_id) = &message.task_id else {
            return Ok(None);
        };
        let sessions = self
            .store
            .list_sessions(SessionFilter {
                task_id: Some(task_id.clone()),
                live_only: true,
                ..Default::default()
            })
            .await?;
        Ok(sessions
            .iter()
            .rev()
            .find(|s| s.role() == Role::Engineer)
            .cloned())
    }

    /// A message for an agent: typed into its pane if it has one, and
    /// otherwise resumed with the message as its instruction.
    ///
    /// An addressee with no session to deliver to — a reviewer between
    /// rounds, an engineer whose task has not started, one whose session went
    /// away — is not a message lost: it keeps its place in the thread, and
    /// the briefings send every agent to read the conversation when it next
    /// starts. It is not a message delivered either, though, so it is a pass
    /// like any other: the later ticks find the session once it exists, and
    /// when they run out with nobody there the author is told rather than
    /// left waiting on an answer that is not coming.
    ///
    /// What comes back is one of [`Wake`]; the caller keeps the count of what
    /// a message has spent, since only it knows how many passes have been
    /// made at this one.
    async fn wake_profile(&mut self, message: &Message, profile_id: &str) -> anyhow::Result<Wake> {
        let Some(session) = self.recipient_session(message, profile_id).await? else {
            info!(message = %message.id, profile = %profile_id, "nobody to wake for this message yet; it waits in the thread");
            return Ok(Wake::Failed(None));
        };
        // An agent does not need waking for what it said itself.
        if message.author_session_id.as_deref() == Some(session.id.as_str()) {
            return Ok(Wake::Nothing);
        }
        let template =
            prompts::template_for(&self.store, profile_id, PromptKind::MessageDelivery).await;
        let text = prompts::message_delivery(&template, message);
        // Asked rather than assumed, the way the spawn guards ask: a tmux
        // that cannot be reached has said nothing about the pane, and an
        // agent relaunched on top of a live one is two agents on one task.
        match self
            .launcher
            .tmux
            .has_session_checked(&session.tmux_session)
            .await
        {
            Ok(true) => {}
            Ok(false) => {
                info!(message = %message.id, session = %session.id, role = %session.role, "resuming the addressed agent with the message");
                return match self.launcher.revive_session(&session.id, Some(&text)).await {
                    Ok(_) => Ok(Wake::Delivered),
                    // Nothing to resume from (no agent id yet, a working
                    // directory that is gone) is worth another pass, and then
                    // the user: a message whose agent cannot be brought back
                    // is one nobody will ever answer.
                    Err(e) => {
                        info!(message = %message.id, session = %session.id, error = %format!("{e:#}"), "the addressed agent could not be resumed");
                        Ok(Wake::Failed(Some(session.id)))
                    }
                };
            }
            Err(e) => {
                warn!(message = %message.id, session = %session.id, error = %format!("{e:#}"), "tmux cannot say whether the addressed agent still has a pane");
                return Ok(Wake::Failed(Some(session.id)));
            }
        }
        // An agent sitting on a dialog is not typed into: what a pane holding
        // a permission prompt does with the Enter behind a paste is answer it,
        // and that is the one decision the daemon must not make. The message
        // waits in the thread, where the agent reads it once the user has
        // dealt with the prompt.
        if matches!(
            session.attention_reason(),
            Some(AttentionReason::WaitingPermission | AttentionReason::WaitingInput)
        ) {
            info!(message = %message.id, session = %session.id, "the addressed agent is waiting on the user; the message waits in the thread");
            return Ok(Wake::Nothing);
        }
        if self.typing.contains(&session.id) {
            return Ok(Wake::Busy);
        }
        info!(message = %message.id, session = %session.id, role = %session.role, "nudging the addressed agent with the message");
        self.spawn_delivery(&session, text, Some(message.id.clone()));
        Ok(Wake::InFlight)
    }

    /// The session a message's addressee works in, the most recent one first.
    ///
    /// A goal-thread message looks at the goal's own sessions — the ones with
    /// no task — because that is where the planner runs, and the planner is
    /// the only agent a goal thread can address; filtering by goal alone
    /// would reach into its tasks.
    ///
    /// A task message looks in that task, and then — for the planner alone —
    /// at the goal's own sessions. Every task thread can address the planner
    /// that wrote it (see `http::recipients`), and the planner works at the
    /// goal level: filtering by task alone would find no session for it and
    /// wake nobody at all. Everyone else a task thread addresses works in
    /// that task, so a session of theirs outside it is somebody else's
    /// conversation and is not typed into for this one.
    async fn recipient_session(
        &self,
        message: &Message,
        profile_id: &str,
    ) -> anyhow::Result<Option<AgentSession>> {
        let sessions = self
            .store
            .list_sessions(SessionFilter {
                goal_id: Some(message.goal_id.clone()),
                ..Default::default()
            })
            .await?;
        let mut at_goal = None;
        for session in sessions.into_iter().rev() {
            if session.profile_id != profile_id {
                continue;
            }
            match &session.task_id {
                Some(_) if session.task_id == message.task_id => return Ok(Some(session)),
                None if at_goal.is_none() && session.role() == Role::Planner => {
                    at_goal = Some(session)
                }
                _ => {}
            }
        }
        // Which leaves the goal's own planner, reached from the task thread
        // that addressed it — and, for a goal thread, the only session it was
        // ever allowed to reach.
        Ok(at_goal)
    }
}
