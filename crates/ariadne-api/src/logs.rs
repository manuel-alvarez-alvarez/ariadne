//! Daemon-log DTOs.

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::{IntoParams, ToSchema};

/// One captured daemon log line.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct LogLineDto {
    /// When the event was recorded, RFC 3339.
    #[schema(example = "2026-08-18T12:34:56.789012Z")]
    pub ts: String,
    /// Log level as tracing prints it.
    #[schema(example = "INFO")]
    pub level: String,
    /// Module path the event was emitted from.
    #[schema(example = "ariadne_daemon::scheduler")]
    pub target: String,
    /// Message followed by the event's fields as ` key=value` pairs.
    pub message: String,
    /// The emitting resource's `service.name` (`ariadned`, `ariadne-desktop`,
    /// `ariadne-cli`).
    #[schema(example = "ariadned")]
    pub service: String,
}

/// Response of `GET /v1/logs`: the telemetry store's tail, oldest first.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct LogSnapshotResponse {
    pub lines: Vec<LogLineDto>,
}

/// The message a record's body and attributes render as: the body, then
/// every attribute appended as ` key=value` — the shape the daemon's old
/// in-process capture layer rendered live, now rebuilt from what the
/// telemetry store holds. The one place this is done, shared by the
/// snapshot and stream routes, the live-feed exporter, and a test that
/// greps the daemon's own log for a line it expects.
pub fn render_message(body: Option<&str>, attributes: &Value) -> String {
    let mut message = body.unwrap_or_default().to_string();
    if let Some(object) = attributes.as_object() {
        for (key, value) in object {
            let rendered = match value {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            let _ = write!(message, " {key}={rendered}");
        }
    }
    message
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, IntoParams)]
pub struct LogsQuery {
    /// Return only the last N lines.
    pub tail: Option<usize>,
    /// Only lines from this `service.name`.
    #[param(example = "ariadned")]
    pub service: Option<String>,
    /// Only lines at or above this level (`"info"`, `"warn"`, ...).
    #[param(example = "warn")]
    pub level: Option<String>,
    /// Only lines at or after this RFC 3339 timestamp.
    #[param(example = "2026-08-18T12:34:56.789012Z")]
    pub since: Option<String>,
}
