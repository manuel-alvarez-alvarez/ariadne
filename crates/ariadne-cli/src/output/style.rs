//! Colour and glyphs: the one place that decides what a status looks like.
//!
//! Two rules. A status is never colour alone — it carries a glyph as well, so
//! the table reads the same to a colour-blind eye, through `NO_COLOR`, and in
//! a pipe. And colour is decided once, from `--color`, `NO_COLOR` and whether
//! stdout is a terminal, so every surface of the CLI agrees on it.
//!
//! The glyph set, which `--color`'s help documents for the reader:
//!
//! | Glyph | Meaning |
//! | --- | --- |
//! | `●` | running: something is working on it |
//! | `○` | pending: waiting for its turn |
//! | `✓` | done: merged, completed, exited |
//! | `✗` | failed or cancelled |
//! | `?` | waiting on you |

use std::io::IsTerminal;

use anstyle::{Ansi256Color, AnsiColor, Style};

/// When to colour output.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum ColorChoice {
    /// Colour a terminal, unless `NO_COLOR` says otherwise.
    #[default]
    Auto,
    Always,
    Never,
}

impl ColorChoice {
    /// Whether output carries escapes.
    ///
    /// `auto` is the docker rule plus [`NO_COLOR`](https://no-color.org): a
    /// terminal is coloured, a pipe is not, and any non-empty `NO_COLOR`
    /// takes the terminal out of it. An explicit `--color` outranks all of
    /// it, which is what makes `--color always | cat` a thing one can ask for.
    pub fn enabled(self) -> bool {
        match self {
            ColorChoice::Always => true,
            ColorChoice::Never => false,
            ColorChoice::Auto => !no_color() && std::io::stdout().is_terminal(),
        }
    }

    /// The same choice as clap's, for the help it colours itself.
    pub fn for_clap(self) -> clap::ColorChoice {
        match self {
            ColorChoice::Always => clap::ColorChoice::Always,
            ColorChoice::Never => clap::ColorChoice::Never,
            ColorChoice::Auto => match no_color() {
                // clap's `Auto` asks the terminal but not `NO_COLOR`.
                true => clap::ColorChoice::Never,
                false => clap::ColorChoice::Auto,
            },
        }
    }

    /// The choice a command line carries, read before clap has parsed it.
    ///
    /// clap colours the help it prints while parsing — including the usage
    /// error that never reaches our code — so the flag has to be found before
    /// the parse it is an argument to. Both spellings, and nothing else: an
    /// unreadable value leaves the default for clap itself to refuse.
    pub fn from_argv<I: IntoIterator<Item = String>>(argv: I) -> Self {
        let mut argv = argv.into_iter();
        while let Some(arg) = argv.next() {
            let value = match arg.strip_prefix("--color") {
                Some("") => argv.next(),
                Some(rest) => rest.strip_prefix('=').map(str::to_owned),
                None => None,
            };
            if let Some(value) = value {
                return match value.as_str() {
                    "always" => ColorChoice::Always,
                    "never" => ColorChoice::Never,
                    _ => ColorChoice::Auto,
                };
            }
        }
        ColorChoice::Auto
    }
}

/// Whether the environment forbids colour: any `NO_COLOR` with something in it.
fn no_color() -> bool {
    std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty())
}

/// The id column: present, and never the thing being read.
pub const ID: Style = Style::new().dimmed();

/// The title column: what the row is about.
pub const TITLE: Style = Style::new().bold();

/// A note that something is off, for the lines that are not table cells.
pub const WARN: Style = Style::new().fg_color(Some(anstyle::Color::Ansi(AnsiColor::Yellow)));

/// `changes_requested`: past a warning, short of a failure. No ANSI-16 colour
/// sits between yellow and red, so it is the 256-colour orange.
const ORANGE: Style = Style::new().fg_color(Some(anstyle::Color::Ansi256(Ansi256Color(208))));

