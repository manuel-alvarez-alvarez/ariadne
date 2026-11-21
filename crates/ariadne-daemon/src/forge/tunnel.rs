//! The webhook tunnel (027): a smee.io channel relayed to the bound webhook
//! listener, kept up, and switched on and off at run time.
//!
//! The forge posts each delivery to the channel, and the channel streams it
//! to the tunnel as a server-sent event, which the tunnel posts on to the
//! listener. The tunnel writes the channel URL into [`WebhookUrl`] while it
//! is up and withdraws it while it is down or off. The hook reconcile does
//! the rest: it moves each hook to the URL, and each repository's fetch
//! between `WakeOnly` and `Timer`. The transport is the daemon's alone: a
//! fetch records the same rows whatever woke it.

use std::collections::VecDeque;
use std::error::Error;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use ariadne_api::repositories::{ForgeTunnelDto, TunnelState};
use ariadne_api::stream::DomainEvent;
use ariadne_client::{SseEvent, SseParser};
use ariadne_store::{Store, StoreError};
use chrono::{SecondsFormat, Utc};
use reqwest::StatusCode;
use serde::Deserialize;
use serde_json::value::RawValue;
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tokio::time::{Instant, sleep_until, timeout};
use tracing::{info, warn};

use super::poll::ForgePoll;
use crate::bus::{BusEvent, EventBus};
use crate::config::Config;
use crate::timeouts::Timeouts;
use crate::webhooks::{WebhookListen, WebhookUrl};

/// The longest wait between two reconnects.
const BACKOFF_CAP: Duration = Duration::from_secs(60);

/// The letters of a random channel name.
const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

/// How long a random channel name is. Anyone who knows the channel reads
/// every delivery sent to it, so the name is the only secret it has: 22
/// letters of 62 carry about 130 bits.
const CHANNEL_LENGTH: usize = 22;

/// The daemon's tunnel. Cheap to clone: every clone is the same tunnel.
#[derive(Clone)]
pub struct Tunnel(Arc<Inner>);

struct Inner {
    store: Store,
    cfg: Arc<Config>,
    events: EventBus,
    url: WebhookUrl,
    timeouts: Timeouts,
    status: Mutex<Status>,
    listen: OnceLock<SocketAddr>,
    /// The switch moved: the supervisor reads what it wants again.
    changed: Notify,
    task: Mutex<Option<JoinHandle<()>>>,
    http: reqwest::Client,
    /// Posts to the listener directly: a proxy in the environment carries
    /// the channel's stream, never the loopback delivery (027).
    listener: reqwest::Client,
}

#[derive(Clone, PartialEq)]
struct Status {
    state: TunnelState,
    url: Option<String>,
    since: String,
    error: Option<String>,
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

impl Tunnel {
    /// A tunnel that runs nothing until [`Tunnel::start`] gives it a listener.
    pub fn new(
        store: Store,
        cfg: Arc<Config>,
        events: EventBus,
        poll: &ForgePoll,
        timeouts: Timeouts,
    ) -> Self {
        Self(Arc::new(Inner {
            store,
            cfg,
            events,
            url: poll.webhook_url(),
            timeouts,
            status: Mutex::new(Status {
                state: TunnelState::Off,
                url: None,
                since: now(),
                error: None,
            }),
            listen: OnceLock::new(),
            changed: Notify::new(),
            task: Mutex::new(None),
            http: reqwest::Client::new(),
            listener: reqwest::Client::builder()
                .no_proxy()
                .build()
                .expect("a client with no proxy builds"),
        }))
    }

    /// Supervise the tunnel to `listen`'s bound address until shutdown.
    pub fn start(&self, listen: &WebhookListen) {
        let address = listen.address();
        if self.0.listen.set(address).is_err() {
            return;
        }
        let inner = self.0.clone();
        let task = tokio::spawn(async move { inner.supervise(address).await });
        *self.0.task.lock().unwrap() = Some(task);
    }

    /// Close the tunnel for good: the daemon is stopping.
    pub async fn shutdown(&self) {
        let task = self.0.task.lock().unwrap().take();
        if let Some(task) = task {
            task.abort();
            let _ = task.await;
        }
    }

