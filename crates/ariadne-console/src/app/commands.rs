//! The slash commands the agent lists, as the suggestion list over the box.
//!
//! The list is the one of the inline pane (`tui/commands.rs`): while the box
//! starts with `/` and the cursor is still in its first word, the list shows
//! the commands whose name holds the rest of that word, and its keys pick
//! one. Here the commands are the session's, as the trajectory holds them.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame as Draw;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use ariadne_api::trajectory::CommandDto;

use crate::theme::{DIM, OPTION_IDLE, OPTION_PICKED, PICKED};

use super::{Action, App};

/// The most rows the suggestion list takes.
const ROWS: usize = 8;

/// The one row of a list that no command matches.
const NO_MATCH: &str = "no matching command";

/// The commands whose name holds `query`, without regard to case: first the
/// names that start with it, then the names that hold it elsewhere, each
/// group in the agent's order.
fn matching<'a>(commands: &'a [CommandDto], query: &str) -> Vec<&'a CommandDto> {
    let query = query.to_lowercase();
    let (mut first, rest): (Vec<_>, Vec<_>) = commands
        .iter()
        .filter(|command| !command.name.is_empty())
        .filter(|command| command.name.to_lowercase().contains(&query))
        .partition(|command| command.name.to_lowercase().starts_with(&query));
    first.extend(rest);
    first
}

impl App {
    fn commands(&self) -> &[CommandDto] {
        self.view
            .as_ref()
            .map_or(&[], |view| view.session().commands.as_slice())
    }

    /// The commands the suggestion list shows, while it is open: the agent
    /// lists some, the box starts with `/`, no white space is before the
    /// cursor, no permission waits, and no Escape closed it since the box
    /// was last empty.
    pub(super) fn suggestions(&self) -> Option<Vec<&CommandDto>> {
        if self.commands().is_empty() || self.dismissed || self.permission().is_some() {
            return None;
        }
        let word = self.input.leading_word()?;
        let query = word.strip_prefix('/')?;
        Some(matching(self.commands(), query))
    }

    fn pick(&self, count: usize) -> usize {
        self.suggested.min(count.saturating_sub(1))
    }

    /// How many rows the suggestion list takes: a row per match, or the one
    /// row that says none does, eight at most, and no more than `room`.
    pub(super) fn suggestion_rows(&self, room: u16) -> u16 {
        let Some(matches) = self.suggestions() else {
            return 0;
        };
        u16::try_from(matches.len().clamp(1, ROWS))
            .unwrap_or(1)
            .min(room)
    }

    /// Draw the suggestion list: one row per command, `/name` and the
    /// description dim, the pick marked, scrolled to keep the pick on the
    /// screen.
    pub(super) fn draw_suggestions(&self, frame: &mut Draw, area: Rect) {
        let Some(matches) = self.suggestions() else {
            return;
        };
        if area.height == 0 {
            return;
        }
        if matches.is_empty() {
            let row = Line::from(vec![Span::raw(OPTION_IDLE), Span::styled(NO_MATCH, DIM)]);
            frame.render_widget(Paragraph::new(row), area);
            return;
        }
        let rows = usize::from(area.height);
        let picked = self.pick(matches.len());
        let mut top = self.scrolled.get();
        if picked < top {
            top = picked;
        } else if picked >= top + rows {
            top = picked + 1 - rows;
        }
        top = top.min(matches.len().saturating_sub(rows));
        self.scrolled.set(top);
        let named = matches
            .iter()
            .map(|command| command.name.width() + 1)
            .max()
            .unwrap_or(0);
        let lines: Vec<Line<'static>> = matches
            .iter()
            .enumerate()
            .skip(top)
            .take(rows)
            .map(|(at, command)| row(command, at == picked, named))
            .collect();
        frame.render_widget(Paragraph::new(Text::from(lines)), area);
    }

    /// The hint of the command the box names, while its argument is empty.
    pub(super) fn hint(&self) -> Option<&str> {
        let text = self.input.joined();
        let (name, argument) = text.strip_prefix('/')?.split_once(char::is_whitespace)?;
        if !argument.trim().is_empty() || text.contains('\n') {
            return None;
        }
        self.commands()
            .iter()
            .find(|command| command.name == name)?
            .hint
            .as_deref()
    }

    /// One key while the suggestion list is open: the arrows move the pick,
    /// Tab writes it into the box, Enter sends it or writes the one that
    /// takes an argument, and Escape closes the list. `None` where the list
    /// is closed, or the key is one the box takes as it does without it.
    pub(super) fn suggest_key(&mut self, key: KeyEvent) -> Option<Action> {
        let matches = self.suggestions()?;
        let count = matches.len();
        let picked = matches
            .get(self.pick(count))
            .map(|command| (*command).clone());
        let plain = !key
            .modifiers
            .intersects(KeyModifiers::SHIFT | KeyModifiers::ALT);
        match key.code {
            KeyCode::Up => self.suggested = self.pick(count).saturating_sub(1),
            KeyCode::Down => self.suggested = (self.pick(count) + 1).min(count.saturating_sub(1)),
            KeyCode::Esc => self.dismissed = true,
            KeyCode::Tab => {
                if let Some(command) = picked {
                    self.input.complete(&format!("/{}", command.name), true);
                }
            }
            KeyCode::Enter if plain => {
                let command = picked?;
                // A command that takes no argument goes as it is: the Enter
                // that sends every prompt sends it.
                let input = command.hint.is_some();
                self.input.complete(&format!("/{}", command.name), input);
                if !input {
                    return None;
                }
            }
            _ => return None,
        }
        Some(Action::None)
    }

    /// The box was edited: the pick starts again at the first match, and an
    /// empty box opens the list again after an Escape.
    pub(super) fn edited(&mut self) {
        self.suggested = 0;
        self.scrolled.set(0);
        if self.input.is_empty() {
            self.dismissed = false;
        }
    }
}

/// One row of the suggestion list: the mark, `/name` padded to `named`
/// columns, and the description dim.
fn row(command: &CommandDto, picked: bool, named: usize) -> Line<'static> {
    let (mark, style) = match picked {
        true => (OPTION_PICKED, PICKED),
        false => (OPTION_IDLE, Style::new()),
    };
    let name = format!("/{}", command.name);
    let pad = " ".repeat(named.saturating_sub(name.width()) + 2);
    let description = command
        .description
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    Line::from(vec![
        Span::styled(mark, style),
        Span::styled(name, style),
        Span::raw(pad),
        Span::styled(description, DIM),
    ])
}
