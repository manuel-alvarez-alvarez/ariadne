//! The task of one pull request operation (029, 032).
//!
//! Ariadne's work on a request is one goal-less task: one open task per
//! request and operation, one agent on the one column Ariadne fixes for the
//! operation ([`crate::defaults::pr_operation_step`]). The task names the
//! request by value (`task_pull_requests`), so the link outlives the
//! request's own row. This module is the lifecycle every operation shares:
//! start or reuse the open task, and end it as finished, failed or
//! cancelled.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use ariadne_core::id::new_id;
use ariadne_core::workflow::PrOperation;
use ariadne_core::{Actor, TaskStatus};
use sqlx::{Sqlite, Transaction};

use crate::tasks::branch_name;
use crate::{AgentPin, Change, NewTaskAgent, Result, Store, StoreError, Task, TaskAgent, now};

/// The lock of one request's operations, and how many callers wait on it.
#[derive(Default)]
pub(crate) struct PrLock {
    mutex: Arc<tokio::sync::Mutex<()>>,
    waiting: AtomicUsize,
}

/// A caller waiting for a [`PrLock`], counted until it has the lock or gives
/// up.
struct Waiting<'a>(&'a AtomicUsize);

impl<'a> Waiting<'a> {
    fn new(count: &'a AtomicUsize) -> Self {
        count.fetch_add(1, Ordering::SeqCst);
        Self(count)
    }
}

impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// What starts an operation on a request.
#[derive(Debug, Clone)]
pub struct StartPrOperation {
    pub operation: PrOperation,
    pub repository_id: String,
    pub number: i64,
    pub url: String,
    /// The request's title, which names the task.
    pub title: String,
    /// What the operation's one agent runs on.
    pub pin: AgentPin,
    /// The skills it loads beside its column's.
    pub skills: Vec<String>,
}

/// The request a task's operation works on, named by value.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TaskPullRequest {
    pub task_id: String,
    pub operation: String,
    pub repository_id: String,
    pub number: i64,
    pub url: String,
    /// Whether the operation is still Ariadne's work on the request.
    pub open: bool,
}

impl TaskPullRequest {
    pub fn operation(&self) -> Option<PrOperation> {
        self.operation.parse().ok()
    }
}

/// The task of an operation and the one agent of that task.
#[derive(Debug, Clone)]
pub struct PrOperationOwner {
    pub task: Task,
    pub agent: TaskAgent,
}

/// How an operation ends: with the request, or by the user's stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrOperationEnd {
    Completed,
    Failed,
    Cancelled,
}

const OPEN_TASK: &str = "SELECT t.* FROM tasks t JOIN task_pull_requests p ON p.task_id = t.id
  WHERE p.repository_id = ? AND p.number = ? AND p.operation = ? AND p.open = 1";

impl Store {
    /// The open task of `operation` on request `number`, where one is.
    pub async fn active_pr_task(
        &self,
        repository_id: &str,
        number: i64,
        operation: PrOperation,
    ) -> Result<Option<Task>> {
        Ok(sqlx::query_as(OPEN_TASK)
            .bind(repository_id)
            .bind(number)
            .bind(operation.as_str())
            .fetch_optional(self.r())
            .await?)
    }

    /// The request a task's operation works on, where the task runs one.
    pub async fn task_pull_request(&self, task_id: &str) -> Result<Option<TaskPullRequest>> {
        Ok(
            sqlx::query_as("SELECT * FROM task_pull_requests WHERE task_id = ?")
                .bind(task_id)
                .fetch_optional(self.r())
                .await?,
        )
    }

    /// The task of an operation read with its agent. None where the task has
    /// no agent.
    pub async fn pr_operation_owner(&self, task: &Task) -> Result<Option<PrOperationOwner>> {
        let Some(agent) = self.list_task_agents(&task.id).await?.into_iter().next() else {
            return Ok(None);
        };
        Ok(Some(PrOperationOwner {
            task: task.clone(),
            agent,
        }))
    }

