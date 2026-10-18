//! The webhook tunnel (027): a localtunnel to the bound webhook listener,
//! kept up, and switched on and off at run time.
//!
//! The tunnel writes its URL into [`WebhookUrl`] while it is up and withdraws
//! it while it is down or off. The hook reconcile does the rest: it moves each
//! hook to the URL, and each repository's fetch between `WakeOnly` and
//! `Timer`. The transport is the daemon's alone: a fetch records the same rows
//! whatever woke it.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use ariadne_api::repositories::{ForgeTunnelDto, TunnelState};
use ariadne_api::stream::DomainEvent;
use ariadne_store::{Store, StoreError};
use chrono::{SecondsFormat, Utc};
use localtunnel_client::{ClientConfig, broadcast, open_tunnel};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Notify, watch};
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::{Instant, sleep_until, timeout};
use tracing::{info, warn};

use super::poll::ForgePoll;
use crate::bus::{BusEvent, EventBus};
use crate::config::Config;
use crate::timeouts::Timeouts;
use crate::webhooks::{WebhookListen, WebhookUrl};

/// The longest wait between two reconnects.
const BACKOFF_CAP: Duration = Duration::from_secs(60);

/// The most connections the client holds open to the tunnel server.
const MAX_CONNECTIONS: u8 = 10;

/// Words for a random subdomain: a word, a hyphen and six digits.
const WORDS: &[&str] = &[
    "amber", "birch", "cedar", "delta", "ember", "fjord", "grove", "heron", "iris", "juniper",
    "kelp", "lumen", "maple", "nectar", "orchid", "pebble",
];

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
        self.url.set(Some(url.clone()));
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
            let attempt = self.open(listen);
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
                    // One outage clock for the whole hold: an unrelated
                    // repository event must not restart it.
                    let lost = open.relay.lost(self.timeouts.tunnel_connect);
                    tokio::pin!(lost);
                    let wanted = loop {
                        tokio::select! {
                            () = &mut lost => break true,
                            _ = self.until_changed(&mut events, None) => {
                                if !self.wanted().await {
                                    break false;
                                }
                            }
                        }
                    };
                    if !wanted {
                        continue;
                    }
                    self.down("the tunnel server closed every connection".into())
                        .await;
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

    /// The subdomain to ask for: the configured one, else the stored one,
    /// else a random one stored now.
    async fn subdomain(&self) -> Result<String, String> {
        if let Some(subdomain) = &self.cfg.tunnel_subdomain {
            return Ok(subdomain.clone());
        }
        let settings = self
            .store
            .forge_settings()
            .await
            .map_err(|e| e.to_string())?;
        if let Some(subdomain) = settings.tunnel_subdomain {
            return Ok(subdomain);
        }
        let mut bytes = [0u8; 8];
        getrandom::fill(&mut bytes).map_err(|e| e.to_string())?;
        let word = WORDS[usize::from(bytes[0]) % WORDS.len()];
        let digits = u32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]) % 1_000_000;
        let picked = format!("{word}-{digits:06}");
        let kept = self
            .store
            .keep_tunnel_subdomain(&picked)
            .await
            .map_err(|e| e.to_string())?;
        Ok(kept.tunnel_subdomain.unwrap_or(picked))
    }

    /// One connection attempt: register with the server and wait for the
    /// first tunnel connection, within `tunnel_connect`.
    async fn open(&self, listen: SocketAddr) -> Result<Open, String> {
        let subdomain = self.subdomain().await?;
        let relay = Relay::bind(listen).await.map_err(|e| e.to_string())?;
        let (stop, _) = broadcast::channel(1);
        let config = ClientConfig {
            server: Some(self.cfg.tunnel_host.trim_end_matches('/').to_string()),
            subdomain: Some(subdomain),
            local_host: Some("127.0.0.1".into()),
            local_port: relay.port,
            shutdown_signal: stop.clone(),
            max_conn: MAX_CONNECTIONS,
            credential: None,
            // The supervisor here re-registers, so the client never does.
            reregister_after: Some(Duration::from_secs(24 * 60 * 60)),
        };
        let mut open = Open {
            url: String::new(),
            stop,
            relay,
        };
        let mut connections = open.relay.connections.clone();
        let limit = self.timeouts.tunnel_connect;
        open.url = timeout(limit, async {
            let url = open_tunnel(config)
                .await
                .map_err(|error| format!("{error:#}"))?;
            connections
                .wait_for(|count| *count > 0)
                .await
                .map_err(|_| "the relay stopped".to_string())?;
            Ok::<_, String>(url)
        })
        .await
        .map_err(|_| format!("no tunnel connection within {limit:?}"))??;
        Ok(open)
    }
}

/// An open tunnel. Dropping it closes the client and the relay.
struct Open {
    url: String,
    stop: broadcast::Sender<()>,
    relay: Relay,
}

impl Drop for Open {
    fn drop(&mut self) {
        let _ = self.stop.send(());
    }
}

/// A loopback relay between the tunnel client and the webhook listener.
///
/// `localtunnel-client` reports no dropped tunnel: it reconnects on its own
/// and says nothing. Each tunnel connection it holds opens one local
/// connection at once, so the relay counts the open tunnel connections. An up
/// tunnel with no open connection for `tunnel_connect` is down. The relay
/// listens on loopback only and forwards to the bound listener address and
/// nowhere else.
struct Relay {
    port: u16,
    connections: watch::Receiver<usize>,
    task: JoinHandle<()>,
}

impl Drop for Relay {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Relay {
    async fn bind(listen: SocketAddr) -> std::io::Result<Self> {
        let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).await?;
        let port = listener.local_addr()?.port();
        let (count, connections) = watch::channel(0usize);
        let count = Arc::new(count);
        let task = tokio::spawn(async move {
            // Dropped with the task, which aborts every forward.
            let mut forwards = JoinSet::new();
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        let Ok((mut inbound, _)) = accepted else { continue };
                        count.send_modify(|n| *n += 1);
                        let count = count.clone();
                        forwards.spawn(async move {
                            if let Ok(mut outbound) = TcpStream::connect(listen).await {
                                let _ = tokio::io::copy_bidirectional(&mut inbound, &mut outbound).await;
                            }
                            count.send_modify(|n| *n -= 1);
                        });
                    }
                    Some(_) = forwards.join_next() => {}
                }
            }
        });
        Ok(Self {
            port,
            connections,
            task,
        })
    }

    /// Answers once no tunnel connection has been open for `grace`.
    async fn lost(&mut self, grace: Duration) {
        loop {
            if self.connections.wait_for(|n| *n == 0).await.is_err() {
                return;
            }
            if timeout(grace, self.connections.wait_for(|n| *n > 0))
                .await
                .is_err()
            {
                return;
            }
        }
    }
}
