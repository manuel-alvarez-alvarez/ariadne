//! What a task wants, by status: an engineer from `ready` to the merge, the
//! reviewers a round is waiting on, and the cleanup its ending owes.

use tracing::{info, warn};

use ariadne_core::{Actor, AttentionReason, GoalStatus, PromptKind, ReviewVerdict, Role, TaskStatus};
use ariadne_store::{AgentSession, SessionFilter, Task, TaskFilter};

use crate::agents::prompts;

use super::SPAWN_RETRY_BUDGET;

impl super::Scheduler {
    pub(super) async fn reconcile_task(&mut self, task_id: &str) -> anyhow::Result<()> {
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
            self.kill_sessions(
                SessionFilter {
                    task_id: Some(task.id.clone()),
                    live_only: true,
                    ..Default::default()
                },
                Some(Role::Reviewer),
                "the reviewers' part of the lifecycle has passed",
            )
            .await;
        }
        // A task that has left `approved` — landed, or sent back to the
        // reviewers with a revision — is one whose engineer wants briefing
        // again the next time it is approved.
        if task.status() != TaskStatus::Approved {
            self.landing_briefed.remove(&task.id);
        }

        match task.status() {
            TaskStatus::Pending => {
                // A dependency that ended without merging is never going to,
                // so neither is the wait on it: the task ends too, naming what
                // stopped it, and the user can retry it once the dependency
                // has landed. Only the terminal endings count — a dependency
                // retried and working again is one this task can still wait
                // for. A goal being cancelled never reaches here: its tasks
                // are cancelled by `reconcile_goal`, and this pass returns
                // above as soon as the goal is no longer active.
                if let Some(blocker) = self.store.task_dependencies_blocked(&task.id).await? {
                    let reason = blocked_reason(&blocker);
                    warn!(task = %task.id, dependency = %blocker.id, "dependency ended unmerged, failing task");
                    let task = self
                        .store
                        .transition_task(
                            &task.id,
                            TaskStatus::Failed,
                            Actor::Daemon,
                            Some(&reason),
                            None,
                        )
                        .await?;
                    self.announce_ending(&task, Some(&reason)).await;
                    return Ok(());
                }
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
                    let landing = self.resume_text(&task).await?;
                    self.launcher.resume_engineer(&task.id, &landing).await?;
                    self.spawn_failures.remove(&task.id);
                } else {
                    self.check_stall(&task).await?;
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

    pub(super) async fn record_spawn_failure(&mut self, task_id: &str) {
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
    pub(super) async fn live_sessions(
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

    /// The agent a task is waiting on, watched.
    ///
    /// Whose turn it is, is the engineer's from the first commit to the merge,
    /// so the engineer is the only role this asks about.
    /// A task with no live engineer gets one started; one that has
    /// reported nothing for too long goes under [`Self::check_session_quiet`],
    /// which is one nudge per (status, round), then the user, then a relaunch.
    /// The task shows that stall too, but nothing here writes it: the flag on
    /// the session is the record of it, and the task's own column is the
    /// store's projection of that (`sync_task_stall`).
    async fn check_stall(&mut self, task: &Task) -> anyhow::Result<()> {
        let sessions = self
            .store
            .list_sessions(SessionFilter {
                task_id: Some(task.id.clone()),
                live_only: true,
                ..Default::default()
            })
            .await?;
        let Some(agent) = sessions.iter().find(|s| s.role() == Role::Engineer) else {
            info!(task = %task.id, "the task is waiting on an engineer and has none live, starting one");
            if let Err(e) = self.start_engineer(task).await {
                // The task still wants this agent and could not get one: the
                // ended session is the thing the user has to look at.
                self.flag_last_disconnected(task).await;
                return Err(e);
            }
            self.spawn_failures.remove(&task.id);
            return Ok(());
        };
        // The same words it would be started again with: an agent that has
        // gone quiet with the work still in front of it and one whose session
        // ended are in the same situation, and there is one text for it.
        let nudge = self.resume_text(task).await?;
        self.check_session_quiet(agent, (task.status.clone(), task.review_round), &nudge)
            .await
    }

    /// Put the agent a task is waiting on back on it: its engineer, resumed
    /// where its session merely ended and started afresh where there is none.
    pub(super) async fn start_engineer(&mut self, task: &Task) -> anyhow::Result<()> {
        let instruction = self.resume_text(task).await?;
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
    async fn resume_text(&self, task: &Task) -> anyhow::Result<String> {
        if task.status() == TaskStatus::Approved {
            let repo = self.store.get_repository(&task.repo_id).await?;
            // One landing briefing per merge strategy, and the repository says
            // which: what reaches the engineer is the procedure it runs.
            let template = prompts::template_for(
                &self.store,
                &task.engineer_profile_id,
                PromptKind::landing_for(repo.merge_strategy()),
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

    /// Raise `disconnected` on the engineer session that was last on this
    /// task, whatever state it ended in. Best effort: this runs while another
    /// failure is being reported, and adds nothing to it if it fails too.
    async fn flag_last_disconnected(&self, task: &Task) {
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
        if let Some(previous) = sessions.iter().rev().find(|s| s.role() == Role::Engineer) {
            warn!(task = %task.id, session = %previous.id, "starting the engineer failed, flagging its last session disconnected");
            let _ = self
                .store
                .set_session_attention(&previous.id, AttentionReason::Disconnected)
                .await;
        }
    }
}

/// Why a task waiting on a dependency that ended is ending too: which
/// dependency it was, and how it ended.
fn blocked_reason(dependency: &Task) -> String {
    let ended = match dependency.status() {
        TaskStatus::Cancelled => "was cancelled",
        _ => "failed",
    };
    format!(
        "dependency \"{}\" ({}) {ended}",
        dependency.title,
        short_id(&dependency.id)
    )
}

/// An id as a person can read it back: 26-character ULIDs are unreadable in
/// full, and the tail is enough to tell two apart — the same shortening the
/// CLI's attention board and the UI show.
fn short_id(id: &str) -> String {
    match id.char_indices().nth_back(7) {
        Some((i, _)) if id.len() > 10 => format!("…{}", &id[i..]),
        _ => id.to_string(),
    }
}
