//! Daemon-log DTOs.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

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
}

/// Response of `GET /v1/logs`: the in-memory ring buffer, oldest first.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct LogSnapshotResponse {
    pub lines: Vec<LogLineDto>,
}
