//! Reconcile one integration against the public URL before its poll worker starts.
use super::{Cli, ForgeClient};
use crate::config::Config;
use crate::webhooks::Public;
use ariadne_store::{ForgeIntegration, Store};

pub(crate) const GITHUB_EVENTS: &[&str] = &[
    "pull_request",
    "pull_request_review",
    "pull_request_review_comment",
    "issue_comment",
    "check_suite",
    "check_run",
    "status",
    "issues",
];

/// The CLI's error without arguments: hook arguments can contain a secret.
///
/// `body` goes to the CLI on standard input rather than as an argument, so a
/// secret inside it never shows up in the process list.
pub(super) async fn request(
    cli: &Cli,
    args: &[String],
    body: Option<&str>,
    secret: Option<&str>,
) -> Result<String, String> {
    let refs: Vec<_> = args.iter().map(String::as_str).collect();
    let result = match cli.run(&refs, body).await {
        Ok(output) if output.status.success() => {
            Ok(String::from_utf8_lossy(&output.stdout).trim().into())
        }
        Ok(output) => {
            let text = if output.stderr.is_empty() {
                &output.stdout
            } else {
                &output.stderr
            };
            let text = String::from_utf8_lossy(text).trim().to_string();
            Err(if text.is_empty() {
                format!("forge hook request failed: {}", output.status)
            } else {
                text
            })
        }
        Err(error) => Err(error),
    };
    result.map_err(|error| match secret {
        Some(secret) => error.replace(secret, "[redacted]"),
        None => error,
    })
}
pub(super) fn hook_id(answer: &str) -> Result<i64, String> {
    serde_json::from_str::<serde_json::Value>(answer)
        .ok()
        .and_then(|value| value["id"].as_i64())
        .filter(|id| *id > 0)
        .ok_or_else(|| "the forge returned no hook id".into())
}
pub(super) fn missing(error: &str) -> bool {
    error.contains("HTTP 404") || error.contains("404 Not Found")
}
/// Whether a refused hook create is a refusal for rights. GitHub answers 404
/// to an account without admin rights, and `gh` adds a hint where its token
/// lacks the `admin:repo_hook` scope. Other answers name a scope the token
/// lacks, or the admin rights. A scope the request itself gets wrong, such as
/// "invalid scope", is no rights refusal.
pub(super) fn rights_refusal(error: &str) -> bool {
    let error = error.to_lowercase();
    let missing_scope = error.contains("required scope")
        || error.contains("missing scope")
        || (error.contains("needs the") && error.contains("scope"));
    missing_scope
        || [
            "http 404",
            "404 not found",
            "admin:repo_hook",
            "admin rights",
        ]
        .iter()
        .any(|word| error.contains(word))
}

impl ForgeClient {
    async fn create_hook(
        &self,
        row: &ForgeIntegration,
        url: &str,
        secret: &str,
    ) -> Result<i64, String> {
        match self {
            Self::Github(client) => client.create_hook(row, url, secret, GITHUB_EVENTS).await,
        }
    }
    async fn update_hook_url(
        &self,
        row: &ForgeIntegration,
        id: i64,
        url: &str,
    ) -> Result<(), String> {
        match self {
            Self::Github(client) => client.update_hook_url(row, id, url).await,
        }
    }
    async fn delete_hook(&self, row: &ForgeIntegration, id: i64) -> Result<(), String> {
        match self {
            Self::Github(client) => client.delete_hook(row, id).await,
        }
    }
    async fn get_hook(&self, row: &ForgeIntegration, id: i64) -> Result<(), String> {
        match self {
            Self::Github(client) => client.get_hook(row, id).await,
        }
    }
}

