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

const TASK_STEPS: &str = "SELECT COALESCE(?, '') AS goal_id, ordinal, id, title, description, skills, ranks, gate FROM remote_task_steps WHERE task_id = ? UNION ALL SELECT goal_id, ordinal, id, title, description, skills, ranks, gate FROM goal_steps WHERE goal_id = ? AND NOT EXISTS (SELECT 1 FROM remote_task_steps WHERE task_id = ?) ORDER BY ordinal";

impl Store {
    /// The workflow saved on a goal-less task.
    pub async fn task_workflow(&self, id: &str) -> Result<Option<String>> {
        Ok(
            sqlx::query_scalar("SELECT workflow FROM task_workflows WHERE task_id = ?")
                .bind(id)
                .fetch_optional(self.r())
                .await?,
        )
    }

    /// Whether a task has no workflow and so no column: local work that was
    /// created without one.
    pub async fn task_has_no_workflow(&self, task: &Task) -> Result<bool> {
        Ok(task.goal_id.is_none() && self.task_workflow(&task.id).await?.is_none())
    }

    /// Create local work and freeze its columns and staffing in one transaction.
    pub async fn create_standalone_task(
        &self,
        repo_id: &str,
        title: &str,
        description: &str,
        workflow: Option<&str>,
        mut agents: Vec<NewTaskAgent>,
    ) -> Result<Task> {
        self.get_repository(repo_id).await?;
        if title.trim().is_empty() {
            return Err(StoreError::Invalid("provide a task title".into()));
        }
        let mut tx = self.w().begin().await?;
        let steps = Self::standalone_steps_in_tx(&mut tx, workflow, &mut agents).await?;
        let id = new_id();
        let ts = now();
        sqlx::query("INSERT INTO tasks (id, goal_id, repo_id, title, description, status, branch, created_at, updated_at) VALUES (?, NULL, ?, ?, ?, 'ready', ?, ?, ?)")
            .bind(&id).bind(repo_id).bind(title).bind(description)
            .bind(crate::tasks::branch_name(title, &id)).bind(&ts).bind(&ts).execute(&mut *tx).await?;
        if let Some(workflow) = workflow {
            sqlx::query("INSERT INTO task_workflows (task_id, workflow) VALUES (?, ?)")
                .bind(&id)
                .bind(workflow)
                .execute(&mut *tx)
                .await?;
        }
        for step in steps {
            sqlx::query("INSERT INTO remote_task_steps (task_id, ordinal, id, title, description, skills, ranks, gate) VALUES (?, ?, ?, ?, ?, ?, ?, ?)")
                .bind(&id).bind(step.ordinal).bind(step.id).bind(step.title).bind(step.description)
                .bind(step.skills).bind(step.ranks).bind(step.gate).execute(&mut *tx).await?;
        }
        Self::staff_agents_in_tx(&mut tx, &id, &agents).await?;
        tx.commit().await?;
        let task = self.get_task(&id).await?;
        self.publish(Change::TaskCreated(task.clone()));
        Ok(task)
    }

    /// Resolve and validate the same workflow snapshot for local and far goal-less admission.
    pub async fn standalone_task_steps(
        &self,
        workflow: Option<&str>,
        agents: &mut [NewTaskAgent],
    ) -> Result<Vec<GoalStep>> {
        let mut tx = self.w().begin().await?;
        Self::standalone_steps_in_tx(&mut tx, workflow, agents).await
    }

