//! Which models a plan is allowed to be staffed on.
//!
//! The catalog itself is not in the database: it is what `ariadne-core`
//! curates per agent CLI, plus what `opencode models --verbose` discovers at
//! runtime. So what is stored here is the user's subtraction from it — the
//! models they turned off — and everything else is available.
//!
//! That way round on purpose. A catalog that grows (a CLI ships a model, a
//! discovery finds one) hands the new entry over usable, rather than needing
//! a write here before anybody can pin it. The rows are the exceptions, and
//! there are usually none.
//!
//! A model is named by the one string it is chosen by,
//! `<agent_kind>:<model>` (`ariadne_core::ModelRef`) — the same spelling
//! `--model` takes, so the id in a row is the id a request is refused by.

use std::collections::BTreeSet;

use crate::{Result, Store, now};

impl Store {
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
