//! The launch file shared by the ACP adapter and the CLI-side ACP client.
//!
//! The daemon writes this beside the normal spawn plan. The `ariadne _spawn`
//! process reads it, launches the configured `acp` agent, and speaks ACP over
//! that process's standard input and output.

use serde::{Deserialize, Serialize};

/// The spawn-plan environment key that selects the ACP client path.
pub const CONFIG_ENV: &str = "ARIADNE_ACP_CONFIG";

/// The ACP launch-file format written by this build.
pub const VERSION: u32 = 1;

/// Everything the CLI-side ACP client sends after it starts the agent.
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
    pub event_sink: Hook,
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

/// The command invoked when the ACP client maps a protocol event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hook {
    pub command: String,
    pub args: Vec<String>,
}
