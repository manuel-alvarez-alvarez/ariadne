//! What a goal wants, by status: a planner while it is being planned, its
//! tasks while it is active, and nothing running once it is over.

use tracing::{debug, info, warn};

use ariadne_core::{
    Actor, AttentionReason, GoalStatus, PromptKind, Role, SessionStatus, TaskStatus,
};
use ariadne_store::{AgentSession, Goal, SessionFilter, TaskFilter};

use crate::agents::prompts;

use super::SPAWN_RETRY_BUDGET;

impl super::Scheduler {
    pub(super) async fn reconcile_goal(&mut self, goal_id: &str) -> anyhow::Result<()> {
        let goal = self.store.get_goal(goal_id).await?;
        match goal.status() {
            // A goal in planning wants a live planner session.
            GoalStatus::Planning => {
                let planners = self.live_sessions(goal_id, None, Role::Planner).await?;
                if !planners.is_empty() {
                    // The goal has the planner it wants, however it came by
                    // one: whatever earlier attempts spent is given back.
                    self.spawn_failures.remove(goal_id);
                } else if self.planner_wanted(&goal).await {
                    info!(goal = %goal.id, "spawning planner");
                    if let Err(e) = self.launcher.spawn_planner(goal_id).await {
                        self.planner_could_not_start(&goal).await;
                        return Err(e);
                    }
                    self.spawn_failures.remove(goal_id);
                }
                // A planner has no task to flag: its session carries the
                // stall, which is the only place a goal still in planning has
                // to say that nothing is happening.
                for planner in planners {
                    let template = prompts::template_for(PromptKind::PlannerResume);
                    let nudge = prompts::planner_resume_briefing(template, &goal);
                    self.check_session_quiet(&planner, (goal.status.clone(), 0), &nudge)
                        .await?;
                }
            }
            GoalStatus::Active => {
                // The plan is finalized, which is the planner's hand-off:
                // what it is owed before it is let go is the compaction of
                // everything it said and read getting there.
                self.owe_planner_compaction(&goal).await;
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

    /// Whether to have another go at starting this goal's planner.
    ///
    /// Not for ever. `spawn_planner` writes the session row before it
    /// launches, so an attempt that dies on the way leaves a `starting` row
    /// the liveness sweep retires and flags `disconnected` — and a goal in
    /// planning always wants a planner, so a launch nothing on this machine
    /// can perform (a model the agent CLI does not know, a CLI that is not
    /// installed) would put a fresh alarm on the strip every tick, for ever.
    /// [`SPAWN_RETRY_BUDGET`] attempts is what it gets, the same budget a
    /// task's engineer spends, out of the same map.
    ///
    /// What holds the daemon back afterwards is the alarm itself rather than
    /// the count, which is how a task retried out of `failed` gets a clean
    /// one: taking the flag down is the user saying they have dealt with what
    /// stopped it — resuming that session does exactly that
    /// (`restart_session`) — and the next pass starts again from zero. A
    /// spawn that never got as far as a row has nothing to raise and nothing
    /// to take down, so there the count alone holds it, until a daemon
    /// restart drops it with the rest of the map.
    async fn planner_wanted(&mut self, goal: &Goal) -> bool {
        if self.spawn_failures.get(&goal.id).copied().unwrap_or(0) < SPAWN_RETRY_BUDGET {
            return true;
        }
        // A store that would not say leaves the pass concluding nothing, the
        // way the sweep does with a tmux it cannot reach.
        let Some(planners) = self.planner_sessions(&goal.id).await else {
            return false;
        };
        match alarm_row(&planners) {
            Some(alarm) if alarm.attention_reason() == Some(AttentionReason::Disconnected) => false,
            None => false,
            _ => {
                info!(goal = %goal.id, "the planner's alarm was dealt with; trying to start one again");
                self.spawn_failures.remove(&goal.id);
                true
            }
        }
    }

    /// Count a planner spawn that did not get off the ground, and leave one
    /// row per goal saying anything about it.
    ///
    /// The row this attempt wrote is retired here rather than left for the
    /// liveness sweep to find, and so is every row an earlier attempt left:
    /// what the user has to see is a planner that will not start, once, not a
    /// line per attempt. Only rows that never launched are touched — a
    /// session that really had an agent and lost it is news of its own, and
    /// the sweep's to tell. While the budget lasts nothing new is raised, the
    /// next pass being about to try again; when it runs out the alarm goes up
    /// on the row that is left, and the goal's own thread says why.
    async fn planner_could_not_start(&mut self, goal: &Goal) {
        let failures = self.spawn_failures.entry(goal.id.clone()).or_insert(0);
        *failures += 1;
        let failures = *failures;
        let Some(planners) = self.planner_sessions(&goal.id).await else {
            return;
        };
        let Some(alarm) = alarm_row(&planners).map(|s| s.id.clone()) else {
            return;
        };
        for session in planners.iter().filter(|s| s.launched_at.is_none()) {
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
        warn!(goal = %goal.id, session = %alarm, failures, "the planner will not start, giving up");
        let _ = self
            .store
            .set_session_attention(&alarm, AttentionReason::Disconnected)
            .await;
    }

    /// The goal's planner sessions, oldest first, or `None` when the store
    /// would not say — which is not the same as a goal that has none.
    async fn planner_sessions(&self, goal_id: &str) -> Option<Vec<AgentSession>> {
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
                .filter(|s| s.role() == Role::Planner)
                .collect(),
        )
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
    /// Ended, not unreachable: what the agent CLI holds is its own history,
    /// not the pane, so a session ended here is one `session resume` puts
    /// back where it was. What is not ended is a planner mid-turn: whatever
    /// it is writing is finished first, and the next tick finds it idle. Nor
    /// one still owed its compaction: that history is exactly what the
    /// compaction shortens, and a pane killed under a running one leaves the
    /// conversation as it was.
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
            if self.compaction_pending(planner) {
                debug!(goal = %goal_id, session = %planner.id, "the idle planner is left up until the compaction it owes is done");
                continue;
            }
            info!(goal = %goal_id, session = %planner.id, "the goal is past planning; ending its idle planner");
            if let Err(e) = self.launcher.kill_session(&planner.id).await {
                warn!(goal = %goal_id, session = %planner.id, error = %e, "ending the planner failed");
            }
        }
    }

    /// Owe the goal's live planners the compaction their hand-off earns,
    /// once: the plan is finalized, and the situation that names is the goal
    /// being active.
    async fn owe_planner_compaction(&mut self, goal: &Goal) {
        let Ok(planners) = self.live_sessions(&goal.id, None, Role::Planner).await else {
            return;
        };
        for planner in planners {
            self.owe_compaction(&planner, (goal.status.clone(), 0))
                .await;
        }
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

/// The row a goal's planner trouble is told on: the one already saying it
/// where there is one — a pane that vanished under a planner that was running
/// is flagged by the sweep, and that is this same trouble seen a moment
/// earlier — and otherwise the last attempt's own row.
fn alarm_row(planners: &[AgentSession]) -> Option<&AgentSession> {
    planners
        .iter()
        .find(|s| s.attention_reason() == Some(AttentionReason::Disconnected))
        .or_else(|| planners.last())
}
