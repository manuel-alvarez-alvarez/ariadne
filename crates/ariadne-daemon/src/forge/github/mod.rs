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
    /// limit itself: the latest reset among exhausted `core` (REST) and
    /// `graphql` resources. `None` where the call failed, no resource is
    /// exhausted, or an exhausted resource has no reset time.
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
        spent_rate_limit_reset(resources)
    }
}

/// The latest reset among primary GitHub resources that have no calls left.
/// A resource with calls left has a reset time too, but cannot explain a
/// primary-limit refusal.
fn spent_rate_limit_reset(resources: &serde_json::Value) -> Option<chrono::DateTime<chrono::Utc>> {
    let reset_of = |key: &str| -> Option<i64> {
        let resource = resources.get(key)?;
        (resource.get("remaining")?.as_i64()? == 0)
            .then(|| resource.get("reset")?.as_i64())
            .flatten()
    };
    let reset = [reset_of("core"), reset_of("graphql")]
        .into_iter()
        .flatten()
        .max()?;
    chrono::DateTime::from_timestamp(reset, 0)
}

#[cfg(test)]
mod tests {
    use super::spent_rate_limit_reset;
    use serde_json::json;

    /// Only an exhausted resource's reset can explain a primary rate limit.
    #[test]
    fn a_rate_limit_reset_uses_the_only_spent_resource() {
        let core = 4_102_444_800_i64;
        let graphql = core + 1_800;
        let resources = json!({
            "core": {"remaining": 0, "reset": core},
            "graphql": {"remaining": 4_000, "reset": graphql},
        });

        assert_eq!(
            spent_rate_limit_reset(&resources),
            chrono::DateTime::from_timestamp(core, 0)
        );
    }

    /// A secondary limit leaves primary resources unspent, so no primary
    /// reset can name when it ends.
    #[test]
    fn a_rate_limit_reset_is_none_when_no_resource_is_spent() {
        let resources = json!({
            "core": {"remaining": 4_000, "reset": 4_102_444_800_i64},
            "graphql": {"remaining": 4_000, "reset": 4_102_446_600_i64},
        });

        assert_eq!(spent_rate_limit_reset(&resources), None);
    }
}
