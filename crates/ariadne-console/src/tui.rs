//! The inline pane: a scrolling transcript with a status line and an input
//! box pinned under it.
//!
//! The shape is the one a person expects of a chat with an agent, and the one
//! the Codex CLI has: the transcript scrolls in the terminal's own buffer, and
//! a small viewport pinned under it holds a status line and the box being
//! typed into. Finished blocks leave the viewport with
//! [`Terminal::insert_before`], so what the agent said is still in the
//! scrollback after the console is closed — which is why the viewport is
//! [`Viewport::Inline`] and never the alternate screen.
//!
//! Nothing here knows where the events come from or where the input goes.
//! [`drive`] takes a stream of [`Frame`]s and a [`Sink`], and the host — the
//! CLI over the daemon's HTTP stream, the daemon in process — is what dials,
//! redials and posts. The host owns the process terminal too: raw mode is
//! taken and given back outside this crate.
//!
//! Three seams keep it testable without a terminal. [`Console`] is the whole
//! state and holds nothing of the terminal, so a `TestBackend` renders it.
//! [`drive`] takes the source, the sink and the key stream as arguments, so
//! an in-memory source and a scripted key stream drive it. And [`open`]
//! takes the backend as an argument, so one that answers no cursor query
//! proves the viewport still opens.

use std::collections::VecDeque;
use std::future::Future;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{Event as TermEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use futures_util::{Stream, StreamExt};
use ratatui::backend::{Backend, ClearType, WindowSize};
use ratatui::buffer::Cell;
use ratatui::layout::{Constraint, Layout, Position, Rect, Size};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Paragraph, Widget};
use ratatui::{Frame as Draw, Terminal, TerminalOptions, Viewport};
use tokio::time::{Instant, interval};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use ariadne_api::events::AgentEventDto;
use ariadne_api::sessions::SessionDto;

use crate::markdown;
use crate::transcript::{self, PermissionOption, Tool, TranscriptItem};

/// How tall the inline viewport is. The bottom [`PINNED`] rows are the status
/// line and the input box; the rest shows the block still being written, which
/// moves into the scrollback the moment it is finished.
const VIEWPORT: u16 = 12;
/// The status line, and the input box with its two border rows.
const PINNED: u16 = 4;
/// How many rows of typed text the input box grows to before it scrolls.
const INPUT_ROWS: usize = 4;
/// A thought and a tool's output are context, not the answer: they are folded
/// to this many lines with a count of what was left out. The output keeps its
/// last lines, which is where a command says how it went.
const FOLD: usize = 4;
/// A diff is read from the top, so it keeps its first lines: this many, with
/// a count of what was left out.
const DIFF_FOLD: usize = 24;
/// How often the spinner turns while a turn runs.
const TICK: Duration = Duration::from_millis(120);
const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

const USER: Style = Style::new().fg(Color::Cyan);
/// A prompt the daemon sent: the user's colour, dimmed — it was said to the
/// agent on the user's behalf, not by them.
const DAEMON: Style = Style::new().fg(Color::Cyan).add_modifier(Modifier::DIM);
const AGENT: Style = Style::new().fg(Color::Green);
const TOOL: Style = Style::new().fg(Color::Yellow);
const PLAN: Style = Style::new().fg(Color::Blue);
const ASK: Style = Style::new().fg(Color::Magenta);
const FAIL: Style = Style::new().fg(Color::Red).add_modifier(Modifier::BOLD);
const DIM: Style = Style::new().add_modifier(Modifier::DIM);
/// The lines of a diff: added, removed, the hunk header, and the file header.
const ADDED: Style = Style::new().fg(Color::Green);
const REMOVED: Style = Style::new().fg(Color::Red);
const HUNK: Style = Style::new().fg(Color::Cyan);
const FILE: Style = Style::new().add_modifier(Modifier::BOLD);

/// A ratatui backend whose failures `anyhow` can carry: the real terminal's
/// and the test one's alike.
pub trait Screen: Backend<Error: std::error::Error + Send + Sync + 'static> {}

impl<B> Screen for B
where
    B: Backend,
    B::Error: std::error::Error + Send + Sync + 'static,
{
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

/// Whether the console is reading its stream or waiting for it to be back.
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
    pub fn of(session: Option<&SessionDto>) -> Self {
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
    /// When the running turn began, as far as the console saw it: the status
    /// line counts up from here. `None` between turns.
    since: Option<Instant>,
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
            since: None,
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
        // Replaying the transcript replays every turn's start and end, and
        // the clock follows: a turn that was running before the stream
        // dropped counts from its own prompt again, and a turn that began
        // while the stream was down counts from its prompt, not the old one.
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
        // A daemon that predates `source` and `text` (021) names neither.
        // Its prompt event is the typed prompt's confirmation where the
        // whole it carries is the system prompt, a blank line and the typed
        // text — so it ends in the blank line and the text, and a daemon
        // prompt that merely ends in the same words does not — and a prompt
        // of the daemon's own otherwise. Left unconfirmed, the typed prompt
        // would wait for ever and hold everything after it out of the
        // scrollback.
        let source = event.payload.get("source").and_then(|s| s.as_str());
        let prompt = event.payload.get("prompt").and_then(|p| p.as_str());
        let confirmed = self
            .pending
            .front()
            .copied()
            .filter(|at| *at < self.items.len())
            .filter(|at| match (&self.items[*at], source) {
                (TranscriptItem::UserPrompt { .. }, Some("console")) => true,
                (TranscriptItem::UserPrompt { text: typed, .. }, None) => {
                    prompt.is_some_and(|whole| whole.ends_with(&format!("\n\n{typed}")))
                }
                _ => false,
            });
        if event.kind == "user_prompt_submit"
            && let Some(at) = confirmed
        {
            self.pending.pop_front();
            let mut item = TranscriptItem::from(event);
            // A confirmation with no `text` knows what was typed no better
            // than the console does, and the console has it.
            if event.payload.get("text").and_then(|t| t.as_str()).is_none()
                && let (
                    TranscriptItem::UserPrompt { text, .. },
                    TranscriptItem::UserPrompt { text: typed, .. },
                ) = (&mut item, &self.items[at])
            {
                text.clone_from(typed);
            }
            self.items[at] = item;
            return;
        }
        if event.kind == "permission_request" {
            self.picked = 0;
        }
        absorb(&mut self.items, event);
    }

    /// What the status line says the agent is doing.
    fn follow_turn(&mut self, event: &AgentEventDto) {
        let turn = match event.kind.as_str() {
            "stop" | "session_end" | "session.error" => Turn::Idle,
            "pre_tool_use" | "tool_call_update" | "post_tool_use" => {
                match TranscriptItem::from(event) {
                    TranscriptItem::ToolCall { tool, .. }
                        if !transcript::tool_is_terminal(tool.status.as_deref()) =>
                    {
                        Turn::Running(tool.name)
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
        self.set_turn(turn, instant_of(&event.created_at));
    }

    /// Move the turn on, starting the clock at `began` as it begins and
    /// stopping it as it ends. A turn already running keeps its start.
    fn set_turn(&mut self, turn: Turn, began: Instant) {
        match (self.turn.running(), turn.running()) {
            (false, true) => self.since = Some(began),
            (_, false) => self.since = None,
            (true, true) => {}
        }
        self.turn = turn;
    }

    /// The stream dropped: say so, until the next snapshot says it is back.
    pub fn dropped(&mut self) {
        self.link = Link::Reconnecting;
    }

    /// Something the console sent did not arrive. It belongs on the
    /// transcript, where the prompt it answers is.
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
        self.set_turn(Turn::Idle, Instant::now());
    }

    pub fn live(&mut self) {
        self.link = Link::Live;
    }

    pub fn tick(&mut self) {
        self.tick = self.tick.wrapping_add(1);
    }

    /// Take one frame of the stream. `true` once the session has ended,
    /// which is the loop's cue to leave.
    ///
    /// A snapshot is what every stream opens with and what a redial brings
    /// again, so it is also what says a dropped stream is back.
    fn frame(&mut self, frame: Frame) -> bool {
        match frame {
            Frame::Snapshot(events) => {
                self.live();
                self.snapshot(&events);
                events.iter().any(|event| event.kind == "session_end")
            }
            Frame::Event(event) => {
                self.apply(&event);
                event.kind == "session_end"
            }
            Frame::Dropped => {
                self.dropped();
                false
            }
        }
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
        self.set_turn(Turn::Thinking, Instant::now());
    }

    /// Text the terminal pasted as one: into the box at the cursor, line
    /// breaks and all. Nothing is sent until Enter.
    pub fn paste(&mut self, text: &str) {
        self.armed = false;
        self.input.paste(text);
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
    pub fn render(&self, frame: &mut Draw) {
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
        if let Some(since) = self.since {
            spans.push(Span::styled(format!(" {}", clock(since.elapsed())), DIM));
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

/// When an event happened, on the console's own clock: a turn already
/// running at attach counts from its prompt, not from the attach. An event
/// whose time cannot be read, or that a clock ahead of this one dated in the
/// future, counts from now.
fn instant_of(created_at: &str) -> Instant {
    let now = Instant::now();
    chrono::DateTime::parse_from_rfc3339(created_at)
        .ok()
        .and_then(|at| {
            (chrono::Utc::now() - at.with_timezone(&chrono::Utc))
                .to_std()
                .ok()
        })
        .and_then(|age| now.checked_sub(age))
        .unwrap_or(now)
}

/// How long the turn has run, in whole seconds: `12s`, or `1m 04s` past a
/// minute.
fn clock(elapsed: Duration) -> String {
    let seconds = elapsed.as_secs();
    if seconds < 60 {
        format!("{seconds}s")
    } else {
        format!("{}m {:02}s", seconds / 60, seconds % 60)
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
        TranscriptItem::UserPrompt { text, source, .. } if source.as_deref() == Some("daemon") => {
            daemon_prompt(text, width)
        }
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
        TranscriptItem::ToolCall { meta, tool } => call(tool, &meta.created_at, width),
        TranscriptItem::PermissionQuestion {
            question,
            tool,
            options,
            answer,
            ..
        } => permission(question, tool, options, answer.as_deref(), picked, width),
        TranscriptItem::SystemNote { text, .. } => prefixed(text, "  ", DIM, DIM, width, None),
        TranscriptItem::Error { text, .. } => prefixed(text, "✗ ", FAIL, FAIL, width, None),
        TranscriptItem::Raw { kind, .. } => {
            vec![Line::from(Span::styled(kind.clone(), DIM))]
        }
    }
}

/// A prompt the daemon itself sent — a briefing, a nudge, a message — under
/// its own marker and label, so it reads apart from what was typed.
fn daemon_prompt(text: &str, width: usize) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(Span::styled("» daemon", DAEMON))];
    for line in wrap(text, width.saturating_sub(2)) {
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(line, DAEMON),
        ]));
    }
    lines
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
    let mut wrapped = wrap(text, width.saturating_sub(marker.width()));
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
                Span::raw(" ".repeat(marker.width()))
            };
            Line::from(vec![lead, Span::styled(line, style)])
        })
        .collect()
}

