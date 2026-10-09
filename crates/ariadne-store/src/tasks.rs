//! Task repository: creation, dependency management, the column moves of a
//! stepped task, and the single transactional entry point for status
//! transitions.

use std::collections::{HashMap, HashSet};

use ariadne_core::id::new_id;
use ariadne_core::{Actor, AttentionReason, GoalStatus, TaskStatus, check_transition};
use chrono::DateTime;

use crate::query::Filtered;
use crate::{
    Change, Goal, GoalStep, NewTaskAgent, Result, Store, StoreError, Task, TaskTransition,
    not_found, now,
};

/// The fact a task writes once it reaches one of its three endings.
const TASK_ENDED: &str = "task_ended";

#[derive(Debug, Clone)]
pub struct NewTask {
    pub goal_id: String,
    pub repo_id: String,
    pub title: String,
    pub description: String,
    /// The agents to staff: one per column of the goal's workflow. What each
    /// one can do is the skills it carries, or its column's where it carries
    /// none.
    pub agents: Vec<NewTaskAgent>,
    pub depends_on: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct TaskUpdate {
    /// The whole staffing, replaced: every column is staffed afresh, with
    /// the skills and the pin the caller gave it. Only a task that is not
    /// running can be edited at all, so no live agent session is replaced.
    pub agents: Option<Vec<NewTaskAgent>>,
    pub title: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct TaskFilter {
    pub goal_id: Option<String>,
    pub status: Option<TaskStatus>,
}

/// How much of the title a branch name keeps, in characters. Long enough for
/// a sentence of title, short enough that the name still reads at a glance in
/// a `git branch` listing or on a pull request.
const SLUG_MAX: usize = 40;

/// How much of the task id rides at the end of a branch name. Six characters
/// of a ULID's random tail: enough that two tasks with the same title never
/// share a branch, short enough to read out.
const ID_TAIL: usize = 6;

/// The branch a task is created on: a slug of its title, then the tail of its
/// id — `fix-the-merging-briefing-real-fetch-r9jr7c`. The branch is what shows
/// on a published request, so it names the change and nothing else: no prefix,
/// no `ariadne` anywhere in it.
///
/// Only ASCII letters and digits survive, which keeps the result a valid git
/// ref (`git check-ref-format --branch`) whatever the title was. A title with
/// nothing to slug falls back to `task-<tail>`.
pub(crate) fn branch_name(title: &str, id: &str) -> String {
    let tail = id_tail(id);
    let slug = slug(title);
    let head = if slug.is_empty() { "task" } else { &slug };
    if tail.is_empty() {
        head.to_string()
    } else {
        format!("{head}-{tail}")
    }
}

/// The last [`ID_TAIL`] characters of an id, in the lowercase alphanumeric
/// form a branch name can carry.
fn id_tail(id: &str) -> String {
    let id: String = id
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect();
    id[id.len().saturating_sub(ID_TAIL)..].to_string()
}

/// A title as lowercase kebab-case, clipped to [`SLUG_MAX`] characters on a
/// word boundary where there is one to clip on.
fn slug(title: &str) -> String {
    let mut slug = String::with_capacity(title.len());
    for ch in title.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
        } else if !slug.is_empty() && !slug.ends_with('-') {
            // Every run of anything else collapses into one separator, and a
            // leading one never starts the slug.
            slug.push('-');
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }

    if slug.len() <= SLUG_MAX {
        return slug;
    }
    // The slug is pure ASCII, so the budget is a byte index. Cutting there can
    // land mid-word: back off to the last separator before it. A first word
    // longer than the budget has none to back off to and is cut where it falls.
    let cut = if slug.as_bytes()[SLUG_MAX] == b'-' {
        SLUG_MAX
    } else {
        slug[..SLUG_MAX].rfind('-').unwrap_or(SLUG_MAX)
    };
    slug.truncate(cut);
    slug
}

/// The columns of `steps` that no agent of `agents` works, in column order:
/// what a plan cannot start with, and what a retry cannot start without.
pub fn unstaffed_columns<'a>(steps: &'a [GoalStep], staffed: &[String]) -> Vec<&'a str> {
    steps
        .iter()
        .filter(|step| !staffed.contains(&step.id))
        .map(|step| step.id.as_str())
        .collect()
}

impl Store {
    /// Whether the prompt for this column entry has reached its agent.
    pub async fn step_briefed(&self, transition: &str) -> Result<bool> {
        Ok(sqlx::query_scalar(
            "SELECT step_briefed_at IS NOT NULL FROM task_transitions WHERE id = ?",
        )
        .bind(transition)
        .fetch_one(self.r())
        .await?)
    }

    /// Claim a column entry only while it is still current.
    pub async fn claim_step_briefing(&self, transition: &str) -> Result<bool> {
        Ok(sqlx::query("UPDATE task_transitions SET step_briefed_at = ? WHERE id = ? AND step_briefed_at IS NULL AND id = (SELECT tr.id FROM task_transitions tr WHERE tr.task_id = task_transitions.task_id ORDER BY tr.id DESC LIMIT 1) AND EXISTS (SELECT 1 FROM tasks t WHERE t.id = task_transitions.task_id AND t.status = 'in_progress' AND t.step = task_transitions.to_step)")
            .bind(now()).bind(transition).execute(self.w()).await?.rows_affected() == 1)
    }

