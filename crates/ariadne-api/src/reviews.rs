//! Review DTOs.

use ariadne_core::{AuthorRole, ReviewVerdict};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ReviewDto {
    pub id: String,
    pub task_id: String,
    pub round: i64,
    /// The task profile whose verdict this is. None exactly when
    /// `author_role` names an author that is nobody's profile.
    pub reviewer_profile_id: Option<String>,
    /// The role that wrote it where no profile did: `forge`, for what the
    /// people reading a published request wrote and the daemon relayed.
    pub author_role: Option<AuthorRole>,
    pub session_id: Option<String>,
    pub verdict: ReviewVerdict,
    pub body: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateReviewRequest {
    pub verdict: ReviewVerdict,
    pub body: Option<String>,
    /// Reviewer profile id or name. Derived from the session context when the
    /// call comes from an agent; required for user-submitted reviews.
    pub reviewer_profile: Option<String>,
}
