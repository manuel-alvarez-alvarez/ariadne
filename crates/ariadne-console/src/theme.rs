//! How the console looks: the colour of every kind of line, and the glyph and
//! marker that says what a line is before a word of it is read.
//!
//! Every style and every glyph of the console is named here, so that one place
//! holds the look and a change of it touches no drawing code.

use ratatui::style::{Color, Modifier, Style};

/// Background for prose supplied as untrusted data.
pub const UNTRUSTED: Style = Style::new().bg(Color::DarkGray);
pub const USER: Style = Style::new().fg(Color::Cyan);
/// The text of a typed prompt: the terminal's own foreground, bold, so it
/// reads on a dark and a light theme alike. The bar and the marker carry the
/// user's colour.
pub const PROMPT: Style = Style::new().add_modifier(Modifier::BOLD);
/// A prompt the daemon sent: the user's colour, dimmed — it was said to the
/// agent on the user's behalf, not by them.
pub const DAEMON: Style = Style::new().fg(Color::Cyan).add_modifier(Modifier::DIM);
pub const AGENT: Style = Style::new().fg(Color::Green);
pub const TOOL: Style = Style::new().fg(Color::Yellow);
pub const PLAN: Style = Style::new().fg(Color::Blue);
pub const ASK: Style = Style::new().fg(Color::Magenta);
/// The question of a permission picker, and the option it is on.
pub const QUESTION: Style = Style::new().add_modifier(Modifier::BOLD);
pub const PICKED: Style = Style::new().fg(Color::Magenta).add_modifier(Modifier::BOLD);
/// The rules that frame the picker.
pub const FRAME: Style = Style::new().add_modifier(Modifier::DIM);
pub const FAIL: Style = Style::new().fg(Color::Red).add_modifier(Modifier::BOLD);
pub const DIM: Style = Style::new().add_modifier(Modifier::DIM);
/// A tool's output body: the terminal's own foreground, not dimmed. A file's
/// contents, a compiler error and a failing test all land here, and it is
/// the text a reader most wants — dimming it would make it the hardest to
/// read.
pub const OUTPUT: Style = Style::new();
/// Markdown headings separate the parts of an agent answer.
pub const HEADING: Style = Style::new()
    .fg(Color::Cyan)
    .add_modifier(Modifier::BOLD.union(Modifier::UNDERLINED));
/// A table's header row: a heading's colour, bold, and not underlined — the
/// rule drawn under the row is its line, and an underline would be a second.
pub const TABLE_HEADER: Style = Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD);
/// Markdown code spans and blocks use one quiet, distinct colour.
pub const CODE: Style = Style::new().fg(Color::Yellow);
/// A fenced block in a language its grammar knows, coloured by what each
/// part of a line is. The eight ANSI colours alone, so the terminal's own
/// theme decides each hue; what is none of these is plain.
pub const CODE_COMMENT: Style = Style::new().add_modifier(Modifier::DIM);
pub const CODE_STRING: Style = Style::new().fg(Color::Green);
pub const CODE_KEYWORD: Style = Style::new().fg(Color::Magenta);
pub const CODE_NUMBER: Style = Style::new().fg(Color::Cyan);
pub const CODE_TYPE: Style = Style::new().fg(Color::Yellow);
pub const CODE_PLAIN: Style = Style::new();
/// Markdown structure is context, not the answer itself.
pub const MARK: Style = Style::new().add_modifier(Modifier::DIM);
/// The lines of a diff: added, removed, the hunk header, and the file header.
pub const ADDED: Style = Style::new().fg(Color::Green);
pub const REMOVED: Style = Style::new().fg(Color::Red);
pub const HUNK: Style = Style::new().fg(Color::Cyan);
pub const FILE: Style = Style::new().add_modifier(Modifier::BOLD);

/// The frames of the spinner, one per tick while a turn runs.
pub const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// What stands between two items of the bottom row.
pub const SEPARATOR: &str = " · ";
pub const GAP: &str = "   ";
/// The hint on the right of the bottom row: `?` opens the list of keys.
pub const KEYS_HINT: &str = "? keys";
/// The mark of a session waiting for a prompt, and of one that has ended.
pub const SESSION_IDLE: &str = "●";
pub const SESSION_ENDED: &str = "■";

