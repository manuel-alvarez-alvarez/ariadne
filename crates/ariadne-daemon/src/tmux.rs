//! TmuxManager: session lifecycle for agent processes.
//!
//! Shells out to `tmux` — boring and reliable. Sessions are created detached;
//! the user attaches with `tmux attach -t <name>` (via `ariadne attach`).

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use tokio::process::Command;

/// Input bytes per `send-keys -H` call; see [`TmuxManager::send_raw`].
const RAW_SEND_BATCH: usize = 512;
/// How long a pane is left alone between the end of a paste and the Enter
/// meant to submit it, so a TUI that reads fast input as a paste has closed
/// that window. Codex needs the beat; without it the Enter becomes a newline.
const PASTE_SETTLE: Duration = Duration::from_millis(400);
/// How long a TUI gets to redraw after an Enter before its pane is read for
/// whether the message went. Doubled for each further attempt.
const SUBMIT_SETTLE: Duration = Duration::from_millis(600);
/// How many Enters one delivery is worth before it is given up on.
const SUBMIT_ATTEMPTS: u32 = 3;
/// Shortest composer row taken as evidence that the message is still there.
/// A row of one or two characters is a bar or a bullet, and short enough to
/// turn up inside any message by chance.
const COMPOSER_ROW_MIN: usize = 4;

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

    /// Resize the session's window to `cols`×`rows`, and leave it there.
    ///
    /// This is what a `tmux attach` does for a pane — the client's size
    /// becomes the window's — done for a viewer that is not a tmux client at
    /// all. The pane's TUI redraws itself against the new grid, and the log
    /// stream reports it (see `http/session_logs.rs`), so the size a viewer
    /// asks for is the size every viewer then renders at.
    ///
    /// Sizing is taken off tmux's hands to do it. The default `window-size`
    /// of `latest` sizes a window after the client last attached to it, and a
    /// window with no client at all is left at tmux's default 80×24: a
    /// detached session — which is every session here until someone attaches
    /// — would ignore the resize. `manual` is what makes the size ours, and
    /// `resize-window -x/-y` sets it of its own accord anyway; it is set here
    /// too so the behaviour is stated rather than inherited.
    ///
    /// Which would leave sizing ours for good, including for the next
    /// `ariadne attach` — a client would then be shown a window it did not
    /// fit, cropped, instead of resizing it the way attaching always has. So
    /// the `client-attached` hook hands sizing straight back: the moment a
    /// real client arrives, `window-size` is unset and the window follows it
    /// again. Last resize wins either way, which is what tmux itself does
    /// with several clients.
    ///
    /// All of it goes out as one `tmux` invocation: three commands, one
    /// process, and no window that is briefly `manual` at the old size.
    pub async fn resize_window(&self, name: &str, cols: u16, rows: u16) -> Result<()> {
        let cols = cols.to_string();
        let rows = rows.to_string();
        self.tmux(&[
            "set-hook",
            "-t",
            name,
            "client-attached",
            "set-window-option -u window-size",
            ";",
            "set-window-option",
            "-t",
            name,
            "window-size",
            "manual",
            ";",
            "resize-window",
            "-t",
            name,
            "-x",
            &cols,
            "-y",
            &rows,
        ])
        .await
        .map(|_| ())
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

    /// Deliver `text` to an agent's TUI and keep pressing Enter until the pane
    /// shows it left the composer. Returns whether that was confirmed.
    ///
    /// Everything here exists because "tmux accepted the keystrokes" is not
    /// "the agent got the message". The text goes in as one bracketed paste —
    /// a `-l` send types it byte by byte, which a coding-agent TUI reads as a
    /// burst and, with Codex, classifies as a paste anyway: an Enter landing
    /// inside that window becomes a newline in the composer instead of a
    /// submission, and the message sits there unsent until a human presses
    /// Return. So the paste is explicit, the Enter comes a beat later with the
    /// burst window shut, and the pane itself is asked whether the composer
    /// let go of the message.
    ///
    /// "The pane changed" is no answer: a swallowed Enter changes it too, by
    /// inserting that newline. What is asked instead is whether the composer
    /// still holds the message — see [`composer_holds`] — and every Enter that
    /// leaves it there earns another, with a widening backoff, up to
    /// [`SUBMIT_ATTEMPTS`]. An unconfirmed delivery is the caller's to report:
    /// it is a session that never heard what it was told.
    pub async fn send_submitted(&self, name: &str, text: &str) -> Result<bool> {
        self.paste(name, text).await?;
        // Let the TUI close its paste window before the Enter, or the Enter is
        // part of the paste.
        tokio::time::sleep(PASTE_SETTLE).await;
        let mut settle = SUBMIT_SETTLE;
        for attempt in 1..=SUBMIT_ATTEMPTS {
            self.send_enter(name).await?;
            tokio::time::sleep(settle).await;
            // Each swallowed Enter pushes the composer down by the newline it
            // inserted, so the rows worth reading grow with the attempts.
            let screen = self.capture_screen(name).await?;
            let cursor = self.pane_geometry(name).await?.cursor_y;
            if !composer_holds(&screen, cursor, attempt as usize, text) {
                return Ok(true);
            }
            tracing::debug!(
                session = %name,
                attempt,
                "the message is still in the agent's composer; pressing Enter again"
            );
            settle *= 2;
        }
        Ok(false)
    }

    /// Put `text` in the pane's composer as one bracketed paste, pressing
    /// nothing.
    ///
    /// A `-l` send cannot carry a whole (multi-line) prompt: every newline
    /// byte in it acts as its own Enter, so the message would submit in
    /// fragments. Wrapped in bracketed-paste markers, the same bytes arrive as
    /// a single paste event — exactly what a terminal in front of a user would
    /// send — and the composer assembles one message out of them.
    async fn paste(&self, name: &str, text: &str) -> Result<()> {
        let mut data = Vec::with_capacity(text.len() + 12);
        data.extend_from_slice(b"\x1b[200~");
        data.extend_from_slice(text.as_bytes());
        data.extend_from_slice(b"\x1b[201~");
        self.send_raw(name, &data).await
    }

    /// The pane's visible screen as plain text: no scrollback, no escape
    /// sequences. What [`composer_holds`] reads.
    async fn capture_screen(&self, name: &str) -> Result<String> {
        self.tmux(&["capture-pane", "-p", "-t", name]).await
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

/// Whether the pane's composer is still holding `text`, one `above` row per
/// Enter already pressed — which is to say, whether the Enter was swallowed.
///
/// The cursor is the anchor. Whatever a TUI's layout is — Codex's composer
/// floats under its transcript, OpenCode's and Claude Code's sit in a box at
/// the foot of the screen — the cursor is in the composer, so the rows that
/// can hold an unsent message are the cursor's own and the few over it: a
/// swallowed Enter inserts a newline, which pushes the text up exactly one row
/// per attempt. Rows further up are the transcript, and a message that *did*
/// submit is drawn there; reading them would call every delivery a failure.
///
/// A row is evidence when what it says — stripped of the prompt marks and
/// rules a TUI draws around a composer — is a piece of the message. A piece,
/// not the whole of it: composers wrap long lines and show tall messages
/// tail-first, so what is on screen is some contiguous slice. The other tell
/// is a placeholder: Codex and OpenCode both collapse a long paste into a
/// `[Pasted …]` summary, and then none of the message's own text is drawn at
/// all.
fn composer_holds(screen: &str, cursor_y: u16, above: usize, text: &str) -> bool {
    let rows: Vec<&str> = screen.lines().collect();
    let cursor = (cursor_y as usize).min(rows.len().saturating_sub(1));
    let first = cursor.saturating_sub(above);
    rows.get(first..=cursor)
        .unwrap_or_default()
        .iter()
        .any(|row| {
            let row = strip_chrome(row);
            is_paste_placeholder(row)
                || (row.chars().count() >= COMPOSER_ROW_MIN && text.contains(row))
        })
}

/// A composer row with the furniture taken off: the bars, rules and prompt
/// marks a TUI draws around what was typed into it.
fn strip_chrome(row: &str) -> &str {
    row.trim_matches(|c: char| {
        c.is_whitespace()
            || matches!(c, '>' | '|' | '*' | '-' | '\u{b7}' | '\u{2022}')
            // Box drawing and block elements: every bar and rule a TUI frames
            // its composer with.
            || ('\u{2500}'..='\u{259f}').contains(&c)
            // The single-angle and heavy-angle quotes used as prompt marks.
            || matches!(c, '\u{ab}' | '\u{bb}' | '\u{2039}' | '\u{203a}' | '\u{276f}')
    })
}

/// A composer that swallowed our paste into a summary of it: `[Pasted Content
/// 1446 chars]` in Codex, `[Pasted ~12 lines]` in OpenCode.
fn is_paste_placeholder(row: &str) -> bool {
    row.to_ascii_lowercase().contains("[pasted")
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

#[cfg(test)]
mod tests {
    use super::*;

    const NUDGE: &str =
        "Finish reviewing this round and submit your verdict with `approve` or `request_changes`.";

    /// Codex, message unsent: the Enter it swallowed became the empty row the
    /// cursor is on, one under the message.
    #[test]
    fn a_message_left_in_a_composer_is_seen() {
        let screen = format!(
            "  I'll take a look.\n\n\n\u{203a} {NUDGE}\n\n\n  gpt-5.6-terra medium \u{b7} /work\n"
        );
        assert!(composer_holds(&screen, 4, 1, NUDGE));
    }

    /// The same pane once it submitted: the composer is back to its prompt and
    /// the message is up in the transcript, where it is not read.
    #[test]
    fn a_submitted_message_is_not_mistaken_for_a_waiting_one() {
        let screen = format!(
            "\u{203a} {NUDGE}\n\n\u{2022} Working (1s \u{b7} esc to interrupt)\n\n\n\u{203a} Ask Codex to do anything\n\n  gpt-5.6-terra medium \u{b7} /work\n"
        );
        assert!(!composer_holds(&screen, 5, 1, NUDGE));
    }

    /// A composer wraps what it is given, so no row holds the whole message —
    /// each holds a slice of it, and the last one can be a few characters.
    #[test]
    fn a_wrapped_message_is_seen_by_its_tail() {
        let screen = "\u{203a} Finish reviewing this round and submit your verdict with `approve` or `req\nuest_changes.`\n";
        assert!(composer_holds(screen, 1, 1, NUDGE));
    }

    /// A long paste is collapsed into a summary of itself, so none of its own
    /// text is on screen to look for.
    #[test]
    fn a_collapsed_paste_is_seen_by_its_placeholder() {
        let screen = "\u{2503}\n\u{2503}  [Pasted ~12 lines]\n\u{2503}\n\u{2503}  Build \u{b7} Claude Opus 5\n";
        assert!(composer_holds(screen, 1, 1, "a long resume instruction"));
    }

    /// An empty composer is bars and a placeholder of the TUI's own, whatever
    /// the message was.
    #[test]
    fn an_empty_composer_holds_nothing() {
        let screen = "\u{2503}\n\u{2503}  Ask anything... \"Fix broken tests\"\n\u{2503}\n\u{2503}  Build \u{b7} Claude Opus 5\n\u{2570}\u{2500}\u{2500}\u{2500}\u{2500}\n";
        assert!(!composer_holds(screen, 1, 1, NUDGE));
        // Claude Code's, which is a rule, a prompt mark and nothing else.
        let screen = "\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n\u{276f} \n\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n";
        assert!(!composer_holds(screen, 1, 1, NUDGE));
    }

    /// Every Enter that is swallowed pushes the message another row up, so the
    /// rows read grow with the attempts — otherwise the second look would find
    /// only the newlines the first one made.
    #[test]
    fn the_rows_read_grow_with_the_attempts() {
        let screen = format!("\u{203a} {NUDGE}\n\n\n");
        assert!(!composer_holds(&screen, 2, 1, NUDGE), "one row up is short");
        assert!(composer_holds(&screen, 2, 2, NUDGE), "two rows up finds it");
    }

    /// A pane whose program keeps no composer at all — the cursor sits below
    /// everything it printed — is read from the rows over the cursor all the
    /// same, rather than from an empty slice.
    #[test]
    fn a_cursor_past_the_last_row_reads_what_is_over_it() {
        let screen = format!("\u{203a} {NUDGE}\n");
        assert!(composer_holds(&screen, 40, 1, NUDGE));
    }
}
