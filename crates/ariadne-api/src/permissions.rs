//! The AI permission model, the local model that answers permission requests
//! in the `ai` permission mode (022).
//!
//! One settings row behind `/v1/permissions/ai`, the Python interpreter the
//! daemon found, and where the install has got to. The install is a Python
//! package and its weights, so it runs in the background: a
//! write answers with `installing` and the `ai_permissions_updated` event
//! says how it ended.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

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
    /// Whether it is Python 3.12 or 3.13, which the model needs.
    pub ok: bool,
}

/// The AI permission settings and the state of the install behind them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct AiPermissionsStatusDto {
    /// Whether the model answers permission requests at all.
    pub enabled: bool,
    /// How sure the model has to be before its answer is taken, 0 to 1.
    #[schema(example = 0.8)]
    pub threshold: f64,
    /// When the daily refresh runs, `HH:MM` in 24-hour local time. `null`
    /// turns the refresh off.
    #[schema(example = "03:30")]
    pub schedule: Option<String>,
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

/// Why the AI permission model did not decide a `permission.replied`, in the
/// words `ariadne events`, the desktop app and the console all use:
/// `guardrail credential-paths`, `AI said escalate (0.41, threshold 0.70)`,
/// `AI timed out`. `None` where the model had no part in the reply: a mode
/// other than `ai`, or a reply the model made itself.
pub fn ai_permission_note(reply: &serde_json::Value) -> Option<String> {
    let text = |key: &str| reply.get(key).and_then(serde_json::Value::as_str);
    if let Some(guardrail) = text("guardrail") {
        return Some(format!("guardrail {guardrail}"));
    }
    if let (Some(label), Some(confidence)) = (
        text("label"),
        reply.get("confidence").and_then(serde_json::Value::as_f64),
    ) {
        return Some(
            match reply.get("threshold").and_then(serde_json::Value::as_f64) {
                Some(threshold) => {
                    format!("AI said {label} ({confidence:.2}, threshold {threshold:.2})")
                }
                None => format!("AI said {label} ({confidence:.2})"),
            },
        );
    }
    text("ai_error").map(|error| format!("AI {error}"))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::ai_permission_note;

    #[test]
    fn a_reply_names_why_the_model_did_not_decide_it() {
        assert_eq!(
            ai_permission_note(&json!({"guardrail": "credential-paths",
                                        "label": null, "ai_error": null})),
            Some("guardrail credential-paths".into())
        );
        assert_eq!(
            ai_permission_note(&json!({"label": "escalate", "confidence": 0.4129,
                                        "threshold": 0.7})),
            Some("AI said escalate (0.41, threshold 0.70)".into())
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
