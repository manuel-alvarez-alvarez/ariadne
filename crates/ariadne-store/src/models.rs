//! Which models a plan is allowed to be staffed on.
//!
//! The catalog itself is not in the database: it is what ACP discovery finds
//! each registry agent offering at runtime. So what is stored here is the
//! user's subtraction from it — the models they turned off — and their ranks.
//! Everything else is available and unranked.
//!
//! That way round on purpose. A catalog that grows (an agent ships a model,
//! a discovery finds one) hands the new entry over usable, rather than
//! needing a write here before anybody can pin it. The rows are the
//! exceptions, and there are usually none.
//!
//! A model is named by the one string it is chosen by, `<agent>:<model>`
//! (`ariadne_core::models::ModelRef`) — the same spelling `--model` takes, so
//! the id in a row is the id a request is refused by.

use std::collections::{BTreeMap, BTreeSet};

use ariadne_core::models::ModelRank;

use crate::{Result, Store, StoreError, now};

impl Store {
    /// User-set ranks keyed by catalog id. An absent entry is unranked.
    pub async fn model_ranks(&self) -> Result<BTreeMap<String, ModelRank>> {
        let rows: Vec<(String, String)> = sqlx::query_as("SELECT id, rank FROM model_ranks")
            .fetch_all(self.r())
            .await?;
        rows.into_iter()
            .map(|(id, rank)| Ok((id, rank.parse().map_err(StoreError::Invalid)?)))
            .collect()
    }

    /// Set or clear a rank. The caller checks that discovery carries the id.
    pub async fn set_model_rank(&self, id: &str, rank: Option<ModelRank>) -> Result<()> {
        match rank {
            Some(rank) => {
                sqlx::query(
                    "INSERT INTO model_ranks (id, rank, updated_at) VALUES (?, ?, ?)
                     ON CONFLICT(id) DO UPDATE
                     SET rank = excluded.rank, updated_at = excluded.updated_at",
                )
                .bind(id)
                .bind(rank.as_str())
                .bind(now())
                .execute(self.w())
                .await?;
            }
            None => {
                sqlx::query("DELETE FROM model_ranks WHERE id = ?")
                    .bind(id)
                    .execute(self.w())
                    .await?;
            }
        }
        Ok(())
    }

    /// The models that have been turned off, as the set a catalog is read
    /// against. Sorted, since it is also what `ariadne models ls --disabled`
    /// and the desktop app list.
    pub async fn disabled_models(&self) -> Result<BTreeSet<String>> {
        let rows: Vec<(String,)> = sqlx::query_as("SELECT id FROM disabled_models")
            .fetch_all(self.r())
            .await?;
        Ok(rows.into_iter().map(|(id,)| id).collect())
    }

    /// Turn a model on or off, and say whether that changed anything: turning
    /// off one that is already off is not an error, and neither is turning on
    /// one nothing ever turned off.
    ///
    /// The id is taken as given. Whether it names an entry of the catalog is
    /// the caller's question — the catalog is code and discovery, and this
    /// module has neither.
    pub async fn set_model_enabled(&self, id: &str, enabled: bool) -> Result<bool> {
        let n = match enabled {
            true => sqlx::query("DELETE FROM disabled_models WHERE id = ?")
                .bind(id)
                .execute(self.w())
                .await?
                .rows_affected(),
            false => {
                sqlx::query("INSERT OR IGNORE INTO disabled_models (id, disabled_at) VALUES (?, ?)")
                    .bind(id)
                    .bind(now())
                    .execute(self.w())
                    .await?
                    .rows_affected()
            }
        };
        Ok(n > 0)
    }
}
