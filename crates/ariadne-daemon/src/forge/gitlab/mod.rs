//! GitLab, through `glab`.

mod open;

use super::Cli;
mod issues;

/// `glab`, as the config names it.
#[derive(Debug, Clone)]
pub struct Gitlab {
    pub(super) cli: Cli,
}

impl Gitlab {
    pub(super) fn new(cli: Cli) -> Gitlab {
        Gitlab { cli }
    }

    /// `glab auth status --hostname <host>`: signed in to `host`, or
    /// `glab`'s own words on why not.
    pub async fn auth_status(&self, host: &str) -> Result<(), String> {
        self.cli
            .answer(&["auth", "status", "--hostname", host])
            .await
            .map(drop)
    }

    /// `glab api user --hostname <host>`, read for its `username`: the
    /// account `glab` is signed in as.
    pub async fn whoami(&self, host: &str) -> Result<String, String> {
        let user = self
            .cli
            .answer(&["api", "user", "--hostname", host])
            .await?;
        serde_json::from_str::<serde_json::Value>(&user)
            .ok()
            .and_then(|user| user["username"].as_str().map(str::to_string))
            .filter(|login| !login.is_empty())
            .ok_or_else(|| format!("`glab api user` named no username on {host}"))
    }
}
