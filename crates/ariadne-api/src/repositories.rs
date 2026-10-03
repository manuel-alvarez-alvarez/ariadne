//! Repository DTOs.
//!
//! A repository is a checkout and a base branch. It also supplies the landing
//! that a new goal uses where its request does not name one.

use ariadne_core::{Landing, PermissionMode};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RepositoryDto {
    pub id: String,
    /// Absolute path of the checkout.
    pub path: String,
    pub base_branch: String,
    pub description: Option<String>,
    /// How the ACP permission requests of every session in this checkout are
    /// answered.
    pub permission_mode: PermissionMode,
    /// The landing a new goal uses where its request leaves landing out.
    pub default_landing: Landing,
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
    /// Omit for `auto`.
    #[serde(default)]
    pub permission_mode: Option<PermissionMode>,
    /// Omit for `merge`.
    #[serde(default)]
    pub default_landing: Option<Landing>,
}

/// Partial update; absent fields stay unchanged.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateRepositoryRequest {
    pub path: Option<String>,
    pub base_branch: Option<String>,
    /// New description, or empty to clear it. Absent = unchanged.
    pub description: Option<String>,
    /// Absent = unchanged.
    #[serde(default)]
    pub permission_mode: Option<PermissionMode>,
    /// Absent = unchanged.
    #[serde(default)]
    pub default_landing: Option<Landing>,
}
