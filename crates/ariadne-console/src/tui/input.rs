//! The box the next prompt is typed into: its text, its cursor and its keys.

use std::cell::Cell;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame as Draw;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::Paragraph;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::theme::{DIM, INPUT_CONTINUATION, INPUT_PLACEHOLDER, INPUT_PROMPT, INPUT_RULE, USER};

/// How many rows of typed text the input box grows to before it scrolls.
const INPUT_ROWS: usize = 4;
/// The top and the bottom border of the box.
const BORDER_ROWS: u16 = 2;

/// The multi-line box at the bottom.
pub(super) struct Input {
    lines: Vec<Vec<char>>,
    row: usize,
    column: usize,
    width: Cell<usize>,
    vertical_column: Option<usize>,
    history: Vec<String>,
    snapshot_history: usize,
    history_at: Option<usize>,
    draft: Option<String>,
}

#[derive(Clone, Copy)]
struct VisualRow {
    line: usize,
    start: usize,
    end: usize,
}

impl Default for Input {
    fn default() -> Self {
        Self {
            lines: vec![Vec::new()],
            row: 0,
            column: 0,
            width: Cell::new(usize::MAX),
            vertical_column: None,
            history: Vec::new(),
            snapshot_history: 0,
            history_at: None,
            draft: None,
        }
    }
}

impl Input {
    fn content_width(width: usize) -> usize {
        width.saturating_sub(INPUT_PROMPT.width()).max(1)
    }

    fn visual_rows(&self, width: usize) -> Vec<VisualRow> {
        let width = Self::content_width(width);
        let mut rows = Vec::new();
        for (line, characters) in self.lines.iter().enumerate() {
            let text = characters.iter().collect::<String>();
            let mut start = 0;
            let mut end = 0;
            let mut used = 0;
            for grapheme in text.graphemes(true) {
                let grapheme_width = grapheme.width();
                if used > 0 && used + grapheme_width > width {
                    rows.push(VisualRow { line, start, end });
                    start = end;
                    used = 0;
                }
                end += grapheme.chars().count();
                used += grapheme_width;
            }
            rows.push(VisualRow { line, start, end });
            if line == self.row && self.column == end && used == width {
                rows.push(VisualRow {
                    line,
                    start: end,
                    end,
                });
            }
        }
        rows
    }

    /// The rows the box takes, borders included: one row of text, growing
    /// with what is typed up to a few, and scrolling past them.
    pub(super) fn height(&self, width: u16) -> u16 {
        self.width.set(usize::from(width));
        let rows = self
            .visual_rows(usize::from(width))
            .len()
            .clamp(1, INPUT_ROWS);
        u16::try_from(rows).unwrap_or(1) + BORDER_ROWS
    }

    /// Draw the box, the text in it and the cursor.
    pub(super) fn draw(&self, frame: &mut Draw, area: Rect) {
        let width = usize::from(area.width);
        self.width.set(width);
        let rule = Line::styled(INPUT_RULE.repeat(width), DIM);
        frame.render_widget(
            Paragraph::new(rule.clone()),
            Rect::new(area.x, area.y, area.width, 1),
        );
        frame.render_widget(
            Paragraph::new(rule),
            Rect::new(
                area.x,
                area.y.saturating_add(area.height.saturating_sub(1)),
                area.width,
                1,
            ),
        );

        let rows = self.visual_rows(width);
        let empty = self.lines.len() == 1 && self.lines[0].is_empty();
        let lines = rows
            .iter()
            .enumerate()
            .map(|(at, row)| {
                let marker = if at == 0 {
                    Span::styled(INPUT_PROMPT, USER)
                } else {
                    Span::raw(INPUT_CONTINUATION)
                };
                let content = if empty {
                    Span::styled(INPUT_PLACEHOLDER, DIM)
                } else {
                    Span::raw(
                        self.lines[row.line][row.start..row.end]
                            .iter()
                            .collect::<String>(),
                    )
                };
                Line::from(vec![marker, content])
            })
            .collect::<Vec<_>>();
        let text_area = Rect::new(
            area.x,
            area.y.saturating_add(1),
            area.width,
            area.height.saturating_sub(BORDER_ROWS),
        );
        let (cursor, scroll) = self.view(usize::from(text_area.height), width);
        frame.render_widget(Paragraph::new(Text::from(lines)).scroll(scroll), text_area);
        frame.set_cursor_position((text_area.x + cursor.0, text_area.y + cursor.1));
    }

