//! What a task wants, by status: an author from `ready` to the merge, the
//! reviewers a review is waiting on, and the cleanup its ending owes.

use std::collections::HashSet;

use tracing::{info, warn};

use ariadne_core::{Actor, AttentionReason, GoalStatus, MessageKind, PromptKind, Seat, TaskStatus};
use ariadne_store::{AgentSession, SessionFilter, Task, TaskAgent, TaskFilter, author_branch};

use crate::agents::prompts;
use crate::launcher;

use super::SPAWN_RETRY_BUDGET;

/// What a task's agent is in, as the watchdog spends its one nudge and its one
/// flag per situation.
///
/// The status alone is not enough while a task is under review: a task sent
/// back for changes and sent for review again reads as `under_review` both
/// times, and a reviewer that went quiet in the first one would have spent the
/// nudge it is owed in the second. So the review it is under is named too — by
/// the request that opened it, which is what a round number used to stand for.
async fn review_situation(task: &Task, store: &ariadne_store::Store) -> anyhow::Result<String> {
    if task.status() != TaskStatus::UnderReview {
        return Ok(task.status.clone());
    }
    let review = store.open_review_request(&task.id).await?;
    Ok(format!("{}:{}", task.status, review.unwrap_or_default()))
}

impl super::Scheduler {
    pub(super) async fn reconcile_task(&mut self, task_id: &str) -> anyhow::Result<()> {
        let task = self.store.get_task(task_id).await?;
        let goal = self.store.get_goal(&task.goal_id).await?;
        if goal.status() != GoalStatus::Active {
            return Ok(());
        }

        // Who writes this task: one author for most tasks, several for a
        // contested one — which runs them side by side until the reviewers
        // pick a winner, and reads as one-author again once they have.
        let authors = self.store.list_task_authors(&task.id).await?;
        let contested = authors.len() > 1 && task.picked_agent_id.is_none();

        // The branch is only followed while somebody is working on it. Finished
        // and cancelled tasks are let go by the cleanup below, but a failed
        // one keeps its worktree — a user can retry it — and until one does
        // there is nobody committing on its branch to report. Here rather than
        // in an arm of the match, so that every way a task can stop being
        // worked on converges on the same pass. A contested task keeps its
        // worktrees on the sessions rather than on the task, so only its
        // ending lets the watches go.
        let followed = match authors.len() > 1 {
            true => !task.status().is_terminal() && task.status() != TaskStatus::Failed,
            false => launcher::worth_following(&task),
        };
        if !followed {
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
        // next review. What ends them is the task ending, which the terminal
        // arms below do through `cleanup_task`.

        // A task that has left `approved` — landed, or sent back to the
        // reviewers with a revision — is one whose author wants briefing
        // again the next time it is approved.
        if task.status() != TaskStatus::Approved {
            self.landing_briefed.remove(&task.id);
        }
        // And one that has left the pick — settled, retried, or reopened by a
        // review — is one whose reviewers want asking afresh next time.
        if !contested || task.status() != TaskStatus::UnderReview {
            self.pick_briefed.retain(|(t, _)| t != &task.id);
        }

        // A settlement interrupted between its writes: the winner is on the
        // task, but the approval never committed. The picks and the
        // approvals are all still there to read, so the settlement simply
        // runs again — losers removed, task approved — on this pass, which
        // after a daemon restart is the startup sweep. What this must not
        // catch is the winner's own revision of a published request: that
        // reopens the winner's review, so `authors_all_approved` is false
        // there and the ordinary review flow below handles it.
        if authors.len() > 1
            && task.status() == TaskStatus::UnderReview
            && let Some(winner_id) = task.picked_agent_id.clone()
        {
            let reviewers = self.store.list_task_reviewers(&task.id).await?;
            let picks = self.store.list_task_picks(&task.id).await?;
            if picks.len() >= reviewers.len() && self.store.authors_all_approved(&task.id).await? {
                info!(task = %task.id, winner = %winner_id, "finishing an interrupted pick settlement");
                return self.settle_pick(&task, &winner_id).await;
            }
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
            TaskStatus::Ready if contested => {
                // Every author starts at once, each in a worktree and on a
                // branch of its own.
                for author in &authors {
                    if self
                        .live_author_session(&task.id, &author.id)
                        .await?
                        .is_none()
                    {
                        info!(task = %task.id, author = %author.id, "spawning author");
                        self.launcher
                            .spawn_author_agent(&task.id, &author.id)
                            .await?;
                    }
                }
                self.store
                    .transition_task(&task.id, TaskStatus::InProgress, Actor::Daemon, None, None)
                    .await?;
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
            TaskStatus::InProgress if contested => {
                // Nobody has asked for a review yet — the first request is
                // what moves the task on — so every author is still writing.
                for author in &authors {
                    let situation = format!("in_progress:{}", author.id);
                    self.check_contested_author(&task, author, situation)
                        .await?;
                }
            }
            TaskStatus::InProgress => {
                self.check_stall(&task).await?;
            }
            TaskStatus::UnderReview if contested => {
                self.reconcile_contest(&task, &authors).await?;
            }
            TaskStatus::UnderReview => {
                let reviewers = self.store.list_task_reviewers(&task.id).await?;
                let verdicts = self.store.open_verdicts(&task.id).await?;
                // What "this review" is, for the watchdog: two reviews of one
                // task read as the same status, and are not the same thing to
                // have gone quiet in.
                let situation = review_situation(&task, &self.store).await?;

                // Verdicts first: they may settle the review.
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
                // so what runs for a second review is that same session
                // resumed — which review it is on is not part of its
                // identity, only of the briefing it is woken with.
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
                    // on a review that already has one.
                    let mut running = None;
                    for s in &live {
                        if s.seat() == Seat::Reviewer
                            && s.task_agent_id.as_deref() == Some(agent_id.as_str())
                            && self.launcher.session_process_alive(s).await
                        {
                            running = Some(s.clone());
                            break;
                        }
                    }
                    // One text for either way this reviewer is picked up:
                    // the verdict the review is waiting on and the diff that
                    // may have moved under it are what it is told whether its
                    // session is being started again or merely nudged.
                    let template = prompts::template_for(PromptKind::ReviewerResume);
                    let resume =
                        prompts::reviewer_resume_briefing(template, &task, summary.as_deref());
                    // A reviewer with no verdict yet is the review's only
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
                        self.check_session_quiet(&reviewer, situation.clone(), &resume)
                            .await?;
                    } else {
                        // A reviewer that came up and was never heard from
                        // spends an attempt of the task's, like its author
                        // does: a review whose reviewer exits the moment it
                        // starts is not one to start a reviewer for every tick.
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
                        info!(task = %task.id, reviewer = %agent_id, "starting reviewer");
                        // Resumes the reviewer's earlier session when there is
                        // one, spawns a first for it otherwise.
                        self.launcher
                            .resume_reviewer(&task.id, &agent_id, &resume)
                            .await?;
                    }
                }
            }
            TaskStatus::ChangesRequested => {
                let verdicts = self.store.open_verdicts(&task.id).await?;
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

    /// One pass over a contested task under review: each author's own review
    /// runs to approval side by side, and once every one of them stands
    /// approved the reviewers are asked to pick the winner.
    ///
    /// A reviewer works one review at a time, so a pass rouses it for at
    /// most one: the oldest it owes, in the order the authors were listed.
    /// The verdict that settles that one is the event whose reconcile hands
    /// it the next — at once, not on the quiet clock.
    async fn reconcile_contest(
        &mut self,
        task: &Task,
        authors: &[TaskAgent],
    ) -> anyhow::Result<()> {
        let reviewers = self.store.list_task_reviewers(&task.id).await?;
        let mut all_approved = true;
        // The first review each reviewer still owes a verdict on, by
        // reviewer id: (the author under review, the request that opened it).
        let mut owed: Vec<(&TaskAgent, &TaskAgent, String)> = Vec::new();
        for author in authors {
            let Some(request) = self
                .store
                .open_review_request_of(&task.id, &author.id)
                .await?
            else {
                // Still writing: the task moved on when a sibling asked
                // first, and this author's own review has not opened yet.
                all_approved = false;
                let situation = format!("in_progress:{}", author.id);
                self.check_contested_author(task, author, situation).await?;
                continue;
            };
            let verdicts = self.store.open_verdicts_of(&task.id, &author.id).await?;
            if verdicts
                .iter()
                .any(|m| m.kind() == Some(MessageKind::RequestChanges))
            {
                // The change request reached the author as a message on the
                // channel; what is watched here is the author revising.
                all_approved = false;
                let situation = format!("changes:{request}");
                self.check_contested_author(task, author, situation).await?;
                continue;
            }
            let approved_by: HashSet<&str> = verdicts
                .iter()
                .filter(|m| m.kind() == Some(MessageKind::Approve))
                .filter_map(|m| m.from_agent_id.as_deref())
                .collect();
            if approved_by.len() >= reviewers.len() {
                continue;
            }
            all_approved = false;
            for reviewer in reviewers
                .iter()
                .filter(|r| !approved_by.contains(r.id.as_str()))
            {
                if !owed.iter().any(|(claimed, _, _)| claimed.id == reviewer.id) {
                    owed.push((reviewer, author, request.clone()));
                }
            }
        }
        for (reviewer, author, request) in owed {
            self.rouse_reviewer_for(task, reviewer, author, &request)
                .await?;
        }
        if !all_approved {
            // A review reopened is a pick to ask for afresh once it closes.
            self.pick_briefed.retain(|(t, _)| t != &task.id);
            return Ok(());
        }
        self.run_the_pick(task, authors, &reviewers).await
    }

    /// The pick itself: every reviewer asked once, and the winner settled as
    /// soon as the last pick is in.
    async fn run_the_pick(
        &mut self,
        task: &Task,
        authors: &[TaskAgent],
        reviewers: &[TaskAgent],
    ) -> anyhow::Result<()> {
        let picks = self.store.list_task_picks(&task.id).await?;
        if picks.len() >= reviewers.len() {
            let Some(winner) = ariadne_store::picked_winner(authors, &picks) else {
                return Ok(());
            };
            info!(task = %task.id, winner = %winner.id, "every reviewer has picked; landing this author");
            self.store.set_task_picked(&task.id, &winner.id).await?;
            return self.settle_pick(task, &winner.id.clone()).await;
        }

        let picked_by: HashSet<&str> = picks.iter().map(|p| p.reviewer_agent_id.as_str()).collect();
        let lines: Vec<(String, String)> = authors
            .iter()
            .map(|a| (a.id.clone(), author_branch(&task.branch, a.ordinal)))
            .collect();
        let template = prompts::template_for(PromptKind::ReviewerPick);
        let briefing = prompts::reviewer_pick_briefing(template, task, &lines);
        for reviewer in reviewers
            .iter()
            .filter(|r| !picked_by.contains(r.id.as_str()))
        {
            let situation = format!("pick:{}", task.id);
            if let Some(session) = self.live_reviewer_session(&task.id, &reviewer.id).await? {
                self.spent_on_a_dead_launch(&reviewer.id, &task.id, &session);
                // Asked once, straight into the pane; from there the quiet
                // clock takes over like any other owed answer.
                let key = (task.id.clone(), reviewer.id.clone());
                if !self.pane_busy(&session.id) && self.pick_briefed.insert(key) {
                    info!(task = %task.id, reviewer = %reviewer.id, "asking the reviewer to pick the winner");
                    self.spawn_delivery(&session, briefing.clone());
                } else {
                    self.check_session_quiet(&session, situation, &briefing)
                        .await?;
                }
            } else {
                if let Some(last) = self
                    .last_session(&task.id, |s| {
                        s.task_agent_id.as_deref() == Some(reviewer.id.as_str())
                    })
                    .await
                    && self.spent_on_a_dead_launch(&reviewer.id, &task.id, &last)
                {
                    warn!(task = %task.id, session = %last.id, "the reviewer came up and was never heard from");
                    if self
                        .record_spawn_failure(&task.id, "its agent stopped as soon as it started")
                        .await
                    {
                        return Ok(());
                    }
                }
                info!(task = %task.id, reviewer = %reviewer.id, "starting a reviewer for the pick");
                self.pick_briefed
                    .insert((task.id.clone(), reviewer.id.clone()));
                self.launcher
                    .resume_reviewer(&task.id, &reviewer.id, &briefing)
                    .await?;
            }
        }
        Ok(())
    }

    /// Finish a settled pick, from wherever the last pass got: the winner's
    /// worktree onto the task, the losers removed, and the task moved to
    /// `approved`.
    ///
    /// Idempotent by construction, because `picked_agent_id` is written
    /// before any of it: a daemon that dies between the pick and the
    /// approval leaves a task that is `under_review` with a winner on it,
    /// and [`Self::reconcile_task`] routes that state straight back here —
    /// on the startup pass, and on every tick until the approval commits.
    async fn settle_pick(&mut self, task: &Task, winner_id: &str) -> anyhow::Result<()> {
        // The winner's worktree becomes the task's own, which is what the
        // landing resumes the author in.
        if task.worktree_path.is_none()
            && let Some(worktree) = self
                .last_session(&task.id, |s| {
                    s.task_agent_id.as_deref() == Some(winner_id) && s.worktree_path.is_some()
                })
                .await
                .and_then(|s| s.worktree_path)
        {
            self.store
                .set_task_worktree(&task.id, Some(&worktree))
                .await?;
        }
        // The losers go before the approval: their changes were judged and
        // set aside, and nothing later comes back for their branches or
        // worktrees.
        self.launcher.cleanup_losing_authors(&task.id).await?;
        self.store
            .transition_task(
                &task.id,
                TaskStatus::Approved,
                Actor::Daemon,
                Some(&format!("the reviewers picked author {winner_id}")),
                None,
            )
            .await?;
        Box::pin(self.reconcile_task(&task.id)).await
    }

    /// One author of a contested task, watched the way [`Self::check_stall`]
    /// watches a lone one: started again where its session is gone, nudged
    /// where it has gone quiet, and its dead launches spent against the
    /// task's budget.
    async fn check_contested_author(
        &mut self,
        task: &Task,
        author: &TaskAgent,
        situation: String,
    ) -> anyhow::Result<()> {
        // What this author is picked up or nudged with: its own branch, in
        // the words a lone author gets.
        let seen = Task {
            branch: author_branch(&task.branch, author.ordinal),
            ..task.clone()
        };
        let template = prompts::template_for(PromptKind::AuthorResume);
        let resume = prompts::author_resume_briefing(template, &seen);

        let Some(session) = self.live_author_session(&task.id, &author.id).await? else {
            if let Some(last) = self
                .last_session(&task.id, |s| {
                    s.seat() == Seat::Author
                        && s.task_agent_id.as_deref() == Some(author.id.as_str())
                })
                .await
                && self.spent_on_a_dead_launch(&author.id, &task.id, &last)
            {
                warn!(task = %task.id, session = %last.id, "the author came up and was never heard from");
                if self
                    .record_spawn_failure(&task.id, "its agent stopped as soon as it started")
                    .await
                {
                    return Ok(());
                }
            }
            info!(task = %task.id, author = %author.id, "the task is waiting on an author and has none live, starting one");
            self.launcher
                .resume_author_agent(&task.id, &author.id, &resume)
                .await?;
            return Ok(());
        };
        self.spent_on_a_dead_launch(&author.id, &task.id, &session);
        self.check_session_quiet(&session, situation, &resume).await
    }

    /// The live session one staffed author runs, if it has one — a pane tmux
    /// would not answer for counts, as everywhere.
    async fn live_author_session(
        &self,
        task_id: &str,
        agent_id: &str,
    ) -> anyhow::Result<Option<AgentSession>> {
        self.live_agent_session(task_id, agent_id, Seat::Author)
            .await
    }

    /// The live session one staffed reviewer runs, if it has one.
    async fn live_reviewer_session(
        &self,
        task_id: &str,
        agent_id: &str,
    ) -> anyhow::Result<Option<AgentSession>> {
        self.live_agent_session(task_id, agent_id, Seat::Reviewer)
            .await
    }

    async fn live_agent_session(
        &self,
        task_id: &str,
        agent_id: &str,
        seat: Seat,
    ) -> anyhow::Result<Option<AgentSession>> {
        let live = self
            .store
            .list_sessions(SessionFilter {
                task_id: Some(task_id.to_string()),
                live_only: true,
                ..Default::default()
            })
            .await?;
        for session in live {
            if session.seat() == seat
                && session.task_agent_id.as_deref() == Some(agent_id)
                && self.launcher.session_process_alive(&session).await
            {
                return Ok(Some(session));
            }
        }
        Ok(None)
    }

    /// One reviewer that owes a verdict on one author's review of a contested
    /// task: resumed onto that author's branch where its session is gone,
    /// and — where its pane survived the last review — handed this one's
    /// briefing the moment it owes it, its worktree moved to the branch the
    /// briefing names first. The quiet clock watches it from there.
    async fn rouse_reviewer_for(
        &mut self,
        task: &Task,
        reviewer: &TaskAgent,
        author: &TaskAgent,
        request: &str,
    ) -> anyhow::Result<()> {
        let summary = self
            .store
            .author_review_summary(&task.id, &author.id)
            .await?;
        let summary = launcher::verdict_addressed_to(&author.id, summary.as_deref());
        let seen = Task {
            branch: author_branch(&task.branch, author.ordinal),
            ..task.clone()
        };
        let template = prompts::template_for(PromptKind::ReviewerResume);
        let resume = prompts::reviewer_resume_briefing(template, &seen, Some(&summary));
        let situation = format!("under_review:{request}");
        let briefed = (reviewer.id.clone(), request.to_string());

        if let Some(session) = self.live_reviewer_session(&task.id, &reviewer.id).await? {
            self.spent_on_a_dead_launch(&session.id, &task.id, &session);
            if !self.pane_busy(&session.id) && !self.review_briefed.contains(&briefed) {
                // A live pane is briefed the way a resumed one is, and at the
                // same moment: when the verdict becomes owed, not when the
                // quiet clock notices. Its detached worktree moves first, so
                // the briefing lands in a tree already on the branch it
                // names. A tree that cannot move yet — a branch with nothing
                // on it — leaves the reviewer to the quiet clock and the
                // next pass.
                if let Err(e) = self
                    .launcher
                    .refresh_reviewer_worktree(&task.id, &reviewer.id, Some(&author.id))
                    .await
                {
                    warn!(task = %task.id, reviewer = %reviewer.id, error = %format!("{e:#}"), "moving the reviewer's worktree failed");
                    return self.check_session_quiet(&session, situation, &resume).await;
                }
                info!(task = %task.id, reviewer = %reviewer.id, author = %author.id, "briefing the live reviewer for this author's review");
                self.review_briefed.insert(briefed);
                self.spawn_delivery(&session, resume.clone());
                // The briefing is this review request's delivery: generic
                // delivery leaves a contested request alone, so the channel's
                // stamp is written here, as the briefing goes out.
                self.store
                    .mark_review_requests_delivered(&task.id, &author.id, &reviewer.id)
                    .await?;
                return Ok(());
            }
            self.check_session_quiet(&session, situation, &resume)
                .await?;
        } else {
            if let Some(last) = self
                .last_session(&task.id, |s| {
                    s.task_agent_id.as_deref() == Some(reviewer.id.as_str())
                })
                .await
                && self.spent_on_a_dead_launch(&last.id, &task.id, &last)
            {
                warn!(task = %task.id, session = %last.id, "the reviewer came up and was never heard from");
                if self
                    .record_spawn_failure(&task.id, "its agent stopped as soon as it started")
                    .await
                {
                    return Ok(());
                }
            }
            info!(task = %task.id, reviewer = %reviewer.id, author = %author.id, "starting reviewer");
            // The resume carries this briefing itself: the live path above
            // must not say it again to the session that comes up with it.
            self.review_briefed.insert(briefed);
            self.launcher
                .resume_reviewer_for(&task.id, &reviewer.id, Some(&author.id), &resume)
                .await?;
            // The launch's briefing is this review request's delivery, the
            // same as the live pane's above.
            self.store
                .mark_review_requests_delivered(&task.id, &author.id, &reviewer.id)
                .await?;
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
            if s.seat() == seat && self.launcher.session_process_alive(&s).await {
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
    /// which is one nudge per situation, then the user, then a relaunch.
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
        self.check_session_quiet(agent, review_situation(task, &self.store).await?, &nudge)
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
        let session = self.launcher.resume_author(&task.id, &instruction).await?;
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
        // On a task the reviewers picked a winner for, the branch every
        // briefing names is the winner's own: that is the change that lands.
        let seen = match self
            .launcher
            .review_branch(task, task.picked_agent_id.as_deref())
            .await?
        {
            Some(branch) => Task {
                branch,
                ..task.clone()
            },
            None => task.clone(),
        };
        if task.status() == TaskStatus::Approved {
            let repo = self.store.get_repository(&task.repo_id).await?;
            // The procedure is the task's: how this task ends was agreed with
            // the user when it was written, and it is the whole of what
            // decides which of the three the author runs.
            return Ok(prompts::landing_briefing(
                task.landing_prompt_text(),
                &seen,
                &repo,
            ));
        }
        let template = prompts::template_for(PromptKind::AuthorResume);
        Ok(prompts::author_resume_briefing(template, &seen))
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
