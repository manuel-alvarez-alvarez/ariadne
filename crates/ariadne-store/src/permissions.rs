//! Learned ACP permission approvals, scoped to one repository.

use crate::{Result, Store, now};

impl Store {
    /// Whether this repository has an approval for this ACP tool signature.
    pub async fn has_learned_permission(
        &self,
        repository_id: &str,
        tool_name: &str,
        kind: &str,
    ) -> Result<bool> {
        let found: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM learned_permissions
              WHERE repository_id = ? AND tool_name = ? AND kind = ?",
        )
        .bind(repository_id)
        .bind(tool_name)
        .bind(kind)
        .fetch_optional(self.r())
        .await?;
        Ok(found.is_some())
    }

    /// Remember an approval. Repeating the same approval is harmless.
    pub async fn learn_permission(
        &self,
        repository_id: &str,
        tool_name: &str,
        kind: &str,
    ) -> Result<()> {
        sqlx::query(
            "INSERT OR IGNORE INTO learned_permissions
                (repository_id, tool_name, kind, created_at)
             VALUES (?, ?, ?, ?)",
        )
        .bind(repository_id)
        .bind(tool_name)
        .bind(kind)
        .bind(now())
        .execute(self.w())
        .await?;
        Ok(())
    }
}
