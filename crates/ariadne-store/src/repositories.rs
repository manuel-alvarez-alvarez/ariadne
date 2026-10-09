//! Repository repository: the git checkouts Ariadne knows about.

use ariadne_core::id::new_id;
use ariadne_core::{Landing, PermissionMode};

use crate::forge::{ForgeWrite, write_forge};
use crate::skills::plural_list;
use crate::{Change, Repository, Result, SetForgeIntegration, Store, StoreError, now};

#[derive(Debug, Clone)]
pub struct NewRepository {
    /// Absolute path of the checkout.
    pub path: String,
    pub base_branch: String,
    pub description: Option<String>,
    /// None = `auto`.
    pub permission_mode: Option<PermissionMode>,
    /// None = `merge`.
    pub default_landing: Option<Landing>,
}

/// Partial update; `None` leaves a field alone.
#[derive(Debug, Clone, Default)]
pub struct RepositoryUpdate {
    pub path: Option<String>,
    pub base_branch: Option<String>,
    /// Some(None) clears the description.
    pub description: Option<Option<String>>,
    pub permission_mode: Option<PermissionMode>,
    pub default_landing: Option<Landing>,
}

impl Store {
    pub async fn create_repository(&self, new: NewRepository) -> Result<Repository> {
        self.create_repository_with_forge(new, None).await
    }

    /// Register a repository and write its forge row in one transaction:
    /// a refused forge leaves no repository behind. The row's
    /// `repository_id` is the new repository's, whatever `forge` says.
    pub async fn create_repository_with_forge(
        &self,
        new: NewRepository,
        forge: Option<SetForgeIntegration>,
    ) -> Result<Repository> {
        let id = new_id();
        let ts = now();
        let mut tx = self.w().begin().await?;
        sqlx::query(
            "INSERT INTO repositories (id, path, base_branch, description, permission_mode, default_landing,
                                       created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(&new.path)
        .bind(&new.base_branch)
        .bind(&new.description)
        .bind(new.permission_mode.unwrap_or(PermissionMode::Auto).as_str())
        .bind(new.default_landing.unwrap_or(Landing::Merge).as_str())
        .bind(&ts)
        .bind(&ts)
        .execute(&mut *tx)
        .await
        .map_err(|e| taken(e, &new.path, &new.base_branch))?;
        if let Some(row) = forge {
            let row = SetForgeIntegration {
                repository_id: id.clone(),
                ..row
            };
            if let Err(e) = write_forge(&mut tx, ForgeWrite::Set(Box::new(row.clone()))).await {
                drop(tx);
                return Err(self.forge_refusal(e, &row).await);
            }
        }
        tx.commit().await?;
        let repository = self.get_repository(&id).await?;
        self.publish(Change::RepositoryCreated(repository.clone()));
        Ok(repository)
    }

    pub async fn get_repository(&self, id: &str) -> Result<Repository> {
        let repository: Repository = self
            .fetch_by("repository", "repositories", "id", id)
            .await?;
        Ok(Repository {
            forge: self.forge_integration(id).await?,
            ..repository
        })
    }

    pub async fn list_repositories(&self) -> Result<Vec<Repository>> {
        let mut repositories = sqlx::query_as::<_, Repository>(
            "SELECT * FROM repositories ORDER BY path, base_branch",
        )
        .fetch_all(self.r())
        .await?;
        self.attach_forges(&mut repositories).await?;
        Ok(repositories)
    }

    pub async fn update_repository(
        &self,
        id: &str,
        update: RepositoryUpdate,
    ) -> Result<Repository> {
        self.update_repository_with_forge(id, update, ForgeWrite::Keep)
            .await
    }

    /// Edit a repository and its forge row in one transaction, publishing
    /// once: a refused forge leaves the repository as it was.
    pub async fn update_repository_with_forge(
        &self,
        id: &str,
        update: RepositoryUpdate,
        forge: ForgeWrite,
    ) -> Result<Repository> {
        let current = self.get_repository(id).await?;
        let permission_mode = update
            .permission_mode
            .unwrap_or_else(|| current.permission_mode());
        let default_landing = update
            .default_landing
            .unwrap_or_else(|| current.default_landing());
        let path = update.path.unwrap_or(current.path);
        let base_branch = update.base_branch.unwrap_or(current.base_branch);
        let description = update.description.unwrap_or(current.description);
        let mut tx = self.w().begin().await?;
        sqlx::query(
            "UPDATE repositories SET path = ?, base_branch = ?, description = ?,
                                     permission_mode = ?, default_landing = ?, updated_at = ?
             WHERE id = ?",
        )
        .bind(&path)
        .bind(&base_branch)
        .bind(&description)
        .bind(permission_mode.as_str())
        .bind(default_landing.as_str())
        .bind(now())
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|e| taken(e, &path, &base_branch))?;
        if let Err(e) = write_forge(&mut tx, forge.clone()).await {
            drop(tx);
            return Err(match forge {
                ForgeWrite::Set(row) => self.forge_refusal(e, &row).await,
                _ => e,
            });
        }
        tx.commit().await?;
        let repository = self.get_repository(id).await?;
        self.publish(Change::RepositoryUpdated(repository.clone()));
        Ok(repository)
    }

    /// Delete a repository; fails with `Conflict` while a goal or a task
    /// still references it, naming which, as [`Store::delete_skill`] does.
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