    /// Start an operation on a request, or keep the one already open: a
    /// repeat moves the pin and the extra skills onto the open task's agent
    /// and creates nothing. The boolean is true only where this call made
    /// the task. The task is in its one column at once.
    ///
    /// An open task that ended — failed when its request closed, or ended by
    /// hand — is no owner: it stops being the open one, and a fresh task
    /// takes the request.
    pub async fn start_pr_operation(
        &self,
        new: StartPrOperation,
    ) -> Result<(PrOperationOwner, bool)> {
        let mut tx = self.w().begin().await?;
        let open: Option<Task> = sqlx::query_as(OPEN_TASK)
            .bind(&new.repository_id)
            .bind(new.number)
            .bind(new.operation.as_str())
            .fetch_optional(&mut *tx)
            .await?;
        let open = match open {
            Some(task) if task.status().is_terminal() || task.status() == TaskStatus::Failed => {
                Self::close_pr_link_in_tx(&mut tx, &task.id).await?;
                None
            }
            other => other,
        };
        if let Some(task) = open {
            tx.commit().await?;
            let owner = self
                .pr_operation_owner(&task)
                .await?
                .ok_or_else(|| StoreError::Invalid("the operation's task has no agent".into()))?;
            return Ok((self.restaff_operation(owner, &new).await?, false));
        }
        let id = new_id();
        let ts = now();
        let verb = title_verb(new.operation);
        let title = format!("{verb} PR #{}: {}", new.number, new.title);
        let description = format!("{verb} pull request {} until it merges or closes.", new.url);
        sqlx::query(
            "INSERT INTO tasks (id, goal_id, repo_id, title, description, status, branch,
                                created_at, updated_at)
             VALUES (?, NULL, ?, ?, ?, 'pending', ?, ?, ?)",
        )
        .bind(&id)
        .bind(&new.repository_id)
        .bind(&title)
        .bind(&description)
        .bind(branch_name(&title, &id))
        .bind(&ts)
        .bind(&ts)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO task_pull_requests (task_id, operation, repository_id, number, url)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(new.operation.as_str())
        .bind(&new.repository_id)
        .bind(new.number)
        .bind(&new.url)
        .execute(&mut *tx)
        .await?;
        sqlx::query("INSERT INTO task_workflows (task_id, workflow) VALUES (?, ?)")
            .bind(&id)
            .bind(crate::defaults::pr_operation_workflow(new.operation))
            .execute(&mut *tx)
            .await?;
        let step = crate::defaults::pr_operation_step(new.operation);
        let ranks: Vec<&str> = step.ranks.iter().map(|r| r.as_str()).collect();
        sqlx::query(
            "INSERT INTO remote_task_steps (task_id, ordinal, id, title, description, skills, ranks, gate)
             VALUES (?, 0, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(&step.id)
        .bind(&step.title)
        .bind(&step.description)
        .bind(serde_json::to_string(&step.skills).expect("skills serialize"))
        .bind(serde_json::to_string(&ranks).expect("ranks serialize"))
        .bind(step.gate.map(|g| g.as_str()))
        .execute(&mut *tx)
        .await?;
        Self::staff_agents_in_tx(
            &mut tx,
            &id,
            &[NewTaskAgent {
                step: step.id.clone(),
                skills: merged_skills(&step.skills, &new.skills),
                pin: new.pin.clone(),
                brief: None,
            }],
        )
        .await?;
        let created: Task = Self::fetch_by_in_tx(&mut tx, "task", "tasks", &id).await?;
        let ready = Self::transition_in_tx(
            &mut tx,
            &created,
            TaskStatus::Ready,
            Actor::Daemon,
            None,
            None,
        )
        .await?;
        let task: Task = Self::fetch_by_in_tx(&mut tx, "task", "tasks", &id).await?;
        let running = Self::transition_in_tx(
            &mut tx,
            &task,
            TaskStatus::InProgress,
            Actor::Daemon,
            None,
            None,
        )
        .await?;
        tx.commit().await?;
        self.publish(Change::TaskCreated(created));
        let task = self.get_task(&id).await?;
        for transition in [ready, running] {
            self.publish(Change::TaskUpdated {
                task: task.clone(),
                transition: Some(transition),
            });
        }
        let owner = self
            .pr_operation_owner(&task)
            .await?
            .ok_or_else(|| StoreError::Invalid("the operation's task has no agent".into()))?;
        Ok((owner, true))
    }

    async fn close_pr_link_in_tx(tx: &mut Transaction<'_, Sqlite>, task_id: &str) -> Result<()> {
        sqlx::query("UPDATE task_pull_requests SET open = 0 WHERE task_id = ?")
            .bind(task_id)
            .execute(&mut **tx)
            .await?;
        Ok(())
    }

    async fn close_pr_link(&self, task_id: &str) -> Result<()> {
        let mut tx = self.w().begin().await?;
        Self::close_pr_link_in_tx(&mut tx, task_id).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Move the open operation's agent onto the pin and skills of a repeated
    /// start; the task and its sessions stay.
    async fn restaff_operation(
        &self,
        mut owner: PrOperationOwner,
        new: &StartPrOperation,
    ) -> Result<PrOperationOwner> {
        let core = crate::defaults::pr_operation_step(new.operation).skills;
        let skills = merged_skills(&core, &new.skills);
        let current: Vec<String> = self
            .agent_skills(&owner.agent.id)
            .await?
            .into_iter()
            .map(|s| s.name)
            .collect();
        if owner.agent.model != new.pin.model || owner.agent.effort != new.pin.effort {
            owner.agent = self.set_agent_pin(&owner.agent.id, &new.pin).await?;
        }
        if current != skills {
            self.set_agent_skills(&owner.agent.id, &skills).await?;
        }
        Ok(owner)
    }

    /// The lock of one request's operations. Hold it across a start or a
    /// stop, and across the part of a scheduler pass that acts on an
    /// operation's goal, so no pass reads a start or a stop half written.
    /// Not reentrant: take it once, and never hold it across a call that
    /// takes it again.
    pub async fn pr_operation_lock(
        &self,
        repository_id: &str,
        number: i64,
    ) -> tokio::sync::OwnedMutexGuard<()> {
        let lock = self.pr_lock_of(repository_id, number);
        let _waiting = Waiting::new(&lock.waiting);
        lock.mutex.clone().lock_owned().await
    }

    /// How many callers wait for the lock of a request's operations right
    /// now, the holder not counted: what a test listens for to know a pass
    /// or a route has reached the lock.
    pub fn pr_operation_waiters(&self, repository_id: &str, number: i64) -> usize {
        self.pr_lock_of(repository_id, number)
            .waiting
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    fn pr_lock_of(&self, repository_id: &str, number: i64) -> Arc<PrLock> {
        self.pr_locks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .entry(format!("{repository_id}#{number}"))
            .or_default()
            .clone()
    }

    /// Stop an operation on a request: the ask on the request's row goes
    /// first, so no pass starts a session for it again, and then its open
    /// task cancels. The one way to end an operation by the user, whichever
    /// control asked. Answers the task as it ended, or None where none was
    /// open. Hold [`Store::pr_operation_lock`] across the call.
    pub async fn stop_pr_operation(
        &self,
        repository_id: &str,
        number: i64,
        operation: PrOperation,
        reason: &str,
    ) -> Result<Option<Task>> {
        if let Some(row) = self.pull_request_by_number(repository_id, number).await? {
            match operation {
                PrOperation::Review if row.review_asked => {
                    self.set_pull_request_review_asked(&row.id, None).await?;
                }
                PrOperation::Babysit if row.babysit_asked => {
                    self.set_pull_request_babysit_asked(&row.id, None).await?;
                }
                _ => {}
            }
        }
        self.end_pr_operation(
            repository_id,
            number,
            operation,
            PrOperationEnd::Cancelled,
            reason,
        )
        .await
    }

    /// End the open operation on a request: its task finishes, fails or
    /// cancels. A failed task stays the open one, as the request closed
    /// under it, until a stop or a new ask; a finished or cancelled one is
    /// no longer open. Answers the task as it ended, or None where none was
    /// open. Safe to repeat, and to race.
    pub async fn end_pr_operation(
        &self,
        repository_id: &str,
        number: i64,
        operation: PrOperation,
        end: PrOperationEnd,
        reason: &str,
    ) -> Result<Option<Task>> {
        let Some(task) = self
            .active_pr_task(repository_id, number, operation)
            .await?
        else {
            return Ok(None);
        };
        let status = task.status();
        if !(status.is_terminal() || status == TaskStatus::Failed) {
            let (to, actor) = match end {
                PrOperationEnd::Failed => (TaskStatus::Failed, Actor::Daemon),
                PrOperationEnd::Completed if status == TaskStatus::InProgress => {
                    (TaskStatus::Finished, Actor::Daemon)
                }
                _ => (TaskStatus::Cancelled, Actor::User),
            };
            match self
                .transition_task(&task.id, to, actor, Some(reason), None)
                .await
            {
                // Another ending moved the task first.
                Ok(_) | Err(StoreError::Transition(_) | StoreError::Conflict(_)) => {}
                Err(e) => return Err(e),
            }
        }
        let task = self.get_task(&task.id).await?;
        let close = match end {
            PrOperationEnd::Failed => false,
            PrOperationEnd::Completed => task.status() == TaskStatus::Finished,
            PrOperationEnd::Cancelled => true,
        };
        if close {
            self.close_pr_link(&task.id).await?;
        }
        Ok(Some(task))
    }
}

fn title_verb(operation: PrOperation) -> &'static str {
    match operation {
        PrOperation::Review => "Review",
        PrOperation::Babysit => "Keep",
    }
}

/// The column's skills, then the extra ones the user picked, each once.
fn merged_skills(core: &[String], extra: &[String]) -> Vec<String> {
    let mut skills: Vec<String> = Vec::new();
    for name in core.iter().chain(extra) {
        if !skills.contains(name) {
            skills.push(name.clone());
        }
    }
    skills
}
