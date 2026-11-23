//! The far tasks of a goal (037): tasks that run on another host. The goal's
//! daemon keeps each one in its own task list, with its host, the linked goal
//! on that host and the task there, and mirrors what the host reports of it.

use ariadne_core::id::new_id;
use ariadne_core::{Actor, TaskStatus};

use crate::{Change, Result, Store, Task, TaskTransition, now};

/// What the host reports of a far task: the fields its goal's daemon mirrors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FarTaskState {
    pub status: TaskStatus,
    pub step: Option<String>,
    pub branch: String,
    pub pr_url: Option<String>,
    pub merge_commit: Option<String>,
    /// Why a `failed` or `cancelled` task ended, as the host gives it.
    pub reason: Option<String>,
}

impl Store {
    /// The far tasks on `host` whose goal is not over, oldest first.
    pub async fn list_far_tasks(&self, host: &str) -> Result<Vec<Task>> {
        // Only the goals that are not over: their far tasks are followed.
        Ok(sqlx::query_as::<_, Task>(
            "SELECT * FROM tasks WHERE host = ?
               AND goal_id IN (SELECT id FROM goals WHERE status IN ('planning', 'active'))
             ORDER BY id",
        )
        .bind(host)
        .fetch_all(self.r())
        .await?)
    }

    /// The far task that `host` knows as `far_task_id`, if there is one.
    pub async fn far_task_by_far_id(&self, host: &str, far_task_id: &str) -> Result<Option<Task>> {
        Ok(
            sqlx::query_as::<_, Task>("SELECT * FROM tasks WHERE host = ? AND far_task_id = ?")
                .bind(host)
                .bind(far_task_id)
                .fetch_optional(self.r())
                .await?,
        )
    }

    /// The linked goal on `host` that the far tasks of `goal_id` go to, once
    /// one of them is sent there.
    pub async fn far_goal_of(&self, goal_id: &str, host: &str) -> Result<Option<String>> {
        Ok(sqlx::query_scalar(
            "SELECT far_goal_id FROM tasks
             WHERE goal_id = ? AND host = ? AND far_goal_id IS NOT NULL
             ORDER BY id LIMIT 1",
        )
        .bind(goal_id)
        .bind(host)
        .fetch_optional(self.r())
        .await?)
    }

    /// Record the linked goal on the host that a far task goes to.
    pub async fn set_far_goal(&self, task_id: &str, far_goal_id: &str) -> Result<()> {
        let n = sqlx::query("UPDATE tasks SET far_goal_id = ?, updated_at = ? WHERE id = ?")
            .bind(far_goal_id)
            .bind(now())
            .bind(task_id)
            .execute(self.w())
            .await?
            .rows_affected();
        self.publish_task_update(task_id, n).await
    }

    /// Record the linked goal and the task that the host made of a far task.
    pub async fn set_far_task(
        &self,
        task_id: &str,
        far_goal_id: &str,
        far_task_id: &str,
    ) -> Result<()> {
        let n = sqlx::query(
            "UPDATE tasks SET far_goal_id = ?, far_task_id = ?, updated_at = ? WHERE id = ?",
        )
        .bind(far_goal_id)
        .bind(far_task_id)
        .bind(now())
        .bind(task_id)
        .execute(self.w())
        .await?
        .rows_affected();
        self.publish_task_update(task_id, n).await
    }

    /// Set `host_unreachable` on each far task of `host` in a goal that is
    /// not over. Only a task that changes is written and announced.
    pub async fn set_host_unreachable(&self, host: &str, unreachable: bool) -> Result<()> {
        let flag = i64::from(unreachable);
        let changed: Vec<String> = sqlx::query_scalar(
            "UPDATE tasks SET host_unreachable = ?, updated_at = ?
             WHERE host = ? AND host_unreachable <> ?
               AND goal_id IN (SELECT id FROM goals WHERE status IN ('planning', 'active'))
             RETURNING id",
        )
        .bind(flag)
        .bind(now())
        .bind(host)
        .bind(flag)
        .fetch_all(self.w())
        .await?;
        for id in changed {
            self.publish_task_update(&id, 1).await?;
        }
        Ok(())
    }

    /// Write what the host reports of a far task, and clear its
    /// `host_unreachable`. A change of status writes a transition by the
    /// daemon with the host's reason. The host's own state machine has
    /// checked the move, so this one checks nothing. Answers whether the
    /// status changed. Nothing is written or announced when nothing changed.
    pub async fn mirror_far_task(&self, task_id: &str, state: &FarTaskState) -> Result<bool> {
        let mut tx = self.w().begin().await?;
        let task: Task = Self::fetch_by_in_tx(&mut tx, "task", "tasks", task_id).await?;
        let moved = task.status() != state.status;
        if !moved
            && task.step == state.step
            && task.branch == state.branch
            && task.pr_url == state.pr_url
            && task.merge_commit == state.merge_commit
            && !task.is_host_unreachable()
        {
            return Ok(false);
        }
        let timestamp = now();
        sqlx::query(
            "UPDATE tasks SET status = ?, step = ?, branch = ?, pr_url = ?, merge_commit = ?,
                              host_unreachable = 0, updated_at = ?
             WHERE id = ?",
        )
        .bind(state.status.as_str())
        .bind(&state.step)
        .bind(&state.branch)
        .bind(&state.pr_url)
        .bind(&state.merge_commit)
        .bind(&timestamp)
        .bind(task_id)
        .execute(&mut *tx)
        .await?;
        let transition = if moved {
            let transition = TaskTransition {
                from_step: task.step.clone(),
                to_step: state.step.clone(),
                id: new_id(),
                task_id: task.id.clone(),
                from_status: task.status.clone(),
                to_status: state.status.as_str().to_string(),
                actor: Actor::Daemon.as_str().to_string(),
                reason: state.reason.clone(),
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
            .execute(&mut *tx)
            .await?;
            Some(transition)
        } else {
            None
        };
        tx.commit().await?;
        let task = self.get_task(task_id).await?;
        self.publish(Change::TaskUpdated { task, transition });
        Ok(moved)
    }
}
