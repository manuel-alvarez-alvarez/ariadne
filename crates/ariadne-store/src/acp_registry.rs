//! The last accepted ACP registry index, replaced as one document.

use crate::{AcpRegistryIndex, Result, Store, now};

impl Store {
    pub async fn acp_registry_index(&self) -> Result<Option<AcpRegistryIndex>> {
        Ok(
            sqlx::query_as("SELECT url, document, fetched_at FROM acp_registry_index WHERE id = 1")
                .fetch_optional(self.r())
                .await?,
        )
    }

    /// Keep a good download in place of the previous index, regardless of URL.
    pub async fn put_acp_registry_index(&self, url: &str, document: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO acp_registry_index (id, url, document, fetched_at)
             VALUES (1, ?, ?, ?)
             ON CONFLICT (id) DO UPDATE SET url = excluded.url,
                                           document = excluded.document,
                                           fetched_at = excluded.fetched_at",
        )
        .bind(url)
        .bind(document)
        .bind(now())
        .execute(self.w())
        .await?;
        Ok(())
    }
}
