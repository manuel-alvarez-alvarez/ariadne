//! Review DTOs.

use ariadne_core::ReviewVerdict;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ReviewDto {
    pub id: String,
    pub task_id: String,
    pub round: i64,
    /// The reviewer of the round whose verdict this is.
    pub reviewer_profile_id: String,
    pub session_id: Option<String>,
    pub verdict: ReviewVerdict,
    pub body: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateReviewRequest {
    pub verdict: ReviewVerdict,
    pub body: Option<String>,
    /// Reviewer profile id or name. Derived from the session context when the
    /// call comes from an agent; required for user-submitted reviews.
    pub reviewer_profile: Option<String>,
}
