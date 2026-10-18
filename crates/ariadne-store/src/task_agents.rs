//! The agents staffed on a task.
//!
//! An agent has no identity of its own: it is a model of a registry agent,
//! an effort, a brief and a set of skills. Its column says only where in the
//! task's workflow it works — which is what the scheduler and the launcher
//! need to know and the whole of what they need.

use ariadne_core::id::new_id;
use sqlx::{Sqlite, Transaction};

use crate::defaults::PR_BABYSIT_SKILL;
use crate::{AgentPin, Result, Skill, SkillSeat, Store, StoreError, TaskAgent};

/// One agent to staff on a task, as the orchestrator describes it.
#[derive(Debug, Clone)]
pub struct NewTaskAgent {
    /// The id of the workflow column this agent works, one of the goal's
    /// columns. A task takes one agent per column.
    pub step: String,
    /// The skills this agent loads, in the order they reach it. An agent with
    /// none takes its column's.
    pub skills: Vec<String>,
    /// What it runs on: its model, `<agent>:<model>`, and the effort where
    /// one was chosen.
    pub pin: AgentPin,
    /// What the orchestrator tells this agent beyond the task itself.
    pub brief: Option<String>,
}

impl NewTaskAgent {
    /// An agent on column `step` on the named skills, on `pin`.
    pub fn new(
        step: impl Into<String>,
        skills: impl IntoIterator<Item = impl Into<String>>,
        pin: AgentPin,
    ) -> Self {
        Self {
            step: step.into(),
            skills: skills.into_iter().map(Into::into).collect(),
            pin,
            brief: None,
        }
    }
}

impl Store {
    /// Write a task's agents, in the order they were given, inside the
    /// transaction that creates or re-staffs the task.
    ///
    /// The skills are written by name and the schema holds them to skills that
    /// exist, so an agent cannot be staffed on a skill nothing ships or the
    /// user never wrote.
    pub(crate) async fn staff_agents_in_tx(
        tx: &mut Transaction<'_, Sqlite>,
        task_id: &str,
        agents: &[NewTaskAgent],
    ) -> Result<()> {
        for (ordinal, agent) in agents.iter().enumerate() {
            let id = new_id();
            let (model, effort) = AgentPin::columns(&agent.pin);
            sqlx::query(
                "INSERT INTO task_agents (id, task_id, step, ordinal, model, effort, brief)
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&id)
            .bind(task_id)
            .bind(&agent.step)
            .bind(ordinal as i64)
            .bind(&model)
            .bind(&effort)
            .bind(&agent.brief)
            .execute(&mut **tx)
            .await?;
            Self::write_agent_skills_in_tx(tx, &id, &agent.skills).await?;
        }
        Ok(())
    }

