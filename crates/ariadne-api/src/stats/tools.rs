//! The `tools` family: What do the agents do?

use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

/// The query `GET /v1/stats/tools` takes: the two filters every family
/// takes, and how many of the top tools to list.
#[derive(Debug, Clone, Default, Deserialize, Serialize, IntoParams)]
pub struct ToolsStatsQuery {
    /// Only facts written since then: an RFC 3339 moment, or a span back
    /// from now, `<n>m`, `<n>h`, `<n>d` or `<n>w` (`24h`, `7d`, `30d`).
    /// Absent is every fact there is.
    pub since: Option<String>,
    /// Only facts about this repository id.
    pub repo: Option<String>,
    /// How many of the top tools to list, 1 to 100. Defaults to 10.
    pub limit: Option<u32>,
}

/// How one kind of tool call performed: how often it ran, how many failed,
/// and how long its calls took.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ToolKindStatDto {
    pub kind: String,
    pub calls: u64,
    pub errors: u64,
    pub median_duration_ms: f64,
    pub p90_duration_ms: f64,
}

/// How one tool performed: how often it ran, how many of its calls failed,
/// and how long they took.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ToolStatDto {
    pub tool_name: String,
    pub kind: String,
    pub calls: u64,
    pub errors: u64,
    pub median_duration_ms: f64,
    pub p90_duration_ms: f64,
}

/// The tools beyond the query's `limit`, summed into one row.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct OtherToolsDto {
    pub tools: u64,
    pub calls: u64,
    pub errors: u64,
}

/// Response of `GET /v1/stats/tools`: what do the agents do?
///
/// `#[serde(default)]`: a shared stats CLI test answers every family's route
/// with `{}`; the other five families are empty structs today and the
/// fields here fall back to [`ToolStatsDto::default`] the same way.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct ToolStatsDto {
    pub calls: u64,
    pub errors: u64,
    /// The count of distinct tool names.
    pub tools: u64,
    /// One row per kind, the most calls first.
    pub by_kind: Vec<ToolKindStatDto>,
    /// The tools with the most calls, up to the query's `limit`.
    pub top: Vec<ToolStatDto>,
    pub other: OtherToolsDto,
}
