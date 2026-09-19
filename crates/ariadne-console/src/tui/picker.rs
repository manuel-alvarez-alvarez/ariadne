//! A permission question as the picker that answers it.

use ratatui::style::Style;
use ratatui::text::{Line, Span};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::theme::{self, ASK, DIM, FRAME, PICKED, QUESTION, TOOL};
use crate::transcript::{PermissionOption, Tool, TranscriptItem};

use super::blocks::{block, command, folded_diff, folded_top, head, wrap};

/// The pending permission question as the picker being answered, in a pane
/// `height` rows tall: the question, its call's command or diff folded to
/// the room left, and its options, between two rules, so the head and the
/// options are on the screen together however long what is between them.
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
    let width = width.max(8);
    let draw = |fold| {
        permission(
            question,
            tool,
            options,
            answer.as_deref(),
            Some(picked),
            width,
            fold,
        )
    };
    let whole = draw(usize::MAX);
    if whole.len() <= height {
        return whole;
    }
    // What the fold leaves room for is what the frame does not take: the two
    // rules, the question and the head, the options, and the line that counts
    // what the fold left out.
    let (above, below) = frame(question, tool, options, Some(picked), width);
    let room = height.saturating_sub(above.len() + below.len() + 1).max(1);
    draw(room)
}

/// A permission question: the options as a picker while it is open, and as
/// the answer once it is given. The call it asks about is drawn under the
/// question — its head, and the command or the diff where it carries one,
/// folded past `fold` lines — so what is being allowed can be read before
/// it is. Once answered it is the question, the head and the option chosen.
pub(super) fn permission(
    question: &str,
    tool: &Tool,
    options: &[PermissionOption],
    answer: Option<&str>,
    picked: Option<usize>,
    width: usize,
    fold: usize,
) -> Vec<Line<'static>> {
    if let Some(answer) = answer {
        // The call's own block says how it went, so the head carries no
        // status here.
        let mut lines = asked(question, tool, Span::raw(" "), width);
        lines.extend(answered(answer, width));
        return lines;
    }
    let (mut lines, below) = frame(question, tool, options, picked, width);
    lines.extend(body(tool, width, fold));
    lines.extend(below);
    lines
}

/// The rows of a pending question that never fold: above the command or the
/// diff, the rule with its label, the question and the head; below it, the
/// options and the closing rule.
fn frame(
    question: &str,
    tool: &Tool,
    options: &[PermissionOption],
    picked: Option<usize>,
    width: usize,
) -> (Vec<Line<'static>>, Vec<Line<'static>>) {
    let lead = format!("{} ", theme::RULE);
    let fill = width.saturating_sub(lead.width() + theme::ASK_LABEL.width() + 1);
    let mut above = vec![Line::from(vec![
        Span::styled(lead, FRAME),
        Span::styled(theme::ASK_LABEL, ASK),
        Span::styled(format!(" {}", theme::RULE.repeat(fill)), FRAME),
    ])];
    above.extend(asked(
        question,
        tool,
        Span::styled(theme::CALL_PENDING, TOOL),
        width,
    ));
    let mut below: Vec<_> = options
        .iter()
        .enumerate()
        .flat_map(|(at, option)| choice(at, option, picked == Some(at), width))
        .collect();
    below.push(Line::from(Span::styled(theme::RULE.repeat(width), FRAME)));
    (above, below)
}

/// The question in bold, wrapped, and under it the head of the call.
fn asked(question: &str, tool: &Tool, mark: Span<'static>, width: usize) -> Vec<Line<'static>> {
    let mut lines: Vec<_> = fit(question, width)
        .into_iter()
        .map(|row| Line::from(Span::styled(row, QUESTION)))
        .collect();
    lines.push(head(tool, mark, None, width));
    lines
}

/// The rest of a command that has more than one line, or the diff, folded
/// past `fold` lines. The head shows a command's first line: the rest of a
/// command is what the question is about.
fn body(tool: &Tool, width: usize, fold: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
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
    lines
}

/// One option, numbered from one: the picked one marked and in the ask
/// colour, the others under two spaces with their number dimmed. A name
/// wider than the line wraps under its first row.
fn choice(at: usize, option: &PermissionOption, chosen: bool, width: usize) -> Vec<Line<'static>> {
    let (marker, number, name) = if chosen {
        (theme::OPTION_PICKED, PICKED, PICKED)
    } else {
        (theme::OPTION_IDLE, DIM, Style::new())
    };
    let number_text = format!("{}. ", at + 1);
    let hang = marker.width() + number_text.width();
    fit(&option.name, width.saturating_sub(hang))
        .into_iter()
        .enumerate()
        .map(|(row, text)| {
            if row == 0 {
                Line::from(vec![
                    Span::styled(marker, if chosen { PICKED } else { Style::new() }),
                    Span::styled(number_text.clone(), number),
                    Span::styled(text, name),
                ])
            } else {
                Line::from(vec![Span::raw(" ".repeat(hang)), Span::styled(text, name)])
            }
        })
        .collect()
}