    /// The settings and the tunnel state, as `GET /v1/forge/tunnel` answers them.
    pub async fn status(&self) -> Result<ForgeTunnelDto, StoreError> {
        self.0.dto().await
    }

    /// Store the switch, and have the supervisor follow it.
    pub async fn set_enabled(&self, enabled: bool) -> Result<ForgeTunnelDto, StoreError> {
        self.0.store.set_tunnel_enabled(enabled).await?;
        self.0.changed.notify_one();
        let dto = self.0.dto().await?;
        self.0.events.forge_settings_updated(dto.clone());
        Ok(dto)
    }
}

impl Inner {
    async fn dto(&self) -> Result<ForgeTunnelDto, StoreError> {
        let settings = self.store.forge_settings().await?;
        let status = self.status.lock().unwrap().clone();
        Ok(ForgeTunnelDto {
            enabled: settings.tunnel_enabled,
            state: status.state,
            url: status.url,
            listen: self.listen.get().map(ToString::to_string),
            since: status.since,
            error: status.error,
        })
    }

    /// Move the status, and publish it where it changed.
    async fn set_status(&self, change: impl FnOnce(&Status) -> Status) {
        let changed = {
            let mut status = self.status.lock().unwrap();
            let next = change(&status);
            let changed = *status != next;
            *status = next;
            changed
        };
        if changed {
            match self.dto().await {
                Ok(dto) => self.events.forge_settings_updated(dto),
                Err(error) => warn!(%error, "cannot read the forge settings"),
            }
        }
    }

    /// Whether a tunnel should run: the switch is on, no public URL is
    /// configured, and at least one integration is enabled.
    async fn wanted(&self) -> bool {
        if self.cfg.webhook_public_url.is_some() {
            return false;
        }
        let read = async {
            Ok::<_, StoreError>(
                self.store.forge_settings().await?.tunnel_enabled
                    && !self.store.enabled_forge_integrations().await?.is_empty(),
            )
        };
        read.await.unwrap_or_else(|error| {
            warn!(%error, "cannot read whether the tunnel should run");
            false
        })
    }

    async fn up(&self, url: String) {
        info!(%url, "webhook tunnel up");
        self.url.set_channel(url.clone());
        self.set_status(|_| Status {
            state: TunnelState::Up,
            url: Some(url),
            since: now(),
            error: None,
        })
        .await;
    }

    async fn down(&self, error: String) {
        warn!(%error, "webhook tunnel down");
        let since = {
            let status = self.status.lock().unwrap();
            match status.state {
                TunnelState::Down => status.since.clone(),
                _ => now(),
            }
        };
        self.url
            .withdraw(Some(format!("tunnel down since {since}")));
        self.set_status(|_| Status {
            state: TunnelState::Down,
            url: None,
            since,
            error: Some(error),
        })
        .await;
    }

    async fn off(&self) {
        // A configured public URL is the URL: the tunnel never touches it.
        if self.cfg.webhook_public_url.is_none() {
            let switched_off = self
                .store
                .forge_settings()
                .await
                .is_ok_and(|settings| !settings.tunnel_enabled);
            self.url
                .withdraw(switched_off.then(|| "tunnel off".to_string()));
        }
        self.set_status(|status| match status.state {
            TunnelState::Off => status.clone(),
            _ => Status {
                state: TunnelState::Off,
                url: None,
                since: now(),
                error: None,
            },
        })
        .await;
    }

    /// Wait for the switch or an integration to move, or for `deadline`.
    /// Answers false at the deadline.
    async fn until_changed(
        &self,
        events: &mut tokio::sync::broadcast::Receiver<BusEvent>,
        deadline: Option<Instant>,
    ) -> bool {
        let deadline = async {
            match deadline {
                Some(at) => sleep_until(at).await,
                None => std::future::pending().await,
            }
        };
        tokio::pin!(deadline);
        loop {
            tokio::select! {
                () = &mut deadline => return false,
                () = self.changed.notified() => return true,
                event = events.recv() => match event {
                    Ok(event) => if matches!(
                        event.event,
                        DomainEvent::RepositoryCreated(_)
                            | DomainEvent::RepositoryUpdated(_)
                            | DomainEvent::RepositoryDeleted(_)
                    ) {
                        return true;
                    },
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => return true,
                    // Only a stopping daemon closes the bus: wait on the rest.
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        *events = self.events.subscribe();
                    }
                },
            }
        }
    }

