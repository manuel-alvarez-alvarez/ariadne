//! The launch file of one ACP agent session.
//!
//! The daemon's ACP adapter writes it into the session's run dir, and the ACP
//! runtime reads it back as the protocol half of the launch: what the agent
//! is told, pinned to and connected to once its process is up.

use serde::{Deserialize, Serialize};

/// The ACP launch-file format written by this build.
pub const VERSION: u32 = 1;

/// Everything the ACP runtime sends after it starts the agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchConfig {
    pub version: u32,
    pub system_prompt: String,
    pub initial_prompt: Option<String>,
    pub model: String,
    pub effort: Option<String>,
    pub resume_session_id: Option<String>,
    pub mcp_servers: Vec<McpServer>,
}

/// One standard-input MCP server passed through `session/new` or restore.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServer {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<EnvVariable>,
}

/// One environment variable for an ACP-provided MCP server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvVariable {
    pub name: String,
    pub value: String,
}