    fn cursor_row(&self, rows: &[VisualRow]) -> usize {
        for (at, row) in rows.iter().enumerate() {
            if row.line != self.row {
                continue;
            }
            if self.column < row.end {
                return at;
            }
            if self.column == row.end {
                if rows
                    .get(at + 1)
                    .is_some_and(|next| next.line == self.row && next.start == self.column)
                {
                    return at + 1;
                }
                return at;
            }
        }
        rows.len().saturating_sub(1)
    }

    fn row_column(&self, row: VisualRow) -> usize {
        self.lines[row.line][row.start..self.column]
            .iter()
            .collect::<String>()
            .width()
    }

    /// Where the cursor is in the wrapped box, and how far it scrolls down.
    pub(super) fn view(&self, height: usize, width: usize) -> ((u16, u16), (u16, u16)) {
        let rows = self.visual_rows(width);
        let row = self.cursor_row(&rows);
        let column = INPUT_PROMPT.width() + self.row_column(rows[row]);
        let down = (row + 1).saturating_sub(height.max(1));
        let at = |value: usize| u16::try_from(value).unwrap_or(u16::MAX);
        ((at(column), at(row - down)), (at(down), 0))
    }

    pub(super) fn newline(&mut self) {
        let tail = self.lines[self.row].split_off(self.column);
        self.lines.insert(self.row + 1, tail);
        self.row += 1;
        self.column = 0;
        self.vertical_column = None;
    }

    pub(super) fn remove_trailing_backslash(&mut self) -> bool {
        if self.column == self.lines[self.row].len() && self.lines[self.row].last() == Some(&'\\') {
            self.lines[self.row].pop();
            self.column -= 1;
            true
        } else {
            false
        }
    }

    /// Put pasted text in at the cursor, its line breaks kept: a terminal
    /// pastes them as `\r`, `\r\n` or `\n`, and each is one new line.
    pub(super) fn paste(&mut self, text: &str) {
        self.vertical_column = None;
        let mut characters = text.chars().peekable();
        while let Some(character) = characters.next() {
            match character {
                '\r' => {
                    characters.next_if_eq(&'\n');
                    self.newline();
                }
                '\n' => self.newline(),
                _ => {
                    self.lines[self.row].insert(self.column, character);
                    self.column += 1;
                }
            }
        }
    }

    /// The start of the word before the cursor: back over the space, then
    /// back over the word.
    fn word_start(&self) -> usize {
        let line = &self.lines[self.row];
        let mut at = self.column;
        while at > 0 && line[at - 1].is_whitespace() {
            at -= 1;
        }
        while at > 0 && !line[at - 1].is_whitespace() {
            at -= 1;
        }
        at
    }

    /// The end of the word after the cursor: forward over the space, then
    /// forward over the word.
    fn word_end(&self) -> usize {
        let line = &self.lines[self.row];
        let mut at = self.column;
        while at < line.len() && line[at].is_whitespace() {
            at += 1;
        }
        while at < line.len() && !line[at].is_whitespace() {
            at += 1;
        }
        at
    }

    fn previous_boundary(&self) -> usize {
        let text = self.lines[self.row].iter().collect::<String>();
        text.graphemes(true)
            .scan(0, |at, grapheme| {
                *at += grapheme.chars().count();
                Some(*at)
            })
            .take_while(|at| *at < self.column)
            .last()
            .unwrap_or(0)
    }

    fn next_boundary(&self) -> usize {
        let text = self.lines[self.row].iter().collect::<String>();
        text.graphemes(true)
            .scan(0, |at, grapheme| {
                *at += grapheme.chars().count();
                Some(*at)
            })
            .find(|at| *at > self.column)
            .unwrap_or(self.lines[self.row].len())
    }

