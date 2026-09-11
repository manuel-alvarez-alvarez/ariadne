//! How a spawn and a resume are spelled for an ACP agent.
//!
//! - Command: the registry command of the agent the pin names, with the
//!   agent's configured flags behind it
//! - Transport: stable ACP v1 over newline-delimited JSON-RPC on stdio
//! - Model and effort: session configuration options with the `model` and
//!   `thought_level` categories
//! - System prompt and skills: prepended to every prompt the agent is sent
//! - MCP: the stdio server in `session/new`, `session/resume`, or
//!   `session/load`
//! - Events: the ACP runtime maps the agent's updates onto the event
//!   vocabulary the daemon ingests
//! - Session id: returned by `session/new` and captured from `session_start`
//! - Resume: `session/resume`, with `session/load` as the protocol fallback
//! - Compaction: a completed `compaction_update`

use anyhow::{Context, Result};
use ariadne_core::acp::{self, EnvVariable, LaunchConfig, McpServer};

use super::{SpawnCtx, SpawnPlan, base_env};

/// Plan a fresh spawn: a new agent session, briefed with the initial prompt.
pub fn plan_spawn(ctx: &SpawnCtx) -> Result<SpawnPlan> {
    plan(ctx, None, Some(&ctx.initial_prompt))
}

/// Plan a resume of the agent session `internal_id`, with `instruction` as
/// its next prompt. An empty instruction resumes into an idle agent that is
/// told nothing.
pub fn plan_resume(ctx: &SpawnCtx, internal_id: &str, instruction: &str) -> Result<SpawnPlan> {
    plan(
        ctx,
        Some(internal_id),
        (!instruction.is_empty()).then_some(instruction),
    )
}

/// Whether an event the runtime reported — `kind` with its payload — says a
/// compaction has just finished, so the agent is back at its prompt rather
/// than mid-turn. Started by the user, or by the agent itself near the
/// context limit: the daemon asks for none.
pub fn compaction_done(kind: &str, payload: &serde_json::Value) -> bool {
    kind == "compaction_update"
        && payload.get("status").and_then(|value| value.as_str()) == Some("completed")
}

fn plan(
    ctx: &SpawnCtx,
    resume_session_id: Option<&str>,
    initial_prompt: Option<&str>,
) -> Result<SpawnPlan> {
    let config = LaunchConfig {
        version: acp::VERSION,
        system_prompt: ctx.system_prompt.clone(),
        initial_prompt: initial_prompt.map(str::to_string),
        model: ctx.model.clone(),
        effort: ctx.effort.clone(),
        resume_session_id: resume_session_id.map(str::to_string),
        mcp_servers: vec![McpServer {
            name: "ariadne".into(),
            command: ctx.cli_bin.clone(),
            args: vec!["mcp".into(), "serve".into()],
            env: base_env(ctx)
                .into_iter()
                .map(|(name, value)| EnvVariable { name, value })
                .collect(),
        }],
    };
    write_config(ctx, &config)?;
    Ok(SpawnPlan {
        args: ctx.extra_flags.clone(),
        env: base_env(ctx),
        cwd: ctx.cwd.clone(),
        internal_session_id: resume_session_id.map(str::to_string),
        config,
    })
}

/// Write the launch file into the run dir: the record of what the agent was
/// told, pinned to and connected to.
fn write_config(ctx: &SpawnCtx, config: &LaunchConfig) -> Result<()> {
    std::fs::create_dir_all(&ctx.run_dir)
        .with_context(|| format!("creating {}", ctx.run_dir.display()))?;
    let path = ctx.run_dir.join("acp.json");
    std::fs::write(&path, serde_json::to_string_pretty(config)?)
        .with_context(|| format!("writing {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::compaction_done;

    use serde_json::json;

    /// A compaction is over when the runtime reports a completed one, and
    /// nothing else it reports is mistaken for it.
    #[test]
    fn a_compaction_is_done_when_the_agent_says_so_and_not_before() {
        assert!(compaction_done(
            "compaction_update",
            &json!({"status": "completed"})
        ));
        for (kind, payload) in [
            ("compaction_update", json!({"status": "in_progress"})),
            ("compaction_update", json!({"status": "failed"})),
            ("session_update", json!({"status": "completed"})),
            ("session_start", json!({})),
        ] {
            assert!(!compaction_done(kind, &payload), "{kind} {payload}");
        }
    }
}
