//! The `time` family: How long does it take?

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Response of `GET /v1/stats/time`: how long does finished work take?
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TimeStatsDto {
    pub tasks: u64,
    pub lead_time: LeadTimeDto,
    pub in_status: Vec<StatusTimeDto>,
    pub waiting_on_person: PersonWaitDto,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct LeadTimeDto {
    pub median_secs: f64,
    pub p90_secs: f64,
    pub mean_secs: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct StatusTimeDto {
    pub status: String,
    pub total_secs: f64,
    pub median_secs: f64,
    pub share: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PersonWaitDto {
    pub prompts: u64,
    pub total_secs: f64,
    pub median_secs: f64,
}
