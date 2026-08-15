//! Goal repository.

use ariadne_core::GoalStatus;
use ariadne_core::id::new_id;

use crate::{Goal, GoalRepo, Result, Store, StoreError, not_found, now};

#[derive(Debug, Clone)]
pub struct NewGoal {
    pub title: String,
    pub description: String,
    pub planner_profile_id: String,
    pub max_tasks: Option<i64>,
    pub required_approvals: i64,
    /// (path, base_branch) pairs; paths must already be validated by the caller.
    pub repos: Vec<(String, String)>,
}

impl Store {
    pub async fn create_goal(&self, new: NewGoal) -> Result<Goal> {
        if new.repos.is_empty() {
            return Err(StoreError::Invalid("a goal needs at least one repo".into()));
        }
        if new.required_approvals < 1 {
            return Err(StoreError::Invalid(
                "required_approvals must be >= 1".into(),
            ));
        }
        let id = new_id();
        let ts = now();
        let mut tx = self.w().begin().await?;
        sqlx::query(
            "INSERT INTO goals (id, title, description, status, max_tasks, required_approvals,
                                planner_profile_id, created_at, updated_at)
             VALUES (?, ?, ?, 'planning', ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(&new.title)
        .bind(&new.description)
        .bind(new.max_tasks)
        .bind(new.required_approvals)
        .bind(&new.planner_profile_id)
        .bind(&ts)
        .bind(&ts)
        .execute(&mut *tx)
        .await?;
        for (path, base_branch) in &new.repos {
            sqlx::query(
                "INSERT INTO goal_repos (id, goal_id, path, base_branch) VALUES (?, ?, ?, ?)",
            )
            .bind(new_id())
            .bind(&id)
            .bind(path)
            .bind(base_branch)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        self.get_goal(&id).await
    }

    pub async fn get_goal(&self, id: &str) -> Result<Goal> {
        sqlx::query_as::<_, Goal>("SELECT * FROM goals WHERE id = ?")
            .bind(id)
            .fetch_optional(self.r())
            .await?
            .ok_or_else(|| not_found("goal", id))
    }

    pub async fn list_goals(&self, status: Option<GoalStatus>) -> Result<Vec<Goal>> {
        let rows = match status {
            Some(s) => {
                sqlx::query_as::<_, Goal>("SELECT * FROM goals WHERE status = ? ORDER BY id")
                    .bind(s.as_str())
                    .fetch_all(self.r())
                    .await?
            }
            None => {
                sqlx::query_as::<_, Goal>("SELECT * FROM goals ORDER BY id")
                    .fetch_all(self.r())
                    .await?
            }
        };
        Ok(rows)
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
        self.get_goal(id).await
    }

    /// Hard-delete a goal and (via ON DELETE CASCADE) all its children.
    /// Admin/maintenance path — the normal lifecycle uses cancel.
    pub async fn delete_goal(&self, id: &str) -> Result<()> {
        let n = sqlx::query("DELETE FROM goals WHERE id = ?")
            .bind(id)
            .execute(self.w())
            .await?
            .rows_affected();
        if n == 0 {
            return Err(not_found("goal", id));
        }
        Ok(())
    }

    pub async fn list_goal_repos(&self, goal_id: &str) -> Result<Vec<GoalRepo>> {
        Ok(
            sqlx::query_as::<_, GoalRepo>("SELECT * FROM goal_repos WHERE goal_id = ? ORDER BY id")
                .bind(goal_id)
                .fetch_all(self.r())
                .await?,
        )
    }

    pub async fn get_goal_repo(&self, repo_id: &str) -> Result<GoalRepo> {
        sqlx::query_as::<_, GoalRepo>("SELECT * FROM goal_repos WHERE id = ?")
            .bind(repo_id)
            .fetch_optional(self.r())
            .await?
            .ok_or_else(|| not_found("goal repo", repo_id))
    }
}
