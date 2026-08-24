//! Scheduler: event-driven reconciliation loop (docker-style).
//!
//! HTTP handlers send [`SchedEvent`]s after writes; a periodic tick
//! reconciles everything so crashes, missed events and dead tmux sessions
//! self-heal. Every rule is idempotent: read state, compare desired, act.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::Context;
use tokio::sync::mpsc;
use tracing::{info, warn};

use ariadne_core::{
    Actor, AttentionReason, AuthorRole, GoalStatus, PromptKind, ReviewVerdict, Role, SessionStatus,
    TaskStatus,
};
use ariadne_store::{
    AgentSession, Message, NewMessage, NewReview, Recipient, Repository, ReviewAuthor,
    SessionFilter, Store, Task, TaskFilter,
};

use crate::agents::prompts;
use crate::attention;
use crate::forge::{self, Conflict, FailedCheck, Feedback, Forge, PrState, WatchedPr};
use crate::gh;
use crate::glab;
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
/// Idle time after which an agent with work in front of it gets one nudge.
const STALL_NUDGE_SECS: i64 = 300;
/// Idle time after which the stall is raised for the user (post-nudge).
const STALL_FLAG_SECS: i64 = 900;
/// Time since a launch after which an agent that never started its turn has
/// Enter pressed into its pane. The same clock as an idle stall, for the same
/// reason: long enough that a slow start is not read as a stuck one.
const UNSTARTED_ENTER_SECS: i64 = STALL_NUDGE_SECS;
/// Time since a launch after which that agent is raised for the user
/// (post-Enter).
const UNSTARTED_FLAG_SECS: i64 = STALL_FLAG_SECS;
/// Consecutive polls that could not read a published request before the user
/// is told about it.
///
/// At the default `pr_poll_secs` that is a quarter of an hour: long enough
/// that a forge having a bad minute, a laptop between networks or a token
/// being refreshed passes unremarked, short enough that a `gh` nobody ever
/// authenticated is not left watching nothing all afternoon. A poll that
/// fails is a poll that said nothing at all — the request may have been
/// merged, closed or commented on meanwhile — so silence here is not the
/// same silence as a quiet review.
const POLL_FAILURE_LIMIT: u32 = 5;

/// Event kinds only an agent whose turn actually started can have reported.
///
/// What is missing from the set is the point of it. Lifecycle alone proves
/// nothing: codex reports `session_start` for a TUI that has merely come up,
/// and opencode keeps emitting `session.updated` whether or not anything is
/// happening — neither says a prompt was ever submitted. Measured against
/// codex 0.148: a resumed thread whose instruction is left sitting in the
/// composer fires no hook whatsoever, and the single Enter that submits it
/// fires `SessionStart`, `UserPromptSubmit` and `Stop` together — so even
/// `session_start` arrives *with* the turn rather than ahead of it there. The
/// discriminator is therefore a prompt, a tool call or a dialog: things that
/// only happen inside a turn.
const TURN_ACTIVITY: [&str; 13] = [
    // Codex and Claude Code hooks.
    "user_prompt_submit",
    "pre_tool_use",
    "post_tool_use",
    "permission_request",
    // OpenCode's plugin. `session.idle` and `stop` are deliberately absent:
    // they end a turn, and a session that reported one is `idle` rather than
    // `running` — the other watchdog's, not this one's.
    //
    // `session.error` is here for the opposite reason: a failed turn is a
    // turn. It leaves the session running (the ingest maps it to no status at
    // all) with the failure raised for the user, and an agent that got far
    // enough to report one is not an agent sitting on an unsubmitted
    // instruction.
    "session.error",
    "tool.execute.before",
    "tool.execute.after",
    "permission.asked",
    "permission.updated",
    "permission.replied",
    "question.asked",
    "question.replied",
    "question.rejected",
];

/// How far the never-started-turn watchdog has taken one session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Unstarted {
    EnterPressed,
    Flagged,
}