/// Where a block starts: what was typed, and what went wrong. A typed prompt
/// has the input box's glyph.
pub const USER_MARKER: &str = "❯ ";
/// The tag of a typed prompt the daemon has not taken yet.
pub const QUEUED: &str = "queued";
pub const ERROR_MARKER: &str = "✗ ";

/// The input box: its rule, first-row prompt, continued-row indent and hint.
pub const INPUT_RULE: &str = "─";
pub const INPUT_PROMPT: &str = "❯ ";
pub const INPUT_CONTINUATION: &str = "  ";
pub const INPUT_PLACEHOLDER: &str = "Tell the agent what to do";

/// How a call stands, at the head of its line. A call in progress has the
/// mark of a plan entry in progress; the agent's marker is another.
pub const CALL_DONE: &str = "✓";
pub const CALL_FAILED: &str = "✗";
pub const CALL_RUNNING: &str = "◐";
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
    ("fetch", "⇣"),
    ("think", "∴"),
    ("switch_mode", "⇄"),
];

/// The glyph of a tool call whose kind is none of those of [`KINDS`].
const KIND_FALLBACK: &str = "◆";

/// The glyph of a subagent, on its summary row.
pub const SUBAGENT: &str = "↳";

/// The glyph of a group of calls of more than one kind.
pub const GROUP: &str = "☰";

/// The glyph of a kind of call: its own, or the fallback.
pub fn kind_icon(kind: Option<&str>) -> &'static str {
    kind.and_then(|kind| KINDS.iter().find(|(name, _)| *name == kind))
        .map_or(KIND_FALLBACK, |(_, glyph)| glyph)
}

/// A plan entry: pending, in progress, and completed.
pub const PLAN_TODO: &str = "  ☐ ";
pub const PLAN_DOING: &str = "  ◐ ";
pub const PLAN_DONE: &str = "  ☑ ";

/// The picker of a permission question: the option it is on and the ones it
/// is not on.
pub const OPTION_PICKED: &str = "❯ ";
pub const OPTION_IDLE: &str = "  ";

/// Markdown marks, kept with the console's other visible vocabulary.
pub const RULE: &str = "─";
pub const QUOTE_BAR: &str = "│ ";
pub const CODE_CONTINUATION: &str = "↪";
pub const LIST_BULLETS: [&str; 3] = ["• ", "◦ ", "▪ "];
pub const TASK_DONE: &str = "☑ ";
pub const TASK_TODO: &str = "☐ ";

#[cfg(test)]
mod tests {
    use super::*;

    /// Every mark the transcript draws, with what it means there. Two marks
    /// of one glyph must mean one thing: a done entry of a plan and of a
    /// task list are both done, and a failed call and an error both failed.
    #[test]
    fn one_glyph_has_one_meaning_over_the_whole_transcript() {
        let mut marks = vec![
            (USER_MARKER, "the user's"),
            (INPUT_PROMPT, "the user's"),
            (OPTION_PICKED, "the user's"),
            (ERROR_MARKER, "failed"),
            (CALL_FAILED, "failed"),
            (CALL_DONE, "done"),
            (CALL_RUNNING, "in progress"),
            (CALL_PENDING, "pending"),
            (PLAN_TODO, "to do"),
            (PLAN_DOING, "in progress"),
            (PLAN_DONE, "done"),
            (TASK_TODO, "to do"),
            (TASK_DONE, "done"),
            (CODE_CONTINUATION, "a line goes on"),
            (QUOTE_BAR, "a quote"),
        ];
        marks.extend(LIST_BULLETS.iter().map(|bullet| (*bullet, "a list item")));
        marks.extend(KINDS.iter().map(|(kind, glyph)| (*glyph, *kind)));
        marks.push((KIND_FALLBACK, "a call of another kind"));
        marks.push((GROUP, "a group of calls"));
        marks.push((SESSION_IDLE, "a session that waits"));
        marks.push((SESSION_ENDED, "a session that ended"));
        // A move names the old place and the new, as a renamed file does.
        let rename = "→";

        for (glyph, meaning) in &marks {
            for (other, said) in &marks {
                let (glyph, other) = (glyph.trim(), other.trim());
                assert!(
                    glyph != other || meaning == said,
                    "{glyph} means both {meaning} and {said}"
                );
            }
        }
        assert!(
            marks
                .iter()
                .all(|(glyph, meaning)| glyph.trim() != rename || *meaning == "move"),
            "{rename} is a move: {marks:?}"
        );
    }
}
