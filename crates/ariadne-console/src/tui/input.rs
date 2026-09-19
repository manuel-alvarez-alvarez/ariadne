//! The box the next prompt is typed into: its text, its cursor and its keys.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame as Draw;
use ratatui::layout::Rect;
use ratatui::text::{Line, Text};
use ratatui::widgets::{Block, Paragraph};
use unicode_width::UnicodeWidthStr;

use crate::theme::DIM;

/// How many rows of typed text the input box grows to before it scrolls.
const INPUT_ROWS: usize = 4;
/// The top and the bottom border of the box.
const BORDER_ROWS: u16 = 2;

/// The multi-line box at the bottom.
pub(super) struct Input {
    lines: Vec<Vec<char>>,
    row: usize,
    column: usize,
}

impl Default for Input {
    fn default() -> Self {
        Self {
            lines: vec![Vec::new()],
            row: 0,
            column: 0,
        }
    }
}

impl Input {
    fn rows(&self) -> usize {
        self.lines.len()
    }

    /// The rows the box takes, borders included: one row of text, growing
    /// with what is typed up to a few, and scrolling past them.
    pub(super) fn height(&self, _width: u16) -> u16 {
        let rows = self.rows().clamp(1, INPUT_ROWS);
        u16::try_from(rows).unwrap_or(1) + BORDER_ROWS
    }

    /// Draw the box, the text in it and the cursor.
    pub(super) fn draw(&self, frame: &mut Draw, area: Rect) {
        let block = Block::bordered().border_style(DIM);
        let inner = block.inner(area);
        frame.render_widget(&block, area);
        let (cursor, scroll) = self.view(usize::from(inner.height), usize::from(inner.width));
        frame.render_widget(Paragraph::new(self.text()).scroll(scroll), inner);
        frame.set_cursor_position((inner.x + cursor.0, inner.y + cursor.1));
    }

    pub(super) fn text(&self) -> Text<'static> {
        Text::from(
            self.lines
                .iter()
                .map(|line| Line::from(line.iter().collect::<String>()))
                .collect::<Vec<_>>(),
        )
    }

    /// Where the cursor is in the box, and how far the box has scrolled — down
    /// and across — to keep it there.
    ///
    /// A line longer than the box scrolls sideways under the cursor rather
    /// than running off the end of it, which is how a long prompt stays
    /// readable while it is being typed. The cursor sits at the column the
    /// characters before it draw up to: the width of the string they make,
    /// which is more than their count where one is wide, and more than the
    /// sum of their own widths where a selector or a joiner makes an emoji
    /// of them — the same measure the drawing takes.
    pub(super) fn view(&self, height: usize, width: usize) -> ((u16, u16), (u16, u16)) {
        let column = self.lines[self.row][..self.column]
            .iter()
            .collect::<String>()
            .width();
        let down = (self.row + 1).saturating_sub(height.max(1));
        let across = (column + 1).saturating_sub(width.max(1));
        let at = |value: usize| u16::try_from(value).unwrap_or(u16::MAX);
        (
            (at(column - across), at(self.row - down)),
            (at(down), at(across)),
        )
    }

    pub(super) fn newline(&mut self) {
        let tail = self.lines[self.row].split_off(self.column);
        self.lines.insert(self.row + 1, tail);
        self.row += 1;
        self.column = 0;
    }

    /// Put pasted text in at the cursor, its line breaks kept: a terminal
    /// pastes them as `\r`, `\r\n` or `\n`, and each is one new line.
    pub(super) fn paste(&mut self, text: &str) {
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

    /// Everything typed so far, emptying the box. `None` when it holds only
    /// whitespace: Enter on an empty box is not a prompt.
    pub(super) fn take(&mut self) -> Option<String> {
        let text = self
            .lines
            .iter()
            .map(|line| line.iter().collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        *self = Self::default();
        (!text.trim().is_empty()).then_some(text)
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
                self.column -= 1;
                self.lines[self.row].remove(self.column);
            }
            KeyCode::Backspace if self.row > 0 => {
                let tail = self.lines.remove(self.row);
                self.row -= 1;
                self.column = self.lines[self.row].len();
                self.lines[self.row].extend(tail);
            }
            KeyCode::Delete if self.column < self.lines[self.row].len() => {
                self.lines[self.row].remove(self.column);
            }
            KeyCode::Left => self.column = self.column.saturating_sub(1),
            KeyCode::Right => self.column = (self.column + 1).min(self.lines[self.row].len()),
            KeyCode::Home => self.column = 0,
            KeyCode::End => self.column = self.lines[self.row].len(),
            KeyCode::Up if self.row > 0 => {
                self.row -= 1;
                self.column = self.column.min(self.lines[self.row].len());
            }
            KeyCode::Down if self.row + 1 < self.lines.len() => {
                self.row += 1;
                self.column = self.column.min(self.lines[self.row].len());
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {

    use crate::tui::testing::*;
    use crate::tui::*;

    fn alt(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::ALT)
    }

    fn left(console: &mut Console, times: usize) {
        for _ in 0..times {
            console.key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        }
    }

    #[test]
    fn a_line_longer_than_the_input_box_scrolls_under_the_cursor() {
        let mut console = Console::new(header());
        let mut terminal = terminal();

        for character in "x".repeat(120).chars() {
            console.key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        console.key(KeyEvent::new(KeyCode::Char('!'), KeyModifiers::NONE));
        terminal.draw(|frame| console.render(frame)).unwrap();

        assert!(
            screen(&terminal).contains('!'),
            "the end being typed is the end shown: {}",
            screen(&terminal)
        );
    }

    #[test]
    fn the_cursor_sits_after_the_columns_a_wide_character_draws_on() {
        let mut console = Console::new(header());
        let mut terminal = terminal();
        type_into(&mut console, "日本");

        terminal.draw(|frame| console.render(frame)).unwrap();

        // The box's left border is column 0, so its text starts at 1.
        assert_eq!(
            terminal.get_cursor_position().unwrap().x,
            5,
            "two wide characters are four columns"
        );
    }

    /// A red heart with the emoji presentation selector: two characters, one
    /// of them a column wide on its own, drawn as one two-column emoji.
    #[test]
    fn the_cursor_sits_after_an_emoji_sequence_as_it_is_drawn() {
        let mut console = Console::new(header());
        let mut terminal = terminal();
        type_into(&mut console, "\u{2764}\u{fe0f}");

        terminal.draw(|frame| console.render(frame)).unwrap();

        assert_eq!(
            terminal.get_cursor_position().unwrap().x,
            3,
            "the sequence is two columns, past the border at column 0"
        );
    }

    #[test]
    fn shift_enter_and_alt_enter_add_a_line_instead_of_sending() {
        let mut console = Console::new(header());

        console.key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        let shift = console.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT));
        console.key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
        let alt = console.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT));
        console.key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
        let send = console.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!((shift, alt), (Action::None, Action::None));
        assert_eq!(send, Action::Send("a\nb\nc".into()));
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
