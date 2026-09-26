//! Laya, the local model that answers permission requests in the `ai`
//! permission mode (022).
//!
//! One settings row behind `/v1/permissions/laya`, the Python interpreter the
//! daemon found, and where the install has got to. The install is a Python
//! package and two gigabytes of weights, so it runs in the background: a
//! write answers with `installing` and the `laya_updated` event says how it
//! ended.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Which checkpoints the install downloads.
///
/// English alone is 843 MB; all three — English, multilingual and
/// typed-decisions — are 2.4 GB together.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum LayaCheckpoints {
    /// The English checkpoint alone.
    English,
    /// English, multilingual and typed-decisions.
    All,
}

impl LayaCheckpoints {
    /// The spelling the settings row and the installer's environment carry.
    pub fn as_str(&self) -> &'static str {
        match self {
            LayaCheckpoints::English => "english",
            LayaCheckpoints::All => "all",
        }
    }
}

/// Where the install has got to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum LayaState {
    /// Laya is off. The files of an earlier install are kept.
    Disabled,
    /// An install is running now.
    Installing,
    /// The package and the weights are on disk.
    Ready,
    /// The last install failed; `last_error` says why.
    Failed,
}

impl LayaState {
    /// The spelling the settings row carries.
    pub fn as_str(&self) -> &'static str {
        match self {
            LayaState::Disabled => "disabled",
            LayaState::Installing => "installing",
            LayaState::Ready => "ready",
            LayaState::Failed => "failed",
        }
    }
}

/// The Python interpreter the daemon found, as it answered `--version`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct PythonDto {
    /// Absolute path, when one was found.
    #[schema(example = "/usr/bin/python3")]
    pub path: Option<String>,
    /// The version it printed, without the `Python ` in front of it.
    #[schema(example = "3.12.1")]
    pub version: Option<String>,
    /// Whether it is Python 3.10 or newer, which Laya needs.
    pub ok: bool,
}

/// The Laya settings and the state of the install behind them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct LayaStatusDto {
    /// Whether Laya answers permission requests at all.
    pub enabled: bool,
    pub checkpoints: LayaCheckpoints,
    /// How sure Laya has to be before its answer is taken, 0 to 1.
    #[schema(example = 0.8)]
    pub threshold: f64,
    /// When the daily refresh runs, `HH:MM` in 24-hour local time. `null`
    /// turns the refresh off.
    #[schema(example = "03:30")]
    pub schedule: Option<String>,
    pub python: PythonDto,
    pub state: LayaState,
    /// The release tag of the package on disk.
    #[schema(example = "v0.1.4")]
    pub installed_release: Option<String>,
    /// The release tag the last download reported.
    pub latest_release: Option<String>,
    /// Whether the checkpoints of the last good install are on disk.
    pub weights_present: bool,
    /// Where the Laya server answers, once one is running (022, Server).
    pub endpoint: Option<String>,
    /// When the last install ended well, RFC 3339 in UTC.
    pub last_refresh_at: Option<String>,
    /// Why the last install failed.
    pub last_error: Option<String>,
}

/// Partial update of the Laya settings; an absent field stays unchanged.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateLayaRequest {
    /// Turning it on starts an install; turning it off keeps the files.
    pub enabled: Option<bool>,
    pub checkpoints: Option<LayaCheckpoints>,
    /// 0 to 1. Anything else is refused.
    pub threshold: Option<f64>,
    /// `HH:MM` in 24-hour local time. Absent keeps the schedule; `null`
    /// turns it off.
    #[serde(
        default,
        deserialize_with = "nullable_schedule",
        skip_serializing_if = "Option::is_none"
    )]
    #[schema(value_type = Option<String>, nullable = true, example = "03:30")]
    pub schedule: Option<Option<String>>,
}

/// Tell "the field is absent" from "the field is `null`", which plain
/// `Option<Option<String>>` cannot: serde reads a `null` into the outer
/// `Option` and both readings arrive as `None`. Absent is the `Default`
/// `None`; anything this sees is a `Some`, holding the `null` as an inner
/// `None`.
fn nullable_schedule<'de, D>(de: D) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<String>::deserialize(de).map(Some)
}