    async fn supervise(&self, listen: SocketAddr) {
        let mut events = self.events.subscribe();
        let base = self.timeouts.tunnel_retry;
        let mut backoff = base;
        loop {
            if !self.wanted().await {
                self.off().await;
                backoff = base;
                self.until_changed(&mut events, None).await;
                continue;
            }
            // A switch off or a last disabled integration cancels the attempt.
            let attempt = self.open();
            tokio::pin!(attempt);
            let attempted = loop {
                tokio::select! {
                    result = &mut attempt => break Some(result),
                    _ = self.until_changed(&mut events, None) => {
                        if !self.wanted().await {
                            break None;
                        }
                    }
                }
            };
            match attempted {
                None => continue,
                Some(Ok(mut open)) => {
                    // The switch may have turned off while the server answered.
                    if !self.wanted().await {
                        continue;
                    }
                    backoff = base;
                    self.up(open.url.clone()).await;
                    // One silence clock for the whole hold: an unrelated
                    // repository event must not restart it.
                    let lost = self.relay(&mut open, listen);
                    tokio::pin!(lost);
                    let lost = loop {
                        tokio::select! {
                            why = &mut lost => break Some(why),
                            _ = self.until_changed(&mut events, None) => {
                                if !self.wanted().await {
                                    break None;
                                }
                            }
                        }
                    };
                    let Some(why) = lost else {
                        continue;
                    };
                    self.down(why).await;
                }
                Some(Err(error)) => self.down(error).await,
            }
            // Wait out the backoff. Only a switch off or a last disabled
            // integration ends it early.
            let retry_at = Instant::now() + backoff;
            backoff = (backoff * 2).min(BACKOFF_CAP);
            while self.until_changed(&mut events, Some(retry_at)).await {
                if !self.wanted().await {
                    break;
                }
            }
        }
    }

    /// The channel to listen on: the configured one, else the stored one,
    /// else a random one stored now.
    async fn channel(&self) -> Result<String, String> {
        if let Some(channel) = &self.cfg.tunnel_channel {
            return Ok(channel.clone());
        }
        let settings = self
            .store
            .forge_settings()
            .await
            .map_err(|e| e.to_string())?;
        if let Some(channel) = settings.tunnel_channel {
            return Ok(channel);
        }
        let mut bytes = [0u8; CHANNEL_LENGTH];
        getrandom::fill(&mut bytes).map_err(|e| e.to_string())?;
        // 256 is 62 * 4 + 8: the bias toward the first eight letters is
        // under 4 percent, and the name stays far beyond guessing.
        let picked: String = bytes
            .iter()
            .map(|b| char::from(ALPHABET[usize::from(*b) % ALPHABET.len()]))
            .collect();
        let kept = self
            .store
            .keep_tunnel_channel(&picked)
            .await
            .map_err(|e| e.to_string())?;
        Ok(kept.tunnel_channel.unwrap_or(picked))
    }

    /// One connection attempt: open the channel's event stream and wait for
    /// its `ready` event, within `tunnel_connect`.
    async fn open(&self) -> Result<Open, String> {
        let channel = self.channel().await?;
        let url = format!("{}/{channel}", self.cfg.tunnel_host.trim_end_matches('/'));
        let limit = self.timeouts.tunnel_connect;
        timeout(limit, async {
            let response = self
                .http
                .get(&url)
                .header(reqwest::header::ACCEPT, "text/event-stream")
                .send()
                .await
                .map_err(|error| chain(&error))?;
            if !response.status().is_success() {
                return Err(format!("{url} answered {}", response.status()));
            }
            let mut stream = Stream {
                response,
                parser: SseParser::new(),
                pending: VecDeque::new(),
            };
            while stream.next().await?.event != "ready" {}
            Ok(Open {
                url: url.clone(),
                stream,
            })
        })
        .await
        .map_err(|_| format!("no answer from {url} within {limit:?}"))?
    }