/// A tool call, the way a coding agent's console reads one: what it did, on
/// what, and how it went. The head line, then the output folded to its last
/// lines and the file change as a diff.
fn call(tool: &Tool, started_at: &str, width: usize) -> Vec<Line<'static>> {
    let (glyph, style) = match tool.status.as_deref() {
        Some("completed") => ("✓", AGENT),
        Some("failed") => ("✗", FAIL),
        Some("in_progress") => ("●", TOOL),
        _ => ("○", TOOL),
    };
    let elapsed = tool
        .ended_at
        .as_deref()
        .and_then(|ended_at| elapsed(started_at, ended_at));
    let mut lines = vec![head(tool, Span::styled(glyph, style), elapsed, width)];
    if let Some(output) = &tool.output {
        lines.extend(folded(output, width, DIM));
    }
    if let Some(diff) = &tool.diff {
        lines.extend(folded_diff(diff, width));
    }
    lines
}

/// The head line of a call: a status mark, the kind's glyph, the subject —
/// what the call is about, from its input — and how long it took, once it
/// has ended.
fn head(tool: &Tool, mark: Span<'static>, elapsed: Option<String>, width: usize) -> Line<'static> {
    let lead = mark.content.width() + 1;
    let tail = elapsed.as_ref().map_or(0, |elapsed| elapsed.width() + 2);
    let room = width.saturating_sub(lead + 2 + tail).max(1);
    let mut spans = vec![
        mark,
        Span::raw(" "),
        Span::styled(format!("{} ", kind_glyph(tool.kind.as_deref())), TOOL),
        Span::raw(clip(&subject(tool), room)),
    ];
    if let Some(elapsed) = elapsed {
        spans.push(Span::styled(format!("  {elapsed}"), DIM));
    }
    Line::from(spans)
}

/// One glyph per ACP kind, so the eye tells a command from a read from an
/// edit before reading a word.
fn kind_glyph(kind: Option<&str>) -> &'static str {
    match kind {
        Some("execute") => "$",
        Some("read") => "≡",
        Some("edit") => "✎",
        Some("delete") => "⌫",
        Some("move") => "→",
        Some("search") => "⌕",
        Some("fetch") => "↓",
        Some("think") => "∴",
        Some("switch_mode") => "⇄",
        _ => "•",
    }
}

/// What the call is about, from the field of its input that names it: the
/// command, the path and line, the pattern and where it is looked for, the
/// URL. The title where the kind names nothing, and never the raw JSON.
/// Locations the head does not already name follow it.
fn subject(tool: &Tool) -> String {
    let input = &tool.input;
    let named =
        match tool.kind.as_deref() {
            Some("execute") => {
                command(input).map(|command| command.lines().next().unwrap_or_default().to_string())
            }
            Some("read" | "edit" | "delete" | "move") => place(tool),
            Some("search") => field(input, &["pattern", "query", "regex"]).map(|pattern| {
                match field(input, &["path", "file_path", "directory", "cwd"])
                    .or_else(|| tool.locations.first().map(|location| location.path.clone()))
                {
                    Some(path) => format!("{pattern} in {path}"),
                    None => pattern,
                }
            }),
            Some("fetch") => field(input, &["url"]),
            _ => None,
        };
    let mut head = named.unwrap_or_else(|| tool.name.clone());
    let elsewhere: Vec<String> = tool
        .locations
        .iter()
        .filter(|location| !head.contains(&location.path))
        .map(|location| match location.line {
            Some(line) => format!("{}:{line}", location.path),
            None => location.path.clone(),
        })
        .collect();
    if !elsewhere.is_empty() {
        head.push_str(&format!(" ({})", elsewhere.join(", ")));
    }
    head
}

/// The command an `execute` call runs: one string, or the argument list
/// some agents send it as.
fn command(input: &serde_json::Value) -> Option<String> {
    if let serde_json::Value::String(text) = input {
        return Some(text.clone());
    }
    match input.get("command")? {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Array(words) => Some(
            words
                .iter()
                .filter_map(serde_json::Value::as_str)
                .collect::<Vec<_>>()
                .join(" "),
        ),
        _ => None,
    }
}

