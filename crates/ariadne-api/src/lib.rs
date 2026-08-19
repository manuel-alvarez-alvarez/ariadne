//! REST API data-transfer objects.
//!
//! Single source of truth for wire types: the daemon serializes them, the
//! client deserializes them, utoipa derives the OpenAPI schemas from them.

pub mod agents;
pub mod error;
pub mod events;
pub mod goals;
pub mod logs;
pub mod messages;
pub mod models;
pub mod profiles;
pub mod repositories;
pub mod reviews;
pub mod sessions;
pub mod stream;
pub mod tasks;

use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

/// Response of `GET /v1/health`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct HealthResponse {
    /// Always "ok" when the daemon is able to answer.
    #[schema(example = "ok")]
    pub status: String,
    /// Seconds since the daemon started.
    pub uptime_secs: u64,
}

/// Response of `GET /v1/version`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct VersionResponse {
    #[schema(example = "ariadned")]
    pub name: String,
    #[schema(example = "0.1.0")]
    pub version: String,
}

/// Keyset pagination query parameters shared by list endpoints.
///
/// Ids are ULIDs (time-sortable), so `after=<last-seen-id>` pages forward in
/// creation order.
#[derive(Debug, Clone, Default, Deserialize, Serialize, IntoParams)]
pub struct Page {
    /// Return items with id greater than this.
    pub after: Option<String>,
    /// Max items to return (default 50, cap 200).
    pub limit: Option<i64>,
}

impl Page {
    pub fn limit(&self) -> i64 {
        self.limit.unwrap_or(50).clamp(1, 200)
    }
}

/// Header carrying the agent-session context on MCP/agent-originated calls.
pub const SESSION_HEADER: &str = "x-ariadne-session";
