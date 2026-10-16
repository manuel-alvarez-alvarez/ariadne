//! GitHub repository hooks through `gh api`.
use super::Github;
use crate::forge::hooks::{hook_id, request};
use ariadne_store::ForgeIntegration;

fn args(row: &ForgeIntegration, suffix: &str, method: &str) -> Vec<String> {
    vec![
        "api".into(),
        format!("repos/{}/{}/hooks{suffix}", row.owner, row.name),
        "--hostname".into(),
        row.host.clone(),
        "--method".into(),
        method.into(),
    ]
}
impl Github {
    pub(crate) async fn create_hook(
        &self,
        row: &ForgeIntegration,
        url: &str,
        secret: &str,
        events: &[&str],
    ) -> Result<i64, String> {
        let mut args = args(row, "", "POST");
        args.extend(["--input".into(), "-".into()]);
        let body = serde_json::json!({
            "name": "web",
            "config": {
                "url": url,
                "content_type": "json",
                "secret": secret,
                "insecure_ssl": "0",
            },
            "active": true,
            "events": events,
        })
        .to_string();
        hook_id(&request(&self.cli, &args, Some(&body), Some(secret)).await?)
    }
    pub(crate) async fn update_hook_url(
        &self,
        row: &ForgeIntegration,
        id: i64,
        url: &str,
    ) -> Result<(), String> {
        // The config endpoint changes only URL and preserves the existing secret.
        let mut args = args(row, &format!("/{id}/config"), "PATCH");
        args.extend(["--input".into(), "-".into()]);
        let body = serde_json::json!({ "url": url }).to_string();
        request(&self.cli, &args, Some(&body), row.webhook_secret.as_deref())
            .await
            .map(drop)
    }
    pub(crate) async fn delete_hook(&self, row: &ForgeIntegration, id: i64) -> Result<(), String> {
        request(
            &self.cli,
            &args(row, &format!("/{id}"), "DELETE"),
            None,
            row.webhook_secret.as_deref(),
        )
        .await
        .map(drop)
    }
    pub(crate) async fn get_hook(&self, row: &ForgeIntegration, id: i64) -> Result<(), String> {
        request(
            &self.cli,
            &args(row, &format!("/{id}"), "GET"),
            None,
            row.webhook_secret.as_deref(),
        )
        .await
        .map(drop)
    }
}
