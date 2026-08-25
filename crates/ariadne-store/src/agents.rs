//! Agent-kind configuration: how each coding-agent CLI is launched.
//!
//! One row per [`AgentKind`], seeded from the built-in defaults and edited
//! from there. Every spawn and resume reads its flags here, so turning a
//! permission bypass off takes effect on the next launch of any profile
//! running on that agent.

use ariadne_core::AgentKind;

use crate::{AgentConfig, Result, Store, StoreError, now};

impl Store {
    /// Give every agent kind a config row, with the flags
    /// [`AgentKind::default_flags`] ships. Runs on every open, so a kind added
    /// to the enum later is seeded on the next start rather than in a
    /// migration; existing rows are left as the user edited them.
    pub(crate) async fn seed_agent_configs(&self) -> Result<()> {
        let ts = now();
        for kind in AgentKind::ALL {
            let defaults: Vec<&str> = kind.default_flags().to_vec();
            let flags = flags_json(&defaults)?;
            sqlx::query(
                "INSERT OR IGNORE INTO agent_configs (agent_kind, extra_flags, updated_at)
                 VALUES (?, ?, ?)",
            )
            .bind(kind.as_str())
            .bind(&flags)
            .bind(&ts)
            .execute(self.w())
            .await?;
        }
        Ok(())
    }

    /// Every agent kind's config, in [`AgentKind::ALL`] order.
    pub async fn list_agent_configs(&self) -> Result<Vec<AgentConfig>> {
        let mut configs = Vec::with_capacity(AgentKind::ALL.len());
        for kind in AgentKind::ALL {
            configs.push(self.get_agent_config(kind).await?);
        }
        Ok(configs)
    }

    pub async fn get_agent_config(&self, kind: AgentKind) -> Result<AgentConfig> {
        self.fetch_by("agent config", "agent_configs", "agent_kind", kind.as_str())
            .await
    }

    /// Replace an agent kind's flag list, whole. An empty one is a legitimate
    /// answer: "launch this CLI with nothing of ours".
    pub async fn update_agent_config(
        &self,
        kind: AgentKind,
        extra_flags: Vec<String>,
    ) -> Result<AgentConfig> {
        self.get_agent_config(kind).await?;
        let flags = flags_json(&extra_flags)?;
        sqlx::query(
            "UPDATE agent_configs SET extra_flags = ?, updated_at = ? WHERE agent_kind = ?",
        )
        .bind(&flags)
        .bind(now())
        .bind(kind.as_str())
        .execute(self.w())
        .await?;
        self.get_agent_config(kind).await
    }
}

/// The stored spelling of a flag list: a JSON array of argv strings.
fn flags_json(flags: &[impl AsRef<str>]) -> Result<String> {
    let flags: Vec<&str> = flags.iter().map(AsRef::as_ref).collect();
    serde_json::to_string(&flags).map_err(|e| StoreError::Invalid(e.to_string()))
}