/// A glyph and a colour for one status, in any of the three lifecycles —
/// task, goal and session all pass through here so that "done" looks the same
/// whatever is done.
///
/// An unknown status keeps its word and loses the glyph: a spelling this build
/// does not know is still a fact, and inventing a glyph for it would be a
/// guess about which of the five it belongs to.
pub fn status(word: &str) -> (Style, Option<char>) {
    match word {
        // Waiting for its turn: a task with dependencies, an idle agent.
        "pending" | "idle" => (grey(), Some(PENDING)),
        // Something is working on it.
        "ready" | "in_progress" | "planning" | "active" | "starting" | "running" => {
            (blue(), Some(RUNNING))
        }
        "under_review" => (yellow(), Some(RUNNING)),
        "changes_requested" => (ORANGE, Some(RUNNING)),
        // Approved is still being landed; merged is the end of it.
        "approved" => (green(), Some(RUNNING)),
        "merged" | "completed" => (green(), Some(DONE)),
        // A session that exited did its work and stopped: done, not failed.
        "exited" => (grey(), Some(DONE)),
        "failed" | "cancelled" => (red(), Some(FAILED)),
        _ => (Style::new(), None),
    }
}

/// A glyph and a colour for a row that is waiting on a person, spelled in
/// `ariadne attention`'s words.
///
/// Magenta and `?` for everything a person is expected to answer; red and `✗`
/// for the two that are a breakage rather than a question. `-` — the cell of a
/// row that wants nothing — stays as plain as it reads.
pub fn attention(label: &str) -> (Style, Option<char>) {
    match label {
        "-" => (Style::new(), None),
        "agent error" | "disconnected" | "failed" => (red(), Some(FAILED)),
        _ => (magenta(), Some(WAITING)),
    }
}

/// Something is working on it.
pub const RUNNING: char = '●';
/// Waiting for its turn.
pub const PENDING: char = '○';
/// Finished: merged, completed, exited.
pub const DONE: char = '✓';
/// Failed or cancelled.
pub const FAILED: char = '✗';
/// Waiting on you.
pub const WAITING: char = '?';

const fn grey() -> Style {
    fg(AnsiColor::BrightBlack)
}
const fn blue() -> Style {
    fg(AnsiColor::Blue)
}
const fn yellow() -> Style {
    fg(AnsiColor::Yellow)
}
const fn green() -> Style {
    fg(AnsiColor::Green)
}
const fn red() -> Style {
    fg(AnsiColor::Red)
}
const fn magenta() -> Style {
    fg(AnsiColor::Magenta)
}

const fn fg(color: AnsiColor) -> Style {
    Style::new().fg_color(Some(anstyle::Color::Ansi(color)))
}

