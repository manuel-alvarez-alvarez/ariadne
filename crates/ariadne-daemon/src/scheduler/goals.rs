//! What a goal wants, by status: an orchestrator while it is being planned, its
//! tasks while it is active, and nothing running once it is over.

use tracing::{info, warn};

use ariadne_core::{
    Actor, AttentionReason, GoalStatus, PromptKind, Seat, SessionStatus, TaskStatus,
};
use ariadne_store::{AgentSession, Goal, SessionFilter, Task, TaskFilter};

use crate::agents::prompts;

use super::SPAWN_RETRY_BUDGET;

impl super::Scheduler {
    pub(super) async fn reconcile_goal(&mut self, goal_id: &str) -> anyhow::Result<()> {
        let goal = self.store.get_goal(goal_id).await?;
        match goal.status() {
            // A goal in planning wants a live orchestrator session.
            GoalStatus::Planning => {
                self.keep_orchestrator(&goal).await?;
                self.deliver_goal_messages(goal_id).await;
                // An orchestrator has no task to flag: its session carries the
                // stall, which is the only place a goal still in planning has
                // to say that nothing is happening.
                for orchestrator in self
                    .live_sessions(goal_id, None, Seat::Orchestrator)
                    .await?
                {
                    let template = prompts::template_for(PromptKind::OrchestratorResume);
                    let nudge = prompts::orchestrator_resume_briefing(template, &goal);
                    self.check_session_quiet(&orchestrator, goal.status.clone(), &nudge)
                        .await?;
                }
            }
            // A goal under way wants its orchestrator too. It is the one agent
            // that outlives its own hand-off: the user talks to it about work
            // already running, and the daemon has somebody to tell when a task
            // needs a decision no author can make. What it is *not* under way
            // is quiet — an orchestrator with every task running has nothing
            // to do — so no watchdog nudges it here. It is woken by what
            // happened, and by nothing else.
            GoalStatus::Active => {
                self.keep_orchestrator(&goal).await?;
                // What the agents have said to the orchestrator, before it is
                // told anything the daemon noticed: an agent waiting on an
                // answer is waiting on this pass.
                self.deliver_goal_messages(goal_id).await;
                let tasks = self
                    .store
                    .list_tasks(TaskFilter {
                        goal_id: Some(goal.id.clone()),
                        status: None,
                    })
                    .await?;
                self.tell_orchestrator(&goal, &tasks).await?;
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

    /// Whether to have another go at starting this goal's orchestrator.
    ///
    /// Not for ever. `spawn_orchestrator` writes the session row before it
    /// launches, so an attempt that dies on the way leaves a `starting` row
    /// the liveness sweep retires and flags `disconnected` — and a goal in
    /// planning always wants an orchestrator, so a launch nothing on this
    /// machine can perform (a model the agent CLI does not know, a CLI that
    /// is not installed) would put a fresh alarm on the strip every tick, for
    /// ever. [`SPAWN_RETRY_BUDGET`] attempts is what it gets, the same budget
    /// a task's author spends, out of the same map.
    ///
    /// What holds the daemon back afterwards is the alarm itself rather than
    /// the count, which is how a task retried out of `failed` gets a clean
    /// one: taking the flag down is the user saying they have dealt with what
    /// stopped it — resuming that session does exactly that
    /// (`restart_session`) — and the next pass starts again from zero. A
    /// spawn that never got as far as a row has nothing to raise and nothing
    /// to take down, so there the count alone holds it, until a daemon
    /// restart drops it with the rest of the map.
    async fn orchestrator_wanted(&mut self, goal: &Goal) -> bool {
        if self.spawn_failures.get(&goal.id).copied().unwrap_or(0) < SPAWN_RETRY_BUDGET {
            return true;
        }
        // A store that would not say leaves the pass concluding nothing, the
        // way the sweep does with a tmux it cannot reach.
        let Some(orchestrators) = self.orchestrator_sessions(&goal.id).await else {
            return false;
        };
        match alarm_row(&orchestrators) {
            Some(alarm) if alarm.attention_reason() == Some(AttentionReason::Disconnected) => false,
            None => false,
            _ => {
                info!(goal = %goal.id, "the orchestrator's alarm was dealt with; trying to start one again");
                self.spawn_failures.remove(&goal.id);
                true
            }
        }
    }

    /// Count an orchestrator spawn that did not get off the ground, and leave
    /// one row per goal saying anything about it.
    ///
    /// The row this attempt wrote is retired here rather than left for the
    /// liveness sweep to find, and so is every row an earlier attempt left:
    /// what the user has to see is an orchestrator that will not start, once,
    /// not a line per attempt. Only rows that never launched are touched — a
    /// session that really had an agent and lost it is news of its own, and
    /// the sweep's to tell. While the budget lasts nothing new is raised, the
    /// next pass being about to try again; when it runs out the alarm goes up
    /// on the row that is left, and the goal's own thread says why.
    async fn orchestrator_could_not_start(&mut self, goal: &Goal) {
        let failures = self.spawn_failures.entry(goal.id.clone()).or_insert(0);
        *failures += 1;
        let failures = *failures;
        let Some(orchestrators) = self.orchestrator_sessions(&goal.id).await else {
            return;
        };
        let Some(alarm) = alarm_row(&orchestrators).map(|s| s.id.clone()) else {
            return;
        };
        for session in orchestrators.iter().filter(|s| s.launched_at.is_none()) {
            if session.status().is_live() {
                let _ = self
                    .store
                    .set_session_status(&session.id, SessionStatus::Exited)
                    .await;
            }
            if session.id != alarm
                && session.attention_reason() == Some(AttentionReason::Disconnected)
            {
                let _ = self.store.clear_session_attention(&session.id).await;
            }
        }
        if failures < SPAWN_RETRY_BUDGET {
            return;
        }
        warn!(goal = %goal.id, session = %alarm, failures, "the orchestrator will not start, giving up");
        let _ = self
            .store
            .set_session_attention(&alarm, AttentionReason::Disconnected)
            .await;
    }

    /// The goal's orchestrator sessions, oldest first, or `None` when the store
    /// would not say — which is not the same as a goal that has none.
    async fn orchestrator_sessions(&self, goal_id: &str) -> Option<Vec<AgentSession>> {
        let sessions = self
            .store
            .list_sessions(SessionFilter {
                goal_id: Some(goal_id.to_string()),
                ..Default::default()
            })
            .await
            .ok()?;
        Some(
            sessions
                .into_iter()
                .filter(|s| s.seat() == Seat::Orchestrator)
                .collect(),
        )
    }

    /// Make sure this goal has an orchestrator, whatever it is doing.
    ///
    /// One per goal, for the whole goal: the agent the user talks to, and the
    /// only one that holds the plan. A goal whose orchestrator has been heard
    /// from gives back whatever earlier attempts spent; one that has none gets
    /// another go, until [`Self::orchestrator_wanted`] says the budget is out.
    ///
    /// Heard from, rather than merely started, because a launch that works and
    /// an agent that runs are not the same thing. An agent that comes up and
    /// exits on a dialog nobody answered leaves the seat empty again within a
    /// tick, and a budget handed back at every launch is no budget at all: the
    /// goal starts an orchestrator every five seconds for as long as it lives,
    /// and nothing ever reaches the user, since the alarm each death raises is
    /// cleared by the replacement that dies the same way. So a death on
    /// arrival spends an attempt like a launch that never got off the ground,
    /// and ends the same way — one alarm, on one row, and the daemon stops.
    async fn keep_orchestrator(&mut self, goal: &Goal) -> anyhow::Result<()> {
        let live = self
            .live_sessions(&goal.id, None, Seat::Orchestrator)
            .await?;
        if let Some(orchestrator) = live.first() {
            self.spent_on_a_dead_launch(&goal.id, &goal.id, orchestrator);
            return Ok(());
        }
        if let Some(orchestrators) = self.orchestrator_sessions(&goal.id).await
            && let Some(last) = orchestrators.last()
            && self.spent_on_a_dead_launch(&goal.id, &goal.id, last)
        {
            warn!(goal = %goal.id, session = %last.id, "the orchestrator came up and was never heard from");
            self.orchestrator_could_not_start(goal).await;
        }
        if !self.orchestrator_wanted(goal).await {
            return Ok(());
        }
        info!(goal = %goal.id, "spawning orchestrator");
        if let Err(e) = self.launcher.spawn_orchestrator(&goal.id).await {
            self.orchestrator_could_not_start(goal).await;
            return Err(e);
        }
        Ok(())
    }

    /// Tell the orchestrator what its tasks need, once per situation.
    ///
    /// Three things it is woken for, and the text names whichever of them is
    /// true: a task that failed, a task that has gone quiet, and a goal with
    /// nothing left to do. Everything else is work in progress, which is
    /// exactly what the orchestrator delegated and has no business
    /// interrupting.
    ///
    /// Once per situation rather than once per pass: the line the tasks render
    /// to is the key, so a second task failing is news and the same one still
    /// failed is not. Typed into the pane rather than sent as a resume, since
    /// the session is up and the user may be mid-conversation with it.
    async fn tell_orchestrator(&mut self, goal: &Goal, tasks: &[Task]) -> anyhow::Result<()> {
        let Some(situation) = goal_attention(tasks) else {
            self.goal_told.remove(&goal.id);
            return Ok(());
        };
        if self.goal_told.get(&goal.id) == Some(&situation) {
            return Ok(());
        }
        let orchestrators = self
            .live_sessions(&goal.id, None, Seat::Orchestrator)
            .await?;
        let Some(orchestrator) = orchestrators
            .iter()
            .find(|s| s.status() == SessionStatus::Idle)
        else {
            // Nothing to type into: an orchestrator still starting, or one
            // mid-turn. The situation is not written down, so the pass that
            // finds it idle says it then.
            return Ok(());
        };
        if self.pane_busy(&orchestrator.id) {
            return Ok(());
        }
        info!(goal = %goal.id, session = %orchestrator.id, "the goal's tasks need the orchestrator");
        self.goal_told.insert(goal.id.clone(), situation.clone());
        let template = prompts::template_for(PromptKind::GoalAttention);
        let text = prompts::goal_attention_briefing(template, goal, &situation);
        self.spawn_delivery(orchestrator, text);
        Ok(())
    }

    /// Every live session of a goal, whatever ended it.
    async fn kill_goal_sessions(&self, goal_id: &str) {
        self.kill_sessions(
            SessionFilter {
                goal_id: Some(goal_id.to_string()),
                live_only: true,
                ..Default::default()
            },
            None,
            "the goal it belongs to is finished",
        )
        .await;
    }
}

/// The row a goal's orchestrator trouble is told on: the one already saying
/// it where there is one — a pane that vanished under an orchestrator that
/// was running is flagged by the sweep, and that is this same trouble seen a
/// moment earlier — and otherwise the last attempt's own row.
fn alarm_row(orchestrators: &[AgentSession]) -> Option<&AgentSession> {
    orchestrators
        .iter()
        .find(|s| s.attention_reason() == Some(AttentionReason::Disconnected))
        .or_else(|| orchestrators.last())
}

/// What the tasks of a goal need from its orchestrator, one line each, or
/// `None` where they need nothing.
///
/// Three situations, and a goal can be in more than one. A failed task and a
/// stalled task are both decisions somebody has to make and no author can:
/// retry it, rewrite it, staff it differently, or give it up. A goal whose
/// tasks are all done is the one moment `complete_goal` is called.
///
/// Work in progress is not in here. The orchestrator delegated it, and a
/// running task is the delegation working.
fn goal_attention(tasks: &[Task]) -> Option<String> {
    let mut lines: Vec<String> = Vec::new();
    for task in tasks {
        match task.status() {
            TaskStatus::Failed => lines.push(format!("- {} ({}) failed", task.title, task.id)),
            _ if task.is_stalled() => {
                lines.push(format!("- {} ({}) has gone quiet", task.title, task.id));
            }
            _ => {}
        }
    }
    // Every task done is the end of the goal, and it is worth saying on its
    // own line: a goal that ends with a task cancelled and the rest finished
    // is still a goal that is over.
    let done = !tasks.is_empty()
        && tasks
            .iter()
            .all(|t| matches!(t.status(), TaskStatus::Finished | TaskStatus::Cancelled));
    if done {
        lines.push("- Every task is done.".to_string());
    }
    (!lines.is_empty()).then(|| lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::goal_attention;

    use ariadne_store::Task;

    fn task(title: &str, status: &str, stalled: bool) -> Task {
        Task {
            id: format!("01{title}"),
            goal_id: "01goal".into(),
            repo_id: "01repo".into(),
            title: title.into(),
            description: String::new(),
            status: status.into(),
            branch: title.into(),
            landing: "merge".into(),
            permission_mode: None,
            worktree_path: None,
            stalled: stalled as i64,
            merge_commit: None,
            pr_url: None,
            picked_agent_id: None,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    /// Work in progress is what the orchestrator delegated, so it is not
    /// woken for it.
    #[test]
    fn a_goal_whose_tasks_are_running_needs_nothing() {
        assert_eq!(
            goal_attention(&[
                task("a", "in_progress", false),
                task("b", "under_review", false)
            ]),
            None
        );
        // Nor is a goal with no tasks at all, which is a plan being written.
        assert_eq!(goal_attention(&[]), None);
    }

    /// The two decisions no author can make, each named with the task it is
    /// about: the orchestrator has to know which one to act on.
    #[test]
    fn a_failed_task_and_a_quiet_one_are_each_named() {
        let lines = goal_attention(&[
            task("Build it", "failed", false),
            task("Wire it", "in_progress", true),
            task("Ship it", "in_progress", false),
        ])
        .expect("two tasks need it");
        assert!(lines.contains("Build it (01Build it) failed"), "{lines}");
        assert!(
            lines.contains("Wire it (01Wire it) has gone quiet"),
            "{lines}"
        );
        assert!(!lines.contains("Ship it"), "{lines}");
    }

    /// Every task done is the goal over, whether they were finished or
    /// cancelled, and it is the one moment `complete_goal` is called.
    #[test]
    fn a_goal_with_nothing_left_to_do_says_so() {
        let lines = goal_attention(&[task("a", "finished", false), task("b", "cancelled", false)])
            .expect("the goal is over");
        assert_eq!(lines, "- Every task is done.");

        // One task still running and the goal is not over, whatever the rest
        // did.
        assert_eq!(
            goal_attention(&[
                task("a", "finished", false),
                task("b", "in_progress", false)
            ]),
            None
        );
    }
}