    /// Staff a task again, column by column, keeping the row of an agent
    /// already on its column: its sessions, its messages and its usage name
    /// that row, and an edit that staffs a column the task lacked is no
    /// reason to lose them. Where two rows sit on one column — a database
    /// from before workflows put every reviewer on `review` (0023) — the
    /// first keeps the column and the others' sessions and messages move to
    /// it before they go. A column the new staffing leaves out loses its
    /// agent only while nothing names it; one with sessions or messages
    /// behind it is refused, since dropping it would drop them.
    pub(crate) async fn restaff_agents_in_tx(
        tx: &mut Transaction<'_, Sqlite>,
        task_id: &str,
        agents: &[NewTaskAgent],
    ) -> Result<()> {
        let existing: Vec<(String, String)> =
            sqlx::query_as("SELECT id, step FROM task_agents WHERE task_id = ? ORDER BY ordinal")
                .bind(task_id)
                .fetch_all(&mut **tx)
                .await?;
        let mut kept: Vec<(String, String)> = Vec::new();
        for (id, step) in &existing {
            let wanted = agents.iter().any(|agent| agent.step == *step);
            match kept.iter().find(|(_, kept_step)| kept_step == step) {
                Some((keeper, _)) if wanted => {
                    Self::move_agent_history_in_tx(tx, id, keeper).await?;
                    sqlx::query("DELETE FROM task_agents WHERE id = ?")
                        .bind(id)
                        .execute(&mut **tx)
                        .await?;
                }
                _ if wanted => kept.push((id.clone(), step.clone())),
                _ => {
                    if Self::agent_has_history_in_tx(tx, id).await? {
                        return Err(StoreError::Conflict(format!(
                            "column {step} has sessions or messages behind it; staff it again \
                             rather than leave it out"
                        )));
                    }
                    sqlx::query("DELETE FROM task_agents WHERE id = ?")
                        .bind(id)
                        .execute(&mut **tx)
                        .await?;
                }
            }
        }
        // The ordinals are reassigned from the new list; they are unique per
        // task, so the rows that stay step out of the way first.
        sqlx::query("UPDATE task_agents SET ordinal = -1 - ordinal WHERE task_id = ?")
            .bind(task_id)
            .execute(&mut **tx)
            .await?;
        for (ordinal, agent) in agents.iter().enumerate() {
            let (model, effort) = AgentPin::columns(&agent.pin);
            match kept.iter().find(|(_, step)| *step == agent.step) {
                Some((id, _)) => {
                    sqlx::query(
                        "UPDATE task_agents SET ordinal = ?, model = ?, effort = ?, brief = ?
                         WHERE id = ?",
                    )
                    .bind(ordinal as i64)
                    .bind(&model)
                    .bind(&effort)
                    .bind(&agent.brief)
                    .bind(id)
                    .execute(&mut **tx)
                    .await?;
                    sqlx::query("DELETE FROM task_agent_skills WHERE agent_id = ?")
                        .bind(id)
                        .execute(&mut **tx)
                        .await?;
                    Self::write_agent_skills_in_tx(tx, id, &agent.skills).await?;
                }
                None => {
                    let id = new_id();
                    sqlx::query(
                        "INSERT INTO task_agents (id, task_id, step, ordinal, model, effort, brief)
                         VALUES (?, ?, ?, ?, ?, ?, ?)",
                    )
                    .bind(&id)
                    .bind(task_id)
                    .bind(&agent.step)
                    .bind(ordinal as i64)
                    .bind(&model)
                    .bind(&effort)
                    .bind(&agent.brief)
                    .execute(&mut **tx)
                    .await?;
                    Self::write_agent_skills_in_tx(tx, &id, &agent.skills).await?;
                }
            }
        }
        Ok(())
    }

    /// Whether a session or a message names this agent.
    async fn agent_has_history_in_tx(
        tx: &mut Transaction<'_, Sqlite>,
        agent_id: &str,
    ) -> Result<bool> {
        let named: i64 = sqlx::query_scalar(
            "SELECT (SELECT COUNT(*) FROM agent_sessions WHERE task_agent_id = ?)
                  + (SELECT COUNT(*) FROM messages WHERE from_agent_id = ? OR to_agent_id = ?)",
        )
        .bind(agent_id)
        .bind(agent_id)
        .bind(agent_id)
        .fetch_one(&mut **tx)
        .await?;
        Ok(named > 0)
    }

    /// Hand one agent's sessions and messages to another of the same column.
    async fn move_agent_history_in_tx(
        tx: &mut Transaction<'_, Sqlite>,
        from: &str,
        to: &str,
    ) -> Result<()> {
        for sql in [
            "UPDATE agent_sessions SET task_agent_id = ? WHERE task_agent_id = ?",
            "UPDATE messages SET from_agent_id = ? WHERE from_agent_id = ?",
            "UPDATE messages SET to_agent_id = ? WHERE to_agent_id = ?",
        ] {
            sqlx::query(sql)
                .bind(to)
                .bind(from)
                .execute(&mut **tx)
                .await?;
        }
        Ok(())
    }

