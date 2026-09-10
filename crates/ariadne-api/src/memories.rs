//! Repository memory DTOs.

use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MemoryDto {
    pub id: String,
    pub repository_id: String,
    pub text: String,
    pub source_session_id: String,
    pub source_task_id: Option<String>,
    pub source_goal_id: String,
    pub created_at: String,
    pub expires_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateMemoryRequest {
    pub text: String,
    /// The RFC 3339 time after which this entry stays hidden.
    pub expires_at: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, IntoParams)]
#[serde(deny_unknown_fields)]
pub struct MemorySearchQuery {
    /// Find entries that contain this text, without case sensitivity.
    pub q: String,
}
