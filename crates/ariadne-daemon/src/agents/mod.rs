//! Agent adapters: translate an Ariadne spawn/resume request into the argv,
//! env and generated config files for a concrete coding-agent CLI.

mod claude;
mod codex;
pub mod contract;
mod opencode;
pub mod prompts;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use ariadne_core::{AgentKind, Seat};

pub use contract::AdapterContract;

/// Everything an adapter needs to plan a spawn. Prompt assembly happens in
/// the launcher; adapters only deal with delivery mechanics.
#[derive(Debug, Clone)]
pub struct SpawnCtx {
    /// Ariadne agent-session id (becomes ARIADNE_SESSION_ID).
    pub session_id: String,
    /// This launch of that session (becomes ARIADNE_LAUNCH_ID): fresh for
    /// every process started under the row, so that what this agent reports
    /// is told apart from what the agent it replaces is still reporting.
    pub launch_id: String,
    pub goal_id: String,
    pub task_id: Option<String>,
    pub seat: Seat,
    /// Per-session directory for generated files (`~/.ariadne/run/<id>/`).
    pub run_dir: PathBuf,
    /// Where the agent process runs: worktree (author/reviewer) or repo
    /// (orchestrator).
    pub cwd: PathBuf,
    pub socket_path: PathBuf,
    /// Path or name of the `ariadne` CLI binary (hooks + MCP entry point).
    pub cli_bin: String,
    /// What the agent is briefed as: what its seat owes, and the index of the
    /// skills it loads.
    pub system_prompt: String,
    /// Where this agent's skill documents are, one `<name>/SKILL.md` under it.
    /// The launcher writes them ([`write_skills`]) before the adapter plans
    /// anything, so the index in the system prompt and the files on disk are
    /// the same list, and an adapter only has to point its CLI at the
    /// directory. `None` for an agent that loads no skill.
    pub skills_dir: Option<PathBuf>,
    /// Task/goal briefing delivered as the first user prompt.
    pub initial_prompt: String,
    /// The model the session is pinned to, as the CLI itself names it. Always
    /// passed on: a launch never falls back to a CLI default.
    pub model: String,
    /// The effort that model is run at, as the session pinned it. None = the
    /// CLI's own default.
    pub effort: Option<String>,
    /// The agent kind's configured flags (its permission bypass and whatever
    /// else the user added), read from the database on every launch. The
    /// structural flags — session ids, MCP and hook config, the system prompt,
    /// the model and its effort — are the adapters' own and are not in here.
    pub extra_flags: Vec<String>,
}

/// Write an agent's skills into its run dir, one `SKILL.md` per skill, and
/// answer with the directory holding them.
///
/// The layout is the one all three CLIs read — `<name>/SKILL.md`, frontmatter
/// and body — so one write serves whichever mechanism the adapter then points
/// at it: a plugin for Claude Code, `skills.paths` for OpenCode. Codex takes
/// neither, because it discovers skills only under its own home or the project
/// root, and the project root of an agent is the worktree. So the index in the
/// system prompt names these paths as well, which is the floor under all
/// three: an agent with no native skill loading opens the file itself.
///
/// Rewritten on every launch rather than cached, so a skill reworded since the
/// task was staffed reaches the next launch of the agent that loads it — the
/// same way every other default text does.
pub fn write_skills(run_dir: &Path, skills: &[(String, String)]) -> Result<PathBuf> {
    let root = run_dir.join("skills");
    // Cleared first: a skill an agent no longer loads must not be left behind
    // for it to read.
    if root.exists() {
        std::fs::remove_dir_all(&root).with_context(|| format!("clearing {}", root.display()))?;
    }
    for (name, document) in skills {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        std::fs::write(dir.join("SKILL.md"), document)
            .with_context(|| format!("writing the {name} skill"))?;
    }
    Ok(root)
}

/// A fully planned process launch for tmux.
#[derive(Debug, Clone)]
pub struct SpawnPlan {
    pub argv: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: PathBuf,
    /// Known ahead of time only for Claude Code (we choose the uuid).
    pub internal_session_id: Option<String>,
    /// Text the launcher types into the pane once the TUI is up, for a CLI
    /// that cannot take the instruction on its argv: OpenCode silently drops
    /// `--prompt` when `--session` resumes an existing conversation (verified
    /// on 1.18.15), so its resume instruction goes in as a bracketed paste
    /// instead. `None` for spawns and for interactive resumes.
    pub post_launch_input: Option<String>,
}

