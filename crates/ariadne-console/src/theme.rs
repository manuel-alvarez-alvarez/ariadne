//! How the pane looks: the colour of every kind of line, and the glyph and
//! marker that says what a line is before a word of it is read.
//!
//! Every style and every glyph of the pane is named here, so that one place
//! holds the look and a change of it touches no drawing code.

use ratatui::style::{Color, Modifier, Style};

pub const USER: Style = Style::new().fg(Color::Cyan);
/// A prompt the daemon sent: the user's colour, dimmed — it was said to the
/// agent on the user's behalf, not by them.
pub const DAEMON: Style = Style::new().fg(Color::Cyan).add_modifier(Modifier::DIM);
pub const AGENT: Style = Style::new().fg(Color::Green);
pub const TOOL: Style = Style::new().fg(Color::Yellow);
pub const PLAN: Style = Style::new().fg(Color::Blue);
pub const ASK: Style = Style::new().fg(Color::Magenta);
pub const FAIL: Style = Style::new().fg(Color::Red).add_modifier(Modifier::BOLD);
pub const DIM: Style = Style::new().add_modifier(Modifier::DIM);
/// The lines of a diff: added, removed, the hunk header, and the file header.
pub const ADDED: Style = Style::new().fg(Color::Green);
pub const REMOVED: Style = Style::new().fg(Color::Red);
pub const HUNK: Style = Style::new().fg(Color::Cyan);
pub const FILE: Style = Style::new().add_modifier(Modifier::BOLD);

/// The frames of the spinner, one per tick while a turn runs.
pub const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// Where a block starts: what was typed, what the agent said, what it
/// thought, what the daemon sent, and what went wrong.
pub const USER_MARKER: &str = "> ";
pub const AGENT_MARKER: &str = "● ";
pub const THOUGHT_MARKER: &str = "· ";
pub const DAEMON_MARKER: &str = "» daemon";
pub const ERROR_MARKER: &str = "✗ ";

/// How a call stands, at the head of its line.
pub const CALL_DONE: &str = "✓";
pub const CALL_FAILED: &str = "✗";
pub const CALL_RUNNING: &str = "●";
pub const CALL_PENDING: &str = "○";

/// One glyph per ACP kind of call, so the eye tells a command from a read from
/// an edit before reading a word, and the glyph of a kind that is none of
/// them.
pub const KINDS: &[(&str, &str)] = &[
    ("execute", "$"),
    ("read", "≡"),
    ("edit", "✎"),
    ("delete", "⌫"),
    ("move", "→"),
    ("search", "⌕"),
    ("fetch", "↓"),
    ("think", "∴"),
    ("switch_mode", "⇄"),
];
pub const KIND_OTHER: &str = "•";

/// A plan entry, done and not yet.
pub const PLAN_DONE: &str = "  ☑ ";
pub const PLAN_TODO: &str = "  ☐ ";

/// A permission question, and the option the picker is on.
pub const ASK_MARKER: &str = "? ";
pub const OPTION_PICKED: &str = "  › ";
