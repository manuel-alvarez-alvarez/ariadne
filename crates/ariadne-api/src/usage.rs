//! Token-usage DTOs: the counters every entity that spends tokens is read
//! with.
//!
//! One shape, wherever it appears — a session's own, a task's engineer and
//! reviewers, a goal's roles — so a reader that can render one can render all
//! of them.

use ariadne_core::TokenUsage;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Tokens spent, as the agents' own transcripts report them.
///
/// Always present and always a number: nothing reported is zero, not null.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct TokenUsageDto {
    /// Prompt tokens, cache reads and cache writes included.
    pub input_tokens: u64,
    /// The subset of `input_tokens` served from the prompt cache, so never
    /// added to it.
    pub cached_input_tokens: u64,
    /// Completion tokens, thinking and reasoning included.
    pub output_tokens: u64,
}

impl From<TokenUsage> for TokenUsageDto {
    fn from(usage: TokenUsage) -> Self {
        Self {
            input_tokens: usage.input_tokens,
            cached_input_tokens: usage.cached_input_tokens,
            output_tokens: usage.output_tokens,
        }
    }
}
