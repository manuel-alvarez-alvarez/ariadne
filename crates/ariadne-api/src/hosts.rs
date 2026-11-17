//! Host DTOs (037).
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// How a host is reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum HostKind {
    /// The daemon answering this request.
    Local,
    /// An `http://` address or a socket path the client reaches directly.
    Direct,
    /// A daemon reached through an SSH forward.
    Ssh,
}

/// Whether a client can use a host's endpoint now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum HostStatus {
    Ready,
    Connecting,
    Down,
}

/// One host a client can address: this daemon, or one `config.toml` lists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct HostDto {
    pub name: String,
    pub kind: HostKind,
    /// The address a client on this machine uses. For an `ssh` host it is
    /// null until a forward is up.
    pub endpoint: Option<String>,
    pub status: HostStatus,
    /// Why the host is down, when it is.
    pub error: Option<String>,
    /// The far daemon's address, for an `ssh` host with one configured.
    pub remote: Option<String>,
    /// How far the host has taken this daemon's catalogs and settings.
    pub sync: HostSyncDto,
}

/// Whether a host holds what this daemon last pushed to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SyncState {
    /// Every local change has reached the host.
    InSync,
    /// A change waits for the host, or a push is under way.
    Pending,
    /// The last push failed. It is tried again while the host is `ready`.
    Failed,
}

/// The sync of one host (037).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct HostSyncDto {
    pub state: SyncState,
    /// Why the last push failed, when it did.
    pub error: Option<String>,
    /// The local repositories that have no checkout of the same forge
    /// repository on the host. Nothing is registered for them.
    pub missing_repositories: Vec<MissingRepositoryDto>,
}

impl HostSyncDto {
    /// A sync with nothing to push and nothing missing.
    pub fn in_sync() -> Self {
        Self {
            state: SyncState::InSync,
            error: None,
            missing_repositories: Vec::new(),
        }
    }
}

/// A local repository a host has no checkout of.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct MissingRepositoryDto {
    /// The local repository's id.
    pub id: String,
    /// `owner/repo` on its forge; null for a repository with no forge.
    pub forge: Option<String>,
}