/// The option that was chosen, wrapped under its arrow.
fn answered(answer: &str, width: usize) -> Vec<Line<'static>> {
    let hang = theme::ANSWER_MARKER.width();
    fit(answer, width.saturating_sub(hang))
        .into_iter()
        .enumerate()
        .map(|(row, text)| {
            let lead = if row == 0 {
                theme::ANSWER_MARKER.to_string()
            } else {
                " ".repeat(hang)
            };
            Line::from(vec![Span::styled(lead, ASK), Span::styled(text, ASK)])
        })
        .collect()
}

/// `text` in rows of at most `width` columns: broken at a space where it
/// can be, and between grapheme clusters where a word is wider than the row,
/// so no character is cut.
fn fit(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    wrap(text, width)
        .into_iter()
        .flat_map(|line| {
            let mut rows = vec![String::new()];
            let mut used = 0;
            for grapheme in line.graphemes(true) {
                let cells = grapheme.width();
                if used > 0 && used + cells > width {
                    rows.push(String::new());
                    used = 0;
                }
                rows.last_mut()
                    .expect("one row at least")
                    .push_str(grapheme);
                used += cells;
            }
            rows
        })
        .collect()
}

#[cfg(test)]
mod tests {

    use ratatui::backend::TestBackend;
    use serde_json::json;
    use unicode_width::UnicodeWidthStr;

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

        assert!(shown.contains("Bash\n○ $ cargo build\n"), "{shown}");
        assert!(
            row_of(&shown, "    cargo nextest run") < row_of(&shown, "❯ 1. Reject"),
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
        let shown = pane(&console, 72, 12);

        assert!(shown.contains("Edit\n○ ✎ src/lib.rs\n"), "{shown}");
        assert!(
            row_of(&shown, "-fn a() {}") < row_of(&shown, "+fn b() {}")
                && row_of(&shown, "+fn b() {}") < row_of(&shown, "❯ 1. Reject"),
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
            shown.contains("Edit\n○ ✎ src/lib.rs\n"),
            "the question is on the screen, whatever came after it: {shown}"
        );
        assert!(
            shown.contains("❯ 1. Reject") && shown.contains("2. Allow"),
            "and so are its options: {shown}"
        );
        assert!(
            shown.contains("more lines") && !shown.contains("-line 30"),
            "the diff is folded to the room the question, its options and its rules leave: {shown}"
        );
    }

    /// The rows of `console` on a pane `width` columns wide and `live` rows
    /// tall above its input box, with nothing in the scrollback.
    fn pane(console: &Console, width: u16, live: u16) -> String {
        let mut terminal =
            Terminal::new(TestBackend::new(width, live + console.pinned_rows(width))).unwrap();
        terminal.draw(|frame| console.render(frame)).unwrap();
        screen(&terminal)
    }

    fn asked_with(question: &str, options: &[&str], input: serde_json::Value) -> AgentEventDto {
        let options: Vec<_> = options
            .iter()
            .enumerate()
            .map(|(at, name)| json!({"optionId": format!("option-{at}"), "name": name}))
            .collect();
        event(
            "permission_request",
            question,
            json!({"tool_name": question, "acp": input, "options": options}),
        )
    }

    fn rule(width: usize) -> String {
        "─".repeat(width)
    }

    #[test]
    fn a_pending_question_draws_a_labelled_rule_above_and_a_rule_below() {
        let mut console = Console::new(header());
        console.apply(&asked_with(
            "Allow this edit?",
            &["Allow once", "Reject"],
            json!({"toolCallId": "edit", "kind": "edit", "rawInput": {"file_path": "src/main.rs"}}),
        ));

        let shown = pane(&console, 60, 12);

        let top = format!("─ permission {}", rule(60 - "─ permission ".width()));
        assert_eq!(row_of(&shown, &top), Some(0), "{shown}");
        assert_eq!(
            row_of(&shown, "Allow this edit?"),
            Some(1),
            "the question is under the rule: {shown}"
        );
        assert_eq!(
            shown.lines().nth(5),
            Some(rule(60).as_str()),
            "and a rule closes the options: {shown}"
        );
    }

