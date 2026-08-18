//! Repository DTOs.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RepositoryDto {
    pub id: String,
    /// Absolute path of the checkout.
    pub path: String,
    pub base_branch: String,
    pub description: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateRepositoryRequest {
    /// Absolute path of an existing git work tree.
    #[schema(example = "/home/me/dev/ariadne")]
    pub path: String,
    /// Omit for the repo's currently checked-out branch.
    pub base_branch: Option<String>,
    pub description: Option<String>,
}

/// Partial update; absent fields stay unchanged.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct UpdateRepositoryRequest {
    pub path: Option<String>,
    pub base_branch: Option<String>,
    /// New description, or empty to clear it. Absent = unchanged.
    pub description: Option<String>,
}
