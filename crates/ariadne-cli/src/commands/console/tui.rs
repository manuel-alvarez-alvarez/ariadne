//! The inline console: `ariadne attach` on a terminal.
//!
//! The shape is the one a person expects of a chat with an agent, and the one
//! the Codex CLI has: the transcript scrolls in the terminal's own buffer, and
//! a small viewport pinned under it holds a status line and the box being
//! typed into. Finished blocks leave the viewport with
//! [`Terminal::insert_before`], so what the agent said is still in the
//! scrollback after the console is closed — which is why the viewport is
//! [`Viewport::Inline`] and never the alternate screen.
//!
//! Three seams keep it testable without a terminal. [`Console`] is the whole
//! state and holds nothing of the terminal, so a `TestBackend` renders it.
//! [`drive`] takes the key stream as an argument, so a scripted one drives it
//! against a stub daemon. And [`Held`] is what takes raw mode and gives it
//! back, so the giving back can be proven on each way out.

use std::collections::VecDeque;
use std::io::{IsTerminal, Write};
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{
    Event as TermEvent, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers,
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use futures_util::{Stream, StreamExt};
use ratatui::backend::{Backend, CrosstermBackend};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Paragraph, Widget};
use ratatui::{Frame, Terminal, TerminalOptions, Viewport};
use tokio::time::{Instant, interval, sleep_until};

use ariadne_api::events::AgentEventDto;
use ariadne_api::sessions::{ConsoleInputRequest, SessionDto};
use ariadne_client::{Client, SseEvent, SseStream};

use super::markdown;
use crate::commands::follow;
use crate::commands::transcript::{self, PermissionOption, TranscriptItem};
use crate::output::note;

/// How tall the inline viewport is. The bottom [`PINNED`] rows are the status
/// line and the input box; the rest shows the block still being written, which
/// moves into the scrollback the moment it is finished.
const VIEWPORT: u16 = 12;
/// The status line, and the input box with its two border rows.
const PINNED: u16 = 4;
/// How many rows of typed text the input box grows to before it scrolls.
const INPUT_ROWS: usize = 4;
/// A thought and a tool's output are context, not the answer: they are folded
/// to this many lines with a count of what was left out.
const FOLD: usize = 4;
/// How often the spinner turns while a turn runs.
const TICK: Duration = Duration::from_millis(120);
const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

const USER: Style = Style::new().fg(Color::Cyan);
const AGENT: Style = Style::new().fg(Color::Green);
const TOOL: Style = Style::new().fg(Color::Yellow);
const PLAN: Style = Style::new().fg(Color::Blue);
const ASK: Style = Style::new().fg(Color::Magenta);
const FAIL: Style = Style::new().fg(Color::Red).add_modifier(Modifier::BOLD);
const DIM: Style = Style::new().add_modifier(Modifier::DIM);

/// A ratatui backend whose failures `anyhow` can carry: the real terminal's
/// and the test one's alike.
pub trait Screen: Backend<Error: std::error::Error + Send + Sync + 'static> {}

impl<B> Screen for B
where
    B: Backend,
    B::Error: std::error::Error + Send + Sync + 'static,
{
}

/// Whether the inline console can be drawn at all.
///
/// Both ends have to be a terminal: the keys come from one and the viewport is
/// drawn on the other, and a console with either of them redirected is a
/// script's, which reads the plain line protocol instead.
pub fn interactive(stdin: bool, stdout: bool) -> bool {
    stdin && stdout
}

/// Open the inline console on the process terminal.
pub async fn attach(client: &Client, id: &str) -> Result<()> {
    // The status line names the seat, the model and the session's status. A
    // session the daemon will not describe is still worth attaching to, so a
    // failure here costs the header and nothing else.
    let session = client
        .get_json::<SessionDto>(&format!("/v1/sessions/{id}"))
        .await
        .ok();
    let mut console = Console::new(Header::of(session.as_ref()));

    let held = Held::take(Raw::default())?;
    let mut terminal = Terminal::with_options(
        CrosstermBackend::new(std::io::stdout()),
        TerminalOptions {
            viewport: Viewport::Inline(VIEWPORT),
        },
    )?;
    let outcome = drive(client, id, &mut terminal, EventStream::new(), &mut console).await;
    // Whatever ended it, the transcript belongs in the scrollback and the
    // viewport does not: both happen before the terminal is handed back.
    let _ = console.close(&mut terminal);
    drop(terminal);
    drop(held);

    note(&format!(
        "left the console — the session is still running; attach again with: ariadne attach {id}"
    ));
    outcome
}

/// What a key asked the console to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Nothing the daemon needs to hear about.
    None,
    /// Post this text to the session's console input: a typed prompt, or the
    /// id of the option a permission question was answered with.
    Send(String),
    /// Cancel the running turn.
    Cancel,
    /// Leave the console. The session stays alive.
    Quit,
}

/// What the agent is doing, as the status line says it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Turn {
    /// Between turns: the agent is waiting to be told something.
    Idle,
    Thinking,
    Running(String),
}

impl Turn {
    fn running(&self) -> bool {
        *self != Turn::Idle
    }
}

/// Whether the console is reading the daemon's stream or dialling it again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Link {
    Live,
    Reconnecting,
}

/// The unchanging half of the status line.
#[derive(Debug, Clone, Default)]
pub struct Header {
    seat: String,
    /// The `<agent>:<model>` pin: the agent and the model it runs, in one.
    model: String,
    status: String,
}

impl Header {
    fn of(session: Option<&SessionDto>) -> Self {
        session.map_or_else(
            || Self {
                seat: "session".into(),
                model: "-".into(),
                status: "unknown".into(),
            },
            |session| Self {
                seat: session.seat.as_str().into(),
                model: session.model.clone(),
                status: session.status.as_str().into(),
            },
        )
    }
}

/// Everything the console shows and everything a key changes about it.
///
/// No terminal in here on purpose: the whole of the drawing is
/// [`Console::render`] into a `Frame`, which a `TestBackend` gives as readily
/// as the real one.
pub struct Console {
    header: Header,
    /// Every block of the transcript, the ones already in the scrollback
    /// included — a reconnect brings the whole snapshot back, and the count
    /// below is how it is not printed twice.
    items: Vec<TranscriptItem>,
    /// How many leading items have gone into the terminal's own buffer.
    committed: usize,
    /// Where each prompt typed but not yet confirmed sits, oldest first, so
    /// the `user_prompt_submit` that comes back replaces the one it belongs
    /// to rather than doubling it. Several can be waiting at once: input
    /// posted while a turn runs is queued by the daemon (008).
    pending: VecDeque<usize>,
    picked: usize,
    turn: Turn,
    link: Link,
    input: Input,
    tick: usize,
    /// One Ctrl-C has been seen: the next one leaves.
    armed: bool,
}

