//! Repository DTOs.
//!
//! A repository is a checkout and a base branch. It also supplies the landing
//! that a new goal uses where its request does not name one.

use ariadne_core::{ForgeKind, Landing, PermissionMode};
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
    /// The forge its remote is on, or null where the checkout has no usable
    /// remote (025).
    pub forge: Option<ForgeDto>,
    pub created_at: String,
    pub updated_at: String,
}

/// Public hook status. The hook secret is never serialized.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct WebhookDto {
    pub state: String,
    pub url: Option<String>,
    pub error: Option<String>,
    pub last_delivery_at: Option<String>,
}

/// Where the webhook tunnel stands (027).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum TunnelState {
    /// The tunnel is open, and its URL is the hooks' URL.
    Up,
    /// The tunnel should run and does not: the fetch runs on its timer.
    Down,
    /// No tunnel runs: the switch is off, `webhook_public_url` is set, or no
    /// integration is enabled.
    Off,
}

/// The forge settings and the tunnel state, as `GET /v1/forge/tunnel` and
/// `forge_settings_updated` carry them (027).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ForgeTunnelDto {
    /// The switch: whether the daemon opens a tunnel.
    pub enabled: bool,
    pub state: TunnelState,
    /// The public URL while the tunnel is up.
    pub url: Option<String>,
    /// The bound webhook listener address the tunnel forwards to.
    #[schema(example = "127.0.0.1:49152")]
    pub listen: Option<String>,
    /// When the tunnel entered its state.
    pub since: String,
    /// Why the last attempt failed, while the tunnel is down.
    pub error: Option<String>,
}

/// `PUT /v1/forge/tunnel`: turn the tunnel on or off.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SetTunnelRequest {
    pub enabled: bool,
}

/// The forge a repository's remote is on, and whether Ariadne works with it.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ForgeDto {
    pub webhook: WebhookDto,
    pub kind: ForgeKind,
    /// Lower-cased, like `owner` and `name`.
    #[schema(example = "github.com")]
    pub host: String,
    pub owner: String,
    pub name: String,
    /// The remote it was read off, `origin` where there is one.
    pub remote: String,
    pub enabled: bool,
    /// The account the forge CLI is signed in as, stored on enable.
    pub login: Option<String>,
    /// The pin of the session that watches a published request; null starts
    /// none.
    pub babysit_model: Option<String>,
    pub babysit_effort: Option<String>,
    /// The pin of the session that reviews a request; null starts none.
    pub review_model: Option<String>,
    pub review_effort: Option<String>,
}

/// A change to the forge integration; absent fields stay unchanged. A model
/// written empty clears that role's pin and its effort.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ForgeUpdate {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub babysit_model: Option<String>,
    #[serde(default)]
    pub babysit_effort: Option<String>,
    #[serde(default)]
    pub review_model: Option<String>,
    #[serde(default)]
    pub review_effort: Option<String>,
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
    /// The forge integration to set up once the remote is detected.
    #[serde(default)]
    pub forge: Option<ForgeUpdate>,
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
    /// Absent = unchanged.
    #[serde(default)]
    pub forge: Option<ForgeUpdate>,
}
