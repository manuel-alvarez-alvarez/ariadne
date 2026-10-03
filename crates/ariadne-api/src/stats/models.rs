//! The models response: comparisons within each seat.

use crate::usage::TokenUsageDto;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Models compared within each seat.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ModelStatsDto {
    pub items: Vec<ModelStatDto>,
}

/// Session measures and the measures specific to this seat.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ModelStatDto {
    pub model: String,
    pub seat: Option<String>,
    pub sessions: u64,
    pub failed_sessions: u64,
    pub stalled_sessions: u64,
    pub exhaustions: u64,
    pub usage: TokenUsageDto,
    pub cached_share: f64,
    pub mean_lifetime_secs: f64,
    pub author: Option<AuthorModelStatDto>,
    pub reviewer: Option<ReviewerModelStatDto>,
}

/// Outcomes attributed to an author model.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct AuthorModelStatDto {
    pub tasks_finished: u64,
    pub tasks_failed: u64,
    pub tasks_cancelled: u64,
    pub finish_rate: f64,
    pub first_pass_rate: f64,
    pub mean_review_rounds: f64,
    pub contests_entered: u64,
    pub contests_won: u64,
    pub win_rate: f64,
    pub tokens_per_finished_task: f64,
}

/// Verdicts attributed to a reviewer model.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ReviewerModelStatDto {
    pub verdicts: u64,
    pub approve_share: f64,
    pub mean_latency_secs: f64,
}