    /// An unwritten prompt gives its claim back for the next launch.
    pub async fn release_step_briefing(&self, transition: &str) -> Result<()> {
        sqlx::query("UPDATE task_transitions SET step_briefed_at = NULL WHERE id = ?")
            .bind(transition)
            .execute(self.w())
            .await?;
        Ok(())
    }

    /// Check a staffing against the goal's columns: every agent names a
    /// column of the workflow, no column is staffed twice, and an agent with
    /// no skills of its own inherits its column's. While the goal is still
    /// being planned a column may be left unstaffed — the orchestrator staffs
    /// a plan task by task, and `finalize_plan` is where a missing column is
    /// named ([`unstaffed_columns`]). Once the goal runs, a task is runnable
    /// the moment it is written or edited, so a column nobody staffs is
    /// refused here by name.
    async fn check_workflow_staffing(
        &self,
        goal: &Goal,
        agents: &mut [NewTaskAgent],
    ) -> Result<()> {
        let steps = self.goal_steps(&goal.id).await?;
        let mut staffed = HashSet::new();
        for agent in agents.iter_mut() {
            let step = steps.iter().find(|s| s.id == agent.step).ok_or_else(|| {
                StoreError::Invalid(format!(
                    "the agent names an unknown column {}; the columns are {}",
                    agent.step,
                    steps
                        .iter()
                        .map(|s| s.id.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            })?;
            if !staffed.insert(step.id.clone()) {
                return Err(StoreError::Invalid(format!(
                    "column {} is staffed twice",
                    step.id
                )));
            }
            if agent.skills.is_empty() {
                agent.skills = serde_json::from_str(&step.skills).map_err(|_| {
                    StoreError::Invalid(format!("column {} has an invalid skills list", step.id))
                })?;
            }
        }
        if goal.status() != GoalStatus::Planning {
            let staffed: Vec<String> = staffed.into_iter().collect();
            let missing = unstaffed_columns(&steps, &staffed);
            if !missing.is_empty() {
                return Err(StoreError::Invalid(format!(
                    "the goal is {}, so every column needs an agent; none staffs {}",
                    goal.status,
                    missing.join(", ")
                )));
            }
        }
        Ok(())
    }

    /// The columns of a task's workflow that no agent of the task works, in
    /// column order. Empty for a task that can start.
    pub async fn unstaffed_columns(&self, task: &Task) -> Result<Vec<String>> {
        let steps = self.goal_steps(&task.goal_id).await?;
        let staffed: Vec<String> = self
            .list_task_agents(&task.id)
            .await?
            .into_iter()
            .map(|agent| agent.step)
            .collect();
        Ok(unstaffed_columns(&steps, &staffed)
            .into_iter()
            .map(str::to_string)
            .collect())
    }

    /// Start or retry at the first column, with the status and audit in one write.
    pub async fn start_first_step(&self, task_id: &str) -> Result<Task> {
        self.transition_task(task_id, TaskStatus::InProgress, Actor::Daemon, None, None)
            .await
    }

    /// End the task only if the column whose gate was checked still owns it.
    pub async fn end_step(
        &self,
        task_id: &str,
        from_step: &str,
        to: TaskStatus,
        reason: &str,
        merge_commit: Option<&str>,
    ) -> Result<Task> {
        let mut tx = self.w().begin().await?;
        let task: Task = Self::fetch_by_in_tx(&mut tx, "task", "tasks", task_id).await?;
        if task.status() != TaskStatus::InProgress
            || task.step.as_deref() != Some(from_step)
            || !matches!(to, TaskStatus::Finished | TaskStatus::Failed)
        {
            return Err(StoreError::Conflict(
                "the column no longer owns this step call".into(),
            ));
        }
        let transition =
            Self::transition_in_tx(&mut tx, &task, to, Actor::Agent, Some(reason), merge_commit)
                .await?;
        tx.commit().await?;
        let task = self.get_task(task_id).await?;
        self.publish(Change::TaskUpdated {
            task: task.clone(),
            transition: Some(transition),
        });
        Ok(task)
    }

    /// Finish the final request column after its agent fell quiet on the merge.
    pub async fn end_step_by_daemon(
        &self,
        task_id: &str,
        from_step: &str,
        reason: &str,
        merge_commit: &str,
    ) -> Result<Task> {
        let mut tx = self.w().begin().await?;
        let task: Task = Self::fetch_by_in_tx(&mut tx, "task", "tasks", task_id).await?;
        if task.status() != TaskStatus::InProgress || task.step.as_deref() != Some(from_step) {
            return Err(StoreError::Conflict(
                "the column no longer owns this step call".into(),
            ));
        }
        let transition = Self::transition_in_tx(
            &mut tx,
            &task,
            TaskStatus::Finished,
            Actor::Daemon,
            Some(reason),
            Some(merge_commit),
        )
        .await?;
        tx.commit().await?;
        let task = self.get_task(task_id).await?;
        self.publish(Change::TaskUpdated {
            task: task.clone(),
            transition: Some(transition),
        });
        Ok(task)
    }

    /// Move between adjacent columns without changing the task status.
    pub async fn move_step(
        &self,
        task_id: &str,
        to_step: &str,
        actor: Actor,
        reason: &str,
    ) -> Result<Task> {
        let mut tx = self.w().begin().await?;
        let task: Task = Self::fetch_by_in_tx(&mut tx, "task", "tasks", task_id).await?;
        if task.status() != TaskStatus::InProgress
            || !matches!(actor, Actor::Agent | Actor::Daemon)
            || reason.trim().is_empty()
        {
            return Err(StoreError::Conflict(
                "only a working agent or the daemon can move a step with a reason".into(),
            ));
        }
        let steps: Vec<String> =
            sqlx::query_scalar("SELECT id FROM goal_steps WHERE goal_id = ? ORDER BY ordinal")
                .bind(&task.goal_id)
                .fetch_all(&mut *tx)
                .await?;
        let from = steps.iter().position(|s| Some(s) == task.step.as_ref());
        let to = steps.iter().position(|s| s == to_step);
        match (from, to) {
            (Some(from), Some(to)) => {
                ariadne_core::state_machine::check_step_move(from, to, steps.len())
                    .map_err(|e| StoreError::Conflict(e.into()))?
            }
            _ => {
                return Err(StoreError::Invalid(
                    "the task names an unknown column".into(),
                ));
            }
        }
        let transition = TaskTransition {
            id: new_id(),
            task_id: task.id.clone(),
            from_status: task.status.clone(),
            to_status: task.status.clone(),
            actor: actor.as_str().into(),
            reason: Some(reason.into()),
            from_step: task.step,
            to_step: Some(to_step.into()),
            created_at: now(),
        };
        sqlx::query("UPDATE tasks SET step = ?, updated_at = ? WHERE id = ?")
            .bind(to_step)
            .bind(&transition.created_at)
            .bind(task_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO task_transitions (id, task_id, from_status, to_status, actor, reason, from_step, to_step, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(&transition.id).bind(task_id).bind(&transition.from_status).bind(&transition.to_status)
            .bind(&transition.actor).bind(reason).bind(&transition.from_step).bind(to_step).bind(&transition.created_at)
            .execute(&mut *tx).await?;
        tx.commit().await?;
        let task = self.get_task(task_id).await?;
        self.publish(Change::TaskUpdated {
            task: task.clone(),
            transition: Some(transition),
        });
        Ok(task)
    }

    /// Create a task in `pending`. Checks the staffing against the goal's
    /// columns, and checks that the dependencies belong to the same goal and
    /// are acyclic.
    ///
    /// Nothing caps how many tasks a goal takes: how a goal breaks down is
    /// what the orchestrator settles with the user before it writes any of
    /// them (003), and a number the store enforced afterwards could only
    /// refuse a plan they had already agreed.
    pub async fn create_task(&self, mut new: NewTask) -> Result<Task> {
        let goal = self.get_goal(&new.goal_id).await?;
        self.check_workflow_staffing(&goal, &mut new.agents).await?;
        let repo = self.get_repository(&new.repo_id).await?;
        if !self
            .list_goal_repositories(&goal.id)
            .await?
            .iter()
            .any(|r| r.id == repo.id)
        {
            return Err(StoreError::Invalid(format!(
                "repo {} does not belong to goal {}",
                repo.id, goal.id
            )));
        }

        let id = new_id();
        let ts = now();
        let branch = branch_name(&new.title, &id);

        let mut tx = self.w().begin().await?;
        sqlx::query(
            "INSERT INTO tasks (id, goal_id, repo_id, title, description, status, branch,
                                created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, 'pending', ?, ?, ?)",
        )
        .bind(&id)
        .bind(&goal.id)
        .bind(&repo.id)
        .bind(&new.title)
        .bind(&new.description)
        .bind(&branch)
        .bind(&ts)
        .bind(&ts)
        .execute(&mut *tx)
        .await?;

        Self::staff_agents_in_tx(&mut tx, &id, &new.agents).await?;

        if !new.depends_on.is_empty() {
            Self::insert_dependencies(&mut tx, &goal.id, &id, &new.depends_on).await?;
        }

        tx.commit().await?;
        let task = self.get_task(&id).await?;
        self.publish(Change::TaskCreated(task.clone()));
        Ok(task)
    }

    /// Replace the dependency set of a task (orchestrator, pre-start only).
    pub async fn set_task_dependencies(&self, task_id: &str, depends_on: &[String]) -> Result<()> {
        let mut tx = self.w().begin().await?;
        // Status is validated on the row inside the write transaction: a check
        // against the read pool could be stale by the time we hold the lock.
        let task: Task = Self::fetch_by_in_tx(&mut tx, "task", "tasks", task_id).await?;
        if !matches!(task.status(), TaskStatus::Pending | TaskStatus::Ready) {
            return Err(StoreError::Conflict(format!(
                "dependencies can only change while pending/ready, task is {}",
                task.status
            )));
        }
        sqlx::query("DELETE FROM task_dependencies WHERE task_id = ?")
            .bind(task_id)
            .execute(&mut *tx)
            .await?;
        Self::insert_dependencies(&mut tx, &task.goal_id, task_id, depends_on).await?;
        // A task that was already ready may need to wait again.
        let transition = if task.status() == TaskStatus::Ready && !depends_on.is_empty() {
            Some(
                Self::transition_in_tx(
                    &mut tx,
                    &task,
                    TaskStatus::Pending,
                    Actor::Orchestrator,
                    Some("dependencies changed"),
                    None,
                )
                .await?,
            )
        } else {
            None
        };
        tx.commit().await?;
        let task = self.get_task(task_id).await?;
        self.publish(Change::TaskUpdated { task, transition });
        Ok(())
    }

    /// Validate deps exist, belong to `goal_id`, and introduce no cycle;
    /// insert them.
    async fn insert_dependencies(
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        goal_id: &str,
        task_id: &str,
        depends_on: &[String],
    ) -> Result<()> {
        for dep in depends_on {
            if dep == task_id {
                return Err(StoreError::Invalid("a task cannot depend on itself".into()));
            }
            let dep_goal: Option<String> =
                sqlx::query_scalar("SELECT goal_id FROM tasks WHERE id = ?")
                    .bind(dep)
                    .fetch_optional(&mut **tx)
                    .await?;
            match dep_goal {
                None => return Err(not_found("task", dep)),
                Some(g) if g != goal_id => {
                    return Err(StoreError::Invalid(format!(
                        "dependency {dep} belongs to a different goal"
                    )));
                }
                _ => {}
            }
        }

        // Cycle check over existing edges plus the new ones.
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT td.task_id, td.depends_on_task_id FROM task_dependencies td
             JOIN tasks t ON t.id = td.task_id WHERE t.goal_id = ?",
        )
        .bind(goal_id)
        .fetch_all(&mut **tx)
        .await?;
        let mut edges: HashMap<&str, Vec<&str>> = HashMap::new();
        for (from, to) in &rows {
            edges.entry(from.as_str()).or_default().push(to.as_str());
        }
        for dep in depends_on {
            edges.entry(task_id).or_default().push(dep.as_str());
        }
        // DFS from task_id: reaching task_id again means a cycle.
        let mut stack: Vec<&str> = edges.get(task_id).cloned().unwrap_or_default();
        let mut seen: HashSet<&str> = HashSet::new();
        while let Some(node) = stack.pop() {
            if node == task_id {
                return Err(StoreError::Invalid("dependency cycle detected".into()));
            }
            if seen.insert(node)
                && let Some(next) = edges.get(node)
            {
                stack.extend(next.iter().copied());
            }
        }

        for dep in depends_on {
            sqlx::query("INSERT OR IGNORE INTO task_dependencies (task_id, depends_on_task_id) VALUES (?, ?)")
                .bind(task_id)
                .bind(dep)
                .execute(&mut **tx)
                .await?;
        }
        Ok(())
    }

    pub async fn get_task(&self, id: &str) -> Result<Task> {
        self.fetch_by("task", "tasks", "id", id).await
    }

    pub async fn list_tasks(&self, filter: TaskFilter) -> Result<Vec<Task>> {
        Filtered::new("tasks")
            .maybe(" AND goal_id = ?", filter.goal_id)
            .maybe(" AND status = ?", filter.status.map(|s| s.as_str()))
            .fetch(self, " ORDER BY id", &[])
            .await
    }

    /// Edit a task that is not running: one that is pending or ready, or one
    /// that failed and waits for a retry — which is when a column the task
    /// lacks an agent on is staffed.
    pub async fn update_task(&self, id: &str, mut update: TaskUpdate) -> Result<Task> {
        let mut tx = self.w().begin().await?;
        // Status is validated on the row inside the write transaction: a check
        // against the read pool could be stale by the time we hold the lock.
        let task: Task = Self::fetch_by_in_tx(&mut tx, "task", "tasks", id).await?;
        if !matches!(
            task.status(),
            TaskStatus::Pending | TaskStatus::Ready | TaskStatus::Failed
        ) {
            return Err(StoreError::Conflict(format!(
                "task can only be edited while pending, ready or failed, it is {}",
                task.status
            )));
        }
        if let Some(agents) = &mut update.agents {
            // Re-staffing is the whole staffing again, column by column: an
            // agent already on a column keeps its row — and with it the
            // sessions, messages and usage that name it — on the skills and
            // the pin the edit gives it; a column staffed for the first time
            // gets a new row, and a column the edit leaves out loses its
            // agent. The task is not running, so no live agent session is
            // replaced by this; a failed task's next run starts on the new
            // staffing.
            let goal = Self::fetch_by_in_tx(&mut tx, "goal", "goals", &task.goal_id).await?;
            self.check_workflow_staffing(&goal, agents).await?;
            Self::restaff_agents_in_tx(&mut tx, id, agents).await?;
        }
        let title = update.title.unwrap_or(task.title);
        let description = update.description.unwrap_or(task.description);
        sqlx::query(
            "UPDATE tasks SET title = ?, description = ?, updated_at = ?
             WHERE id = ?",
        )
        .bind(&title)
        .bind(&description)
        .bind(now())
        .bind(id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        let task = self.get_task(id).await?;
        self.publish(Change::TaskUpdated {
            task: task.clone(),
            transition: None,
        });
        Ok(task)
    }

    /// The one and only way to change a task's status.
    ///
    /// Validates against the core state machine, applies side-column updates
    /// (the merge commit) and writes the audit row — all in one transaction.
    pub async fn transition_task(
        &self,
        id: &str,
        to: TaskStatus,
        actor: Actor,
        reason: Option<&str>,
        merge_commit: Option<&str>,
    ) -> Result<Task> {
        let mut tx = self.w().begin().await?;
        let task: Task = Self::fetch_by_in_tx(&mut tx, "task", "tasks", id).await?;
        let transition =
            Self::transition_in_tx(&mut tx, &task, to, actor, reason, merge_commit).await?;
        tx.commit().await?;
        let task = self.get_task(id).await?;
        self.publish(Change::TaskUpdated {
            task: task.clone(),
            transition: Some(transition),
        });
        Ok(task)
    }

    /// Change a task status at a supplied time. Integration tests use this to
    /// prove elapsed status time without depending on a wall clock.
    pub async fn transition_task_at(
        &self,
        id: &str,
        to: TaskStatus,
        actor: Actor,
        reason: Option<&str>,
        merge_commit: Option<&str>,
        at: &str,
    ) -> Result<Task> {
        let mut tx = self.w().begin().await?;
        let task: Task = Self::fetch_by_in_tx(&mut tx, "task", "tasks", id).await?;
        let transition =
            Self::transition_in_tx_at(&mut tx, &task, to, actor, reason, merge_commit, Some(at))
                .await?;
        tx.commit().await?;
        let task = self.get_task(id).await?;
        self.publish(Change::TaskUpdated {
            task: task.clone(),
            transition: Some(transition),
        });
        Ok(task)
    }

    /// Validate against the state machine, apply the status change with its
    /// side-column updates, and write the audit row — the shared body of every
    /// status change, inside the caller's transaction. Returns the audit row
    /// so callers can attach it to the change notification.
    async fn transition_in_tx(
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        task: &Task,
        to: TaskStatus,
        actor: Actor,
        reason: Option<&str>,
        merge_commit: Option<&str>,
    ) -> Result<TaskTransition> {
        Self::transition_in_tx_at(tx, task, to, actor, reason, merge_commit, None).await
    }

    async fn transition_in_tx_at(
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        task: &Task,
        to: TaskStatus,
        actor: Actor,
        reason: Option<&str>,
        merge_commit: Option<&str>,
        at: Option<&str>,
    ) -> Result<TaskTransition> {
        let from = task.status();
        check_transition(from, to, actor)?;
        let timestamp = at.map(str::to_string).unwrap_or_else(now);

        // A task starting, or starting again, enters its first column; one
        // retried waits in no column until it does; every other move keeps
        // the column it is in.
        let first_step: Option<String> =
            if to == TaskStatus::InProgress && from == TaskStatus::Ready {
                sqlx::query_scalar(
                    "SELECT id FROM goal_steps WHERE goal_id = ? ORDER BY ordinal LIMIT 1",
                )
                .bind(&task.goal_id)
                .fetch_optional(&mut **tx)
                .await?
            } else {
                None
            };
        let to_step = if to == TaskStatus::Ready && from == TaskStatus::Failed {
            None
        } else {
            first_step.or_else(|| task.step.clone())
        };
        // The stall is not reset here: it belongs to the agent that stopped
        // working and comes down when that agent's own flag does
        // (`sync_task_stall`).
        sqlx::query(
            "UPDATE tasks SET status = ?, merge_commit = COALESCE(?, merge_commit),
                              updated_at = ?, step = ?
             WHERE id = ?",
        )
        .bind(to.as_str())
        .bind(merge_commit)
        .bind(&timestamp)
        .bind(&to_step)
        .bind(&task.id)
        .execute(&mut **tx)
        .await?;

        let transition = TaskTransition {
            from_step: task.step.clone(),
            to_step,
            id: new_id(),
            task_id: task.id.clone(),
            from_status: from.as_str().to_string(),
            to_status: to.as_str().to_string(),
            actor: actor.as_str().to_string(),
            reason: reason.map(str::to_string),
            created_at: timestamp,
        };
        sqlx::query(
            "INSERT INTO task_transitions (id, task_id, from_status, to_status, actor, reason, created_at, from_step, to_step)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&transition.id)
        .bind(&transition.task_id)
        .bind(&transition.from_status)
        .bind(&transition.to_status)
        .bind(&transition.actor)
        .bind(&transition.reason)
        .bind(&transition.created_at)
        .bind(&transition.from_step)
        .bind(&transition.to_step)
        .execute(&mut **tx)
        .await?;

        if matches!(
            to,
            TaskStatus::Finished | TaskStatus::Cancelled | TaskStatus::Failed
        ) {
            Self::record_task_ended_in_tx(
                tx,
                task,
                to,
                reason,
                merge_commit,
                &transition.created_at,
            )
            .await?;
        }

        Ok(transition)
    }

    /// Write the `task_ended` fact (023): one row, filled from the agent of
    /// the column the task ended in, where it ended in one. A task cancelled
    /// or failed before its first column names no agent at all.
    async fn record_task_ended_in_tx(
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        task: &Task,
        to: TaskStatus,
        reason: Option<&str>,
        merge_commit: Option<&str>,
        ended_at: &str,
    ) -> Result<()> {
        let agent: Option<(String, String, Option<String>)> = sqlx::query_as(
            "SELECT id, model, effort FROM task_agents
              WHERE task_id = ? AND step = ? ORDER BY ordinal LIMIT 1",
        )
        .bind(&task.id)
        .bind(&task.step)
        .fetch_optional(&mut **tx)
        .await?;
        let skills: Vec<String> = match &agent {
            Some((agent_id, _, _)) => {
                sqlx::query_scalar(
                    "SELECT skill_name FROM task_agent_skills
                      WHERE agent_id = ? ORDER BY ordinal",
                )
                .bind(agent_id)
                .fetch_all(&mut **tx)
                .await?
            }
            None => Vec::new(),
        };
        let lead_time_secs = match (
            DateTime::parse_from_rfc3339(&task.created_at),
            DateTime::parse_from_rfc3339(ended_at),
        ) {
            (Ok(created), Ok(ended)) => (ended - created).num_seconds().max(0),
            _ => 0,
        };
        let status_secs = Self::task_status_secs_in_tx(tx, task).await?;
        let mut data = serde_json::json!({
            "status": to.as_str(),
            "reason": reason,
            "lead_time_secs": lead_time_secs,
            "status_secs": status_secs,
            // Whether a change reached its base branch: the last column
            // reported the commit it landed as.
            "landed": merge_commit.or(task.merge_commit.as_deref()).is_some(),
        });
        if let Some(step) = &task.step {
            data["step"] = serde_json::json!(step);
        }
        let skills_json = serde_json::to_string(&skills).expect("a list of names serializes");
        sqlx::query(
            "INSERT INTO stat_facts (id, kind, created_at, repo_id, goal_id, task_id, session_id,
                                      launch_id, seat, model, effort, skills, data)
             VALUES (?, ?, ?, ?, ?, ?, NULL, NULL, ?, ?, ?, ?, ?)",
        )
        .bind(new_id())
        .bind(TASK_ENDED)
        .bind(ended_at)
        .bind(&task.repo_id)
        .bind(&task.goal_id)
        .bind(&task.id)
        .bind(agent.as_ref().map(|_| "agent"))
        .bind(agent.as_ref().map(|(_, model, _)| model.clone()))
        .bind(agent.as_ref().and_then(|(_, _, effort)| effort.clone()))
        .bind(skills_json)
        .bind(data.to_string())
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    /// Time starts at task creation in `pending`, then each transition ends
    /// the status before it and begins the next one. The ending status has no
    /// later transition, so it carries no time.
    async fn task_status_secs_in_tx(
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        task: &Task,
    ) -> Result<serde_json::Value> {
        let transitions: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT from_status, to_status, created_at FROM task_transitions
             WHERE task_id = ? ORDER BY created_at, id",
        )
        .bind(&task.id)
        .fetch_all(&mut **tx)
        .await?;
        let mut seconds = serde_json::Map::new();
        let mut status = "pending".to_string();
        let mut started = task.created_at.clone();
        for (from, to, at) in transitions {
            if status == from {
                let elapsed = match (
                    DateTime::parse_from_rfc3339(&started),
                    DateTime::parse_from_rfc3339(&at),
                ) {
                    (Ok(started), Ok(ended)) => (ended - started).num_seconds().max(0),
                    _ => 0,
                };
                let total = seconds
                    .get(&status)
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(0)
                    + elapsed;
                seconds.insert(status.clone(), serde_json::Value::from(total));
            }
            status = to;
            started = at;
        }
        Ok(serde_json::Value::Object(seconds))
    }

    pub async fn list_task_transitions(&self, task_id: &str) -> Result<Vec<TaskTransition>> {
        Ok(sqlx::query_as::<_, TaskTransition>(
            "SELECT * FROM task_transitions WHERE task_id = ? ORDER BY id",
        )
        .bind(task_id)
        .fetch_all(self.r())
        .await?)
    }

    /// Why a task that ended without finishing ended: the reason on the
    /// transition that put it into `failed` or `cancelled`.
    ///
    /// The status is the fact and the transition is the words, so this is
    /// where the two are put back together — what an agent's `fail_task`
    /// said, what a dependency that never landed said, what cancelled the
    /// goal. Read off the audit row that recorded it rather than kept in a
    /// column of its own, which would be a second copy to drift.
    ///
    /// `None` for a task that has not ended that way, and for one that ended
    /// with nothing said.
    pub async fn ended_reason(&self, task: &Task) -> Result<Option<String>> {
        let status = task.status();
        if !matches!(status, TaskStatus::Failed | TaskStatus::Cancelled) {
            return Ok(None);
        }
        Ok(sqlx::query_scalar::<_, Option<String>>(
            "SELECT reason FROM task_transitions
              WHERE task_id = ? AND to_status = ?
              ORDER BY id DESC LIMIT 1",
        )
        .bind(&task.id)
        .bind(status.as_str())
        .fetch_optional(self.r())
        .await?
        .flatten())
    }

    pub async fn list_task_dependencies(&self, task_id: &str) -> Result<Vec<String>> {
        Ok(sqlx::query_scalar(
            "SELECT depends_on_task_id FROM task_dependencies WHERE task_id = ? ORDER BY depends_on_task_id",
        )
        .bind(task_id)
        .fetch_all(self.r())
        .await?)
    }

    /// True when every dependency of the task is finished.
    pub async fn task_dependencies_merged(&self, task_id: &str) -> Result<bool> {
        let unmerged: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM task_dependencies td
             JOIN tasks dep ON dep.id = td.depends_on_task_id
             WHERE td.task_id = ? AND dep.status <> 'finished'",
        )
        .bind(task_id)
        .fetch_one(self.r())
        .await?;
        Ok(unmerged == 0)
    }

    /// The first dependency of the task that ended without finishing —
    /// `failed` or `cancelled` — if there is one.
    ///
    /// Such a dependency is never going to finish, so the task behind it is
    /// never going to start: the scheduler reads this to end it rather than
    /// leave it waiting for ever. `None` while every dependency can still get
    /// there, finished ones included.
    pub async fn task_dependencies_blocked(&self, task_id: &str) -> Result<Option<Task>> {
        Ok(sqlx::query_as::<_, Task>(
            "SELECT dep.* FROM task_dependencies td
             JOIN tasks dep ON dep.id = td.depends_on_task_id
             WHERE td.task_id = ? AND dep.status IN ('failed', 'cancelled')
             ORDER BY dep.id
             LIMIT 1",
        )
        .bind(task_id)
        .fetch_optional(self.r())
        .await?)
    }

    pub async fn set_task_worktree(
        &self,
        task_id: &str,
        worktree_path: Option<&str>,
    ) -> Result<()> {
        let n = sqlx::query("UPDATE tasks SET worktree_path = ?, updated_at = ? WHERE id = ?")
            .bind(worktree_path)
            .bind(now())
            .bind(task_id)
            .execute(self.w())
            .await?
            .rows_affected();
        self.publish_task_update(task_id, n).await
    }

    /// Record the pull or merge request a task was opened as.
    ///
    /// The URL is the whole of it: the daemon opens the request once and
    /// answers it on every later call, and the URL is what the UI and the
    /// CLI show.
    pub async fn set_task_pull_request(&self, task_id: &str, url: &str) -> Result<()> {
        let n = sqlx::query("UPDATE tasks SET pr_url = ?, updated_at = ? WHERE id = ?")
            .bind(url)
            .bind(now())
            .bind(task_id)
            .execute(self.w())
            .await?
            .rows_affected();
        self.publish_task_update(task_id, n).await
    }

    /// Forget the request a task was opened as: a task retried after its
    /// request was closed unmerged would otherwise show the user a request
    /// nobody will merge.
    pub async fn clear_task_pull_request(&self, task_id: &str) -> Result<()> {
        let n = sqlx::query(
            "UPDATE tasks SET pr_url = NULL, updated_at = ?
             WHERE id = ? AND pr_url IS NOT NULL",
        )
        .bind(now())
        .bind(task_id)
        .execute(self.w())
        .await?
        .rows_affected();
        self.publish_task_update(task_id, n).await
    }

    /// Bring a task's stall into line with what its agents' own flags say.
    ///
    /// A stalled task *is* a task whose agent stopped working: one condition,
    /// decided by the session's attention. The task's column is this
    /// projection of it, written by every write that can change what a
    /// session's attention says and by nothing else, so the two cannot drift
    /// apart. An orchestrator's session has no task to project onto and
    /// carries its stall on its own row alone.
    pub(crate) async fn sync_task_stall(&self, session_id: &str) -> Result<()> {
        let task_id: Option<String> =
            sqlx::query_scalar("SELECT task_id FROM agent_sessions WHERE id = ?")
                .bind(session_id)
                .fetch_optional(self.r())
                .await?
                .flatten();
        let Some(task_id) = task_id else {
            return Ok(());
        };
        let stalled: bool = sqlx::query_scalar(
            "SELECT EXISTS (
                 SELECT 1 FROM agent_sessions WHERE task_id = ? AND attention_reason = ?
             )",
        )
        .bind(&task_id)
        .bind(AttentionReason::Stalled.as_str())
        .fetch_one(self.r())
        .await?;
        // Only a change is written, so that a task nobody's stall moved is
        // neither restamped nor announced to the watchers of its row.
        let n = sqlx::query(
            "UPDATE tasks SET stalled = ?, updated_at = ? WHERE id = ? AND stalled <> ?",
        )
        .bind(stalled as i64)
        .bind(now())
        .bind(&task_id)
        .bind(stalled as i64)
        .execute(self.w())
        .await?
        .rows_affected();
        self.publish_task_update(&task_id, n).await
    }

    /// Announce a non-transitional task write, unless it matched no row.
    pub(crate) async fn publish_task_update(
        &self,
        task_id: &str,
        rows_affected: u64,
    ) -> Result<()> {
        if rows_affected > 0 {
            let task = self.get_task(task_id).await?;
            self.publish(Change::TaskUpdated {
                task,
                transition: None,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::*;

    const ID: &str = "01m0sktv47w6b8ze6xf4r9jr7c";

    #[test]
    fn branch_is_the_title_slugged_and_the_id_tail() {
        assert_eq!(
            branch_name("Fix the merging briefing: real fetch/rebase", ID),
            // 45 characters of slug is over the budget, and cutting at 40
            // would land inside `rebase`, so the whole word goes.
            "fix-the-merging-briefing-real-fetch-r9jr7c"
        );
        assert_eq!(
            branch_name("Add a health check", ID),
            "add-a-health-check-r9jr7c"
        );
    }

    #[test]
    fn branch_collapses_everything_that_is_not_a_letter_or_a_digit() {
        assert_eq!(
            branch_name(r#"Don't break "feat/x"... v1.2"#, ID),
            "don-t-break-feat-x-v1-2-r9jr7c"
        );
        assert_eq!(branch_name("  padded  ", ID), "padded-r9jr7c");
        assert_eq!(
            branch_name("-leading and trailing-", ID),
            "leading-and-trailing-r9jr7c"
        );
        assert_eq!(branch_name("Grüße, cafétería", ID), "gr-e-caf-ter-a-r9jr7c");
    }

    #[test]
    fn branch_falls_back_to_the_id_when_the_title_slugs_to_nothing() {
        for title in ["", "   ", "!!!", "修复登录", "—"] {
            assert_eq!(branch_name(title, ID), "task-r9jr7c", "title {title:?}");
        }
    }

    #[test]
    fn branch_clips_a_long_title_on_a_word_boundary() {
        // Exactly the budget: nothing to clip.
        let forty = "aaaa-bbbb-cccc-dddd-eeee-ffff-gggg-hhhhh";
        assert_eq!(forty.len(), SLUG_MAX);
        assert_eq!(slug(forty), forty);
        // Over the budget, but the character at it is a separator: the words
        // inside the budget are whole already, so none of them goes.
        assert_eq!(slug(&format!("{forty}-iiii")), forty);
        // A word straddling the budget goes entirely.
        assert_eq!(
            slug("aaaa bbbb cccc dddd eeee ffff gggg hhhh iiii"),
            "aaaa-bbbb-cccc-dddd-eeee-ffff-gggg-hhhh"
        );
        // A single word with no boundary to back off to is cut where it falls.
        let long = "x".repeat(60);
        assert_eq!(slug(&long), "x".repeat(40));
        assert_eq!(branch_name(&long, ID), format!("{}-r9jr7c", "x".repeat(40)));
    }

    /// Whatever the title, git must take the name: the store hands it straight
    /// to `git worktree add -b`, and a rejected ref would fail the task.
    #[test]
    fn every_branch_name_is_a_valid_git_ref() {
        let titles = [
            "Fix the merging briefing: real fetch/rebase",
            "",
            "   ",
            "!!!",
            "修复登录",
            "-leading dash",
            "trailing dash-",
            "...dots... and .lock",
            "refs/heads/main",
            "a//b",
            "feature@{upstream}",
            "back\\slash and ~tilde^ and :colon",
            "question? star* bracket[",
            "line\nbreak\tand\ttabs",
            "@",
            "HEAD",
            &"x".repeat(200),
        ];
        for title in titles {
            let branch = branch_name(title, ID);
            let checked = Command::new("git")
                .args(["check-ref-format", "--branch", &branch])
                .output()
                .expect("git must be on PATH to check ref formats");
            assert!(
                checked.status.success(),
                "git rejected {branch:?} from title {title:?}"
            );
        }
    }

    /// The columns nobody staffs are named in column order, and a task with
    /// an agent on every column has none to name.
    #[test]
    fn unstaffed_columns_are_named_in_column_order() {
        let step = |ordinal: i64, id: &str| GoalStep {
            goal_id: "01goal".into(),
            ordinal,
            id: id.into(),
            title: id.into(),
            description: String::new(),
            skills: "[]".into(),
            rank: None,
            gate: None,
        };
        let steps = [step(0, "develop"), step(1, "review"), step(2, "pr")];
        assert_eq!(
            unstaffed_columns(&steps, &["review".to_string()]),
            ["develop", "pr"]
        );
        assert!(
            unstaffed_columns(
                &steps,
                &[
                    "pr".to_string(),
                    "develop".to_string(),
                    "review".to_string()
                ]
            )
            .is_empty()
        );
    }
}
