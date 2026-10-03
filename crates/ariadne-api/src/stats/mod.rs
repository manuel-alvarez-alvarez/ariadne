//! Stats DTOs: what `GET /v1/stats/<family>` answers with, one shape per
//! family in a file of its own, and the filters every family takes.

mod attention;
mod models;
mod spend;
mod time;
mod tools;
mod work;

pub use attention::AttentionStatsDto;
pub use models::ModelStatsDto;
pub use spend::SpendStatsDto;
pub use time::TimeStatsDto;
pub use tools::ToolStatsDto;
pub use work::WorkStatsDto;

use serde::{Deserialize, Serialize};
use utoipa::IntoParams;

/// The filters every stats route takes.
#[derive(Debug, Clone, Default, Deserialize, Serialize, IntoParams)]
pub struct StatsQuery {
    /// Only facts written since then: an RFC 3339 moment, or a span back
    /// from now, `<n>m`, `<n>h`, `<n>d` or `<n>w` (`24h`, `7d`, `30d`).
    /// Absent is every fact there is.
    pub since: Option<String>,
    /// Only facts about this repository id.
    pub repo: Option<String>,
}
