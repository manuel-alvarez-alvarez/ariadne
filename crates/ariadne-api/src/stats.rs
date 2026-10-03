//! Stats DTOs: what `GET /v1/stats/<family>` answers with, one shape per
//! family, and the filters every family takes.

use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::usage::TokenUsageDto;

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

/// One skill the sessions of a row loaded, and how many of them loaded it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct SkillCountDto {
    pub name: String,
    pub sessions: u64,
}

/// How one model did in one seat, over the session runs that ended.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ModelStatDto {
    /// `<agent>:<model>`, as the sessions ran it.
    pub model: String,
    /// `orchestrator`, `author` or `reviewer`; null for a loose session.
    pub seat: Option<String>,
    /// Session runs that ended.
    pub sessions: u64,
    /// Of them, the runs that ended `failed`.
    pub failed: u64,
    /// Of them, the runs that ended flagged `stalled`.
    pub stalled: u64,
    /// What those runs spent, summed.
    pub usage: TokenUsageDto,
    /// `cached_input_tokens` over `input_tokens`, from 0 to 1; 0 where
    /// nothing went in.
    pub cached_share: f64,
    /// The mean time from a session's creation to its end, in seconds.
    pub mean_lifetime_secs: f64,
    /// Each skill those runs loaded, the most loaded first.
    pub skills: Vec<SkillCountDto>,
}

/// Response of `GET /v1/stats/models`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ModelStatsResponse {
    /// One row per model and seat, ordered by model and then by seat.
    pub items: Vec<ModelStatDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct AuthorReviewStatDto {
    pub model: String,
    pub approvals: u64,
    pub mean_rounds: f64,
    pub median_rounds: f64,
    pub first_pass_rate: f64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ReviewerStatDto {
    pub model: String,
    pub verdicts: u64,
    pub approve_share: f64,
    pub mean_latency_secs: f64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MessageStatDto {
    pub kind: String,
    pub from_actor: String,
    pub total: u64,
    pub mean_per_task: f64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ReviewStatsDto {
    pub authors: Vec<AuthorReviewStatDto>,
    pub reviewers: Vec<ReviewerStatDto>,
    pub messages: Vec<MessageStatDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ToolStatDto {
    pub tool_name: String,
    pub calls: u64,
    pub errors: u64,
    pub median_duration_ms: f64,
    pub p90_duration_ms: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ToolModelStatDto {
    pub model: String,
    pub calls: u64,
    pub mean_duration_ms: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PermissionStatDto {
    pub decided_by: String,
    pub answer: String,
    pub permissions: u64,
    pub mean_wait_ms: f64,
}

/// Response of `GET /v1/stats/tools`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ToolStatsDto {
    pub tools: Vec<ToolStatDto>,
    pub models: Vec<ToolModelStatDto>,
    pub permissions: Vec<PermissionStatDto>,
}

/// One reason sessions left a model, and how many switches gave it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct SwitchReasonCountDto {
    pub reason: String,
    pub switches: u64,
}

/// How one model did as sessions left it or arrived on it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SwitchStatDto {
    /// `<agent>:<model>`.
    pub model: String,
    /// Sessions that left this model.
    pub switches: u64,
    /// Each reason a switch left this model, the most common first.
    pub by_reason: Vec<SwitchReasonCountDto>,
    /// Of `switches`, those left for `exhausted`.
    pub exhaustions: u64,
    /// The automatic share of `switches`, from 0 to 1; 0 where it had none.
    pub automatic_share: f64,
    /// Sessions that arrived on this model from another.
    pub arrivals: u64,
}

/// Response of `GET /v1/stats/switches`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SwitchStatsResponse {
    /// One row per model, ordered by model.
    pub items: Vec<SwitchStatDto>,
    /// Switches over every row.
    pub switches: u64,
    /// Exhaustions over every row.
    pub exhaustions: u64,
}
