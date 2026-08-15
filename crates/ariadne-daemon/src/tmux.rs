//! TmuxManager: session lifecycle for agent processes.
//!
//! Shells out to `tmux` — boring and reliable. Sessions are created detached;
//! the user attaches with `tmux attach -t <name>` (via `ariadne attach`).

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use tokio::process::Command;

#[derive(Debug, Clone)]
pub struct TmuxManager {
    bin: String,
}

/// Everything needed to launch one agent in a detached tmux session.
#[derive(Debug, Clone)]
pub struct TmuxSpawn {
    /// Unique tmux session name, e.g. `ariadne-<goal>-<task>-eng`.
    pub session: String,
    pub cwd: PathBuf,
    pub env: Vec<(String, String)>,
    /// Command argv; argv[0] is the program.
    pub argv: Vec<String>,
    /// When set, `pipe-pane` appends the full console stream to this file.
    pub log_file: Option<PathBuf>,
}

impl Default for TmuxManager {
    fn default() -> Self {
        Self::new("tmux")
    }
}

impl TmuxManager {
    pub fn new(bin: impl Into<String>) -> Self {
        Self { bin: bin.into() }
    }

    async fn tmux(&self, args: &[&str]) -> Result<String> {
        let output = Command::new(&self.bin)
            .args(args)
            .output()
            .await
            .with_context(|| format!("running {} {}", self.bin, args.join(" ")))?;
        if !output.status.success() {
            bail!(
                "tmux {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    /// Create a detached session running `spawn.argv` in `spawn.cwd`.
    pub async fn new_session(&self, spawn: &TmuxSpawn) -> Result<()> {
        if spawn.argv.is_empty() {
            bail!("empty argv for tmux session {}", spawn.session);
        }
        let mut args: Vec<String> = vec![
            "new-session".into(),
            "-d".into(),
            "-s".into(),
            spawn.session.clone(),
            "-c".into(),
            spawn.cwd.display().to_string(),
        ];
        for (k, v) in &spawn.env {
            args.push("-e".into());
            args.push(format!("{k}={v}"));
        }
        args.push("--".into());
        args.extend(spawn.argv.iter().cloned());
        let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
        self.tmux(&arg_refs).await?;

        if let Some(log) = &spawn.log_file {
            // -o: only start piping if not already piping.
            self.tmux(&[
                "pipe-pane",
                "-o",
                "-t",
                &spawn.session,
                &format!("cat >> '{}'", log.display()),
            ])
            .await?;
        }
        Ok(())
    }

    pub async fn has_session(&self, name: &str) -> bool {
        Command::new(&self.bin)
            .args(["has-session", "-t", name])
            .output()
            .await
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    pub async fn kill_session(&self, name: &str) -> Result<()> {
        self.tmux(&["kill-session", "-t", name]).await.map(|_| ())
    }

    /// Snapshot of the last `lines` of the pane (including scrollback).
    pub async fn capture_pane(&self, name: &str, lines: u32) -> Result<String> {
        self.tmux(&["capture-pane", "-p", "-t", name, "-S", &format!("-{lines}")])
            .await
    }

    /// Type `text` into the session followed by Enter (used to nudge
    /// interactive agents).
    pub async fn send_text(&self, name: &str, text: &str) -> Result<()> {
        // -l = literal (no key-name lookup), then a separate Enter press.
        self.tmux(&["send-keys", "-t", name, "-l", text]).await?;
        self.tmux(&["send-keys", "-t", name, "Enter"]).await?;
        Ok(())
    }

    /// Press Enter in the session (accepts pre-selected TUI dialogs).
    pub async fn send_enter(&self, name: &str) -> Result<()> {
        self.tmux(&["send-keys", "-t", name, "Enter"])
            .await
            .map(|_| ())
    }

    /// List ariadne-managed session names.
    pub async fn list_sessions(&self) -> Result<Vec<String>> {
        match self.tmux(&["list-sessions", "-F", "#{session_name}"]).await {
            Ok(out) => Ok(out.lines().map(str::to_string).collect()),
            // No server running = no sessions.
            Err(_) => Ok(Vec::new()),
        }
    }
}

/// Build the canonical tmux session name for an agent session.
pub fn session_name(
    goal_id: &str,
    task_id: Option<&str>,
    role: &str,
    round: Option<i64>,
) -> String {
    // ULIDs are long; the trailing 8 chars are the distinctive random part.
    fn tail(id: &str) -> &str {
        &id[id.len().saturating_sub(8)..]
    }
    let mut name = format!("ariadne-{}", tail(goal_id));
    if let Some(task) = task_id {
        name.push_str(&format!("-{}", tail(task)));
    }
    name.push_str(&format!("-{}", &role[..3.min(role.len())]));
    if let Some(r) = round {
        name.push_str(&format!("-r{r}"));
    }
    name
}