impl Console {
    pub fn new(header: Header) -> Self {
        Self {
            header,
            items: Vec::new(),
            committed: 0,
            pending: VecDeque::new(),
            picked: 0,
            turn: Turn::Idle,
            link: Link::Live,
            input: Input::default(),
            tick: 0,
            armed: false,
        }
    }

    /// Rebuild from a fresh snapshot, keeping what is already in the
    /// scrollback out of it.
    ///
    /// This is what a reconnect lands on. The daemon's stream has no replay,
    /// so the whole transcript arrives again; the items before `committed`
    /// were printed by the connection that dropped and are not printed twice.
    ///
    /// The count is all there is to go on, and the two folds do not always
    /// agree on it: a turn that streamed around a tool call is two blocks
    /// live and one block stored. A block can therefore be lost from the
    /// viewport across a reconnect. It is in the scrollback, where it was
    /// printed, and the next block redraws the viewport.
    pub fn snapshot(&mut self, events: &[AgentEventDto]) {
        let mut items = Vec::new();
        for event in events {
            absorb(&mut items, event);
            self.follow_turn(event);
        }
        self.committed = self.committed.min(items.len());
        self.items = items;
        self.pending.clear();
    }

    /// Fold one streamed event into the transcript.
    pub fn apply(&mut self, event: &AgentEventDto) {
        self.follow_turn(event);
        if event.kind == "user_prompt_submit"
            && event.payload.get("source").and_then(|s| s.as_str()) == Some("console")
            && let Some(at) = self.pending.pop_front()
            && at < self.items.len()
        {
            self.items[at] = TranscriptItem::from(event);
            return;
        }
        if event.kind == "permission_request" {
            self.picked = 0;
        }
        absorb(&mut self.items, event);
    }

    /// What the status line says the agent is doing.
    fn follow_turn(&mut self, event: &AgentEventDto) {
        self.turn = match event.kind.as_str() {
            "stop" | "session_end" | "session.error" => Turn::Idle,
            "pre_tool_use" | "tool_call_update" | "post_tool_use" => {
                match TranscriptItem::from(event) {
                    TranscriptItem::ToolCall { name, status, .. }
                        if !transcript::tool_is_terminal(status.as_deref()) =>
                    {
                        Turn::Running(name)
                    }
                    _ => Turn::Thinking,
                }
            }
            "user_prompt_submit"
            | "agent_message"
            | "agent_message_chunk"
            | "agent_thought"
            | "agent_thought_chunk"
            | "plan"
            | "permission_request"
            | "permission.replied" => Turn::Thinking,
            _ => return,
        };
    }

    /// The stream dropped: say so, until it is back.
    pub fn dropped(&mut self) {
        self.link = Link::Reconnecting;
    }

    /// Something the console asked the daemon for did not happen. It belongs
    /// on the transcript, where the prompt it answers is.
    ///
    /// The turn goes back to idle with it: nothing was asked of the agent, so
    /// a spinner saying it is thinking would be a spinner over nothing.
    pub fn failed(&mut self, text: &str) {
        self.items.push(TranscriptItem::Error {
            meta: transcript::ItemMeta {
                created_at: chrono::Utc::now().to_rfc3339(),
                kinds: vec!["session.error".into()],
            },
            text: text.to_string(),
        });
        self.turn = Turn::Idle;
    }

    pub fn live(&mut self) {
        self.link = Link::Live;
    }

    pub fn tick(&mut self) {
        self.tick = self.tick.wrapping_add(1);
    }

    /// The pending permission question, if one is waiting for an answer.
    fn question(&self) -> Option<usize> {
        self.items.iter().rposition(|item| {
            matches!(
                item,
                TranscriptItem::PermissionQuestion { answer: None, .. }
            )
        })
    }

    fn options(&self, at: usize) -> &[PermissionOption] {
        match &self.items[at] {
            TranscriptItem::PermissionQuestion { options, .. } => options,
            _ => &[],
        }
    }

