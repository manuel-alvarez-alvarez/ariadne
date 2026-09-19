//! A permission question as the picker that answers it.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::theme::{self, ASK, DIM};
use crate::transcript::{PermissionOption, Tool, TranscriptItem};

use super::blocks::{block, command, folded_diff, folded_top, head, wrap};

/// The pending permission question as the picker being answered, in a pane
/// `height` rows tall: the question, its call's command or diff folded to
/// the room left, and its options, so the head and the options are on the
/// screen together however long what is between them.
pub(super) fn picker(
    item: &TranscriptItem,
    picked: usize,
    width: usize,
    height: usize,
) -> Vec<Line<'static>> {
    let TranscriptItem::PermissionQuestion {
        question,
        tool,
        options,
        answer,
        ..
    } = item
    else {
        return block(item, width, None);
    };
    let draw = |fold| {
        permission(
            question,
            tool,
            options,
            answer.as_deref(),
            Some(picked),
            width.max(8),
            fold,
        )
    };
    let whole = draw(usize::MAX);
    if whole.len() <= height {
        return whole;
    }
    // The question and the head, the options, and the line that counts what
    // the fold left out.
    let room = height.saturating_sub(2 + options.len() + 1).max(1);
    draw(room)
}

/// A permission question: the options as a picker while it is open, and as
/// the answer once it is given. The call it asks about is drawn under the
/// question — its head, and the command or the diff where it carries one,
/// folded past `fold` lines — so what is being allowed can be read before
/// it is.
pub(super) fn permission(
    question: &str,
    tool: &Tool,
    options: &[PermissionOption],
    answer: Option<&str>,
    picked: Option<usize>,
    width: usize,
    fold: usize,
) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(vec![
        Span::styled(theme::ASK_MARKER, ASK),
        Span::styled(question.to_string(), ASK.add_modifier(Modifier::BOLD)),
    ])];
    lines.push(head(tool, Span::styled(" ", ASK), None, width));
    if let Some(answer) = answer {
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(format!("answered: {answer}"), DIM),
        ]));
        return lines;
    }
    // The head shows a command's first line: the rest of a command that has
    // more is what the question is about.
    if tool.kind.as_deref() == Some("execute")
        && let Some(command) = command(&tool.input)
        && command.lines().count() > 1
    {
        lines.extend(folded_top(
            wrap(command.trim_end(), width.saturating_sub(4))
                .into_iter()
                .map(|line| Line::from(vec![Span::raw("    "), Span::styled(line, DIM)]))
                .collect(),
            fold,
        ));
    }
    if let Some(diff) = &tool.diff {
        lines.extend(folded_diff(diff, width, fold));
    }
    for (at, option) in options.iter().enumerate() {
        let chosen = picked == Some(at);
        lines.push(Line::from(vec![
            Span::styled(if chosen { theme::OPTION_PICKED } else { "    " }, ASK),
            Span::styled(
                format!("{}. {}", at + 1, option.name),
                if chosen {
                    ASK.add_modifier(Modifier::REVERSED)
                } else {
                    Style::new()
                },
            ),
        ]));
    }
    lines
}

#[cfg(test)]
mod tests {

    use serde_json::json;

    use crate::tui::testing::*;
    use crate::tui::*;

