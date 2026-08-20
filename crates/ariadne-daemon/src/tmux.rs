//! TmuxManager: session lifecycle for agent processes.
//!
//! Shells out to `tmux` — boring and reliable. Sessions are created detached;
//! the user attaches with `tmux attach -t <name>` (via `ariadne attach`).

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use tokio::process::Command;

/// Input bytes per `send-keys -H` call; see [`TmuxManager::send_raw`].
const RAW_SEND_BATCH: usize = 512;

#[derive(Debug, Clone)]
pub struct TmuxManager {
    bin: String,
}

/// A pane's screen: its grid, and where its cursor sits in it (0-based, from
/// the top-left of the visible area).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaneGeometry {
    pub cols: u16,
    pub rows: u16,
    pub cursor_x: u16,
    pub cursor_y: u16,
}

/// `"80x24 2,21"`, as [`TmuxManager::pane_geometry`] asks tmux to print it.
fn parse_geometry(raw: &str) -> Option<PaneGeometry> {
    let (size, cursor) = raw.split_once(' ')?;
    let (cols, rows) = parse_size(size)?;
    let (x, y) = cursor.split_once(',')?;
    Some(PaneGeometry {
        cols,
        rows,
        cursor_x: x.trim().parse().ok()?,
        cursor_y: y.trim().parse().ok()?,
    })
}