    fn joined(&self) -> String {
        self.lines
            .iter()
            .map(|line| line.iter().collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn replace(&mut self, text: &str) {
        self.lines = text
            .split('\n')
            .map(|line| line.chars().collect::<Vec<_>>())
            .collect();
        self.row = self.lines.len().saturating_sub(1);
        self.column = self.lines[self.row].len();
        self.vertical_column = None;
    }

    pub(super) fn sync_history(&mut self, prompts: Vec<String>) {
        let local = self.history[self.snapshot_history.min(self.history.len())..].to_vec();
        let confirmed = (0..=local.len())
            .rev()
            .find(|count| prompts.ends_with(&local[..*count]))
            .unwrap_or(0);
        self.history = prompts;
        self.snapshot_history = self.history.len();
        self.history.extend_from_slice(&local[confirmed..]);
        self.history_at = None;
        self.draft = None;
    }

    fn load_history(&mut self, at: usize) {
        let text = self.history[at].clone();
        self.replace(&text);
        self.history_at = Some(at);
    }

    fn history_up(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let at = match self.history_at {
            Some(at) => at.saturating_sub(1),
            None => {
                self.draft = Some(self.joined());
                self.history.len() - 1
            }
        };
        self.load_history(at);
    }

    fn history_down(&mut self) {
        let Some(at) = self.history_at else {
            return;
        };
        if at + 1 < self.history.len() {
            self.load_history(at + 1);
        } else {
            let draft = self.draft.take().unwrap_or_default();
            self.replace(&draft);
            self.history_at = None;
        }
    }

    fn move_to_row(&mut self, row: VisualRow, column: usize) {
        let mut at = row.start;
        let mut used = 0;
        let text = self.lines[row.line][row.start..row.end]
            .iter()
            .collect::<String>();
        for grapheme in text.graphemes(true) {
            let next = used + grapheme.width();
            if next > column {
                break;
            }
            used = next;
            at += grapheme.chars().count();
        }
        self.row = row.line;
        self.column = at;
    }

    fn move_vertical(&mut self, down: bool) -> bool {
        let rows = self.visual_rows(self.width.get());
        let current = self.cursor_row(&rows);
        let Some(target) = (if down {
            rows.get(current + 1)
        } else {
            current.checked_sub(1).and_then(|at| rows.get(at))
        }) else {
            return false;
        };
        let column = self
            .vertical_column
            .unwrap_or_else(|| self.row_column(rows[current]));
        self.vertical_column = Some(column);
        self.move_to_row(*target, column);
        true
    }

    /// Everything typed so far, emptying the box. `None` when it holds only
    /// whitespace: Enter on an empty box is not a prompt.
    pub(super) fn take(&mut self) -> Option<String> {
        let text = self.joined();
        if text.trim().is_empty() {
            return None;
        }
        self.history.push(text.clone());
        self.lines = vec![Vec::new()];
        self.row = 0;
        self.column = 0;
        self.vertical_column = None;
        self.history_at = None;
        self.draft = None;
        Some(text)
    }

    /// One key in the box. The line-editing keys are the shell's: Ctrl-A and
    /// Ctrl-E to the ends of the line, Ctrl-U and Ctrl-K deleting to them,
    /// Ctrl-W deleting the word before the cursor, and Alt with an arrow —
    /// or Alt-B and Alt-F, which is what a terminal that sends the readline
    /// sequences for Alt-Left and Alt-Right gives — moving by word.
    pub(super) fn key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        match key.code {
            KeyCode::Up => {
                if !self.move_vertical(false) {
                    self.history_up();
                }
                return;
            }
            KeyCode::Down => {
                if !self.move_vertical(true) {
                    self.history_down();
                }
                return;
            }
            _ => self.vertical_column = None,
        }
        match key.code {
            KeyCode::Char('a') if ctrl => self.column = 0,
            KeyCode::Char('e') if ctrl => self.column = self.lines[self.row].len(),
            KeyCode::Char('u') if ctrl => {
                self.lines[self.row].drain(..self.column);
                self.column = 0;
            }
            KeyCode::Char('k') if ctrl => {
                self.lines[self.row].truncate(self.column);
            }
            KeyCode::Char('w') if ctrl => {
                let start = self.word_start();
                self.lines[self.row].drain(start..self.column);
                self.column = start;
            }
            KeyCode::Left | KeyCode::Char('b') if alt => self.column = self.word_start(),
            KeyCode::Right | KeyCode::Char('f') if alt => self.column = self.word_end(),
            KeyCode::Char(_) if ctrl || alt => {}
            KeyCode::Char(character) => {
                self.lines[self.row].insert(self.column, character);
                self.column += 1;
            }
            KeyCode::Backspace if self.column > 0 => {
                let start = self.previous_boundary();
                self.lines[self.row].drain(start..self.column);
                self.column = start;
            }
            KeyCode::Backspace if self.row > 0 => {
                let tail = self.lines.remove(self.row);
                self.row -= 1;
                self.column = self.lines[self.row].len();
                self.lines[self.row].extend(tail);
            }
            KeyCode::Delete if self.column < self.lines[self.row].len() => {
                let end = self.next_boundary();
                self.lines[self.row].drain(self.column..end);
            }
            KeyCode::Left => self.column = self.previous_boundary(),
            KeyCode::Right => self.column = self.next_boundary(),
            KeyCode::Home => self.column = 0,
            KeyCode::End => self.column = self.lines[self.row].len(),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {

    use crate::tui::testing::*;
    use crate::tui::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::style::Modifier;

    fn alt(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::ALT)
    }

    fn left(console: &mut Console, times: usize) {
        for _ in 0..times {
            console.key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        }
    }

    fn terminal_of(width: u16, height: u16) -> Terminal<TestBackend> {
        Terminal::new(TestBackend::new(width, height)).unwrap()
    }

    #[test]
    fn the_input_box_draws_two_rules_and_no_side_or_corner_border() {
        let console = Console::new(header());
        let mut terminal = terminal_of(20, 8);

        terminal.draw(|frame| console.render(frame)).unwrap();

        let shown = screen(&terminal);
        let rule = "────────────────────";
        assert_eq!(shown.matches(rule).count(), 2);
        assert!(!shown.chars().any(|character| "│┌┐└┘".contains(character)));
        let rule_row = shown
            .lines()
            .position(|row| row == rule)
            .expect("top input rule");
        assert!(
            terminal.backend().buffer()[(0, rule_row as u16)]
                .modifier
                .contains(Modifier::DIM)
        );
    }

    #[test]
    fn the_first_input_row_has_a_prompt_and_each_wrapped_row_has_two_spaces() {
        let mut console = Console::new(header());
        let mut terminal = terminal_of(12, 8);
        type_into(&mut console, "abcdefghijklmnop");

        terminal.draw(|frame| console.render(frame)).unwrap();

        let shown = screen(&terminal);
        assert!(shown.lines().any(|line| line == "❯ abcdefghij"));
        assert!(shown.lines().any(|line| line == "  klmnop"));
    }

    #[test]
    fn two_hundred_columns_wrap_to_three_rows_without_hiding_the_end() {
        let mut console = Console::new(header());
        let mut terminal = terminal_of(80, 10);

        type_into(&mut console, &"x".repeat(199));
        type_into(&mut console, "!");
        terminal.draw(|frame| console.render(frame)).unwrap();

        let shown = screen(&terminal);
        let input = shown
            .lines()
            .filter(|line| line.starts_with("❯ ") || line.starts_with("  x"))
            .collect::<Vec<_>>();
        assert_eq!(input.len(), 3, "{shown}");
        assert_eq!(
            input
                .iter()
                .map(|line| line.matches('x').count())
                .sum::<usize>(),
            199
        );
        assert!(input[2].ends_with('!'));
        assert_eq!(terminal.get_cursor_position().unwrap().x, 46);
        assert_eq!(
            console.pinned_rows(80),
            7,
            "status, two rules, three text rows and footer"
        );
    }

    #[test]
    fn wide_characters_wrap_at_display_width_and_put_the_cursor_after_their_cells() {
        let mut console = Console::new(header());
        let mut terminal = terminal_of(20, 8);
        type_into(&mut console, "日日日日日日日日日日");

        terminal.draw(|frame| console.render(frame)).unwrap();

        let shown = screen(&terminal);
        assert!(shown.lines().any(|line| line == "❯ 日日日日日日日日日"));
        assert!(shown.lines().any(|line| line == "  日"));
        assert_eq!(terminal.get_cursor_position().unwrap().x, 4);
    }

    #[test]
    fn the_cursor_uses_the_continuation_cell_after_a_full_wrapped_row() {
        let mut console = Console::new(header());
        let mut terminal = terminal_of(20, 8);
        type_into(&mut console, "日日日日日日日日日");

        terminal.draw(|frame| console.render(frame)).unwrap();

        assert_eq!(console.pinned_rows(20), 6);
        assert_eq!(terminal.get_cursor_position().unwrap().x, 2);
    }

    #[test]
    fn wrapping_keeps_an_emoji_grapheme_whole() {
        let mut console = Console::new(header());
        let mut terminal = terminal_of(6, 8);
        type_into(&mut console, "ab❤️c");

        terminal.draw(|frame| console.render(frame)).unwrap();

        let shown = screen(&terminal);
        assert!(shown.lines().any(|line| line == "❯ ab❤️"), "{shown}");
        assert!(shown.lines().any(|line| line == "  c"), "{shown}");
    }

    /// A red heart with the emoji presentation selector: two characters, one
    /// of them a column wide on its own, drawn as one two-column emoji.
    #[test]
    fn the_cursor_sits_after_an_emoji_sequence_as_it_is_drawn() {
        let mut console = Console::new(header());
        let mut terminal = terminal();
        type_into(&mut console, "\u{2764}\u{fe0f}");

        terminal.draw(|frame| console.render(frame)).unwrap();

        assert_eq!(terminal.get_cursor_position().unwrap().x, 4);
    }

    #[test]
    fn every_newline_key_adds_a_line_and_sends_only_on_the_next_enter() {
        let mut console = Console::new(header());

        console.key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        let shift = console.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT));
        console.key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
        let alt = console.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT));
        console.key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
        let ctrl_j = console.key(ctrl('j'));
        type_into(&mut console, "d\\");
        let backslash = enter(&mut console);
        type_into(&mut console, "e");
        let send = console.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(
            (shift, alt, ctrl_j, backslash),
            (Action::None, Action::None, Action::None, Action::None)
        );
        assert_eq!(send, Action::Send("a\nb\nc\nd\ne".into()));
    }

    #[test]
    fn alt_j_and_shift_j_are_not_newline_keys() {
        let mut console = Console::new(header());

        let alt = console.key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::ALT));
        let shift = console.key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::SHIFT));
        type_into(&mut console, "x");

        assert_eq!((alt, shift), (Action::None, Action::None));
        assert_eq!(enter(&mut console), Action::Send("jx".into()));
    }

    #[test]
    fn ctrl_j_encoded_as_a_line_feed_adds_a_line() {
        let mut console = Console::new(header());
        type_into(&mut console, "a");

        let newline = console.key(KeyEvent::new(KeyCode::Char('\n'), KeyModifiers::CONTROL));
        type_into(&mut console, "b");

        assert_eq!(newline, Action::None);
        assert_eq!(enter(&mut console), Action::Send("a\nb".into()));
    }

    #[test]
    fn permission_enter_keeps_a_trailing_backslash_for_the_input() {
        let mut console = Console::new(header());
        console.apply(&asked());
        type_into(&mut console, "draft\\");

        assert_eq!(enter(&mut console), Action::Send("no".into()));
        console.apply(&event(
            "permission.replied",
            "answered",
            serde_json::json!({"option_id": "no"}),
        ));
        assert_eq!(enter(&mut console), Action::None);
        type_into(&mut console, "continued");

        assert_eq!(enter(&mut console), Action::Send("draft\ncontinued".into()));
    }

    #[test]
    fn a_backslash_before_the_line_end_stays_in_the_prompt_enter_sends() {
        let mut console = Console::new(header());
        type_into(&mut console, "a\\b");

        assert_eq!(enter(&mut console), Action::Send("a\\b".into()));
    }

    #[test]
    fn up_recalls_the_last_prompt_and_down_restores_the_draft() {
        let mut console = Console::new(header());
        type_into(&mut console, "sent");
        assert_eq!(enter(&mut console), Action::Send("sent".into()));
        type_into(&mut console, "draft");

        console.key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(enter(&mut console), Action::Send("sent".into()));

        type_into(&mut console, "draft");
        console.key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        console.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(enter(&mut console), Action::Send("draft".into()));
    }

    #[test]
    fn up_and_down_move_through_a_wrapped_draft_before_history() {
        let mut console = Console::new(header());
        let mut terminal = terminal_of(20, 8);
        type_into(&mut console, "sent");
        assert_eq!(enter(&mut console), Action::Send("sent".into()));
        type_into(&mut console, "abcdefghijklmnopqrst");
        terminal.draw(|frame| console.render(frame)).unwrap();

        console.key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        type_into(&mut console, "X");
        console.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        type_into(&mut console, "Y");

        assert_eq!(
            enter(&mut console),
            Action::Send("abXcdefghijklmnopqrstY".into())
        );
    }

    #[test]
    fn a_daemon_prompt_never_enters_history() {
        let mut console = Console::new(header());
        console.snapshot(&[
            event(
                "user_prompt_submit",
                "typed",
                serde_json::json!({"text": "typed", "source": "console"}),
            ),
            event(
                "user_prompt_submit",
                "daemon",
                serde_json::json!({"text": "daemon", "source": "daemon"}),
            ),
        ]);

        console.key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));

        assert_eq!(enter(&mut console), Action::Send("typed".into()));
    }

    #[test]
    fn an_empty_input_shows_a_dim_placeholder_that_is_never_sent() {
        let mut console = Console::new(header());
        let mut terminal = terminal_of(40, 8);
        terminal.draw(|frame| console.render(frame)).unwrap();

        let placeholder = "Tell the agent what to do";
        let shown = screen(&terminal);
        let position = terminal
            .backend()
            .buffer()
            .area
            .positions()
            .find(|position| terminal.backend().buffer()[*position].symbol() == "T")
            .expect("placeholder start");
        assert!(shown.contains(placeholder));
        assert!(
            terminal.backend().buffer()[position]
                .modifier
                .contains(Modifier::DIM)
        );
        assert_eq!(enter(&mut console), Action::None);

        type_into(&mut console, "x");
        terminal.draw(|frame| console.render(frame)).unwrap();
        assert!(!screen(&terminal).contains(placeholder));
        assert_eq!(enter(&mut console), Action::Send("x".into()));
    }

    #[test]
    fn more_than_four_visual_rows_scroll_to_keep_the_cursor_visible() {
        let mut console = Console::new(header());
        let mut terminal = terminal_of(20, 9);
        type_into(&mut console, &format!("{}!", "x".repeat(99)));

        terminal.draw(|frame| console.render(frame)).unwrap();

        assert_eq!(
            console.pinned_rows(20),
            8,
            "status, two rules, four visible rows and footer"
        );
        let shown = screen(&terminal);
        assert!(shown.lines().any(|line| line.ends_with('!')));
        let bottom_rule = shown
            .lines()
            .enumerate()
            .filter_map(|(row, line)| (line == "────────────────────").then_some(row))
            .last()
            .expect("bottom input rule") as u16;
        assert_eq!(terminal.get_cursor_position().unwrap().y, bottom_rule - 1);
    }

    #[test]
    fn a_paste_goes_in_at_the_cursor_and_a_carriage_return_is_a_line_break() {
        let mut console = Console::new(header());
        type_into(&mut console, "ad");
        left(&mut console, 1);

        console.paste("b\r\nc\r");

        assert_eq!(enter(&mut console), Action::Send("ab\nc\nd".into()));
    }

    #[test]
    fn ctrl_a_moves_to_the_line_start() {
        let mut console = Console::new(header());
        type_into(&mut console, "world");

        console.key(ctrl('a'));
        type_into(&mut console, "hello ");

        assert_eq!(enter(&mut console), Action::Send("hello world".into()));
    }

    #[test]
    fn ctrl_e_moves_to_the_line_end() {
        let mut console = Console::new(header());
        type_into(&mut console, "hello");
        left(&mut console, 5);

        console.key(ctrl('e'));
        type_into(&mut console, "!");

        assert_eq!(enter(&mut console), Action::Send("hello!".into()));
    }

    #[test]
    fn ctrl_u_deletes_to_the_line_start() {
        let mut console = Console::new(header());
        type_into(&mut console, "drop this keep");
        left(&mut console, 4);

        console.key(ctrl('u'));

        assert_eq!(enter(&mut console), Action::Send("keep".into()));
    }

    #[test]
    fn ctrl_k_deletes_to_the_line_end() {
        let mut console = Console::new(header());
        type_into(&mut console, "keep drop this");
        left(&mut console, 10);

        console.key(ctrl('k'));

        assert_eq!(enter(&mut console), Action::Send("keep".into()));
    }

    #[test]
    fn ctrl_w_deletes_the_word_before_the_cursor() {
        let mut console = Console::new(header());
        type_into(&mut console, "keep the last  ");

        console.key(ctrl('w'));
        type_into(&mut console, "one");

        assert_eq!(enter(&mut console), Action::Send("keep the one".into()));
    }

    #[test]
    fn alt_left_and_alt_right_move_by_word() {
        let mut console = Console::new(header());
        type_into(&mut console, "one two");

        console.key(alt(KeyCode::Left));
        console.key(alt(KeyCode::Left));
        type_into(&mut console, "zero ");
        console.key(alt(KeyCode::Right));
        type_into(&mut console, "!");

        assert_eq!(enter(&mut console), Action::Send("zero one! two".into()));
    }
}
