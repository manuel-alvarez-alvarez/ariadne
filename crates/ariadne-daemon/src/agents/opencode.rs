//! OpenCode adapter.
//!
//! - Config injection: `OPENCODE_CONFIG=<run>/opencode.json` env var —
//!   the permission block below, a custom `ariadne` agent carrying the system
//!   prompt, the MCP server entry, and the Ariadne events plugin.
//! - Agent pinning: the system prompt exists only on the `ariadne` agent, so
//!   every message must run as it. `default_agent` alone is not enough —
//!   OpenCode falls back to `build` whenever the named agent is unavailable,
//!   and Tab in an attached TUI cycles primaries — so `build` and `plan` are
//!   disabled outright, which leaves nothing to fall back or cycle to.
//! - Autonomy: the `--auto` flag comes from the agent config
//!   ([`AgentKind::default_flags`]); the permission block here is structural.
//! - Effort: `agent.ariadne.variant` in that config, which covers spawns and
//!   resumes alike — the TUI entry point used for resumes has no `--variant`
//!   flag of its own. It only takes effect beside the agent's own `model`
//!   (see [`OpencodeAdapter::write_config`]).
//! - Session id: captured by the plugin from `session.created` events.
//! - Resume: TUI `opencode --session <id>` so the resumed session stays
//!   interactively attachable. The instruction cannot ride the argv —
//!   OpenCode (verified on 1.18.15) silently drops `--prompt` when resuming
//!   an existing session — so it goes out as [`SpawnPlan::post_launch_input`]
//!   for the launcher to type into the TUI once it is up.
//! - Compaction: `/compact` typed into the TUI — no focus text — reported
//!   done by the `session.compacted` event the plugin forwards.

use anyhow::{Context, Result};
use serde_json::json;

use ariadne_core::{AgentKind, Role};

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
            "mode": "primary",
            "prompt": ctx.system_prompt,
        });
        // OpenCode expects provider/model; skip when the profile model has no
        // provider prefix so the user default applies.
        if let Some(model) = &ctx.model
            && model.contains('/')
        {
            agent["model"] = json!(model);
            // The effort is the model's variant, and the schema's "applies
            // only when using the agent's configured model" is literal:
            // verified on 1.18.15, an `ariadne` agent carrying `variant` and
            // `model` together starts a session recorded as
            // `{"id":"hy3-free","providerID":"opencode","variant":"high"}`,
            // while the same agent carrying the variant alone — its model
            // left to the user's config — starts one recorded as
            // `"variant":"default"`. So the variant goes in beside a model we
            // write, and nowhere else.
            if let Some(effort) = &ctx.effort {
                agent["variant"] = json!(effort);
            }
        } else if ctx.effort.is_some() {
            tracing::warn!(
                session = %ctx.session_id,
                model = ?ctx.model,
                "opencode ignores an effort with no provider-prefixed model to hang it on; the session runs at the model's default variant"
            );
        }

        let config = json!({
            "$schema": "https://opencode.ai/config.json",
            // The catch-all allow does not silence everything: OpenCode
            // resolves its built-in *ask* rules (`doom_loop`,
            // `external_directory`, reading `.env`) after the config's, and
            // it is the `--auto` flag from the agent config that approves
            // whatever still asks. What `--auto` never overrides are denies,
            // which is exactly what the entries here are for: each of these
            // tools hands control to a human — the very thing a tmux-parked
            // agent must never do.
            "permission": {
                "*": "allow",
                "question": "deny",
                "plan_enter": "deny",
                "plan_exit": "deny",
            },
            // Every message must run as `ariadne` — its prompt is the system
            // layer. See the module docs on why build/plan are disabled
            // rather than merely not defaulted to.
            "default_agent": "ariadne",
            "agent": {
                "ariadne": agent,
                "build": { "disable": true },
                "plan": { "disable": true },
            },
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
            post_launch_input: None,
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
        argv.extend(ctx.extra_flags.iter().cloned());
        Ok(SpawnPlan {
            env: self.env_with_config(ctx, &config_file),
            argv,
            cwd: ctx.cwd.clone(),
            internal_session_id: Some(internal_id.to_string()),
            // Typed into the TUI, not passed as `--prompt`: see module docs.
            // Empty instruction = interactive resume without a message.
            post_launch_input: (!instruction.is_empty()).then(|| instruction.to_string()),
        })
    }

    fn compaction_command(&self, _role: Role) -> Option<String> {
        Some("/compact".into())
    }

    fn compaction_done(&self, kind: &str, _payload: &serde_json::Value) -> bool {
        kind == "session.compacted"
    }
}
