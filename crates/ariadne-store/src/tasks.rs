//! Task repository: creation, dependency management, and the single
//! transactional entry point for status transitions.

use std::collections::{HashMap, HashSet};

use ariadne_core::id::new_id;
use ariadne_core::{Actor, TaskStatus, check_transition};

use crate::{Result, Store, StoreError, Task, TaskTransition, not_found, now};

#[derive(Debug, Clone)]
pub struct NewTask {
    pub goal_id: String,
    pub repo_id: String,
    pub title: String,
    pub description: String,
    pub engineer_profile_id: String,
    pub reviewer_profile_ids: Vec<String>,
    pub depends_on: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct TaskUpdate {
    pub title: Option<String>,
    pub description: Option<String>,
    pub reviewer_profile_ids: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default)]
pub struct TaskFilter {
    pub goal_id: Option<String>,
    pub status: Option<TaskStatus>,
}

impl Store {
    /// Create a task in `pending`. Enforces the goal's `max_tasks`, validates
    /// reviewers are non-empty and deps belong to the same goal and are acyclic.
    pub async fn create_task(&self, new: NewTask) -> Result<Task> {
        if new.reviewer_profile_ids.is_empty() {
            return Err(StoreError::Invalid(
                "a task needs at least one reviewer".into(),
            ));
        }
        let goal = self.get_goal(&new.goal_id).await?;
        let repo = self.get_goal_repo(&new.repo_id).await?;
        if repo.goal_id != goal.id {
            return Err(StoreError::Invalid(format!(
                "repo {} does not belong to goal {}",
                repo.id, goal.id
            )));
        }

        let id = new_id();
        let ts = now();
        let branch = format!("ariadne/task-{id}");

        let mut tx = self.w().begin().await?;

        if let Some(max) = goal.max_tasks {
            let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tasks WHERE goal_id = ?")
                .bind(&goal.id)
                .fetch_one(&mut *tx)
                .await?;
            if count >= max {
                return Err(StoreError::Conflict(format!(
                    "goal {} already has {count} of max {max} tasks",
                    goal.id
                )));
            }
        }

        sqlx::query(
            "INSERT INTO tasks (id, goal_id, repo_id, title, description, status,
                                engineer_profile_id, branch, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, 'pending', ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(&goal.id)
        .bind(&repo.id)
        .bind(&new.title)
        .bind(&new.description)
        .bind(&new.engineer_profile_id)
        .bind(&branch)
        .bind(&ts)
        .bind(&ts)
        .execute(&mut *tx)
        .await?;

        for (position, profile_id) in new.reviewer_profile_ids.iter().enumerate() {
            sqlx::query(
                "INSERT INTO task_reviewers (task_id, profile_id, position) VALUES (?, ?, ?)",
            )
            .bind(&id)
            .bind(profile_id)
            .bind(position as i64)
            .execute(&mut *tx)
            .await?;
        }

        if !new.depends_on.is_empty() {
            Self::insert_dependencies(&mut tx, &goal.id, &id, &new.depends_on).await?;
        }

        tx.commit().await?;
        self.get_task(&id).await
    }

    /// Replace the dependency set of a task (planner, pre-start only).
    pub async fn set_task_dependencies(&self, task_id: &str, depends_on: &[String]) -> Result<()> {
        let task = self.get_task(task_id).await?;
        if !matches!(task.status(), TaskStatus::Pending | TaskStatus::Ready) {
            return Err(StoreError::Conflict(format!(
                "dependencies can only change while pending/ready, task is {}",
                task.status
            )));
        }
        let mut tx = self.w().begin().await?;
        sqlx::query("DELETE FROM task_dependencies WHERE task_id = ?")
            .bind(task_id)
            .execute(&mut *tx)
            .await?;
        Self::insert_dependencies(&mut tx, &task.goal_id, task_id, depends_on).await?;
        // A task that was already ready may need to wait again.
        if task.status() == TaskStatus::Ready && !depends_on.is_empty() {
            sqlx::query("UPDATE tasks SET status = 'pending', updated_at = ? WHERE id = ?")
                .bind(now())
                .bind(task_id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Validate deps exist, belong to `goal_id`, and introduce no cycle; insert them.
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
        sqlx::query_as::<_, Task>("SELECT * FROM tasks WHERE id = ?")
            .bind(id)
            .fetch_optional(self.r())
            .await?
            .ok_or_else(|| not_found("task", id))
    }

    pub async fn list_tasks(&self, filter: TaskFilter) -> Result<Vec<Task>> {
        let mut sql = String::from("SELECT * FROM tasks WHERE 1=1");
        if filter.goal_id.is_some() {
            sql.push_str(" AND goal_id = ?");
        }
        if filter.status.is_some() {
            sql.push_str(" AND status = ?");
        }
        sql.push_str(" ORDER BY id");
        // Safe: only fixed clause fragments are appended; values are bound.
        let mut q = sqlx::query_as::<_, Task>(sqlx::AssertSqlSafe(sql));
        if let Some(g) = &filter.goal_id {
            q = q.bind(g.clone());
        }
        if let Some(s) = filter.status {
            q = q.bind(s.as_str());
        }
        Ok(q.fetch_all(self.r()).await?)
    }

    pub async fn update_task(&self, id: &str, update: TaskUpdate) -> Result<Task> {
        let task = self.get_task(id).await?;
        if !matches!(task.status(), TaskStatus::Pending | TaskStatus::Ready) {
            return Err(StoreError::Conflict(format!(
                "task can only be edited while pending/ready, it is {}",
                task.status
            )));
        }
        let title = update.title.unwrap_or(task.title);
        let description = update.description.unwrap_or(task.description);
        let mut tx = self.w().begin().await?;
        sqlx::query("UPDATE tasks SET title = ?, description = ?, updated_at = ? WHERE id = ?")
            .bind(&title)
            .bind(&description)
            .bind(now())
            .bind(id)
            .execute(&mut *tx)
            .await?;
        if let Some(reviewers) = update.reviewer_profile_ids {
            if reviewers.is_empty() {
                return Err(StoreError::Invalid(
                    "a task needs at least one reviewer".into(),
                ));
            }
            sqlx::query("DELETE FROM task_reviewers WHERE task_id = ?")
                .bind(id)
                .execute(&mut *tx)
                .await?;
            for (position, profile_id) in reviewers.iter().enumerate() {
                sqlx::query(
                    "INSERT INTO task_reviewers (task_id, profile_id, position) VALUES (?, ?, ?)",
                )
                .bind(id)
                .bind(profile_id)
                .bind(position as i64)
                .execute(&mut *tx)
                .await?;
            }
        }
        tx.commit().await?;
        self.get_task(id).await
    }

    /// The one and only way to change a task's status.
    ///
    /// Validates against the core state machine, applies side-column updates
    /// (review round bump, merge commit, stalled reset) and writes the audit
    /// row — all in one transaction.
    pub async fn transition_task(
        &self,
        id: &str,
        to: TaskStatus,
        actor: Actor,
        reason: Option<&str>,
        merge_commit: Option<&str>,
    ) -> Result<Task> {
        let mut tx = self.w().begin().await?;
        let task = sqlx::query_as::<_, Task>("SELECT * FROM tasks WHERE id = ?")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| not_found("task", id))?;
        let from = task.status();
        check_transition(from, to, actor)?;

        if to == TaskStatus::Merged && merge_commit.is_none() {
            return Err(StoreError::Invalid(
                "merged transition requires a merge commit".into(),
            ));
        }

        let review_round = if to == TaskStatus::UnderReview {
            task.review_round + 1
        } else {
            task.review_round
        };

        sqlx::query(
            "UPDATE tasks SET status = ?, review_round = ?, merge_commit = COALESCE(?, merge_commit),
                              stalled = 0, updated_at = ?
             WHERE id = ?",
        )
        .bind(to.as_str())
        .bind(review_round)
        .bind(merge_commit)
        .bind(now())
        .bind(id)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            "INSERT INTO task_transitions (id, task_id, from_status, to_status, actor, reason, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(new_id())
        .bind(id)
        .bind(from.as_str())
        .bind(to.as_str())
        .bind(actor.as_str())
        .bind(reason)
        .bind(now())
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        self.get_task(id).await
    }

    pub async fn list_task_transitions(&self, task_id: &str) -> Result<Vec<TaskTransition>> {
        Ok(sqlx::query_as::<_, TaskTransition>(
            "SELECT * FROM task_transitions WHERE task_id = ? ORDER BY id",
        )
        .bind(task_id)
        .fetch_all(self.r())
        .await?)
    }

    /// Reviewer profile ids in planner-assigned order.
    pub async fn list_task_reviewers(&self, task_id: &str) -> Result<Vec<String>> {
        Ok(sqlx::query_scalar(
            "SELECT profile_id FROM task_reviewers WHERE task_id = ? ORDER BY position",
        )
        .bind(task_id)
        .fetch_all(self.r())
        .await?)
    }

    pub async fn list_task_dependencies(&self, task_id: &str) -> Result<Vec<String>> {
        Ok(sqlx::query_scalar(
            "SELECT depends_on_task_id FROM task_dependencies WHERE task_id = ? ORDER BY depends_on_task_id",
        )
        .bind(task_id)
        .fetch_all(self.r())
        .await?)
    }

    /// True when every dependency of the task is merged.
    pub async fn task_dependencies_merged(&self, task_id: &str) -> Result<bool> {
        let unmerged: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM task_dependencies td
             JOIN tasks dep ON dep.id = td.depends_on_task_id
             WHERE td.task_id = ? AND dep.status <> 'merged'",
        )
        .bind(task_id)
        .fetch_one(self.r())
        .await?;
        Ok(unmerged == 0)
    }

    pub async fn set_task_worktree(
        &self,
        task_id: &str,
        worktree_path: Option<&str>,
    ) -> Result<()> {
        sqlx::query("UPDATE tasks SET worktree_path = ?, updated_at = ? WHERE id = ?")
            .bind(worktree_path)
            .bind(now())
            .bind(task_id)
            .execute(self.w())
            .await?;
        Ok(())
    }

    pub async fn set_task_stalled(&self, task_id: &str, stalled: bool) -> Result<()> {
        sqlx::query("UPDATE tasks SET stalled = ?, updated_at = ? WHERE id = ?")
            .bind(stalled as i64)
            .bind(now())
            .bind(task_id)
            .execute(self.w())
            .await?;
        Ok(())
    }
}