    #[test]
    fn a_permission_question_renders_as_a_picker_the_arrows_move() {
        let mut console = Console::new(header());
        console.apply(&asked());
        let marked = |shown: &str| -> Vec<String> {
            shown
                .lines()
                .filter(|row| row.starts_with("❯ "))
                .map(str::to_string)
                .collect()
        };

        let first = pane(&console, 60, 12);
        console.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        let second = pane(&console, 60, 12);

        assert!(first.contains("Write"), "{first}");
        assert_eq!(marked(&first), ["❯ 1. Reject"], "{first}");
        assert!(first.contains("\n  2. Allow"), "{first}");
        assert_eq!(
            marked(&second),
            ["❯ 2. Allow"],
            "down moved the mark: {second}"
        );
        assert!(second.contains("\n  1. Reject"), "{second}");
    }

    #[test]
    fn a_question_with_a_long_diff_shows_the_question_both_rules_and_each_option_in_twelve_rows() {
        let mut console = Console::new(header());
        let old: String = (1..=200).map(|n| format!("line {n}\n")).collect();
        let new: String = (1..=200).map(|n| format!("row {n}\n")).collect();
        console.apply(&asked_with(
            "Allow this edit?",
            &["Allow once", "Allow always", "Reject"],
            json!({"toolCallId": "edit", "kind": "edit",
                   "rawInput": {"file_path": "src/main.rs"},
                   "content": [{"type": "diff", "path": "src/main.rs",
                                "oldText": old, "newText": new}]}),
        ));

        let shown = pane(&console, 80, 12);
        let live: Vec<_> = shown.lines().take(12).collect();

        assert!(live[0].starts_with("─ permission ─"), "{shown}");
        assert_eq!(live[1], "Allow this edit?", "{shown}");
        assert_eq!(
            &live[8..],
            [
                "❯ 1. Allow once",
                "  2. Allow always",
                "  3. Reject",
                rule(80).as_str()
            ],
            "{shown}"
        );
        assert!(
            live[2..8].iter().any(|row| row.contains("more lines")),
            "the diff folds to the room left, and says so: {shown}"
        );
    }

    #[test]
    fn an_answered_question_draws_the_chosen_option_and_no_other() {
        let mut console = Console::new(header());
        let mut terminal = terminal();
        console.apply(&asked_with(
            "Allow this edit?",
            &["Allow once", "Allow always", "Reject"],
            json!({"toolCallId": "edit", "kind": "edit", "rawInput": {"file_path": "src/main.rs"}}),
        ));
        console.apply(&event(
            "permission.replied",
            "answered",
            json!({"option_id": "option-0"}),
        ));
        console.apply(&event("agent_message", "done", json!({"text": "done"})));

        console.commit(&mut terminal).unwrap();
        let shown = screen(&terminal);

        assert!(
            shown.contains("Allow this edit?\n  ✎ src/main.rs\n→ Allow once\n"),
            "{shown}"
        );
        assert!(!shown.contains("Allow always"), "{shown}");
        assert!(!shown.contains("Reject"), "{shown}");
        assert!(!shown.contains("permission ─"), "no frame: {shown}");
        assert!(!shown.contains("❯"), "no picker: {shown}");
    }

    #[test]
    fn a_question_of_200_columns_and_a_long_option_name_wrap_in_80_without_losing_a_character() {
        let mut console = Console::new(header());
        let question: String = (1..=20).map(|n| format!("question{n:02} ")).collect();
        let question = question.trim_end();
        let unbroken = "n".repeat(90);
        let name = format!("Allow every edit under {unbroken} for now");
        console.apply(&asked_with(
            question,
            &[&name, "Reject"],
            json!({"toolCallId": "edit", "kind": "edit", "rawInput": {"file_path": "src/main.rs"}}),
        ));

        let shown = pane(&console, 80, 20);
        let bare = |text: &str| text.split_whitespace().collect::<String>();

        assert!(question.width() > 200);
        assert!(
            bare(&shown).contains(&bare(question)),
            "every word of the question is drawn, in order: {shown}"
        );
        assert!(
            bare(&shown).contains(&format!("1.{}", bare(&name))),
            "and every character of the option name: {shown}"
        );
        assert!(
            shown.lines().all(|row| row.width() <= 80),
            "and no row is wider than the pane: {shown}"
        );
    }
}
