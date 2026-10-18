//! The models response: comparisons within each seat.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Models compared within each seat.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ModelStatsDto {
    pub items: Vec<ModelStatDto>,
}

/// What one model did in one seat.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ModelStatDto {
    pub model: String,
    pub seat: Option<String>,
    /// Distinct tasks with a session of this model in this seat.
    pub tasks: u64,
    /// Distinct goals with a session of this model in this seat.
    pub goals: u64,
    /// Input and output tokens of the ended sessions. Cached tokens are part of input.
    pub tokens: u64,
    /// The sum of the session lifetimes.
    pub time_secs: f64,
    /// The messages this model sent in this seat.
    pub messages: u64,
    /// The tasks this model's column ended `finished`.
    pub tasks_finished: u64,
}
