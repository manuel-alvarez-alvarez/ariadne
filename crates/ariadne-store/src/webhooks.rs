//! Additive hook metadata. Secrets never enter a public DTO.
use crate::{Change, ForgeIntegration, ForgeSettings, Result, Store, not_found, now};

impl Store {
    /// The forge settings row. The migration seeds it, so every database has one.
    pub async fn forge_settings(&self) -> Result<ForgeSettings> {
        sqlx::query_as(
            "SELECT tunnel_enabled, tunnel_subdomain, updated_at FROM forge_settings WHERE id = 1",
        )
        .fetch_optional(self.r())
        .await?
        .ok_or_else(|| not_found("forge settings", "1"))
    }

    /// Turn the tunnel on or off, and answer the row as it now stands.
    pub async fn set_tunnel_enabled(&self, enabled: bool) -> Result<ForgeSettings> {
        sqlx::query("UPDATE forge_settings SET tunnel_enabled = ?, updated_at = ? WHERE id = 1")
            .bind(enabled)
            .bind(now())
            .execute(self.w())
            .await?;
        self.forge_settings().await
    }

    /// Keep the subdomain the first tunnel picked. A stored one stays.
    pub async fn keep_tunnel_subdomain(&self, subdomain: &str) -> Result<ForgeSettings> {
        sqlx::query(
            "UPDATE forge_settings SET tunnel_subdomain = ?, updated_at = ?
             WHERE id = 1 AND tunnel_subdomain IS NULL",
        )
        .bind(subdomain)
        .bind(now())
        .execute(self.w())
        .await?;
        self.forge_settings().await
    }

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

    /// Record how the last fetch of an enabled integration's requests went:
    /// the error, or None for a fetch that worked. Only a change is written
    /// and published, so a timer fetch that keeps working is silent.
    pub async fn set_forge_fetch_error(&self, id: &str, error: Option<&str>) -> Result<bool> {
        let changed = sqlx::query(
            "UPDATE forge_integrations SET fetch_error = ?, updated_at = ?
             WHERE repository_id = ? AND enabled = 1 AND fetch_error IS NOT ?",
        )
        .bind(error)
        .bind(now())
        .bind(id)
        .bind(error)
        .execute(self.w())
        .await?
        .rows_affected()
            > 0;
        if changed {
            self.publish(Change::RepositoryUpdated(self.get_repository(id).await?));
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
