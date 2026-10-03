//! The AI permission model, the local model that answers permission requests
//! in the `ai` permission mode (022).
//!
//! One settings row behind `/v1/permissions/ai`, the Python interpreter the
//! daemon found, and where the install has got to. The install is a Python
//! package and its weights, so it runs in the background: a
//! write answers with `installing` and the `ai_permissions_updated` event
//! says how it ended.

use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

/// The repository permission mode a learned permission was decided under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum LearnedPermissionTarget {
    Auto,
    Ask,
    Learn,
    Ai,
}

impl LearnedPermissionTarget {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Ask => "ask",
            Self::Learn => "learn",
            Self::Ai => "ai",
        }
    }
}

/// How much of a request a learned permission answers for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum LearnedPermissionLevel {
    /// This one request.
    Once,
    /// Every request with the same key.
    Command,
    /// Every request of the same command family.
    Family,
}

impl LearnedPermissionLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Once => "once",
            Self::Command => "command",
            Self::Family => "family",
        }
    }
}

/// Where a learned permission answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum LearnedPermissionScope {
    /// The repository the request came from.
    Repository,
    /// Every repository.
    All,
}

impl LearnedPermissionScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Repository => "repository",
            Self::All => "all",
        }
    }

    pub fn parse(wire: &str) -> Option<Self> {
        Some(match wire {
            "repository" => Self::Repository,
            "all" => Self::All,
            _ => return None,
        })
    }
}

/// Widen a row to every repository, or narrow it back to its own.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateLearnedPermissionRequest {
    pub scope: LearnedPermissionScope,
}

/// One user choice or denial of an ACP permission request, keyed by the
/// repository, the tool name, the level and the normalized input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct LearnedPermissionDto {
    pub id: String,
    pub repository_id: String,
    /// `toolCall.name`, else `toolCall._meta.claudeCode.toolName`, else `toolCall.title`.
    #[schema(example = "Bash")]
    pub tool_name: String,
    /// The kept fields of `rawInput` as compact JSON with sorted keys, its
    /// one-time values replaced by placeholders such as `<HASH>`.
    #[schema(example = r#"{"command":"git rebase main"}"#)]
    pub key: String,
    pub level: LearnedPermissionLevel,
    /// The command family of a `Bash` request, such as `git rebase`, else
    /// the tool name.
    #[schema(example = "git rebase")]
    pub family: String,
    /// The derived risk tags of the request. The row answers only a request
    /// whose tags are all among them.
    pub risk_tags: Vec<String>,
    pub scope: LearnedPermissionScope,
    /// The ACP `toolCall`, its `rawInput` with sorted keys.
    pub tool_call: serde_json::Value,
    /// The ACP `options`.
    pub options: serde_json::Value,
    /// The option id of the final choice.
    pub selected_option: String,
    /// The repository permission mode at the time of the decision.
    pub target: LearnedPermissionTarget,
    /// The model decision, when the model was called; null otherwise.
    #[schema(required = true)]
    pub output: Option<serde_json::Value>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct LearnedPermissionsResponse {
    pub items: Vec<LearnedPermissionDto>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, IntoParams)]
pub struct LearnedPermissionQuery {
    pub repository: Option<String>,
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

/// A Kev flavour: how large a model to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub enum Flavour {
    #[serde(rename = "0.8b")]
    Kev08B,
    #[serde(rename = "4b")]
    Kev4B,
    #[serde(rename = "9b")]
    Kev9B,
    #[serde(rename = "27b")]
    Kev27B,
}

impl Flavour {
    /// Every flavour, in the order the wire always lists them.
    pub const ALL: [Flavour; 4] = [
        Flavour::Kev08B,
        Flavour::Kev4B,
        Flavour::Kev9B,
        Flavour::Kev27B,
    ];

    /// The spelling the store and the wire carry.
    pub fn as_str(&self) -> &'static str {
        match self {
            Flavour::Kev08B => "0.8b",
            Flavour::Kev4B => "4b",
            Flavour::Kev9B => "9b",
            Flavour::Kev27B => "27b",
        }
    }

    pub fn parse(wire: &str) -> Option<Self> {
        Some(match wire {
            "0.8b" => Flavour::Kev08B,
            "4b" => Flavour::Kev4B,
            "9b" => Flavour::Kev9B,
            "27b" => Flavour::Kev27B,
            _ => return None,
        })
    }
}

/// Where a Kev flavour runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Device {
    Mlx,
    Cuda,
    Cpu,
}

impl Device {
    /// Every device, in the order the wire always lists them.
    pub const ALL: [Device; 3] = [Device::Mlx, Device::Cuda, Device::Cpu];

    pub fn as_str(&self) -> &'static str {
        match self {
            Device::Mlx => "mlx",
            Device::Cuda => "cuda",
            Device::Cpu => "cpu",
        }
    }

    pub fn parse(wire: &str) -> Option<Self> {
        Some(match wire {
            "mlx" => Device::Mlx,
            "cuda" => Device::Cuda,
            "cpu" => Device::Cpu,
            _ => return None,
        })
    }
}

