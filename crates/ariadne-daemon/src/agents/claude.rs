//! Claude Code adapter.
//!
//! - Bypass: `--dangerously-skip-permissions`, from the agent config rather
//!   than from here (`SpawnCtx::extra_flags`) so the user can drop it
//! - Session id: chosen by us via `--session-id <uuid>` (deterministic capture)
//! - System prompt: `--append-system-prompt <content>` (inline; the installed
//!   CLI has no `-file` variant — a copy is kept in the run dir for debugging)
//! - MCP: `--mcp-config <run>/mcp.json`
//! - Hooks: `--settings <run>/settings.json` (command hooks piping JSON into
//!   `ariadne agent-event --kind claude`)

use anyhow::{Context, Result};
use serde_json::json;

use ariadne_core::AgentKind;

use super::{AgentAdapter, SpawnCtx, SpawnPlan, base_env, env_json};

pub struct ClaudeAdapter;

impl ClaudeAdapter {
    /// Write system-prompt.md, mcp.json and settings.json into the run dir;
    /// return the common flag block.
    fn write_configs(&self, ctx: &SpawnCtx) -> Result<Vec<String>> {
        std::fs::create_dir_all(&ctx.run_dir)
            .with_context(|| format!("creating {}", ctx.run_dir.display()))?;

        // Debugging copy; the prompt itself is passed inline.
        std::fs::write(ctx.run_dir.join("system-prompt.md"), &ctx.system_prompt)?;

        let mcp_file = ctx.run_dir.join("mcp.json");
        let mcp = json!({
            "mcpServers": {
                "ariadne": {
                    "command": ctx.cli_bin,
                    "args": ["mcp", "serve"],
                    "env": env_json(ctx),
                }
            }
        });
        std::fs::write(&mcp_file, serde_json::to_string_pretty(&mcp)?)?;

        let settings_file = ctx.run_dir.join("settings.json");
        let hook_cmd = format!("{} agent-event --kind claude", ctx.cli_bin);
        let hook =
            |_event: &str| json!([{ "hooks": [{ "type": "command", "command": hook_cmd }] }]);
        let settings = json!({
            "hooks": {
                "SessionStart": hook("SessionStart"),
                "PostToolUse": hook("PostToolUse"),
                "Stop": hook("Stop"),
                "SessionEnd": hook("SessionEnd"),
            }
        });
        std::fs::write(&settings_file, serde_json::to_string_pretty(&settings)?)?;

        Ok(vec![
            "--append-system-prompt".into(),
            ctx.system_prompt.clone(),
            "--mcp-config".into(),
            mcp_file.display().to_string(),
            "--settings".into(),
            settings_file.display().to_string(),
        ])
    }

    fn common_tail(&self, ctx: &SpawnCtx, argv: &mut Vec<String>) {
        if let Some(model) = &ctx.model {
            argv.push("--model".into());
            argv.push(model.clone());
        }
        argv.extend(ctx.extra_flags.iter().cloned());
    }
}

impl AgentAdapter for ClaudeAdapter {
    fn kind(&self) -> AgentKind {
        AgentKind::ClaudeCode
    }

    fn plan_spawn(&self, ctx: &SpawnCtx) -> Result<SpawnPlan> {
        // We pick the session uuid so it is captured before the process runs.
        let session_uuid = uuid::Uuid::new_v4().to_string();
        let mut argv = vec!["claude".to_string()];
        argv.extend(self.write_configs(ctx)?);
        argv.push("--session-id".into());
        argv.push(session_uuid.clone());
        self.common_tail(ctx, &mut argv);
        argv.push(ctx.initial_prompt.clone());
        Ok(SpawnPlan {
            argv,
            env: base_env(ctx),
            cwd: ctx.cwd.clone(),
            internal_session_id: Some(session_uuid),
        })
    }

    fn plan_resume(
        &self,
        ctx: &SpawnCtx,
        internal_id: &str,
        instruction: &str,
    ) -> Result<SpawnPlan> {
        let mut argv = vec!["claude".to_string()];
        argv.extend(self.write_configs(ctx)?);
        argv.push("--resume".into());
        argv.push(internal_id.to_string());
        self.common_tail(ctx, &mut argv);
        // Empty instruction = interactive resume (used by `ariadne attach`
        // when reviving a session): drop into the TUI without a message.
        if !instruction.is_empty() {
            argv.push(instruction.to_string());
        }
        Ok(SpawnPlan {
            argv,
            env: base_env(ctx),
            cwd: ctx.cwd.clone(),
            internal_session_id: Some(internal_id.to_string()),
        })
    }
}