    #[test]
    fn a_permission_question_draws_the_call_above_its_options() {
        let mut console = Console::new(header());
        let mut terminal = terminal();
        console.apply(&event(
            "permission_request",
            "Permission requested for Bash",
            json!({"tool_name": "Bash",
                   "acp": {"toolCallId": "run", "kind": "execute",
                           "rawInput": {"command": "cargo build\ncargo nextest run"}},
                   "options": [{"optionId": "no", "name": "Reject"},
                               {"optionId": "yes", "name": "Allow"}]}),
        ));
        terminal.draw(|frame| console.render(frame)).unwrap();
        let shown = screen(&terminal);

        assert!(shown.contains("? Bash\n  $ cargo build\n"), "{shown}");
        assert!(
            row_of(&shown, "    cargo nextest run") < row_of(&shown, "1. Reject"),
            "the whole command is above the options: {shown}"
        );

        let mut console = Console::new(header());
        console.apply(&event(
            "permission_request",
            "Permission requested for Edit",
            json!({"tool_name": "Edit",
                   "acp": {"toolCallId": "edit", "kind": "edit",
                           "rawInput": {"file_path": "src/lib.rs"},
                           "content": [{"type": "diff", "path": "src/lib.rs",
                                        "oldText": "fn a() {}\n", "newText": "fn b() {}\n"}]},
                   "options": [{"optionId": "no", "name": "Reject"},
                               {"optionId": "yes", "name": "Allow"}]}),
        ));
        terminal.draw(|frame| console.render(frame)).unwrap();
        let shown = screen(&terminal);

        assert!(shown.contains("? Edit\n  ✎ src/lib.rs\n"), "{shown}");
        assert!(
            row_of(&shown, "-fn a() {}") < row_of(&shown, "+fn b() {}")
                && row_of(&shown, "+fn b() {}") < row_of(&shown, "1. Reject"),
            "the diff is above the options: {shown}"
        );
    }

    /// A snapshot taken mid-turn ends on the text so far (008), which comes
    /// after the question in it; and a question's diff can be longer than
    /// the pane. The picker is drawn last, with its diff folded to the room
    /// its question and options leave, so the keys act on what is on the
    /// screen.
    #[test]
    fn a_pending_picker_is_on_the_screen_whatever_came_after_it_and_however_long_its_diff() {
        let mut console = Console::new(header());
        let mut terminal = terminal();
        let old: String = (1..=30).map(|n| format!("line {n}\n")).collect();
        let new: String = (1..=30).map(|n| format!("row {n}\n")).collect();
        let so_far: String = (1..=12).map(|n| format!("paragraph {n}\n\n")).collect();
        console.snapshot(&[
            event(
                "permission_request",
                "Permission requested for Edit",
                json!({"tool_name": "Edit",
                       "acp": {"toolCallId": "edit", "kind": "edit",
                               "rawInput": {"file_path": "src/lib.rs"},
                               "content": [{"type": "diff", "path": "src/lib.rs",
                                            "oldText": old, "newText": new}]},
                       "options": [{"optionId": "no", "name": "Reject"},
                                   {"optionId": "yes", "name": "Allow"}]}),
            ),
            event("agent_message_chunk", "so far", json!({"text": so_far})),
        ]);

        console.commit(&mut terminal).unwrap();
        terminal.draw(|frame| console.render(frame)).unwrap();
        // Nothing is committed while the question waits, so the pane is the
        // whole of what is drawn.
        let shown = screen(&terminal);

        assert!(
            shown.contains("? Edit\n  ✎ src/lib.rs\n"),
            "the question is on the screen, whatever came after it: {shown}"
        );
        assert!(
            shown.contains("› 1. Reject") && shown.contains("2. Allow"),
            "and so are its options: {shown}"
        );
        assert!(
            shown.contains("-line 1\n") && shown.contains("more lines"),
            "the diff is folded to the room the question and its options leave: {shown}"
        );
    }

    #[test]
    fn a_permission_question_renders_as_a_picker_the_arrows_move() {
        let mut console = Console::new(header());
        let mut terminal = terminal();
        console.apply(&asked());

        terminal.draw(|frame| console.render(frame)).unwrap();
        let first = screen(&terminal);
        console.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        terminal.draw(|frame| console.render(frame)).unwrap();
        let second = screen(&terminal);

        assert!(first.contains("? Write"), "{first}");
        assert!(
            first.contains("› 1. Reject"),
            "the first option is on: {first}"
        );
        assert!(
            first.contains("2. Allow") && !first.contains("› 2."),
            "{first}"
        );
        assert!(
            second.contains("› 2. Allow"),
            "down moved the pick: {second}"
        );
        assert!(
            !second.contains("› 1."),
            "and moved it off the first: {second}"
        );
    }
}
