//! The forge a repository's remote is on (025): one row per repository, and
//! none where the checkout has no usable remote.
//!
//! A write here is a write to the repository as clients see it, so it
//! publishes [`Change::RepositoryUpdated`] with the forge on the row. A
//! repository write that carries a forge change makes both in one
//! transaction (`create_repository_with_forge`,
//! `update_repository_with_forge`), so a refused forge writes nothing at all.

use ariadne_core::ForgeKind;

use sqlx::{Sqlite, Transaction};

use crate::{Change, ForgeIntegration, Repository, Result, Store, StoreError, now};

/// The whole row a write leaves behind, timestamps apart. `host`, `owner`
/// and `name` are lower-cased on the way in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetForgeIntegration {
    pub repository_id: String,
    pub kind: ForgeKind,
    pub host: String,
    pub owner: String,
    pub name: String,
    pub remote: String,
    pub enabled: bool,
    pub login: Option<String>,
    pub babysit_model: Option<String>,
    pub babysit_effort: Option<String>,
    pub review_model: Option<String>,
    pub review_effort: Option<String>,
}

/// What a write does to a repository's forge row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForgeWrite {
    /// Leave the row as it is.
    Keep,
    /// Drop the row of this repository.
    Clear(String),
    /// Write the row whole.
    Set(Box<SetForgeIntegration>),
}