pub trait AgentAdapter: Send + Sync {
    fn kind(&self) -> AgentKind;
    /// How this CLI spells each clause of the adapter contract
    /// ([`contract`]). What one suite holds every adapter to.
    fn contract(&self) -> AdapterContract;
    /// Write run-dir files and return the launch plan.
    fn plan_spawn(&self, ctx: &SpawnCtx) -> Result<SpawnPlan>;
    /// Plan a resume of a previous session with a new instruction.
    fn plan_resume(
        &self,
        ctx: &SpawnCtx,
        internal_id: &str,
        instruction: &str,
    ) -> Result<SpawnPlan>;
    /// Whether an event this CLI reported — `kind` as `ariadne agent-event`
    /// spells it, with its payload — says a compaction has just finished, so
    /// the agent is back at its prompt rather than mid-turn. Started by the
    /// user at the pane, or by the CLI itself near the context limit: the
    /// daemon asks for none.
    fn compaction_done(&self, kind: &str, payload: &serde_json::Value) -> bool;
}

pub fn adapter_for(kind: AgentKind) -> &'static dyn AgentAdapter {
    match kind {
        AgentKind::ClaudeCode => &claude::ClaudeAdapter,
        AgentKind::Codex => &codex::CodexAdapter,
        AgentKind::Opencode => &opencode::OpencodeAdapter,
    }
}

/// Env vars common to every agent kind. The MCP server and the event hook
/// read these to know which session they act for.
pub fn base_env(ctx: &SpawnCtx) -> Vec<(String, String)> {
    let mut env = vec![
        ("ARIADNE_SESSION_ID".into(), ctx.session_id.clone()),
        ("ARIADNE_LAUNCH_ID".into(), ctx.launch_id.clone()),
        ("ARIADNE_GOAL_ID".into(), ctx.goal_id.clone()),
        ("ARIADNE_SEAT".into(), ctx.seat.as_str().to_string()),
        (
            "ARIADNE_SOCKET".into(),
            ctx.socket_path.display().to_string(),
        ),
    ];
    if let Some(task) = &ctx.task_id {
        env.push(("ARIADNE_TASK_ID".into(), task.clone()));
    }
    env
}

/// The same env rendered as a JSON object (for MCP server configs).
pub fn env_json(ctx: &SpawnCtx) -> serde_json::Map<String, serde_json::Value> {
    base_env(ctx)
        .into_iter()
        .map(|(k, v)| (k, serde_json::Value::String(v)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::adapter_for;

    use ariadne_core::AgentKind;
    use serde_json::json;

    /// Each CLI says a compaction is over in its own vocabulary, and nothing
    /// else it reports is mistaken for it — least of all the session start of
    /// a resume, which Claude Code spells with the same hook.
    #[test]
    fn a_compaction_is_done_when_the_cli_says_so_and_not_before() {
        let claude = adapter_for(AgentKind::ClaudeCode);
        assert!(claude.compaction_done(
            "session_start",
            &json!({"hook_event_name": "SessionStart", "source": "compact"})
        ));
        for (kind, payload) in [
            ("session_start", json!({"source": "resume"})),
            ("session_start", json!({"source": "startup"})),
            ("session_start", json!({})),
            ("pre_compact", json!({"trigger": "manual"})),
            ("stop", json!({})),
        ] {
            assert!(!claude.compaction_done(kind, &payload), "{kind} {payload}");
        }

        let codex = adapter_for(AgentKind::Codex);
        assert!(codex.compaction_done("post_compact", &json!({})));
        for kind in ["pre_compact", "session_start", "stop"] {
            assert!(!codex.compaction_done(kind, &json!({})), "{kind}");
        }

        let opencode = adapter_for(AgentKind::Opencode);
        assert!(opencode.compaction_done("session.compacted", &json!({"sessionID": "ses_x"})));
        for kind in ["session.idle", "session.updated", "session.created"] {
            assert!(!opencode.compaction_done(kind, &json!({})), "{kind}");
        }
    }
}
