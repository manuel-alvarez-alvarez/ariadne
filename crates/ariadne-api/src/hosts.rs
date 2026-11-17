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
}
