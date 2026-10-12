//! GitHub, through `gh`.

mod comments;
mod details;
mod hooks;
mod open;
mod pulls;
mod reviews;

use super::Cli;
mod issues;

/// `gh`, as the config names it.
#[derive(Debug, Clone)]
pub struct Github {
    pub(super) cli: Cli,
}

impl Github {
    pub(super) fn new(cli: Cli) -> Github {
        Github { cli }
    }

    /// `gh auth status --hostname <host>`: signed in to `host`, or `gh`'s
    /// own words on why not.
    pub async fn auth_status(&self, host: &str) -> Result<(), String> {
        self.cli
            .answer(&["auth", "status", "--hostname", host])
            .await
            .map(drop)
    }

    /// `gh api user --jq .login --hostname <host>`: the login `gh` is signed
    /// in as.
    pub async fn whoami(&self, host: &str) -> Result<String, String> {
        let login = self
            .cli
            .answer(&["api", "user", "--jq", ".login", "--hostname", host])
            .await?;
        match login.is_empty() {
            true => Err(format!("`gh api user` named no login on {host}")),
            false => Ok(login),
        }
    }

    /// `gh api rate_limit --hostname <host>`, which spends none of the
    /// limit itself: the later of the `core` (REST) and `graphql` reset
    /// times, since one repository fetch can draw on both. `None` where
    /// the call failed or either reset could not be read.
    pub(crate) async fn rate_limit_reset(
        &self,
        host: &str,
    ) -> Option<chrono::DateTime<chrono::Utc>> {
        let answer = self
            .cli
            .answer(&["api", "rate_limit", "--hostname", host])
            .await
            .ok()?;
        let body: serde_json::Value = serde_json::from_str(&answer).ok()?;
        let resources = body.get("resources")?;
        let reset_of = |key: &str| -> Option<i64> { resources.get(key)?.get("reset")?.as_i64() };
        let reset = reset_of("core")?.max(reset_of("graphql")?);
        chrono::DateTime::from_timestamp(reset, 0)
    }
}
