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
    /// The sum of the lifetimes of the ended sessions.
    pub total_lifetime_secs: f64,
    pub interventions: ModelInterventionsDto,
    pub author: Option<AuthorModelStatDto>,
    pub reviewer: Option<ReviewerModelStatDto>,
}

/// The times a person stepped in for this model in this seat: permissions
/// decided at the console, questions and stalls. `person_secs` is how long
/// those waited on the person.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ModelInterventionsDto {
    pub permissions: u64,
    pub questions: u64,
    pub stalls: u64,
    pub total: u64,
    pub person_secs: f64,
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
    pub median_lead_time_secs: f64,
    /// Interventions divided by finished tasks; null without a finished task.
    pub interventions_per_finished_task: Option<f64>,
}

/// Verdicts attributed to a reviewer model.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ReviewerModelStatDto {
    pub verdicts: u64,
    pub approve_share: f64,
    pub mean_latency_secs: f64,
}
