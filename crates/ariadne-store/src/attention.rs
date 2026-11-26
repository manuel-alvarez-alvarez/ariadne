//! Dismissed attention occurrences, independent of requests and work.
use crate::{Result, Store};
use std::collections::HashMap;

impl Store {
    /// Observe one producer's eligibility evidence, retaining its revision across restarts.
    pub async fn observe_attention(&self, id: &str, state: &str) -> Result<i64> {
        Ok(sqlx::query_scalar(
            "INSERT INTO attention_observations (item_id, state) VALUES (?, ?)
            ON CONFLICT(item_id) DO UPDATE SET
            version = version + (state IS NOT excluded.state), state = excluded.state
            RETURNING version",
        )
        .bind(id)
        .bind(state)
        .fetch_one(&self.write)
        .await?)
    }

    /// Revisions of causes, maintained transactionally by source mutations.
    pub async fn attention_source_versions(&self) -> Result<HashMap<(String, String), i64>> {
        let rows: Vec<(String, String, i64)> =
            sqlx::query_as("SELECT kind, source_id, version FROM attention_source_versions")
                .fetch_all(&self.read)
                .await?;
        Ok(rows
            .into_iter()
            .map(|(kind, id, version)| ((kind, id), version))
            .collect())
    }

    /// Serialize attention reads and dismissals across daemon clients.
    pub async fn lock_attention(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.attention_order.lock().await
    }

    pub async fn attention_dismissals(&self) -> Result<HashMap<String, String>> {
        Ok(sqlx::query_as::<_, (String, String)>(
            "SELECT item_id, occurrence FROM attention_dismissals",
        )
        .fetch_all(&self.read)
        .await?
        .into_iter()
        .collect())
    }

    pub async fn dismiss_attention(&self, id: &str, occurrence: &str) -> Result<()> {
        sqlx::query("INSERT INTO attention_dismissals (item_id, occurrence) VALUES (?, ?) ON CONFLICT(item_id) DO UPDATE SET occurrence = excluded.occurrence")
            .bind(id).bind(occurrence).execute(&self.write).await?;
        self.publish(crate::Change::AttentionChanged);
        Ok(())
    }
}
