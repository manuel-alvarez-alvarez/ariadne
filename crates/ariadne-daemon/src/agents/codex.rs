//! Codex CLI adapter.
//!
//! - Bypass: `--dangerously-bypass-approvals-and-sandbox`
//! - Events: hooks declared in the global `~/.codex/hooks.json`, installed by
//!   `ariadne setup codex-hooks` and trusted once, manually, by the user.
//!   Every hook pipes its JSON payload into `ariadne agent-event --kind
//!   codex`. Nothing is passed at spawn: hook trust is keyed on the hooks-file
//!   path, so a per-worktree file would need a new trust prompt every time.
//!   The `SessionStart` payload carries `session_id`, which the ingestion
//!   endpoint captures before the first turn — notify only fired on
//!   agent-turn-complete, which a session killed mid-turn never reaches.
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
            "--dangerously-bypass-approvals-and-sandbox".to_string(),
            "-c".into(),
            format!("mcp_servers.ariadne.command=\"{}\"", ctx.cli_bin),
            "-c".into(),
            "mcp_servers.ariadne.args=[\"mcp\",\"serve\"]".to_string(),
            "-c".into(),
            format!("mcp_servers.ariadne.env={{ {env_table} }}"),
        ];
        if let Some(model) = &ctx.model {
            flags.push("-m".into());
            flags.push(model.clone());
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
        })
    }
}
