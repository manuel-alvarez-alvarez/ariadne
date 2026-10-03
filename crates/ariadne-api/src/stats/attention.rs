//! The `attention` family: How much did it need me?

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Response of `GET /v1/stats/attention`: how much did it need me?
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct AttentionStatsDto {
    pub permissions: PermissionStatsDto,
    pub flags: Vec<AttentionFlagDto>,
    pub sessions_failed: u64,
    pub sessions_stalled: u64,
    pub exhaustions: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PermissionStatsDto {
    pub total: u64,
    pub person_share: f64,
    pub by_decider: Vec<PermissionDeciderDto>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PermissionDeciderDto {
    pub decided_by: String,
    pub total: u64,
    pub allowed: u64,
    pub denied: u64,
    pub cancelled: u64,
    pub mean_wait_ms: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct AttentionFlagDto {
    pub reason: String,
    pub raised: u64,
    pub mean_wait_secs: f64,
}
