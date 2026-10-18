//! Everything a goal is read with beside its own row, for many goals at once.
//!
//! A goal as the API answers it carries its repositories, its columns, what
//! each seat spent, and what every agent of every task spent. Read goal by
//! goal and task by task, that is hundreds of queries for a list of goals,
//! each a round trip to the database thread, and a busy machine makes every
//! round trip slow. Read here, it is a fixed handful of queries whatever the
//! number of goals, tasks and agents.

use std::collections::HashMap;

use ariadne_core::Seat;

use crate::usage::{SUMS, usage_of};
use crate::{AgentUsage, GoalStep, Result, SeatUsage, Store, TaskAgent};

/// The ids a query is narrowed to, bound as one JSON array that
/// `json_each` reads back.
const GOAL_IDS: &str = "SELECT value FROM json_each(?)";

/// What [`Store::goal_parts`] read, keyed by goal id. A goal with nothing in
/// one of the maps has nothing of that kind.
#[derive(Debug, Default)]
pub struct GoalParts {
    /// The ids of the repositories each goal references, in the order
    /// [`Store::list_goal_repositories`] answers them.
    pub repository_ids: HashMap<String, Vec<String>>,
    /// The columns of each goal, as [`Store::goal_steps`] answers them.
    pub steps: HashMap<String, Vec<GoalStep>>,
    /// What each goal spent, one entry per seat that has a session on it —
    /// its orchestrator, and the agents of every column of its tasks —
    /// ordered by seat. Outer-joined like [`Store::task_usage`], and for the
    /// same reason.
    pub seats: HashMap<String, Vec<SeatUsage>>,
    /// The tasks of each goal, ordered by id like [`Store::list_tasks`].
    pub tasks: HashMap<String, Vec<TaskParts>>,
}

/// One task of a goal, as [`GoalParts`] holds it.
#[derive(Debug)]
pub struct TaskParts {
    pub task_id: String,
    /// The agents staffed on the task in column order, each with the names
    /// of the skills it loads, in the order they are listed to it.
    pub agents: Vec<(TaskAgent, Vec<String>)>,
    /// As [`Store::task_usage`] answers it.
    pub usage: Vec<AgentUsage>,
    /// The skills of an agent in `usage` that is not in `agents`.
    pub unstaffed_skills: HashMap<String, Vec<String>>,
}