pub struct Scheduler {
    store: Store,
    launcher: Arc<Launcher>,
    /// Spawn failures per task (in-memory: resets on daemon restart, which is
    /// fine — a restart is exactly when a retry is warranted).
    spawn_failures: HashMap<String, u32>,
    /// Sessions nudged in the (status, round) they were nudged for, keyed by
    /// session id; a transition changes the key, which is what allows the one
    /// nudge per situation rather than one per session for ever.
    nudged: HashMap<String, (String, i64)>,
    /// Sessions this watchdog has acted on and the launch (`launched_at`) it
    /// acted on, keyed by session id: a relaunch changes the key the same way
    /// a transition changes `nudged`'s, which is what keeps it to one Enter
    /// and one flag per launch rather than one per tick.
    unstarted: HashMap<String, (String, Unstarted)>,
    /// When each task's pull or merge request was last looked at, by task
    /// id: what keeps the polling to `pr_poll_secs` rather than to every tick and
    /// every event. In memory like the maps above — a daemon that restarts
    /// simply looks once immediately, which is what it wants to do anyway.
    pr_polled: HashMap<String, std::time::Instant>,
    /// Consecutive polls of each task's request that could not read it, by
    /// task id: what turns a CLI failing over and over into the one message
    /// [`POLL_FAILURE_LIMIT`] is the threshold for. Cleared by the first poll
    /// that reads the request again — and in memory beside `pr_polled`, so a
    /// daemon that restarts starts the count over, which is the right thing
    /// to do about a CLI that may well have been fixed in between.
    poll_failures: HashMap<String, u32>,
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
    /// just been told what to do — so the stall watch counts idle time from
    /// here as well as from what the agent itself last reported.
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
        nudged: HashMap::new(),
        unstarted: HashMap::new(),
        pr_polled: HashMap::new(),
        poll_failures: HashMap::new(),
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
                    self.check_session_stall(
                        &planner,
                        (goal.status.clone(), 0),
                        "Keep planning this goal: create the tasks it still needs with `create_task`, or call `finalize_plan` once the user agrees the plan is complete.",
                    )
                    .await?;
                }
            }
            GoalStatus::Active => {
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

        // Reviewer sessions only belong to under_review, integrator ones to
        // integrating: an agent whose part of the lifecycle has passed is not
        // left running on the task.
        if task.status() != TaskStatus::UnderReview {
            self.kill_role_sessions(&task, Role::Reviewer).await;
        }
        if task.status() != TaskStatus::Integrating {
            self.kill_role_sessions(&task, Role::Integrator).await;
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
                // A published request has reviewers of its own, and they are
                // people: the round the engineer just answered them in is
                // theirs to judge on the forge, not one for the reviewer
                // profiles to sit through again. So it is approved on the
                // spot and the branch goes back to the integrator, which
                // pushes the revision to the request and hands the answers
                // on.
                if let Some(watched) = forge::watched_pull_request(&task) {
                    self.approve_published_revision(&task, &watched).await?;
                    return Box::pin(self.reconcile_task(task_id)).await;
                }
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
                    .filter_map(|r| r.reviewer_profile_id.clone())
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
                    // A reviewer with no verdict yet is the round's only
                    // reason to still be open, so an idle one is watched the
                    // same way an engineer is. Reviewers that already voted
                    // are not in `pending` and are left to sit: waiting for
                    // the others is not a stall. There is no task-level flag
                    // for this — the session's own is the signal.
                    if let Some(reviewer) = running {
                        self.check_session_stall(
                            &reviewer,
                            (task.status.clone(), task.review_round),
                            "Finish reviewing this round and submit your verdict with `approve` or `request_changes`.",
                        )
                        .await?;
                    } else {
                        info!(task = %task.id, reviewer = %profile_id, round = task.review_round, "starting reviewer");
                        // Resumes the reviewer's earlier session when there is
                        // one, spawns a first for it otherwise.
                        let template = prompts::template_for(
                            &self.store,
                            &profile_id,
                            PromptKind::ReviewerResume,
                        )
                        .await;
                        self.launcher
                            .resume_reviewer(
                                &task.id,
                                &profile_id,
                                &prompts::reviewer_resume_briefing(
                                    &template,
                                    &task,
                                    summary.as_deref(),
                                ),
                            )
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
                // Who asked, as the engineer reads it: the profile's own name
                // and role, since a round can also be closed by the integrator
                // sending the task back. The id is the fallback for a profile
                // that has since been deleted. A round the daemon relayed from
                // a published request has no profile behind it at all — it is
                // named after the request the humans wrote on, whose comments
                // carry their own names inside it.
                let mut feedback: Vec<(String, String)> = Vec::new();
                for review in reviews
                    .iter()
                    .filter(|r| r.verdict() == ReviewVerdict::RequestChanges)
                {
                    let who = match review.author() {
                        ReviewAuthor::Profile(profile_id) => {
                            match self.store.get_profile(&profile_id).await {
                                Ok(profile) => format!("{} ({})", profile.name, profile.role),
                                Err(_) => format!("reviewer {profile_id}"),
                            }
                        }
                        ReviewAuthor::Role(AuthorRole::Forge) => {
                            match forge::watched_pull_request(&task) {
                                Some(watched) => {
                                    format!("{} on {}", watched.label(), watched.forge.name())
                                }
                                None => "the published request".to_string(),
                            }
                        }
                        ReviewAuthor::Role(role) => role.as_str().to_string(),
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
                info!(task = %task.id, "approved: handing the task to its integrator");
                // The engineer is about to lose its worktree and its session
                // with it — the integrator checks the branch out. A line in
                // the thread is what it costs to have that happen to an agent
                // that was told why: addressed to nobody, so nothing is woken
                // for it.
                let _ = self
                    .store
                    .create_message(NewMessage {
                        goal_id: task.goal_id.clone(),
                        task_id: Some(task.id.clone()),
                        author_role: AuthorRole::System,
                        author_session_id: None,
                        recipient: None,
                        body: format!(
                            "Round {} of \"{}\" is approved. Its integrator takes the \
                             branch from here, and the engineer's worktree with it.",
                            task.review_round, task.title,
                        ),
                    })
                    .await;
                self.store
                    .transition_task(&task.id, TaskStatus::Integrating, Actor::Daemon, None, None)
                    .await?;
                // The integrator is started by the arm below, on the status it
                // belongs to: one place decides what an integrating task wants,
                // whether it got here from an approval or from a daemon that
                // restarted halfway through one.
                return Box::pin(self.reconcile_task(task_id)).await;
            }
            TaskStatus::Integrating => match forge::watched_pull_request(&task) {
                // A published task is waiting on people, not on its
                // integrator: the pull request is watched instead, and the
                // idle agent that opened it is left alone rather than nudged
                // for not having landed anything.
                Some(pr) => self.watch_pull_request(&task, &pr).await?,
                None => self.check_stall(&task, Role::Integrator).await?,
            },
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
    /// `role` is whose turn it is: the engineer while the task is being
    /// written, the integrator once it has been approved and is being landed.
    /// A task with no live session of that role gets one started; one that is
    /// idle too long gets exactly one tmux nudge per (status, round), and if it
    /// stays idle the task is flagged stalled for the user (never an endless
    /// loop) and the session says why.
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
        let nudge = match role {
            Role::Integrator => {
                "Land this task as the integration instructions say: rebase, squash, fast-forward the base and call `mark_merged` with the resulting sha — or, if the rebase conflicts, abort it and call `return_to_engineer` with the conflicting files."
            }
            _ => {
                "Keep working on this task, and call `request_review` with a summary once the work is complete and verified."
            }
        };
        let stalled = self
            .check_session_stall(agent, (task.status.clone(), task.review_round), nudge)
            .await?;
        // Whoever owns the task right now is the one role whose stall has
        // somewhere else to show: the task carries a flag of its own, next to
        // the session's.
        if stalled && !task.is_stalled() {
            warn!(task = %task.id, role = role.as_str(), "task stalled, flagging for user attention");
            self.store.set_task_stalled(&task.id, true).await?;
        }
        Ok(())
    }

    /// Watch the pull or merge request a task was published as, and wake
    /// whoever the humans on it have given something to do.
    ///
    /// Nothing here is the integrator's to be nudged about: the review moves
    /// when a person reads it, which is why the integrator ends its turn after
    /// opening one and why this arm replaces the stall watch rather than
    /// running beside it. What it does instead is look every `pr_poll_secs`
    /// and act on what the forge's CLI says:
    ///
    /// - **merged**, and the integrator finishes the task locally;
    /// - **commented on**, and every comment nobody has relayed yet is
    ///   written straight to the engineer as a round of requested changes —
    ///   no agent in between (see [`Self::relay_pr_feedback`]);
    /// - **conflicting** or **failing its checks**, and what the forge said
    ///   goes the same way, to the same engineer, once each;
    /// - **approved**, green and still merging, and the user is told once
    ///   that it is theirs to merge.
    ///
    /// They read the same on either forge; which CLI answers for them is
    /// [`Self::poll_forge`]'s to decide.
    ///
    /// An integrator mid-turn is left to finish it: resuming an agent means
    /// relaunching its pane, and whatever it is doing on the branch right now
    /// is more current than a poll taken a moment ago. The next poll asks
    /// again.
    ///
    /// The poll comes before any of that, and before an integrator is started
    /// for a task that has none — a daemon that restarts on a request with
    /// comments waiting on it hands them to the engineer on that first pass,
    /// rather than standing an agent up for a task that is leaving
    /// `integrating` in the same breath. One is started once the poll has
    /// said the task is still the integrator's.
    async fn watch_pull_request(&mut self, task: &Task, watched: &WatchedPr) -> anyhow::Result<()> {
        let number = watched.number;
        // Whichever integrator is on the task already, asked for rather than
        // started: what the poll below says may be that the task is leaving
        // `integrating` altogether, and an agent started for that is an agent
        // started to be killed. Starting one is what the tail of this does,
        // once the poll has said the task is still the integrator's.
        let integrator = self.integrator_session(task).await?;
        // The one watchdog that still applies: an agent whose instruction is
        // sitting unsubmitted in its composer has not started the turn this
        // arm woke it for, and no amount of polling will move it. The idle
        // half of the stall watch is what does not apply — an integrator with
        // a review open is waiting on people, not stalling.
        if let Some(running) = integrator
            .as_ref()
            .filter(|s| s.status() == SessionStatus::Running)
        {
            self.check_unstarted_turn(running).await?;
        }
        let interval = std::time::Duration::from_secs(self.launcher.cfg.pr_poll_secs);
        let now = std::time::Instant::now();
        if self
            .pr_polled
            .get(&task.id)
            .is_some_and(|last| now.duration_since(*last) < interval)
        {
            return Ok(());
        }
        self.pr_polled.insert(task.id.clone(), now);

        let repo = self.store.get_repository(&task.repo_id).await?;
        let repo_path = std::path::PathBuf::from(&repo.path);
        let poll = match self
            .poll_forge(
                &task.id,
                &repo_path,
                watched,
                &task.pr_relayed_comments(),
                task.pr_approved_notified(),
            )
            .await
        {
            Ok(poll) => poll,
            // Nothing was read at all: there is no state to act on, and the
            // only thing this poll has to say is that it failed.
            Err(e) => {
                return self
                    .note_poll_failure(task, integrator.as_ref(), watched, &format!("{e:#}"))
                    .await;
            }
        };
        match poll.failure.clone() {
            Some(error) => {
                self.note_poll_failure(task, integrator.as_ref(), watched, &error)
                    .await?;
            }
            None => {
                self.note_poll_success(task, integrator.as_ref(), watched)
                    .await?;
            }
        }
        // An approval that was dismissed or overtaken by a new review is one
        // the user has to be told about again when it comes back. A poll that
        // could not tell is not one withdrawn, so it leaves the flag alone —
        // and neither is a poll one of whose reads failed: what came back of
        // it is half an answer, and half an answer withdraws nothing.
        if poll.failure.is_none() && poll.approved == Some(false) && task.pr_approved_notified() {
            self.store
                .set_task_pr_approved_notified(&task.id, false)
                .await?;
        }
        if poll.state != PrState::Quiet
            && let Some(working) = integrator
                .as_ref()
                .filter(|s| s.status() != SessionStatus::Idle)
        {
            info!(task = %task.id, pr = number, session = %working.id, "the review moved while its integrator is working; leaving it to finish");
            return Ok(());
        }
        // A request closed without being merged is the end of the task, and
        // the one thing a poll can say that no later poll will take back:
        // nothing in Ariadne reopens one, and nobody is coming to press the
        // button. Read as quiet — as it was — the task sat in `integrating`
        // being polled every few minutes with nothing to show for it, and no
        // stall watch running either, since this arm replaces it.
        if poll.state == PrState::Closed {
            info!(task = %task.id, pr = number, "the review was closed without being merged; failing the task");
            return self.fail_on_closed_request(task, watched).await;
        }
        // Comments wake nobody: they are the engineer's to answer, and the
        // task is about to be its own again. Whether an integrator is running
        // makes no difference to that — a task with none is exactly the case
        // a daemon restart leaves behind, and starting one here only to have
        // the transition kill it is the hop this whole arm exists to spare.
        //
        // A branch that stopped merging or stopped building goes the same
        // way, and for the same reason: the fix is a commit, commits are the
        // engineer's, and an integrator asleep over a published request would
        // only discover either of them the next time somebody woke it.
        match &poll.state {
            PrState::Feedback(feedback) => {
                info!(task = %task.id, pr = number, comments = feedback.len(), "the review was commented on; sending it to the engineer");
                self.relay_pr_feedback(task, watched, feedback).await?;
                return Box::pin(self.reconcile_task(&task.id)).await;
            }
            PrState::Conflicting(conflict) => {
                info!(task = %task.id, pr = number, "the branch no longer merges into its base; sending it to the engineer");
                let base = conflict_base(conflict, &repo);
                self.relay_to_engineer(
                    task,
                    pr_conflict_review(watched, &base),
                    std::slice::from_ref(&conflict.id),
                    &format!("{} no longer merges into {base}", watched.label()),
                )
                .await?;
                return Box::pin(self.reconcile_task(&task.id)).await;
            }
            PrState::ChecksFailed(checks) => {
                info!(task = %task.id, pr = number, checks = checks.len(), "the checks on the review are failing; sending it to the engineer");
                let ids: Vec<String> = checks.iter().map(|c| c.id.clone()).collect();
                self.relay_to_engineer(
                    task,
                    pr_checks_review(watched, checks),
                    &ids,
                    &format!("{}'s checks are failing", watched.label()),
                )
                .await?;
                return Box::pin(self.reconcile_task(&task.id)).await;
            }
            // Read before this match, and never reaching it: the closed
            // request ends the task rather than sending it anywhere.
            PrState::Closed | PrState::Quiet | PrState::Merged | PrState::Approved => {}
        }

        // The rest is the integrator's, and so is a quiet review: a published
        // task being integrated keeps an agent on it, started here when a
        // restart or a send-back left none. That is what pushes the next
        // revision to the request, and what a merge is finished by.
        if self.live_integrator(task, integrator).await?.is_none() {
            // A task back from the engineer, or a daemon that restarted: the
            // integrator is started with its resume briefing, which tells it
            // to update the review it already opened.
            return Ok(());
        }
        match poll.state {
            // All handled above: the three that are the engineer's, and the
            // one that ends the task.
            PrState::Feedback(_)
            | PrState::Conflicting(_)
            | PrState::ChecksFailed(_)
            | PrState::Closed => {}
            // A quiet review asks nothing of anybody. The user was told once
            // that the request is theirs — when it was opened, and again when
            // it was approved — and the flag that went up with each of those
            // is theirs to take down: no agent event clears a `waiting_user`,
            // so there is nothing here to put back.
            PrState::Quiet => {}
            PrState::Merged => {
                info!(task = %task.id, pr = number, "the review was merged; waking the integrator to finish the task");
                self.launcher
                    .resume_integrator(&task.id, &pr_merged_instruction(watched, &repo.base_branch))
                    .await?;
                self.spawn_failures.remove(&task.id);
            }
            PrState::Approved => {
                info!(task = %task.id, pr = number, "the review is approved; telling the user it is theirs to merge");
                self.notify_pull_request_approved(task, watched).await?;
            }
        }
        Ok(())
    }

    /// End a task whose request was closed without being merged, and tell the
    /// user what became of it.
    ///
    /// The transition comes first, and that ordering is what keeps the user to
    /// one message: a task still `integrating` is polled again in
    /// `pr_poll_secs`, and a second poll of the same closed request would say
    /// the same thing twice. Once the task is `failed` nothing polls it again,
    /// so a daemon that dies between the two writes loses the message rather
    /// than repeating it — and the transition it kept is the one the user can
    /// see and act on.
    ///
    /// What to do about it is theirs, which is why the message says both
    /// halves: retrying the task puts the engineer back on the same branch and
    /// the recorded request is cleared as it goes ready again, so its next
    /// integrator publishes afresh rather than pushing to a request nobody
    /// will merge; cancelling it keeps the branch and the worktree for
    /// whatever is worth salvaging.
    async fn fail_on_closed_request(
        &mut self,
        task: &Task,
        watched: &WatchedPr,
    ) -> anyhow::Result<()> {
        let label = watched.label();
        let noun = watched.forge.noun();
        self.store
            .transition_task(
                &task.id,
                TaskStatus::Failed,
                Actor::Daemon,
                Some(&format!("{label} was closed without being merged")),
                None,
            )
            .await?;
        // This ending writes its own notice rather than the one
        // [`notify::task_ended`] writes for the others, and instead of it:
        // what the user needs is the request itself — the URL, and that
        // retrying publishes a fresh one — and two notices for one ending is
        // the noise every ending here is written once to avoid. It is
        // delivered like any other, so it goes up the attention path the user
        // reads such things from.
        let notice = self
            .store
            .create_message(NewMessage {
                goal_id: task.goal_id.clone(),
                task_id: Some(task.id.clone()),
                author_role: AuthorRole::System,
                author_session_id: None,
                recipient: Some(Recipient::User),
                body: format!(
                    "{label} for \"{title}\" was closed without being merged: {url}\n\n\
                     Nothing is going to merge it now, so the task is failed. Retry it and \
                     the engineer picks the branch up where it left off, with a fresh \
                     {noun} published for it; cancel it if the work is not wanted after \
                     all — the branch and the worktree are kept either way.",
                    title = task.title,
                    url = watched.url,
                ),
            })
            .await?;
        self.deliver_message(&notice.id).await;
        // The agents on it are stood down the way a finished task stands them
        // down, worktree and branch kept: a retry puts the engineer back on
        // that branch, and a task nobody retries may still hold work worth
        // reading.
        self.launcher.cleanup_task(&task.id, false, false).await?;
        self.pr_polled.remove(&task.id);
        self.poll_failures.remove(&task.id);
        Ok(())
    }

    /// Count one poll that could not read the request, and tell the user once
    /// it has happened [`POLL_FAILURE_LIMIT`] times in a row.
    ///
    /// A single failure says nothing worth waking anybody for: forges have bad
    /// minutes, laptops change networks, tokens are refreshed. A run of them
    /// says the watch itself is broken — a `gh` that is not installed on the
    /// daemon's PATH, or one nobody ever signed in — and that is invisible
    /// from the outside, because a published task looks exactly the same
    /// whether it is being watched or not.
    ///
    /// Exactly at the threshold, so it is said once however long the CLI stays
    /// broken; the attention flag beside it is what puts the task on the strip
    /// the user reads such things from.
    async fn note_poll_failure(
        &mut self,
        task: &Task,
        integrator: Option<&AgentSession>,
        watched: &WatchedPr,
        error: &str,
    ) -> anyhow::Result<()> {
        let failures = self.poll_failures.entry(task.id.clone()).or_insert(0);
        *failures += 1;
        let failures = *failures;
        warn!(task = %task.id, pr = watched.number, failures, error, "reading the review failed");
        if failures != POLL_FAILURE_LIMIT {
            return Ok(());
        }
        let cli = watched.forge.cli();
        self.store
            .create_message(NewMessage {
                goal_id: task.goal_id.clone(),
                task_id: Some(task.id.clone()),
                author_role: AuthorRole::System,
                author_session_id: None,
                recipient: Some(Recipient::User),
                body: format!(
                    "Ariadne cannot read the {noun} for \"{title}\": `{cli}` has failed \
                     {failures} polls in a row.\n\n{error}\n\n\
                     {label} ({url}) is not being watched while that lasts — nothing \
                     written on it reaches the engineer, and its merge would go \
                     unnoticed. Check that `{cli}` is installed where the daemon can \
                     run it and signed in ({cli} auth status; `ariadne doctor` reports \
                     both), and the watch picks up again by itself.",
                    noun = watched.forge.noun(),
                    title = task.title,
                    label = watched.label(),
                    url = watched.url,
                ),
            })
            .await?;
        // On the integrator's session because that is the one this task shows
        // on the strip: the agent itself is fine, and what is broken is the
        // daemon's own reading of the request it is waiting on.
        //
        // Under its own reason rather than the `waiting_user` a delivered
        // notice raises, because the two are cleared by different people: a
        // notice is up until the user has read it, and this is up until the
        // CLI works again, which the daemon is the one that finds out (see
        // [`Self::note_poll_success`]).
        if let Some(integrator) = integrator {
            self.store
                .set_session_attention(&integrator.id, AttentionReason::AgentError)
                .await?;
        }
        Ok(())
    }

    /// Forget the failed polls, and say so if the user was ever told about
    /// them.
    ///
    /// Only if they were told: a run of failures that never reached the
    /// threshold is nothing anybody heard about, and announcing its end would
    /// be news about news. The flag comes down with the message, and only the
    /// one this raised — an approval waiting on the user is a different flag
    /// on the same session, and it stays up.
    async fn note_poll_success(
        &mut self,
        task: &Task,
        integrator: Option<&AgentSession>,
        watched: &WatchedPr,
    ) -> anyhow::Result<()> {
        let Some(failures) = self.poll_failures.remove(&task.id) else {
            return Ok(());
        };
        if failures < POLL_FAILURE_LIMIT {
            return Ok(());
        }
        info!(task = %task.id, pr = watched.number, failures, "reading the review works again");
        self.store
            .create_message(NewMessage {
                goal_id: task.goal_id.clone(),
                task_id: Some(task.id.clone()),
                author_role: AuthorRole::System,
                author_session_id: None,
                recipient: Some(Recipient::User),
                body: format!(
                    "`{cli}` can read the {noun} for \"{title}\" again: {label} ({url}) \
                     is being watched as before, and whatever was written on it while it \
                     was not is read now.",
                    cli = watched.forge.cli(),
                    noun = watched.forge.noun(),
                    title = task.title,
                    label = watched.label(),
                    url = watched.url,
                ),
            })
            .await?;
        if let Some(integrator) =
            integrator.filter(|s| s.attention_reason() == Some(AttentionReason::AgentError))
        {
            self.store.clear_session_attention(&integrator.id).await?;
        }
        Ok(())
    }

    /// Hand what the humans wrote to the engineer, without waking anybody in
    /// between.
    ///
    /// The daemon polled the comments and has them in hand: relaying them
    /// through the integrator would be a turn spent copying them from one
    /// forge into the other end of the same database, and a turn is a place
    /// a relay can go wrong. So the send-back is written by the daemon itself
    /// ([`Self::relay_to_engineer`]), and the engineer is resumed with it by
    /// the `changes_requested` arm like any other round of feedback. Its
    /// worktree is checked out again as it resumes, which is what releases
    /// the integrator's hold on the branch.
    ///
    /// What that send-back is written by, for comments as for everything else
    /// a poll can send the task back for, is [`Self::relay_to_engineer`].
    async fn relay_pr_feedback(
        &mut self,
        task: &Task,
        watched: &WatchedPr,
        feedback: &[Feedback],
    ) -> anyhow::Result<()> {
        let ids: Vec<String> = feedback.iter().map(|f| f.id.clone()).collect();
        self.relay_to_engineer(
            task,
            pr_feedback_review(watched, feedback),
            &ids,
            &format!("{} was commented on", watched.label()),
        )
        .await
    }

    /// The send-back itself, whatever the review said to send the task back
    /// for: what the humans wrote, a branch that no longer merges, a check
    /// that failed.
    ///
    /// `ids` are what the poll read it from, remembered on the task so that
    /// the same comment, the same conflict and the same failing check are one
    /// round of changes rather than one per poll.
    ///
    /// The row is one of the ones `return_to_engineer` writes for the
    /// integrator's own send-backs — a change request on the round the
    /// request was published from — but under the forge's own name rather
    /// than any profile's: what it carries is what the people reading the
    /// request wrote, or what their checks said, and no agent of ours has
    /// read a word of it.
    ///
    /// The three records it is made of go down together, in one store write:
    /// the round of changes the engineer is resumed with, the ids that keep
    /// any of it from being relayed into a second round, and the transition
    /// that wakes it. Written one after another, as they were, a daemon that
    /// failed in between left something no later poll could put right — a
    /// failure marked relayed into a round that was never opened, or a second
    /// round of what the forge said once — so the store writes all three or
    /// none of them ([`Store::relay_pull_request_feedback`]), and a failure
    /// leaves the task where the next poll expects it: still `integrating`,
    /// with nothing relayed.
    async fn relay_to_engineer(
        &mut self,
        task: &Task,
        body: String,
        ids: &[String],
        reason: &str,
    ) -> anyhow::Result<()> {
        self.store
            .relay_pull_request_feedback(
                NewReview {
                    task_id: task.id.clone(),
                    round: task.review_round,
                    author: ReviewAuthor::Role(AuthorRole::Forge),
                    session_id: None,
                    verdict: ReviewVerdict::RequestChanges,
                    body: Some(body),
                },
                ids,
                reason,
            )
            .await?;
        Ok(())
    }

    /// Approve the revision of a published task without a review round of
    /// our own.
    ///
    /// Once a pull or merge request is open, the people reading it are the
    /// reviewers: they asked for the change, the engineer answered them, and
    /// the only thing standing between that answer and them is the push that
    /// carries it. Putting the reviewer profiles through a round of their own
    /// first would hold the answer back for a verdict nobody on the request
    /// is waiting for — so the round is closed here instead, with no reviewer
    /// started and no review row wanted.
    ///
    /// Written down once, on the transition that closes the round: the round
    /// was decided, not announced, and the reason a transition carries is
    /// what both the audit and the task's history tab read it from.
    async fn approve_published_revision(
        &self,
        task: &Task,
        watched: &WatchedPr,
    ) -> anyhow::Result<()> {
        let label = watched.label();
        info!(task = %task.id, pr = watched.number, round = task.review_round, "the revision answers a published request: approving it without an internal review round");
        self.store
            .transition_task(
                &task.id,
                TaskStatus::Approved,
                Actor::Daemon,
                Some(&format!(
                    "{label} is published: its reviewers replace the internal review round"
                )),
                None,
            )
            .await?;
        Ok(())
    }

    /// One look at the review, through the CLI of the forge it is on.
    ///
    /// A CLI that cannot answer — not installed, not authenticated, the
    /// network down — is not a reason to fail the task: the review is still
    /// there, and the next poll asks again. That is the `Err`, and what it
    /// carries is what the caller counts and eventually tells the user about
    /// ([`Self::note_poll_failure`]), because a watch that reads nothing is
    /// indistinguishable from a watch on a request nobody is touching.
    ///
    /// The two forges differ in how many reads one look takes and in nothing
    /// else: GitHub keeps what was written on the diff away from the pull
    /// request itself, and GitLab keeps the approvals away from the merge
    /// request. Either way what comes back is the same reading — with
    /// [`Poll::failure`] set when it is only part of one.
    async fn poll_forge(
        &self,
        task_id: &str,
        repo_path: &std::path::Path,
        watched: &WatchedPr,
        relayed: &[String],
        approved_notified: bool,
    ) -> anyhow::Result<Poll> {
        let number = watched.number;
        match watched.forge {
            Forge::GitHub => {
                let gh_cli = self.launcher.gh();
                let pr = gh_cli
                    .pr_view(repo_path, watched)
                    .await
                    .with_context(|| format!("reading pull request {number}"))?;
                // What was written on the diff rather than in the
                // conversation, which is where most review feedback lives.
                // Failing to read them is not worth dropping the poll for —
                // the conversation may be carrying something too, and the next
                // poll asks for both again — but it is worth saying so: a
                // comment nobody could read is not a pull request nobody
                // commented on.
                let mut failure = None;
                let review_comments = match gh_cli.pr_review_comments(repo_path, watched).await {
                    Ok(comments) => comments,
                    Err(e) => {
                        let error = format!("{e:#}");
                        warn!(task = %task_id, pr = number, error, "reading the pull request's review comments failed");
                        failure = Some(error);
                        Vec::new()
                    }
                };
                // Approved *and* ready: a branch that is red, conflicting or
                // still building is nobody's to press a button on, however
                // many people signed it off.
                let approved = pr.is_approved() && pr.health().is_ready();
                Ok(Poll {
                    approved: Some(approved),
                    state: gh::poll_state(
                        &pr,
                        &review_comments,
                        relayed,
                        approved && approved_notified,
                    ),
                    failure,
                })
            }
            Forge::GitLab => {
                let glab_cli = self.launcher.glab();
                let mr = glab_cli
                    .mr_view(repo_path, watched)
                    .await
                    .with_context(|| format!("reading merge request {number}"))?;
                // Approvals and discussions are their own resources on
                // GitLab. One of them failing leaves the other still worth
                // acting on, so the poll goes on with what it has — and an
                // approval it could not read is reported as unknown rather
                // than as withdrawn.
                let mut failure = None;
                let (approvals, approved) = match glab_cli.mr_approvals(repo_path, watched).await {
                    Ok(approvals) => {
                        // Approved *and* ready: a branch that is red,
                        // conflicting or still building is nobody's to press
                        // a button on, however many people signed it off.
                        let approved =
                            approvals.is_approved() && mr.is_open() && mr.health().is_ready();
                        (approvals, Some(approved))
                    }
                    Err(e) => {
                        let error = format!("{e:#}");
                        warn!(task = %task_id, mr = number, error, "reading the merge request's approvals failed");
                        failure = Some(error);
                        (glab::Approvals::default(), None)
                    }
                };
                let discussions = match glab_cli.mr_discussions(repo_path, watched).await {
                    Ok(discussions) => discussions,
                    Err(e) => {
                        let error = format!("{e:#}");
                        warn!(task = %task_id, mr = number, error, "reading the merge request's discussions failed");
                        failure = failure.or(Some(error));
                        Vec::new()
                    }
                };
                Ok(Poll {
                    approved,
                    state: glab::poll_state(
                        &mr,
                        &approvals,
                        &discussions,
                        relayed,
                        approved.unwrap_or(false) && approved_notified,
                    ),
                    failure,
                })
            }
        }
    }

    /// The integrator on this task, if one is live — asked, never started.
    ///
    /// What a poll does about a review depends on whether an agent is already
    /// working on it, and that question has to be answerable without starting
    /// one: a review with comments on it is answered by the engineer, and the
    /// task leaves `integrating` without an integrator being wanted at all.
    async fn integrator_session(&self, task: &Task) -> anyhow::Result<Option<AgentSession>> {
        let sessions = self
            .store
            .list_sessions(SessionFilter {
                task_id: Some(task.id.clone()),
                live_only: true,
                ..Default::default()
            })
            .await?;
        Ok(sessions.into_iter().find(|s| s.role() == Role::Integrator))
    }

    /// The integrator working on this task, started if there is none.
    ///
    /// `found` is what [`Self::integrator_session`] already answered this
    /// pass, so the question is not asked twice. `None` means one was just
    /// started and there is nothing else to decide this pass: an agent
    /// resuming is about to read the branch and the pull request for itself.
    async fn live_integrator(
        &mut self,
        task: &Task,
        found: Option<AgentSession>,
    ) -> anyhow::Result<Option<AgentSession>> {
        if let Some(integrator) = found {
            return Ok(Some(integrator));
        }
        info!(task = %task.id, "no live integrator for a task with a pull request, starting one");
        if let Err(e) = self.start_role(task, Role::Integrator).await {
            self.flag_last_disconnected(task, Role::Integrator).await;
            return Err(e);
        }
        self.spawn_failures.remove(&task.id);
        Ok(None)
    }

    /// Tell the user their pull request is ready to merge: a message in the
    /// task thread addressed to them, which the delivery path puts on the
    /// attention strip they read it from.
    ///
    /// Once per approval — `pr_approved_notified` is what says it has been
    /// done, and it is written before the message so a failure repeats
    /// nothing.
    async fn notify_pull_request_approved(
        &mut self,
        task: &Task,
        watched: &WatchedPr,
    ) -> anyhow::Result<()> {
        self.store
            .set_task_pr_approved_notified(&task.id, true)
            .await?;
        let noun = watched.forge.noun();
        let where_ = task.pr_url.clone().unwrap_or_else(|| watched.label());
        let msg = self
            .store
            .create_message(NewMessage {
                goal_id: task.goal_id.clone(),
                task_id: Some(task.id.clone()),
                author_role: AuthorRole::System,
                author_session_id: None,
                recipient: Some(Recipient::User),
                body: format!(
                    "The {noun} for \"{}\" is approved and ready for you to merge: {where_}\n\n\
                     Merging it is yours to do — Ariadne watches the {noun} and finishes \
                     the task once you have.",
                    task.title,
                ),
            })
            .await?;
        self.deliver_message(&msg.id).await;
        Ok(())
    }

    /// Put the agent the task is waiting on back on its feet: the same session
    /// resumed where there is one to resume, a fresh spawn otherwise (both
    /// launcher calls fall back to the spawn themselves).
    async fn start_role(&mut self, task: &Task, role: Role) -> anyhow::Result<()> {
        match role {
            Role::Integrator => {
                let repo = self.store.get_repository(&task.repo_id).await?;
                // A published task is never picked up from scratch: the
                // request is open, the branch carries a revision the engineer
                // wrote for the people reading it, and both what to do with
                // it and what to tell them are the daemon's to say. The
                // stored resume briefing is for the other case — a task whose
                // landing nobody has started yet.
                let instruction = match forge::watched_pull_request(task) {
                    Some(watched) => published_revision_instruction(
                        &watched,
                        task,
                        &repo,
                        self.store.review_summary(&task.id).await?.as_deref(),
                    ),
                    None => {
                        let profile = self.launcher.integrator_profile(task).await?;
                        let template = prompts::template_for(
                            &self.store,
                            &profile.id,
                            PromptKind::IntegrationResume,
                        )
                        .await;
                        prompts::integration_resume_briefing(&template, task, &repo)
                    }
                };
                self.launcher
                    .resume_integrator(&task.id, &instruction)
                    .await?;
            }
            _ => {
                self.launcher
                    .resume_engineer(&task.id, "Your previous session ended: continue this task on the same branch in your worktree, and call `request_review` when the work is complete and verified.")
                    .await?;
            }
        }
        Ok(())
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

    /// One agent that is not getting on with the work in front of it.
    ///
    /// There are two ways to be doing nothing and they want opposite
    /// remedies. An idle agent finished a turn and stopped: it is measured
    /// against the stall thresholds below — a single nudge at
    /// [`STALL_NUDGE_SECS`], and at [`STALL_FLAG_SECS`] the session is raised
    /// for the user. Returns whether it crossed that second threshold, which
    /// is all the caller needs to decide about the work itself. A running one
    /// that never started a turn is stuck holding its instruction, and what
    /// that needs is a keystroke rather than another message — see
    /// [`Self::check_unstarted_turn`], which has thresholds of its own and
    /// nothing to say about the task.
    ///
    /// `key` is the situation the nudge is spent on — the status and round the
    /// agent was idle in — so moving on earns a fresh one.
    async fn check_session_stall(
        &mut self,
        session: &AgentSession,
        key: (String, i64),
        nudge: &str,
    ) -> anyhow::Result<bool> {
        if session.status() == SessionStatus::Running {
            self.check_unstarted_turn(session).await?;
            return Ok(false);
        }
        if session.status() != SessionStatus::Idle {
            return Ok(false);
        }
        // An agent waiting on a person is not stalled, it is blocked, and the
        // attention entry already says so. Nudging it would type into whatever
        // it is waiting on — a permission prompt takes Enter for an answer —
        // so neither the keystroke nor the escalation behind it applies here.
        if matches!(
            session.attention_reason(),
            Some(AttentionReason::WaitingPermission | AttentionReason::WaitingInput)
        ) {
            return Ok(false);
        }
        let Some(last) = &session.last_activity_at else {
            return Ok(false);
        };
        let Ok(last) = chrono::DateTime::parse_from_rfc3339(last) else {
            return Ok(false);
        };
        // A confirmed delivery is the freshest thing that happened to this
        // agent, and it is a nudge in its own right: whichever of the two is
        // later is what the idle time is counted from, so an agent told
        // something a moment ago is not asked why it has stopped.
        let mut since = last.with_timezone(&chrono::Utc);
        if let Some(delivered) = self.delivered_at.get(&session.id) {
            since = since.max(*delivered);
        }
        let idle_secs = (chrono::Utc::now() - since).num_seconds();
        let already_nudged = self.nudged.get(&session.id) == Some(&key);

        if idle_secs >= STALL_FLAG_SECS && already_nudged {
            warn!(session = %session.id, role = %session.role, idle_secs, "session stalled, flagging for user attention");
            self.store
                .set_session_attention(&session.id, AttentionReason::Stalled)
                .await?;
            return Ok(true);
        }
        if idle_secs >= STALL_NUDGE_SECS && !already_nudged {
            // A pane already being typed into is being nudged by that: this
            // one waits for the next pass rather than interleaving with it.
            if self.typing.contains(&session.id) {
                return Ok(false);
            }
            info!(session = %session.id, role = %session.role, idle_secs, "nudging idle agent");
            // Spent as the delivery goes out, and off the loop: a pane that
            // takes the nudge and will not submit it is raised for the user
            // rather than nudged again, and one tmux would not take at all
            // gives the nudge back — see [`Self::delivery_settled`].
            self.nudged.insert(session.id.clone(), key);
            self.spawn_delivery(session, nudge.to_string(), None);
        }
        Ok(false)
    }

    /// One launched agent that never started its turn: an Enter at
    /// [`UNSTARTED_ENTER_SECS`], and the user at [`UNSTARTED_FLAG_SECS`].
    ///
    /// The instruction a resume carries can end up composed but unsent — an
    /// Enter the TUI swallowed, or `codex resume <thread> <instruction>`,
    /// which hands the prompt to the composer through argv and leaves it
    /// there for somebody to submit. The agent then runs no turn and reports
    /// nothing, and since the launch marked it `running` it stays that way for
    /// ever: the idle stall never looks at a running session, so nothing else
    /// in the daemon can see this at all. What a human does on finding such a
    /// pane is press Enter, so that is what happens here — it submits a held
    /// message and does nothing to an empty composer. If the turn has still
    /// not started a threshold later, the session is raised instead: the
    /// keystroke was not the answer and only a person can say why.
    ///
    /// "Started" is read off what the session reported since its launch, and
    /// only [`TURN_ACTIVITY`] counts — lifecycle events fire on a TUI that is
    /// merely up.
    async fn check_unstarted_turn(&mut self, session: &AgentSession) -> anyhow::Result<()> {
        // An agent waiting on a person is blocked, not stuck: an Enter into a
        // pane holding a dialog answers it, which is the one thing the daemon
        // must not decide (same reasoning as the idle nudge above). An agent
        // that reported an error is already asking for the user by name, and
        // overwriting that reason with a stall would take away the more
        // useful half of what it said.
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
        // A launch nobody dated is a launch nothing is concluded from.
        let Some(launched) = session.launched_at.clone() else {
            return Ok(());
        };
        let Ok(at) = chrono::DateTime::parse_from_rfc3339(&launched) else {
            return Ok(());
        };
        let silent_secs = (chrono::Utc::now() - at.with_timezone(&chrono::Utc)).num_seconds();
        if silent_secs < UNSTARTED_ENTER_SECS {
            return Ok(());
        }
        if self
            .store
            .session_reported_since(&session.id, &launched, &TURN_ACTIVITY)
            .await?
        {
            return Ok(());
        }
        // Only what was done for *this* launch counts: a relaunched session
        // is a fresh instruction that may be stuck in its own right.
        let done = self
            .unstarted
            .get(&session.id)
            .filter(|(l, _)| *l == launched)
            .map(|(_, step)| *step);

        if silent_secs >= UNSTARTED_FLAG_SECS && done == Some(Unstarted::EnterPressed) {
            warn!(session = %session.id, role = %session.role, silent_secs, "the agent never started its turn, flagging for user attention");
            self.store
                .set_session_attention(&session.id, AttentionReason::Stalled)
                .await?;
            self.unstarted
                .insert(session.id.clone(), (launched, Unstarted::Flagged));
            return Ok(());
        }
        if done.is_none() {
            info!(session = %session.id, role = %session.role, silent_secs, "no turn since the launch, pressing Enter into the pane");
            // Spent whether or not tmux took it, exactly as the nudge is: a
            // pane that refused the keystroke this pass will refuse the next.
            self.unstarted
                .insert(session.id.clone(), (launched, Unstarted::EnterPressed));
            self.launcher.tmux.send_enter(&session.tmux_session).await?;
        }
        Ok(())
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
                    // This agent has just been told what to do: the idle
                    // clock starts again here and the nudge that may have
                    // been spent comes back, so that nothing tells it to get
                    // on with what it was asked to do a moment ago.
                    self.nudged.remove(&report.session_id);
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
                    self.nudged.remove(&report.session_id);
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

    /// The live session a task's own notices are raised on: its integrator
    /// while one is landing it, its engineer otherwise, and the most recent of
    /// either.
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
        Ok([Role::Integrator, Role::Engineer]
            .into_iter()
            .find_map(|role| sessions.iter().rev().find(|s| s.role() == role).cloned()))
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
        let text = delivery_text(message);
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

/// What one poll of a review came back with.
struct Poll {
    /// Whether the forge says nothing stands between it and its merge right
    /// now — approved, still merging into its base, and with every check it
    /// has finished and green — or `None` when this poll could not tell,
    /// since an answer that never came is not an approval withdrawn.
    ///
    /// The state of the branch is part of it because the notice it drives is
    /// "ready for you to merge": a branch that went red, stopped merging or
    /// is building the revision somebody just pushed is not that, and the
    /// flag comes back down until it is all three again.
    approved: Option<bool>,
    state: PrState,
    /// What a read of the request failed with, when one of the reads a look
    /// takes did.
    ///
    /// A request is more than one resource on either forge — what was written
    /// on the diff is its own read on GitHub, the approvals are on GitLab —
    /// and one of them failing leaves the rest worth acting on. What it does
    /// not leave is a quiet review: a comment nobody could read looks exactly
    /// like a comment nobody wrote, so a look that came back in pieces says
    /// so, and the counting that tells the user about a broken watch counts
    /// it like any other failure.
    failure: Option<String>,
}

/// What the humans wrote on the review, as the round of requested changes the
/// engineer is sent back with.
///
/// Quoted verbatim rather than pointed at, for the reason a delivered message
/// is quoted whole: an agent sent to go and read what it was woken for has
/// been woken for nothing — and here there is nothing to send it to, since
/// the branch is on a forge and the comments were read by the daemon. Every
/// entry carries who wrote it, and where on the diff it hangs when that is
/// where it was written.
///
/// What to do with them is spelled out here rather than in the engineer's
/// briefing, because it is particular to a published request: the commits
/// people are reading stay where they are, and the answer to each comment
/// goes into the summary the engineer hands back.
fn pr_feedback_review(watched: &WatchedPr, feedback: &[Feedback]) -> String {
    let quoted = feedback
        .iter()
        .map(|f| {
            let what = if f.blocking {
                "requested changes"
            } else {
                "commented"
            };
            let at = match &f.file {
                Some(file) => format!(" on {file}"),
                None => String::new(),
            };
            let body = f
                .body
                .trim()
                .lines()
                .map(|line| format!("> {line}").trim_end().to_string())
                .collect::<Vec<_>>()
                .join("\n");
            format!("### {} {what}{at}\n{body}", f.author)
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    let count = feedback.len();
    let plural = if count == 1 { "comment" } else { "comments" };
    let label = watched.label();
    let noun = watched.forge.noun();
    format!(
        "{label} ({url}) has {count} new {plural} from the humans reviewing it:\n\n\
         {quoted}\n\n\
         Every one of them is yours to answer: change the code where it asks for a change, and \
         where it does not — a question, a suggestion you disagree with — say why the code stays \
         as it is. The {noun} is published and people are reading the commits on it, so add new \
         commits on top of them: no `commit --amend`, no rebase, no forced push over what they \
         have already seen. Then call `request_review` with a summary that replies to every \
         comment above, naming its author the way this does — that summary is what they are \
         answered with.",
        url = watched.url,
    )
}

/// The rule every send-back on a published request carries: the commits
/// people are reading stay where they are, so whatever answers the request is
/// a commit on top of them.
fn published_branch_rule(watched: &WatchedPr) -> String {
    format!(
        "The {noun} is published and people are reading the commits on it, so the branch only \
         ever grows: add new commits on top of what is already there — no `commit --amend`, no \
         rebase, no forced push over what they have already seen.",
        noun = watched.forge.noun(),
    )
}

/// The branch a conflict is with: the one the forge named it against, and the
/// repository's own base where the forge named none.
fn conflict_base(conflict: &Conflict, repo: &Repository) -> String {
    conflict
        .base
        .clone()
        .unwrap_or_else(|| repo.base_branch.clone())
}

/// What the forge says is failing on the branch, as the round of requested
/// changes the engineer is sent back with.
///
/// Named rather than pointed at, for the reason the comments are quoted
/// rather than linked: an agent woken to go and find out what it was woken
/// for has been woken for nothing. Each check travels as the forge spells it
/// — its name, the verdict it finished with, and where the run is read —
/// which is what the engineer looks for on the request itself.
fn pr_checks_review(watched: &WatchedPr, checks: &[FailedCheck]) -> String {
    let listed = checks
        .iter()
        .map(|check| {
            let conclusion = match check.conclusion.trim() {
                "" => String::new(),
                conclusion => format!(" ({conclusion})"),
            };
            let url = match check
                .url
                .as_deref()
                .map(str::trim)
                .filter(|u| !u.is_empty())
            {
                Some(url) => format!(" — {url}"),
                None => String::new(),
            };
            format!("- {}{conclusion}{url}", check.name)
        })
        .collect::<Vec<_>>()
        .join("\n");
    let count = checks.len();
    let plural = if count == 1 { "check" } else { "checks" };
    format!(
        "{label} ({url}) has {count} failing {plural} on the commit it is open with:\n\n\
         {listed}\n\n\
         Making them pass is yours to do: read each run where the {noun} links to it, fix what \
         it is failing on, and where it is failing on something that is not the branch's fault, \
         say so. {rule} Then call `request_review` with a summary of what was failing and what \
         fixed it — that summary is what the people reading the {noun} are answered with.",
        label = watched.label(),
        url = watched.url,
        noun = watched.forge.noun(),
        rule = published_branch_rule(watched),
    )
}

/// The same for a branch that no longer merges into its base.
///
/// Nobody else can answer it: the integrator hits the conflict during its own
/// merge, and one asleep over a published request — waiting on the humans
/// reading it — would not hit it until somebody woke it for something else.
/// Neither forge names the conflicting files on the request, so what the
/// engineer is given is the branch to merge in, which is where it reads them
/// from.
fn pr_conflict_review(watched: &WatchedPr, base: &str) -> String {
    let noun = watched.forge.noun();
    format!(
        "{label} ({url}) no longer merges into {base}: the base moved under the branch, and \
         reconciling it is yours to do.\n\n\
         Bring {base} into the branch — `git fetch <remote> {base} && git merge --no-edit \
         <remote>/{base}`, or `git merge --no-edit {base}` where the base is only local — \
         resolve every conflict it reports, and commit the resolution. {rule} The merge commit \
         is fine: the forge squashes the {noun} when it merges it. Then call `request_review` \
         with a summary of what you reconciled — that summary is what the people reading the \
         {noun} are answered with.",
        label = watched.label(),
        url = watched.url,
        rule = published_branch_rule(watched),
    )
}

/// What the integrator is woken with when the engineer has answered the
/// people reading a published request.
///
/// Two things have to reach the request, and only one of them is a commit:
/// the revision, which is pushed, and the answers, which are the engineer's
/// words and are quoted here whole so that the agent has nothing to compose
/// and nothing to look up. It writes them to the user — one message, so the
/// user can paste the replies onto the request themselves — because the
/// daemon has no account on the forge to answer with.
///
/// The instruction is built here rather than stored as a briefing template
/// because the engineer's summary is not a value a briefing kind can name:
/// the resume the store holds is for the task whose landing nobody has
/// started yet, and lengthening its placeholder list for one situation would
/// leave the other rendering a token it has nothing to fill in.
fn published_revision_instruction(
    watched: &WatchedPr,
    task: &Task,
    repo: &Repository,
    replies: Option<&str>,
) -> String {
    let label = watched.label();
    let noun = watched.forge.noun();
    let base = &repo.base_branch;
    let branch = &task.branch;
    // Whatever the engineer wrote, byte for byte: the emptiness check reads a
    // trimmed copy, and what goes into the instruction is the summary itself —
    // its indentation, its blank lines and its trailing newline are part of
    // what the people on the request are being answered with.
    let replies = replies
        .filter(|r| !r.trim().is_empty())
        .unwrap_or("(the engineer left no summary of this revision)");
    format!(
        "The engineer has answered the people reviewing {label} ({url}), and the branch is \
         yours again. Push the revision to that same {noun} — never a second one — and hand \
         their answers on.\n\n\
         1. Bring {branch} up to date in your worktree: `git fetch <remote> {base} && \
         git merge --no-edit <remote>/{base}`, then a plain `git push <remote> {branch}`, \
         never forced and never rewriting a commit the {noun} already shows. The merge \
         commit is fine: the forge squashes the {noun} when it merges it. If the merge \
         conflicts, do not resolve it: name the files with `git diff --name-only \
         --diff-filter=U`, then `git merge --abort` and `return_to_engineer` with them and \
         what to reconcile, which ends your turn.\n\
         2. Then `post_message` to \"user\" — one message, carrying {url} and the engineer's \
         replies below verbatim, one per comment — so they can answer on the {noun} \
         themselves. Then end your turn: Ariadne watches the {noun} and wakes you when it \
         moves.\n\n\
         The engineer's replies, as it wrote them:\n\n{replies}",
        url = watched.url,
    )
}

/// And when it was merged: the task is finished off the branch it landed on.
fn pr_merged_instruction(watched: &WatchedPr, base_branch: &str) -> String {
    let label = watched.label();
    let forge = watched.forge.name();
    format!(
        "{label} was merged on {forge}. Finish the task: fetch the remote in the primary \
         checkout, fast-forward {base_branch} onto it, and call the `mark_merged` MCP tool with \
         the sha it now points at. The daemon verifies the merge against {forge}, so report it \
         truthfully."
    )
}

/// What the woken agent is told: who wrote, what they wrote, and where to
/// answer. The message is quoted whole — an agent asked to go and look it up
/// before it knows what it says has been woken for nothing — with the pointer
/// there for the rest of the thread.
fn delivery_text(message: &Message) -> String {
    let thread = match message.task_id {
        Some(_) => "your task conversation",
        None => "the goal's planning thread",
    };
    format!(
        "New message from the {author} in {thread}:\n\n{body}\n\n\
         Read the rest with `list_messages`, answer with `post_message` — both MCP tools.",
        author = message.author_role().as_str(),
        body = message.body,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pull_request() -> WatchedPr {
        WatchedPr {
            forge: Forge::GitHub,
            number: 12,
            url: "https://github.com/owner/repo/pull/12".into(),
        }
    }

    fn merge_request() -> WatchedPr {
        WatchedPr {
            forge: Forge::GitLab,
            number: 12,
            url: "https://gitlab.com/owner/repo/-/merge_requests/12".into(),
        }
    }

    fn published_task() -> Task {
        Task {
            id: "T1".into(),
            goal_id: "G1".into(),
            repo_id: "R1".into(),
            title: "Render the board".into(),
            description: "Do the thing.".into(),
            status: "under_review".into(),
            engineer_profile_id: "E1".into(),
            integrator_profile_id: ariadne_store::defaults::INTEGRATOR_ID.into(),
            agent_kind: None,
            model: None,
            branch: "ariadne/task-t1".into(),
            worktree_path: None,
            review_round: 2,
            stalled: 0,
            merge_commit: None,
            pr_number: Some(12),
            pr_url: Some("https://github.com/owner/repo/pull/12".into()),
            pr_relayed_comments: None,
            pr_approved_notified: 0,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
        }
    }

    fn repository() -> Repository {
        Repository {
            id: "R1".into(),
            path: "/repos/ariadne".into(),
            base_branch: "main".into(),
            description: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
        }
    }

    fn delivery_message() -> Message {
        Message {
            id: "M1".into(),
            goal_id: "G1".into(),
            task_id: Some("T1".into()),
            author_role: "planner".into(),
            author_session_id: None,
            recipient_kind: None,
            recipient_profile_id: None,
            body: "the scope grew: drop the second forge".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
        }
    }

    /// The send-back is the whole of what the engineer reads: every comment
    /// quoted verbatim with its author and, where it hangs on the diff, its
    /// file and line — and what to do about them in one readable paragraph,
    /// with no run-on whitespace from a template that got folded wrong.
    #[test]
    fn the_send_back_quotes_every_comment_that_was_written_on_the_review() {
        let review = pr_feedback_review(
            &pull_request(),
            &[
                Feedback {
                    id: "C1".into(),
                    author: "maria".into(),
                    body: "why a new module?".into(),
                    file: None,
                    blocking: false,
                },
                Feedback {
                    id: "RC2".into(),
                    author: "jon".into(),
                    body: "this allocates per row\n\nand the row is hot".into(),
                    file: Some("src/board.rs:42".into()),
                    blocking: true,
                },
            ],
        );
        assert!(review.contains("Pull request #12"), "{review}");
        assert!(
            review.contains("https://github.com/owner/repo/pull/12"),
            "{review}"
        );
        assert!(review.contains("2 new comments"), "{review}");
        assert!(
            review.contains("### maria commented\n> why a new module?"),
            "{review}"
        );
        assert!(
            review.contains("### jon requested changes on src/board.rs:42"),
            "{review}"
        );
        // Verbatim, every line of it, and the empty line between them is a
        // quote too rather than the end of the quotation.
        assert!(
            review.contains("> this allocates per row\n>\n> and the row is hot"),
            "{review}"
        );
        // What to do with them, and nothing about relaying anything: no agent
        // stands between the comments and the engineer any more.
        assert!(review.contains("`request_review`"), "{review}");
        assert!(review.contains("no `commit --amend`"), "{review}");
        assert!(!review.contains("return_to_engineer"), "{review}");
        assert!(!review.contains("  "), "{review}");

        let one = pr_feedback_review(
            &pull_request(),
            &[Feedback {
                id: "C1".into(),
                author: "maria".into(),
                body: "why?".into(),
                file: None,
                blocking: false,
            }],
        );
        assert!(one.contains("1 new comment from the humans"), "{one}");
    }

    /// The same for what the forge says is failing rather than what a person
    /// wrote: every check named as the forge names it, where its run is read,
    /// and the rule a published branch is answered under.
    #[test]
    fn the_send_back_names_every_check_the_forge_reported_as_failed() {
        let review = pr_checks_review(
            &pull_request(),
            &[
                FailedCheck {
                    id: "CHKabc123:test".into(),
                    name: "test".into(),
                    conclusion: "FAILURE".into(),
                    url: Some("https://github.com/owner/repo/actions/runs/17".into()),
                },
                // A forge that answered with no URL and no verdict still
                // answers with a name, which is what the engineer looks for.
                FailedCheck {
                    id: "CHKabc123:lint".into(),
                    name: "lint".into(),
                    conclusion: String::new(),
                    url: None,
                },
            ],
        );
        assert!(review.contains("Pull request #12"), "{review}");
        assert!(review.contains("2 failing checks"), "{review}");
        assert!(
            review.contains("- test (FAILURE) — https://github.com/owner/repo/actions/runs/17"),
            "{review}"
        );
        assert!(review.contains("\n- lint\n"), "{review}");
        assert!(review.contains("`request_review`"), "{review}");
        assert!(review.contains("no `commit --amend`"), "{review}");
        assert!(!review.contains("  "), "{review}");

        let one = pr_checks_review(
            &merge_request(),
            &[FailedCheck {
                id: "CHKabc123:pipeline".into(),
                name: "pipeline".into(),
                conclusion: "failed".into(),
                url: None,
            }],
        );
        assert!(one.contains("1 failing check on the commit"), "{one}");
        assert!(one.contains("merge request is published"), "{one}");
    }

    /// And for a branch that no longer merges: the base to bring in, since
    /// neither forge names the files, and the same rule about how.
    #[test]
    fn the_send_back_for_a_conflict_names_the_base_to_merge_in() {
        let review = pr_conflict_review(&pull_request(), "main");
        assert!(
            review.contains(
                "Pull request #12 (https://github.com/owner/repo/pull/12) no longer \
                             merges into main"
            ),
            "{review}"
        );
        assert!(
            review.contains("git merge --no-edit <remote>/main"),
            "{review}"
        );
        assert!(review.contains("`request_review`"), "{review}");
        assert!(review.contains("no `commit --amend`"), "{review}");
        assert!(!review.contains("  "), "{review}");

        // The branch the forge named, and the repository's own base where it
        // named none.
        let repo = repository();
        assert_eq!(
            conflict_base(
                &Conflict {
                    id: "MRGabc123".into(),
                    base: Some("release/2".into()),
                },
                &repo
            ),
            "release/2"
        );
        assert_eq!(
            conflict_base(
                &Conflict {
                    id: "MRGabc123".into(),
                    base: None,
                },
                &repo
            ),
            repo.base_branch
        );
    }

    /// The delivery nudge carries the message itself, not a pointer to go
    /// and read it, and names both tools it hands the woken agent as the MCP
    /// tool calls they are.
    #[test]
    fn the_delivery_nudge_quotes_the_message_and_names_its_mcp_tools() {
        let text = delivery_text(&delivery_message());
        assert!(
            text.contains("New message from the planner in your task conversation"),
            "{text}"
        );
        assert!(
            text.contains("the scope grew: drop the second forge"),
            "{text}"
        );
        assert!(
            text.contains("`list_messages`, answer with `post_message` — both MCP tools"),
            "{text}"
        );
        assert!(!text.contains("  "), "{text}");

        let planning = delivery_text(&Message {
            task_id: None,
            ..delivery_message()
        });
        assert!(
            planning.contains("in the goal's planning thread"),
            "{planning}"
        );
    }

    /// The revision instruction carries both things the request is waiting
    /// for: the commits, pushed the one way a published branch may be
    /// updated, and the engineer's answers, quoted whole for the user to put
    /// on the request.
    #[test]
    fn the_revision_instruction_pushes_the_branch_and_quotes_the_replies() {
        const REPLIES: &str = "Reply to @maria on src/board.rs:42: it allocates once now.\n\
                               Reply to @jon: the module stays, and here is why.";
        let instruction = published_revision_instruction(
            &pull_request(),
            &published_task(),
            &repository(),
            Some(REPLIES),
        );
        assert!(instruction.contains("Pull request #12"), "{instruction}");
        assert!(
            instruction.contains("https://github.com/owner/repo/pull/12"),
            "{instruction}"
        );
        // The one way a published branch is brought up to date, on the branch
        // and the base it names.
        assert!(
            instruction.contains("git merge --no-edit <remote>/main"),
            "{instruction}"
        );
        assert!(
            instruction.contains("git push <remote> ariadne/task-t1"),
            "{instruction}"
        );
        assert!(instruction.contains("never a second one"), "{instruction}");
        assert!(instruction.contains("git merge --abort"), "{instruction}");
        assert!(instruction.contains("return_to_engineer"), "{instruction}");
        // And never the ways that rewrite what people are already reading.
        for never in ["rebase", "--force", "--amend"] {
            assert!(!instruction.contains(never), "{never}: {instruction}");
        }
        // The replies verbatim, and who to give them to.
        assert!(instruction.contains(REPLIES), "{instruction}");
        assert!(
            instruction.contains("`post_message` to \"user\""),
            "{instruction}"
        );
        assert!(!instruction.contains("  "), "{instruction}");

        // Verbatim to the byte: an agent that lays its replies out is not
        // reformatted on the way through, blank lines, indentation, trailing
        // newline and all.
        let laid_out = "\n  1. @maria: it allocates once now.\n\n  2. @jon: the module stays.\n";
        let kept = published_revision_instruction(
            &pull_request(),
            &published_task(),
            &repository(),
            Some(laid_out),
        );
        assert!(
            kept.ends_with(laid_out),
            "the replies were reflowed on the way in: {kept:?}"
        );

        // A revision with nothing said about it still pushes.
        let silent =
            published_revision_instruction(&pull_request(), &published_task(), &repository(), None);
        assert!(silent.contains("left no summary"), "{silent}");
        let blank = published_revision_instruction(
            &pull_request(),
            &published_task(),
            &repository(),
            Some("  \n "),
        );
        assert!(blank.contains("left no summary"), "{blank}");
    }

    #[test]
    fn the_merge_instruction_names_the_branch_to_fast_forward() {
        let instruction = pr_merged_instruction(&pull_request(), "main");
        assert!(instruction.contains("#12 was merged"), "{instruction}");
        assert!(instruction.contains("fast-forward main"), "{instruction}");
        assert!(instruction.contains("mark_merged"), "{instruction}");
        assert!(!instruction.contains("  "), "{instruction}");
    }

    /// The same two instructions on GitLab, in GitLab's own words: an agent
    /// told to go and read more is told to read it with the CLI it has, and
    /// `gh` is not that CLI.
    #[test]
    fn a_merge_request_is_named_and_reread_the_gitlab_way() {
        let review = pr_feedback_review(
            &merge_request(),
            &[Feedback {
                id: "N1".into(),
                author: "maria".into(),
                body: "why a new module?".into(),
                file: Some("src/board.rs:7".into()),
                blocking: true,
            }],
        );
        assert!(
            review.contains("Merge request !12 (https://gitlab.com/owner/repo/-/merge_requests/12) has 1 new comment"),
            "{review}"
        );
        assert!(
            review.contains("### maria requested changes on src/board.rs:7"),
            "{review}"
        );
        assert!(
            review.contains("The merge request is published"),
            "{review}"
        );
        assert!(!review.contains("pull request"), "{review}");
        assert!(!review.contains("  "), "{review}");

        let revision = published_revision_instruction(
            &merge_request(),
            &published_task(),
            &repository(),
            Some("Reply to @maria: fixed."),
        );
        assert!(
            revision.contains(
                "reviewing Merge request !12 (https://gitlab.com/owner/repo/-/merge_requests/12)"
            ),
            "{revision}"
        );
        assert!(revision.contains("same merge request"), "{revision}");
        assert!(!revision.contains("pull request"), "{revision}");
        assert!(!revision.contains("  "), "{revision}");

        let merged = pr_merged_instruction(&merge_request(), "main");
        assert!(
            merged.contains("Merge request !12 was merged on GitLab"),
            "{merged}"
        );
        assert!(merged.contains("fast-forward main"), "{merged}");
        assert!(merged.contains("mark_merged"), "{merged}");
        assert!(!merged.contains("GitHub"), "{merged}");
        assert!(!merged.contains("  "), "{merged}");
    }
}
