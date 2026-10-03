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