    /// Post each delivery the channel streams on to the listener, until the
    /// stream ends or stays silent for `tunnel_idle`. Answers why it ended.
    async fn relay(&self, open: &mut Open, listen: SocketAddr) -> String {
        let idle = self.timeouts.tunnel_idle;
        loop {
            match timeout(idle, open.stream.next()).await {
                Err(_) => return format!("no word from the tunnel server for {idle:?}"),
                Ok(Err(error)) => return error,
                // `ready` and `ping` only prove the stream is alive.
                Ok(Ok(event)) if event.event == "message" => {
                    self.forward(listen, &event.data).await;
                }
                Ok(Ok(_)) => {}
            }
        }
    }

    /// Post one delivery to the listener, as the forge sent it to the channel.
    ///
    /// smee.io parses the body and streams it as JSON, so the bytes posted
    /// here are its serialization, not necessarily the forge's. A forge's
    /// compact JSON usually survives that whole, and its signature holds; a
    /// body that does not, fails the listener's signature check, and that
    /// delivery is lost (027).
    async fn forward(&self, listen: SocketAddr, data: &str) {
        let delivery: Delivery = match serde_json::from_str(data) {
            Ok(delivery) => delivery,
            Err(error) => {
                warn!(%error, "the tunnel dropped a delivery it cannot read");
                return;
            }
        };
        let Some(route) = delivery
            .query
            .route
            .filter(|route| route.starts_with("/webhooks/"))
        else {
            warn!("the tunnel dropped a delivery that names no webhook route");
            return;
        };
        let mut request = self
            .listener
            .post(format!("http://{listen}{route}"))
            .timeout(self.timeouts.tunnel_connect)
            .body(delivery.body.map_or("", RawValue::get).to_owned());
        for (name, value) in [
            ("Content-Type", delivery.content_type),
            ("X-Github-Event", delivery.event),
            ("X-Github-Delivery", delivery.id),
            ("X-Hub-Signature-256", delivery.signature),
        ] {
            if let Some(value) = value {
                request = request.header(name, value);
            }
        }
        match request.send().await.map(|response| response.status()) {
            Ok(StatusCode::ACCEPTED) => {}
            Ok(StatusCode::UNAUTHORIZED) => warn!(
                %route,
                "the listener refused a delivery through the tunnel: its signature \
                 does not match the body as smee.io passed it on"
            ),
            Ok(status) => {
                warn!(%route, %status, "the listener refused a delivery through the tunnel")
            }
            Err(error) => {
                warn!(%route, error = chain(&error), "the tunnel cannot reach the listener")
            }
        }
    }
}

/// An error and every cause under it: reqwest's own text names only the URL.
fn chain(error: &dyn Error) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}

/// One delivery as smee.io streams it: the forge's headers, lowercased, at
/// the top level beside the parsed body and the query.
#[derive(Deserialize)]
struct Delivery<'a> {
    #[serde(borrow)]
    body: Option<&'a RawValue>,
    #[serde(default)]
    query: Query,
    #[serde(rename = "content-type")]
    content_type: Option<String>,
    #[serde(rename = "x-github-event")]
    event: Option<String>,
    #[serde(rename = "x-github-delivery")]
    id: Option<String>,
    #[serde(rename = "x-hub-signature-256")]
    signature: Option<String>,
}

/// The query of the hook URL: the listener route the delivery is for.
#[derive(Default, Deserialize)]
struct Query {
    route: Option<String>,
}

/// An open tunnel: the channel URL and its event stream. Dropping it closes
/// the stream.
struct Open {
    url: String,
    stream: Stream,
}

/// A channel's event stream, read one whole event at a time.
struct Stream {
    response: reqwest::Response,
    parser: SseParser,
    pending: VecDeque<SseEvent>,
}

impl Stream {
    async fn next(&mut self) -> Result<SseEvent, String> {
        loop {
            if let Some(event) = self.pending.pop_front() {
                return Ok(event);
            }
            match self.response.chunk().await {
                Ok(Some(chunk)) => {
                    let mut out = Vec::new();
                    self.parser.push(&chunk, &mut out);
                    self.pending.extend(out);
                }
                Ok(None) => return Err("the tunnel server closed the stream".into()),
                Err(error) => return Err(chain(&error)),
            }
        }
    }
}
