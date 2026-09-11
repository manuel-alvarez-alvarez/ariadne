//! The ACP adapter: translate an Ariadne spawn/resume request into the
//! launch of one ACP agent — the flags behind its registry command, the
//! environment, and the launch file the ACP runtime drives it from.

mod acp;
pub mod prompts;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use ariadne_core::Seat;
use ariadne_core::acp::LaunchConfig;

pub use acp::{compaction_done, plan_resume, plan_spawn};

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
    /// Path or name of the `ariadne` CLI binary (the MCP entry point).
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
    /// The model the session is pinned to, as the agent itself names it —
    /// the bare half of the pin, after the registry id. Always passed on: a
    /// launch never falls back to an agent default.
    pub model: String,
    /// The effort that model is run at, as the session pinned it. None = the
    /// agent's own default.
    pub effort: Option<String>,
    /// The agent's configured flags, read from the database on every launch
    /// and appended to its registry command. Everything structural — the
    /// session, the MCP server, the system prompt, the model and its effort —
    /// travels in the launch file instead.
    pub extra_flags: Vec<String>,
}

/// Write an agent's skills into its run dir, one `SKILL.md` per skill, and
/// answer with the directory holding them.
///
/// The layout is `<name>/SKILL.md`, frontmatter and body. An ACP agent has no
/// way to be pointed at a skill directory, so the index in the system prompt
/// names these paths, and the agent opens the file itself.
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

/// A fully planned launch of one ACP agent.
#[derive(Debug, Clone)]
pub struct SpawnPlan {
    /// What rides behind the agent's registry command: its configured flags.
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: PathBuf,
    /// The agent session a resume continues; `None` for a fresh spawn.
    pub internal_session_id: Option<String>,
    /// The protocol half of the launch, also written to the run dir as
    /// `acp.json`, the record of what the agent was told.
    pub config: LaunchConfig,
}

/// Env vars every agent is launched with. The MCP server reads these to know
/// which session it acts for.
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
