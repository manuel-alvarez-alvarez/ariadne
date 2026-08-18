//! Repository repository: the git checkouts Ariadne knows about.

use ariadne_core::id::new_id;

use crate::{Change, Repository, Result, Store, StoreError, not_found, now};

#[derive(Debug, Clone)]
pub struct NewRepository {
    /// Absolute path of the checkout.
    pub path: String,
    pub base_branch: String,
    pub description: Option<String>,
}

/// Partial update; `None` leaves a field alone.
#[derive(Debug, Clone, Default)]
pub struct RepositoryUpdate {
    pub path: Option<String>,
    pub base_branch: Option<String>,
    /// Some(None) clears the description.
    pub description: Option<Option<String>>,
}

impl Store {
    pub async fn create_repository(&self, new: NewRepository) -> Result<Repository> {
        let id = new_id();
        let ts = now();
        sqlx::query(
            "INSERT INTO repositories (id, path, base_branch, description, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(&new.path)
        .bind(&new.base_branch)
        .bind(&new.description)
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
        sqlx::query_as::<_, Repository>("SELECT * FROM repositories WHERE id = ?")
            .bind(id)
            .fetch_optional(self.r())
            .await?
            .ok_or_else(|| not_found("repository", id))
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
        sqlx::query(
            "UPDATE repositories SET path = ?, base_branch = ?, description = ?, updated_at = ?
             WHERE id = ?",
        )
        .bind(&path)
        .bind(&base_branch)
        .bind(&description)
        .bind(now())
        .bind(id)
        .execute(self.w())
        .await
        .map_err(|e| taken(e, &path, &base_branch))?;
        let repository = self.get_repository(id).await?;
        self.publish(Change::RepositoryUpdated(repository.clone()));
        Ok(repository)
    }

    /// Delete a repository. Nothing references one yet, so there is no in-use
    /// check to make: goals still carry their own `goal_repos` rows.
    pub async fn delete_repository(&self, id: &str) -> Result<()> {
        self.get_repository(id).await?;
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