/// The GPU with the largest VRAM the daemon's probe found.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct GpuDto {
    pub name: String,
    pub vram_bytes: u64,
}

/// The machine the daemon runs on, as far as choosing a Kev flavour and
/// device cares.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct HardwareDto {
    #[schema(example = "macos")]
    pub os: String,
    #[schema(example = "aarch64")]
    pub arch: String,
    pub memory_bytes: u64,
    pub gpu: Option<GpuDto>,
}

/// Whether one device can run one flavour, and why not.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DeviceOptionDto {
    pub device: Device,
    pub can_run: bool,
    /// Why it cannot, e.g. `needs 24 GB VRAM, found 8 GB`.
    pub reason: Option<String>,
    /// A note only: the flavour can still be chosen on this device.
    pub slow: bool,
}

/// One flavour with every device it might run on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct FlavourOptionsDto {
    pub flavour: Flavour,
    pub devices: Vec<DeviceOptionDto>,
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
    /// Whether it is Python 3.12 or 3.13, which the model needs.
    pub ok: bool,
}

/// The AI permission settings and the state of the install behind them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct AiPermissionsStatusDto {
    /// Whether the model answers permission requests at all.
    pub enabled: bool,
    /// Danger at or below this value is allowed, 0 to 1.
    #[schema(example = 0.0201)]
    pub allow_threshold: f64,
    /// Danger at or above this value is denied, 0 to 1.
    #[schema(example = 0.6321)]
    pub deny_threshold: f64,
    /// The Kev flavour chosen, `4b` by default where the machine can run it.
    pub flavour: Flavour,
    /// The device the flavour runs on: the best one the machine could run it
    /// on, unless another was chosen.
    pub device: Device,
    /// The machine the daemon runs on, probed afresh.
    pub hardware: HardwareDto,
    /// Every flavour with every device it might run on, in wire order.
    pub flavours: Vec<FlavourOptionsDto>,
    pub python: PythonDto,
    pub state: AiPermissionsState,
    /// The pinned model package and run on disk.
    #[schema(example = "kev@f1535963 jaredpalmer/kev-4b@139fdd94f1b6a6ad80cc15e08fcb99cac885a101")]
    pub installed_release: Option<String>,
    /// The pinned model package and run the last install used.
    pub latest_release: Option<String>,
    /// Whether the checkpoints of the last good install are on disk.
    pub weights_present: bool,
    /// Where the model server answers, once one is running (022, Server).
    pub endpoint: Option<String>,
    /// When the last install ended well, RFC 3339 in UTC.
    pub last_refresh_at: Option<String>,
    /// Why the last install failed.
    pub last_error: Option<String>,
}

/// Partial update of the AI permission settings; an absent field stays unchanged.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateAiPermissionsRequest {
    /// Turning it on starts an install; turning it off keeps the files.
    pub enabled: Option<bool>,
    /// Danger at or below this value is allowed. Values outside 0 to 1 are refused.
    pub allow_threshold: Option<f64>,
    /// Danger at or above this value is denied. Values outside 0 to 1 are refused.
    pub deny_threshold: Option<f64>,
    /// A flavour with no device picks the best device that runs it.
    pub flavour: Option<Flavour>,
    /// A device with no flavour keeps the stored flavour.
    pub device: Option<Device>,
}

/// One permission request to score with the AI permission model, without
/// selecting an ACP option or recording an approval.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TestAiPermissionRequest {
    /// The tool call title the model sees, such as `git status` or
    /// `Read /etc/hosts`, not the tool name.
    pub tool: String,
    /// The tool call kind the model sees.
    pub kind: Option<String>,
    /// The raw JSON input the model sees in compact form.
    pub input: serde_json::Value,
    /// The option names the model sees.
    pub options: Option<Vec<String>>,
    /// The paths the tool call touches, as an agent sends them in `locations`.
    pub locations: Option<Vec<String>>,
    /// The workspace used to derive whether a path is outside it.
    pub workspace: Option<String>,
}

/// The AI permission model's score for a request, held to the current
/// thresholds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TestAiPermissionResponse {
    /// `allow`, `ask`, or `deny` when the model answered.
    pub label: Option<String>,
    /// The model's normalized danger score when it answered.
    pub danger: Option<f64>,
    /// Danger at or below this value is allowed.
    pub allow_threshold: f64,
    /// Danger at or above this value is denied.
    pub deny_threshold: f64,
    /// Why the model did not answer.
    pub ai_error: Option<String>,
    /// The derived operation hint, when one is known.
    pub operation: Option<String>,
    /// The ordered risk tags derived from the complete request.
    pub risk_tags: Option<Vec<String>>,
    /// The first cap that changed an allow into an ask.
    pub cap: Option<String>,
    /// Kev's probabilities for the decision question.
    pub probabilities: Option<serde_json::Value>,
}

