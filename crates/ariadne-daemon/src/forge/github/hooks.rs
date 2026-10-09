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
        for field in [
            "name=web".into(),
            format!("config[url]={url}"),
            "config[content_type]=json".into(),
            format!("config[secret]={secret}"),
            "config[insecure_ssl]=0".into(),
        ] {
            args.extend(["-f".into(), field]);
        }
        args.extend(["-F".into(), "active=true".into()]);
        for event in events {
            args.extend(["-f".into(), format!("events[]={event}")]);
        }
        hook_id(&request(&self.cli, &args, Some(secret)).await?)
    }
    pub(crate) async fn update_hook_url(
        &self,
        row: &ForgeIntegration,
        id: i64,
        url: &str,
    ) -> Result<(), String> {
        // The config endpoint changes only URL and preserves the existing secret.
        let mut args = args(row, &format!("/{id}/config"), "PATCH");
        args.extend(["-f".into(), format!("url={url}")]);
        request(&self.cli, &args, row.webhook_secret.as_deref())
            .await
            .map(drop)
    }
    pub(crate) async fn delete_hook(&self, row: &ForgeIntegration, id: i64) -> Result<(), String> {
        request(
            &self.cli,
            &args(row, &format!("/{id}"), "DELETE"),
            row.webhook_secret.as_deref(),
        )
        .await
        .map(drop)
    }
    pub(crate) async fn get_hook(&self, row: &ForgeIntegration, id: i64) -> Result<(), String> {
        request(
            &self.cli,
            &args(row, &format!("/{id}"), "GET"),
            row.webhook_secret.as_deref(),
        )
        .await
        .map(drop)
    }
}