/// `text` in `style` — or `text` untouched when colour is off, which is what
/// keeps a pipe free of escapes without every caller asking twice.
pub fn paint(color: bool, style: Style, text: &str) -> String {
    match color && style != Style::new() {
        true => format!("{}{text}{}", style.render(), style.render_reset()),
        false => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glyph(word: &str) -> Option<char> {
        status(word).1
    }

    /// Every status of every lifecycle carries one of the five glyphs, and the
    /// ones that mean the same thing carry the same one.
    #[test]
    fn a_status_maps_to_a_glyph_of_the_documented_set() {
        assert_eq!(glyph("pending"), Some(PENDING));
        assert_eq!(glyph("idle"), Some(PENDING));
        assert_eq!(glyph("ready"), Some(RUNNING));
        assert_eq!(glyph("in_progress"), Some(RUNNING));
        assert_eq!(glyph("under_review"), Some(RUNNING));
        assert_eq!(glyph("changes_requested"), Some(RUNNING));
        assert_eq!(glyph("approved"), Some(RUNNING));
        assert_eq!(glyph("merged"), Some(DONE));
        assert_eq!(glyph("completed"), Some(DONE));
        assert_eq!(glyph("exited"), Some(DONE));
        assert_eq!(glyph("failed"), Some(FAILED));
        assert_eq!(glyph("cancelled"), Some(FAILED));
        assert_eq!(glyph("planning"), Some(RUNNING));
        assert_eq!(glyph("active"), Some(RUNNING));
        assert_eq!(glyph("starting"), Some(RUNNING));
        assert_eq!(glyph("running"), Some(RUNNING));
    }

    /// Every status the daemon can send is one this table knows: a lifecycle
    /// that grows a variant fails here rather than printing a bare word.
    #[test]
    fn every_status_of_every_lifecycle_is_covered() {
        let words = ariadne_core::TaskStatus::ALL
            .iter()
            .map(|s| s.as_str())
            .chain(ariadne_core::GoalStatus::ALL.iter().map(|s| s.as_str()))
            .chain(ariadne_core::SessionStatus::ALL.iter().map(|s| s.as_str()));
        for word in words {
            assert!(glyph(word).is_some(), "no glyph for {word}");
        }
    }

    /// A word this build does not know keeps its cell and loses the glyph,
    /// rather than being guessed into a category.
    #[test]
    fn an_unknown_status_is_left_alone() {
        assert_eq!(status("integrating"), (Style::new(), None));
    }

    /// The colours the reader is told about: blue for work in flight, green
    /// for done, red for broken, magenta for "you".
    #[test]
    fn a_status_is_coloured_by_what_it_means() {
        assert_eq!(status("in_progress").0, blue());
        assert_eq!(status("ready").0, blue());
        assert_eq!(status("pending").0, grey());
        assert_eq!(status("under_review").0, yellow());
        assert_eq!(status("changes_requested").0, ORANGE);
        assert_eq!(status("approved").0, green());
        assert_eq!(status("merged").0, green());
        assert_eq!(status("failed").0, red());
        assert_eq!(status("cancelled").0, red());
        assert_eq!(
            attention("waiting for permission"),
            (magenta(), Some(WAITING))
        );
        assert_eq!(attention("waiting for you"), (magenta(), Some(WAITING)));
        assert_eq!(attention("stalled"), (magenta(), Some(WAITING)));
        assert_eq!(attention("agent error"), (red(), Some(FAILED)));
        assert_eq!(attention("disconnected"), (red(), Some(FAILED)));
        assert_eq!(attention("-"), (Style::new(), None));
    }

    /// Colour is escapes or nothing: the same string, wrapped or bare.
    #[test]
    fn paint_writes_escapes_only_when_colour_is_on() {
        assert_eq!(paint(false, TITLE, "Ship it"), "Ship it");
        assert_eq!(paint(true, Style::new(), "Ship it"), "Ship it");
        let painted = paint(true, TITLE, "Ship it");
        assert!(painted.starts_with("\u{1b}["), "{painted:?}");
        assert!(painted.ends_with("\u{1b}[0m"), "{painted:?}");
        assert!(painted.contains("Ship it"), "{painted:?}");
    }

    /// `--color` is read off the command line before clap parses it, because
    /// clap colours its own help and usage errors on the way through.
    #[test]
    fn the_colour_choice_is_readable_before_the_parse() {
        let argv = |args: &[&str]| ColorChoice::from_argv(args.iter().map(|a| (*a).to_string()));
        assert_eq!(argv(&["ariadne", "task", "ls"]), ColorChoice::Auto);
        assert_eq!(
            argv(&["ariadne", "--color", "never", "task", "ls"]),
            ColorChoice::Never
        );
        assert_eq!(
            argv(&["ariadne", "--color=always", "task", "ls"]),
            ColorChoice::Always
        );
        assert_eq!(
            argv(&["ariadne", "task", "msg", "01T", "--color always"]),
            ColorChoice::Auto,
            "a message body that merely mentions the flag is not the flag"
        );
        assert_eq!(argv(&["ariadne", "--color"]), ColorChoice::Auto);
        assert_eq!(argv(&["ariadne", "--color", "purple"]), ColorChoice::Auto);
    }

    /// `always` and `never` are answers, not preferences: neither the
    /// terminal nor `NO_COLOR` overrules them.
    #[test]
    fn an_explicit_choice_outranks_the_environment() {
        assert!(ColorChoice::Always.enabled());
        assert!(!ColorChoice::Never.enabled());
        assert_eq!(ColorChoice::Always.for_clap(), clap::ColorChoice::Always);
        assert_eq!(ColorChoice::Never.for_clap(), clap::ColorChoice::Never);
    }
}
