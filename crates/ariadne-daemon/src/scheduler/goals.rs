//! What a goal wants, by status: a planner while it is being planned, its
//! tasks while it is active, and nothing running once it is over.

use tracing::{info, warn};

use ariadne_core::{Actor, GoalStatus, PromptKind, Role, SessionStatus, TaskStatus};
use ariadne_store::{SessionFilter, TaskFilter};

use crate::agents::prompts;
use crate::notify;

impl super::Scheduler {
    pub(super) async fn reconcile_goal(&mut self, goal_id: &str) -> anyhow::Result<()> {
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
                    let template = prompts::template_for(
                        &self.store,
                        &planner.profile_id,
                        PromptKind::PlannerResume,
                    )
                    .await;
                    let nudge = prompts::planner_resume_briefing(&template, &goal);
                    self.check_session_quiet(&planner, (goal.status.clone(), 0), &nudge)
                        .await?;
                }
            }
            GoalStatus::Active => {
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
    /// Ended, not unreachable: the goal's tasks may still have something to
    /// say to their planner, and a message addressed to an exited session is
    /// what `wake_profile` revives it with — the conversation is in the agent
    /// CLI's own history, not in the pane. What is not ended is a planner
    /// mid-turn: whatever it is writing is finished first, and the next tick
    /// finds it idle.
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
            info!(goal = %goal_id, session = %planner.id, "the goal is past planning; ending its idle planner");
            if let Err(e) = self.launcher.kill_session(&planner.id).await {
                warn!(goal = %goal_id, session = %planner.id, error = %e, "ending the planner failed");
            }
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
