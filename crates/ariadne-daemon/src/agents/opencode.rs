//! OpenCode adapter.
//!
//! - Config injection: `OPENCODE_CONFIG=<run>/opencode.json` env var —
//!   permission allow-all, custom `ariadne` agent carrying the system prompt,
//!   MCP server entry, and the Ariadne events plugin.
//! - Session id: captured by the plugin from `session.created` events.
//! - Resume: TUI `opencode --session <id> --prompt "<instruction>"` so the
//!   resumed session stays interactively attachable.

use anyhow::{Context, Result};
use serde_json::json;

use ariadne_core::AgentKind;

use super::{AgentAdapter, SpawnCtx, SpawnPlan, base_env};

pub struct OpencodeAdapter;

impl OpencodeAdapter {
    fn write_config(&self, ctx: &SpawnCtx) -> Result<std::path::PathBuf> {
        std::fs::create_dir_all(&ctx.run_dir)
            .with_context(|| format!("creating {}", ctx.run_dir.display()))?;

        // The events plugin is installed globally by the daemon at startup
        // (~/.ariadne/opencode-plugin/ariadne-events.js) and referenced here.
        let plugin_path = crate::opencode_plugin::plugin_path();

        let mut agent = json!({
            "description": "Ariadne-orchestrated agent",
            "prompt": ctx.system_prompt,
        });
        // OpenCode expects provider/model; skip when the profile model has no
        // provider prefix so the user default applies.
        if let Some(model) = &ctx.model
            && model.contains('/')
        {
            agent["model"] = json!(model);
        }

        let config = json!({
            "$schema": "https://opencode.ai/config.json",
            "permission": { "*": "allow" },
            "agent": { "ariadne": agent },
            "mcp": {
                "ariadne": {
                    "type": "local",
                    "command": [ctx.cli_bin, "mcp", "serve"],
                    "enabled": true,
                    "environment": super::env_json(ctx),
                }
            },
            "plugin": [format!("file://{}", plugin_path.display())],
        });
        let config_file = ctx.run_dir.join("opencode.json");
        std::fs::write(&config_file, serde_json::to_string_pretty(&config)?)?;
        Ok(config_file)
    }

    fn env_with_config(
        &self,
        ctx: &SpawnCtx,
        config_file: &std::path::Path,
    ) -> Vec<(String, String)> {
        let mut env = base_env(ctx);
        env.push(("OPENCODE_CONFIG".into(), config_file.display().to_string()));
        env
    }
}

impl AgentAdapter for OpencodeAdapter {
    fn kind(&self) -> AgentKind {
        AgentKind::Opencode
    }

    fn plan_spawn(&self, ctx: &SpawnCtx) -> Result<SpawnPlan> {
        let config_file = self.write_config(ctx)?;
        let mut argv = vec![
            "opencode".to_string(),
            "--agent".into(),
            "ariadne".into(),
            "--prompt".into(),
            ctx.initial_prompt.clone(),
        ];
        argv.extend(ctx.extra_flags.iter().cloned());
        Ok(SpawnPlan {
            env: self.env_with_config(ctx, &config_file),
            argv,
            cwd: ctx.cwd.clone(),
            // Captured from the plugin's session.created event.
            internal_session_id: None,
        })
    }

    fn plan_resume(
        &self,
        ctx: &SpawnCtx,
        internal_id: &str,
        instruction: &str,
    ) -> Result<SpawnPlan> {
        let config_file = self.write_config(ctx)?;
        let mut argv = vec![
            "opencode".to_string(),
            "--agent".into(),
            "ariadne".into(),
            "--session".into(),
            internal_id.to_string(),
        ];
        // Empty instruction = interactive resume without a message.
        if !instruction.is_empty() {
            argv.push("--prompt".into());
            argv.push(instruction.to_string());
        }
        argv.extend(ctx.extra_flags.iter().cloned());
        Ok(SpawnPlan {
            env: self.env_with_config(ctx, &config_file),
            argv,
            cwd: ctx.cwd.clone(),
            internal_session_id: Some(internal_id.to_string()),
        })
    }
}