    /// Handle one key press. The caller does what the answer asks for.
    pub fn key(&mut self, key: KeyEvent) -> Action {
        if key.kind == KeyEventKind::Release {
            return Action::None;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let newline = key
            .modifiers
            .intersects(KeyModifiers::SHIFT | KeyModifiers::ALT);

        // Ctrl-C twice, so that one of them cannot throw away a half-typed
        // prompt; Ctrl-D at once, which is the end-of-input a shell gives.
        if ctrl && key.code == KeyCode::Char('c') {
            return match std::mem::replace(&mut self.armed, true) {
                true => Action::Quit,
                false => Action::None,
            };
        }
        self.armed = false;
        if ctrl && key.code == KeyCode::Char('d') {
            return Action::Quit;
        }

        if let Some(at) = self.question() {
            let count = self.options(at).len();
            match key.code {
                KeyCode::Up => {
                    self.picked = self.picked.saturating_sub(1);
                    return Action::None;
                }
                KeyCode::Down => {
                    self.picked = (self.picked + 1).min(count.saturating_sub(1));
                    return Action::None;
                }
                KeyCode::Char(digit @ '1'..='9') => {
                    let index = digit as usize - '1' as usize;
                    if index < count {
                        self.picked = index;
                    }
                    return Action::None;
                }
                KeyCode::Enter => {
                    return match self.options(at).get(self.picked) {
                        Some(option) => Action::Send(option.id.clone()),
                        None => Action::None,
                    };
                }
                _ => {}
            }
        }

        match key.code {
            KeyCode::Esc if self.turn.running() => Action::Cancel,
            KeyCode::Enter if newline => {
                self.input.newline();
                Action::None
            }
            KeyCode::Enter => match self.input.take() {
                Some(text) => {
                    self.show_pending(&text);
                    Action::Send(text)
                }
                None => Action::None,
            },
            _ => {
                self.input.key(key);
                Action::None
            }
        }
    }

    /// Put a just-typed prompt on the transcript straight away, rather than
    /// after the round trip that confirms it.
    fn show_pending(&mut self, text: &str) {
        self.pending.push_back(self.items.len());
        self.items.push(TranscriptItem::UserPrompt {
            meta: transcript::ItemMeta {
                created_at: chrono::Utc::now().to_rfc3339(),
                kinds: vec!["user_prompt_submit".into()],
            },
            text: text.to_string(),
            source: Some("console".into()),
        });
        self.turn = Turn::Thinking;
    }

    /// How many leading items are finished with and may leave the viewport.
    ///
    /// The last item is never one of them — it is what is still being written
    /// — and neither is an unanswered question, which is the picker, nor a
    /// prompt still waiting to be confirmed.
    ///
    /// It never goes backwards: what is in the scrollback is in the
    /// scrollback, whatever a fresh snapshot makes of the items around it.
    fn settled(&self) -> usize {
        let mut end = self.items.len().saturating_sub(1);
        if let Some(pending) = self.pending.front() {
            end = end.min(*pending);
        }
        for (at, item) in self.items.iter().enumerate().take(end).skip(self.committed) {
            if matches!(
                item,
                TranscriptItem::PermissionQuestion { answer: None, .. }
            ) {
                return at;
            }
        }
        end.max(self.committed)
    }

    /// Move every finished block into the terminal's own buffer, above the
    /// viewport, where the scrollback keeps it.
    pub fn commit<B: Screen>(&mut self, terminal: &mut Terminal<B>) -> Result<()> {
        let end = self.settled();
        self.emit(terminal, end)
    }

    /// The way out: everything left goes to the scrollback, and the viewport
    /// is wiped so the shell comes back to a clean line.
    pub fn close<B: Screen>(&mut self, terminal: &mut Terminal<B>) -> Result<()> {
        let end = self.items.len();
        self.emit(terminal, end)?;
        terminal.clear()?;
        Ok(())
    }

    fn emit<B: Screen>(&mut self, terminal: &mut Terminal<B>, end: usize) -> Result<()> {
        let width = usize::from(terminal.size()?.width);
        for at in self.committed..end {
            let mut lines = block(&self.items[at], width, None);
            lines.push(Line::default());
            let height = u16::try_from(lines.len()).unwrap_or(u16::MAX);
            terminal.insert_before(height, |buffer| {
                Paragraph::new(Text::from(lines)).render(buffer.area, buffer);
            })?;
        }
        self.committed = end;
        Ok(())
    }

    /// Draw the viewport: what is being written, the status line, the box.
    pub fn render(&self, frame: &mut Frame) {
        // The pinned area is the status line and a one-row input box, and
        // grows only as the box does.
        let rows = self.input.rows().clamp(1, INPUT_ROWS);
        let grown = u16::try_from(rows - 1).unwrap_or(0);
        let [live, status, input] = Layout::vertical([
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(PINNED - 1 + grown),
        ])
        .areas(frame.area());

        let width = usize::from(live.width);
        let asking = self.question();
        let mut lines = Vec::new();
        for (at, item) in self.items.iter().enumerate().skip(self.committed) {
            let picked = (Some(at) == asking).then_some(self.picked);
            lines.extend(block(item, width, picked));
        }
        // The tail is what is happening now; the head of a long block has
        // scrolled past, exactly as it would have in the scrollback.
        let height = usize::from(live.height);
        let skip = lines.len().saturating_sub(height);
        frame.render_widget(Paragraph::new(Text::from(lines[skip..].to_vec())), live);

        frame.render_widget(Paragraph::new(self.status()), status);

        let block = Block::bordered().border_style(DIM);
        let inner = block.inner(input);
        frame.render_widget(&block, input);
        let (cursor, scroll) = self
            .input
            .view(usize::from(inner.height), usize::from(inner.width));
        frame.render_widget(Paragraph::new(self.input.text()).scroll(scroll), inner);
        frame.set_cursor_position((inner.x + cursor.0, inner.y + cursor.1));
    }

    /// The status line: who is answering, on what, and what it is doing.
    fn status(&self) -> Line<'static> {
        let mut spans = vec![
            Span::styled(format!("{} ", self.header.seat), AGENT),
            Span::styled(self.header.model.clone(), DIM),
            Span::styled(" · ", DIM),
        ];
        match self.link {
            Link::Reconnecting => spans.push(Span::styled("reconnecting", TOOL)),
            Link::Live => spans.push(Span::raw(self.header.status.clone())),
        }
        match &self.turn {
            Turn::Idle => {}
            Turn::Thinking => {
                spans.push(Span::styled(format!("  {} ", self.spinner()), AGENT));
                spans.push(Span::styled("thinking", DIM));
            }
            Turn::Running(tool) => {
                spans.push(Span::styled(format!("  {} ", self.spinner()), TOOL));
                spans.push(Span::styled(format!("running {tool}"), DIM));
            }
        }
        spans.push(Span::styled(
            match (self.armed, self.question().is_some(), self.turn.running()) {
                (true, _, _) => "   ctrl-c again to leave".to_string(),
                (_, true, _) => "   ↑↓ choose · enter answer".to_string(),
                (_, _, true) => "   enter send · esc cancel · ctrl-c quit".to_string(),
                _ => "   enter send · shift+enter newline · ctrl-c quit".to_string(),
            },
            DIM,
        ));
        Line::from(spans)
    }

    fn spinner(&self) -> &'static str {
        SPINNER[self.tick % SPINNER.len()]
    }
}

/// Fold one event into a live transcript.
///
/// [`transcript::fold_into`] does everything a snapshot needs; a live console
/// needs one thing more, which is that a run of chunks is one block and not
/// one block per chunk.
fn absorb(items: &mut Vec<TranscriptItem>, event: &AgentEventDto) {
    let chunk = matches!(
        event.kind.as_str(),
        "agent_message_chunk" | "agent_thought_chunk"
    );
    let whole = matches!(event.kind.as_str(), "agent_message" | "agent_thought");
    if !chunk && !whole {
        transcript::fold_into(items, event);
        return;
    }
    let thought = event.kind.starts_with("agent_thought");

    if chunk {
        // Only the *last* block is still being written into. A block the
        // viewport has already handed to the scrollback cannot grow, so text
        // the agent writes after a tool call starts a block of its own — and
        // one turn is two blocks with the call between them, the way it read
        // as it happened.
        let open = items.last_mut().filter(|last| is_open(last, thought));
        if let Some(
            TranscriptItem::Thought { meta, text } | TranscriptItem::AgentText { meta, text },
        ) = open
        {
            text.push_str(&chunk_text(event));
            meta.kinds.push(event.kind.clone());
            return;
        }
        items.push(TranscriptItem::from(event));
        return;
    }

    // The whole the daemon stores at the end of a turn (021) is every chunk
    // of that turn joined into one. Where chunks arrived it says again what
    // is already on the screen — as one block, where a turn with a tool call
    // in it drew two. So it closes the blocks the chunks opened and adds no
    // text; only a turn that streamed nothing pushes a block of its own.
    let mut closed = false;
    for item in items.iter_mut() {
        if is_open(item, thought)
            && let TranscriptItem::Thought { meta, .. } | TranscriptItem::AgentText { meta, .. } =
                item
        {
            meta.kinds.push(event.kind.clone());
            closed = true;
        }
    }
    if !closed {
        items.push(TranscriptItem::from(event));
    }
}

