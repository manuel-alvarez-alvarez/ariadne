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

/// A host's optional colour override.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum HostColor {
    Red,
    Orange,
    Amber,
    Green,
    Teal,
    Blue,
    Violet,
    Pink,
}

/// One host a client can address: this daemon, or one the database stores.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct HostDto {
    pub name: String,
    pub color: Option<HostColor>,
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
    /// Configuration is saved and waits for an idle restart.
    PendingRestart,
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
    /// How many local learned permissions are not pushed, because the host
    /// has no checkout of their repository.
    pub skipped_permissions: u32,
}

impl HostSyncDto {
    /// A sync with nothing to push and nothing missing.
    pub fn in_sync() -> Self {
        Self {
            state: SyncState::InSync,
            error: None,
            missing_repositories: Vec::new(),
            skipped_permissions: 0,
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

/// An SSH worker installation request. SSH uses the daemon user's keys and config.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SetupRequest {
    pub ssh: String,
    pub remote: Option<String>,
    pub source: BinarySource,
    /// Selected registry agent IDs and forge tooling. Omission preserves binary-only setup.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<SetupTools>,
    /// Source credential references only: `codex` and `github`. Never send credential values.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub credentials: Vec<String>,
}

/// Binaries on the daemon machine, or assets from an explicitly selected release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum BinarySource {
    Local { cli: String, daemon: String },
    Release { repository: String, tag: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SetupState {
    Pending,
    Running,
    Incomplete,
    Failed,
    Complete,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SetupStep {
    pub name: String,
    pub complete: bool,
    pub error: Option<String>,
}

/// Durable setup progress. No credential values are accepted or returned.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SetupDto {
    pub name: String,
    pub state: SetupState,
    pub steps: Vec<SetupStep>,
}

/// Settings shared between machines. Worker locations never enter this payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PortableConfigDto {
    pub delete_merged_branches: bool,
    pub delete_merged_worktrees: bool,
    pub auto_switch: bool,
    pub exhausted_patterns: Vec<String>,
    pub log_filter: String,
    pub telemetry_samples_days: i64,
    pub telemetry_logs_days: i64,
    pub telemetry_rollups_days: i64,
    pub acp_registry_url: String,
    pub acp_agents: Vec<PortableAgentDto>,
    /// Only explicitly configured flags participate in synchronization.
    pub agent_flags: std::collections::BTreeMap<String, Vec<String>>,
}

/// An agent command, keyed by its stable registry identifier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PortableAgentDto {
    pub id: String,
    pub command: Vec<String>,
}

/// The worker's effective live settings and pending registry changes.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ConfigSyncDto {
    pub state: SyncState,
    pub error: Option<String>,
    pub configuration: PortableConfigDto,
}

/// Programs required on the worker. Git is always required when this section is present.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SetupTools {
    #[serde(default)]
    pub agents: Vec<String>,
    #[serde(default)]
    pub github: bool,
}
