//! Stats DTOs: what `GET /v1/stats/<family>` answers with, one shape per
//! family in a file of its own, and the filters every family takes.

mod attention;
mod models;
mod spend;
mod time;
mod tools;
mod work;

pub use attention::{
    AttentionFlagDto, AttentionStatsDto, PermissionDeciderDto, PermissionStatsDto,
};
pub use models::{
    AuthorModelStatDto, ModelInterventionsDto, ModelStatDto, ModelStatsDto, ReviewerModelStatDto,
};
pub use spend::{
    BucketDto, ModelSpendDto, PerFinishedTaskDto, SpendBucketDto, SpendStatsDto, SpendTotalsDto,
};
pub use time::{LeadTimeDto, PersonWaitDto, StatusTimeDto, TimeStatsDto};
pub use tools::{OtherToolsDto, ToolKindStatDto, ToolStatDto, ToolStatsDto, ToolsStatsQuery};
pub use work::{WorkBucketDto, WorkStatsDto, WorkTotalsDto};

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
