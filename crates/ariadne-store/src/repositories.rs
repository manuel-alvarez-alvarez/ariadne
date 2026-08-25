//! Repository repository: the git checkouts Ariadne knows about.

use ariadne_core::MergeStrategy;
use ariadne_core::id::new_id;

use crate::profiles::plural_list;
use crate::{Change, Repository, Result, Store, StoreError, now};

#[derive(Debug, Clone)]
pub struct NewRepository {
    /// Absolute path of the checkout.
    pub path: String,
    pub base_branch: String,
    pub description: Option<String>,
    /// How a task lands on `base_branch` here.
    pub merge_strategy: MergeStrategy,
}

/// Partial update; `None` leaves a field alone.
#[derive(Debug, Clone, Default)]
pub struct RepositoryUpdate {
    pub path: Option<String>,
    pub base_branch: Option<String>,
    /// Some(None) clears the description.
    pub description: Option<Option<String>>,
    pub merge_strategy: Option<MergeStrategy>,
}

impl Store {
    pub async fn create_repository(&self, new: NewRepository) -> Result<Repository> {
        let id = new_id();
        let ts = now();
        sqlx::query(
            "INSERT INTO repositories (id, path, base_branch, description, merge_strategy,
                                       created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(&new.path)
        .bind(&new.base_branch)
        .bind(&new.description)
        .bind(new.merge_strategy.as_str())
        .bind(&ts)
        .bind(&ts)
        .execute(self.w())
        .await
        .map_err(|e| taken(e, &new.path, &new.base_branch))?;
        let repository = self.get_repository(&id).await?;
        self.publish(Change::RepositoryCreated(repository.clone()));
        Ok(repository)
    }

    pub async fn get_repository(&self, id: &str) -> Result<Repository> {
        self.fetch_by("repository", "repositories", "id", id).await
    }

    pub async fn list_repositories(&self) -> Result<Vec<Repository>> {
        Ok(
            sqlx::query_as::<_, Repository>(
                "SELECT * FROM repositories ORDER BY path, base_branch",
            )
            .fetch_all(self.r())
            .await?,
        )
    }

    pub async fn update_repository(
        &self,
        id: &str,
        update: RepositoryUpdate,
    ) -> Result<Repository> {
        let current = self.get_repository(id).await?;
        let path = update.path.unwrap_or(current.path);
        let base_branch = update.base_branch.unwrap_or(current.base_branch);
        let description = update.description.unwrap_or(current.description);
        let merge_strategy = update
            .merge_strategy
            .map_or(current.merge_strategy, |s| s.as_str().to_string());
        sqlx::query(
            "UPDATE repositories SET path = ?, base_branch = ?, description = ?,
                                     merge_strategy = ?, updated_at = ?
             WHERE id = ?",
        )
        .bind(&path)
        .bind(&base_branch)
        .bind(&description)
        .bind(&merge_strategy)
        .bind(now())
        .bind(id)
        .execute(self.w())
        .await
        .map_err(|e| taken(e, &path, &base_branch))?;
        let repository = self.get_repository(id).await?;
        self.publish(Change::RepositoryUpdated(repository.clone()));
        Ok(repository)
    }

    /// Delete a repository; fails with `Conflict` while a goal or a task
    /// still references it, naming which, as [`Store::delete_profile`] does.
    pub async fn delete_repository(&self, id: &str) -> Result<()> {
        self.get_repository(id).await?;
        let (goals, tasks): (i64, i64) = sqlx::query_as(
            "SELECT (SELECT COUNT(*) FROM goal_repositories WHERE repository_id = ?1),
                    (SELECT COUNT(*) FROM tasks WHERE repo_id = ?1)",
        )
        .bind(id)
        .fetch_one(self.r())
        .await?;
        let referenced = plural_list(&[(goals, "goal", "goals"), (tasks, "task", "tasks")]);
        if !referenced.is_empty() {
            return Err(StoreError::Conflict(format!(
                "repository {id} is still used by {referenced}"
            )));
        }
        sqlx::query("DELETE FROM repositories WHERE id = ?")
            .bind(id)
            .execute(self.w())
            .await?;
        self.publish(Change::RepositoryDeleted(id.to_string()));
        Ok(())
    }
}

/// The `UNIQUE (path, base_branch)` violation, said in the terms the caller
/// used: one repository per checkout and base branch.
fn taken(e: sqlx::Error, path: &str, base_branch: &str) -> StoreError {
    match e {
        sqlx::Error::Database(ref db) if db.is_unique_violation() => {
            StoreError::Conflict(format!("repository already exists: {path} [{base_branch}]"))
        }
        other => StoreError::Db(other),
    }
}