/// Apply `write` inside `tx`. Answers whether a row was written or dropped.
/// A second enabled row of one forge repository fails on the unique index;
/// [`Store::forge_refusal`] says it in the caller's terms.
pub(crate) async fn write_forge(
    tx: &mut Transaction<'_, Sqlite>,
    write: ForgeWrite,
) -> Result<bool> {
    let set = match write {
        ForgeWrite::Keep => return Ok(false),
        ForgeWrite::Clear(repo_id) => {
            let dropped = sqlx::query("DELETE FROM forge_integrations WHERE repository_id = ?")
                .bind(repo_id)
                .execute(&mut **tx)
                .await?
                .rows_affected();
            return Ok(dropped > 0);
        }
        ForgeWrite::Set(set) => *set,
    };
    let ts = now();
    sqlx::query(
        "INSERT INTO forge_integrations
             (repository_id, kind, host, owner, name, remote, enabled, login,
              babysit_model, babysit_effort, review_model, review_effort,
              detected_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT (repository_id) DO UPDATE SET
             detected_at = CASE
                 WHEN kind = excluded.kind AND host = excluded.host AND owner = excluded.owner
                      AND name = excluded.name AND remote = excluded.remote
                 THEN detected_at ELSE excluded.detected_at END,
             kind = excluded.kind, host = excluded.host, owner = excluded.owner,
             name = excluded.name, remote = excluded.remote, enabled = excluded.enabled,
             login = excluded.login, babysit_model = excluded.babysit_model,
             babysit_effort = excluded.babysit_effort, review_model = excluded.review_model,
             review_effort = excluded.review_effort, updated_at = excluded.updated_at",
    )
    .bind(&set.repository_id)
    .bind(set.kind.as_str())
    .bind(set.host.to_lowercase())
    .bind(set.owner.to_lowercase())
    .bind(set.name.to_lowercase())
    .bind(&set.remote)
    .bind(set.enabled)
    .bind(&set.login)
    .bind(&set.babysit_model)
    .bind(&set.babysit_effort)
    .bind(&set.review_model)
    .bind(&set.review_effort)
    .bind(&ts)
    .bind(&ts)
    .execute(&mut **tx)
    .await?;
    Ok(true)
}

impl Store {
    /// The forge of one repository, or None where it has no usable remote.
    pub async fn forge_integration(&self, repo_id: &str) -> Result<Option<ForgeIntegration>> {
        Ok(
            sqlx::query_as("SELECT * FROM forge_integrations WHERE repository_id = ?")
                .bind(repo_id)
                .fetch_optional(self.r())
                .await?,
        )
    }

    /// Every forge Ariadne works with: the rows that are enabled.
    pub async fn enabled_forge_integrations(&self) -> Result<Vec<ForgeIntegration>> {
        Ok(sqlx::query_as(
            "SELECT * FROM forge_integrations WHERE enabled = 1 ORDER BY repository_id",
        )
        .fetch_all(self.r())
        .await?)
    }

    /// Write the forge of a repository whole, and answer the repository as it
    /// now stands. `detected_at` moves only when the remote it names moved.
    ///
    /// One forge repository is enabled on one repository row at a time: an
    /// enable that would make a second is refused with `Conflict`, naming the
    /// row that holds it, and writes nothing.
    pub async fn set_forge_integration(&self, set: SetForgeIntegration) -> Result<Repository> {
        let id = set.repository_id.clone();
        self.get_repository(&id).await?;
        let mut tx = self.w().begin().await?;
        if let Err(e) = write_forge(&mut tx, ForgeWrite::Set(Box::new(set.clone()))).await {
            drop(tx);
            return Err(self.forge_refusal(e, &set).await);
        }
        tx.commit().await?;
        let repository = self.get_repository(&id).await?;
        self.publish(Change::RepositoryUpdated(repository.clone()));
        Ok(repository)
    }

    /// Whether a forge repository may be enabled on `repo_id`: refused with
    /// `Conflict`, naming the row, while another repository row has it on.
    pub async fn forge_enabled_elsewhere(
        &self,
        repo_id: &str,
        host: &str,
        owner: &str,
        name: &str,
    ) -> Result<()> {
        let (host, owner, name) = (
            host.to_lowercase(),
            owner.to_lowercase(),
            name.to_lowercase(),
        );
        let holder: Option<String> = sqlx::query_scalar(
            "SELECT repository_id FROM forge_integrations
              WHERE enabled = 1 AND host = ? AND owner = ? AND name = ? AND repository_id <> ?",
        )
        .bind(&host)
        .bind(&owner)
        .bind(&name)
        .bind(repo_id)
        .fetch_optional(self.r())
        .await?;
        match holder {
            Some(_) => Err(self.enabled_elsewhere(&host, &owner, &name).await),
            None => Ok(()),
        }
    }

    /// Drop the forge of a repository whose remote went away. Publishes only
    /// where there was a row to drop; answers whether there was.
    pub async fn clear_forge_integration(&self, repo_id: &str) -> Result<bool> {
        let mut tx = self.w().begin().await?;
        let dropped = write_forge(&mut tx, ForgeWrite::Clear(repo_id.to_string())).await?;
        tx.commit().await?;
        if dropped {
            let repository = self.get_repository(repo_id).await?;
            self.publish(Change::RepositoryUpdated(repository));
        }
        Ok(dropped)
    }

    /// A failed forge write, as the caller is told it: the unique index on
    /// enabled rows is the second enabled row of one forge repository.
    pub(crate) async fn forge_refusal(
        &self,
        e: StoreError,
        set: &SetForgeIntegration,
    ) -> StoreError {
        match e {
            StoreError::Db(sqlx::Error::Database(ref db)) if db.is_unique_violation() => {
                self.enabled_elsewhere(
                    &set.host.to_lowercase(),
                    &set.owner.to_lowercase(),
                    &set.name.to_lowercase(),
                )
                .await
            }
            other => other,
        }
    }

    /// Fill the forge of each repository read, in one query.
    pub(crate) async fn attach_forges(&self, repositories: &mut [Repository]) -> Result<()> {
        if repositories.is_empty() {
            return Ok(());
        }
        let rows: Vec<ForgeIntegration> = sqlx::query_as("SELECT * FROM forge_integrations")
            .fetch_all(self.r())
            .await?;
        for repository in repositories {
            repository.forge = rows
                .iter()
                .find(|row| row.repository_id == repository.id)
                .cloned();
        }
        Ok(())
    }

    /// The refusal of a second enabled row, naming the first.
    async fn enabled_elsewhere(&self, host: &str, owner: &str, name: &str) -> StoreError {
        let holder: std::result::Result<Option<Repository>, _> = sqlx::query_as(
            "SELECT r.* FROM forge_integrations f JOIN repositories r ON r.id = f.repository_id
              WHERE f.enabled = 1 AND f.host = ? AND f.owner = ? AND f.name = ?",
        )
        .bind(host)
        .bind(owner)
        .bind(name)
        .fetch_optional(self.r())
        .await;
        let held_by = match holder {
            Ok(Some(r)) => format!("repository {} ({} [{}])", r.id, r.path, r.base_branch),
            _ => "another repository".to_string(),
        };
        StoreError::Conflict(format!(
            "the forge integration of {host}/{owner}/{name} is already enabled on {held_by}; \
             disable it there first"
        ))
    }
}
