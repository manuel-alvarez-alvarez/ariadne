//! The box the next prompt is typed into: its text, its cursor and its keys.
//!
//! The box of the inline pane (`tui/input.rs`), taken over whole: the
//! full-screen console replaces that pane, which goes with it. Only
//! [`Input::draw`] differs, since the box here is one of two zones and
//! shows the cursor only while it has the keys.

use std::cell::Cell;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame as Draw;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::Paragraph;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::theme::{DIM, INPUT_CONTINUATION, INPUT_PLACEHOLDER, INPUT_PROMPT, INPUT_RULE, USER};
use crate::wrap;

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
            for row in wrap::wrap_exact(&text, width) {
                end = start + row.chars().count();
                used = wrap::width(&row);
                rows.push(VisualRow { line, start, end });
                start = end;
            }
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

    /// Draw the box, the text in it, and `hint` dim after the text, which is
    /// not input. The cursor is drawn only where the box is `focused`.
    pub(super) fn draw(&self, frame: &mut Draw, area: Rect, hint: Option<&str>, focused: bool) {
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
        let empty = self.is_empty();
        let mut lines = rows
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
        if let (Some(hint), Some(last)) = (hint, lines.last_mut()) {
            last.push_span(Span::styled(hint.to_string(), DIM));
        }
        let text_area = Rect::new(
            area.x,
            area.y.saturating_add(1),
            area.width,
            area.height.saturating_sub(BORDER_ROWS),
        );
        let (cursor, scroll) = self.view(usize::from(text_area.height), width);
        frame.render_widget(Paragraph::new(Text::from(lines)).scroll(scroll), text_area);
        if focused {
            frame.set_cursor_position((text_area.x + cursor.0, text_area.y + cursor.1));
        }
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

    /// Whether the box holds nothing at all.
    pub(super) fn is_empty(&self) -> bool {
        self.lines.len() == 1 && self.lines[0].is_empty()
    }

    /// The first word of the box, while no white space is before the
    /// cursor: the word the cursor is in. `None` once it has left it.
    pub(super) fn leading_word(&self) -> Option<String> {
        if self.row != 0
            || self.lines[0][..self.column]
                .iter()
                .any(|c| c.is_whitespace())
        {
            return None;
        }
        Some(
            self.lines[0]
                .iter()
                .take_while(|character| !character.is_whitespace())
                .collect(),
        )
    }

    /// Write `word` over the first word of the box, with a space after it
    /// where `space` is set, and put the cursor after both.
    pub(super) fn complete(&mut self, word: &str, space: bool) {
        let line = &mut self.lines[0];
        let end = line
            .iter()
            .position(|character| character.is_whitespace())
            .unwrap_or(line.len());
        let mut written: Vec<char> = word.chars().collect();
        let spaced = space && line.get(end).is_some_and(|c| c.is_whitespace());
        if space && !spaced {
            written.push(' ');
        }
        let length = written.len() + usize::from(spaced);
        line.splice(..end, written);
        self.row = 0;
        self.column = length;
        self.vertical_column = None;
    }

    pub(super) fn joined(&self) -> String {
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
