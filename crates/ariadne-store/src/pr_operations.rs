//! The goal of one pull request operation (029, 030).
//!
//! Ariadne's work on a request is a goal of its own: one open goal per
//! request and operation, no orchestrator, one task, one agent on the one
//! column of the operation's workflow. The goal names the request by value
//! (`goals.pr_*`), so it outlives the request's own row. This module is the
//! lifecycle every operation shares: start or reuse the open goal, and end
//! it as completed or cancelled.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use ariadne_core::id::new_id;
use ariadne_core::workflow::PrOperation;
use ariadne_core::{Actor, GoalStatus, TaskStatus};

use crate::defaults::pr_operation_workflow;
use crate::goals::{NewGoal, NewPrGoal};
use crate::tasks::branch_name;
use crate::{
    AgentPin, Change, Goal, NewTaskAgent, Result, Store, StoreError, Task, TaskAgent, now,
};

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
    /// The request's title, which names the goal and its task.
    pub title: String,
    /// What the operation's one agent runs on.
    pub pin: AgentPin,
    /// The skills it loads beside its column's.
    pub skills: Vec<String>,
}

/// The goal of an operation with its one task and the one agent of that task.
#[derive(Debug, Clone)]
pub struct PrOperationOwner {
    pub goal: Goal,
    pub task: Task,
    pub agent: TaskAgent,
}

/// How an operation ends: with the request, or by the user's stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrOperationEnd {
    Completed,
    Cancelled,
}

impl Store {
    /// The open goal of `operation` on request `number`, where one is.
    pub async fn active_pr_goal(
        &self,
        repository_id: &str,
        number: i64,
        operation: PrOperation,
    ) -> Result<Option<Goal>> {
        Ok(sqlx::query_as(
            "SELECT * FROM goals
              WHERE pr_repository_id = ? AND pr_number = ? AND pr_operation = ?
                AND status IN ('planning', 'active')",
        )
        .bind(repository_id)
        .bind(number)
        .bind(operation.as_str())
        .fetch_optional(self.r())
        .await?)
    }

    /// The goal of an operation read with its task and agent. None where the
    /// goal has no task yet, which a start finishes.
    pub async fn pr_operation_owner(&self, goal: &Goal) -> Result<Option<PrOperationOwner>> {
        let task: Option<Task> =
            sqlx::query_as("SELECT * FROM tasks WHERE goal_id = ? ORDER BY created_at, id LIMIT 1")
                .bind(&goal.id)
                .fetch_optional(self.r())
                .await?;
        let Some(task) = task else {
            return Ok(None);
        };
        let Some(agent) = self.list_task_agents(&task.id).await?.into_iter().next() else {
            return Ok(None);
        };
        Ok(Some(PrOperationOwner {
            goal: goal.clone(),
            task,
            agent,
        }))
    }

    /// Start an operation on a request, or keep the one already open: a
    /// repeat moves the pin and the extra skills onto the open goal's agent
    /// and creates nothing. The boolean is true only where this call made
    /// the goal. The goal is active at once, with its task in the one column.
    pub async fn start_pr_operation(
        &self,
        new: StartPrOperation,
    ) -> Result<(PrOperationOwner, bool)> {
        let workflow = pr_operation_workflow(new.operation);
        let mut created = false;
        let mut goal = match self
            .active_pr_goal(&new.repository_id, new.number, new.operation)
            .await?
        {
            Some(goal) => goal,
            None => {
                created = true;
                self.create_operation_goal(&new, workflow).await?
            }
        };
        let mut owner = self.ensure_operation_task(&goal, &new).await?;
        // A task that ended while its goal stayed open is no owner: the goal
        // goes and a fresh one takes the request.
        if owner.task.status().is_terminal() || owner.task.status() == TaskStatus::Failed {
            self.end_goal(&goal.id, GoalStatus::Cancelled).await?;
            created = true;
            goal = self.create_operation_goal(&new, workflow).await?;
            owner = self.ensure_operation_task(&goal, &new).await?;
        }
        if !created {
            owner = self.restaff_operation(owner, &new).await?;
        }
        Ok((owner, created))
    }

