//! What a task wants, by status: an engineer from `ready` to the merge, the
//! reviewers a round is waiting on, and the cleanup its ending owes.

use tracing::{debug, info, warn};

use ariadne_core::{Actor, AttentionReason, GoalStatus, PromptKind, ReviewVerdict, Role, TaskStatus};
use ariadne_store::{AgentSession, SessionFilter, Task, TaskFilter};

use crate::agents::prompts;
use crate::launcher;

use super::SPAWN_RETRY_BUDGET;

impl super::Scheduler {
    pub(super) async fn reconcile_task(&mut self, task_id: &str) -> anyhow::Result<()> {
        let task = self.store.get_task(task_id).await?;
        let goal = self.store.get_goal(&task.goal_id).await?;
        if goal.status() != GoalStatus::Active {
            return Ok(());
        }

        // The branch is only followed while somebody is working on it. Merged
        // and cancelled tasks are let go by the cleanup below, but a failed
        // one keeps its worktree — a user can retry it — and until one does
        // there is nobody committing on its branch to report. Here rather than
        // in an arm of the match, so that every way a task can stop being
        // worked on converges on the same pass.
        if !launcher::worth_following(&task) {
            self.launcher.branches.unwatch(&task.id);
        }

        // Reviewer sessions only belong to under_review: an agent whose part
        // of the lifecycle has passed is not left running on the task. The
        // engineer's is not one of them — it holds the worktree from the
        // first commit to the merge.
        if task.status() != TaskStatus::UnderReview {
            self.end_reviewers(&task).await;
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
                    self.store
                        .transition_task(
                            &task.id,
                            TaskStatus::Failed,
                            Actor::Daemon,
                            Some(&reason),
                            None,
                        )
                        .await?;
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

                // Two hand-offs meet here, and each earns the session that
                // made it a compaction: the engineer's, which requested this
                // review, and every reviewer's whose verdict is in. Owed
                // before the verdicts are read, so that a round they close
                // ends the reviewers only once the compaction is done.
                self.owe_review_compactions(&task, &reviews).await?;

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
                    let template = prompts::template_for(PromptKind::ReviewerResume);
                    let resume =
                        prompts::reviewer_resume_briefing(template, &task, summary.as_deref());
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
                    } else if let Some(compacting) = live
                        .iter()
                        .find(|s| s.profile_id == profile_id && self.compaction_in_flight(s))
                    {
                        // Its earlier session is still compacting the last
                        // round: relaunching it now would kill the pane
                        // under that, so the round waits for the pass after
                        // the compaction ends.
                        info!(task = %task.id, session = %compacting.id, "the reviewer is compacting its conversation; its next round starts after it");
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
                // The engineer's pane is not killed under a compaction: the
                // feedback goes out on the pass after it ends, and the task
                // waits here for that pass.
                if let Some(compacting) = self.engineer_compacting(&task).await? {
                    info!(task = %task.id, session = %compacting.id, "the engineer is compacting its conversation; the review feedback goes out after it");
                    return Ok(());
                }
                info!(task = %task.id, "resuming engineer with review feedback");
                let template = prompts::template_for(PromptKind::ChangesRequested);
                self.launcher
                    .resume_engineer(
                        &task.id,
                        &prompts::changes_requested_briefing(template, &feedback),
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
                // watched like any other. Once its pane is free: an engineer
                // compacting its conversation is briefed on the pass after.
                if !self.landing_briefed.contains(&task.id)
                    && let Some(compacting) = self.engineer_compacting(&task).await?
                {
                    info!(task = %task.id, session = %compacting.id, "the engineer is compacting its conversation; the landing briefing goes out after it");
                    return Ok(());
                }
                if self.landing_briefed.insert(task.id.clone()) {
                    info!(task = %task.id, "approved: briefing the engineer to land it");
                    self.start_engineer(&task).await?;
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

    /// Owe the compactions the review hand-offs earn: the engineer's for
    /// requesting the round, and each voting reviewer's for its verdict —
    /// once per round each, since the round is what the hand-off is about.
    async fn owe_review_compactions(
        &mut self,
        task: &Task,
        reviews: &[ariadne_store::Review],
    ) -> anyhow::Result<()> {
        let situation = (task.status.clone(), task.review_round);
        let live = self
            .store
            .list_sessions(SessionFilter {
                task_id: Some(task.id.clone()),
                live_only: true,
                ..Default::default()
            })
            .await?;
        for engineer in live.iter().filter(|s| s.role() == Role::Engineer) {
            self.owe_compaction(engineer, situation.clone()).await;
        }
        for review in reviews {
            let Some(session_id) = &review.session_id else {
                continue;
            };
            if let Some(reviewer) = live.iter().find(|s| &s.id == session_id) {
                self.owe_compaction(reviewer, situation.clone()).await;
            }
        }
        Ok(())
    }

    /// The task's live engineer session while a compaction is running in
    /// its pane, if that is where it is: the one moment the engineer is not
    /// relaunched with what the task has for it.
    async fn engineer_compacting(&self, task: &Task) -> anyhow::Result<Option<AgentSession>> {
        let live = self
            .store
            .list_sessions(SessionFilter {
                task_id: Some(task.id.clone()),
                live_only: true,
                ..Default::default()
            })
            .await?;
        Ok(live
            .into_iter()
            .find(|s| s.role() == Role::Engineer && self.compaction_in_flight(s)))
    }

    /// End the reviewers of a task whose review is over — but not one still
    /// owed the compaction its verdict earned: the session serves every
    /// round of the task, and what the compaction shortens is exactly what
    /// the next round would otherwise replay. It is ended on the pass after
    /// the compaction is done, or given up on.
    async fn end_reviewers(&self, task: &Task) {
        let live = match self
            .store
            .list_sessions(SessionFilter {
                task_id: Some(task.id.clone()),
                live_only: true,
                ..Default::default()
            })
            .await
        {
            Ok(live) => live,
            Err(e) => {
                warn!(task = %task.id, error = %e, "listing the reviewers to end failed");
                return;
            }
        };
        for reviewer in live.iter().filter(|s| s.role() == Role::Reviewer) {
            if self.compaction_pending(reviewer) {
                debug!(task = %task.id, session = %reviewer.id, "the reviewer is left up until the compaction it owes is done");
                continue;
            }
            info!(session = %reviewer.id, role = %reviewer.role, "killing session: the reviewers' part of the lifecycle has passed");
            if let Err(e) = self.launcher.kill_session(&reviewer.id).await {
                warn!(session = %reviewer.id, error = %e, "killing the session failed");
            }
        }
    }

    pub(super) async fn record_spawn_failure(&mut self, task_id: &str) {
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
                    Some("the agent could not be started"),
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
    ///
    /// Whatever the user is owed comes back up with it
    /// ([`Self::keep_waiting_user`]): starting the engineer again is the
    /// recovery for the agent, and no answer at all to a person who still has
    /// a request to merge.
    pub(super) async fn start_engineer(&mut self, task: &Task) -> anyhow::Result<()> {
        let instruction = self.resume_text(task).await?;
        let session = self
            .launcher
            .resume_engineer(&task.id, &instruction)
            .await?;
        self.keep_waiting_user(&session, None).await
    }

    /// Put back on the agent that came up what a human still owes its work.
    ///
    /// `waiting_user` is nobody's flag but the user's: it says a person owes
    /// this task something — a message written to them, a request that is
    /// theirs to merge — and putting the agent underneath back on its feet
    /// answers none of it. Both ways of doing that lose it all the same: a
    /// resume revives the row through `restart_session`, which drops its
    /// attention with everything else, and a spawn that had to start afresh
    /// leaves the flag on a row nobody looks at any more
    /// (`clear_superseded_attention`). So it goes back on the session that
    /// came up.
    ///
    /// Two ways to know it is owed, and either is enough. `carried` is what
    /// the row that went down was flagged with, for the caller that has that
    /// row. The task is the other, and the one that answers where the flag
    /// was already lost — swept aside by a `disconnected` before the resume,
    /// or left on a superseded row: an approved task with a request recorded
    /// on it has handed the merge to a human, and no restart of its engineer
    /// merges it for them.
    pub(super) async fn keep_waiting_user(
        &self,
        back: &AgentSession,
        carried: Option<AttentionReason>,
    ) -> anyhow::Result<()> {
        let mut owed = carried.is_some_and(|reason| reason.is_for_the_user());
        if !owed
            && back.role() == Role::Engineer
            && let Some(task_id) = back.task_id.as_deref()
        {
            let task = self.store.get_task(task_id).await?;
            owed = task.status() == TaskStatus::Approved && task.pr_url.is_some();
        }
        if owed {
            info!(session = %back.id, role = %back.role, "the agent is back on its feet and the user is still owed, raising it again");
            self.store
                .set_session_attention(&back.id, AttentionReason::WaitingUser)
                .await?;
        }
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
            // The landing briefing is the repository's: the text set on it, or
            // the default of its merge strategy. What reaches the engineer is
            // the procedure that repository lands by.
            return Ok(prompts::landing_briefing(
                repo.landing_prompt_text(),
                task,
                &repo,
            ));
        }
        let template = prompts::template_for(PromptKind::EngineerResume);
        Ok(prompts::engineer_resume_briefing(template, task))
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
