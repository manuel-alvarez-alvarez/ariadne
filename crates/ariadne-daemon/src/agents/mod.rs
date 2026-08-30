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
    /// The effort that model is run at, as the session pinned it. None = the
    /// CLI's own default.
    pub effort: Option<String>,
    /// The agent kind's configured flags (its permission bypass and whatever
    /// else the user added), read from the database on every launch. The
    /// structural flags — session ids, MCP and hook config, the system prompt,
    /// the model and its effort — are the adapters' own and are not in here.
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
    /// Text the launcher types into the pane once the TUI is up, for a CLI
    /// that cannot take the instruction on its argv: OpenCode silently drops
    /// `--prompt` when `--session` resumes an existing conversation (verified
    /// on 1.18.15), so its resume instruction goes in as a bracketed paste
    /// instead. `None` for spawns and for interactive resumes.
    pub post_launch_input: Option<String>,
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
    /// What the daemon types into this CLI's composer to have it compact its
    /// conversation, with the focus of `role` where the CLI takes one
    /// ([`compaction_focus`]); `None` for a CLI whose compaction cannot be
    /// started from outside.
    fn compaction_command(&self, role: Role) -> Option<String>;
    /// Whether an event this CLI reported — `kind` as `ariadne agent-event`
    /// spells it, with its payload — says a compaction has just finished.
    /// Whatever started it: one the daemon asked for, one the user typed, or
    /// the CLI's own near the context limit.
    fn compaction_done(&self, kind: &str, payload: &serde_json::Value) -> bool;
}

/// What a compaction is told to keep, per role, for the CLIs that take a
/// focus beside the command.
///
/// Simplified Technical English: short imperative sentences, one instruction
/// each, so the summary that comes out carries what the next resume of this
/// role has to know and nothing it does not.
pub fn compaction_focus(role: Role) -> &'static str {
    match role {
        Role::Engineer => {
            "Keep the task and the branch. Keep what changed and why. \
             Keep how you verified it. Keep the open review points."
        }
        Role::Reviewer => "Keep what you checked. Keep each finding of each verdict you gave.",
        Role::Planner => "Keep the goal. Keep the decisions. Keep the tasks you created.",
    }
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
    kind.binary()
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

#[cfg(test)]
mod tests {
    use super::{adapter_for, compaction_focus};

    use ariadne_core::{AgentKind, Role};
    use serde_json::json;

    /// Every CLI can be told to compact from its composer, and only Claude
    /// Code takes the focus text on the same line: the other two run
    /// `/compact` bare and are handed nothing else.
    #[test]
    fn every_cli_has_a_compaction_command_and_only_claude_takes_a_focus() {
        for role in [Role::Planner, Role::Engineer, Role::Reviewer] {
            let claude = adapter_for(AgentKind::ClaudeCode)
                .compaction_command(role)
                .unwrap();
            assert_eq!(
                claude,
                format!("/compact {}", compaction_focus(role)),
                "{role:?}"
            );
            assert!(
                !claude.contains('\n'),
                "one line, or the paste submits in pieces"
            );
            for kind in [AgentKind::Codex, AgentKind::Opencode] {
                assert_eq!(
                    adapter_for(kind).compaction_command(role).as_deref(),
                    Some("/compact"),
                    "{kind:?} {role:?}"
                );
            }
        }
    }

    /// The focus texts are Simplified Technical English: short imperative
    /// sentences, each one an instruction of its own.
    #[test]
    fn the_focus_texts_are_short_imperative_sentences() {
        for role in [Role::Planner, Role::Engineer, Role::Reviewer] {
            for sentence in compaction_focus(role)
                .split('.')
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                assert!(
                    sentence.starts_with("Keep"),
                    "{role:?}: {sentence:?} does not start with an imperative"
                );
                assert!(
                    sentence.split_whitespace().count() <= 20,
                    "{role:?}: {sentence:?} is longer than a Simplified Technical English sentence"
                );
            }
        }
    }

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