    async fn create_operation_goal(&self, new: &StartPrOperation, workflow: &str) -> Result<Goal> {
        let verb = title_verb(new.operation);
        let created = self
            .create_pr_goal(
                NewGoal {
                    workflow: Some(workflow.to_string()),
                    title: format!("{verb} PR #{}: {}", new.number, new.title),
                    description: format!(
                        "{verb} pull request {} until it merges or closes.",
                        new.url
                    ),
                    issue_url: None,
                    repository_ids: vec![new.repository_id.clone()],
                    pin: new.pin.clone(),
                },
                NewPrGoal {
                    operation: new.operation,
                    repository_id: new.repository_id.clone(),
                    number: new.number,
                    url: new.url.clone(),
                },
            )
            .await;
        match created {
            Ok(goal) => Ok(goal),
            // A start that raced this one made the goal first: it is the
            // goal of the request.
            Err(StoreError::Conflict(_)) => self
                .active_pr_goal(&new.repository_id, new.number, new.operation)
                .await?
                .ok_or_else(|| StoreError::Conflict("the open goal ended while starting".into())),
            Err(e) => Err(e),
        }
    }

    /// The goal's one task, made and moved into its column where it is not
    /// yet. Read and written in one transaction on the one writer, so two
    /// starts make one task between them.
    async fn ensure_operation_task(
        &self,
        goal: &Goal,
        new: &StartPrOperation,
    ) -> Result<PrOperationOwner> {
        let mut tx = self.w().begin().await?;
        let steps: Vec<crate::GoalStep> =
            sqlx::query_as("SELECT * FROM goal_steps WHERE goal_id = ? ORDER BY ordinal")
                .bind(&goal.id)
                .fetch_all(&mut *tx)
                .await?;
        let step = steps
            .first()
            .ok_or_else(|| StoreError::Invalid("the operation's workflow has no column".into()))?;
        let existing: Option<Task> =
            sqlx::query_as("SELECT * FROM tasks WHERE goal_id = ? ORDER BY created_at, id LIMIT 1")
                .bind(&goal.id)
                .fetch_optional(&mut *tx)
                .await?;
        let mut published = None;
        let mut task = match existing {
            Some(task) => task,
            None => {
                let id = new_id();
                let ts = now();
                sqlx::query(
                    "INSERT INTO tasks (id, goal_id, repo_id, title, description, status, branch,
                                        created_at, updated_at)
                     VALUES (?, ?, ?, ?, ?, 'pending', ?, ?, ?)",
                )
                .bind(&id)
                .bind(&goal.id)
                .bind(&new.repository_id)
                .bind(&goal.title)
                .bind(&goal.description)
                .bind(branch_name(&goal.title, &id))
                .bind(&ts)
                .bind(&ts)
                .execute(&mut *tx)
                .await?;
                let core: Vec<String> = serde_json::from_str(&step.skills).unwrap_or_default();
                Self::staff_agents_in_tx(
                    &mut tx,
                    &id,
                    &[NewTaskAgent {
                        step: step.id.clone(),
                        skills: merged_skills(&core, &new.skills),
                        pin: new.pin.clone(),
                        brief: None,
                    }],
                )
                .await?;
                let task: Task = Self::fetch_by_in_tx(&mut tx, "task", "tasks", &id).await?;
                published = Some(task.clone());
                task
            }
        };
        let mut transitions = Vec::new();
        if task.status() == TaskStatus::Pending {
            transitions.push(
                Self::transition_in_tx(
                    &mut tx,
                    &task,
                    TaskStatus::Ready,
                    Actor::Daemon,
                    None,
                    None,
                )
                .await?,
            );
            task = Self::fetch_by_in_tx(&mut tx, "task", "tasks", &task.id).await?;
        }
        if task.status() == TaskStatus::Ready {
            transitions.push(
                Self::transition_in_tx(
                    &mut tx,
                    &task,
                    TaskStatus::InProgress,
                    Actor::Daemon,
                    None,
                    None,
                )
                .await?,
            );
        }
        tx.commit().await?;
        if let Some(task) = published {
            self.publish(Change::TaskCreated(task));
        }
        let task = self.get_task(&task.id).await?;
        for transition in transitions {
            self.publish(Change::TaskUpdated {
                task: task.clone(),
                transition: Some(transition),
            });
        }
        let agent = self
            .list_task_agents(&task.id)
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| StoreError::Invalid("the operation's task has no agent".into()))?;
        Ok(PrOperationOwner {
            goal: goal.clone(),
            task,
            agent,
        })
    }

    /// Move the open operation's agent onto the pin and skills of a repeated
    /// start; the goal, task and sessions stay.
    async fn restaff_operation(
        &self,
        mut owner: PrOperationOwner,
        new: &StartPrOperation,
    ) -> Result<PrOperationOwner> {
        let steps = self.goal_steps(&owner.goal.id).await?;
        let core: Vec<String> = steps
            .first()
            .and_then(|s| serde_json::from_str(&s.skills).ok())
            .unwrap_or_default();
        let skills = merged_skills(&core, &new.skills);
        let current: Vec<String> = self
            .agent_skills(&owner.agent.id)
            .await?
            .into_iter()
            .map(|s| s.name)
            .collect();
        if owner.agent.model != new.pin.model || owner.agent.effort != new.pin.effort {
            owner.agent = self.set_agent_pin(&owner.agent.id, &new.pin).await?;
            owner.goal = self.set_goal_pin(&owner.goal.id, &new.pin).await?;
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
    /// goal and task cancel. The one way to end an operation by the user,
    /// whichever control asked. Answers the goal as it ended, or None where
    /// none was open. Hold [`Store::pr_operation_lock`] across the call.
    pub async fn stop_pr_operation(
        &self,
        repository_id: &str,
        number: i64,
        operation: PrOperation,
        reason: &str,
    ) -> Result<Option<Goal>> {
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

    /// End the open operation on a request: its task finishes or cancels and
    /// the goal completes or cancels with it. Answers the goal as it ended,
    /// or None where no goal was open. Safe to repeat, and to race.
    pub async fn end_pr_operation(
        &self,
        repository_id: &str,
        number: i64,
        operation: PrOperation,
        end: PrOperationEnd,
        reason: &str,
    ) -> Result<Option<Goal>> {
        let Some(goal) = self
            .active_pr_goal(repository_id, number, operation)
            .await?
        else {
            return Ok(None);
        };
        let tasks = self
            .list_tasks(crate::TaskFilter {
                goal_id: Some(goal.id.clone()),
                status: None,
            })
            .await?;
        for task in tasks {
            let status = task.status();
            if status.is_terminal() || status == TaskStatus::Failed {
                continue;
            }
            let result = if end == PrOperationEnd::Completed && status == TaskStatus::InProgress {
                self.transition_task(
                    &task.id,
                    TaskStatus::Finished,
                    Actor::Daemon,
                    Some(reason),
                    None,
                )
                .await
            } else {
                self.transition_task(
                    &task.id,
                    TaskStatus::Cancelled,
                    Actor::User,
                    Some(reason),
                    None,
                )
                .await
            };
            match result {
                // Another ending moved the task first.
                Ok(_) | Err(StoreError::Transition(_) | StoreError::Conflict(_)) => {}
                Err(e) => return Err(e),
            }
        }
        let status = match end {
            PrOperationEnd::Completed => GoalStatus::Completed,
            PrOperationEnd::Cancelled => GoalStatus::Cancelled,
        };
        match self.end_goal(&goal.id, status).await {
            Ok(goal) => Ok(Some(goal)),
            Err(StoreError::Conflict(_)) => Ok(None),
            Err(e) => Err(e),
        }
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
