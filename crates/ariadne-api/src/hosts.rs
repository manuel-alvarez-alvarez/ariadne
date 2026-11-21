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
    /// Selected source repository IDs and absolute destination paths on the worker.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub repositories: std::collections::BTreeMap<String, String>,
}

/// Explicit updates use the selected worker's saved SSH connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<BinarySource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<SetupTools>,
}

/// The worker's explicit maintenance hold and drain state.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MaintenanceDto {
    pub active: bool,
    pub ready: bool,
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
    /// What the step does, in plain words.
    #[serde(default)]
    #[schema(required = true)]
    pub title: String,
    /// The work the user does by hand when the step waits for it. It is the
    /// same as the `manual` of the plan item of this step.
    #[serde(default)]
    #[schema(required = true)]
    pub manual: Option<ManualAction>,
}

/// Who installs one item of a setup on the new machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SetupGroup {
    /// Ariadne installs or copies the item during setup.
    AriadneInstalls,
    /// The item is already on the new machine. Nobody installs it.
    AlreadyPresent,
    /// The user installs or signs in by hand on the new machine.
    Manual,
}

/// Work the user does by hand on the new machine. It never holds a credential value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ManualAction {
    /// Why Ariadne cannot do this, and what to do, in plain words.
    pub reason: String,
    /// The exact command to run on the new machine, or null where no command exists.
    pub command: Option<String>,
}

/// Durable setup progress. No credential values are accepted or returned.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SetupDto {
    pub name: String,
    pub state: SetupState,
    pub steps: Vec<SetupStep>,
}

/// Which kind of saved operation a [`HostSetupDto`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SetupOperation {
    Setup,
    Update,
}

/// One saved setup or update. Credential references and values never appear.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct HostSetupDto {
    pub name: String,
    pub operation: SetupOperation,
    pub ssh: String,
    pub state: SetupState,
    pub steps: Vec<SetupStep>,
    pub tools: Option<SetupTools>,
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

/// Read-only inspection needs only an account with noninteractive SSH access.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct InspectRequest {
    pub ssh: String,
}

/// A capability or prerequisite. Reports contain guidance, never command output.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SetupCheck {
    pub id: String,
    pub satisfied: bool,
    pub missing: String,
    pub resolution: String,
    pub recheck: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct InspectionDto {
    pub target: String,
    pub checks: Vec<SetupCheck>,
    pub supported_agents: Vec<String>,
    pub supported_credentials: Vec<String>,
    /// Each agent that a setup request can name.
    #[serde(default)]
    #[schema(required = true)]
    pub agents: Vec<InspectedAgent>,
    /// Who installs GitHub CLI on the inspected machine.
    #[serde(default = "github_unknown")]
    #[schema(required = true)]
    pub github: InspectedGithub,
    /// Each sign-in that a setup request can name in `credentials`.
    #[serde(default)]
    #[schema(required = true)]
    pub sign_ins: Vec<SignIn>,
}

/// One agent that setup can install or that the user installs by hand.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct InspectedAgent {
    pub id: String,
    /// The display name.
    pub name: String,
    /// Whether this daemon can run the agent on its own machine now.
    pub on_this_machine: bool,
    /// Who installs the agent on the inspected machine.
    pub group: SetupGroup,
    pub manual: Option<ManualAction>,
    /// The `id` of the `sign_ins` entry the agent uses, or null.
    pub sign_in: Option<String>,
}

/// GitHub CLI on the inspected machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct InspectedGithub {
    pub group: SetupGroup,
    pub manual: Option<ManualAction>,
}

/// The value an older daemon's inspection, which has no `github`, reads as.
fn github_unknown() -> InspectedGithub {
    InspectedGithub {
        group: SetupGroup::Manual,
        manual: None,
    }
}

/// A sign-in service. It never carries a credential value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct SignIn {
    /// The reference a setup request names in `credentials`.
    pub id: String,
    pub name: String,
    /// Whether Ariadne can copy this sign-in from this machine.
    pub copy_supported: bool,
    /// The command that signs in on the new machine, or null where none exists.
    pub command: Option<String>,
}

/// One thing that a setup request causes, and who does it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct PlanItem {
    /// The name of the progress step that does the item.
    pub id: String,
    pub title: String,
    pub group: SetupGroup,
    /// What happens, in plain words.
    pub detail: String,
    /// The work by hand, for an item in the `manual` group. Null otherwise.
    pub manual: Option<ManualAction>,
}

/// Review of selected actions. Inspection and review never persist an operation.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SetupReviewDto {
    pub inspection: InspectionDto,
    pub actions: Vec<String>,
    pub prerequisites: Vec<SetupCheck>,
    pub preserved: Vec<String>,
    /// One item for each thing that the request causes, each in one group.
    #[serde(default)]
    #[schema(required = true)]
    pub plan: Vec<PlanItem>,
}