pub(crate) async fn reconcile(
    store: &Store,
    cfg: &Config,
    mut row: ForgeIntegration,
    public: &Public,
) -> Result<ForgeIntegration, String> {
    let client = ForgeClient::for_repository(cfg, &row);
    let desired = public
        .hook(&row.kind, &row.repository_id)
        .filter(|_| row.enabled && row.webhook_enabled);
    row.webhook_error = None;
    row.webhook_rights_refused = false;
    if let Some(url) = desired {
        if row.webhook_secret.is_none() {
            let mut bytes = [0; 32];
            getrandom::fill(&mut bytes).map_err(|e| e.to_string())?;
            row.webhook_secret = Some(hex::encode(bytes));
            // A forge may send its ping before the create call returns.
            store.set_webhook(&row).await.map_err(|e| e.to_string())?;
        }
        let secret = row.webhook_secret.as_deref().unwrap();
        let creating = row.webhook_id.is_none();
        let result = match row.webhook_id {
            None => client.create_hook(&row, &url, secret).await.map(|id| {
                row.webhook_id = Some(id);
            }),
            Some(id) if row.webhook_url.as_deref() != Some(&url) => {
                client.update_hook_url(&row, id, &url).await
            }
            Some(id) => client.get_hook(&row, id).await,
        };
        match result {
            Ok(()) => {
                row.webhook_state = "live".into();
                row.webhook_url = Some(url);
            }
            Err(error) => {
                if missing(&error) {
                    row.webhook_id = None;
                    row.webhook_url = None;
                }
                row.webhook_state = if row.webhook_id.is_none() || missing(&error) {
                    "polling"
                } else {
                    "failed"
                }
                .into();
                row.webhook_rights_refused = creating && rights_refusal(&error);
                row.webhook_error = Some(error.replace(secret, "[redacted]"));
            }
        }
    } else {
        row.webhook_state = "polling".into();
        if row.enabled && row.webhook_enabled {
            // Keep the registered ID for a later URL, but expose no current public address.
            row.webhook_url = None;
            row.webhook_error = public.why.clone();
        } else if let Some(id) = row.webhook_id {
            match client.delete_hook(&row, id).await {
                Ok(()) => {
                    row.webhook_id = None;
                    row.webhook_url = None;
                }
                Err(error) if missing(&error) => {
                    row.webhook_id = None;
                    row.webhook_url = None;
                }
                Err(error) => {
                    row.webhook_state = "failed".into();
                    row.webhook_error = Some(error);
                }
            }
        }
    }
    store.set_webhook(&row).await.map_err(|e| e.to_string())?;
    // A disable during a forge call must not reactivate the poll or leave a new hook behind.
    let current = store
        .forge_integration(&row.repository_id)
        .await
        .map_err(|e| e.to_string())?;
    if current.as_ref().is_none_or(|r| {
        r.enabled != row.enabled
            || r.webhook_enabled != row.webhook_enabled
            || r.kind != row.kind
            || r.host != row.host
            || r.owner != row.owner
            || r.name != row.name
    }) {
        if let Some(id) = row.webhook_id {
            client.delete_hook(&row, id).await?;
        }
        return Err("integration changed during hook reconciliation".into());
    }
    Ok(current.unwrap())
}

/// Remove a replaced integration's hook using its original forge coordinates.
/// The committed new row has already cleared its old secret and hook ID.
pub(crate) async fn remove_replaced(
    cfg: &Config,
    previous: Option<&ForgeIntegration>,
    current: Option<&ForgeIntegration>,
) {
    let Some(previous) = previous else {
        return;
    };
    if current.is_some_and(|row| {
        row.kind == previous.kind
            && row.host == previous.host
            && row.owner == previous.owner
            && row.name == previous.name
    }) {
        return;
    }
    if let Some(id) = previous.webhook_id
        && let Err(error) = ForgeClient::for_repository(cfg, previous)
            .delete_hook(previous, id)
            .await
        && !missing(&error)
    {
        tracing::warn!(%error, repository = previous.repository_id, "cannot delete the replaced forge hook");
    }
}

#[cfg(test)]
mod tests {
    use super::rights_refusal;

    #[test]
    fn a_create_refusal_for_rights_names_404_the_hook_scope_or_admin_rights() {
        for error in [
            "gh: Not Found (HTTP 404)\nThis API operation needs the \"admin:repo_hook\" scope.",
            "HTTP 404: Not Found",
            "404 Not Found",
            "HTTP 403: Must have admin rights to Repository.",
            "your token has not been granted the required scopes",
            "HTTP 403: missing scope repo",
            "This API operation needs the \"repo\" scope.",
        ] {
            assert!(rights_refusal(error), "{error}");
        }
        for error in [
            "HTTP 422: Validation Failed",
            "HTTP 422: invalid scope for this URL",
            "HTTP 502: Bad Gateway",
            "the forge returned no hook id",
        ] {
            assert!(!rights_refusal(error), "{error}");
        }
    }
}
