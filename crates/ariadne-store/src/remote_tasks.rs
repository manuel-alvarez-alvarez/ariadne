//! Worker-owned execution context, independent of local goals.
use crate::{Change, Goal, GoalStep, NewTaskAgent, Result, Store, StoreError, Task, now};
use ariadne_core::{Actor, GoalStatus, TaskStatus, id::new_id};
use sqlx::{Sqlite, Transaction};

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RemoteTaskContext {
    pub task_id: String,
    pub source_daemon_id: String,
    pub source_goal_id: String,
    pub source_title: String,
    pub source_description: String,
    pub workflow: String,
    pub request: String,
    pub revision: i64,
    pub authorized_revision: Option<i64>,
}

pub struct NewRemoteTask {
    pub id: String,
    pub repo_id: String,
    pub title: String,
    pub description: String,
    pub source_daemon_id: String,
    pub source_goal_id: String,
    pub source_title: String,
    pub source_description: String,
    pub workflow: String,
    pub request: String,
    pub steps: Vec<GoalStep>,
    pub agents: Vec<NewTaskAgent>,
}

const TASK_STEPS: &str = "SELECT ? AS goal_id, ordinal, id, title, description, skills, ranks, gate FROM remote_task_steps WHERE task_id = ? UNION ALL SELECT goal_id, ordinal, id, title, description, skills, ranks, gate FROM goal_steps WHERE goal_id = ? AND NOT EXISTS (SELECT 1 FROM remote_task_contexts WHERE task_id = ?) ORDER BY ordinal";

impl Store {
    /// The persistent identity of this database, independent of connection aliases.
    pub async fn daemon_id(&self) -> Result<String> {
        if let Some(id) = sqlx::query_scalar("SELECT id FROM daemon_identity WHERE singleton = 1")
            .fetch_optional(self.r())
            .await?
        {
            return Ok(id);
        }
        sqlx::query("INSERT OR IGNORE INTO daemon_identity (singleton, id) VALUES (1, ?)")
            .bind(new_id())
            .execute(self.w())
            .await?;
        Ok(
            sqlx::query_scalar("SELECT id FROM daemon_identity WHERE singleton = 1")
                .fetch_one(self.r())
                .await?,
        )
    }

    pub async fn remote_task_context(&self, id: &str) -> Result<Option<RemoteTaskContext>> {
        Ok(
            sqlx::query_as("SELECT * FROM remote_task_contexts WHERE task_id = ?")
                .bind(id)
                .fetch_optional(self.r())
                .await?,
        )
    }

    /// Local context or a task's saved source context. This never creates a goal.
    pub async fn task_goal(&self, task: &Task) -> Result<Goal> {
        let Some(ctx) = self.remote_task_context(&task.id).await? else {
            return self.get_goal(&task.goal_id).await;
        };
        Ok(Goal {
            id: ctx.source_goal_id.clone(),
            title: ctx.source_title,
            description: ctx.source_description,
            workflow: ctx.workflow,
            status: if ctx.authorized_revision.is_some() {
                GoalStatus::Active
            } else {
                GoalStatus::Planning
            }
            .as_str()
            .into(),
            orchestrated: false,
            origin_goal_id: Some(ctx.source_goal_id),
            model: String::new(),
            effort: None,
            issue_url: None,
            created_at: task.created_at.clone(),
            updated_at: task.updated_at.clone(),
            orchestrator_answered_failed_task_ids: None,
            orchestrator_given_up_at: None,
            orchestrator_given_up_wedged: false,
            pr_operation: None,
            pr_repository_id: None,
            pr_number: None,
            pr_url: None,
        })
    }

    pub async fn session_goal(&self, session: &crate::AgentSession) -> Result<Goal> {
        if let Some(id) = &session.task_id {
            return self.task_goal(&self.get_task(id).await?).await;
        }
        self.get_goal(session.goal_id.as_deref().unwrap_or_default())
            .await
    }

    pub async fn task_steps(&self, task: &Task) -> Result<Vec<GoalStep>> {
        Ok(sqlx::query_as(TASK_STEPS)
            .bind(&task.goal_id)
            .bind(&task.id)
            .bind(&task.goal_id)
            .bind(&task.id)
            .fetch_all(self.r())
            .await?)
    }

    pub(crate) async fn task_steps_in_tx(
        tx: &mut Transaction<'_, Sqlite>,
        task: &Task,
    ) -> Result<Vec<GoalStep>> {
        Ok(sqlx::query_as(TASK_STEPS)
            .bind(&task.goal_id)
            .bind(&task.id)
            .bind(&task.goal_id)
            .bind(&task.id)
            .fetch_all(&mut **tx)
            .await?)
    }

