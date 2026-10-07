//! GitLab project hooks through `glab api`.
use super::Gitlab;
use crate::forge::hooks::{hook_id, request};
use ariadne_store::ForgeIntegration;

fn args(row: &ForgeIntegration, suffix: &str, method: &str) -> Vec<String> {
    let mut url = reqwest::Url::parse("https://gitlab.invalid").unwrap();
    url.path_segments_mut()
        .unwrap()
        .push(&format!("{}/{}", row.owner, row.name));
    vec![
        "api".into(),
        format!(
            "projects/{}/hooks{suffix}",
            url.path().trim_start_matches('/')
        ),
        "--hostname".into(),
        row.host.clone(),
        "--method".into(),
        method.into(),
    ]
}
impl Gitlab {
    pub(crate) async fn create_hook(
        &self,
        row: &ForgeIntegration,
        url: &str,
        secret: &str,
        events: &[&str],
    ) -> Result<i64, String> {
        let mut args = args(row, "", "POST");
        args.extend([
            "-f".into(),
            format!("url={url}"),
            "-f".into(),
            format!("token={secret}"),
            "-F".into(),
            "enable_ssl_verification=true".into(),
        ]);
        for event in events {
            args.extend(["-F".into(), format!("{event}=true")]);
        }
        hook_id(&request(&self.cli, &args, Some(secret)).await?)
    }
    pub(crate) async fn update_hook_url(
        &self,
        row: &ForgeIntegration,
        id: i64,
        url: &str,
    ) -> Result<(), String> {
        let mut args = args(row, &format!("/{id}"), "PUT");
        args.extend(["-f".into(), format!("url={url}")]);
        // GitLab resets the token when the URL changes, so supply it again.
        if let Some(secret) = &row.webhook_secret {
            args.extend(["-f".into(), format!("token={secret}")]);
        }
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
