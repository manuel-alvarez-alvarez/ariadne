//! The `work` family: What got done?

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Response of `GET /v1/stats/work`: what got done?
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct WorkStatsDto {
    pub totals: WorkTotalsDto,
    /// `"day"` or `"week"`: the bucket every [`WorkBucketDto::start`] falls
    /// on.
    pub bucket: String,
    /// One row per bucket from the first fact to the last, zeros included.
    pub buckets: Vec<WorkBucketDto>,
}

/// The counts `WorkStatsDto` answers over the whole span a filter keeps.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct WorkTotalsDto {
    pub goals_completed: i64,
    pub goals_cancelled: i64,
    pub median_goal_lead_time_secs: f64,
    pub tasks_finished: i64,
    pub tasks_failed: i64,
    pub tasks_cancelled: i64,
    /// `tasks_finished` over the three endings; 0 where there are none.
    pub finish_rate: f64,
    /// Finished tasks whose `landing` was `merge` or `pull_request`.
    pub landed: i64,
}

/// One bucket of the time axis: the counts of the facts that fall in it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct WorkBucketDto {
    /// The RFC 3339 start of the bucket.
    pub start: String,
    pub tasks_finished: i64,
    pub tasks_failed: i64,
    pub tasks_cancelled: i64,
    pub goals_completed: i64,
    pub landed: i64,
}
