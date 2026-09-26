//! Laya, the local model the `ai` permission mode answers with (022).
//!
//! What this holds is the settings, the Python check and the install. Running
//! the server is 022's `Server` section, and deciding a permission request is
//! its `Decisions` section; both are built on the `endpoint` and `live`
//! handles here.
//!
//! The settings are one row of the store, and the install is one background
//! task at a time. Everything a client reads comes back as one
//! [`LayaStatusDto`], and every change to it is published as `laya_updated`.

pub mod install;
pub mod python;
pub mod schedule;

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, RwLock};

use ariadne_api::permissions::{LayaCheckpoints, LayaState, LayaStatusDto};
use ariadne_store::Store;

use crate::bus::EventBus;
use crate::config::Config;
use crate::timeouts::Timeouts;

/// What the `ai` permission mode needs to answer a request: a Laya that is
/// on, and somewhere to ask it.
#[derive(Debug, Clone, PartialEq)]
pub struct LayaLive {
    /// Where the Laya server answers.
    pub endpoint: String,
    /// How sure Laya has to be before its answer is taken, 0 to 1.
    pub threshold: f64,
}

/// The daemon's Laya: its settings, its install, and where its server is.
#[derive(Clone)]
pub struct Laya {
    store: Store,
    events: EventBus,
    /// `<home>/laya`: the virtual environment, and the Hugging Face cache
    /// under it. A disabled Laya keeps everything here.
    home: PathBuf,
    /// The `python_bin` config key, or `None` for `python3` on PATH.
    python_bin: Option<String>,
    /// Where the release document is read from (`laya_release_url`).
    release_url: String,
    /// The command that stands in for the whole install, in the suite.
    installer: Option<Vec<String>>,
    /// The `laya_endpoint` config key, which wins over [`Laya::set_endpoint`].
    configured_endpoint: Option<String>,
    /// Where the server the daemon started answers, once one has.
    endpoint: Arc<RwLock<Option<String>>>,
    /// Whether an install is running now. Claimed with a compare-and-swap, so
    /// exactly one install runs at a time however many callers ask at once.
    installing: Arc<AtomicBool>,
    timeouts: Timeouts,
}

impl Laya {
    /// The Laya of a daemon configured by `cfg`, writing to `store` and
    /// publishing on `events`.
    pub fn new(store: Store, events: EventBus, cfg: &Config, timeouts: Timeouts) -> Self {
        Self {
            store,
            events,
            home: cfg.root.join("laya"),
            python_bin: cfg.python_bin.clone(),
            release_url: cfg.laya_release_url.clone(),
            installer: cfg.laya_installer.clone(),
            configured_endpoint: cfg.laya_endpoint.clone(),
            endpoint: Arc::default(),
            installing: Arc::default(),
            timeouts,
        }
    }

    /// The settings and the state of the install behind them, with the
    /// interpreter probed afresh: what a client reads is what a start would
    /// find now, not what it found when the daemon came up.
    pub async fn status(&self) -> LayaStatusDto {
        let path = std::env::var_os("PATH");
        let python = python::probe_python(self.python_bin.as_deref(), path.as_deref()).await;
        let row = match self.store.laya_settings().await {
            Ok(row) => row,
            Err(error) => {
                tracing::warn!(error = %error, "reading the Laya settings failed");
                return LayaStatusDto {
                    enabled: false,
                    checkpoints: LayaCheckpoints::English,
                    threshold: DEFAULT_THRESHOLD,
                    schedule: None,
                    python,
                    state: LayaState::Failed,
                    installed_release: None,
                    latest_release: None,
                    weights_present: false,
                    endpoint: self.endpoint(),
                    last_refresh_at: None,
                    last_error: Some(error.to_string()),
                };
            }
        };
        LayaStatusDto {
            enabled: row.enabled,
            checkpoints: checkpoints_of(&row.checkpoints),
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

    /// Where the Laya server answers: the configured endpoint where there is
    /// one, else whatever the server last reported through
    /// [`Laya::set_endpoint`], else nothing.
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

    /// What the `ai` permission mode needs, or `None` where Laya cannot
    /// answer: it is off, or nothing is serving it.
    pub async fn live(&self) -> Option<LayaLive> {
        let endpoint = self.endpoint()?;
        let row = self.store.laya_settings().await.ok()?;
        row.enabled.then_some(LayaLive {
            endpoint,
            threshold: row.threshold,
        })
    }

    /// Publish the status as it now stands, whatever moved it.
    pub(crate) async fn announce(&self) -> LayaStatusDto {
        let status = self.status().await;
        self.events.laya_updated(status.clone());
        status
    }
}

/// The threshold a daemon that cannot read its settings reports: the same one
/// the schema defaults to, so a failure does not invent a number.
const DEFAULT_THRESHOLD: f64 = 0.8;

/// The checkpoints a stored spelling names. A row written by a future build
/// that spells it some other way reads as the smaller download.
fn checkpoints_of(stored: &str) -> LayaCheckpoints {
    match stored {
        "all" => LayaCheckpoints::All,
        _ => LayaCheckpoints::English,
    }
}

/// The state a stored spelling names. One nothing here knows reads as
/// `failed`: a state that cannot be read is not one to answer requests on.
fn state_of(stored: &str) -> LayaState {
    match stored {
        "installing" => LayaState::Installing,
        "ready" => LayaState::Ready,
        "disabled" => LayaState::Disabled,
        _ => LayaState::Failed,
    }
}
