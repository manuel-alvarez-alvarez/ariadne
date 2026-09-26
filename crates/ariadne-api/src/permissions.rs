//! The AI permission model, the local model that answers permission requests
//! in the `ai` permission mode (022).
//!
//! One settings row behind `/v1/permissions/ai`, the Python interpreter the
//! daemon found, and where the install has got to. The install is a Python
//! package and two gigabytes of weights, so it runs in the background: a
//! write answers with `installing` and the `ai_permissions_updated` event
//! says how it ended.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Which checkpoints the install downloads.
///
/// English alone is 843 MB; all three — English, multilingual and
/// typed-decisions — are 2.4 GB together.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AiPermissionsCheckpoints {
    /// The English checkpoint alone.
    English,
    /// English, multilingual and typed-decisions.
    All,
}

impl AiPermissionsCheckpoints {
    /// The spelling the settings row and the installer's environment carry.
    pub fn as_str(&self) -> &'static str {
        match self {
            AiPermissionsCheckpoints::English => "english",
            AiPermissionsCheckpoints::All => "all",
        }
    }
}

/// Where the install has got to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AiPermissionsState {
    /// The model is off. The files of an earlier install are kept.
    Disabled,
    /// An install is running now.
    Installing,
    /// The package and the weights are on disk.
    Ready,
    /// The last install failed; `last_error` says why.
    Failed,
}

impl AiPermissionsState {
    /// The spelling the settings row carries.
    pub fn as_str(&self) -> &'static str {
        match self {
            AiPermissionsState::Disabled => "disabled",
            AiPermissionsState::Installing => "installing",
            AiPermissionsState::Ready => "ready",
            AiPermissionsState::Failed => "failed",
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
    /// Whether it is Python 3.10 or newer, which the model needs.
    pub ok: bool,
}

/// The AI permission settings and the state of the install behind them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct AiPermissionsStatusDto {
    /// Whether the model answers permission requests at all.
    pub enabled: bool,
    pub checkpoints: AiPermissionsCheckpoints,
    /// How sure the model has to be before its answer is taken, 0 to 1.
    #[schema(example = 0.8)]
    pub threshold: f64,
    /// When the daily refresh runs, `HH:MM` in 24-hour local time. `null`
    /// turns the refresh off.
    #[schema(example = "03:30")]
    pub schedule: Option<String>,
    pub python: PythonDto,
    pub state: AiPermissionsState,
    /// The release tag of the package on disk.
    #[schema(example = "v0.1.4")]
    pub installed_release: Option<String>,
    /// The release tag the last download reported.
    pub latest_release: Option<String>,
    /// Whether the checkpoints of the last good install are on disk.
    pub weights_present: bool,
    /// Where the model server answers, once one is running (022, Server).
    pub endpoint: Option<String>,
    /// When the last install ended well, RFC 3339 in UTC.
    pub last_refresh_at: Option<String>,
    /// Why the last install failed.
    pub last_error: Option<String>,
    /// The prompt texts each decision sends the model now.
    pub prompts: AiPermissionsPrompts,
    /// The built-in prompt texts, which a `null` prompt restores.
    pub default_prompts: AiPermissionsPrompts,
}

/// The prompt texts of the one choice question each decision asks the model.
/// The answer names, `allow` and `review`, are fixed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct AiPermissionsPrompts {
    /// The question the model answers.
    pub question: String,
    /// What the `allow` answer covers.
    pub allow_criteria: String,
    /// What the `review` answer covers.
    pub review_criteria: String,
}

/// Partial update of the AI permission settings; an absent field stays unchanged.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateAiPermissionsRequest {
    /// Turning it on starts an install; turning it off keeps the files.
    pub enabled: Option<bool>,
    pub checkpoints: Option<AiPermissionsCheckpoints>,
    /// 0 to 1. Anything else is refused.
    pub threshold: Option<f64>,
    /// `HH:MM` in 24-hour local time. Absent keeps the schedule; `null`
    /// turns it off.
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schema(value_type = Option<String>, nullable = true, example = "03:30")]
    pub schedule: Option<Option<String>>,
    /// The question the model answers. Absent keeps it; `null` or a blank
    /// text restores the built-in one. At most 4000 characters.
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schema(value_type = Option<String>, nullable = true)]
    pub question: Option<Option<String>>,
    /// What the `allow` answer covers, kept, restored and limited as
    /// `question` is.
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schema(value_type = Option<String>, nullable = true)]
    pub allow_criteria: Option<Option<String>>,
    /// What the `review` answer covers, kept, restored and limited as
    /// `question` is.
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schema(value_type = Option<String>, nullable = true)]
    pub review_criteria: Option<Option<String>>,
}

/// Tell "the field is absent" from "the field is `null`", which plain
/// `Option<Option<String>>` cannot: serde reads a `null` into the outer
/// `Option` and both readings arrive as `None`. Absent is the `Default`
/// `None`; anything this sees is a `Some`, holding the `null` as an inner
/// `None`.
fn nullable<'de, D>(de: D) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<String>::deserialize(de).map(Some)
}
