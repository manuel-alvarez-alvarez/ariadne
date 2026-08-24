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
    Actor, AttentionReason, AuthorRole, GoalStatus, PromptKind, ReviewVerdict, Role, SessionStatus,
    TaskStatus,
};
use ariadne_store::{
    AgentSession, Message, NewMessage, Recipient, SessionFilter, Store, Task, TaskFilter,
};

use crate::agents::prompts;
use crate::attention;
use crate::forge::{self, Feedback, Forge, PrState, WatchedPr};
use crate::gh;
use crate::glab;
use crate::launcher::Launcher;
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

/// How often the full reconciliation tick runs.
const TICK_SECS: u64 = 15;
/// Spawn attempts per task before it is failed.
const SPAWN_RETRY_BUDGET: u32 = 3;
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
    /// Messages already delivered to their addressee, by message id. In
    /// memory like the two maps above, and for the same reason: what it
    /// prevents is typing one message into a pane twice, which is only ever
    /// at stake while the daemon that saw it posted is still running.
    delivered: HashSet<String>,
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
    let mut scheduler = Scheduler {
        store,
        launcher,
        spawn_failures: HashMap::new(),
        nudged: HashMap::new(),
        unstarted: HashMap::new(),
        pr_polled: HashMap::new(),
        delivered: HashSet::new(),
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
                    self.store
                        .set_goal_status(&goal.id, GoalStatus::Completed)
                        .await?;
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
                        let _ = self
                            .store
                            .transition_task(
                                &task.id,
                                TaskStatus::Cancelled,
                                Actor::User,
                                Some("goal cancelled"),
                                None,
                            )
                            .await;
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
            let _ = self
                .store
                .transition_task(
                    task_id,
                    TaskStatus::Failed,
                    Actor::Daemon,
                    Some("spawn retry budget exhausted"),
                    None,
                )
                .await;
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
                let summary = self.launcher.engineer_summary(&task.id).await?;
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
                // that has since been deleted.
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
                info!(task = %task.id, "approved: handing the task to its integrator");
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
    /// - **commented on**, and every comment nobody has relayed yet goes to
    ///   the engineer through the integrator's send-back;
    /// - **approved**, and the user is told once that it is theirs to merge.
    ///
    /// The three read the same on either forge; which CLI answers for them is
    /// [`Self::poll_forge`]'s to decide.
    ///
    /// An integrator mid-turn is left to finish it: resuming an agent means
    /// relaunching its pane, and whatever it is doing on the branch right now
    /// is more current than a poll taken a moment ago. The next poll asks
    /// again.
    async fn watch_pull_request(&mut self, task: &Task, watched: &WatchedPr) -> anyhow::Result<()> {
        let number = watched.number;
        let Some(integrator) = self.live_integrator(task).await? else {
            // A task back from the engineer, or a daemon that restarted: the
            // integrator is started with its resume briefing, which tells it
            // to update the review it already opened.
            return Ok(());
        };
        // The one watchdog that still applies: an agent whose instruction is
        // sitting unsubmitted in its composer has not started the turn this
        // arm woke it for, and no amount of polling will move it. The idle
        // half of the stall watch is what does not apply — an integrator with
        // a review open is waiting on people, not stalling.
        if integrator.status() == SessionStatus::Running {
            self.check_unstarted_turn(&integrator).await?;
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
        let Some(poll) = self
            .poll_forge(
                &task.id,
                &repo_path,
                watched,
                &task.pr_relayed_comments(),
                task.pr_approved_notified(),
            )
            .await
        else {
            return Ok(());
        };
        // An approval that was dismissed or overtaken by a new review is one
        // the user has to be told about again when it comes back. A poll that
        // could not tell is not one withdrawn, so it leaves the flag alone.
        if poll.approved == Some(false) && task.pr_approved_notified() {
            self.store
                .set_task_pr_approved_notified(&task.id, false)
                .await?;
        }
        if poll.state != PrState::Quiet && integrator.status() != SessionStatus::Idle {
            info!(task = %task.id, pr = number, session = %integrator.id, "the review moved while its integrator is working; leaving it to finish");
            return Ok(());
        }
        match poll.state {
            PrState::Quiet => {}
            PrState::Merged => {
                info!(task = %task.id, pr = number, "the review was merged; waking the integrator to finish the task");
                self.launcher
                    .resume_integrator(&task.id, &pr_merged_instruction(watched, &repo.base_branch))
                    .await?;
                self.spawn_failures.remove(&task.id);
            }
            PrState::Feedback(feedback) => {
                info!(task = %task.id, pr = number, comments = feedback.len(), "the review was commented on; waking the integrator to relay it");
                // Spent before the attempt, like the stall nudge and the
                // message delivery: what a comment relayed twice costs the
                // engineer is a round of work it has already done, and a
                // resume that fails will not go better for being repeated.
                let ids: Vec<String> = feedback.iter().map(|f| f.id.clone()).collect();
                self.store
                    .add_task_pr_relayed_comments(&task.id, &ids)
                    .await?;
                self.launcher
                    .resume_integrator(&task.id, &pr_feedback_instruction(watched, &feedback))
                    .await?;
                self.spawn_failures.remove(&task.id);
            }
            PrState::Approved => {
                info!(task = %task.id, pr = number, "the review is approved; telling the user it is theirs to merge");
                self.notify_pull_request_approved(task, &integrator, watched)
                    .await?;
            }
        }
        Ok(())
    }

    /// One look at the review, through the CLI of the forge it is on.
    ///
    /// A CLI that cannot answer — not installed, not authenticated, the
    /// network down — is not a reason to fail the task: the review is still
    /// there, and the next poll asks again. That is the `None`.
    ///
    /// The two forges differ in how many reads one look takes and in nothing
    /// else: GitHub keeps what was written on the diff away from the pull
    /// request itself, and GitLab keeps the approvals away from the merge
    /// request. Either way what comes back is the same reading.
    async fn poll_forge(
        &self,
        task_id: &str,
        repo_path: &std::path::Path,
        watched: &WatchedPr,
        relayed: &[String],
        approved_notified: bool,
    ) -> Option<Poll> {
        let number = watched.number;
        match watched.forge {
            Forge::GitHub => {
                let gh_cli = self.launcher.gh();
                let pr = match gh_cli.pr_view(repo_path, watched).await {
                    Ok(pr) => pr,
                    Err(e) => {
                        warn!(task = %task_id, pr = number, error = %format!("{e:#}"), "reading the pull request failed");
                        return None;
                    }
                };
                // What was written on the diff rather than in the
                // conversation, which is where most review feedback lives.
                // Failing to read them is not worth dropping the poll for —
                // the conversation may be carrying something too, and the next
                // poll asks for both again.
                let review_comments = match gh_cli.pr_review_comments(repo_path, watched).await {
                    Ok(comments) => comments,
                    Err(e) => {
                        warn!(task = %task_id, pr = number, error = %format!("{e:#}"), "reading the pull request's review comments failed");
                        Vec::new()
                    }
                };
                let approved = pr.is_approved();
                Some(Poll {
                    approved: Some(approved),
                    state: gh::poll_state(
                        &pr,
                        &review_comments,
                        relayed,
                        approved && approved_notified,
                    ),
                })
            }
            Forge::GitLab => {
                let glab_cli = self.launcher.glab();
                let mr = match glab_cli.mr_view(repo_path, watched).await {
                    Ok(mr) => mr,
                    Err(e) => {
                        warn!(task = %task_id, mr = number, error = %format!("{e:#}"), "reading the merge request failed");
                        return None;
                    }
                };
                // Approvals and discussions are their own resources on
                // GitLab. One of them failing leaves the other still worth
                // acting on, so the poll goes on with what it has — and an
                // approval it could not read is reported as unknown rather
                // than as withdrawn.
                let (approvals, approved) = match glab_cli.mr_approvals(repo_path, watched).await {
                    Ok(approvals) => {
                        let approved = approvals.is_approved() && mr.is_open();
                        (approvals, Some(approved))
                    }
                    Err(e) => {
                        warn!(task = %task_id, mr = number, error = %format!("{e:#}"), "reading the merge request's approvals failed");
                        (glab::Approvals::default(), None)
                    }
                };
                let discussions = match glab_cli.mr_discussions(repo_path, watched).await {
                    Ok(discussions) => discussions,
                    Err(e) => {
                        warn!(task = %task_id, mr = number, error = %format!("{e:#}"), "reading the merge request's discussions failed");
                        Vec::new()
                    }
                };
                Some(Poll {
                    approved,
                    state: glab::poll_state(
                        &mr,
                        &approvals,
                        &discussions,
                        relayed,
                        approved.unwrap_or(false) && approved_notified,
                    ),
                })
            }
        }
    }

    /// The integrator working on this task, started if there is none.
    ///
    /// `None` means one was just started and there is nothing else to decide
    /// this pass: an agent resuming is about to read the branch and the pull
    /// request for itself.
    async fn live_integrator(&mut self, task: &Task) -> anyhow::Result<Option<AgentSession>> {
        let sessions = self
            .store
            .list_sessions(SessionFilter {
                task_id: Some(task.id.clone()),
                live_only: true,
                ..Default::default()
            })
            .await?;
        if let Some(integrator) = sessions.into_iter().find(|s| s.role() == Role::Integrator) {
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
    /// task thread addressed to them, and the flag that puts the integrator's
    /// session on the attention strip they read it from.
    ///
    /// Once per approval — `pr_approved_notified` is what says it has been
    /// done, and it is written before the flag so a failure repeats nothing.
    async fn notify_pull_request_approved(
        &self,
        task: &Task,
        integrator: &AgentSession,
        watched: &WatchedPr,
    ) -> anyhow::Result<()> {
        self.store
            .set_task_pr_approved_notified(&task.id, true)
            .await?;
        let noun = watched.forge.noun();
        let where_ = task.pr_url.clone().unwrap_or_else(|| watched.label());
        self.store
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
        self.store
            .set_session_attention(&integrator.id, AttentionReason::WaitingInput)
            .await?;
        Ok(())
    }

    /// Put the agent the task is waiting on back on its feet: the same session
    /// resumed where there is one to resume, a fresh spawn otherwise (both
    /// launcher calls fall back to the spawn themselves).
    async fn start_role(&mut self, task: &Task, role: Role) -> anyhow::Result<()> {
        match role {
            Role::Integrator => {
                let repo = self.store.get_repository(&task.repo_id).await?;
                let profile = self.launcher.integrator_profile(task).await?;
                let template =
                    prompts::template_for(&self.store, &profile.id, PromptKind::IntegrationResume)
                        .await;
                self.launcher
                    .resume_integrator(
                        &task.id,
                        &prompts::integration_resume_briefing(&template, task, &repo),
                    )
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
        let idle_secs = (chrono::Utc::now() - last.with_timezone(&chrono::Utc)).num_seconds();
        let already_nudged = self.nudged.get(&session.id) == Some(&key);

        if idle_secs >= STALL_FLAG_SECS && already_nudged {
            warn!(session = %session.id, role = %session.role, idle_secs, "session stalled, flagging for user attention");
            self.store
                .set_session_attention(&session.id, AttentionReason::Stalled)
                .await?;
            return Ok(true);
        }
        if idle_secs >= STALL_NUDGE_SECS && !already_nudged {
            info!(session = %session.id, role = %session.role, idle_secs, "nudging idle agent");
            // The nudge counts as spent either way: a pane that would not take
            // it this pass will not take it the next one, and the user is told
            // instead.
            let delivered = self
                .launcher
                .tmux
                .send_submitted(&session.tmux_session, nudge)
                .await?;
            self.nudged.insert(session.id.clone(), key);
            if !delivered {
                warn!(session = %session.id, role = %session.role, "the nudge stayed in the agent's composer, flagging for user attention");
                self.store
                    .set_session_attention(&session.id, AttentionReason::Stalled)
                    .await?;
            }
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

    async fn deliver(&mut self, message_id: &str) -> anyhow::Result<()> {
        let message = self.store.get_message(message_id).await?;
        let Some(recipient) = message.recipient() else {
            return Ok(());
        };
        // One delivery per message, however often the event is seen: the tick
        // resweeps everything, and a second pass would type the same message
        // into the pane again. Spent before the attempt rather than after it,
        // like the stall nudge — whatever comes of this one, repeating it is
        // not the remedy.
        if !self.delivered.insert(message.id.clone()) {
            return Ok(());
        }
        match recipient {
            Recipient::User => self.raise_for_user(&message).await,
            Recipient::Profile(profile_id) => self.wake_profile(&message, &profile_id).await,
        }
    }

    /// A message for the human, which no agent is woken for: it goes up the
    /// attention path the UI strip and `ariadne attention` already show, on
    /// the session of the agent that wrote it — the session the user answers
    /// in, and the one place the message can be traced back to.
    async fn raise_for_user(&self, message: &Message) -> anyhow::Result<()> {
        let Some(session_id) = &message.author_session_id else {
            // The user wrote to the user: nobody is waiting on an answer.
            return Ok(());
        };
        info!(message = %message.id, session = %session_id, "message addressed to the user, raising its author for them");
        self.store
            .set_session_attention(session_id, AttentionReason::WaitingInput)
            .await?;
        Ok(())
    }

    /// A message for an agent: nudged into its pane if it has one, and
    /// otherwise resumed with the message as its instruction.
    ///
    /// An addressee with no session to deliver to at all — a reviewer between
    /// rounds, an engineer whose task has not started — is not a failure and
    /// not a message lost: it stays in the thread, and the briefings send
    /// every agent to read the conversation when it next starts.
    async fn wake_profile(&self, message: &Message, profile_id: &str) -> anyhow::Result<()> {
        let Some(session) = self.recipient_session(message, profile_id).await? else {
            info!(message = %message.id, profile = %profile_id, "nobody to wake for this message; it waits in the thread");
            return Ok(());
        };
        // An agent does not need waking for what it said itself.
        if message.author_session_id.as_deref() == Some(session.id.as_str()) {
            return Ok(());
        }
        let text = delivery_text(message);
        if !self.launcher.tmux.has_session(&session.tmux_session).await {
            info!(message = %message.id, session = %session.id, role = %session.role, "resuming the addressed agent with the message");
            // Nothing to resume from (no agent id yet, a working directory
            // that is gone) reads the same way as no session at all: the
            // message keeps its place in the thread and is read from there.
            if let Err(e) = self.launcher.revive_session(&session.id, Some(&text)).await {
                info!(message = %message.id, session = %session.id, error = %format!("{e:#}"), "the addressed agent could not be resumed; the message waits in the thread");
            }
            return Ok(());
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
            return Ok(());
        }
        info!(message = %message.id, session = %session.id, role = %session.role, "nudging the addressed agent with the message");
        let delivered = self
            .launcher
            .tmux
            .send_submitted(&session.tmux_session, &text)
            .await?;
        if !delivered {
            warn!(message = %message.id, session = %session.id, "the message stayed in the agent's composer, flagging for user attention");
            self.store
                .set_session_attention(&session.id, AttentionReason::Stalled)
                .await?;
        }
        Ok(())
    }

    /// The session a message's addressee works in, the most recent one first.
    ///
    /// A task message looks in that task. A goal-thread one looks at the
    /// goal's own sessions — the ones with no task — because that is where
    /// the planner runs, and the planner is the only agent a goal thread can
    /// address; filtering by goal alone would reach into its tasks.
    async fn recipient_session(
        &self,
        message: &Message,
        profile_id: &str,
    ) -> anyhow::Result<Option<AgentSession>> {
        let sessions = self
            .store
            .list_sessions(SessionFilter {
                goal_id: Some(message.goal_id.clone()),
                task_id: message.task_id.clone(),
                ..Default::default()
            })
            .await?;
        Ok(sessions.into_iter().rev().find(|s| {
            s.profile_id == profile_id && (message.task_id.is_some() || s.task_id.is_none())
        }))
    }
}

/// What one poll of a review came back with.
struct Poll {
    /// Whether the forge says it is approved right now, or `None` when this
    /// poll could not tell — an answer that never came is not an approval
    /// withdrawn.
    approved: Option<bool>,
    state: PrState,
}

/// What the integrator is woken with when humans have written on the review:
/// what they wrote, quoted, and what to do with it.
///
/// Quoted rather than pointed at, for the reason a delivered message is
/// quoted whole: an agent told only that there is something to go and read
/// has been woken for nothing. What to do about it is in its own briefing —
/// this says which of the situations it was briefed for has happened.
fn pr_feedback_instruction(watched: &WatchedPr, feedback: &[Feedback]) -> String {
    let quoted = feedback
        .iter()
        .map(|f| {
            let what = if f.blocking {
                "requested changes"
            } else {
                "commented"
            };
            format!("### {} {what}\n{}", f.author, f.body.trim())
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    let count = feedback.len();
    let plural = if count == 1 { "comment" } else { "comments" };
    let label = watched.label();
    let noun = watched.forge.noun();
    // Where the rest of what was said is, in the words of the CLI the
    // integrator has: an agent sent to read more is sent to the right place.
    let reread = match watched.forge {
        Forge::GitHub => "`gh pr view --comments` and the inline review threads",
        Forge::GitLab => "`glab mr view --comments` and the diff's discussion threads",
    };
    format!(
        "{label} has {count} new {plural} to relay:\n\n\
         {quoted}\n\n\
         Relay every comment that asks for something with the `return_to_engineer` MCP tool, \
         quoted and attributed. Write no code; read the {noun} first with {reread} if you need \
         the code they mean. If none asks for a change — a bot notice, a thank-you — report \
         that in the task thread with the `post_message` MCP tool instead of sending the task \
         back. Your turn ends either way."
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
         Read the rest with the `list_messages` MCP tool; answer with `post_message`.",
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

    /// The wake instruction is the whole of what the integrator knows when it
    /// comes back: every comment quoted with its author, and what to do about
    /// them in one readable paragraph — no run-on whitespace from a template
    /// that got folded wrong.
    #[test]
    fn the_wake_instruction_quotes_every_comment_it_was_woken_for() {
        let instruction = pr_feedback_instruction(
            &pull_request(),
            &[
                Feedback {
                    id: "C1".into(),
                    author: "maria".into(),
                    body: "why a new module?".into(),
                    blocking: false,
                },
                Feedback {
                    id: "RC2".into(),
                    author: "jon".into(),
                    body: "src/board.rs: this allocates per row".into(),
                    blocking: true,
                },
            ],
        );
        assert!(instruction.contains("2 new comments"), "{instruction}");
        assert!(instruction.contains("### maria commented"), "{instruction}");
        assert!(
            instruction.contains("### jon requested changes"),
            "{instruction}"
        );
        assert!(
            instruction.contains("src/board.rs: this allocates per row"),
            "{instruction}"
        );
        assert!(instruction.contains("return_to_engineer"), "{instruction}");
        assert!(instruction.contains("gh pr view"), "{instruction}");
        assert!(!instruction.contains("glab"), "{instruction}");
        assert!(!instruction.contains("  "), "{instruction}");

        let one = pr_feedback_instruction(
            &pull_request(),
            &[Feedback {
                id: "C1".into(),
                author: "maria".into(),
                body: "why?".into(),
                blocking: false,
            }],
        );
        assert!(one.contains("1 new comment to relay"), "{one}");
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
        let instruction = pr_feedback_instruction(
            &merge_request(),
            &[Feedback {
                id: "N1".into(),
                author: "maria".into(),
                body: "src/board.rs: why a new module?".into(),
                blocking: true,
            }],
        );
        assert!(
            instruction.contains("Merge request !12 has 1 new comment to relay"),
            "{instruction}"
        );
        assert!(
            instruction.contains("### maria requested changes"),
            "{instruction}"
        );
        assert!(instruction.contains("glab mr view"), "{instruction}");
        assert!(!instruction.contains("gh pr view"), "{instruction}");
        assert!(instruction.contains("return_to_engineer"), "{instruction}");
        assert!(!instruction.contains("  "), "{instruction}");

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
