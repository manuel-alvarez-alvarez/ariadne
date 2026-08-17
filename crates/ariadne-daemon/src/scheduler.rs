//! Scheduler: event-driven reconciliation loop (docker-style).
//!
//! HTTP handlers send [`SchedEvent`]s after writes; a periodic tick
//! reconciles everything so crashes, missed events and dead tmux sessions
//! self-heal. Every rule is idempotent: read state, compare desired, act.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::mpsc;
use tracing::{info, warn};

use ariadne_core::{Actor, GoalStatus, PromptKind, ReviewVerdict, Role, SessionStatus, TaskStatus};
use ariadne_store::{SessionFilter, Store, Task, TaskFilter};

use crate::agents::prompts;
use crate::launcher::Launcher;

/// Events that wake the scheduler for a scoped reconciliation.
#[derive(Debug, Clone)]
pub enum SchedEvent {
    /// A task's status, reviews or deps changed.
    TaskChanged(String),
    /// A goal was created / cancelled / finalized.
    GoalChanged(String),
    /// An agent session reported activity.
    SessionEvent(String),
}

/// How often the full reconciliation tick runs.
const TICK_SECS: u64 = 15;
/// Spawn attempts per task before it is failed.
const SPAWN_RETRY_BUDGET: u32 = 3;
/// Idle time after which an engineer on an active task gets one nudge.
const STALL_NUDGE_SECS: i64 = 300;
/// Idle time after which the task is flagged stalled (post-nudge).
const STALL_FLAG_SECS: i64 = 900;

pub struct Scheduler {
    store: Store,
    launcher: Arc<Launcher>,
    /// Spawn failures per task (in-memory: resets on daemon restart, which is
    /// fine — a restart is exactly when a retry is warranted).
    spawn_failures: HashMap<String, u32>,
    /// Tasks nudged in their current (status, round); cleared on transition.
    nudged: HashMap<String, (String, i64)>,
}