/// `"80x24"` — the size half of a geometry, which is also how a pane's last
/// known size is stored for a session that has ended (see
/// `Launcher::record_pane_size`).
pub fn parse_size(raw: &str) -> Option<(u16, u16)> {
    let (cols, rows) = raw.split_once('x')?;
    Some((cols.trim().parse().ok()?, rows.trim().parse().ok()?))
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

    /// Whether tmux has this session, with "could not ask" folded into "no" —
    /// which is what most callers want, since they go on to create or replace
    /// the session either way. Anything that would *conclude* something from a
    /// "no" — that a session is over, say — wants
    /// [`Self::has_session_checked`] instead.
    pub async fn has_session(&self, name: &str) -> bool {
        self.has_session_checked(name).await.unwrap_or(false)
    }

    /// Whether the session might still be there, with "could not ask" folded
    /// into "yes".
    ///
    /// For deciding whether to *create* something — a second agent for a role
    /// that may already have one — where a wrong "no" duplicates work that is
    /// already under way, while a wrong "yes" costs a scheduler tick that does
    /// nothing and asks again in fifteen seconds.
    pub async fn has_session_or_unknown(&self, name: &str) -> bool {
        self.has_session_checked(name).await.unwrap_or(true)
    }

    /// Whether tmux has this session, as an answer rather than a guess.
    ///
    /// `Err` means the question never reached tmux, which is not the same as
    /// tmux saying no: a daemon that cannot spawn a process for a moment has
    /// learned nothing about the pane, and a viewer told the session is over
    /// stops asking for good.
    pub async fn has_session_checked(&self, name: &str) -> Result<bool> {
        let output = Command::new(&self.bin)
            .args(["has-session", "-t", name])
            .output()
            .await
            .with_context(|| format!("running {} has-session -t {name}", self.bin))?;
        Ok(output.status.success())
    }

    pub async fn kill_session(&self, name: &str) -> Result<()> {
        self.tmux(&["kill-session", "-t", name]).await.map(|_| ())
    }

    /// Snapshot of the last `lines` of the pane (including scrollback).
    ///
    /// `-e` keeps the escape sequences that colour it. Without them the
    /// capture is plain text, and everything an agent printed before a viewer
    /// connected would arrive grey while only later output had colour.
    pub async fn capture_pane(&self, name: &str, lines: u32) -> Result<String> {
        self.tmux(&[
            "capture-pane",
            "-p",
            "-e",
            "-t",
            name,
            "-S",
            &format!("-{lines}"),
        ])
        .await
    }

    /// Where the pane's screen stands: how big it is and where its cursor is.
    ///
    /// The size is what the agent's TUI draws against — cursor addressing and
    /// line erasing are relative to it — so anything rendering the pane's
    /// bytes has to use the same one. It is not fixed: tmux creates these
    /// sessions at its default size and resizes the window to whatever client
    /// attaches to it later.
    ///
    /// The cursor matters for the same reason. A capture says what is on the
    /// screen but not where the pane was about to write next, and a TUI's next
    /// repaint is addressed from there.
    pub async fn pane_geometry(&self, name: &str) -> Result<PaneGeometry> {
        let out = self
            .tmux(&[
                "display-message",
                "-p",
                "-t",
                name,
                "#{pane_width}x#{pane_height} #{cursor_x},#{cursor_y}",
            ])
            .await?;
        let raw = out.trim();
        parse_geometry(raw).with_context(|| format!("unexpected pane geometry for {name}: {raw:?}"))
    }

    /// Type `text` into the session followed by Enter (used to nudge
    /// interactive agents).
    pub async fn send_text(&self, name: &str, text: &str) -> Result<()> {
        // -l = literal (no key-name lookup), then a separate Enter press.
        self.tmux(&["send-keys", "-t", name, "-l", text]).await?;
        self.tmux(&["send-keys", "-t", name, "Enter"]).await?;
        Ok(())
    }

    /// Type `data` into the session's pane exactly as given: no Enter
    /// appended, no key-name lookup, no shell-quoting hazards.
    ///
    /// This is what a terminal in front of a user produces — `\r` for Return,
    /// `\x03` for Ctrl-C, `\x1b[A` for Up — so the bytes have to reach the
    /// pane untouched. `-H` takes one hexadecimal byte per argument, which is
    /// the only `send-keys` form that carries control bytes and escape
    /// sequences through verbatim; `-l` would still mangle a leading `-` and
    /// key names would reinterpret the rest.
    pub async fn send_raw(&self, name: &str, data: &[u8]) -> Result<()> {
        // tmux is exec'd, so the whole payload rides in argv — three bytes of
        // argument per input byte. Long pastes are split to stay clear of
        // ARG_MAX; a pane receives them as one uninterrupted burst either way.
        for batch in data.chunks(RAW_SEND_BATCH) {
            let mut args: Vec<String> =
                vec!["send-keys".into(), "-t".into(), name.into(), "-H".into()];
            args.extend(batch.iter().map(|b| format!("{b:02x}")));
            let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
            self.tmux(&arg_refs).await?;
        }
        Ok(())
    }

    /// Paste `text` into the session as one bracketed paste, then press
    /// Enter to submit it.
    ///
    /// For delivering a whole (multi-line) prompt to an agent TUI.
    /// [`Self::send_text`] cannot carry one: every newline byte in a `-l`
    /// send acts as its own Enter, so the message would submit in fragments.
    /// Wrapped in bracketed-paste markers, the same bytes arrive as a single
    /// paste event — exactly what a terminal in front of a user would send —
    /// and the trailing Enter submits the one assembled message.
    pub async fn send_paste(&self, name: &str, text: &str) -> Result<()> {
        let mut data = Vec::with_capacity(text.len() + 12);
        data.extend_from_slice(b"\x1b[200~");
        data.extend_from_slice(text.as_bytes());
        data.extend_from_slice(b"\x1b[201~");
        self.send_raw(name, &data).await?;
        self.send_enter(name).await
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

/// Short display form of a ULID: ULIDs are long; the trailing 8 chars are
/// the distinctive random part.
pub(crate) fn tail(id: &str) -> &str {
    &id[id.len().saturating_sub(8)..]
}

/// Build the canonical tmux session name for an agent session.
///
/// The name is the session's identity for its whole life, so it names what
/// does not change: the goal, the task, the role — and, for a reviewer, which
/// reviewer (`suffix`), since a task can have several and each keeps one
/// session across every review round.
pub fn session_name(
    goal_id: &str,
    task_id: Option<&str>,
    role: &str,
    suffix: Option<&str>,
) -> String {
    let mut name = format!("ariadne-{}", tail(goal_id));
    if let Some(task) = task_id {
        name.push_str(&format!("-{}", tail(task)));
    }
    name.push_str(&format!("-{}", &role[..3.min(role.len())]));
    if let Some(suffix) = suffix {
        name.push_str(&format!("-{suffix}"));
    }
    name
}
