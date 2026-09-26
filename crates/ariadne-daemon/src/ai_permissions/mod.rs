//! The AI permission model, the local model the `ai` permission mode answers with (022).
//!
//! What this holds is the settings, the Python check and the install. Running
//! the server is 022's `Server` section, and deciding a permission request is
//! its `Decisions` section; both are built on the `endpoint` and `live`
//! handles here.
//!
//! The settings are one row of the store, and the install is one background
//! task at a time. Everything a client reads comes back as one
//! [`AiPermissionsStatusDto`], and every change to it is published as `ai_permissions_updated`.

pub(crate) mod decide;
pub mod install;
pub mod python;
pub mod schedule;
mod server;

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, RwLock};
use tokio::sync::{mpsc, watch};

use ariadne_api::permissions::{AiPermissionsState, AiPermissionsStatusDto};
use ariadne_store::Store;

use crate::bus::EventBus;
use crate::config::Config;
use crate::timeouts::Timeouts;

/// What the `ai` permission mode needs to answer a request: a model that is
/// on, and somewhere to ask it.
#[derive(Debug, Clone, PartialEq)]
pub struct AiPermissionsLive {
    /// Where the model server answers.
    pub endpoint: String,
    /// How sure the model has to be before its answer is taken, 0 to 1.
    pub threshold: f64,
}

/// The daemon's model: its settings, its install, and where its server is.
#[derive(Clone)]
pub struct AiPermissions {
    store: Store,
    events: EventBus,
    /// `<home>/ai-permissions`: the virtual environment, and the Hugging Face cache
    /// under it. A disabled model keeps everything here.
    home: PathBuf,
    /// The `python_bin` config key, or `None` for `python3` on PATH.
    python_bin: Option<String>,
    /// Where the release document is read from (`ai_permissions_release_url`).
    release_url: String,
    /// The command that stands in for the whole install, in the suite.
    installer: Option<Vec<String>>,
    /// The command that stands in for `laya-serve` in integration tests.
    serve_command: Option<Vec<String>>,
    /// The `ai_permissions_endpoint` config key, which wins over [`AiPermissions::set_endpoint`].
    configured_endpoint: Option<String>,
    /// Where the server the daemon started answers, once one has.
    endpoint: Arc<RwLock<Option<String>>>,
    /// Whether a server has been launched and is still loading its weights:
    /// neither healthy nor given up on yet.
    starting: Arc<watch::Sender<bool>>,
    /// Whether an install is running now. Claimed with a compare-and-swap, so
    /// exactly one install runs at a time however many callers ask at once.
    installing: Arc<AtomicBool>,
    timeouts: Timeouts,
    server_tx: mpsc::UnboundedSender<server::Command>,
}

impl AiPermissions {
    /// The AI permission model of a daemon configured by `cfg`, writing to `store` and
    /// publishing on `events`.
    pub fn new(store: Store, events: EventBus, cfg: &Config, timeouts: Timeouts) -> Self {
        let (server_tx, server_rx) = mpsc::unbounded_channel();
        let ai_permissions = Self {
            store,
            events,
            home: cfg.root.join("ai-permissions"),
            python_bin: cfg.python_bin.clone(),
            release_url: cfg.ai_permissions_release_url.clone(),
            installer: cfg.ai_permissions_installer.clone(),
            serve_command: cfg.ai_permissions_serve_command.clone(),
            configured_endpoint: cfg.ai_permissions_endpoint.clone(),
            endpoint: Arc::default(),
            starting: Arc::new(watch::Sender::new(false)),
            installing: Arc::default(),
            timeouts,
            server_tx,
        };
        server::start(ai_permissions.clone(), server_rx);
        ai_permissions
    }

