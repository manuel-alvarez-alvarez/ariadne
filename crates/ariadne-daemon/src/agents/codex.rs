//! Codex CLI adapter.
//!
//! - Bypass: `--dangerously-bypass-approvals-and-sandbox`, from the agent
//!   config rather than from here (`SpawnCtx::extra_flags`)
//! - Events: `-c hooks.<Event>=[...]` overrides piping each hook's JSON
//!   payload into `ariadne agent-event --kind codex`
//!   ([`ariadne_core::codex_hooks`]). Nothing is written to the user's
//!   `~/.codex`: the declaration is per session, and the trust the user grants
//!   once at install time covers it because codex keys command-line hook trust
//!   on a synthetic path rather than the worktree. The `SessionStart` payload
//!   carries `session_id`, captured by the ingestion endpoint before the first
//!   turn — notify only fired on agent-turn-complete, which a session killed
//!   mid-turn never reaches.
//! - Effort: `-c model_reasoning_effort=<level>`, a config override like the
//!   rest — codex has no flag for it
//! - System prompt: no append-safe flag — prepended to the initial prompt
//! - Resume: `codex resume <thread-id>`; flags must be re-passed (they are
//!   not inherited from the original session)

use anyhow::Result;

use ariadne_core::AgentKind;

use super::{AgentAdapter, SpawnCtx, SpawnPlan, base_env};

pub struct CodexAdapter;

impl CodexAdapter {
    fn config_flags(&self, ctx: &SpawnCtx) -> Vec<String> {
        // TOML inline values passed via -c key=value (no shell quoting: these
        // go straight into argv).
        let env_table = super::base_env(ctx)
            .into_iter()
            .map(|(k, v)| format!("{k} = \"{v}\""))
            .collect::<Vec<_>>()
            .join(", ");
        let mut flags = vec![
            "-c".to_string(),
            format!("mcp_servers.ariadne.command=\"{}\"", ctx.cli_bin),
            "-c".into(),
            "mcp_servers.ariadne.args=[\"mcp\",\"serve\"]".to_string(),
            "-c".into(),
            format!("mcp_servers.ariadne.env={{ {env_table} }}"),
        ];
        // Byte-identical to what `ariadne setup codex-hooks` had the user
        // trust; anything else and the session stalls at a trust prompt.
        flags.extend(ariadne_core::codex_hooks::config_flags(&ctx.cli_bin));
        if let Some(model) = &ctx.model {
            flags.push("-m".into());
            flags.push(model.clone());
        }
        if let Some(effort) = &ctx.effort {
            // No flag of its own: the config override is how codex takes an
            // effort. Quoted, like every other string value here — a bare
            // level works too (0.150.1 falls back to the raw string when the
            // value does not parse as TOML), but a quoted one is TOML either
            // way. Verified on 0.150.1: the session header reads "reasoning
            // effort: xhigh" and the rollout's turn context records it.
            flags.push("-c".into());
            flags.push(format!("model_reasoning_effort=\"{effort}\""));
        }
        flags.extend(ctx.extra_flags.iter().cloned());
        flags
    }

    /// Codex has no append-only system prompt mechanism; deliver the system
    /// layer as a preamble of the first user message.
    fn compose_prompt(&self, system: &str, prompt: &str) -> String {
        format!("<instructions>\n{system}\n</instructions>\n\n{prompt}")
    }
}

impl AgentAdapter for CodexAdapter {
    fn kind(&self) -> AgentKind {
        AgentKind::Codex
    }

    fn plan_spawn(&self, ctx: &SpawnCtx) -> Result<SpawnPlan> {
        let mut argv = vec!["codex".to_string()];
        argv.extend(self.config_flags(ctx));
        argv.push(self.compose_prompt(&ctx.system_prompt, &ctx.initial_prompt));
        Ok(SpawnPlan {
            argv,
            env: base_env(ctx),
            cwd: ctx.cwd.clone(),
            // The session id arrives with the SessionStart hook event.
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
        let mut argv = vec![
            "codex".to_string(),
            "resume".into(),
            internal_id.to_string(),
        ];
        // Known Codex issue: bypass/config flags are not inherited on resume.
        argv.extend(self.config_flags(ctx));
        // Empty instruction = interactive resume without a message.
        if !instruction.is_empty() {
            argv.push(instruction.to_string());
        }
        Ok(SpawnPlan {
            argv,
            env: base_env(ctx),
            cwd: ctx.cwd.clone(),
            internal_session_id: Some(internal_id.to_string()),
            post_launch_input: None,
        })
    }
}
