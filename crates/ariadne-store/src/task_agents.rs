//! The agents staffed on a task.
//!
//! An agent has no identity of its own: it is an agent CLI, a model, an
//! effort, a brief and a set of skills. Its seat says only where it sits — the
//! one author that carries the task from its first commit to the end, or one
//! of the reviewers that vote on it — which is what the state machine and the
//! launcher need to know and the whole of what they need.

use ariadne_core::{Seat, id::new_id};
use sqlx::{Sqlite, Transaction};

use crate::{AgentPin, Result, Skill, Store, StoreError, TaskAgent, not_found};

/// One agent to staff on a task, as the orchestrator describes it.
#[derive(Debug, Clone)]
pub struct NewTaskAgent {
    /// `Author` or `Reviewer`. A task takes exactly one author.
    pub seat: Seat,
    /// The skills this agent loads, in the order they reach it. An agent with
    /// none is a generic agent with nothing but its task, which is legal and
    /// rarely what anybody wants.
    pub skills: Vec<String>,
    /// What it runs on. None = auto CLI, that CLI's default model, and
    /// whatever it runs the model at.
    pub pin: Option<AgentPin>,
    /// What the orchestrator tells this agent beyond the task itself.
    pub brief: Option<String>,
}

impl NewTaskAgent {
    /// An agent in `seat` on the named skills, sized by whoever staffs it.
    pub fn new(seat: Seat, skills: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            seat,
            skills: skills.into_iter().map(Into::into).collect(),
            pin: None,
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
            let (agent_kind, model, effort) = AgentPin::columns(agent.pin.as_ref());
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
            .await
            .map_err(|e| one_author(e, task_id))?;
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

    /// The one agent that authors the task.
    pub async fn task_author(&self, task_id: &str) -> Result<TaskAgent> {
        sqlx::query_as("SELECT * FROM task_agents WHERE task_id = ? AND seat = 'author'")
            .bind(task_id)
            .fetch_optional(self.r())
            .await?
            .ok_or_else(|| not_found("author", task_id))
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

    /// Move an agent onto another CLI, model and effort, or, with None, back
    /// onto auto. What the user chose replaces what the orchestrator sized.
    pub async fn set_agent_pin(&self, agent_id: &str, pin: Option<&AgentPin>) -> Result<TaskAgent> {
        self.get_task_agent(agent_id).await?;
        let (agent_kind, model, effort) = AgentPin::columns(pin);
        sqlx::query(
            "UPDATE task_agents SET agent_kind = ?, model = ?, effort = ? WHERE id = ?",
        )
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

/// The partial unique index that holds a task to one author, said in the
/// terms the caller used.
fn one_author(e: sqlx::Error, task_id: &str) -> StoreError {
    match e {
        sqlx::Error::Database(ref db) if db.is_unique_violation() => StoreError::Conflict(format!(
            "task {task_id} already has an author; a task takes exactly one"
        )),
        other => StoreError::Db(other),
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