/// The file a call is on, and the line where one is named: `path:line`.
fn place(tool: &Tool) -> Option<String> {
    let first = tool.locations.first();
    let path = field(&tool.input, &["path", "file_path", "filePath", "file"])
        .or_else(|| first.map(|location| location.path.clone()))?;
    let line = tool
        .input
        .get("line")
        .and_then(serde_json::Value::as_u64)
        .or_else(|| first.filter(|location| location.path == path)?.line);
    Some(match line {
        Some(line) => format!("{path}:{line}"),
        None => path,
    })
}

/// The first of `keys` the input carries as a string.
fn field(input: &serde_json::Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| input.get(key)?.as_str().map(str::to_string))
}

/// Cut a line to `room` columns, saying so. The cut falls between grapheme
/// clusters, each as wide as it draws, so an emoji of several characters is
/// kept or dropped whole.
fn clip(text: &str, room: usize) -> String {
    if text.width() <= room {
        return text.to_string();
    }
    let mut cut = String::new();
    let mut used = 0;
    for grapheme in text.graphemes(true) {
        let width = grapheme.width();
        if used + width > room.saturating_sub(1) {
            break;
        }
        used += width;
        cut.push_str(grapheme);
    }
    cut.push('…');
    cut
}

/// How long a call took, in the unit that fits it.
fn elapsed(started_at: &str, ended_at: &str) -> Option<String> {
    let started = chrono::DateTime::parse_from_rfc3339(started_at).ok()?;
    let ended = chrono::DateTime::parse_from_rfc3339(ended_at).ok()?;
    let millis = (ended - started).num_milliseconds().max(0);
    Some(if millis < 1_000 {
        format!("{millis}ms")
    } else if millis < 60_000 {
        format!("{:.1}s", millis as f64 / 1_000.0)
    } else {
        format!("{}m {:02}s", millis / 60_000, millis % 60_000 / 1_000)
    })
}

/// A tool's output: indented, and folded to the last few lines, which is
/// where a command says how it went. Trailing blank lines are not output.
fn folded(text: &str, width: usize, style: Style) -> Vec<Line<'static>> {
    let all = wrap(text.trim_end(), width.saturating_sub(4));
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

/// Text read from the top — a command, a diff — indented and folded to its
/// first lines, with a count of what was left out.
fn folded_top(lines: Vec<Line<'static>>) -> Vec<Line<'static>> {
    let hidden = lines.len().saturating_sub(DIFF_FOLD);
    if hidden == 0 {
        return lines;
    }
    let mut kept: Vec<_> = lines.into_iter().take(DIFF_FOLD).collect();
    kept.push(Line::from(Span::styled(
        format!("    … {hidden} more lines"),
        DIM,
    )));
    kept
}

/// A unified diff: the file it changes as a header — the old name to the new
/// where they differ — then each line coloured for what it is, folded past
/// the limit.
fn folded_diff(diff: &str, width: usize) -> Vec<Line<'static>> {
    let indent =
        |text: String, style: Style| Line::from(vec![Span::raw("    "), Span::styled(text, style)]);
    let room = width.saturating_sub(4);
    let mut lines = Vec::new();
    let (mut old, mut new) = (None, None);
    let mut in_hunk = false;
    for line in diff.trim_end().lines() {
        if !in_hunk {
            if let Some(path) = line.strip_prefix("--- ") {
                old = file_name(path);
                continue;
            }
            if let Some(path) = line.strip_prefix("+++ ") {
                new = file_name(path);
                continue;
            }
            if !line.starts_with("@@") {
                // What git writes above a hunk — `diff --git`, `index`, a
                // mode — names nothing the header does not.
                continue;
            }
            in_hunk = true;
            let header = match (&old, &new) {
                (Some(old), Some(new)) if old != new => Some(format!("{old} → {new}")),
                (_, Some(name)) | (Some(name), None) => Some(name.clone()),
                (None, None) => None,
            };
            if let Some(header) = header {
                lines.push(indent(clip(&header, room), FILE));
            }
        }
        let style = match line.chars().next() {
            Some('+') => ADDED,
            Some('-') => REMOVED,
            Some('@') => HUNK,
            _ => DIM,
        };
        lines.push(indent(clip(line, room), style));
    }
    folded_top(lines)
}

/// The path a diff's file header names, bare of git's `a/` and `b/`, and
/// nothing for the `/dev/null` of a file that did not exist.
fn file_name(header: &str) -> Option<String> {
    let path = header.split('\t').next().unwrap_or(header).trim();
    if path == "/dev/null" || path.is_empty() {
        return None;
    }
    Some(
        path.strip_prefix("a/")
            .or_else(|| path.strip_prefix("b/"))
            .unwrap_or(path)
            .to_string(),
    )
}