impl Store {
    /// Read [`GoalParts`] for every goal in `goal_ids`, in seven queries.
    pub async fn goal_parts(&self, goal_ids: &[String]) -> Result<GoalParts> {
        if goal_ids.is_empty() {
            return Ok(GoalParts::default());
        }
        let ids = serde_json::Value::from(goal_ids.to_vec()).to_string();
        let tasks_of_goals = format!("SELECT id FROM tasks WHERE goal_id IN ({GOAL_IDS})");

        let pairs: Vec<(String, String)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT gr.goal_id, r.id FROM goal_repositories gr
               JOIN repositories r ON r.id = gr.repository_id
              WHERE gr.goal_id IN ({GOAL_IDS})
              ORDER BY gr.goal_id, r.path, r.base_branch"
        )))
        .bind(&ids)
        .fetch_all(self.r())
        .await?;
        let mut parts = GoalParts::default();
        for (goal_id, repository_id) in pairs {
            parts
                .repository_ids
                .entry(goal_id)
                .or_default()
                .push(repository_id);
        }

        let steps: Vec<GoalStep> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT * FROM goal_steps WHERE goal_id IN ({GOAL_IDS}) ORDER BY goal_id, ordinal"
        )))
        .bind(&ids)
        .fetch_all(self.r())
        .await?;
        for step in steps {
            parts
                .steps
                .entry(step.goal_id.clone())
                .or_default()
                .push(step);
        }

        let seats: Vec<(String, String, i64, i64, i64)> =
            sqlx::query_as(sqlx::AssertSqlSafe(format!(
                "SELECT s.goal_id, s.seat, {SUMS}
                   FROM agent_sessions s
              LEFT JOIN session_usage u ON u.session_id = s.id
                  WHERE s.goal_id IN ({GOAL_IDS})
               GROUP BY s.goal_id, s.seat
               ORDER BY s.goal_id, s.seat"
            )))
            .bind(&ids)
            .fetch_all(self.r())
            .await?;
        for (goal_id, seat, input, cached, output) in seats {
            parts.seats.entry(goal_id).or_default().push(SeatUsage {
                seat: seat.parse::<Seat>().expect("valid seat in db"),
                usage: usage_of((input, cached, output)),
            });
        }

        let tasks: Vec<(String, String)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT id, goal_id FROM tasks WHERE goal_id IN ({GOAL_IDS}) ORDER BY id"
        )))
        .bind(&ids)
        .fetch_all(self.r())
        .await?;

        let agents: Vec<TaskAgent> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT * FROM task_agents WHERE task_id IN ({tasks_of_goals})
              ORDER BY task_id, ordinal"
        )))
        .bind(&ids)
        .fetch_all(self.r())
        .await?;

        // The skills of every agent that is staffed on one of the tasks or
        // has a session on one, which is every agent the usage names.
        let skills: Vec<(String, String)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT a.agent_id, s.name FROM task_agent_skills a
               JOIN skills s ON s.name = a.skill_name
              WHERE a.agent_id IN (SELECT id FROM task_agents
                                    WHERE task_id IN ({tasks_of_goals}))
                 OR a.agent_id IN (SELECT task_agent_id FROM agent_sessions
                                    WHERE task_id IN ({tasks_of_goals}))
              ORDER BY a.agent_id, a.ordinal"
        )))
        .bind(&ids)
        .bind(&ids)
        .fetch_all(self.r())
        .await?;
        let mut skills_of: HashMap<String, Vec<String>> = HashMap::new();
        for (agent_id, name) in skills {
            skills_of.entry(agent_id).or_default().push(name);
        }

        let spent: Vec<(String, String, i64, i64, i64)> =
            sqlx::query_as(sqlx::AssertSqlSafe(format!(
                "SELECT s.task_id, s.task_agent_id, {SUMS}
                   FROM agent_sessions s
              LEFT JOIN session_usage u ON u.session_id = s.id
                  WHERE s.task_id IN ({tasks_of_goals}) AND s.task_agent_id IS NOT NULL
               GROUP BY s.task_id, s.task_agent_id
               ORDER BY s.task_id, s.task_agent_id"
            )))
            .bind(&ids)
            .fetch_all(self.r())
            .await?;

        let mut agents_of: HashMap<String, Vec<(TaskAgent, Vec<String>)>> = HashMap::new();
        for agent in agents {
            let skills = skills_of.get(&agent.id).cloned().unwrap_or_default();
            agents_of
                .entry(agent.task_id.clone())
                .or_default()
                .push((agent, skills));
        }
        let mut usage_of_task: HashMap<String, Vec<AgentUsage>> = HashMap::new();
        for (task_id, agent_id, input, cached, output) in spent {
            usage_of_task.entry(task_id).or_default().push(AgentUsage {
                agent_id,
                usage: usage_of((input, cached, output)),
            });
        }
        for (task_id, goal_id) in tasks {
            let agents = agents_of.remove(&task_id).unwrap_or_default();
            let usage = usage_of_task.remove(&task_id).unwrap_or_default();
            let unstaffed_skills = usage
                .iter()
                .filter(|spent| agents.iter().all(|(a, _)| a.id != spent.agent_id))
                .map(|spent| {
                    let skills = skills_of.get(&spent.agent_id).cloned().unwrap_or_default();
                    (spent.agent_id.clone(), skills)
                })
                .collect();
            parts.tasks.entry(goal_id).or_default().push(TaskParts {
                task_id,
                agents,
                usage,
                unstaffed_skills,
            });
        }
        Ok(parts)
    }
}
