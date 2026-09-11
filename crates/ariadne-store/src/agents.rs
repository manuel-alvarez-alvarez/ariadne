//! Agent configuration: the flags each registry agent is launched with.
//!
//! Keyed by the id of the agent in the ACP registry. Nothing is seeded: an
//! agent nobody has set flags for is launched with its registry command
//! alone, and a row appears the first time somebody sets some. Every spawn
//! and resume reads its flags here, so an edit takes effect on the next
//! launch of any session on that agent.

use crate::{AgentConfig, Result, Store, StoreError, now};

impl Store {
    /// Every stored agent config, by agent id.
    pub async fn list_agent_configs(&self) -> Result<Vec<AgentConfig>> {
        Ok(
            sqlx::query_as::<_, AgentConfig>("SELECT * FROM agent_configs ORDER BY agent_id")
                .fetch_all(self.r())
                .await?,
        )
    }

    /// The flags `agent_id` is launched with: the stored list, or none where
    /// nobody has set any.
    pub async fn agent_flags(&self, agent_id: &str) -> Result<Vec<String>> {
        let row =
            sqlx::query_as::<_, AgentConfig>("SELECT * FROM agent_configs WHERE agent_id = ?")
                .bind(agent_id)
                .fetch_optional(self.r())
                .await?;
        Ok(row.map(|config| config.extra_flags()).unwrap_or_default())
    }

    /// Replace an agent's flag list, whole. An empty one is a legitimate
    /// answer: "launch this agent with nothing of ours".
    pub async fn update_agent_config(
        &self,
        agent_id: &str,
        extra_flags: Vec<String>,
    ) -> Result<AgentConfig> {
        let flags = flags_json(&extra_flags)?;
        sqlx::query(
            "INSERT INTO agent_configs (agent_id, extra_flags, updated_at) VALUES (?, ?, ?)
             ON CONFLICT (agent_id) DO UPDATE SET extra_flags = excluded.extra_flags,
                                                 updated_at = excluded.updated_at",
        )
        .bind(agent_id)
        .bind(&flags)
        .bind(now())
        .execute(self.w())
        .await?;
        self.fetch_by("agent config", "agent_configs", "agent_id", agent_id)
            .await
    }
}

/// The stored spelling of a flag list: a JSON array of argv strings.
fn flags_json(flags: &[impl AsRef<str>]) -> Result<String> {
    let flags: Vec<&str> = flags.iter().map(AsRef::as_ref).collect();
    serde_json::to_string(&flags).map_err(|e| StoreError::Invalid(e.to_string()))
}
