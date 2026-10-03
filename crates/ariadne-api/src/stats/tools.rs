//! The `tools` family: What do the agents do?

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Response of `GET /v1/stats/tools`: what do the agents do? Empty until its task
/// fills it in.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ToolStatsDto {}
