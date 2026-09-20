//! Ariadne's launch types as the ACP SDK's typed v1 schema.
//!
//! The daemon owns its own launch file (`ariadne_core::acp::LaunchConfig`),
//! which is what the launcher writes and the runtime reads back. The wire is
//! the SDK's: every `session/*` request the runtime sends is built as a
//! `schema::v1` value here, so a field the protocol renamed is a compile
//! error rather than a payload an agent quietly ignores.

use agent_client_protocol::schema::v1;
use ariadne_core::acp::{EnvVariable, LaunchConfig, McpServer};

/// One Ariadne MCP server as the SDK's stdio transport, which every ACP agent
/// must support.
fn mcp_server(server: &McpServer) -> v1::McpServer {
    v1::McpServer::Stdio(
        v1::McpServerStdio::new(server.name.clone(), server.command.clone())
            .args(server.args.clone())
            .env(server.env.iter().map(env_variable).collect()),
    )
}

fn env_variable(variable: &EnvVariable) -> v1::EnvVariable {
    v1::EnvVariable::new(variable.name.clone(), variable.value.clone())
}

/// The MCP servers a session is opened with, whichever way it is opened.
pub(crate) fn mcp_servers(config: &LaunchConfig) -> Vec<v1::McpServer> {
    config.mcp_servers.iter().map(mcp_server).collect()
}
