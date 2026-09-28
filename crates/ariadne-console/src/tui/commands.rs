//! The slash commands the agent lists, as the suggestion list over the box.
//!
//! The agent sends its commands as `available_commands_update` (008): each
//! list replaces the one before it, whole. While the box starts with `/` and
//! the cursor is still in its first word, the list shows the commands whose
//! name holds the rest of that word, and its keys pick one. A command picked
//! is text in the box, sent as every prompt is: the daemon is what tells a
//! command from a prompt.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame as Draw;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::Paragraph;
use serde_json::Value;
use unicode_width::UnicodeWidthStr;

use ariadne_api::events::AgentEventDto;

use crate::theme::{DIM, OPTION_IDLE, OPTION_PICKED, PICKED};

use super::{Action, Console};

/// The kind of the event that lists the agent's commands.
const LISTED: &str = "available_commands_update";

/// The most rows the suggestion list takes.
const ROWS: usize = 8;

/// The one row of a list that no command matches.
const NO_MATCH: &str = "no matching command";

/// One command the agent lists: an ACP `AvailableCommand`.
#[derive(Debug, Clone)]
pub(super) struct Command {
    /// The name, without its `/`.
    name: String,
    description: String,
    /// Whether it takes an argument: the entry has an `input`.
    input: bool,
    /// What the argument is, as the agent says it.
    hint: Option<String>,
}

/// The commands an event lists, in the agent's order: an entry with no name
/// is left out.
fn listed(event: &AgentEventDto) -> Vec<Command> {
    let Some(entries) = event
        .payload
        .get("available_commands")
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|entry| {
            let name = entry
                .get("name")?
                .as_str()
                .filter(|name| !name.is_empty())?;
            let input = entry.get("input").filter(|input| !input.is_null());
            Some(Command {
                name: name.to_string(),
                description: entry
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                input: input.is_some(),
                hint: input
                    .and_then(|input| input.get("hint"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
            })
        })
        .collect()
}

/// The commands whose name holds `query`, without regard to case: first the
/// names that start with it, then the names that hold it elsewhere, each
/// group in the agent's order.
fn matching<'a>(commands: &'a [Command], query: &str) -> Vec<&'a Command> {
    let query = query.to_lowercase();
    let (mut first, rest): (Vec<_>, Vec<_>) = commands
        .iter()
        .filter(|command| command.name.to_lowercase().contains(&query))
        .partition(|command| command.name.to_lowercase().starts_with(&query));
    first.extend(rest);
    first
}

impl Console {
    /// Take the list an `available_commands_update` carries in place of the
    /// one before it. `true` where the event is one: it draws no block and
    /// moves nothing else.
    pub(super) fn follow_commands(&mut self, event: &AgentEventDto) -> bool {
        if event.kind != LISTED {
            return false;
        }
        self.commands = listed(event);
        true
    }

    /// The commands the suggestion list shows, while it is open: the agent
    /// lists some, the box starts with `/`, no white space is before the
    /// cursor, no permission question waits, and no Escape closed it since
    /// the box was last empty.
    fn suggestions(&self) -> Option<Vec<&Command>> {
        if self.commands.is_empty() || self.dismissed || self.question().is_some() {
            return None;
        }
        let word = self.input.leading_word()?;
        let query = word.strip_prefix('/')?;
        Some(matching(&self.commands, query))
    }

    /// The pick among `count` matches.
    fn pick(&self, count: usize) -> usize {
        self.suggested.min(count.saturating_sub(1))
    }

    /// How many rows the suggestion list takes on a pane `width` by
    /// `height`: a row per match, or the one row that says none does, eight
    /// at most, and no more than the status row, the box and the footer
    /// leave.
    pub(super) fn suggestion_rows(&self, width: u16, height: u16) -> u16 {
        let Some(matches) = self.suggestions() else {
            return 0;
        };
        let room = height.saturating_sub(self.pinned_rows(width));
        u16::try_from(matches.len().clamp(1, ROWS))
            .unwrap_or(1)
            .min(room)
    }

    /// Draw the suggestion list: one row per command, `/name` and the
    /// description dim, the pick marked as the permission picker marks its
    /// own, scrolled to keep the pick on the screen. A row too wide is cut.
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

