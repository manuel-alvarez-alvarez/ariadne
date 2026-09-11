//! Daemon-side environment report DTOs (`GET /v1/doctor`).
//!
//! What `ariadned` itself sees, which is not what the shell that asks sees: a
//! daemon started by launchd or systemd gets the PATH its service file bakes
//! in, and it is the one that spawns sessions, so its view decides.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::agents::AcpAgentDto;

/// The daemon's own environment, as `ariadne doctor` renders it.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DaemonReportDto {
    #[schema(example = "0.1.0")]
    pub version: String,
    /// The daemon's `PATH`, the one every agent and git lookup uses.
    pub path: Option<String>,
    /// Home directory the daemon resolved, and the socket it listens on.
    pub home: String,
    pub socket_path: String,
    /// Every registry ACP agent and its cached discovery result: the agents
    /// a session can be spawned on.
    #[serde(default)]
    pub acp_agents: Vec<AcpAgentDto>,
    /// The other binaries the daemon runs: git, without which no worktree can
    /// be cut at all, and the forge CLIs `gh` and `glab`, which are what a
    /// published task is watched through.
    pub tools: Vec<BinaryDto>,
    pub db: PathStateDto,
    pub worktree_root: PathStateDto,
}

/// A binary as the daemon can — or cannot — find it.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct BinaryDto {
    /// Executable name as it is looked up on PATH ("git", "gh").
    #[schema(example = "git")]
    pub name: String,
    /// Absolute path, when it was found.
    pub path: Option<String>,
    /// First line of its version output, when it answered in time. A binary
    /// that is found but does not answer keeps its path and no version.
    pub version: Option<String>,
    /// Whether it holds credentials for the service it speaks to, for the
    /// binaries that hold any: `gh auth status` and `glab auth status`, asked
    /// of the daemon's own environment because that is where the polling
    /// runs. `None` for a binary with nothing to sign in to — git — and for
    /// one that was not found to ask.
    ///
    /// The distinction it exists for is the one that used to be invisible: a
    /// `gh` that is installed and signed out answers every poll of a pull
    /// request with a failure, and a task published to a forge is then
    /// watched by nothing.
    pub authenticated: Option<bool>,
}

/// A file or directory the daemon depends on.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PathStateDto {
    pub path: String,
    pub exists: bool,
    /// Whether the daemon may write it, asked of the kernel (`access(2)`)
    /// rather than inferred from the permission bits, which say nothing
    /// about the user the daemon happens to run as. For a path that does not
    /// exist yet this is its directory's answer: whether it could be created.
    /// Nothing is written to find out.
    pub writable: bool,
}
