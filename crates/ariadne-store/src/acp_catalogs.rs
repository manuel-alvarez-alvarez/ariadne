//! The catalog each registry agent offered, kept per agent version.
//!
//! Discovery reads an agent's models and efforts from a `session/new`, and a
//! session is not free: some agents store every one they open. So the
//! catalog is kept here under the command and version it was read from, and
//! a daemon start opens a session only on an agent whose version it has not
//! read. One row per agent: a newer read replaces the older.

use crate::{AcpCatalog, Result, Store, StoreError, now};

impl Store {
    /// Every kept catalog, by agent id.
    pub async fn list_acp_catalogs(&self) -> Result<Vec<AcpCatalog>> {
        Ok(
            sqlx::query_as::<_, AcpCatalog>("SELECT * FROM acp_catalogs ORDER BY agent_id")
                .fetch_all(self.r())
                .await?,
        )
    }

    /// Keep `catalog` as what `agent_id`, run as `command`, offers at
    /// `version`, in place of whatever was kept for it before.
    pub async fn put_acp_catalog(
        &self,
        agent_id: &str,
        command: &[String],
        version: &str,
        catalog: &str,
    ) -> Result<AcpCatalog> {
        let command =
            serde_json::to_string(command).map_err(|e| StoreError::Invalid(e.to_string()))?;
        sqlx::query(
            "INSERT INTO acp_catalogs (agent_id, command, version, catalog, read_at)
             VALUES (?, ?, ?, ?, ?)
             ON CONFLICT (agent_id) DO UPDATE SET command = excluded.command,
                                                 version = excluded.version,
                                                 catalog = excluded.catalog,
                                                 read_at = excluded.read_at",
        )
        .bind(agent_id)
        .bind(&command)
        .bind(version)
        .bind(catalog)
        .bind(now())
        .execute(self.w())
        .await?;
        self.fetch_by("acp catalog", "acp_catalogs", "agent_id", agent_id)
            .await
    }
}