    /// The hint of the command the box names, while its argument is empty:
    /// the box is `/name` and white space alone after it, on one line.
    pub(super) fn hint(&self) -> Option<&str> {
        let text = self.input.joined();
        let (name, argument) = text.strip_prefix('/')?.split_once(char::is_whitespace)?;
        if !argument.trim().is_empty() || text.contains('\n') {
            return None;
        }
        self.commands
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
                // A command without an argument goes as it is: the Enter
                // that sends every prompt sends it.
                self.input
                    .complete(&format!("/{}", command.name), command.input);
                if !command.input {
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
/// columns, and the description dim, on one line.
fn row(command: &Command, picked: bool, named: usize) -> Line<'static> {
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

#[cfg(test)]
mod tests {

    use ratatui::backend::TestBackend;
    use ratatui::style::Modifier;
    use serde_json::json;
    use unicode_width::UnicodeWidthStr;

    use crate::tui::testing::*;
    use crate::tui::*;

    fn listing(commands: serde_json::Value) -> AgentEventDto {
        event(
            "available_commands_update",
            "commands",
            json!({"session_id": "agent-session", "available_commands": commands}),
        )
    }

    /// The agent's list: a command with an argument, three without, and an
    /// entry with no name.
    fn listed() -> AgentEventDto {
        listing(json!([
            {"name": "review", "description": "Review the branch",
             "input": {"hint": "what to review"}},
            {"name": "mcp:code", "description": "Run the code server's prompt"},
            {"name": "compact", "description": "Compact the conversation"},
            {"description": "an entry with no name"},
            {"name": "init", "description": "Write an AGENTS.md"},
        ]))
    }

    fn key_of(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn press(console: &mut Console, code: KeyCode) -> Action {
        console.key(key_of(code))
    }

    fn drawn(console: &Console, width: u16, height: u16) -> Terminal<TestBackend> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| console.render(frame)).unwrap();
        terminal
    }

    fn lines_of(console: &Console, width: u16, height: u16) -> Vec<String> {
        screen(&drawn(console, width, height))
            .lines()
            .map(str::to_string)
            .collect()
    }

    /// The rows between the status row and the top rule of the box, with
    /// the status row: the pinned rows of the pane, from its bottom up.
    fn suggested(console: &Console, width: u16, height: u16) -> (String, Vec<String>) {
        let lines = lines_of(console, width, height);
        let status = lines
            .iter()
            .rposition(|line| line.starts_with(" author · "))
            .expect("a status row");
        let rule = lines
            .iter()
            .skip(status)
            .position(|line| line.starts_with('─'))
            .expect("the box's top rule")
            + status;
        (lines[status].clone(), lines[status + 1..rule].to_vec())
    }

    #[test]
    fn a_slash_in_an_empty_box_opens_the_list_with_each_command_and_its_description() {
        let mut console = Console::new(header());
        console.apply(&listed());

        type_into(&mut console, "/");
        let (status, rows) = suggested(&console, 72, 20);

        assert_eq!(
            rows,
            [
                "❯ /review    Review the branch",
                "  /mcp:code  Run the code server's prompt",
                "  /compact   Compact the conversation",
                "  /init      Write an AGENTS.md",
            ],
            "one row per named command, over the box, under the status row"
        );
        assert!(status.contains("claude:opus"), "{status}");
    }

    #[test]
    fn the_description_is_dim_and_the_pick_is_marked_as_the_picker_marks_its_own() {
        let mut console = Console::new(header());
        console.apply(&listed());
        type_into(&mut console, "/");

        let terminal = drawn(&console, 72, 20);
        let buffer = terminal.backend().buffer();
        let shown = screen(&terminal);
        let row = row_of(&shown, "/review").unwrap() as u16;
        let description = shown
            .lines()
            .nth(row.into())
            .unwrap()
            .find("Review")
            .unwrap() as u16;

        assert_eq!(buffer[(0, row)].symbol(), "❯");
        assert_eq!(
            buffer[(2, row)].style().fg,
            Some(ratatui::style::Color::Magenta)
        );
        assert!(buffer[(2, row)].modifier.contains(Modifier::BOLD));
        assert!(buffer[(description, row)].modifier.contains(Modifier::DIM));
    }

    #[test]
    fn with_no_list_or_an_empty_list_a_slash_opens_nothing() {
        let mut none = Console::new(header());
        type_into(&mut none, "/");
        let mut empty = Console::new(header());
        empty.apply(&listed());
        empty.apply(&listing(json!([])));
        type_into(&mut empty, "/");

        for console in [&none, &empty] {
            let (_, rows) = suggested(console, 72, 20);
            assert!(rows.is_empty(), "{rows:?}");
            assert_eq!(console.suggestion_rows(72, 20), 0);
        }
    }

    #[test]
    fn the_names_that_start_with_the_text_come_before_the_names_that_contain_it() {
        let mut console = Console::new(header());
        console.apply(&listed());

        type_into(&mut console, "/CO");
        let (_, rows) = suggested(&console, 72, 20);

        assert_eq!(
            rows,
            [
                "❯ /compact   Compact the conversation",
                "  /mcp:code  Run the code server's prompt",
            ],
            "case is not read, and a name without the text is hidden"
        );
    }

    #[test]
    fn down_then_tab_writes_the_second_name_and_a_space_and_sends_nothing() {
        let mut console = Console::new(header());
        console.apply(&listed());
        type_into(&mut console, "/");

        let moved = press(&mut console, KeyCode::Down);
        let tabbed = press(&mut console, KeyCode::Tab);
        let (_, rows) = suggested(&console, 72, 20);

        assert_eq!((moved, tabbed), (Action::None, Action::None));
        assert!(rows.is_empty(), "the space closed the list: {rows:?}");
        assert_eq!(console.input.joined(), "/mcp:code ");
    }

    #[test]
    fn up_and_down_move_the_pick_and_not_through_the_history() {
        let mut console = Console::new(header());
        console.apply(&listed());
        type_into(&mut console, "sent");
        enter(&mut console);
        type_into(&mut console, "/");

        press(&mut console, KeyCode::Down);
        press(&mut console, KeyCode::Down);
        press(&mut console, KeyCode::Up);
        let (_, rows) = suggested(&console, 72, 20);

        assert_eq!(console.input.joined(), "/");
        assert!(rows[1].starts_with("❯ /mcp:code"), "{rows:?}");
    }

    #[test]
    fn enter_on_a_command_without_input_sends_it_and_empties_the_box() {
        let mut console = Console::new(header());
        console.apply(&listed());
        type_into(&mut console, "/comp");

        let action = enter(&mut console);

        assert_eq!(action, Action::Send("/compact".into()));
        assert!(console.input.is_empty());
    }

    #[test]
    fn enter_on_a_command_with_input_writes_it_shows_the_hint_and_sends_nothing() {
        let mut console = Console::new(header());
        console.apply(&listed());
        type_into(&mut console, "/rev");

        let action = enter(&mut console);
        let terminal = drawn(&console, 72, 20);
        let shown = screen(&terminal);
        let row = row_of(&shown, "❯ /review what to review").expect(&shown);

        assert_eq!(action, Action::None);
        assert_eq!(console.input.joined(), "/review ", "the hint is not input");
        assert!(
            terminal.backend().buffer()[(10, row as u16)]
                .modifier
                .contains(Modifier::DIM),
            "the hint is dim: {shown}"
        );
    }

    #[test]
    fn the_next_enter_sends_the_command_with_its_argument() {
        let mut console = Console::new(header());
        console.apply(&listed());
        type_into(&mut console, "/rev");
        enter(&mut console);

        type_into(&mut console, "x");
        let shown = screen(&drawn(&console, 72, 20));
        let action = enter(&mut console);

        assert!(!shown.contains("what to review"), "{shown}");
        assert_eq!(action, Action::Send("/review x".into()));
    }

    #[test]
    fn a_text_no_command_matches_shows_no_matching_command_and_enter_sends_it_as_typed() {
        let mut console = Console::new(header());
        console.apply(&listed());
        type_into(&mut console, "/Users/x");

        let (_, rows) = suggested(&console, 72, 20);
        let terminal = drawn(&console, 72, 20);
        let row = row_of(&screen(&terminal), "no matching command").unwrap();
        let action = enter(&mut console);

        assert_eq!(rows, ["  no matching command"]);
        assert!(
            terminal.backend().buffer()[(2, row as u16)]
                .modifier
                .contains(Modifier::DIM)
        );
        assert_eq!(action, Action::Send("/Users/x".into()));
    }

    #[test]
    fn escape_closes_the_list_keeps_the_text_and_cancels_no_running_turn() {
        let mut console = Console::new(header());
        console.apply(&listed());
        console.apply(&event(
            "user_prompt_submit",
            "go",
            json!({"text": "go", "source": "console"}),
        ));
        type_into(&mut console, "/co");

        let action = press(&mut console, KeyCode::Esc);
        let (_, rows) = suggested(&console, 72, 20);

        assert_eq!(action, Action::None);
        assert!(rows.is_empty(), "{rows:?}");
        assert_eq!(console.input.joined(), "/co");
    }

    #[test]
    fn a_second_escape_while_a_turn_runs_cancels_it() {
        let mut console = Console::new(header());
        console.apply(&listed());
        console.apply(&event(
            "user_prompt_submit",
            "go",
            json!({"text": "go", "source": "console"}),
        ));
        type_into(&mut console, "/co");
        press(&mut console, KeyCode::Esc);

        assert_eq!(press(&mut console, KeyCode::Esc), Action::Cancel);
    }

    #[test]
    fn after_escape_the_list_stays_closed_until_the_box_is_empty_again() {
        let mut console = Console::new(header());
        console.apply(&listed());
        type_into(&mut console, "/co");
        press(&mut console, KeyCode::Esc);

        press(&mut console, KeyCode::Backspace);
        let closed = console.suggestion_rows(72, 20);
        press(&mut console, KeyCode::Backspace);
        press(&mut console, KeyCode::Backspace);
        type_into(&mut console, "/");

        assert_eq!(closed, 0, "one character less is not an empty box");
        assert_eq!(
            console.suggestion_rows(72, 20),
            4,
            "an empty box opened it again"
        );
    }

    #[test]
    fn while_a_permission_question_waits_a_slash_opens_no_list_and_the_picker_keys_work() {
        let mut console = Console::new(header());
        console.apply(&listed());
        console.apply(&asked());

        type_into(&mut console, "/");
        let rows = console.suggestion_rows(72, 20);
        press(&mut console, KeyCode::Down);

        assert_eq!(rows, 0);
        assert_eq!(enter(&mut console), Action::Send("yes".into()));
    }

    #[test]
    fn a_list_of_twenty_commands_draws_eight_rows_and_down_past_the_last_scrolls_it() {
        let mut console = Console::new(header());
        let twenty: Vec<_> = (1..=20)
            .map(|n| json!({"name": format!("command{n:02}"), "description": "d"}))
            .collect();
        console.apply(&listing(json!(twenty)));
        type_into(&mut console, "/");

        let (_, first) = suggested(&console, 72, 40);
        for _ in 0..8 {
            press(&mut console, KeyCode::Down);
        }
        let (_, scrolled) = suggested(&console, 72, 40);

        assert_eq!(first.len(), 8, "{first:?}");
        assert!(first[0].starts_with("❯ /command01"), "{first:?}");
        assert_eq!(scrolled.len(), 8, "{scrolled:?}");
        assert!(scrolled[0].starts_with("  /command02"), "{scrolled:?}");
        assert!(scrolled[7].starts_with("❯ /command09"), "{scrolled:?}");
    }

    #[test]
    fn at_60_by_20_with_the_list_open_the_status_row_the_box_and_the_footer_are_whole() {
        let mut console = Console::new(header());
        let twenty: Vec<_> = (1..=20)
            .map(|n| json!({"name": format!("command{n:02}"), "description": "d"}))
            .collect();
        console.apply(&listing(json!(twenty)));
        type_into(&mut console, "/");

        for (height, rows) in [(20, 8), (9, 4)] {
            let lines = lines_of(&console, 60, height);
            let last = lines.len() - 1;

            assert!(lines[last].starts_with(" enter send"), "{lines:#?}");
            assert!(lines[last - 1].starts_with('─'), "{lines:#?}");
            assert_eq!(lines[last - 2], "❯ /", "{lines:#?}");
            assert!(lines[last - 3].starts_with('─'), "{lines:#?}");
            assert!(
                lines[last - 4 - rows].starts_with(" author · "),
                "{rows} rows of the list at {height}: {lines:#?}"
            );
        }
    }

    #[test]
    fn a_row_wider_than_the_pane_is_cut_and_not_wrapped() {
        let mut console = Console::new(header());
        let long = "word ".repeat(30);
        console.apply(&listing(json!([
            {"name": "compact", "description": long},
            {"name": "init", "description": "short"},
        ])));
        type_into(&mut console, "/");

        let (_, rows) = suggested(&console, 40, 20);

        assert_eq!(rows.len(), 2, "{rows:?}");
        assert!(rows[0].width() <= 40, "{rows:?}");
        assert!(rows[1].starts_with("  /init"), "{rows:?}");
    }

    #[test]
    fn a_snapshot_without_the_list_after_one_with_it_leaves_no_list() {
        let mut console = Console::new(header());
        let prompt = event(
            "user_prompt_submit",
            "go",
            json!({"text": "go", "source": "console"}),
        );
        console.snapshot(&[listed(), prompt.clone()]);
        type_into(&mut console, "/");
        let opened = console.suggestion_rows(72, 20);

        console.snapshot(&[prompt]);

        assert_eq!(opened, 4, "the list at the head of a snapshot is taken");
        assert_eq!(console.suggestion_rows(72, 20), 0);
    }

    #[test]
    fn the_list_adds_no_block_to_the_transcript_and_leaves_the_status_row() {
        let mut console = Console::new(Header {
            status: "idle".into(),
            ..header()
        });
        let said = event("agent_message", "hi", json!({"text": "hi"}));
        let stop = event("stop", "stop", json!({"stop_reason": "end_turn"}));
        console.apply(&said);
        console.apply(&stop);
        let before = screen(&drawn(&console, 72, 20));

        console.apply(&listed());
        let streamed = screen(&drawn(&console, 72, 20));
        console.snapshot(&[said, listed(), stop]);
        let snapshot = screen(&drawn(&console, 72, 20));

        assert_eq!(streamed, before);
        assert_eq!(snapshot, before);
        assert!(!before.contains("available_commands_update"), "{before}");
    }
}