/// Why the AI permission model did not decide a `permission.replied`, in the
/// words `ariadne events`, the desktop app and the console all use:
/// `AI said ask · allow 62%, deny 5% · danger 21% (allow up to 2%, deny from 63%)`,
/// or `AI timed out`. A reply the model itself decided reads `AI allowed` or
/// `AI denied` in place of `AI said <label>`. Return `None` when the model had
/// no part in the reply.
pub fn ai_permission_note(reply: &serde_json::Value) -> Option<String> {
    let text = |key: &str| reply.get(key).and_then(serde_json::Value::as_str);
    if let (Some(label), Some(metrics)) = (text("label"), ai_permission_metrics(reply)) {
        let decision = match text("decided_by") {
            Some("ai") if label == "deny" => "AI denied".to_string(),
            Some("ai") => "AI allowed".to_string(),
            _ => format!("AI said {label}"),
        };
        return Some(format!("{decision} · {metrics}"));
    }
    text("ai_error").map(|error| format!("AI {error}"))
}

/// A probability or a danger score, rounded to a whole percent.
fn percent(value: f64) -> i64 {
    (value * 100.0).round() as i64
}

/// Format the model probabilities, danger and the two danger thresholds as
/// whole percentages, followed by the risk tags and the cap where the reply
/// carries them.
pub fn ai_permission_metrics(reply: &serde_json::Value) -> Option<String> {
    let number = |key: &str| reply.get(key).and_then(serde_json::Value::as_f64);
    let probabilities = reply.get("probabilities")?.as_object()?;
    let last = probabilities.len().checked_sub(1)?.to_string();
    let allow_probability = probabilities.get("0")?.as_f64()?;
    let deny_probability = probabilities.get(&last)?.as_f64()?;
    let danger = number("danger")?;
    let allow = number("allow_threshold")?;
    let deny = number("deny_threshold")?;
    let mut metrics = format!(
        "allow {}%, deny {}% · danger {}% (allow up to {}%, deny from {}%)",
        percent(allow_probability),
        percent(deny_probability),
        percent(danger),
        percent(allow),
        percent(deny)
    );
    if let Some(tags) = reply.get("risk_tags").and_then(serde_json::Value::as_array)
        && !tags.is_empty()
    {
        let tags = tags
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        metrics.push_str(&format!(" · tags: {tags}"));
    }
    if let Some(cap) = reply.get("cap").and_then(serde_json::Value::as_str) {
        metrics.push_str(&format!(" · capped by {cap}"));
    }
    Some(metrics)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::ai_permission_note;

    #[test]
    fn a_reply_names_why_the_model_did_not_decide_it() {
        for label in ["allow", "ask", "deny"] {
            assert_eq!(
                ai_permission_note(&json!({"label": label, "danger": 0.21,
                    "probabilities": {"0": 0.62, "1": 0.33, "2": 0.05},
                    "allow_threshold": 0.0201, "deny_threshold": 0.6321})),
                Some(format!(
                    "AI said {label} · allow 62%, deny 5% · danger 21% (allow up to 2%, deny from 63%)"
                ))
            );
        }
        assert_eq!(
            ai_permission_note(
                &json!({"decided_by": "ai", "label": "allow", "danger": 0.01,
                "probabilities": {"0": 0.99, "1": 0.01, "2": 0.0},
                "allow_threshold": 0.0201, "deny_threshold": 0.6321})
            ),
            Some(
                "AI allowed · allow 99%, deny 0% · danger 1% (allow up to 2%, deny from 63%)"
                    .into()
            )
        );
        assert_eq!(
            ai_permission_note(&json!({"decided_by": "ai", "label": "deny", "danger": 0.93,
                "probabilities": {"0": 0.01, "1": 0.11, "2": 0.88},
                "allow_threshold": 0.0201, "deny_threshold": 0.6321})),
            Some(
                "AI denied · allow 1%, deny 88% · danger 93% (allow up to 2%, deny from 63%)"
                    .into()
            )
        );
        assert_eq!(
            ai_permission_note(&json!({"label": "ask", "danger": 0.02,
                "probabilities": {"0": 0.96, "1": 0.03, "2": 0.01},
                "allow_threshold": 0.0201, "deny_threshold": 0.6321,
                "cap": "reviewer_directive", "risk_tags": ["reviewer_directive"]})),
            Some("AI said ask · allow 96%, deny 1% · danger 2% (allow up to 2%, deny from 63%) · tags: reviewer_directive · capped by reviewer_directive".into())
        );
        assert_eq!(
            ai_permission_note(&json!({"ai_error": "timed out"})),
            Some("AI timed out".into())
        );
        assert_eq!(
            ai_permission_note(&json!({"decided_by": "auto", "label": null})),
            None
        );
    }
}
