//! The `models` family: Which model does the job?

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Response of `GET /v1/stats/models`: which model does the job? Empty until its task
/// fills it in.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ModelStatsDto {}