/// Start the scheduler; returns the event sender for the HTTP layer.
pub fn start(store: Store, launcher: Arc<Launcher>) -> mpsc::UnboundedSender<SchedEvent> {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut scheduler = Scheduler {
        store,
        launcher,
        spawn_failures: HashMap::new(),
        nudged: HashMap::new(),
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
        self.liveness_sweep().await;
        let goals = match self.store.list_goals(None).await {
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
    async fn liveness_sweep(&mut self) {
        let Ok(live) = self
            .store
            .list_sessions(SessionFilter {
                live_only: true,
                ..Default::default()
            })
            .await
        else {
            return;
        };
        for session in live {
            match self
                .launcher
                .tmux
                .pane_geometry(&session.tmux_session)
                .await
            {
                Ok(geometry) => {
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
                    }
                    Ok(true) => {
                        warn!(session = %session.id, error = %e, "measuring the pane failed")
                    }
                    Err(check) => {
                        warn!(session = %session.id, error = %e, check = %check, "cannot reach tmux")
                    }
                },
            }
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
                let planner_alive = self.live_sessions(goal_id, None, Role::Planner).await?;
                if planner_alive == 0 {
                    info!(goal = %goal.id, "spawning planner");
                    self.launcher.spawn_planner(goal_id).await?;
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
            GoalStatus::Completed => {}
        }
        Ok(())
    }

    async fn kill_goal_sessions(&self, goal_id: &str) {
        if let Ok(sessions) = self
            .store
            .list_sessions(SessionFilter {
                goal_id: Some(goal_id.to_string()),
                live_only: true,
                ..Default::default()
            })
            .await
        {
            for session in sessions {
                let _ = self.launcher.kill_session(&session.id).await;
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

    /// How many sessions for this role are still running — counting the ones
    /// tmux would not answer for.
    ///
    /// This number decides whether to spawn, so an unanswered question has to
    /// count as a session: the sweep leaves such rows alone precisely because
    /// nothing is known about them, and reconciling on the assumption they are
    /// dead is how a tmux outage turns into two agents on one task.
    async fn live_sessions(
        &self,
        goal_id: &str,
        task_id: Option<&str>,
        role: Role,
    ) -> anyhow::Result<usize> {
        let sessions = self
            .store
            .list_sessions(SessionFilter {
                goal_id: Some(goal_id.to_string()),
                task_id: task_id.map(str::to_string),
                live_only: true,
                ..Default::default()
            })
            .await?;
        let mut count = 0;
        for s in &sessions {
            if s.role() == role
                && self
                    .launcher
                    .tmux
                    .has_session_or_unknown(&s.tmux_session)
                    .await
            {
                count += 1;
            }
        }
        Ok(count)
    }

    async fn reconcile_task(&mut self, task_id: &str) -> anyhow::Result<()> {
        let task = self.store.get_task(task_id).await?;
        let goal = self.store.get_goal(&task.goal_id).await?;
        if goal.status() != GoalStatus::Active {
            return Ok(());
        }

        // Reviewer sessions only belong to under_review.
        if task.status() != TaskStatus::UnderReview {
            self.kill_role_sessions(&task, Role::Reviewer).await;
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
                    == 0
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
                self.check_stall(&task).await?;
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
                    let mut has_live = false;
                    for s in &live {
                        if s.role() == Role::Reviewer
                            && s.profile_id == profile_id
                            && self
                                .launcher
                                .tmux
                                .has_session_or_unknown(&s.tmux_session)
                                .await
                        {
                            has_live = true;
                            break;
                        }
                    }
                    if !has_live {
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
                let feedback: Vec<(String, String)> = reviews
                    .iter()
                    .filter(|r| r.verdict() == ReviewVerdict::RequestChanges)
                    .map(|r| {
                        (
                            format!("reviewer {}", r.reviewer_profile_id),
                            r.body.clone().unwrap_or_else(|| "(no details)".into()),
                        )
                    })
                    .collect();
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
                let repo = self.store.get_goal_repo(&task.repo_id).await?;
                info!(task = %task.id, "resuming engineer with merge instruction");
                let template = prompts::template_for(
                    &self.store,
                    &task.engineer_profile_id,
                    PromptKind::MergeInstructions,
                )
                .await;
                self.launcher
                    .resume_engineer(&task.id, &prompts::merge_briefing(&template, &task, &repo))
                    .await?;
                self.spawn_failures.remove(&task.id);
                self.store
                    .transition_task(&task.id, TaskStatus::Merging, Actor::Daemon, None, None)
                    .await?;
            }
            TaskStatus::Merging => {
                self.check_stall(&task).await?;
            }
            TaskStatus::Merged => {
                // Post-merge cleanup (idempotent), then wake dependents.
                // Worktrees are kept unless delete_merged_worktrees is set,
                // so merged work can be inspected later.
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

    /// An engineer idle too long on an active task gets exactly one tmux
    /// nudge per (status, round); if it stays idle, the task is flagged
    /// stalled for the user (never an endless loop).
    async fn check_stall(&mut self, task: &Task) -> anyhow::Result<()> {
        let sessions = self
            .store
            .list_sessions(SessionFilter {
                task_id: Some(task.id.clone()),
                live_only: true,
                ..Default::default()
            })
            .await?;
        let Some(engineer) = sessions.iter().find(|s| s.role() == Role::Engineer) else {
            // No live engineer at all: respawn/resume path.
            info!(task = %task.id, "no live engineer session, resuming");
            self.launcher
                .resume_engineer(&task.id, "Your previous session ended unexpectedly. Continue the task from where it left off.")
                .await?;
            return Ok(());
        };
        if engineer.status() != SessionStatus::Idle {
            return Ok(());
        }
        let Some(last) = &engineer.last_activity_at else {
            return Ok(());
        };
        let Ok(last) = chrono::DateTime::parse_from_rfc3339(last) else {
            return Ok(());
        };
        let idle_secs = (chrono::Utc::now() - last.with_timezone(&chrono::Utc)).num_seconds();
        let key = (task.status.clone(), task.review_round);
        let already_nudged = self.nudged.get(&task.id) == Some(&key);

        if idle_secs >= STALL_FLAG_SECS && already_nudged && !task.is_stalled() {
            warn!(task = %task.id, idle_secs, "task stalled, flagging for user attention");
            self.store.set_task_stalled(&task.id, true).await?;
        } else if idle_secs >= STALL_NUDGE_SECS && !already_nudged {
            info!(task = %task.id, idle_secs, "nudging idle engineer");
            let nudge = match task.status() {
                TaskStatus::Merging => {
                    "Reminder: your task is approved and waiting to be merged. Follow the merge instructions and call mark_merged."
                }
                _ => {
                    "Reminder: your task is still in progress. Continue working on it, or call request_review if it is complete."
                }
            };
            self.launcher
                .tmux
                .send_text(&engineer.tmux_session, nudge)
                .await?;
            self.nudged.insert(task.id.clone(), key);
        }
        Ok(())
    }
}
