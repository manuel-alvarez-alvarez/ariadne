//! Daemon-side environment report DTOs (`GET /v1/doctor`).
//!
//! What `ariadned` itself sees, which is not what the shell that asks sees: a
//! daemon started by launchd or systemd gets the PATH its service file bakes
//! in, so an agent CLI installed after the service was registered can be on
//! the user's PATH and invisible to the process that spawns sessions. The
//! daemon is the one that spawns them, so its view is the one that decides.

use ariadne_core::AgentKind;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// The daemon's own environment, as `ariadne doctor` renders it.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DaemonReportDto {
    #[schema(example = "0.1.0")]
    pub version: String,
    /// The daemon's `PATH`, the one every agent, tmux and git lookup uses.
    pub path: Option<String>,
    /// Home directory the daemon resolved, and the socket it listens on.
    pub home: String,
    pub socket_path: String,
    /// One entry per [`AgentKind`], in `AgentKind::ALL` order.
    pub agents: Vec<BinaryDto>,
    /// The other binaries a session needs: tmux and git.
    pub tools: Vec<BinaryDto>,
    pub db: PathStateDto,
    pub worktree_root: PathStateDto,
}

/// A binary as the daemon can — or cannot — find it.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct BinaryDto {
    /// Executable name as it is looked up on PATH ("claude", "tmux").
    #[schema(example = "claude")]
    pub name: String,
    /// Set for the coding-agent CLIs, absent for tmux and git.
    pub agent_kind: Option<AgentKind>,
    /// Absolute path, when it was found.
    pub path: Option<String>,
    /// First line of its version output, when it answered in time. A binary
    /// that is found but does not answer keeps its path and no version.
    pub version: Option<String>,
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