    pub async fn remote_tasks(&self) -> Result<Vec<Task>> {
        Ok(sqlx::query_as(
            "SELECT t.* FROM tasks t JOIN remote_task_contexts r ON r.task_id = t.id ORDER BY t.id",
        )
        .fetch_all(self.r())
        .await?)
    }

    /// Repeat creation without replacing an edited draft or a running task.
    pub async fn create_remote_task(&self, mut new: NewRemoteTask) -> Result<(Task, bool)> {
        self.get_repository(&new.repo_id).await?;
        let mut tx = self.w().begin().await?;
        let prior: Option<RemoteTaskContext> =
            sqlx::query_as("SELECT * FROM remote_task_contexts WHERE task_id = ?")
                .bind(&new.id)
                .fetch_optional(&mut *tx)
                .await?;
        if let Some(prior) = prior {
            if prior.request != new.request {
                return Err(StoreError::Conflict(
                    "the task ID already has another creation request; edit the task instead"
                        .into(),
                ));
            }
            drop(tx);
            return Ok((self.get_task(&new.id).await?, false));
        }
        if sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM tasks WHERE id = ?)")
            .bind(&new.id)
            .fetch_one(&mut *tx)
            .await?
        {
            return Err(StoreError::Conflict("the task ID already exists".into()));
        }
        Self::check_step_staffing(&new.steps, &mut new.agents, None)?;
        let ts = now();
        sqlx::query("INSERT INTO remote_task_contexts (task_id, source_daemon_id, source_goal_id, source_title, source_description, workflow, request) VALUES (?, ?, ?, ?, ?, ?, ?)")
            .bind(&new.id).bind(&new.source_daemon_id).bind(&new.source_goal_id).bind(&new.source_title)
            .bind(&new.source_description).bind(&new.workflow).bind(&new.request).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO tasks (id, goal_id, repo_id, title, description, branch, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(&new.id).bind(&new.source_goal_id).bind(&new.repo_id).bind(&new.title).bind(&new.description)
            .bind(crate::tasks::branch_name(&new.title, &new.id)).bind(&ts).bind(&ts).execute(&mut *tx).await?;
        for step in new.steps {
            sqlx::query("INSERT INTO remote_task_steps (task_id, ordinal, id, title, description, skills, ranks, gate) VALUES (?, ?, ?, ?, ?, ?, ?, ?)")
                .bind(&new.id).bind(step.ordinal).bind(step.id).bind(step.title).bind(step.description)
                .bind(step.skills).bind(step.ranks).bind(step.gate).execute(&mut *tx).await?;
        }
        Self::staff_agents_in_tx(&mut tx, &new.id, &new.agents).await?;
        tx.commit().await?;
        let task = self.get_task(&new.id).await?;
        self.publish(Change::TaskCreated(task.clone()));
        Ok((task, true))
    }

    pub async fn authorize_remote_task(&self, id: &str, revision: i64) -> Result<Task> {
        let mut tx = self.w().begin().await?;
        let task: Task = Self::fetch_by_in_tx(&mut tx, "task", "tasks", id).await?;
        let ctx: RemoteTaskContext =
            sqlx::query_as("SELECT * FROM remote_task_contexts WHERE task_id = ?")
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or_else(|| crate::not_found("remote task", id))?;
        if task.status() == TaskStatus::Cancelled {
            return Err(StoreError::Conflict(
                "a cancelled task cannot receive authorization".into(),
            ));
        }
        if ctx.authorized_revision == Some(revision) {
            drop(tx);
            return Ok(task);
        }
        if ctx.authorized_revision.is_some()
            || ctx.revision != revision
            || task.status() != TaskStatus::Pending
        {
            return Err(StoreError::Conflict(
                "authorization does not match the current draft revision".into(),
            ));
        }
        let steps = Self::task_steps_in_tx(&mut tx, &task).await?;
        let staffed: Vec<String> =
            sqlx::query_scalar("SELECT step FROM task_agents WHERE task_id = ?")
                .bind(id)
                .fetch_all(&mut *tx)
                .await?;
        if !crate::unstaffed_columns(&steps, &staffed).is_empty() {
            return Err(StoreError::Invalid(
                "staff every workflow column before authorization".into(),
            ));
        }
        sqlx::query(
            "UPDATE remote_task_contexts SET authorized_revision = revision WHERE task_id = ?",
        )
        .bind(id)
        .execute(&mut *tx)
        .await?;
        let transition = Self::transition_in_tx(
            &mut tx,
            &task,
            TaskStatus::Ready,
            Actor::Daemon,
            Some("source authorized execution"),
            None,
        )
        .await?;
        tx.commit().await?;
        let task = self.get_task(id).await?;
        self.publish(Change::TaskUpdated {
            task: task.clone(),
            transition: Some(transition),
        });
        Ok(task)
    }
}