    async fn standalone_steps_in_tx(
        tx: &mut Transaction<'_, Sqlite>,
        workflow: Option<&str>,
        agents: &mut [NewTaskAgent],
    ) -> Result<Vec<GoalStep>> {
        if let Some(workflow) = workflow
            && crate::defaults::pr_operation_of(workflow).is_some()
        {
            return Err(StoreError::Invalid(format!(
                "workflow {workflow} runs a pull request operation; a goal cannot use it"
            )));
        }
        let steps: Vec<GoalStep> = match workflow {
            None => Vec::new(),
            Some(workflow) => {
                let saved: crate::Workflow =
                    sqlx::query_as("SELECT * FROM workflows WHERE name = ?")
                        .bind(workflow)
                        .fetch_optional(&mut **tx)
                        .await?
                        .ok_or_else(|| crate::not_found("workflow", workflow))?;
                saved
                    .steps()
                    .into_iter()
                    .enumerate()
                    .map(|(ordinal, step)| GoalStep {
                        goal_id: String::new(),
                        ordinal: ordinal as i64,
                        id: step.id,
                        title: step.title,
                        description: step.description,
                        skills: serde_json::to_string(&step.skills).expect("skills serialize"),
                        ranks: serde_json::to_string(&step.ranks).expect("ranks serialize"),
                        gate: step.gate.map(|gate| gate.as_str().to_string()),
                    })
                    .collect()
            }
        };
        Self::check_step_staffing(&steps, agents, Some("active"))?;
        Ok(steps)
    }

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
        let ctx = match self.remote_task_context(&task.id).await? {
            Some(ctx) => ctx,
            None if task.goal_id.is_some() => {
                return self
                    .get_goal(task.goal_id.as_deref().expect("goal exists"))
                    .await;
            }
            None => RemoteTaskContext {
                task_id: task.id.clone(),
                source_daemon_id: String::new(),
                source_goal_id: task.id.clone(),
                source_title: task.title.clone(),
                source_description: task.description.clone(),
                workflow: self.task_workflow(&task.id).await?.unwrap_or_default(),
                request: String::new(),
                revision: 1,
                authorized_revision: Some(1),
            },
        };
        Ok(Goal {
            id: if ctx.source_goal_id.is_empty() {
                task.id.clone()
            } else {
                ctx.source_goal_id.clone()
            },
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
            origin_goal_id: task.goal_id.as_ref().map(|_| ctx.source_goal_id),
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

    pub async fn tasks_without_local_goals(&self) -> Result<Vec<Task>> {
        Ok(sqlx::query_as(
            "SELECT t.* FROM tasks t WHERE t.goal_id IS NULL OR EXISTS (SELECT 1 FROM remote_task_contexts r WHERE r.task_id = t.id) ORDER BY t.id",
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
        sqlx::query("INSERT INTO tasks (id, goal_id, repo_id, title, description, branch, created_at, updated_at) VALUES (?, NULLIF(?, ''), ?, ?, ?, ?, ?, ?)")
            .bind(&new.id).bind(&new.source_goal_id).bind(&new.repo_id).bind(&new.title).bind(&new.description)
            .bind(crate::tasks::branch_name(&new.title, &new.id)).bind(&ts).bind(&ts).execute(&mut *tx).await?;
        if new.source_goal_id.is_empty() && !new.workflow.is_empty() {
            sqlx::query("INSERT INTO task_workflows (task_id, workflow) VALUES (?, ?)")
                .bind(&new.id)
                .bind(&new.workflow)
                .execute(&mut *tx)
                .await?;
        }
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

impl Store {
    /// Fail exactly one source-observed draft revision. Replays never change a later attempt.
    pub async fn fail_remote_dependency(
        &self,
        id: &str,
        revision: i64,
        operation: &str,
        reason: &str,
    ) -> Result<Task> {
        if revision < 1
            || operation.trim().is_empty()
            || operation.len() > 128
            || reason.trim().is_empty()
        {
            return Err(StoreError::Invalid(
                "provide revision, operation_id, and dependency reason".into(),
            ));
        }
        let mut tx = self.w().begin().await?;
        let task: Task = Self::fetch_by_in_tx(&mut tx, "task", "tasks", id).await?;
        let ctx: RemoteTaskContext =
            sqlx::query_as("SELECT * FROM remote_task_contexts WHERE task_id=?")
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or_else(|| crate::not_found("remote task", id))?;
        let prior: Option<(i64,String)> = sqlx::query_as("SELECT request_revision,reason FROM remote_dependency_controls WHERE task_id=? AND operation_id=?")
            .bind(id).bind(operation).fetch_optional(&mut *tx).await?;
        if let Some((prior_revision, prior_reason)) = prior {
            if prior_revision != revision || prior_reason != reason {
                return Err(StoreError::Conflict(
                    "operation_id already identifies another dependency failure".into(),
                ));
            }
            return Ok(task);
        }
        if ctx.authorized_revision.is_some()
            || ctx.revision != revision
            || task.status() != TaskStatus::Pending
        {
            return Err(StoreError::Conflict(
                "dependency failure does not match an unauthorized pending draft revision".into(),
            ));
        }
        sqlx::query("INSERT INTO remote_dependency_controls (task_id,operation_id,request_revision,reason) VALUES (?,?,?,?)")
            .bind(id).bind(operation).bind(revision).bind(reason).execute(&mut *tx).await?;
        sqlx::query("UPDATE remote_task_contexts SET revision=revision+1 WHERE task_id=?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        let transition = Self::transition_in_tx(
            &mut tx,
            &task,
            TaskStatus::Failed,
            Actor::Daemon,
            Some(reason),
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

    /// Return a source-failed draft to pending without admitting execution.
    pub async fn retry_remote_dependency(
        &self,
        id: &str,
        revision: i64,
        operation: &str,
    ) -> Result<Task> {
        if revision < 1 || operation.trim().is_empty() || operation.len() > 128 {
            return Err(StoreError::Invalid(
                "provide revision and dependency operation_id".into(),
            ));
        }
        let mut tx = self.w().begin().await?;
        let task: Task = Self::fetch_by_in_tx(&mut tx, "task", "tasks", id).await?;
        let ctx: RemoteTaskContext =
            sqlx::query_as("SELECT * FROM remote_task_contexts WHERE task_id=?")
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or_else(|| crate::not_found("remote task", id))?;
        let prior: Option<(i64,Option<i64>)> = sqlx::query_as("SELECT request_revision,retry_request_revision FROM remote_dependency_controls WHERE task_id=? AND operation_id=?")
            .bind(id).bind(operation).fetch_optional(&mut *tx).await?;
        let Some((failure_revision, retried)) = prior else {
            return Err(StoreError::Conflict(
                "operation_id does not identify a source dependency failure".into(),
            ));
        };
        if let Some(retried) = retried {
            if retried != revision {
                return Err(StoreError::Conflict(
                    "retry does not match the original request revision".into(),
                ));
            }
            return Ok(task);
        }
        if ctx.authorized_revision.is_some()
            || ctx.revision != revision
            || revision <= failure_revision
            || task.status() != TaskStatus::Failed
        {
            return Err(StoreError::Conflict(
                "retry does not match the current unauthorized dependency failure".into(),
            ));
        }
        // This source-only recovery is deliberately outside generic User transition permissions.
        let timestamp = now();
        let transition = crate::TaskTransition {
            id: new_id(),
            task_id: id.into(),
            from_status: "failed".into(),
            to_status: "pending".into(),
            actor: Actor::Daemon.as_str().into(),
            reason: Some("source retried dependency failure".into()),
            created_at: timestamp.clone(),
            from_step: task.step.clone(),
            to_step: None,
        };
        sqlx::query("UPDATE tasks SET status='pending',step=NULL,updated_at=? WHERE id=?")
            .bind(timestamp)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE remote_task_contexts SET revision=revision+1 WHERE task_id=?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE remote_dependency_controls SET retry_request_revision=?,retry_revision=? WHERE task_id=? AND operation_id=?")
            .bind(revision).bind(revision+1).bind(id).bind(operation).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO task_transitions (id,task_id,from_status,to_status,actor,reason,created_at,from_step,to_step) VALUES (?,?,'failed','pending','daemon',?,?,?,NULL)")
            .bind(&transition.id).bind(id).bind(&transition.reason).bind(&transition.created_at).bind(&transition.from_step).execute(&mut *tx).await?;
        tx.commit().await?;
        let task = self.get_task(id).await?;
        self.publish(Change::TaskUpdated {
            task: task.clone(),
            transition: Some(transition),
        });
        Ok(task)
    }
}