    async fn write_agent_skills_in_tx(
        tx: &mut Transaction<'_, Sqlite>,
        agent_id: &str,
        skills: &[String],
    ) -> Result<()> {
        for (ordinal, name) in skills.iter().enumerate() {
            // The orchestrator's skill is nobody's to staff: its seat is a
            // fact of the name, so the refusal reads it the same way the
            // launcher does, and no schema has to know it.
            match SkillSeat::of(name) {
                SkillSeat::Orchestrator => {
                    return Err(StoreError::Conflict(format!(
                        "skill {name} is the orchestrator's; a task agent cannot load it"
                    )));
                }
                // The `pr` column stages `pr-babysit` (030); `pr-reviewer`
                // is the daemon's own, loaded onto a review session (029).
                SkillSeat::PullRequest if name == PR_BABYSIT_SKILL => {}
                SkillSeat::PullRequest => {
                    return Err(StoreError::Conflict(format!(
                        "skill {name} is loaded by Ariadne itself, where a request is reviewed; \
                         staff it on no task agent"
                    )));
                }
                SkillSeat::Task => {}
            }
            sqlx::query(
                "INSERT INTO task_agent_skills (agent_id, skill_name, ordinal) VALUES (?, ?, ?)",
            )
            .bind(agent_id)
            .bind(name)
            .bind(ordinal as i64)
            .execute(&mut **tx)
            .await
            .map_err(|e| unknown_skill(e, name))?;
        }
        Ok(())
    }

    /// Every agent of a task, in the order the orchestrator listed them.
    pub async fn list_task_agents(&self, task_id: &str) -> Result<Vec<TaskAgent>> {
        Ok(
            sqlx::query_as("SELECT * FROM task_agents WHERE task_id = ? ORDER BY ordinal")
                .bind(task_id)
                .fetch_all(self.r())
                .await?,
        )
    }

    pub async fn get_task_agent(&self, id: &str) -> Result<TaskAgent> {
        self.fetch_by("task agent", "task_agents", "id", id).await
    }

    /// The skills an agent loads, in the order they reach it.
    pub async fn agent_skills(&self, agent_id: &str) -> Result<Vec<Skill>> {
        Ok(sqlx::query_as(
            "SELECT s.* FROM skills s
             JOIN task_agent_skills a ON a.skill_name = s.name
             WHERE a.agent_id = ? ORDER BY a.ordinal",
        )
        .bind(agent_id)
        .fetch_all(self.r())
        .await?)
    }

    /// Move an agent onto another model and effort. What the user chose
    /// replaces what the orchestrator sized.
    pub async fn set_agent_pin(&self, agent_id: &str, pin: &AgentPin) -> Result<TaskAgent> {
        self.get_task_agent(agent_id).await?;
        let (model, effort) = AgentPin::columns(pin);
        sqlx::query("UPDATE task_agents SET model = ?, effort = ? WHERE id = ?")
            .bind(&model)
            .bind(&effort)
            .bind(agent_id)
            .execute(self.w())
            .await?;
        self.get_task_agent(agent_id).await
    }

    /// Replace the skills an agent loads. The list is written whole: a skill
    /// left out is one the agent no longer loads.
    pub async fn set_agent_skills(&self, agent_id: &str, skills: &[String]) -> Result<()> {
        self.get_task_agent(agent_id).await?;
        let mut tx = self.w().begin().await?;
        sqlx::query("DELETE FROM task_agent_skills WHERE agent_id = ?")
            .bind(agent_id)
            .execute(&mut *tx)
            .await?;
        Self::write_agent_skills_in_tx(&mut tx, agent_id, skills).await?;
        tx.commit().await?;
        Ok(())
    }
}

/// The `REFERENCES skills (name)` violation, said by the name that was asked
/// for rather than as a foreign key.
fn unknown_skill(e: sqlx::Error, name: &str) -> StoreError {
    match e {
        sqlx::Error::Database(ref db) if db.is_foreign_key_violation() => {
            StoreError::Conflict(format!("no skill is called {name}"))
        }
        other => StoreError::Db(other),
    }
}
