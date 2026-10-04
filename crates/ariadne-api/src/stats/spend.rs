//! The `spend` family: What did it spend?

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Response of `GET /v1/stats/spend`: what did it spend? Tokens over time, by
/// model, and per finished task — never a cost.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct SpendStatsDto {
    pub totals: SpendTotalsDto,
    pub per_finished_task: PerFinishedTaskDto,
    /// The step `buckets` is drawn at.
    pub bucket: BucketDto,
    /// One row per bucket from the first fact to the last, zeros included.
    pub buckets: Vec<SpendBucketDto>,
    /// One row per model, every seat pooled, heaviest first.
    pub by_model: Vec<ModelSpendDto>,
}

/// Tokens spent across every ended session the filter keeps.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SpendTotalsDto {
    pub sessions: u64,
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub output_tokens: u64,
    /// `cached_input_tokens` over `input_tokens`; 0 where `input_tokens` is 0.
    pub cached_share: f64,
}

/// What a finished task spends, on average: the tokens of every session of a
/// task that finished, divided by how many tasks finished. 0 where none.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PerFinishedTaskDto {
    pub tasks: u64,
    pub input_tokens: f64,
    pub output_tokens: f64,
}

/// The step a time axis is drawn at: a bar a day over a short span, a bar a
/// week over a long one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum BucketDto {
    Day,
    #[default]
    Week,
}

/// What was spent in one bucket of the time axis.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SpendBucketDto {
    /// The RFC 3339 start of the bucket.
    pub start: String,
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub output_tokens: u64,
}

/// What one model spent, every seat that ran on it pooled together.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ModelSpendDto {
    pub model: String,
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub output_tokens: u64,
    /// `input_tokens + output_tokens` over the same total of every model.
    pub share: f64,
}
