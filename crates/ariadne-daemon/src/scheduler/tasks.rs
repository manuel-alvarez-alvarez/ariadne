//! What a task wants, by status: its dependencies watched while it waits, the
//! agent of its current column from `ready` to the last column, and the
//! cleanup its ending owes.

use tracing::{info, warn};

use ariadne_core::{
    Actor, AttentionReason, GoalStatus, PromptKind, Seat, SessionStatus, TaskStatus,
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
        // The branch is only followed while somebody is working on it.
        // Finished and cancelled tasks are let go by the cleanup below, but a
        // failed one keeps its worktree — a user can retry it — and until one
        // does there is nobody committing on its branch to report.
        if !launcher::worth_following(&task) {
            self.launcher.branches.unwatch(&task.id);
        }
        // Heap-allocate the large branch future. Dependency reconciliation
        // can poll this method recursively, so its inline state multiplies on
        // the stack even when a different branch owns the current task.
        Box::pin(self.reconcile_steps(task)).await
    }

    /// One pass over a stepped task: the dependencies it waits on, the column
    /// it is in and the one agent of that column, and the cleanup its ending
    /// owes.
    async fn reconcile_steps(&mut self, mut task: Task) -> anyhow::Result<()> {
        match task.status() {
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
                return Ok(());
            }
            TaskStatus::Failed | TaskStatus::Cancelled => {
                // Kill leftover agents; always keep worktrees and branch — a
                // failed or cancelled task may hold uncommitted work worth
                // salvaging, and a failed one may be retried into it.
                self.launcher.cleanup_task(&task.id, false, false).await?;
                return Ok(());
            }
            TaskStatus::Pending => {
                // A dependency that ended without finishing is never going
                // to, so neither is the wait on it: the task ends too, naming
                // what stopped it, and the user can retry it once the
                // dependency has landed. Only the terminal endings count — a
                // dependency retried and working again is one this task can
                // still wait for. A goal being cancelled never reaches here:
                // its tasks are cancelled by `reconcile_goal`, and this pass
                // returns above as soon as the goal is no longer active.
                if let Some(blocker) = self.store.task_dependencies_blocked(&task.id).await? {
                    let reason = blocked_reason(&blocker);
                    warn!(task = %task.id, dependency = %blocker.id, "dependency ended unfinished, failing task");
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
                if !self.store.task_dependencies_merged(&task.id).await? {
                    return Ok(());
                }
                info!(task = %task.id, "dependencies finished, task ready");
                task = self
                    .store
                    .transition_task(&task.id, TaskStatus::Ready, Actor::Daemon, None, None)
                    .await?;
            }
            _ => {}
        }
        if task.status() == TaskStatus::Ready && Box::pin(self.wait_for_dependencies(&task)).await?
        {
            return Ok(());
        }
        if task.status() == TaskStatus::Ready {
            // A task starts on its first column and walks every one of them,
            // so each needs its agent before the first is started. The store
            // refuses a staffing that leaves one out once the goal runs, and
            // `finalize_plan` and a retry refuse by name; what reaches here
            // unstaffed is a task a database from before workflows carried
            // (0023). It fails naming the column rather than spending its
            // launch budget on an agent that is not there, and the retry is
            // refused until the orchestrator staffs it.
            let unstaffed = self.store.unstaffed_columns(&task).await?;
            if !unstaffed.is_empty() {
                let reason = format!(
                    "no agent on column {}; staff it with update_task and retry",
                    unstaffed.join(", ")
                );
                warn!(task = %task.id, columns = %unstaffed.join(", "), "a column has no agent, failing the task before it starts");
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
            task = self.store.start_first_step(&task.id).await?;
        }
        // Whatever the agents of this task have said to each other since the
        // last pass, delivered before anything else is decided: a question
        // answered is what unblocks the agent that asked, and the answer is
        // no use to it a pass later than it could have had it. A task in no
        // column has no agent to tell, but what its agents said to the
        // orchestrator still reaches it (018).
        self.deliver_task_messages(&task.id).await;
        if task.status() != TaskStatus::InProgress {
            return Ok(());
        }
        let steps = self.store.goal_steps(&task.goal_id).await?;
        let step = steps
            .iter()
            .find(|s| Some(&s.id) == task.step.as_ref())
            .ok_or_else(|| anyhow::anyhow!("task has no current column"))?;
        // A merged request is the request column's to act on, which moves
        // the task on (030).
        if task.pr_url.is_some() && Box::pin(self.merge_ended(&task)).await? {
            return Ok(());
        }
        // The task was read before its transitions, and an agent completing
        // its column in between moves the task on: the newest entry is then
        // the next column's, and briefing the column read above on it would
        // spend the next column's briefing on the agent that just left it.
        // The move wakes a pass of its own, which reads both afresh.
        let transitions = self.store.list_task_transitions(&task.id).await?;
        let entry = transitions
            .last()
            .ok_or_else(|| anyhow::anyhow!("step has no entry transition"))?;
        if entry.to_status != "in_progress" || entry.to_step.as_ref() != Some(&step.id) {
            return Ok(());
        }
        let agents = self.store.list_task_agents(&task.id).await?;
        let agent = agents
            .iter()
            .find(|a| Some(&a.step) == task.step.as_ref())
            .ok_or_else(|| anyhow::anyhow!("column {} has no agent", step.id))?;
        let sessions = self
            .store
            .list_sessions(SessionFilter {
                task_id: Some(task.id.clone()),
                ..Default::default()
            })
            .await?;
        // Only the current column's agent is owed anything: a flag another
        // column's agent still carries says nothing anybody has to act on.
        for session in &sessions {
            if session.task_agent_id.as_ref() != Some(&agent.id)
                && session.attention_reason().is_some()
            {
                self.store.clear_session_attention(&session.id).await?;
            }
        }
        let live = sessions.iter().rev().find(|s| {
            s.task_agent_id.as_ref() == Some(&agent.id) && self.launcher.acp.is_running(&s.id)
        });
        let launched = live.is_none();
        let session = match live {
            Some(session) => {
                // Up and reporting: what earlier attempts at starting this
                // agent spent of the task's budget comes back here rather
                // than at the launch that started it.
                self.spent_on_a_dead_launch(&agent.id, &task.id, session);
                session.clone()
            }
            None => {
                // An agent that came up and was never heard from spends an
                // attempt like a launch that never got off the ground.
                // Without that, a CLI that exits the moment it starts — a
                // dialog nobody answered, a folder it will not open — is a
                // task that starts an agent every tick for as long as its
                // goal is active, and says so to nobody.
                if let Some(last) = sessions
                    .iter()
                    .rev()
                    .find(|s| s.task_agent_id.as_ref() == Some(&agent.id))
                {
                    if last.attention_reason() == Some(AttentionReason::Exhausted) {
                        return Ok(());
                    }
                    if self.spent_on_a_dead_launch(&agent.id, &task.id, last)
                        && self
                            .record_spawn_failure(
                                &task.id,
                                "its agent stopped as soon as it started",
                            )
                            .await
                    {
                        return Ok(());
                    }
                }
                info!(task = %task.id, column = %step.id, "the column has no live agent, starting one");
                match self.launcher.start_step_agent(&task, agent).await {
                    Ok(session) => session,
                    Err(e) => {
                        // The task still wants this agent and could not get
                        // one: the ended session is the thing the user has to
                        // look at.
                        self.flag_last_disconnected(&task, &agent.id).await;
                        return Err(e);
                    }
                }
            }
        };
        if !self.store.step_briefed(&entry.id).await? {
            let seen = transitions[..transitions.len() - 1]
                .iter()
                .any(|t| t.to_status == "in_progress" && t.to_step.as_ref() == Some(&step.id));
            let retry = entry.from_status == "ready"
                && transitions
                    .iter()
                    .any(|t| t.from_status == "failed" && t.to_status == "ready");
            let reason = entry
                .reason
                .as_deref()
                .or_else(|| {
                    if retry {
                        transitions
                            .iter()
                            .rev()
                            .find(|t| t.to_status == "failed")
                            .and_then(|t| t.reason.as_deref())
                    } else {
                        None
                    }
                })
                .unwrap_or("");
            let mut briefing = if seen {
                let direction = if retry {
                    "retry"
                } else if entry.from_step.as_ref().is_some_and(|id| {
                    steps
                        .iter()
                        .any(|s| &s.id == id && s.ordinal > step.ordinal)
                }) {
                    "back"
                } else {
                    "forward"
                };
                prompts::step_return(
                    prompts::template_for(PromptKind::StepReturn),
                    &task,
                    step,
                    direction,
                    reason,
                )
            } else {
                self.launcher
                    .step_first_briefing(&task, step, reason)
                    .await?
            };
            // A fresh conversation on a column the task has visited before
            // has read nothing: it is briefed on the task first, then told
            // what the return says.
            if seen && launched && !sessions.iter().any(|s| s.id == session.id) {
                briefing = format!(
                    "{}\n\n{briefing}",
                    self.launcher
                        .step_first_briefing(&task, step, reason)
                        .await?
                );
            }
            if let Some(brief) = &agent.brief {
                briefing.push_str("\n\n");
                briefing.push_str(brief);
            }
            let handed = self
                .launcher
                .acp
                .send_step(&session.id, &entry.id, briefing);
            self.handed(&session, handed);
            return Ok(());
        }
        let resume =
            prompts::agent_resume(prompts::template_for(PromptKind::AgentResume), &task, step);
        if launched {
            // Whatever the user is owed comes back up with the agent
            // ([`Self::keep_waiting_user`]): starting the agent again is the
            // recovery for the agent, and no answer at all to a message
            // still waiting on them.
            self.keep_waiting_user(&session, None).await?;
            let resume = if sessions.iter().any(|s| s.id == session.id) {
                resume
            } else {
                format!(
                    "{}\n\n{resume}",
                    self.launcher.step_first_briefing(&task, step, "").await?
                )
            };
            self.hand_prompt(&session, resume);
        } else if !(task.pr_url.is_some() && session.status() == SessionStatus::Idle) {
            // An agent that keeps an open request waits on the forge between
            // its news (030): idle there is not quiet, and it is never nudged
            // for it. Only a turn that never ends is watched.
            self.check_session_quiet(&session, entry.id.clone(), &resume)
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

    /// The sessions for this seat whose agent is still running, as the
    /// runtime answers for it. Their number decides whether to spawn.
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
            if s.seat() == Some(seat) && self.launcher.session_process_alive(&s).await {
                out.push(s);
            }
        }
        Ok(out)
    }

    /// Send a ready task whose dependencies have not all finished back to
    /// `pending`, and say whether it was.
    ///
    /// A retry makes a task ready whatever its dependencies say: one that
    /// failed because its dependency failed is retried while that dependency,
    /// retried as well, is still at work. It waits in `pending` again, which
    /// starts it once they finish — or fails it again, naming the dependency,
    /// if one has ended unmerged. The next pass decides which, not this one:
    /// dependencies that move under it would otherwise bounce it between the
    /// two, a level deeper each time. Boxed by its caller, as
    /// `reconcile_task`'s future is already as large as a test thread's
    /// stack holds.
    async fn wait_for_dependencies(&self, task: &Task) -> anyhow::Result<bool> {
        if self.store.task_dependencies_merged(&task.id).await? {
            return Ok(false);
        }
        info!(task = %task.id, "retried before its dependencies finished, waiting for them");
        self.store
            .transition_task(
                &task.id,
                TaskStatus::Pending,
                Actor::Daemon,
                Some("waits for its dependencies to finish"),
                None,
            )
            .await?;
        Ok(true)
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

    /// Put back on the agent that came up what a human still owes its work.
    ///
    /// `waiting_user` is nobody's flag but the user's: it says a person owes
    /// this task something — a message written to them — and putting the
    /// agent underneath back on its feet answers none of it. Both ways of
    /// doing that lose it all the same: a resume revives the row through
    /// `restart_session`, which drops its attention with everything else,
    /// and a spawn that had to start afresh leaves the flag on a row nobody
    /// looks at any more (`clear_superseded_attention`). So it goes back on
    /// the session that came up. `carried` is what the row that went down
    /// was flagged with, for the caller that has that row.
    ///
    /// A request that last read ready to merge is deliberately not read
    /// here any more: whether it still is is the `pull_request` attention
    /// producer's own call, read fresh off the forge's own evidence every
    /// time (031) rather than this session's bare `ready` flag, which on
    /// its own says nothing about a current approval, an open review
    /// comment, a failing check or an unconfirmed mergeability.
    pub(super) async fn keep_waiting_user(
        &self,
        back: &AgentSession,
        carried: Option<AttentionReason>,
    ) -> anyhow::Result<()> {
        if carried.is_some_and(|reason| reason.is_for_the_user()) {
            info!(session = %back.id, seat = ?back.seat, "the agent is back on its feet and the user is still owed, raising it again");
            self.store
                .set_session_attention(&back.id, AttentionReason::WaitingUser)
                .await?;
        }
        Ok(())
    }

    /// Raise `disconnected` on the session that was last this agent's,
    /// whatever state it ended in. Best effort: this runs while another
    /// failure is being reported, and adds nothing to it if it fails too.
    async fn flag_last_disconnected(&self, task: &Task, agent_id: &str) {
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
        if let Some(previous) = sessions
            .iter()
            .rev()
            .find(|s| s.task_agent_id.as_deref() == Some(agent_id))
        {
            warn!(task = %task.id, session = %previous.id, "starting the column's agent failed, flagging its last session disconnected");
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
    use super::*;

    fn dependency(title: &str, id: &str, status: &str) -> Task {
        Task {
            step: None,
            id: id.into(),
            goal_id: "01goal".into(),
            repo_id: "01repo".into(),
            title: title.into(),
            description: String::new(),
            status: status.into(),
            branch: "dep".into(),
            worktree_path: None,
            stalled: 0,
            merge_commit: None,
            pr_url: None,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    /// A task that fails because its dependency ended carries which one and
    /// how, with the id shortened the way the board shows it.
    #[test]
    fn a_blocked_task_names_the_dependency_and_how_it_ended() {
        assert_eq!(
            blocked_reason(&dependency(
                "Build it",
                "01m0sktv47w6b8ze6xf4r9jr7c",
                "failed"
            )),
            "dependency \"Build it\" (…f4r9jr7c) failed"
        );
        assert_eq!(
            blocked_reason(&dependency("Ship it", "short", "cancelled")),
            "dependency \"Ship it\" (short) was cancelled"
        );
    }
}
