//! Conversation-message DTOs.

use ariadne_core::AuthorRole;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MessageDto {
    pub id: String,
    pub goal_id: String,
    /// None = goal-level thread.
    pub task_id: Option<String>,
    pub author_role: AuthorRole,
    pub author_session_id: Option<String>,
    pub body: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateMessageRequest {
    pub body: String,
}
