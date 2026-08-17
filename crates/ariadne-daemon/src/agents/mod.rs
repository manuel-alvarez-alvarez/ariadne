//! Agent adapters: translate an Ariadne spawn/resume request into the argv,
//! env and generated config files for a concrete coding-agent CLI.

mod claude;
mod codex;
mod opencode;
pub mod prompts;

use std::path::PathBuf;

use anyhow::Result;

use ariadne_core::{AgentKind, Role};

/// Everything an adapter needs to plan a spawn. Prompt assembly happens in
/// the launcher; adapters only deal with delivery mechanics.
#[derive(Debug, Clone)]
pub struct SpawnCtx {
    /// Ariadne agent-session id (becomes ARIADNE_SESSION_ID).
    pub session_id: String,
    pub goal_id: String,
    pub task_id: Option<String>,
    pub role: Role,
    /// Per-session directory for generated files (`~/.ariadne/run/<id>/`).
    pub run_dir: PathBuf,
    /// Where the agent process runs: worktree (engineer/reviewer) or repo (planner).
    pub cwd: PathBuf,
    pub socket_path: PathBuf,
    /// Path or name of the `ariadne` CLI binary (hooks + MCP entry point).
    pub cli_bin: String,
    /// The profile's system prompt, as stored.
    pub system_prompt: String,
    /// Task/goal briefing delivered as the first user prompt.
    pub initial_prompt: String,
    pub model: Option<String>,
    pub extra_flags: Vec<String>,
}

/// A fully planned process launch for tmux.
#[derive(Debug, Clone)]
pub struct SpawnPlan {
    pub argv: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: PathBuf,
    /// Known ahead of time only for Claude Code (we choose the uuid).
    pub internal_session_id: Option<String>,
}

pub trait AgentAdapter: Send + Sync {
    fn kind(&self) -> AgentKind;
    /// Write run-dir files and return the launch plan.
    fn plan_spawn(&self, ctx: &SpawnCtx) -> Result<SpawnPlan>;
    /// Plan a resume of a previous session with a new instruction.
    fn plan_resume(
        &self,
        ctx: &SpawnCtx,
        internal_id: &str,
        instruction: &str,
    ) -> Result<SpawnPlan>;
}

pub fn adapter_for(kind: AgentKind) -> &'static dyn AgentAdapter {
    match kind {
        AgentKind::ClaudeCode => &claude::ClaudeAdapter,
        AgentKind::Codex => &codex::CodexAdapter,
        AgentKind::Opencode => &opencode::OpencodeAdapter,
    }
}

/// Preference order used when a profile has no explicit agent kind.
pub const AGENT_PREFERENCE: [AgentKind; 3] =
    [AgentKind::ClaudeCode, AgentKind::Codex, AgentKind::Opencode];

/// The executable each agent kind is launched with.
pub fn binary_for(kind: AgentKind) -> &'static str {
    match kind {
        AgentKind::ClaudeCode => "claude",
        AgentKind::Codex => "codex",
        AgentKind::Opencode => "opencode",
    }
}

/// First agent CLI installed on this machine, in [`AGENT_PREFERENCE`] order.
pub fn detect_first_available() -> Option<AgentKind> {
    let path = std::env::var_os("PATH")?;
    AGENT_PREFERENCE
        .into_iter()
        .find(|kind| std::env::split_paths(&path).any(|dir| dir.join(binary_for(*kind)).is_file()))
}

/// Env vars common to every agent kind. The MCP server and the event hook
/// read these to know which session they act for.
pub fn base_env(ctx: &SpawnCtx) -> Vec<(String, String)> {
    let mut env = vec![
        ("ARIADNE_SESSION_ID".into(), ctx.session_id.clone()),
        ("ARIADNE_GOAL_ID".into(), ctx.goal_id.clone()),
        ("ARIADNE_ROLE".into(), ctx.role.as_str().to_string()),
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