/// A permission question: the options as a picker while it is open, and as
/// the answer once it is given. The call it asks about is drawn under the
/// question — its head, and the command or the diff where it carries one —
/// so what is being allowed can be read before it is.
fn permission(
    question: &str,
    tool: &Tool,
    options: &[PermissionOption],
    answer: Option<&str>,
    picked: Option<usize>,
    width: usize,
) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(vec![
        Span::styled("? ", ASK),
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
        ));
    }
    if let Some(diff) = &tool.diff {
        lines.extend(folded_diff(diff, width));
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

/// Hard-wrap text to `width` columns, keeping the line breaks it already has.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut out = Vec::new();
    for source in text.split('\n') {
        let mut line = String::new();
        let mut used = 0usize;
        for word in source.split_inclusive(char::is_whitespace) {
            let length = word.width();
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
    /// readable while it is being typed. The cursor sits at the column the
    /// characters before it draw up to: the width of the string they make,
    /// which is more than their count where one is wide, and more than the
    /// sum of their own widths where a selector or a joiner makes an emoji
    /// of them — the same measure the drawing takes.
    fn view(&self, height: usize, width: usize) -> ((u16, u16), (u16, u16)) {
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

    fn newline(&mut self) {
        let tail = self.lines[self.row].split_off(self.column);
        self.lines.insert(self.row + 1, tail);
        self.row += 1;
        self.column = 0;
    }

    /// Put pasted text in at the cursor, its line breaks kept: a terminal
    /// pastes them as `\r`, `\r\n` or `\n`, and each is one new line.
    fn paste(&mut self, text: &str) {
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

    /// One key in the box. The line-editing keys are the shell's: Ctrl-A and
    /// Ctrl-E to the ends of the line, Ctrl-U and Ctrl-K deleting to them,
    /// Ctrl-W deleting the word before the cursor, and Alt with an arrow —
    /// or Alt-B and Alt-F, which is what a terminal that sends the readline
    /// sequences for Alt-Left and Alt-Right gives — moving by word.
    fn key(&mut self, key: KeyEvent) {
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

/// One frame of the console stream, whichever transport carries it.
#[derive(Debug, Clone)]
pub enum Frame {
    /// The transcript as it stands: what every stream opens with, and what
    /// a redial brings again.
    Snapshot(Vec<AgentEventDto>),
    /// One event, as it happened.
    Event(AgentEventDto),
    /// The stream dropped. The source is dialling it again, and the next
    /// snapshot is what says it is back.
    Dropped,
}

/// Where what the console sends goes: the session's console input, over
/// whichever transport the host has.
pub trait Sink {
    /// Post a typed prompt, or the id of the option a permission question
    /// was answered with. The error is what the transcript says about a post
    /// that did not happen, in words a person reads.
    fn send(&mut self, text: String) -> impl Future<Output = Result<(), String>> + Send;

    /// Cancel the running turn. A turn that ended between the key and the
    /// post has nothing to cancel, and that is not a failure of the console.
    fn cancel(&mut self) -> impl Future<Output = ()> + Send;
}

/// One turn of the loop: whichever of the things the console waits on
/// happened first.
enum Step {
    Frame(Frame),
    /// The source has nothing more to give.
    Ended,
    Key(TermEvent),
    Closed,
    Tick,
}

/// Run the console until the session ends, the source ends, or the user
/// leaves.
///
/// The source, the sink and the key stream are arguments rather than the
/// daemon and the process terminal, so that a test scripts all three; the
/// terminal is whatever backend is under it.
pub async fn drive<B, S, W, K>(
    terminal: &mut Terminal<B>,
    source: S,
    sink: &mut W,
    keys: K,
    console: &mut Console,
) -> Result<()>
where
    B: Screen,
    S: Stream<Item = Result<Frame>>,
    W: Sink,
    K: Stream<Item = std::io::Result<TermEvent>> + Unpin,
{
    let mut source = std::pin::pin!(source);
    let mut keys = keys;
    let mut ticker = interval(TICK);

    // The stream opens with the transcript snapshot. Take it before any key,
    // so the first thing drawn is the conversation as it stands and a key
    // pressed at once answers a question that is already on the screen. A
    // session that had already ended is over here too: the caller's close
    // puts the transcript in the scrollback, and nothing more will come.
    if let Some(frame) = source.next().await
        && console.frame(frame?)
    {
        return Ok(());
    }

    loop {
        console.commit(terminal)?;
        terminal.draw(|frame| console.render(frame))?;

        let step = tokio::select! {
            biased;
            frame = source.next() => match frame {
                Some(frame) => Step::Frame(frame?),
                None => Step::Ended,
            },
            key = keys.next() => match key {
                Some(Ok(event)) => Step::Key(event),
                Some(Err(_)) | None => Step::Closed,
            },
            _ = ticker.tick() => Step::Tick,
        };

        match step {
            Step::Frame(frame) => {
                if console.frame(frame) {
                    return Ok(());
                }
            }
            Step::Key(TermEvent::Key(key)) => match console.key(key) {
                Action::None => {}
                Action::Quit => return Ok(()),
                Action::Send(text) => {
                    // A refused or undelivered prompt is said on the
                    // transcript rather than thrown: a console that closed
                    // itself on one bad post would take a half-typed
                    // conversation with it.
                    if let Err(reason) = sink.send(text).await {
                        console.failed(&reason);
                    }
                }
                Action::Cancel => sink.cancel().await,
            },
            Step::Key(TermEvent::Paste(text)) => console.paste(&text),
            // The loop redraws on its way round; the resize is what makes
            // the viewport the new size before it does.
            Step::Key(TermEvent::Resize(width, height)) => {
                terminal.resize(Rect::new(0, 0, width, height))?;
            }
            Step::Key(_) => {}
            Step::Ended | Step::Closed => return Ok(()),
            Step::Tick => console.tick(),
        }
    }
}

/// Open the inline viewport, from the cursor where the terminal says where
/// that is, and from the bottom row where it does not.
///
/// [`Viewport::Inline`] asks the terminal for the cursor position, and a
/// terminal that never answers — a pseudo-terminal with nothing behind it,
/// for one — costs crossterm's timeout and the error "The cursor position
/// could not be read within a normal duration". The console opens all the
/// same: the cursor is moved to the bottom row, which needs no answer, and
/// the viewport is opened from there on a backend that answers every later
/// query itself. It is the one inline viewport on both paths, never the
/// alternate screen, so the scrollback keeps the transcript either way.
///
/// `fresh` makes a backend, and makes another for the second attempt: the
/// one the failed attempt took cannot be had back.
pub fn open<B: Screen>(fresh: impl Fn() -> B) -> Result<Terminal<Anchored<B>>> {
    let inline = |backend| {
        Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Inline(VIEWPORT),
            },
        )
    };
    if let Ok(terminal) = inline(Anchored::asking(fresh())) {
        return Ok(terminal);
    }
    let mut backend = fresh();
    let bottom = Position {
        x: 0,
        y: backend.size()?.height.saturating_sub(1),
    };
    backend.set_cursor_position(bottom)?;
    Ok(inline(Anchored::at(backend, bottom))?)
}

/// A backend whose cursor position can be known without asking the terminal.
///
/// ratatui asks where the cursor is when it opens an inline viewport, when it
/// clears one and when the terminal is resized. Anchored to a position, every
/// one of those is answered from here — where the last
/// [`Backend::set_cursor_position`] put it, which is where ratatui's own
/// drawing leaves it — rather than sent to a terminal that has already failed
/// to answer once. Asking, it is the backend it wraps and nothing more.
pub struct Anchored<B> {
    inner: B,
    known: Option<Position>,
}

impl<B> Anchored<B> {
    fn asking(inner: B) -> Self {
        Self { inner, known: None }
    }

    fn at(inner: B, at: Position) -> Self {
        Self {
            inner,
            known: Some(at),
        }
    }
}

impl<B: Backend> Backend for Anchored<B> {
    type Error = B::Error;

    fn draw<'a, I>(&mut self, content: I) -> Result<(), B::Error>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        self.inner.draw(content)
    }

    fn append_lines(&mut self, n: u16) -> Result<(), B::Error> {
        self.inner.append_lines(n)
    }

    fn hide_cursor(&mut self) -> Result<(), B::Error> {
        self.inner.hide_cursor()
    }

    fn show_cursor(&mut self) -> Result<(), B::Error> {
        self.inner.show_cursor()
    }

    fn get_cursor_position(&mut self) -> Result<Position, B::Error> {
        match self.known {
            Some(at) => Ok(at),
            None => self.inner.get_cursor_position(),
        }
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> Result<(), B::Error> {
        let at = position.into();
        self.inner.set_cursor_position(at)?;
        if self.known.is_some() {
            self.known = Some(at);
        }
        Ok(())
    }

    fn clear(&mut self) -> Result<(), B::Error> {
        self.inner.clear()
    }

    fn clear_region(&mut self, clear_type: ClearType) -> Result<(), B::Error> {
        self.inner.clear_region(clear_type)
    }

    fn size(&self) -> Result<Size, B::Error> {
        self.inner.size()
    }

    fn window_size(&mut self) -> Result<WindowSize, B::Error> {
        self.inner.window_size()
    }

    fn flush(&mut self) -> Result<(), B::Error> {
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;

    use futures_util::stream;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use serde_json::json;
    use tokio::sync::mpsc;

    use super::*;

    /// A daemon console, as far as the pane can tell: a snapshot, deltas of
    /// its own, and deltas it answers a post with — which is what makes a
    /// test that types deterministic, since nothing arrives that the typing
    /// did not cause. In memory: the sink it is, and the source it feeds.
    struct Stub {
        snapshot: Vec<AgentEventDto>,
        deltas: Vec<AgentEventDto>,
        on_input: Vec<AgentEventDto>,
        on_cancel: Vec<AgentEventDto>,
        /// What the stream says after its deltas: a drop, a fresh snapshot.
        after: Vec<Frame>,
        outbox: mpsc::UnboundedSender<Result<Frame>>,
        inbox: Option<mpsc::UnboundedReceiver<Result<Frame>>>,
        prompts: Vec<String>,
        cancels: usize,
        /// A session that will take no more input, which is the `409` a
        /// finished one gives.
        refuses: bool,
    }

    impl Stub {
        fn new(snapshot: Vec<AgentEventDto>) -> Self {
            let (outbox, inbox) = mpsc::unbounded_channel();
            Self {
                snapshot,
                deltas: Vec::new(),
                on_input: Vec::new(),
                on_cancel: Vec::new(),
                after: Vec::new(),
                outbox,
                inbox: Some(inbox),
                prompts: Vec::new(),
                cancels: 0,
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

        fn after(mut self, frames: Vec<Frame>) -> Self {
            self.after = frames;
            self
        }

        /// The stream as the loop reads it: the snapshot, the deltas, then
        /// whatever a post causes. It never ends while the stub is alive, so
        /// only the daemon or a Ctrl-C stops the loop — as in the real one.
        fn source(&mut self) -> impl Stream<Item = Result<Frame>> + use<> {
            let _ = self
                .outbox
                .send(Ok(Frame::Snapshot(std::mem::take(&mut self.snapshot))));
            for delta in std::mem::take(&mut self.deltas) {
                let _ = self.outbox.send(Ok(Frame::Event(delta)));
            }
            for frame in std::mem::take(&mut self.after) {
                let _ = self.outbox.send(Ok(frame));
            }
            let inbox = self.inbox.take().expect("one source per stub");
            stream::unfold(inbox, |mut inbox| async move {
                inbox.recv().await.map(|frame| (frame, inbox))
            })
        }
    }

    impl Sink for Stub {
        async fn send(&mut self, text: String) -> Result<(), String> {
            self.prompts.push(text);
            for event in &self.on_input {
                let _ = self.outbox.send(Ok(Frame::Event(event.clone())));
            }
            match self.refuses {
                true => Err("409 Conflict: the session takes no more input".into()),
                false => Ok(()),
            }
        }

        async fn cancel(&mut self) {
            self.cancels += 1;
            for event in &self.on_cancel {
                let _ = self.outbox.send(Ok(Frame::Event(event.clone())));
            }
        }
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
        rows(terminal.backend().buffer())
    }

    /// The cell after a wide character is the blank the buffer leaves under
    /// its second column, and is not read as a space.
    fn rows(buffer: &Buffer) -> String {
        (0..buffer.area.height)
            .map(|y| {
                let mut row = String::new();
                let mut x = 0;
                while x < buffer.area.width {
                    let symbol = buffer[(x, y)].symbol();
                    row.push_str(symbol);
                    x += u16::try_from(symbol.width().max(1)).unwrap();
                }
                row.trim_end().to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn key(code: KeyCode) -> TermEvent {
        TermEvent::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn ctrl(character: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(character), KeyModifiers::CONTROL)
    }

    fn alt(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::ALT)
    }

    fn paste(text: &str) -> TermEvent {
        TermEvent::Paste(text.into())
    }

    /// Type `text` into the console, one key per character.
    fn type_into(console: &mut Console, text: &str) {
        for character in text.chars() {
            console.key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
    }

    fn enter(console: &mut Console) -> Action {
        console.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
    }

    fn left(console: &mut Console, times: usize) {
        for _ in 0..times {
            console.key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        }
    }

    fn typed(text: &str) -> Vec<TermEvent> {
        text.chars().map(|c| key(KeyCode::Char(c))).collect()
    }

    /// Drive the console against the stub until the session ends, then read
    /// the screen. No server anywhere: the stub is the source and the sink.
    async fn console(mut stub: Stub, keys: Vec<TermEvent>) -> (String, Vec<String>, usize) {
        let source = stub.source();
        let mut terminal = terminal();
        let mut console = Console::new(header());
        let keys = stream::iter(keys.into_iter().map(Ok)).chain(stream::pending());

        drive(
            &mut terminal,
            source,
            &mut stub,
            Box::pin(keys),
            &mut console,
        )
        .await
        .unwrap();

        (screen(&terminal), stub.prompts, stub.cancels)
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
                        "acp": {"toolCallId": "call-1", "kind": "execute", "status": "completed",
                                "rawOutput": {"stdout": "one\ntwo\nthree\nfour\nfive\nsix\n\n\n"}}
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
        assert!(
            shown.contains("✓ $ cargo nextest run"),
            "a completed call is ticked, and its head is its command: {shown}"
        );
        assert!(
            shown.contains("… 2 more lines") && shown.contains("    six\n\n"),
            "the output is folded to its last lines, the blank ones trimmed: {shown}"
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
            shown.find("Let me run the tests.") < shown.find("✓ • Bash")
                && shown.find("✓ • Bash") < shown.find("The tests pass."),
            "in the order the turn happened: {shown}"
        );
    }

    /// One tool call as the daemon records it (021): the ACP call under
    /// `acp`, with its kind and its raw input.
    fn called(id: &str, kind: &str, status: &str, input: serde_json::Value) -> AgentEventDto {
        event(
            "post_tool_use",
            id,
            json!({
                "tool_name": kind,
                "tool_input": input,
                "acp": {"toolCallId": id, "title": kind, "kind": kind, "status": status,
                        "rawInput": input}
            }),
        )
    }

    #[tokio::test]
    async fn each_kind_of_call_draws_its_glyph_and_what_it_is_about() {
        let (shown, _, _) = console(
            Stub::new(vec![
                called(
                    "run",
                    "execute",
                    "completed",
                    json!({"command": "cargo nextest run"}),
                ),
                called(
                    "read",
                    "read",
                    "completed",
                    json!({"file_path": "src/main.rs", "line": 12}),
                ),
                called(
                    "grep",
                    "search",
                    "completed",
                    json!({"pattern": "fn main", "path": "src"}),
                ),
                called(
                    "get",
                    "fetch",
                    "failed",
                    json!({"url": "https://example.com/spec"}),
                ),
            ])
            .deltas(vec![ended()]),
            Vec::new(),
        )
        .await;

        assert!(shown.contains("✓ $ cargo nextest run"), "{shown}");
        assert!(shown.contains("✓ ≡ src/main.rs:12"), "{shown}");
        assert!(shown.contains("✓ ⌕ fn main in src"), "{shown}");
        assert!(shown.contains("✗ ↓ https://example.com/spec"), "{shown}");
        assert!(
            !shown.contains('{'),
            "no raw JSON where a field names the subject: {shown}"
        );
    }

    /// A live `tool_call_update` carries the daemon's merged call under
    /// `acp` and no `tool_name` (021).
    fn updated(id: &str, status: &str, output: &str) -> AgentEventDto {
        event(
            "tool_call_update",
            &format!("{id}-{output}"),
            json!({
                "tool_call_id": id,
                "acp": {"toolCallId": id, "title": "Bash", "kind": "execute", "status": status,
                        "rawInput": {"command": "cargo build"}, "rawOutput": output}
            }),
        )
    }

    #[tokio::test]
    async fn updates_of_one_call_draw_one_block() {
        let (shown, _, _) = console(
            Stub::new(vec![event(
                "pre_tool_use",
                "build",
                json!({"tool_name": "Bash",
                       "acp": {"toolCallId": "build", "kind": "execute", "status": "pending",
                               "rawInput": {"command": "cargo build"}}}),
            )])
            .deltas(vec![
                updated("build", "in_progress", "Compiling a"),
                updated("build", "in_progress", "Compiling b"),
                updated("build", "in_progress", "Compiling c"),
                ended(),
            ]),
            Vec::new(),
        )
        .await;

        assert_eq!(
            shown.matches("$ cargo build").count(),
            1,
            "one block, however many updates: {shown}"
        );
        assert!(
            shown.contains("Compiling c") && !shown.contains("Compiling a"),
            "showing the latest update: {shown}"
        );
    }

    fn event_at(kind: &str, summary: &str, payload: serde_json::Value, at: &str) -> AgentEventDto {
        let mut event = event(kind, summary, payload);
        event.created_at = at.into();
        event
    }

    #[tokio::test]
    async fn a_completed_call_draws_its_duration() {
        let (shown, _, _) = console(
            Stub::new(vec![
                event_at(
                    "pre_tool_use",
                    "test",
                    json!({"acp": {"toolCallId": "test", "kind": "execute", "status": "pending",
                                   "rawInput": {"command": "cargo test"}}}),
                    "2026-09-12T10:00:00.000Z",
                ),
                event_at(
                    "post_tool_use",
                    "test",
                    json!({"acp": {"toolCallId": "test", "kind": "execute", "status": "completed",
                                   "rawInput": {"command": "cargo test"}, "rawOutput": "ok"}}),
                    "2026-09-12T10:00:01.500Z",
                ),
            ])
            .deltas(vec![ended()]),
            Vec::new(),
        )
        .await;

        assert!(shown.contains("✓ $ cargo test  1.5s"), "{shown}");
    }

    /// The style of the first cell of the first line that, past its indent,
    /// starts with `text`.
    fn style_of(buffer: &Buffer, text: &str) -> Style {
        for y in 0..buffer.area.height {
            let row: String = (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect();
            if row.trim_start().starts_with(text) {
                let x = u16::try_from(row.len() - row.trim_start().len()).unwrap();
                return buffer[(x, y)].style();
            }
        }
        panic!("{text:?} is not on the screen:\n{}", rows(buffer));
    }

    #[test]
    fn a_diff_draws_its_file_header_its_lines_coloured_and_a_fold_count() {
        let mut console = Console::new(header());
        let mut terminal = terminal();
        let body: String = (1..=30).map(|n| format!("+line {n}\n")).collect();
        let patch = format!("--- /dev/null\n+++ b/src/new.rs\n@@ -0,0 +1,30 @@\n{body}");
        console.apply(&event(
            "post_tool_use",
            "write",
            json!({"acp": {"toolCallId": "write", "kind": "edit", "status": "completed",
                           "rawInput": {"file_path": "src/new.rs"},
                           "content": [{"type": "diff", "patch": {"text": patch}}]}}),
        ));
        console.apply(&event("agent_message", "done", json!({"text": "Written."})));

        console.commit(&mut terminal).unwrap();
        let shown = screen(&terminal);
        let buffer = terminal.backend().buffer();

        assert!(shown.contains("✓ ✎ src/new.rs"), "{shown}");
        assert!(
            row_of(&shown, "    src/new.rs") < row_of(&shown, "@@ -0,0 +1,30 @@"),
            "the file header is above the hunk: {shown}"
        );
        assert_eq!(style_of(buffer, "src/new.rs").add_modifier, Modifier::BOLD);
        assert_eq!(style_of(buffer, "@@ -0,0").fg, Some(Color::Cyan));
        assert_eq!(style_of(buffer, "+line 1").fg, Some(Color::Green));
        // The header, the hunk line and thirty added lines.
        assert!(
            shown.contains(&format!("… {} more lines", 32 - DIFF_FOLD)),
            "the tail past the limit is counted: {shown}"
        );
        assert!(!shown.contains("+line 30"), "{shown}");
    }

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

    /// Four words of three CJK characters, each six columns wide, on a
    /// terminal sixteen columns wide: two words fill a line of the block,
    /// where a count of characters would fit three.
    #[test]
    fn a_line_of_wide_characters_wraps_at_the_display_width() {
        let mut console = Console::new(header());
        let mut terminal = Terminal::with_options(
            TestBackend::new(16, 40),
            TerminalOptions {
                viewport: Viewport::Inline(VIEWPORT),
            },
        )
        .unwrap();
        console.apply(&event(
            "user_prompt_submit",
            "wide",
            json!({"text": "日本語 日本語 日本語 日本語", "source": "console"}),
        ));

        terminal.draw(|frame| console.render(frame)).unwrap();

        let shown = screen(&terminal);
        assert!(
            shown.contains("> 日本語 日本語\n  日本語 日本語"),
            "{shown}"
        );
    }

    /// A woman scientist — three characters joined into one two-column
    /// emoji — heads a command too long for a pane sixteen columns wide.
    /// The head has twelve columns for it: eleven of text and the mark. A
    /// cut that counted characters would take two columns too many for the
    /// emoji and stop two characters short.
    #[test]
    fn a_head_is_cut_between_whole_emoji_sequences() {
        let mut console = Console::new(header());
        let mut terminal = Terminal::with_options(
            TestBackend::new(16, 40),
            TerminalOptions {
                viewport: Viewport::Inline(VIEWPORT),
            },
        )
        .unwrap();
        console.apply(&called(
            "run",
            "execute",
            "completed",
            json!({"command": "\u{1f469}\u{200d}\u{1f52c} lab notes now"}),
        ));

        terminal.draw(|frame| console.render(frame)).unwrap();

        let shown = screen(&terminal);
        assert!(
            shown.contains("✓ $ \u{1f469}\u{200d}\u{1f52c} lab note…"),
            "{shown}"
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

    /// An event dated this many seconds ago on the wall clock, as the
    /// daemon dates the ones it stores. Zero is an event happening now.
    fn ago(seconds: i64, kind: &str, summary: &str, payload: serde_json::Value) -> AgentEventDto {
        let at = chrono::Utc::now() - chrono::Duration::seconds(seconds);
        event_at(kind, summary, payload, &at.to_rfc3339())
    }

    #[tokio::test(start_paused = true)]
    async fn an_attach_during_a_turn_counts_from_the_prompt_that_began_it() {
        let mut console = Console::new(header());
        let mut terminal = terminal();

        console.snapshot(&[ago(
            45,
            "user_prompt_submit",
            "first",
            json!({"text": "first", "source": "console"}),
        )]);
        terminal.draw(|frame| console.render(frame)).unwrap();

        assert!(
            screen(&terminal).contains("thinking 45s"),
            "the turn began 45 seconds before the attach: {}",
            screen(&terminal)
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_reconnect_keeps_the_clock_of_the_turn_still_running() {
        let mut console = Console::new(header());
        let mut terminal = terminal();
        let events = vec![
            ago(
                40,
                "user_prompt_submit",
                "first",
                json!({"text": "first", "source": "console"}),
            ),
            ago(35, "stop", "stopped", json!({"stop_reason": "end_turn"})),
            ago(
                30,
                "user_prompt_submit",
                "second",
                json!({"text": "second", "source": "console"}),
            ),
        ];
        console.snapshot(&events);

        console.dropped();
        console.snapshot(&events);
        console.live();
        terminal.draw(|frame| console.render(frame)).unwrap();

        assert!(
            screen(&terminal).contains("thinking 30s"),
            "the replay did not restart the clock: {}",
            screen(&terminal)
        );
    }

    /// The stream is down while one turn ends and the next begins: the
    /// replay brings both, and the clock is the new turn's.
    #[tokio::test(start_paused = true)]
    async fn a_turn_begun_while_the_stream_was_down_counts_from_its_own_prompt() {
        let mut console = Console::new(header());
        let mut terminal = terminal();
        let first = ago(
            30,
            "user_prompt_submit",
            "first",
            json!({"text": "first", "source": "console"}),
        );
        console.snapshot(std::slice::from_ref(&first));
        terminal.draw(|frame| console.render(frame)).unwrap();
        assert!(
            screen(&terminal).contains("thinking 30s"),
            "{}",
            screen(&terminal)
        );

        console.dropped();
        console.snapshot(&[
            first,
            ago(20, "stop", "stopped", json!({"stop_reason": "end_turn"})),
            ago(
                5,
                "user_prompt_submit",
                "second",
                json!({"text": "second", "source": "console"}),
            ),
        ]);
        console.live();
        terminal.draw(|frame| console.render(frame)).unwrap();

        assert!(
            screen(&terminal).contains("thinking 5s"),
            "the clock is the new turn's, not the old one's: {}",
            screen(&terminal)
        );
    }

    #[tokio::test(start_paused = true)]
    async fn the_status_line_counts_the_running_turn_and_stops_between_turns() {
        let mut console = Console::new(header());
        let mut terminal = terminal();
        let prompt = |text: &str| {
            ago(
                0,
                "user_prompt_submit",
                text,
                json!({"text": text, "source": "console"}),
            )
        };
        let stopped = ago(0, "stop", "stopped", json!({"stop_reason": "end_turn"}));
        let status_line = |terminal: &Terminal<TestBackend>| {
            screen(terminal)
                .lines()
                .find(|line| line.starts_with("author"))
                .unwrap()
                .to_string()
        };

        console.apply(&prompt("first"));
        tokio::time::advance(Duration::from_secs(64)).await;
        terminal.draw(|frame| console.render(frame)).unwrap();
        assert!(
            status_line(&terminal).contains("thinking 1m 04s"),
            "{}",
            screen(&terminal)
        );

        console.apply(&stopped);
        terminal.draw(|frame| console.render(frame)).unwrap();
        assert!(
            !status_line(&terminal).contains("1m 04s"),
            "the clock is off between turns: {}",
            screen(&terminal)
        );

        console.apply(&prompt("second"));
        tokio::time::advance(Duration::from_secs(5)).await;
        terminal.draw(|frame| console.render(frame)).unwrap();
        assert!(
            status_line(&terminal).contains("thinking 5s"),
            "the next turn starts from zero: {}",
            screen(&terminal)
        );
    }

    #[tokio::test]
    async fn a_resize_redraws_the_viewport_at_the_new_size() {
        let mut stub = Stub::new(Vec::new());
        let source = stub.source();
        let mut terminal = terminal();
        let mut console = Console::new(header());
        terminal.backend_mut().resize(40, 20);
        let ctrl_c = TermEvent::Key(ctrl('c'));
        let keys = stream::iter(vec![
            Ok(TermEvent::Resize(40, 20)),
            Ok(ctrl_c.clone()),
            Ok(ctrl_c),
        ])
        .chain(stream::pending());

        drive(
            &mut terminal,
            source,
            &mut stub,
            Box::pin(keys),
            &mut console,
        )
        .await
        .unwrap();

        let shown = screen(&terminal);
        assert!(
            shown
                .lines()
                .any(|line| line.width() == 40 && line.ends_with('┐')),
            "the box's border reaches the new right edge: {shown}"
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

    /// The seat's system prompt, as the daemon sends it ahead of every
    /// prompt (021): the one thing the console must never draw.
    const SYSTEM_PROMPT: &str = "You plan one Ariadne goal into tasks, with the user.";

    #[tokio::test]
    async fn a_prompt_draws_its_text_alone_and_never_the_system_prompt() {
        let (shown, _, _) = console(
            Stub::new(vec![event(
                "user_prompt_submit",
                SYSTEM_PROMPT,
                json!({
                    "prompt": format!("{SYSTEM_PROMPT}\n\nRun the tests"),
                    "text": "Run the tests",
                    "source": "console",
                }),
            )])
            .deltas(vec![ended()]),
            Vec::new(),
        )
        .await;

        assert!(shown.contains("> Run the tests"), "{shown}");
        assert!(
            !shown.contains("You plan"),
            "the system prompt stays off the screen: {shown}"
        );
    }

    /// A daemon older than 021 rule 4 stored the whole prompt and no `text`.
    /// The typed text cannot be told from the system prompt in it, so the
    /// console says so rather than draw the whole.
    #[tokio::test]
    async fn a_prompt_carrying_only_the_whole_prompt_draws_no_system_prompt() {
        let (shown, _, _) = console(
            Stub::new(vec![event(
                "user_prompt_submit",
                SYSTEM_PROMPT,
                json!({
                    "prompt": format!("{SYSTEM_PROMPT}\n\nRun the tests"),
                    "source": "console",
                }),
            )])
            .deltas(vec![ended()]),
            Vec::new(),
        )
        .await;

        assert!(
            !shown.contains("You plan"),
            "the system prompt stays off the screen: {shown}"
        );
        assert!(
            shown.contains("> (prompt text not recorded)"),
            "the prompt is on the transcript, said to be unrecorded: {shown}"
        );
    }

    /// A daemon older than 021 rule 4 confirms with the whole `prompt` and
    /// neither `text` nor `source`.
    #[tokio::test]
    async fn a_typed_line_confirmed_without_its_text_keeps_what_was_typed() {
        let mut keys = typed("hi");
        keys.push(key(KeyCode::Enter));
        let (shown, prompts, _) = console(
            Stub::new(Vec::new()).on_input(vec![
                event(
                    "user_prompt_submit",
                    SYSTEM_PROMPT,
                    json!({"prompt": format!("{SYSTEM_PROMPT}\n\nhi")}),
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
            "the typed text stays, once: {shown}"
        );
        assert!(
            !shown.contains("(prompt text not recorded)"),
            "the confirmation took the pending prompt's place: {shown}"
        );
    }

    /// An older daemon's own prompt — a nudge — names no `source` either. It
    /// reaches the console ahead of the typed prompt's confirmation, which
    /// the daemon queued behind the running turn, and it happens to end in
    /// the typed words: only the blank line ahead of them tells the
    /// confirmation from it.
    #[test]
    fn an_older_daemons_own_prompt_does_not_take_a_pending_prompts_place() {
        let mut console = Console::new(header());
        let mut terminal = terminal();
        for character in "tests".chars() {
            console.key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        console.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        console.apply(&event(
            "user_prompt_submit",
            SYSTEM_PROMPT,
            json!({"prompt": format!("{SYSTEM_PROMPT}\n\nRun the tests")}),
        ));
        terminal.draw(|frame| console.render(frame)).unwrap();
        let shown = screen(&terminal);
        assert_eq!(
            shown.matches("> tests").count(),
            1,
            "the typed prompt is still pending: {shown}"
        );
        assert_eq!(
            shown.matches("(prompt text not recorded)").count(),
            1,
            "the nudge is a prompt of its own, its text unknown: {shown}"
        );

        console.apply(&event(
            "user_prompt_submit",
            SYSTEM_PROMPT,
            json!({"prompt": format!("{SYSTEM_PROMPT}\n\ntests")}),
        ));
        terminal.draw(|frame| console.render(frame)).unwrap();
        let shown = screen(&terminal);
        assert_eq!(
            shown.matches("> tests").count(),
            1,
            "the confirmation took the typed prompt's place: {shown}"
        );
        assert_eq!(
            shown.matches("(prompt text not recorded)").count(),
            1,
            "and added no placeholder: {shown}"
        );
    }

    #[tokio::test]
    async fn a_daemon_sourced_prompt_draws_under_its_own_marker() {
        let (shown, _, _) = console(
            Stub::new(vec![
                event(
                    "user_prompt_submit",
                    "Land the change.",
                    json!({"text": "Land the change.", "source": "daemon"}),
                ),
                event(
                    "user_prompt_submit",
                    "Go ahead.",
                    json!({"text": "Go ahead.", "source": "console"}),
                ),
            ])
            .deltas(vec![ended()]),
            Vec::new(),
        )
        .await;

        assert!(
            shown.contains("» daemon\n  Land the change."),
            "the briefing is labelled and drawn under its own marker: {shown}"
        );
        assert!(
            !shown.contains("> Land the change."),
            "and not as typed input: {shown}"
        );
        assert!(shown.contains("> Go ahead."), "{shown}");
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

    /// Both runs leave by Ctrl-C rather than wait for the daemon, so a
    /// console that sends nothing fails the assertion instead of hanging.
    #[tokio::test]
    async fn a_pasted_text_is_one_prompt_with_its_line_breaks_and_sends_nothing_until_enter() {
        let ctrl_c = TermEvent::Key(ctrl('c'));
        let (_, prompts, _) = console(
            Stub::new(Vec::new()),
            vec![paste("one\n\ntwo"), ctrl_c.clone(), ctrl_c.clone()],
        )
        .await;
        assert!(
            prompts.is_empty(),
            "the paste alone sent nothing: {prompts:?}"
        );

        let (_, prompts, _) = console(
            Stub::new(Vec::new()),
            vec![
                paste("one\n\ntwo"),
                key(KeyCode::Enter),
                ctrl_c.clone(),
                ctrl_c,
            ],
        )
        .await;
        assert_eq!(
            prompts,
            ["one\n\ntwo"],
            "Enter sent the paste as one prompt, its blank line kept"
        );
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

    /// A screen that never says where its cursor is: the terminal that
    /// answers no query, as ratatui sees it.
    struct Mute(TestBackend);

    impl Mute {
        fn new() -> Self {
            Self(TestBackend::new(72, 40))
        }
    }

    /// What the test backend cannot fail at, as the error the mute one can.
    fn sure<T>(result: Result<T, Infallible>) -> std::io::Result<T> {
        match result {
            Ok(value) => Ok(value),
            Err(never) => match never {},
        }
    }

    impl Backend for Mute {
        type Error = std::io::Error;

        fn draw<'a, I>(&mut self, content: I) -> std::io::Result<()>
        where
            I: Iterator<Item = (u16, u16, &'a Cell)>,
        {
            sure(self.0.draw(content))
        }

        fn append_lines(&mut self, n: u16) -> std::io::Result<()> {
            sure(self.0.append_lines(n))
        }

        fn hide_cursor(&mut self) -> std::io::Result<()> {
            sure(self.0.hide_cursor())
        }

        fn show_cursor(&mut self) -> std::io::Result<()> {
            sure(self.0.show_cursor())
        }

        fn get_cursor_position(&mut self) -> std::io::Result<Position> {
            Err(std::io::Error::other(
                "The cursor position could not be read within a normal duration",
            ))
        }

        fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> std::io::Result<()> {
            sure(self.0.set_cursor_position(position))
        }

        fn clear(&mut self) -> std::io::Result<()> {
            sure(self.0.clear())
        }

        fn clear_region(&mut self, clear_type: ClearType) -> std::io::Result<()> {
            sure(self.0.clear_region(clear_type))
        }

        fn size(&self) -> std::io::Result<Size> {
            sure(self.0.size())
        }

        fn window_size(&mut self) -> std::io::Result<WindowSize> {
            sure(self.0.window_size())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            sure(self.0.flush())
        }
    }

    /// The row a line of text is on, in a screen `rows` read.
    fn row_of(shown: &str, text: &str) -> Option<usize> {
        shown.lines().position(|line| line.contains(text))
    }

    #[test]
    fn the_console_opens_at_the_bottom_when_the_cursor_position_cannot_be_read() {
        let mut terminal = super::open(Mute::new).unwrap();
        let mut console = Console::new(header());

        terminal.draw(|frame| console.render(frame)).unwrap();

        let shown = rows(terminal.backend().inner.0.buffer());
        assert_eq!(
            row_of(&shown, "author claude:opus · running"),
            Some(40 - usize::from(PINNED)),
            "the status line is drawn, with the pane on the bottom rows: {shown}"
        );
        assert!(
            console.close(&mut terminal).is_ok(),
            "and closing, which asks where the cursor is again, still works"
        );
    }

    #[test]
    fn a_finished_block_reaches_the_scrollback_when_the_cursor_position_cannot_be_read() {
        let mut terminal = super::open(Mute::new).unwrap();
        let mut console = Console::new(header());
        console.snapshot(&[
            event(
                "user_prompt_submit",
                "first",
                json!({"text": "first", "source": "console"}),
            ),
            event("agent_message", "second", json!({"text": "second"})),
        ]);

        console.commit(&mut terminal).unwrap();
        terminal.draw(|frame| console.render(frame)).unwrap();

        let shown = rows(terminal.backend().inner.0.buffer());
        let prompt = row_of(&shown, "> first").expect(&shown);
        assert!(
            prompt < 40 - usize::from(VIEWPORT),
            "the finished prompt is above the pane, in the scrollback: {shown}"
        );
    }

    /// The fallback viewport — the terminal that answers no cursor query —
    /// runs the loop and closes like the inline one.
    #[tokio::test]
    async fn the_console_runs_and_closes_on_the_fallback_viewport() {
        let mut stub = Stub::new(Vec::new());
        let source = stub.source();
        let mut terminal = super::open(Mute::new).unwrap();
        let mut console = Console::new(header());
        let ctrl_c = TermEvent::Key(ctrl('c'));
        let keys = stream::iter(vec![Ok(ctrl_c.clone()), Ok(ctrl_c)]).chain(stream::pending());

        drive(
            &mut terminal,
            source,
            &mut stub,
            Box::pin(keys),
            &mut console,
        )
        .await
        .unwrap();

        assert!(console.close(&mut terminal).is_ok());
    }

    /// The source says the stream dropped, and says nothing of its return:
    /// the snapshot the redial brings is what says that.
    #[tokio::test]
    async fn a_dropped_frame_says_reconnecting_until_the_next_snapshot() {
        let ctrl_c = TermEvent::Key(ctrl('c'));
        let leave = vec![ctrl_c.clone(), ctrl_c];
        let down = Stub::new(Vec::new()).after(vec![Frame::Dropped]);
        let (shown, _, _) = console(down, leave.clone()).await;
        assert!(shown.contains("reconnecting"), "{shown}");

        let back = Stub::new(Vec::new()).after(vec![Frame::Dropped, Frame::Snapshot(Vec::new())]);
        let (shown, _, _) = console(back, leave).await;
        assert!(
            !shown.contains("reconnecting") && shown.contains("running"),
            "{shown}"
        );
    }

    /// A snapshot that already holds the session's end is the whole of the
    /// stream: the loop leaves on it without a key, and the transcript is
    /// on the screen.
    #[tokio::test]
    async fn a_snapshot_that_holds_the_session_end_leaves_the_console() {
        let mut stub = Stub::new(vec![
            event("agent_message", "over", json!({"text": "all done"})),
            ended(),
        ]);
        let source = stub.source();
        let mut terminal = terminal();
        let mut console = Console::new(header());
        let keys = stream::pending();

        let outcome = tokio::time::timeout(
            Duration::from_secs(5),
            drive(
                &mut terminal,
                source,
                &mut stub,
                Box::pin(keys),
                &mut console,
            ),
        )
        .await;

        assert!(outcome.is_ok(), "the loop must leave without a key");
        outcome.unwrap().unwrap();
        console.close(&mut terminal).unwrap();
        assert!(
            screen(&terminal).contains("all done"),
            "{}",
            screen(&terminal)
        );
    }
}
