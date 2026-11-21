//! Durable host connections and local colour overrides.
use crate::{Result, Store};

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct StoredHost {
    pub name: String,
    pub endpoint: Option<String>,
    pub remote: Option<String>,
    pub color: Option<String>,
}

impl Store {
    pub async fn list_hosts(&self) -> Result<Vec<StoredHost>> {
        Ok(
            sqlx::query_as("SELECT name, endpoint, remote, color FROM hosts ORDER BY rowid")
                .fetch_all(self.r())
                .await?,
        )
    }

    pub async fn save_host(
        &self,
        name: &str,
        endpoint: Option<&str>,
        remote: Option<&str>,
        color: Option<&str>,
    ) -> Result<()> {
        sqlx::query("INSERT INTO hosts(name, endpoint, remote, color) VALUES(?,?,?,?) ON CONFLICT(name) DO UPDATE SET endpoint=excluded.endpoint, remote=excluded.remote, color=excluded.color")
            .bind(name).bind(endpoint).bind(remote).bind(color).execute(self.w()).await?;
        Ok(())
    }

    pub async fn delete_host(&self, name: &str) -> Result<()> {
        sqlx::query("DELETE FROM hosts WHERE name=?")
            .bind(name)
            .execute(self.w())
            .await?;
        Ok(())
    }
}
