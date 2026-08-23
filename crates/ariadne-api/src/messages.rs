//! Conversation-message DTOs.

use ariadne_core::{AuthorRole, RecipientKind};
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
    /// Whom the message addresses. None = the thread, addressed to nobody in
    /// particular.
    pub recipient: Option<MessageRecipientDto>,
    pub body: String,
    pub created_at: String,
}

/// A message's addressee, resolved: an agent profile comes with its name, so a
/// client renders "to Alice" without a lookup of its own.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MessageRecipientDto {
    pub kind: RecipientKind,
    /// The addressed profile, set exactly when `kind` is `profile`.
    pub profile_id: Option<String>,
    /// That profile's name, unless the profile is gone.
    pub profile_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateMessageRequest {
    pub body: String,
    /// Whom to address: a profile id or name, as tasks name their profiles, or
    /// the literal `"user"`. Omitted leaves the message addressed to the
    /// thread. Only a participant of the thread may be addressed.
    #[schema(example = "reviewer-default")]
    pub to: Option<String>,
}
