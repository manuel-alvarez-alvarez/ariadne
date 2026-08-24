//! Task repository: creation, dependency management, and the single
//! transactional entry point for status transitions.

use std::collections::{HashMap, HashSet};

use ariadne_core::id::new_id;
use ariadne_core::{Actor, TaskStatus, check_transition};

use crate::{
    Change, Result, Store, StoreError, Task, TaskReviewer, TaskTransition, not_found, now,
};

#[derive(Debug, Clone)]
pub struct NewTask {
    pub goal_id: String,
    pub repo_id: String,
    pub title: String,
    pub description: String,
    pub engineer_profile_id: String,
    /// Profile that lands the task once it is approved.
    pub integrator_profile_id: String,
    pub reviewer_profile_ids: Vec<String>,
    pub depends_on: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct TaskUpdate {
    pub title: Option<String>,
    pub description: Option<String>,
    pub reviewer_profile_ids: Option<Vec<String>>,
    pub integrator_profile_id: Option<String>,
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

        // The engineer's agent and model are copied onto the task here and
        // never re-read: editing the profile later must not move a task that
        // is already defined, let alone one mid-flight.
        let engineer = Self::get_profile_in_tx(&mut tx, &new.engineer_profile_id).await?;

        sqlx::query(
            "INSERT INTO tasks (id, goal_id, repo_id, title, description, status,
                                engineer_profile_id, integrator_profile_id, agent_kind,
                                model, branch, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, 'pending', ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(&goal.id)
        .bind(&repo.id)
        .bind(&new.title)
        .bind(&new.description)
        .bind(&new.engineer_profile_id)
        .bind(&new.integrator_profile_id)
        .bind(&engineer.agent_kind)
        .bind(&engineer.model)
        .bind(&branch)
        .bind(&ts)
        .bind(&ts)
        .execute(&mut *tx)
        .await?;

        Self::insert_reviewers(&mut tx, &id, &new.reviewer_profile_ids).await?;

        if !new.depends_on.is_empty() {
            Self::insert_dependencies(&mut tx, &goal.id, &id, &new.depends_on).await?;
        }

        tx.commit().await?;
        let task = self.get_task(&id).await?;
        self.publish(Change::TaskCreated(task.clone()));
        Ok(task)
    }

    /// Replace the dependency set of a task (planner, pre-start only).
    pub async fn set_task_dependencies(&self, task_id: &str, depends_on: &[String]) -> Result<()> {
        let mut tx = self.w().begin().await?;
        // Status is validated on the row inside the write transaction: a check
        // against the read pool could be stale by the time we hold the lock.
        let task = Self::get_task_in_tx(&mut tx, task_id).await?;
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
                    Actor::Planner,
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
        let mut tx = self.w().begin().await?;
        // Status is validated on the row inside the write transaction: a check
        // against the read pool could be stale by the time we hold the lock.
        let task = Self::get_task_in_tx(&mut tx, id).await?;
        if !matches!(task.status(), TaskStatus::Pending | TaskStatus::Ready) {
            return Err(StoreError::Conflict(format!(
                "task can only be edited while pending/ready, it is {}",
                task.status
            )));
        }
        let title = update.title.unwrap_or(task.title);
        let description = update.description.unwrap_or(task.description);
        // The integrator is reassignable while the task has not started, the
        // way the reviewers below are: nothing of it is pinned onto the task,
        // so the swap is the id and nothing else.
        let integrator = update
            .integrator_profile_id
            .unwrap_or(task.integrator_profile_id);
        sqlx::query(
            "UPDATE tasks SET title = ?, description = ?, integrator_profile_id = ?,
                              updated_at = ?
             WHERE id = ?",
        )
        .bind(&title)
        .bind(&description)
        .bind(&integrator)
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
            // Reassigning reviewers writes new slots, so each one pins the
            // profile as it stands now, the same way creation does.
            Self::insert_reviewers(&mut tx, id, &reviewers).await?;
        }
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
        let task = Self::get_task_in_tx(&mut tx, id).await?;
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

    /// Fetch a task row inside an open write transaction, so status checks see
    /// the state the transaction will actually commit against.
    async fn get_task_in_tx(
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        id: &str,
    ) -> Result<Task> {
        sqlx::query_as::<_, Task>("SELECT * FROM tasks WHERE id = ?")
            .bind(id)
            .fetch_optional(&mut **tx)
            .await?
            .ok_or_else(|| not_found("task", id))
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
        .bind(&task.id)
        .execute(&mut **tx)
        .await?;

        let transition = TaskTransition {
            id: new_id(),
            task_id: task.id.clone(),
            from_status: from.as_str().to_string(),
            to_status: to.as_str().to_string(),
            actor: actor.as_str().to_string(),
            reason: reason.map(str::to_string),
            created_at: now(),
        };
        sqlx::query(
            "INSERT INTO task_transitions (id, task_id, from_status, to_status, actor, reason, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&transition.id)
        .bind(&transition.task_id)
        .bind(&transition.from_status)
        .bind(&transition.to_status)
        .bind(&transition.actor)
        .bind(&transition.reason)
        .bind(&transition.created_at)
        .execute(&mut **tx)
        .await?;

        Ok(transition)
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

    /// The reviewer slots themselves, in the same order, each carrying the
    /// agent and model it was pinned to when it was assigned.
    pub async fn list_task_reviewer_pins(&self, task_id: &str) -> Result<Vec<TaskReviewer>> {
        Ok(sqlx::query_as::<_, TaskReviewer>(
            "SELECT * FROM task_reviewers WHERE task_id = ? ORDER BY position",
        )
        .bind(task_id)
        .fetch_all(self.r())
        .await?)
    }

    /// Write one reviewer slot per profile, in the order given, pinning each
    /// profile's agent and model onto the slot. Read inside the transaction
    /// that writes the slots, so a profile edited in between cannot land
    /// half-applied across them.
    async fn insert_reviewers(
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        task_id: &str,
        reviewer_profile_ids: &[String],
    ) -> Result<()> {
        for (position, profile_id) in reviewer_profile_ids.iter().enumerate() {
            let profile = Self::get_profile_in_tx(tx, profile_id).await?;
            sqlx::query(
                "INSERT INTO task_reviewers (task_id, profile_id, position, agent_kind, model)
                 VALUES (?, ?, ?, ?, ?)",
            )
            .bind(task_id)
            .bind(&profile.id)
            .bind(position as i64)
            .bind(&profile.agent_kind)
            .bind(&profile.model)
            .execute(&mut **tx)
            .await?;
        }
        Ok(())
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
        let n = sqlx::query("UPDATE tasks SET worktree_path = ?, updated_at = ? WHERE id = ?")
            .bind(worktree_path)
            .bind(now())
            .bind(task_id)
            .execute(self.w())
            .await?
            .rows_affected();
        self.publish_task_update(task_id, n).await
    }

    /// Record the pull request a task was published as, in full: the number
    /// the daemon polls with and the URL that says which forge it is on.
    ///
    /// Idempotent by construction — the integrator reports the pull request it
    /// opened, and re-reporting the same one writes the same row. Recording a
    /// *different* pull request resets the bookkeeping with it: the comments
    /// relayed and the approval announced belonged to the old one.
    pub async fn set_task_pull_request(&self, task_id: &str, number: i64, url: &str) -> Result<()> {
        let task = self.get_task(task_id).await?;
        let same = task.pr_url.as_deref() == Some(url);
        let n = sqlx::query(
            "UPDATE tasks
             SET pr_number = ?, pr_url = ?, pr_relayed_comments = ?,
                 pr_approved_notified = ?, updated_at = ?
             WHERE id = ?",
        )
        .bind(number)
        .bind(url)
        .bind(same.then(|| task.pr_relayed_comments.clone()).flatten())
        .bind(i64::from(same && task.pr_approved_notified()))
        .bind(now())
        .bind(task_id)
        .execute(self.w())
        .await?
        .rows_affected();
        self.publish_task_update(task_id, n).await
    }

    /// Mark pull request comments as relayed to the engineer, adding to
    /// whatever was relayed before: what keeps the daemon from relaying one
    /// comment twice as it polls.
    pub async fn add_task_pr_relayed_comments(&self, task_id: &str, ids: &[String]) -> Result<()> {
        let task = self.get_task(task_id).await?;
        let mut relayed = task.pr_relayed_comments();
        for id in ids {
            if !relayed.contains(id) {
                relayed.push(id.clone());
            }
        }
        let json = serde_json::to_string(&relayed).unwrap_or_else(|_| "[]".into());
        let n =
            sqlx::query("UPDATE tasks SET pr_relayed_comments = ?, updated_at = ? WHERE id = ?")
                .bind(&json)
                .bind(now())
                .bind(task_id)
                .execute(self.w())
                .await?
                .rows_affected();
        self.publish_task_update(task_id, n).await
    }

    /// Whether the user has been told the pull request is approved: set when
    /// they are told, cleared when the approval goes away again, so a second
    /// approval is announced and a poll that changes nothing is not.
    pub async fn set_task_pr_approved_notified(&self, task_id: &str, notified: bool) -> Result<()> {
        let n =
            sqlx::query("UPDATE tasks SET pr_approved_notified = ?, updated_at = ? WHERE id = ?")
                .bind(notified as i64)
                .bind(now())
                .bind(task_id)
                .execute(self.w())
                .await?
                .rows_affected();
        self.publish_task_update(task_id, n).await
    }

    pub async fn set_task_stalled(&self, task_id: &str, stalled: bool) -> Result<()> {
        let n = sqlx::query("UPDATE tasks SET stalled = ?, updated_at = ? WHERE id = ?")
            .bind(stalled as i64)
            .bind(now())
            .bind(task_id)
            .execute(self.w())
            .await?
            .rows_affected();
        self.publish_task_update(task_id, n).await
    }

    /// Announce a non-transitional task write, unless it matched no row.
    async fn publish_task_update(&self, task_id: &str, rows_affected: u64) -> Result<()> {
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
