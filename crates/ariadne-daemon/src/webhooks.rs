//! The public ingress has a separate port and only two authenticated routes (027).
use crate::{config::Config, forge::poll::ForgePoll};
use ariadne_store::Store;
use axum::{
    Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, StatusCode},
    routing::post,
};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::{net::SocketAddr, sync::Arc};
use subtle::ConstantTimeEq;
use tokio::{net::TcpListener, sync::watch};

/// The tunnel writes the current URL, or None when public ingress is unavailable.
#[derive(Clone)]
pub struct WebhookUrl(watch::Sender<Option<String>>);
impl WebhookUrl {
    pub(crate) fn new(url: Option<String>) -> Self {
        Self(watch::channel(url).0)
    }
    pub fn set(&self, url: Option<String>) {
        self.0.send_replace(url);
    }
    pub(crate) fn subscribe(&self) -> watch::Receiver<Option<String>> {
        self.0.subscribe()
    }
}

/// Keep this handle alive for as long as the ingress should serve requests.
#[derive(Clone)]
pub struct WebhookListen(Arc<Listener>);
struct Listener {
    address: SocketAddr,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Listener {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl WebhookListen {
    pub async fn bind(cfg: &Config, store: Store, poll: ForgePoll) -> std::io::Result<Self> {
        let listener = TcpListener::bind(
            cfg.webhook_listen
                .unwrap_or_else(|| SocketAddr::from(([127, 0, 0, 1], 0))),
        )
        .await?;
        let address = listener.local_addr()?;
        tracing::info!(%address, "webhook listener enabled");
        let router = Router::new()
            .route("/webhooks/github/{repository_id}", post(github))
            .route("/webhooks/gitlab/{repository_id}", post(gitlab))
            .layer(DefaultBodyLimit::max(1024 * 1024))
            .with_state(Ingress { store, poll });
        let task = tokio::spawn(async move {
            if let Err(error) = axum::serve(listener, router).await {
                tracing::error!(%error, "webhook listener stopped");
            }
        });
        Ok(Self(Arc::new(Listener { address, task })))
    }
    pub fn address(&self) -> SocketAddr {
        self.0.address
    }
}
#[derive(Clone)]
struct Ingress {
    store: Store,
    poll: ForgePoll,
}
async fn github(
    State(state): State<Ingress>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> StatusCode {
    deliver(state, id, headers, body, "github").await
}
async fn gitlab(
    State(state): State<Ingress>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> StatusCode {
    deliver(state, id, headers, body, "gitlab").await
}
async fn deliver(
    state: Ingress,
    id: String,
    headers: HeaderMap,
    body: Bytes,
    kind: &str,
) -> StatusCode {
    let row = match state.store.forge_integration(&id).await {
        Ok(Some(row)) if row.enabled && row.kind == kind => row,
        Ok(_) => return StatusCode::UNAUTHORIZED,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR,
    };
    let Some(secret) = row.webhook_secret.as_deref() else {
        return StatusCode::UNAUTHORIZED;
    };
    let valid = if kind == "github" {
        headers
            .get("X-Hub-Signature-256")
            .and_then(|h| h.to_str().ok())
            .and_then(|s| s.strip_prefix("sha256="))
            .and_then(|s| hex::decode(s).ok())
            .is_some_and(|signature| {
                let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
                    .expect("HMAC accepts every key size");
                mac.update(&body);
                mac.verify_slice(&signature).is_ok()
            })
    } else {
        headers
            .get("X-Gitlab-Token")
            .is_some_and(|token| bool::from(token.as_bytes().ct_eq(secret.as_bytes())))
    };
    if !valid {
        return StatusCode::UNAUTHORIZED;
    }
    match state.store.webhook_delivered(&id, secret).await {
        Ok(true) => {}
        Ok(false) => return StatusCode::UNAUTHORIZED,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR,
    }
    let event = if kind == "github" {
        "X-Github-Event"
    } else {
        "X-Gitlab-Event"
    };
    if headers.get(event).is_none_or(|value| value != "ping") {
        state.poll.wake(&id);
    }
    StatusCode::ACCEPTED
}
