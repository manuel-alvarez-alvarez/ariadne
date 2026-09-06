//! What a task wants, by status: an author from `ready` to the merge, the
//! reviewers a round is waiting on, and the cleanup its ending owes.

use tracing::{info, warn};

use ariadne_core::{
    Actor, AttentionReason, GoalStatus, MessageKind, PromptKind, Seat, TaskStatus,
};
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

        // The branch is only followed while somebody is working on it. Finished
        // and cancelled tasks are let go by the cleanup below, but a failed
        // one keeps its worktree — a user can retry it — and until one does
        // there is nobody committing on its branch to report. Here rather than
        // in an arm of the match, so that every way a task can stop being
        // worked on converges on the same pass.
        if !launcher::worth_following(&task) {
            self.launcher.branches.unwatch(&task.id);
        }

        // Whatever the agents of this task have said to each other since the
        // last pass, delivered before anything else is decided: a question
        // answered is what unblocks the agent that asked, and the answer is
        // no use to it a pass later than it could have had it.
        self.deliver_task_messages(&task.id).await;

        // Every agent of a task stays up until the task is over. A reviewer
        // that has voted is not done with the task — the author may have
        // something to ask it, and it may have something to ask the author —
        // so it sits idle instead of being killed and started again for the
        // next round. What ends them is the task ending, which the terminal
        // arms below do through `cleanup_task`.
        
        // A task that has left `approved` — landed, or sent back to the
        // reviewers with a revision — is one whose author wants briefing
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
                    info!(task = %task.id, "dependencies finished, task ready");
                    self.store
                        .transition_task(&task.id, TaskStatus::Ready, Actor::Daemon, None, None)
                        .await?;
                    // Fall through on the next event/tick.
                    return Box::pin(self.reconcile_task(task_id)).await;
                }
            }
            TaskStatus::Ready => {
                if self
                    .live_sessions(&goal.id, Some(&task.id), Seat::Author)
                    .await?
                    .is_empty()
                {
                    info!(task = %task.id, "spawning author");
                    self.launcher.spawn_author(&task.id).await?;
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
                let verdicts = self
                    .store
                    .round_verdicts(&task.id, task.review_round)
                    .await?;

                // Verdicts first: they may close the round.
                let changes_requested = verdicts
                    .iter()
                    .any(|m| m.kind() == Some(MessageKind::RequestChanges));
                let approvals = verdicts
                    .iter()
                    .filter(|m| m.kind() == Some(MessageKind::Approve))
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
                // Every reviewer the task was staffed with, and no number of
                // its own: the orchestrator staffs the review the work needs
                // and agrees it with the user, so each reviewer it put there
                // is one whose verdict was wanted.
                if approvals >= reviewers.len() as i64 {
                    info!(task = %task.id, approvals, "every reviewer has approved");
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
                let verdict_by: std::collections::HashSet<_> = verdicts
                    .iter()
                    .filter_map(|m| m.from_agent_id.clone())
                    .collect();
                let pending: Vec<String> = reviewers
                    .into_iter()
                    .map(|r| r.id)
                    .filter(|id| !verdict_by.contains(id))
                    .collect();
                if pending.is_empty() {
                    return Ok(());
                }
                let summary = self.store.review_summary(&task.id).await?;
                for agent_id in pending {
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
                        if s.seat() == Seat::Reviewer
                            && s.task_agent_id.as_deref() == Some(agent_id.as_str())
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
                    // same way an author is. Reviewers that already voted
                    // are not in `pending` and are left to sit: waiting for
                    // the others is not a stall. There is no task-level flag
                    // for this — the session's own is the signal.
                    if let Some(reviewer) = running {
                        // Up and reporting: what earlier attempts at starting
                        // this reviewer spent of the task's budget comes back
                        // here rather than at the launch that started it.
                        self.spent_on_a_dead_launch(&reviewer.id, &task.id, &reviewer);
                        self.check_session_quiet(
                            &reviewer,
                            (task.status.clone(), task.review_round),
                            &resume,
                        )
                        .await?;
                    } else {
                        // A reviewer that came up and was never heard from
                        // spends an attempt of the task's, like its author
                        // does: a round whose reviewer exits the moment it
                        // starts is not a round to start one for every tick.
                        if let Some(last) = self
                            .last_session(&task.id, |s| {
                                s.task_agent_id.as_deref() == Some(agent_id.as_str())
                            })
                            .await
                            && self.spent_on_a_dead_launch(&last.id, &task.id, &last)
                        {
                            warn!(task = %task.id, session = %last.id, "the reviewer came up and was never heard from");
                            if self
                                .record_spawn_failure(
                                    &task.id,
                                    "its agent stopped as soon as it started",
                                )
                                .await
                            {
                                return Ok(());
                            }
                        }
                        info!(task = %task.id, reviewer = %agent_id, round = task.review_round, "starting reviewer");
                        // Resumes the reviewer's earlier session when there is
                        // one, spawns a first for it otherwise.
                        self.launcher
                            .resume_reviewer(&task.id, &agent_id, &resume)
                            .await?;
                    }
                }
            }
            TaskStatus::ChangesRequested => {
                let verdicts = self
                    .store
                    .round_verdicts(&task.id, task.review_round)
                    .await?;
                // Who asked, as the author reads it. An agent has no name of
                // its own, so it is named by the skills it reviewed with, and
                // by its id where it is staffed no longer.
                let mut feedback: Vec<(String, String)> = Vec::new();
                for verdict in verdicts
                    .iter()
                    .filter(|m| m.kind() == Some(MessageKind::RequestChanges))
                {
                    let agent_id = verdict.from_agent_id.clone().unwrap_or_default();
                    let skills = self.store.agent_skills(&agent_id).await.unwrap_or_default();
                    let who = match skills.is_empty() {
                        false => format!(
                            "reviewer ({})",
                            skills
                                .iter()
                                .map(|s| s.name.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                        true => format!("reviewer {agent_id}"),
                    };
                    feedback.push((who, verdict.body.clone()));
                }
                info!(task = %task.id, "resuming author with review feedback");
                let template = prompts::template_for(PromptKind::ChangesRequested);
                self.launcher
                    .resume_author(
                        &task.id,
                        &prompts::changes_requested_briefing(template, &feedback),
                    )
                    .await?;
                self.store
                    .transition_task(&task.id, TaskStatus::InProgress, Actor::Daemon, None, None)
                    .await?;
            }
            TaskStatus::Approved => {
                // Landing the change is the author's last turn, and the
                // session that wrote it is still there to take it: nothing
                // took the worktree away. What it has not had is the briefing
                // that says the task is approved and how its repository takes
                // it, so that goes out once — and from there the turn is
                // watched like any other.
                if self.landing_briefed.insert(task.id.clone()) {
                    info!(task = %task.id, "approved: briefing the author to land it");
                    self.start_author(&task).await?;
                } else {
                    self.check_stall(&task).await?;
                }
            }
            TaskStatus::Finished => {
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

    /// Count an attempt at giving this task an agent that came to nothing, and
    /// say whether that was the last one there was.
    ///
    /// `why` is what the task will carry if it was: a launch that could not be
    /// performed and an agent that died the moment it started are both a task
    /// nobody is coming back to, and the two are worth telling apart by the
    /// user reading it afterwards.
    pub(super) async fn record_spawn_failure(&mut self, task_id: &str, why: &str) -> bool {
        let failures = self.spawn_failures.entry(task_id.to_string()).or_insert(0);
        *failures += 1;
        if *failures < SPAWN_RETRY_BUDGET {
            return false;
        }
        warn!(task = %task_id, failures, why, "retry budget exhausted, failing task");
        let _ = self
            .store
            .transition_task(task_id, TaskStatus::Failed, Actor::Daemon, Some(why), None)
            .await;
        self.spawn_failures.remove(task_id);
        true
    }

    /// The sessions for this seat that are still running — including the ones
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
        seat: Seat,
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
            if s.seat() == seat
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
    /// Whose turn it is, is the author's from the first commit to the merge,
    /// so the author is the only seat this asks about.
    /// A task with no live author gets one started; one that has
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
        let Some(agent) = sessions.iter().find(|s| s.seat() == Seat::Author) else {
            // An author that came up and was never heard from spends an
            // attempt like a launch that never got off the ground. Without
            // that, a CLI that exits the moment it starts — a dialog nobody
            // answered, a folder it will not open — is a task that starts an
            // agent every tick for as long as its goal is active, and says so
            // to nobody.
            if let Some(last) = self
                .last_session(&task.id, |s| s.seat() == Seat::Author)
                .await
                && self.spent_on_a_dead_launch(&task.id, &task.id, &last)
            {
                warn!(task = %task.id, session = %last.id, "the author came up and was never heard from");
                if self
                    .record_spawn_failure(&task.id, "its agent stopped as soon as it started")
                    .await
                {
                    return Ok(());
                }
            }
            info!(task = %task.id, "the task is waiting on an author and has none live, starting one");
            if let Err(e) = self.start_author(task).await {
                // The task still wants this agent and could not get one: the
                // ended session is the thing the user has to look at.
                self.flag_last_disconnected(task).await;
                return Err(e);
            }
            return Ok(());
        };
        // What the attempts before this one spent comes back here, where the
        // author is up and has said something, rather than at the launch that
        // started it: a launch that worked is not yet an agent that runs.
        self.spent_on_a_dead_launch(&task.id, &task.id, agent);
        // The same words it would be started again with: an agent that has
        // gone quiet with the work still in front of it and one whose session
        // ended are in the same situation, and there is one text for it.
        let nudge = self.resume_text(task).await?;
        self.check_session_quiet(agent, (task.status.clone(), task.review_round), &nudge)
            .await
    }

    /// Put the agent a task is waiting on back on it: its author, resumed
    /// where its session merely ended and started afresh where there is none.
    ///
    /// Whatever the user is owed comes back up with it
    /// ([`Self::keep_waiting_user`]): starting the author again is the
    /// recovery for the agent, and no answer at all to a person who still has
    /// a request to merge.
    pub(super) async fn start_author(&mut self, task: &Task) -> anyhow::Result<()> {
        let instruction = self.resume_text(task).await?;
        let session = self
            .launcher
            .resume_author(&task.id, &instruction)
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
    /// on it has handed the merge to a human, and no restart of its author
    /// merges it for them.
    pub(super) async fn keep_waiting_user(
        &self,
        back: &AgentSession,
        carried: Option<AttentionReason>,
    ) -> anyhow::Result<()> {
        let mut owed = carried.is_some_and(|reason| reason.is_for_the_user());
        if !owed
            && back.seat() == Seat::Author
            && let Some(task_id) = back.task_id.as_deref()
        {
            let task = self.store.get_task(task_id).await?;
            owed = task.status() == TaskStatus::Approved && task.pr_url.is_some();
        }
        if owed {
            info!(session = %back.id, seat = %back.seat, "the agent is back on its feet and the user is still owed, raising it again");
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
    /// task is one the author is landing, and what it is picked up with is
    /// the landing briefing — the whole procedure, which is what a session
    /// that ended over it has to be given back. Anything earlier is work in
    /// the worktree, and the resume nudge is what that wants.
    async fn resume_text(&self, task: &Task) -> anyhow::Result<String> {
        if task.status() == TaskStatus::Approved {
            let repo = self.store.get_repository(&task.repo_id).await?;
            // The procedure is the task's: how this task ends was agreed with
            // the user when it was written, and it is the whole of what
            // decides which of the three the author runs.
            return Ok(prompts::landing_briefing(
                task.landing_prompt_text(),
                task,
                &repo,
            ));
        }
        let template = prompts::template_for(PromptKind::AuthorResume);
        Ok(prompts::author_resume_briefing(template, task))
    }

    /// The session that was last this task's, of the ones `which` picks out,
    /// whatever state it ended in: the author's seat, or the one staffed
    /// agent's among several reviewers.
    async fn last_session(
        &self,
        task_id: &str,
        which: impl Fn(&AgentSession) -> bool,
    ) -> Option<AgentSession> {
        let sessions = self
            .store
            .list_sessions(SessionFilter {
                task_id: Some(task_id.to_string()),
                ..Default::default()
            })
            .await
            .ok()?;
        sessions.into_iter().rev().find(|s| which(s))
    }

    /// Raise `disconnected` on the author session that was last on this
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
        if let Some(previous) = sessions.iter().rev().find(|s| s.seat() == Seat::Author) {
            warn!(task = %task.id, session = %previous.id, "starting the author failed, flagging its last session disconnected");
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

#[cfg(test)]
mod tests {
    /// A task is approved when every reviewer staffed on it has approved, so
    /// a task with no reviewer is approved as soon as its author asks: there
    /// is nobody to ask. That is what makes work with nothing to review — a
    /// release, a dependency bump the suite already judged — a task rather
    /// than a special case.
    #[test]
    fn a_task_is_approved_by_every_reviewer_staffed_on_it() {
        let approved = |approvals: i64, reviewers: usize| approvals >= reviewers as i64;
        assert!(approved(0, 0), "nobody to ask");
        assert!(!approved(0, 1));
        assert!(!approved(1, 2), "one of two is not the round closed");
        assert!(approved(2, 2));
    }
}
