//! Repository DTOs.
//!
//! A repository is a checkout and a base branch. How a change *reaches* that
//! base branch is not here: that is the task's own `landing`, agreed with the
//! user task by task, and the procedure it names is Ariadne's own.

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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub struct UpdateRepositoryRequest {
    pub path: Option<String>,
    pub base_branch: Option<String>,
    /// New description, or empty to clear it. Absent = unchanged.
    pub description: Option<String>,
}
