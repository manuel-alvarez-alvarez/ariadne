//! The long output: a diff, a log snapshot — coloured, and handed to `$PAGER`
//! when there is a person at the other end.
//!
//! Paging is for terminals only. A pipe, a file and `--format json` get the
//! text on stdout exactly as they always did, so `task diff | git apply` is
//! still a diff and not a screenful of pager control codes.

use std::io::{IsTerminal, Write};
use std::process::{Command, Stdio};

use anstyle::{AnsiColor, Color, Style};
use anyhow::Result;

use super::{style::paint, view};

/// What `$PAGER` falls back to. `-R` because the text handed to it is
/// coloured, and a pager that does not pass the escapes through prints them.
const PAGER: &[&str] = &["less", "-R"];

/// `less` options for the pager we start ourselves, unless the reader has
/// their own `LESS`: quit if the text fits on one screen (`F`), leave it on
/// the screen when it does (`X`), and pass the colours through (`R`). Git's
/// defaults, because this is git's situation.
const LESS: &str = "FRX";

/// Print `text`, through the pager when stdout is a terminal and `--no-pager`
/// did not say otherwise.
///
/// A pager that cannot be started is not a failure of the command that had
/// something to show: the text goes to stdout instead.
pub fn page(text: &str) -> Result<()> {
    let Some(mut child) = spawn() else {
        print!("{text}");
        std::io::stdout().flush()?;
        return Ok(());
    };
    if let Some(mut stdin) = child.stdin.take() {
        // A reader who quits the pager early closes the pipe under us. That
        // is how one leaves a pager, not an error to report.
        if let Err(e) = stdin.write_all(text.as_bytes())
            && e.kind() != std::io::ErrorKind::BrokenPipe
        {
            return Err(e.into());
        }
    }
    child.wait()?;
    Ok(())
}

/// The pager, started and waiting for the text — or nothing at all when
/// paging is off, stdout is not a terminal, or `$PAGER` cannot be run.
fn spawn() -> Option<std::process::Child> {
    if !view().pager || !std::io::stdout().is_terminal() {
        return None;
    }
    let configured = std::env::var("PAGER").ok().filter(|p| !p.trim().is_empty());
    let words: Vec<String> = match &configured {
        Some(pager) => pager.split_whitespace().map(str::to_owned).collect(),
        None => PAGER.iter().map(|w| (*w).to_string()).collect(),
    };
    let (program, args) = words.split_first()?;
    // `cat` as a PAGER means "no pager"; so does a pager we cannot start.
    let mut cmd = Command::new(program);
    cmd.args(args).stdin(Stdio::piped());
    if std::env::var_os("LESS").is_none() {
        cmd.env("LESS", LESS);
    }
    cmd.spawn().ok()
}

/// A unified diff with its structure coloured: what was added, what was
/// taken away, and where each hunk starts.
///
/// The `+++`/`---` file headers are checked before the `+`/`-` lines they
/// would otherwise be read as: a header is not an added line.
pub fn diff(text: &str, color: bool) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let body = line.trim_end_matches(['\n', '\r']);
        let style = if body.starts_with("+++") || body.starts_with("---") {
            BOLD
        } else if body.starts_with("@@") {
            CYAN
        } else if body.starts_with('+') {
            GREEN
        } else if body.starts_with('-') {
            RED
        } else if is_header(body) {
            BOLD
        } else {
            Style::new()
        };
        out.push_str(&paint(color, style, body));
        out.push_str(&line[body.len()..]);
    }
    out
}

/// The lines git puts above a hunk: which file, and what happened to it.
fn is_header(line: &str) -> bool {
    const HEADERS: &[&str] = &[
        "diff --git",
        "index ",
        "new file",
        "deleted file",
        "old mode",
        "new mode",
        "similarity index",
        "rename ",
        "copy ",
        "Binary files",
    ];
    HEADERS.iter().any(|h| line.starts_with(h))
}

const BOLD: Style = Style::new().bold();
const GREEN: Style = fg(AnsiColor::Green);
const RED: Style = fg(AnsiColor::Red);
const CYAN: Style = fg(AnsiColor::Cyan);

const fn fg(color: AnsiColor) -> Style {
    Style::new().fg_color(Some(Color::Ansi(color)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIFF: &str = concat!(
        "diff --git a/a.rs b/a.rs\n",
        "index 1234567..89abcde 100644\n",
        "--- a/a.rs\n",
        "+++ b/a.rs\n",
        "@@ -1,3 +1,3 @@\n",
        " context\n",
        "-gone\n",
        "+added\n",
    );

    /// Colour off, byte for byte the diff the daemon sent: `task diff | git
    /// apply` has to keep working.
    #[test]
    fn a_diff_is_untouched_when_colour_is_off() {
        assert_eq!(diff(DIFF, false), DIFF);
        assert_eq!(diff("", false), "");
    }

    /// Added, removed, hunk and header each in their own colour — and the
    /// `+++`/`---` headers as headers rather than as the additions and
    /// deletions they start like.
    #[test]
    fn a_diff_is_coloured_by_what_each_line_is() {
        let painted = diff(DIFF, true);
        let line = |body: &str, style: Style| format!("{}\n", paint(true, style, body));
        assert!(
            painted.contains(&line("diff --git a/a.rs b/a.rs", BOLD)),
            "{painted:?}"
        );
        assert!(painted.contains(&line("--- a/a.rs", BOLD)), "{painted:?}");
        assert!(painted.contains(&line("+++ b/a.rs", BOLD)), "{painted:?}");
        assert!(
            painted.contains(&line("@@ -1,3 +1,3 @@", CYAN)),
            "{painted:?}"
        );
        assert!(painted.contains(&line("-gone", RED)), "{painted:?}");
        assert!(painted.contains(&line("+added", GREEN)), "{painted:?}");
        assert!(
            painted.contains(" context\n"),
            "context is left alone: {painted:?}"
        );
    }

    /// The text keeps its shape: same lines, same order, and a last line with
    /// no newline still has none.
    #[test]
    fn colouring_changes_nothing_but_the_colours() {
        let strip = |s: &str| {
            let mut out = String::new();
            let mut escaped = false;
            for c in s.chars() {
                match (escaped, c) {
                    (false, '\u{1b}') => escaped = true,
                    (true, 'm') => escaped = false,
                    (true, _) => {}
                    (false, c) => out.push(c),
                }
            }
            out
        };
        assert_eq!(strip(&diff(DIFF, true)), DIFF);
        assert_eq!(
            strip(&diff("+no trailing newline", true)),
            "+no trailing newline"
        );
    }
}
