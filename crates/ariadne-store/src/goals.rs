//! Goal repository.

use ariadne_core::GoalStatus;
use ariadne_core::id::new_id;

use crate::{AgentPin, Change, Goal, Repository, Result, Store, StoreError, not_found, now};

#[derive(Debug, Clone)]
pub struct NewGoal {
    pub title: String,
    pub description: String,
    /// Ids of registered repositories the goal works in; each must exist.
    /// The goal reads them live, so editing one moves the goal with it.
    pub repository_ids: Vec<String>,
    /// What this goal's orchestrator runs on. None = auto CLI, that CLI's
    /// default model, and whatever it runs the model at.
    pub pin: Option<AgentPin>,
}

impl Store {
    pub async fn create_goal(&self, new: NewGoal) -> Result<Goal> {
        if new.repository_ids.is_empty() {
            return Err(StoreError::Invalid("a goal needs at least one repo".into()));
        }
        // Validated before the goal row is written, so an unknown id leaves
        // nothing behind. The same repository named twice is one reference.
        let mut repository_ids: Vec<String> = Vec::with_capacity(new.repository_ids.len());
        for id in &new.repository_ids {
            self.get_repository(id).await?;
            if !repository_ids.contains(id) {
                repository_ids.push(id.clone());
            }
        }
        let id = new_id();
        let ts = now();
        let mut tx = self.w().begin().await?;
        let (agent_kind, model, effort) = AgentPin::columns(new.pin.as_ref());
        sqlx::query(
            "INSERT INTO goals (id, title, description, status,
                                agent_kind, model, effort, created_at, updated_at)
             VALUES (?, ?, ?, 'planning', ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(&new.title)
        .bind(&new.description)
        .bind(&agent_kind)
        .bind(&model)
        .bind(&effort)
        .bind(&ts)
        .bind(&ts)
        .execute(&mut *tx)
        .await?;
        for repository_id in &repository_ids {
            sqlx::query("INSERT INTO goal_repositories (goal_id, repository_id) VALUES (?, ?)")
                .bind(&id)
                .bind(repository_id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        let goal = self.get_goal(&id).await?;
        self.publish(Change::GoalCreated(goal.clone()));
        Ok(goal)
    }

    pub async fn get_goal(&self, id: &str) -> Result<Goal> {
        self.fetch_by("goal", "goals", "id", id).await
    }

    /// List goals, narrowed to `statuses` (a goal matches any one of them).
    /// An empty slice means no status filter at all.
    pub async fn list_goals(&self, statuses: &[GoalStatus]) -> Result<Vec<Goal>> {
        let mut sql = String::from("SELECT * FROM goals");
        if !statuses.is_empty() {
            sql.push_str(" WHERE status IN (");
            for i in 0..statuses.len() {
                if i > 0 {
                    sql.push_str(", ");
                }
                sql.push('?');
            }
            sql.push(')');
        }
        sql.push_str(" ORDER BY id");
        // Safe: only fixed clause fragments are appended; values are bound.
        let mut q = sqlx::query_as::<_, Goal>(sqlx::AssertSqlSafe(sql));
        for status in statuses {
            q = q.bind(status.as_str());
        }
        Ok(q.fetch_all(self.r()).await?)
    }

    pub async fn set_goal_status(&self, id: &str, status: GoalStatus) -> Result<Goal> {
        let n = sqlx::query("UPDATE goals SET status = ?, updated_at = ? WHERE id = ?")
            .bind(status.as_str())
            .bind(now())
            .bind(id)
            .execute(self.w())
            .await?
            .rows_affected();
        if n == 0 {
            return Err(not_found("goal", id));
        }
        let goal = self.get_goal(id).await?;
        self.publish(Change::GoalUpdated(goal.clone()));
        Ok(goal)
    }

    /// Announce a goal as it now stands, for a write that changed something
    /// a goal is read with rather than the goal row itself — the usage of a
    /// session under it, which rides in the goal's own fat event.
    pub(crate) async fn publish_goal_update(&self, id: &str) -> Result<()> {
        let goal = self.get_goal(id).await?;
        self.publish(Change::GoalUpdated(goal));
        Ok(())
    }

    /// Hard-delete a goal and, via ON DELETE CASCADE, all its children. The
    /// normal lifecycle uses cancel; deleting drops a finished goal for good,
    /// so nothing is left to refetch and the event carries the id alone.
    pub async fn delete_goal(&self, id: &str) -> Result<()> {
        let n = sqlx::query("DELETE FROM goals WHERE id = ?")
            .bind(id)
            .execute(self.w())
            .await?
            .rows_affected();
        if n == 0 {
            return Err(not_found("goal", id));
        }
        self.publish(Change::GoalDeleted(id.to_string()));
        Ok(())
    }

    /// The repositories a goal works in, as they stand right now: the goal
    /// holds references, not copies. Ordered like
    /// [`Store::list_repositories`].
    pub async fn list_goal_repositories(&self, goal_id: &str) -> Result<Vec<Repository>> {
        Ok(sqlx::query_as::<_, Repository>(
            "SELECT r.* FROM goal_repositories gr
               JOIN repositories r ON r.id = gr.repository_id
              WHERE gr.goal_id = ?
              ORDER BY r.path, r.base_branch",
        )
        .bind(goal_id)
        .fetch_all(self.r())
        .await?)
    }
}
