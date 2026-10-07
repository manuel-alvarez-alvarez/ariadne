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
pub(crate) mod derive;
pub mod flavours;
pub mod hardware;
pub mod install;
pub(crate) mod operations;
pub mod python;
mod server;

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, RwLock};

use tokio::sync::{mpsc, watch};

use ariadne_api::permissions::{
    AiPermissionsState, AiPermissionsStatusDto, Device, DeviceOptionDto, Flavour,
    FlavourOptionsDto, GpuDto, HardwareDto,
};
use ariadne_store::{AiPermissionSettings, AiPermissionSettingsUpdate, Store};

use hardware::HardwareOverride;

use crate::bus::EventBus;
use crate::config::Config;
use crate::timeouts::Timeouts;

/// What the `ai` permission mode needs to answer a request: a model that is
/// on, and somewhere to ask it.
#[derive(Debug, Clone, PartialEq)]
pub struct AiPermissionsLive {
    /// Where the model server answers.
    pub endpoint: String,
    /// Danger at or below this value is allowed, 0 to 1.
    pub allow_threshold: f64,
    /// Danger at or above this value is denied, 0 to 1.
    pub deny_threshold: f64,
}

/// The daemon's model: its settings, its install, and where its server is.
#[derive(Clone)]
pub struct AiPermissions {
    store: Store,
    events: EventBus,
    /// `<home>/ai-permissions`: the virtual environment, and the Hugging Face cache
    /// under it. A disabled model keeps everything here.
    home: PathBuf,
    /// The `python_bin` config key, or `None` for supported Python on PATH.
    python_bin: Option<String>,
    /// The `nvidia_smi_bin` config key, or `None` for `nvidia-smi` on PATH.
    nvidia_smi_bin: Option<String>,
    /// `ai_permissions_hardware`: replaces the whole hardware probe in tests.
    hardware_override: Option<HardwareOverride>,
    /// The command that stands in for the whole install, in the suite.
    installer: Option<Vec<String>>,
    /// The command that stands in for the server's Python in integration tests.
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
    /// The one advisory failure-diagnosis request now reaching this model,
    /// if any (024, `crate::failure_diagnosis`). A permission decision
    /// aborts it before it asks the same model itself, so a diagnosis never
    /// delays a decision an agent's turn is blocked on.
    diagnosis_request: Arc<Mutex<Option<tokio::task::AbortHandle>>>,
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
            nvidia_smi_bin: cfg.nvidia_smi_bin.clone(),
            hardware_override: cfg.ai_permissions_hardware.clone(),
            installer: cfg.ai_permissions_installer.clone(),
            serve_command: cfg.ai_permissions_serve_command.clone(),
            configured_endpoint: cfg.ai_permissions_endpoint.clone(),
            endpoint: Arc::default(),
            starting: Arc::new(watch::Sender::new(false)),
            installing: Arc::default(),
            timeouts,
            server_tx,
            diagnosis_request: Arc::default(),
        };
        server::start(ai_permissions.clone(), server_rx);
        ai_permissions
    }

    /// The settings and the state of the install behind them, with the
    /// interpreter and the hardware probed afresh: what a client reads is
    /// what a start would find now, not what it found when the daemon came up.
    pub async fn status(&self) -> AiPermissionsStatusDto {
        let path = std::env::var_os("PATH");
        let python = python::probe_python(self.python_bin.as_deref(), path.as_deref()).await;
        let hardware = self.hardware().await;
        let row = match self.store.ai_permission_settings().await {
            Ok(row) => row,
            Err(error) => {
                tracing::warn!(error = %error, "reading the AI permission settings failed");
                let (flavour, device) = effective_pair(&hardware, Flavour::Kev4B, None);
                let (allow_threshold, deny_threshold) = default_thresholds(flavour);
                return AiPermissionsStatusDto {
                    enabled: false,
                    allow_threshold,
                    deny_threshold,
                    thresholds_default: true,
                    flavour,
                    device,
                    hardware: hardware_dto(&hardware),
                    flavours: flavours_dto(&hardware),
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
        let stored_flavour = Flavour::parse(&row.flavour).unwrap_or(Flavour::Kev4B);
        let stored_device = row.device.as_deref().and_then(Device::parse);
        let (flavour, device) = effective_pair(&hardware, stored_flavour, stored_device);
        let (allow_threshold, deny_threshold) = thresholds(&row, flavour);
        AiPermissionsStatusDto {
            enabled: row.enabled,
            allow_threshold,
            deny_threshold,
            thresholds_default: !row.thresholds_hand_set,
            flavour,
            device,
            hardware: hardware_dto(&hardware),
            flavours: flavours_dto(&hardware),
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

    /// The machine this daemon runs on: `ai_permissions_hardware` where the
    /// suite set it, else the real probe.
    pub(crate) async fn hardware(&self) -> hardware::Hardware {
        hardware::hardware(
            self.hardware_override.as_ref(),
            self.nvidia_smi_bin.as_deref(),
            std::env::var_os("PATH").as_deref(),
        )
        .await
    }

    /// The flavour as stored, ignoring the display fallback `status` can
    /// substitute for it when no device runs that flavour here. What a
    /// device-only `PUT` keeps: `status`'s own `flavour` is not it, since a
    /// fallback there is not a change anybody asked for.
    pub(crate) async fn stored_flavour(&self) -> Flavour {
        self.store
            .ai_permission_settings()
            .await
            .ok()
            .and_then(|row| Flavour::parse(&row.flavour))
            .unwrap_or(Flavour::Kev4B)
    }

    /// The flavour and device an install puts on disk and the server runs:
    /// the pair `status` reports.
    pub(crate) async fn chosen(&self) -> (Flavour, Device) {
        let hardware = self.hardware().await;
        let row = self.store.ai_permission_settings().await.ok();
        let flavour = row
            .as_ref()
            .and_then(|row| Flavour::parse(&row.flavour))
            .unwrap_or(Flavour::Kev4B);
        let device = row
            .as_ref()
            .and_then(|row| row.device.as_deref())
            .and_then(Device::parse);
        effective_pair(&hardware, flavour, device)
    }

    /// Fill a `NULL` stored device with the best device that runs the stored
    /// flavour. An install from before flavours existed keeps `4b` and gets a
    /// device without a fresh choice; a fresh row does the same at its first
    /// start. Where no device runs the stored flavour on this machine, the
    /// device stays `NULL`: nothing here writes a device its own table marks
    /// unable to run it. Silent: it is a backfill, not a setting somebody
    /// chose.
    pub async fn ensure_device(&self) {
        let Ok(row) = self.store.ai_permission_settings().await else {
            return;
        };
        if row.device.is_some() {
            return;
        }
        let hardware = self.hardware().await;
        let flavour = Flavour::parse(&row.flavour).unwrap_or(Flavour::Kev4B);
        let Some(device) = flavours::best_device(&hardware, flavour) else {
            return;
        };
        if let Err(error) = self
            .store
            .update_ai_permission_settings(AiPermissionSettingsUpdate {
                device: Some(device.as_str().to_string()),
                ..Default::default()
            })
            .await
        {
            tracing::warn!(error = %error, "filling the AI permission device failed");
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

    /// The interpreter the server runs under: the venv's Python, or the
    /// command the suite stands in for it. The server adds the entry point.
    pub(crate) fn serve_command(&self) -> Vec<String> {
        self.serve_command
            .clone()
            .unwrap_or_else(|| vec![self.home.join("venv/bin/python").display().to_string()])
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
        if !row.enabled {
            return None;
        }
        // A stored device pairs with the stored flavour (`effective_pair`), so
        // only a row without one probes the hardware for the flavour it runs.
        let flavour = match row.device {
            Some(_) => Flavour::parse(&row.flavour).unwrap_or(Flavour::Kev4B),
            None => self.chosen().await.0,
        };
        let (allow_threshold, deny_threshold) = thresholds(&row, flavour);
        Some(AiPermissionsLive {
            endpoint,
            allow_threshold,
            deny_threshold,
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

    /// The maximum time one model scoring request may take.
    pub(crate) fn decision_timeout(&self) -> std::time::Duration {
        self.timeouts.ai_permissions_decision
    }

    /// Hand over the advisory diagnosis request now reaching this model, so
    /// a permission decision about to ask it too can abort it first
    /// (`preempt_diagnosis`). Replaces whatever was registered before —
    /// ending that one is not this call's business, since at most one
    /// diagnosis ever runs at a time.
    pub(crate) fn register_diagnosis_request(&self, handle: tokio::task::AbortHandle) {
        *self
            .diagnosis_request
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(handle);
    }

    /// Forget a diagnosis request that ended on its own, so a decision that
    /// preempts later never aborts an unrelated task that happens to reuse
    /// the slot.
    pub(crate) fn clear_diagnosis_request(&self) {
        *self
            .diagnosis_request
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    }

    /// Abort a diagnosis request reaching this model right now, so a
    /// permission decision needing the same model never waits behind it —
    /// the one answer a running agent turn is blocked on. A no-op where
    /// none is in flight, or where it already ended on its own.
    pub(crate) fn preempt_diagnosis(&self) {
        if let Some(handle) = self
            .diagnosis_request
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
        {
            handle.abort();
        }
    }

    /// Publish the status as it now stands, whatever moved it.
    pub(crate) async fn announce(&self) -> AiPermissionsStatusDto {
        let status = self.status().await;
        self.events.ai_permissions_updated(status.clone());
        status
    }
}

/// The flavour and device `status` reports: the stored device where there is
/// one, alongside the stored flavour. Where there is none — a row `ensure_device`
/// left `NULL` because nothing runs the stored flavour, or no row could be
/// read at all — the best device of the stored flavour where one runs it,
/// else the flavour and device a fresh row would settle on. Never the stored
/// flavour paired with a device its own `options` mark unable to run it.
fn effective_pair(
    hardware: &hardware::Hardware,
    stored_flavour: Flavour,
    stored_device: Option<Device>,
) -> (Flavour, Device) {
    if let Some(device) = stored_device {
        return (stored_flavour, device);
    }
    if let Some(device) = flavours::best_device(hardware, stored_flavour) {
        return (stored_flavour, device);
    }
    let flavour = flavours::default_flavour(hardware);
    let device = flavours::best_device(hardware, flavour).unwrap_or(Device::Cpu);
    (flavour, device)
}

fn hardware_dto(hardware: &hardware::Hardware) -> HardwareDto {
    HardwareDto {
        os: hardware.os.clone(),
        arch: hardware.arch.clone(),
        memory_bytes: hardware.memory_bytes,
        gpu: hardware.gpu.as_ref().map(|gpu| GpuDto {
            name: gpu.name.clone(),
            vram_bytes: gpu.vram_bytes,
        }),
    }
}

fn flavours_dto(hardware: &hardware::Hardware) -> Vec<FlavourOptionsDto> {
    flavours::options(hardware)
        .into_iter()
        .map(|flavour_options| FlavourOptionsDto {
            flavour: flavour_options.flavour,
            devices: flavour_options
                .devices
                .into_iter()
                .map(|device| DeviceOptionDto {
                    device: device.device,
                    can_run: device.can_run,
                    reason: device.reason,
                    slow: device.slow,
                })
                .collect(),
        })
        .collect()
}

/// The allow and deny thresholds of a flavour whose pair nobody set by hand
/// (022, rule 24). The benchmark selects each pair on that flavour's run.
pub(crate) fn default_thresholds(flavour: Flavour) -> (f64, f64) {
    match flavour {
        // The benchmark's kev-9b pair, -0.0499 / 0.9220, allows nothing: an
        // allow threshold under zero admits no case. 9b takes the 4b pair.
        Flavour::Kev9B => (0.0201, 0.6321),
        Flavour::Kev4B => (0.0201, 0.6321),
        // The benchmark did not measure 0.8b or 27b; they take the 4b pair.
        Flavour::Kev08B | Flavour::Kev27B => (0.0201, 0.6321),
    }
}

/// The pair in force: the one stored where the user set it by hand, else the
/// default of `flavour`, the flavour the model runs.
fn thresholds(row: &AiPermissionSettings, flavour: Flavour) -> (f64, f64) {
    match row.thresholds_hand_set {
        true => (row.allow_threshold, row.deny_threshold),
        false => default_thresholds(flavour),
    }
}

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
