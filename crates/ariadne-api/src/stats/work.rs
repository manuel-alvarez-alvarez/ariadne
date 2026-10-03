//! The `work` family: What got done?

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Response of `GET /v1/stats/work`: what got done? Empty until its task
/// fills it in.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct WorkStatsDto {}