/// Whether this block is still taking chunks of the kind asked for.
fn is_open(item: &TranscriptItem, thought: bool) -> bool {
    let is_kind = match item {
        TranscriptItem::Thought { .. } => thought,
        TranscriptItem::AgentText { .. } => !thought,
        _ => false,
    };
    is_kind
        && item
            .meta()
            .kinds
            .last()
            .is_some_and(|kind| kind.ends_with("_chunk"))
}

/// The text one chunk carries, whichever of the two kinds it is.
fn chunk_text(event: &AgentEventDto) -> String {
    match TranscriptItem::from(event) {
        TranscriptItem::Thought { text, .. } | TranscriptItem::AgentText { text, .. } => text,
        _ => String::new(),
    }
}

/// One transcript item as the lines the console draws it on.
///
/// `picked` is the highlighted option of a permission question being answered
/// right now — `None` everywhere else, including once it is in the scrollback.
fn block(item: &TranscriptItem, width: usize, picked: Option<usize>) -> Vec<Line<'static>> {
    let width = width.max(8);
    match item {
        TranscriptItem::UserPrompt { text, .. } => prefixed(text, "> ", USER, USER, width, None),
        TranscriptItem::AgentText { text, .. } => {
            let mut lines = vec![Line::from(Span::styled("● ", AGENT))];
            for line in markdown::render(text, width.saturating_sub(2)) {
                let mut spans = vec![Span::raw("  ")];
                spans.extend(line.spans);
                lines.push(Line::from(spans));
            }
            // The bullet marks where the agent started speaking; its first
            // line of text rides on it rather than under it.
            join_first(lines)
        }
        TranscriptItem::Thought { text, .. } => prefixed(text, "· ", DIM, DIM, width, Some(FOLD)),
        TranscriptItem::Plan { entries, .. } => {
            let mut lines = vec![Line::from(Span::styled("plan", PLAN))];
            for entry in entries {
                let done = entry.status == "completed";
                lines.push(Line::from(vec![
                    Span::styled(if done { "  ☑ " } else { "  ☐ " }, PLAN),
                    Span::styled(entry.content.clone(), if done { DIM } else { Style::new() }),
                ]));
            }
            lines
        }
        TranscriptItem::ToolCall {
            name,
            input,
            output,
            diff,
            status,
            ..
        } => tool(
            name,
            input,
            output.as_deref(),
            diff.as_deref(),
            status.as_deref(),
            width,
        ),
        TranscriptItem::PermissionQuestion {
            question,
            options,
            answer,
            ..
        } => permission(question, options, answer.as_deref(), picked),
        TranscriptItem::SystemNote { text, .. } => prefixed(text, "  ", DIM, DIM, width, None),
        TranscriptItem::Error { text, .. } => prefixed(text, "✗ ", FAIL, FAIL, width, None),
        TranscriptItem::Raw { kind, .. } => {
            vec![Line::from(Span::styled(kind.clone(), DIM))]
        }
    }
}

/// Pull the first line of text up onto the marker line above it.
fn join_first(mut lines: Vec<Line<'static>>) -> Vec<Line<'static>> {
    if lines.len() > 1 {
        let second = lines.remove(1);
        let marker = lines[0].spans.clone();
        let mut spans = marker;
        spans.extend(second.spans.into_iter().skip(1));
        lines[0] = Line::from(spans);
    }
    lines
}

/// A block of plain text behind a two-column marker, wrapped and optionally
/// folded to a few lines.
fn prefixed(
    text: &str,
    marker: &str,
    marker_style: Style,
    style: Style,
    width: usize,
    fold: Option<usize>,
) -> Vec<Line<'static>> {
    let mut wrapped = wrap(text, width.saturating_sub(marker.chars().count()));
    let hidden = fold.map_or(0, |fold| wrapped.len().saturating_sub(fold));
    if hidden > 0 {
        wrapped.truncate(fold.unwrap_or(wrapped.len()));
        wrapped.push(format!("… {hidden} more lines"));
    }
    wrapped
        .into_iter()
        .enumerate()
        .map(|(at, line)| {
            let lead = if at == 0 {
                Span::styled(marker.to_string(), marker_style)
            } else {
                Span::raw(" ".repeat(marker.chars().count()))
            };
            Line::from(vec![lead, Span::styled(line, style)])
        })
        .collect()
}

fn tool(
    name: &str,
    input: &serde_json::Value,
    output: Option<&str>,
    diff: Option<&str>,
    status: Option<&str>,
    width: usize,
) -> Vec<Line<'static>> {
    let glyph = match status {
        Some("completed") => "✓",
        Some("failed") => "✗",
        Some("in_progress") => "●",
        _ => "○",
    };
    let argument = match input {
        serde_json::Value::Null => String::new(),
        serde_json::Value::String(text) => format!(" {text}"),
        input => format!(" {}", serde_json::to_string(input).unwrap_or_default()),
    };
    let head: String = format!("{name}{argument}")
        .chars()
        .take(width.saturating_sub(2))
        .collect();
    let mut lines = vec![Line::from(vec![
        Span::styled(format!("{glyph} "), TOOL),
        Span::raw(head),
    ])];
    if let Some(output) = output {
        lines.extend(folded(output, width, DIM));
    }
    if let Some(diff) = diff {
        lines.extend(folded_diff(diff, width));
    }
    lines
}

/// A tool's output: indented, and folded to the last few lines, which is
/// where a command says how it went.
fn folded(text: &str, width: usize, style: Style) -> Vec<Line<'static>> {
    let all = wrap(text.trim_end_matches('\n'), width.saturating_sub(4));
    let hidden = all.len().saturating_sub(FOLD);
    let mut lines = Vec::new();
    if hidden > 0 {
        lines.push(Line::from(Span::styled(
            format!("    … {hidden} more lines"),
            DIM,
        )));
    }
    for line in all.into_iter().skip(hidden) {
        lines.push(Line::from(vec![
            Span::raw("    "),
            Span::styled(line, style),
        ]));
    }
    lines
}

/// A diff, coloured the way every other diff the CLI prints is.
fn folded_diff(diff: &str, width: usize) -> Vec<Line<'static>> {
    diff.trim_end_matches('\n')
        .lines()
        .map(|line| {
            let style = match line.chars().next() {
                Some('+') => Style::new().fg(Color::Green),
                Some('-') => Style::new().fg(Color::Red),
                Some('@') => Style::new().fg(Color::Cyan),
                _ => DIM,
            };
            let text: String = line.chars().take(width.saturating_sub(4)).collect();
            Line::from(vec![Span::raw("    "), Span::styled(text, style)])
        })
        .collect()
}