    /// The settings and the state of the install behind them, with the
    /// interpreter probed afresh: what a client reads is what a start would
    /// find now, not what it found when the daemon came up.
    pub async fn status(&self) -> AiPermissionsStatusDto {
        let path = std::env::var_os("PATH");
        let python = python::probe_python(self.python_bin.as_deref(), path.as_deref()).await;
        let row = match self.store.ai_permission_settings().await {
            Ok(row) => row,
            Err(error) => {
                tracing::warn!(error = %error, "reading the AI permission settings failed");
                return AiPermissionsStatusDto {
                    enabled: false,
                    threshold: DEFAULT_THRESHOLD,
                    schedule: None,
                    python,
                    state: AiPermissionsState::Failed,
                    installed_release: None,
                    latest_release: None,
                    weights_present: false,
                    endpoint: self.endpoint(),
                    last_refresh_at: None,
                    last_error: Some(error.to_string()),
                };
            }
        };
        AiPermissionsStatusDto {
            enabled: row.enabled,
            threshold: row.threshold,
            schedule: row.schedule,
            python,
            state: state_of(&row.state),
            installed_release: row.installed_release,
            latest_release: row.latest_release,
            weights_present: row.weights_present,
            endpoint: self.endpoint(),
            last_refresh_at: row.last_refresh_at,
            last_error: row.last_error,
        }
    }

    /// Where the model server answers: the configured endpoint where there is
    /// one, else whatever the server last reported through
    /// [`AiPermissions::set_endpoint`], else nothing.
    pub fn endpoint(&self) -> Option<String> {
        self.configured_endpoint.clone().or_else(|| {
            self.endpoint
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone()
        })
    }

    /// Say where the server that just started answers, or that none does.
    /// The configured endpoint wins over it, so a harness that pins one is
    /// not moved by a server.
    pub fn set_endpoint(&self, endpoint: Option<String>) {
        *self
            .endpoint
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = endpoint;
    }

    /// Say that a server is loading its weights, or that it is done: healthy,
    /// or given up on.
    pub(crate) fn set_starting(&self, starting: bool) {
        self.starting.send_replace(starting);
    }

    pub(crate) fn notify_server(&self) {
        let _ = self.server_tx.send(server::Command::Reconcile);
    }

    pub(crate) fn serve_command(&self) -> Vec<String> {
        self.serve_command
            .clone()
            .unwrap_or_else(|| vec![self.home.join("venv/bin/laya-serve").display().to_string()])
    }

    /// Reap the server and its process group before daemon shutdown finishes.
    pub async fn shutdown(&self) {
        let (done_tx, done_rx) = tokio::sync::oneshot::channel();
        if self.server_tx.send(server::Command::Stop(done_tx)).is_ok() {
            let _ = done_rx.await;
        }
    }

    /// What the `ai` permission mode needs, or `None` where the model cannot
    /// answer: it is off, or nothing is serving it.
    pub async fn live(&self) -> Option<AiPermissionsLive> {
        let endpoint = self.endpoint()?;
        let row = self.store.ai_permission_settings().await.ok()?;
        row.enabled.then_some(AiPermissionsLive {
            endpoint,
            threshold: row.threshold,
        })
    }

    /// [`AiPermissions::live`], after waiting out a server that is still loading its
    /// weights, so a request made just after the daemon starts is still
    /// the model's to decide. A model with no server starting answers at once.
    pub async fn live_once_started(&self) -> Option<AiPermissionsLive> {
        let mut starting = self.starting.subscribe();
        loop {
            let was_starting = *starting.borrow_and_update();
            if let Some(live) = self.live().await {
                return Some(live);
            }
            if !was_starting || starting.changed().await.is_err() {
                return None;
            }
        }
    }

    /// Publish the status as it now stands, whatever moved it.
    pub(crate) async fn announce(&self) -> AiPermissionsStatusDto {
        let status = self.status().await;
        self.events.ai_permissions_updated(status.clone());
        status
    }
}

/// The threshold a daemon that cannot read its settings reports: the same one
/// the schema defaults to, so a failure does not invent a number.
const DEFAULT_THRESHOLD: f64 = 0.8;

/// The state a stored spelling names. One nothing here knows reads as
/// `failed`: a state that cannot be read is not one to answer requests on.
fn state_of(stored: &str) -> AiPermissionsState {
    match stored {
        "installing" => AiPermissionsState::Installing,
        "ready" => AiPermissionsState::Ready,
        "disabled" => AiPermissionsState::Disabled,
        _ => AiPermissionsState::Failed,
    }
}
