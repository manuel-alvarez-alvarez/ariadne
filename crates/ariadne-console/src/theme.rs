//! How the pane looks: the colour of every kind of line, and the glyph and
//! marker that says what a line is before a word of it is read.
//!
//! Every style and every glyph of the pane is named here, so that one place
//! holds the look and a change of it touches no drawing code.

use ratatui::style::{Color, Modifier, Style};

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
/// The welcome banner that identifies a console as it opens.
pub const BANNER: Style = Style::new().fg(Color::Blue);

/// The frames of the spinner, one per tick while a turn runs.
pub const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// What stands between two parts of the status row or two hints of the
/// footer, and between the session's status and what the turn is doing.
pub const SEPARATOR: &str = " · ";
pub const GAP: &str = "   ";
/// The tokens the session has read and written, on the right of the footer.
pub const TOKENS_IN: &str = "↑ ";
pub const TOKENS_OUT: &str = "↓ ";

/// Where a block starts: what was typed, what the agent said, what it
/// thought, what the daemon sent, and what went wrong. A typed prompt has
/// the input box's glyph, and a bar down the left edge of each of its rows.
pub const USER_MARKER: &str = "❯ ";
pub const USER_BAR: &str = "▌";
/// The tag of a typed prompt the daemon has not taken yet.
pub const QUEUED: &str = "queued";
pub const AGENT_MARKER: &str = "● ";
pub const THOUGHT_MARKER: &str = "· ";
pub const DAEMON_MARKER: &str = "» daemon";
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
pub const KIND_OTHER: &str = "◇";
/// What ties a call's output to its head: the first row of the output
/// starts with it, two columns in.
pub const OUTPUT_MARKER: &str = "  ⎿ ";

/// A plan entry: pending, in progress, and completed.
pub const PLAN_TODO: &str = "  ☐ ";
pub const PLAN_DOING: &str = "  ◐ ";
pub const PLAN_DONE: &str = "  ☑ ";

/// The picker of a permission question: the label in the rule above, the option
/// it is on and the ones it is not on, and the option that was chosen once the
/// question is answered.
pub const ASK_LABEL: &str = "permission";
pub const OPTION_PICKED: &str = "❯ ";
pub const OPTION_IDLE: &str = "  ";
pub const ANSWER_MARKER: &str = "↳ ";

/// Markdown marks, kept with the pane's other visible vocabulary.
pub const RULE: &str = "─";
pub const QUOTE_BAR: &str = "│ ";
pub const CODE_CONTINUATION: &str = "↪";
pub const LIST_BULLETS: [&str; 3] = ["• ", "◦ ", "▪ "];
pub const TASK_DONE: &str = "☑ ";
pub const TASK_TODO: &str = "☐ ";

/// The box around a welcome banner where the pane is wide enough for one.
pub const BANNER_TOP_LEFT: &str = "╭";
pub const BANNER_TOP_RIGHT: &str = "╮";
pub const BANNER_BOTTOM_LEFT: &str = "╰";
pub const BANNER_BOTTOM_RIGHT: &str = "╯";
pub const BANNER_HORIZONTAL: &str = "─";
pub const BANNER_VERTICAL: &str = "│";

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
            (AGENT_MARKER, "the agent speaks"),
            (THOUGHT_MARKER, "the agent thinks"),
            (DAEMON_MARKER, "the daemon speaks"),
            (ERROR_MARKER, "failed"),
            (CALL_FAILED, "failed"),
            (CALL_DONE, "done"),
            (CALL_RUNNING, "in progress"),
            (CALL_PENDING, "pending"),
            (KIND_OTHER, "a call of another kind"),
            (OUTPUT_MARKER, "output"),
            (PLAN_TODO, "to do"),
            (PLAN_DOING, "in progress"),
            (PLAN_DONE, "done"),
            (TASK_TODO, "to do"),
            (TASK_DONE, "done"),
            (ANSWER_MARKER, "the answer"),
            (CODE_CONTINUATION, "a line goes on"),
            (QUOTE_BAR, "a quote"),
        ];
        marks.extend(LIST_BULLETS.iter().map(|bullet| (*bullet, "a list item")));
        marks.extend(KINDS.iter().map(|(kind, glyph)| (*glyph, *kind)));
        // The footer's tokens are part of the pane too.
        marks.push((TOKENS_IN, "tokens read"));
        marks.push((TOKENS_OUT, "tokens written"));
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