/// A permission question: the options as a picker while it is open, and as
/// the answer once it is given.
fn permission(
    question: &str,
    options: &[PermissionOption],
    answer: Option<&str>,
    picked: Option<usize>,
) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(vec![
        Span::styled("? ", ASK),
        Span::styled(question.to_string(), ASK.add_modifier(Modifier::BOLD)),
    ])];
    if let Some(answer) = answer {
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(format!("answered: {answer}"), DIM),
        ]));
        return lines;
    }
    for (at, option) in options.iter().enumerate() {
        let chosen = picked == Some(at);
        lines.push(Line::from(vec![
            Span::styled(if chosen { "  › " } else { "    " }, ASK),
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

/// Hard-wrap text to `width`, keeping the line breaks it already has.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut out = Vec::new();
    for source in text.split('\n') {
        let mut line = String::new();
        let mut used = 0usize;
        for word in source.split_inclusive(char::is_whitespace) {
            let length = word.chars().count();
            if used > 0 && used + length > width {
                out.push(std::mem::take(&mut line).trim_end().to_string());
                used = 0;
            }
            used += length;
            line.push_str(word);
        }
        out.push(line.trim_end().to_string());
    }
    out
}

/// The multi-line box at the bottom.
struct Input {
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

    fn text(&self) -> Text<'static> {
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
    /// readable while it is being typed.
    fn view(&self, height: usize, width: usize) -> ((u16, u16), (u16, u16)) {
        let down = (self.row + 1).saturating_sub(height.max(1));
        let across = (self.column + 1).saturating_sub(width.max(1));
        let at = |value: usize| u16::try_from(value).unwrap_or(u16::MAX);
        (
            (at(self.column - across), at(self.row - down)),
            (at(down), at(across)),
        )
    }

    fn newline(&mut self) {
        let tail = self.lines[self.row].split_off(self.column);
        self.lines.insert(self.row + 1, tail);
        self.row += 1;
        self.column = 0;
    }

    /// Everything typed so far, emptying the box. `None` when it holds only
    /// whitespace: Enter on an empty box is not a prompt.
    fn take(&mut self) -> Option<String> {
        let text = self
            .lines
            .iter()
            .map(|line| line.iter().collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        *self = Self::default();
        (!text.trim().is_empty()).then_some(text)
    }

    fn key(&mut self, key: KeyEvent) {
        match key.code {
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

/// One turn of the loop: whichever of the four things the console waits on
/// happened first.
enum Step {
    Frame(SseEvent),
    Dropped,
    Redial,
    Key(TermEvent),
    Closed,
    Tick,
}

/// Run the console until the session ends or the user leaves it.
///
/// The key stream is an argument rather than `EventStream::new()` so that a
/// test can script one; everything else here is the real client and the real
/// terminal, whatever backend is under it.
pub async fn drive<B, K>(
    client: &Client,
    id: &str,
    terminal: &mut Terminal<B>,
    keys: K,
    console: &mut Console,
) -> Result<()>
where
    B: Screen,
    K: Stream<Item = std::io::Result<TermEvent>> + Unpin,
{
    let path = format!("/v1/sessions/{id}/console/stream");
    let mut keys = keys;
    let mut ticker = interval(TICK);

    // The stream opens with the transcript snapshot. Take it before any key,
    // so the first thing drawn is the conversation as it stands and a key
    // pressed at once answers a question that is already on the screen.
    let mut stream = Some(client.stream(&path).await?);
    if let Some(frame) = next_frame(&mut stream).await {
        console.snapshot(&super::events(&frame?)?);
    }

    let mut retry: Option<(Instant, Duration)> = None;
    loop {
        console.commit(terminal)?;
        terminal.draw(|frame| console.render(frame))?;

        let step = tokio::select! {
            biased;
            frame = next_frame(&mut stream), if stream.is_some() => match frame {
                Some(Ok(frame)) => Step::Frame(frame),
                Some(Err(_)) | None => Step::Dropped,
            },
            () = due(retry.map(|(at, _)| at)), if stream.is_none() => Step::Redial,
            key = keys.next() => match key {
                Some(Ok(event)) => Step::Key(event),
                Some(Err(_)) | None => Step::Closed,
            },
            _ = ticker.tick() => Step::Tick,
        };

        match step {
            Step::Frame(frame) => {
                let events = super::events(&frame)?;
                match frame.event.as_str() {
                    "snapshot" => console.snapshot(&events),
                    _ => {
                        for event in &events {
                            console.apply(event);
                        }
                    }
                }
                if events.iter().any(|event| event.kind == "session_end") {
                    return Ok(());
                }
            }
            Step::Dropped => {
                stream = None;
                console.dropped();
                let wait = follow::backoff(None);
                retry = Some((Instant::now() + wait, wait));
            }
            Step::Redial => match client.stream(&path).await {
                Ok(fresh) => {
                    stream = Some(fresh);
                    console.live();
                    retry = None;
                }
                Err(_) => {
                    let wait = follow::backoff(retry.map(|(_, wait)| wait));
                    retry = Some((Instant::now() + wait, wait));
                }
            },
            Step::Key(TermEvent::Key(key)) => match console.key(key) {
                Action::None => {}
                Action::Quit => return Ok(()),
                Action::Send(text) => {
                    // A refused or undelivered prompt is said on the
                    // transcript rather than thrown: a console that closed
                    // itself on one bad post would take a half-typed
                    // conversation with it.
                    if let Err(e) = client
                        .send_no_content(
                            http::Method::POST,
                            &format!("/v1/sessions/{id}/console/input"),
                            Some(&ConsoleInputRequest { text }),
                        )
                        .await
                    {
                        console.failed(&e.human());
                    }
                }
                Action::Cancel => {
                    // A turn that ended between the key and the post has
                    // nothing to cancel, and says so with a 409: not a
                    // failure of the console.
                    let _ = client
                        .send_no_content::<()>(
                            http::Method::POST,
                            &format!("/v1/sessions/{id}/console/cancel"),
                            None,
                        )
                        .await;
                }
            },
            Step::Key(_) => {}
            Step::Closed => return Ok(()),
            Step::Tick => console.tick(),
        }
    }
}

/// The next frame of a stream that may not be there.
async fn next_frame(
    stream: &mut Option<SseStream>,
) -> Option<Result<SseEvent, ariadne_client::ClientError>> {
    match stream {
        Some(stream) => stream.next().await,
        None => std::future::pending().await,
    }
}

/// Wait until `at`, or forever where there is nothing to wait for.
async fn due(at: Option<Instant>) {
    match at {
        Some(at) => sleep_until(at).await,
        None => std::future::pending().await,
    }
}

/// What the console takes from the terminal on the way in, and gives back on
/// the way out.
///
/// A trait so that the giving back can be proven: a console that left raw mode
/// on hands the shell back a terminal that echoes nothing.
pub trait Terminals {
    fn enter(&mut self) -> Result<()>;
    fn leave(&mut self);
}

/// The process terminal: raw mode, and the key protocol that tells Shift+Enter
/// from Enter where the terminal can report it.
#[derive(Default)]
pub struct Raw {
    enhanced: bool,
}

impl Terminals for Raw {
    fn enter(&mut self) -> Result<()> {
        crossterm::terminal::enable_raw_mode()?;
        // Only a terminal that speaks the keyboard protocol can report
        // Shift+Enter at all; Alt+Enter is the newline everywhere else.
        self.enhanced = crossterm::terminal::supports_keyboard_enhancement().unwrap_or(false)
            && crossterm::execute!(
                std::io::stdout(),
                PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
            )
            .is_ok();
        Ok(())
    }

    fn leave(&mut self) {
        if self.enhanced {
            let _ = crossterm::execute!(std::io::stdout(), PopKeyboardEnhancementFlags);
        }
        let _ = crossterm::terminal::disable_raw_mode();
        let _ = crossterm::execute!(std::io::stdout(), crossterm::cursor::Show);
        let _ = std::io::stdout().flush();
    }
}

/// The terminal, held for as long as the console runs.
///
/// Giving it back is a `Drop` rather than a line at the end of the function,
/// so it happens on every way out: the normal one, an error, a panic, and the
/// Ctrl-C that is a key here rather than a signal.
pub struct Held<T: Terminals>(T);

impl<T: Terminals> Held<T> {
    pub fn take(mut terminal: T) -> Result<Self> {
        terminal.enter()?;
        Ok(Self(terminal))
    }
}

impl<T: Terminals> Drop for Held<T> {
    fn drop(&mut self) {
        self.0.leave();
    }
}

/// Whether this process is attached to a terminal on both ends.
pub fn on_a_terminal() -> bool {
    interactive(
        std::io::stdin().is_terminal(),
        std::io::stdout().is_terminal(),
    )
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use axum::extract::State;
    use axum::http::StatusCode;
    use axum::response::sse::{Event as SseFrame, Sse};
    use axum::routing::{get, post};
    use axum::{Json, Router};
    use futures_util::stream;
    use ratatui::backend::TestBackend;
    use serde_json::json;
    use tokio::sync::broadcast;

    use super::*;

    /// A daemon console, as far as the CLI can tell: a snapshot, deltas of its
    /// own, and deltas it answers a post with — which is what makes a test
    /// that types deterministic, since nothing arrives that the typing did not
    /// cause.
    #[derive(Clone)]
    struct Stub {
        snapshot: Vec<AgentEventDto>,
        deltas: Vec<AgentEventDto>,
        on_input: Vec<AgentEventDto>,
        on_cancel: Vec<AgentEventDto>,
        outbox: broadcast::Sender<AgentEventDto>,
        prompts: Arc<Mutex<Vec<String>>>,
        cancels: Arc<AtomicUsize>,
        /// A session that will take no more input, which is the `409` a
        /// finished one gives.
        refuses: bool,
    }

    impl Stub {
        fn new(snapshot: Vec<AgentEventDto>) -> Self {
            Self {
                snapshot,
                deltas: Vec::new(),
                on_input: Vec::new(),
                on_cancel: Vec::new(),
                outbox: broadcast::channel(64).0,
                prompts: Arc::new(Mutex::new(Vec::new())),
                cancels: Arc::new(AtomicUsize::new(0)),
                refuses: false,
            }
        }

        fn refuses(mut self) -> Self {
            self.refuses = true;
            self
        }

        fn deltas(mut self, deltas: Vec<AgentEventDto>) -> Self {
            self.deltas = deltas;
            self
        }

        fn on_input(mut self, events: Vec<AgentEventDto>) -> Self {
            self.on_input = events;
            self
        }

        fn on_cancel(mut self, events: Vec<AgentEventDto>) -> Self {
            self.on_cancel = events;
            self
        }
    }

    async fn open(
        State(stub): State<Stub>,
    ) -> Sse<impl futures_util::Stream<Item = Result<SseFrame, Infallible>>> {
        let snapshot = SseFrame::default()
            .event("snapshot")
            .data(serde_json::to_string(&stub.snapshot).unwrap());
        let inbox = stub.outbox.subscribe();
        for delta in &stub.deltas {
            let _ = stub.outbox.send(delta.clone());
        }
        let live = stream::unfold(inbox, |mut inbox| async move {
            let event = inbox.recv().await.ok()?;
            let frame = SseFrame::default()
                .event("event")
                .data(serde_json::to_string(&event).unwrap());
            Some((Ok(frame), inbox))
        });
        Sse::new(stream::once(async move { Ok(snapshot) }).chain(live))
    }

    async fn input(State(stub): State<Stub>, Json(body): Json<ConsoleInputRequest>) -> StatusCode {
        stub.prompts.lock().unwrap().push(body.text);
        for event in &stub.on_input {
            let _ = stub.outbox.send(event.clone());
        }
        match stub.refuses {
            true => StatusCode::CONFLICT,
            false => StatusCode::NO_CONTENT,
        }
    }

    async fn cancel(State(stub): State<Stub>) -> StatusCode {
        stub.cancels.fetch_add(1, Ordering::SeqCst);
        for event in &stub.on_cancel {
            let _ = stub.outbox.send(event.clone());
        }
        StatusCode::NO_CONTENT
    }

    async fn serve(stub: Stub) -> (Client, tokio::task::JoinHandle<()>) {
        let app = Router::new()
            .route("/v1/sessions/session/console/stream", get(open))
            .route("/v1/sessions/session/console/input", post(input))
            .route("/v1/sessions/session/console/cancel", post(cancel))
            .with_state(stub);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (Client::tcp(format!("http://{address}")), server)
    }

    fn header() -> Header {
        Header {
            seat: "author".into(),
            model: "claude:opus".into(),
            status: "running".into(),
        }
    }

    fn terminal() -> Terminal<TestBackend> {
        Terminal::with_options(
            TestBackend::new(72, 40),
            TerminalOptions {
                viewport: Viewport::Inline(VIEWPORT),
            },
        )
        .unwrap()
    }

    /// Everything the terminal shows: the scrollback above the viewport and
    /// the viewport itself, as one block of text.
    fn screen(terminal: &Terminal<TestBackend>) -> String {
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn key(code: KeyCode) -> TermEvent {
        TermEvent::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn typed(text: &str) -> Vec<TermEvent> {
        text.chars().map(|c| key(KeyCode::Char(c))).collect()
    }

    /// Drive the console against the stub until the session ends, then read
    /// the screen. The key stream never ends, so only the daemon or a Ctrl-C
    /// stops the loop — as in the real console.
    async fn console(stub: Stub, keys: Vec<TermEvent>) -> (String, Vec<String>, usize) {
        let prompts = stub.prompts.clone();
        let cancels = stub.cancels.clone();
        let (client, server) = serve(stub).await;
        let mut terminal = terminal();
        let mut console = Console::new(header());
        let keys = stream::iter(keys.into_iter().map(Ok)).chain(stream::pending());

        drive(
            &client,
            "session",
            &mut terminal,
            Box::pin(keys),
            &mut console,
        )
        .await
        .unwrap();

        let shown = screen(&terminal);
        server.abort();
        let prompts = prompts.lock().unwrap().clone();
        (shown, prompts, cancels.load(Ordering::SeqCst))
    }

    fn event(kind: &str, summary: &str, payload: serde_json::Value) -> AgentEventDto {
        AgentEventDto {
            id: format!("event-{kind}-{summary}"),
            session_id: Some("session".into()),
            task_id: None,
            kind: kind.into(),
            payload,
            summary: summary.into(),
            created_at: "2026-09-12T00:00:00Z".into(),
        }
    }

    fn ended() -> AgentEventDto {
        event("session_end", "session ended", json!({}))
    }

    #[tokio::test]
    async fn a_transcript_renders_the_prompt_the_markdown_the_tool_call_and_the_status_line() {
        let (shown, _, _) = console(
            Stub::new(vec![
                event(
                    "user_prompt_submit",
                    "Run the tests",
                    json!({"text": "Run the tests", "source": "console"}),
                ),
                event(
                    "agent_message",
                    "report",
                    json!({"text": "# Report\n\nRan them:\n\n```sh\ncargo nextest run\n```"}),
                ),
                event(
                    "post_tool_use",
                    "Bash",
                    json!({
                        "tool_name": "Bash",
                        "tool_input": {"command": "cargo nextest run"},
                        "acp": {"toolCallId": "call-1", "status": "completed",
                                "rawOutput": {"stdout": "one\ntwo\nthree\nfour\nfive\nsix\n"}}
                    }),
                ),
            ])
            .deltas(vec![ended()]),
            Vec::new(),
        )
        .await;

        assert!(shown.contains("> Run the tests"), "{shown}");
        assert!(
            shown.contains("# Report"),
            "the heading is a heading: {shown}"
        );
        assert!(shown.contains("```sh"), "the code block is fenced: {shown}");
        assert!(shown.contains("cargo nextest run"), "{shown}");
        assert!(
            shown.contains("✓ Bash"),
            "a completed call is ticked: {shown}"
        );
        assert!(
            shown.contains("… 2 more lines") && shown.contains("six"),
            "the output is folded to its last lines: {shown}"
        );
        assert!(
            shown.contains("author") && shown.contains("claude:opus") && shown.contains("running"),
            "the status line names the seat, the model and the status: {shown}"
        );
    }

    #[tokio::test]
    async fn streamed_chunks_append_to_the_agent_block_that_is_already_open() {
        let (shown, _, _) = console(
            Stub::new(vec![event(
                "agent_message_chunk",
                "Half ",
                json!({"text": "Half "}),
            )])
            .deltas(vec![
                event("agent_message_chunk", "done.", json!({"text": "done."})),
                event("agent_message", "whole", json!({"text": "Half done."})),
                ended(),
            ]),
            Vec::new(),
        )
        .await;

        assert!(shown.contains("Half done."), "{shown}");
        assert_eq!(
            shown.matches('●').count(),
            1,
            "one block marker, not one per chunk: {shown}"
        );
    }

    /// The shape of a turn that says something, runs a tool, then says
    /// something else (021 rule 6). The block before the call is in the
    /// scrollback by the time the second chunk arrives, so it cannot grow:
    /// the text after the call is a block of its own, and the stored whole —
    /// which is both of them joined — repeats neither.
    #[tokio::test]
    async fn agent_text_after_a_tool_call_is_a_block_of_its_own() {
        let (shown, _, _) = console(
            Stub::new(Vec::new()).deltas(vec![
                event(
                    "agent_message_chunk",
                    "before",
                    json!({"text": "Let me run the tests."}),
                ),
                event(
                    "pre_tool_use",
                    "Bash",
                    json!({"tool_name": "Bash",
                           "acp": {"toolCallId": "call-1", "status": "pending"}}),
                ),
                event(
                    "post_tool_use",
                    "Bash",
                    json!({"tool_name": "Bash",
                           "acp": {"toolCallId": "call-1", "status": "completed",
                                   "rawOutput": "ok"}}),
                ),
                event(
                    "agent_message_chunk",
                    "after",
                    json!({"text": "The tests pass."}),
                ),
                event(
                    "agent_message",
                    "whole",
                    json!({"text": "Let me run the tests.The tests pass."}),
                ),
                event("stop", "stopped", json!({"stop_reason": "end_turn"})),
                ended(),
            ]),
            Vec::new(),
        )
        .await;

        assert_eq!(
            shown.matches("Let me run the tests.").count(),
            1,
            "the text before the call is said once: {shown}"
        );
        assert_eq!(
            shown.matches("The tests pass.").count(),
            1,
            "and the text after it is said once, not lost: {shown}"
        );
        assert!(
            shown.find("Let me run the tests.") < shown.find("✓ Bash")
                && shown.find("✓ Bash") < shown.find("The tests pass."),
            "in the order the turn happened: {shown}"
        );
    }

    #[test]
    fn a_second_prompt_typed_before_the_first_is_confirmed_keeps_both_apart() {
        let mut console = Console::new(header());
        let mut terminal = terminal();
        let submitted = |text: &str| {
            event(
                "user_prompt_submit",
                text,
                json!({"text": text, "source": "console"}),
            )
        };

        console.key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        console.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        console.key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
        console.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        console.apply(&submitted("a"));
        console.apply(&submitted("b"));
        terminal.draw(|frame| console.render(frame)).unwrap();

        let shown = screen(&terminal);
        assert_eq!(shown.matches("> a").count(), 1, "{shown}");
        assert_eq!(shown.matches("> b").count(), 1, "{shown}");
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

    fn asked() -> AgentEventDto {
        event(
            "permission_request",
            "Permission requested for Write",
            json!({"tool_name": "Write", "options": [
                {"optionId": "no", "name": "Reject"},
                {"optionId": "yes", "name": "Allow"}
            ]}),
        )
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

    #[tokio::test]
    async fn enter_posts_the_permission_option_the_picker_is_on() {
        let (_, prompts, _) = console(
            Stub::new(vec![asked()]).on_input(vec![
                event(
                    "permission.replied",
                    "answered",
                    json!({"option_id": "yes"}),
                ),
                ended(),
            ]),
            vec![key(KeyCode::Down), key(KeyCode::Enter)],
        )
        .await;

        assert_eq!(prompts, ["yes"], "the arrow moved the pick before Enter");
    }

    #[tokio::test]
    async fn escape_during_a_running_turn_cancels_it() {
        let (_, prompts, cancels) = console(
            Stub::new(vec![event(
                "pre_tool_use",
                "Bash",
                json!({"tool_name": "Bash", "acp": {"toolCallId": "call-1", "status": "pending"}}),
            )])
            .on_cancel(vec![ended()]),
            vec![key(KeyCode::Esc)],
        )
        .await;

        assert_eq!(cancels, 1, "escape cancelled the turn");
        assert!(prompts.is_empty(), "and posted no prompt");
    }

    #[tokio::test]
    async fn a_typed_line_is_posted_and_its_pending_prompt_is_replaced_by_the_confirmed_one() {
        let mut keys = typed("hi");
        keys.push(key(KeyCode::Enter));
        let (shown, prompts, _) = console(
            Stub::new(Vec::new()).on_input(vec![
                event(
                    "user_prompt_submit",
                    "hi",
                    json!({"text": "hi", "source": "console"}),
                ),
                ended(),
            ]),
            keys,
        )
        .await;

        assert_eq!(prompts, ["hi"]);
        assert_eq!(
            shown.matches("> hi").count(),
            1,
            "the confirmed prompt took the pending one's place: {shown}"
        );
    }

    #[tokio::test]
    async fn a_refused_prompt_is_said_on_the_transcript_and_does_not_close_the_console() {
        let mut keys = typed("hi");
        keys.push(key(KeyCode::Enter));
        let (shown, prompts, _) = console(
            Stub::new(Vec::new()).refuses().on_input(vec![ended()]),
            keys,
        )
        .await;

        assert_eq!(prompts, ["hi"], "the post was made");
        assert!(
            shown.contains('✗'),
            "and its refusal is on the transcript: {shown}"
        );
    }

    #[test]
    fn a_pending_prompt_is_on_the_screen_before_the_daemon_confirms_it() {
        let mut console = Console::new(header());
        let mut terminal = terminal();

        for character in "ship it".chars() {
            console.key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        let action = console.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        terminal.draw(|frame| console.render(frame)).unwrap();

        assert_eq!(action, Action::Send("ship it".into()));
        assert!(
            screen(&terminal).contains("> ship it"),
            "the prompt shows before any event comes back: {}",
            screen(&terminal)
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
    fn one_ctrl_c_keeps_the_console_and_the_second_leaves_it() {
        let mut console = Console::new(header());
        let ctrl = |code| KeyEvent::new(code, KeyModifiers::CONTROL);

        assert_eq!(console.key(ctrl(KeyCode::Char('c'))), Action::None);
        assert_eq!(console.key(ctrl(KeyCode::Char('c'))), Action::Quit);

        let mut fresh = Console::new(header());
        assert_eq!(fresh.key(ctrl(KeyCode::Char('c'))), Action::None);
        fresh.key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        assert_eq!(
            fresh.key(ctrl(KeyCode::Char('c'))),
            Action::None,
            "a key between the two disarms the first"
        );
        assert_eq!(fresh.key(ctrl(KeyCode::Char('d'))), Action::Quit);
    }

    #[test]
    fn a_reconnect_redraws_the_fresh_snapshot_without_repeating_the_scrollback() {
        let events = vec![
            event(
                "user_prompt_submit",
                "first",
                json!({"text": "first", "source": "console"}),
            ),
            event("agent_message", "second", json!({"text": "second"})),
            event("agent_message", "third", json!({"text": "third"})),
        ];
        let mut console = Console::new(header());
        let mut terminal = terminal();

        console.snapshot(&events);
        console.commit(&mut terminal).unwrap();
        let committed = console.committed;

        // The stream drops and comes back on the same transcript.
        console.dropped();
        console.snapshot(&events);
        console.live();
        console.commit(&mut terminal).unwrap();
        terminal.draw(|frame| console.render(frame)).unwrap();

        let shown = screen(&terminal);
        assert_eq!(committed, 2, "everything but the open block was printed");
        assert_eq!(shown.matches("> first").count(), 1, "{shown}");
        assert_eq!(shown.matches("second").count(), 1, "{shown}");
        assert_eq!(shown.matches("third").count(), 1, "{shown}");
    }

    #[test]
    fn a_dropped_stream_says_reconnecting_on_the_status_line() {
        let mut console = Console::new(header());
        let mut terminal = terminal();

        console.dropped();
        terminal.draw(|frame| console.render(frame)).unwrap();

        assert!(
            screen(&terminal).contains("reconnecting"),
            "{}",
            screen(&terminal)
        );
    }

    /// A terminal that only records what was done to it, so that giving it
    /// back can be proven without taking the real one.
    #[derive(Clone, Default)]
    struct Recorder(Arc<AtomicUsize>);

    impl Terminals for Recorder {
        fn enter(&mut self) -> Result<()> {
            Ok(())
        }

        fn leave(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn the_terminal_is_given_back_on_the_normal_path_on_an_error_and_on_ctrl_c() {
        let normal = Recorder::default();
        {
            let _held = Held::take(normal.clone()).unwrap();
        }
        assert_eq!(normal.0.load(Ordering::SeqCst), 1, "the normal path");

        let failed = Recorder::default();
        let outcome: Result<()> = (|| {
            let _held = Held::take(failed.clone())?;
            anyhow::bail!("the daemon went away")
        })();
        assert!(outcome.is_err());
        assert_eq!(failed.0.load(Ordering::SeqCst), 1, "an error on the way");

        // Ctrl-C twice ends `drive`, and the terminal goes back after it.
        let interrupted = Recorder::default();
        let (client, server) = serve(Stub::new(Vec::new())).await;
        let ctrl_c = TermEvent::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        {
            let _held = Held::take(interrupted.clone()).unwrap();
            let mut terminal = terminal();
            let mut console = Console::new(header());
            let keys = stream::iter(vec![Ok(ctrl_c.clone()), Ok(ctrl_c)]).chain(stream::pending());
            drive(
                &client,
                "session",
                &mut terminal,
                Box::pin(keys),
                &mut console,
            )
            .await
            .unwrap();
        }
        server.abort();
        assert_eq!(interrupted.0.load(Ordering::SeqCst), 1, "Ctrl-C");
    }

    #[test]
    fn only_a_terminal_on_both_ends_gets_the_inline_console() {
        assert!(interactive(true, true));
        assert!(!interactive(false, true));
        assert!(!interactive(true, false));
        assert!(!interactive(false, false));
    }
}
