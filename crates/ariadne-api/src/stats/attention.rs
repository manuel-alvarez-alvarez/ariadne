//! The `attention` family: How much did it need me?

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Response of `GET /v1/stats/attention`: how much did it need me? Empty until its task
/// fills it in.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct AttentionStatsDto {}
