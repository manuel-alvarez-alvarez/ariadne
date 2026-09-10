//! The agents staffed on a task.
//!
//! An agent has no identity of its own: it is an agent CLI, a model, an
//! effort, a brief and a set of skills. Its seat says only where it sits —
//! one of the authors that write the task, each on a branch of its own, or
//! one of the reviewers that vote on it — which is what the state machine and
//! the launcher need to know and the whole of what they need.

use ariadne_core::{Seat, id::new_id};
use sqlx::{Sqlite, Transaction};

use crate::{AgentPin, Result, Skill, SkillSeat, Store, StoreError, TaskAgent, not_found};

/// One agent to staff on a task, as the orchestrator describes it.
#[derive(Debug, Clone)]
pub struct NewTaskAgent {
    /// `Author` or `Reviewer`. A task takes one author or more; several
    /// authors need a reviewer to pick the winner.
    pub seat: Seat,
    /// The skills this agent loads, in the order they reach it. An agent with
    /// none is a generic agent with nothing but its task, which is legal and
    /// rarely what anybody wants.
    pub skills: Vec<String>,
    /// What it runs on: its agent CLI and its model, and the effort where one
    /// was chosen.
    pub pin: AgentPin,
    /// What the orchestrator tells this agent beyond the task itself.
    pub brief: Option<String>,
}

impl NewTaskAgent {
    /// An agent in `seat` on the named skills, on `pin`.
    pub fn new(
        seat: Seat,
        skills: impl IntoIterator<Item = impl Into<String>>,
        pin: AgentPin,
    ) -> Self {
        Self {
            seat,
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
        let mut ordinals = (0i64, 0i64);
        for agent in agents {
            let ordinal = match agent.seat {
                Seat::Author => {
                    ordinals.0 += 1;
                    ordinals.0 - 1
                }
                Seat::Reviewer => {
                    ordinals.1 += 1;
                    ordinals.1 - 1
                }
                Seat::Orchestrator => {
                    return Err(StoreError::Conflict(
                        "an orchestrator belongs to a goal, not to a task".into(),
                    ));
                }
            };
            let id = new_id();
            let (agent_kind, model, effort) = AgentPin::columns(&agent.pin);
            sqlx::query(
                "INSERT INTO task_agents (id, task_id, seat, ordinal, agent_kind, model,
                                          effort, brief)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&id)
            .bind(task_id)
            .bind(agent.seat.as_str())
            .bind(ordinal)
            .bind(&agent_kind)
            .bind(&model)
            .bind(&effort)
            .bind(&agent.brief)
            .execute(&mut **tx)
            .await?;
            Self::write_agent_skills_in_tx(tx, &id, &agent.skills).await?;
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
            if SkillSeat::of(name) == SkillSeat::Orchestrator {
                return Err(StoreError::Conflict(format!(
                    "skill {name} is the orchestrator's; a task agent cannot load it"
                )));
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

    /// Every agent of a task, authors before reviewers, each seat in the order
    /// the orchestrator listed it.
    pub async fn list_task_agents(&self, task_id: &str) -> Result<Vec<TaskAgent>> {
        Ok(sqlx::query_as(
            "SELECT * FROM task_agents WHERE task_id = ?
             ORDER BY seat = 'reviewer', ordinal",
        )
        .bind(task_id)
        .fetch_all(self.r())
        .await?)
    }

    /// The first author of a task, which for most tasks is the only one.
    ///
    /// A task staffed with several authors is read with
    /// [`Store::list_task_authors`]; this stays the answer for the paths that
    /// hold for a one-author task alone.
    pub async fn task_author(&self, task_id: &str) -> Result<TaskAgent> {
        sqlx::query_as(
            "SELECT * FROM task_agents WHERE task_id = ? AND seat = 'author'
             ORDER BY ordinal LIMIT 1",
        )
        .bind(task_id)
        .fetch_optional(self.r())
        .await?
        .ok_or_else(|| not_found("author", task_id))
    }

    /// The authors of a task, in the order the orchestrator listed them.
    pub async fn list_task_authors(&self, task_id: &str) -> Result<Vec<TaskAgent>> {
        Ok(sqlx::query_as(
            "SELECT * FROM task_agents WHERE task_id = ? AND seat = 'author' ORDER BY ordinal",
        )
        .bind(task_id)
        .fetch_all(self.r())
        .await?)
    }

    /// The reviewers of a task, in the order the orchestrator listed them.
    pub async fn list_task_reviewers(&self, task_id: &str) -> Result<Vec<TaskAgent>> {
        Ok(sqlx::query_as(
            "SELECT * FROM task_agents WHERE task_id = ? AND seat = 'reviewer' ORDER BY ordinal",
        )
        .bind(task_id)
        .fetch_all(self.r())
        .await?)
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

    /// Move an agent onto another CLI, model and effort. What the user chose
    /// replaces what the orchestrator sized.
    pub async fn set_agent_pin(&self, agent_id: &str, pin: &AgentPin) -> Result<TaskAgent> {
        self.get_task_agent(agent_id).await?;
        let (agent_kind, model, effort) = AgentPin::columns(pin);
        sqlx::query("UPDATE task_agents SET agent_kind = ?, model = ?, effort = ? WHERE id = ?")
            .bind(&agent_kind)
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
