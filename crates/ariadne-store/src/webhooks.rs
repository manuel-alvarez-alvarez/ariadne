//! Additive hook metadata. Secrets never enter a public DTO.
use crate::{Change, ForgeIntegration, Result, Store, now};

impl Store {
    /// Write hook fields only if the integration still names the same enabled state and forge.
    /// A delivery timestamp has its own write, so reconciliation never overwrites it.
    pub async fn set_webhook(&self, row: &ForgeIntegration) -> Result<bool> {
        let changed = sqlx::query(
            "UPDATE forge_integrations SET webhook_id = ?, webhook_secret = ?, webhook_url = ?,
             webhook_state = ?, webhook_error = ?, updated_at = ?
             WHERE repository_id = ? AND kind = ? AND host = ? AND owner = ? AND name = ? AND enabled = ?
             AND (webhook_id IS NOT ? OR webhook_secret IS NOT ? OR webhook_url IS NOT ?
                  OR webhook_state IS NOT ? OR webhook_error IS NOT ?)",
        )
        .bind(row.webhook_id).bind(&row.webhook_secret).bind(&row.webhook_url)
        .bind(&row.webhook_state).bind(&row.webhook_error).bind(now())
        .bind(&row.repository_id).bind(&row.kind).bind(&row.host).bind(&row.owner).bind(&row.name).bind(row.enabled)
        .bind(row.webhook_id).bind(&row.webhook_secret).bind(&row.webhook_url)
        .bind(&row.webhook_state).bind(&row.webhook_error)
        .execute(self.w()).await?.rows_affected() > 0;
        if changed {
            self.publish(Change::RepositoryUpdated(
                self.get_repository(&row.repository_id).await?,
            ));
        }
        Ok(changed)
    }

    /// Record an authenticated delivery only while its integration and secret remain active.
    pub async fn webhook_delivered(&self, id: &str, secret: &str) -> Result<bool> {
        let changed = sqlx::query("UPDATE forge_integrations SET webhook_last_delivery_at = ?, updated_at = ? WHERE repository_id = ? AND enabled = 1 AND webhook_secret = ?")
            .bind(now()).bind(now()).bind(id).bind(secret).execute(self.w()).await?.rows_affected() > 0;
        if changed {
            self.publish(Change::RepositoryUpdated(self.get_repository(id).await?));
        }
        Ok(changed)
    }
}
