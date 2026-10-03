//! The `time` family: How long does it take?

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Response of `GET /v1/stats/time`: how long does it take? Empty until its task
/// fills it in.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TimeStatsDto {}
