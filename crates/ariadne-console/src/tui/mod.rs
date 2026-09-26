//! The inline pane: a scrolling transcript with a status line and an input
//! box pinned under it.
//!
//! The shape is the one a person expects of a chat with an agent: a viewport
//! as tall as the terminal, its last rows a status line and the box being
//! typed into, which are the bottom rows of the screen whatever the
//! transcript holds, and the transcript filling every row above them, its
//! last line right over the status line. A line of a finished block leaves
//! the viewport as it scrolls off the top, with [`Terminal::insert_before`],
//! so what the agent said is in the terminal's own scrollback, and is still
//! there after the console is closed — which is why the viewport is
//! [`ratatui::Viewport::Inline`] and never the alternate screen.
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
//! proves the viewport still opens, and still grows and shrinks.

use std::collections::{BTreeMap, VecDeque};
use std::future::Future;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{Event as TermEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use futures_util::{Stream, StreamExt};
use ratatui::Terminal;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Paragraph, Widget};
use tokio::time::{Instant, interval};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use ariadne_api::events::AgentEventDto;
use ariadne_api::sessions::SessionDto;
use ariadne_api::usage::TokenUsageDto;

use crate::transcript::{self, PermissionOption, TranscriptItem};

mod banner;
mod blocks;
mod chrome;
mod input;
mod picker;
mod viewport;

#[cfg(test)]
mod scenario;
#[cfg(test)]
mod testing;

pub use viewport::{Anchored, Screen, open};

use blocks::block;
use input::Input;

/// How often the spinner turns while a turn runs.
const TICK: Duration = Duration::from_millis(120);

/// How long the terminal keeps one size before a console that redraws whole
/// on a resize does so: a window dragged wider is many resizes, and the
/// transcript is drawn again once, when the drag stops.
const SETTLE: Duration = Duration::from_millis(250);

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

/// The unchanging half of the status line, and the tokens the session had
/// spent at attach.
#[derive(Debug, Clone, Default)]
pub struct Header {
    seat: String,
    /// The `<agent>:<model>` pin: the agent and the model it runs, in one.
    model: Option<String>,
    effort: Option<String>,
    id: Option<String>,
    task: Option<String>,
    repository: Option<String>,
    status: String,
    usage: TokenUsageDto,
}

impl Header {
    pub fn of(session: Option<&SessionDto>) -> Self {
        session.map_or_else(
            || Self {
                seat: "session".into(),
                model: None,
                effort: None,
                id: None,
                task: None,
                repository: None,
                status: "unknown".into(),
                usage: TokenUsageDto::default(),
            },
            |session| Self {
                seat: session.seat.map_or("-", |seat| seat.as_str()).into(),
                model: Some(session.model.clone()),
                effort: session.effort.clone(),
                id: Some(session.id.clone()),
                task: None,
                repository: None,
                status: session.status.as_str().into(),
                usage: session.usage,
            },
        )
    }

    /// Add the task (or goal) and repository the session row does not hold.
    pub fn with_task(mut self, title: Option<String>, repository: Option<String>) -> Self {
        self.task = title;
        self.repository = repository;
        self
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
    /// How many leading items have gone into the terminal's own buffer
    /// whole.
    committed: usize,
    /// Where the attach banner is: it leads the transcript, and goes into
    /// the scrollback as it scrolls off the top, like any other line.
    banner: Banner,
    /// The last lines of the piece — the banner where it is still
    /// [`Banner::Live`], the item at `committed` otherwise — whose first
    /// lines are in the scrollback already: as they were drawn when the
    /// first of them went, so the piece keeps the width and the fold that
    /// its lines in the scrollback have. Empty where no piece is part-way.
    head: Vec<Line<'static>>,
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
    /// The row had ended when the console attached: the daemon moves a live
    /// row only, so no replayed event moves the status line off that end.
    ended: bool,
    /// The last totals each launch of the session reported, by the launch
    /// they were read under: what the footer sums.
    launches: BTreeMap<String, TokenUsageDto>,
    /// A resize wipes the terminal, its scrollback too, and draws the whole
    /// transcript again at the new size ([`Console::redraws_whole_on_resize`]).
    whole_on_resize: bool,
    /// Whether a fold in the pane draws every line instead: a tool's output,
    /// a diff, a thought and a daemon prompt. Folded at each attach, and
    /// toggled by Ctrl-O for as long as the attach lasts. A block moved to
    /// the scrollback while this is set carries every line of it, since a
    /// block already there cannot be drawn again; a block part-way there
    /// keeps the fold its first lines had.
    whole: bool,
}

/// Where the attach banner is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Banner {
    /// Not drawn: [`Console::banner`] was not called.
    Unshown,
    /// Drawn over the first block, on the screen, its first lines in the
    /// scrollback where `head` holds its last ones.
    Live,
    /// In the scrollback, whole.
    Committed,
}

impl Console {
    pub fn new(header: Header) -> Self {
        let ended = matches!(header.status.as_str(), "exited" | "failed");
        Self {
            header,
            items: Vec::new(),
            committed: 0,
            banner: Banner::Unshown,
            head: Vec::new(),
            pending: VecDeque::new(),
            picked: 0,
            turn: Turn::Idle,
            link: Link::Live,
            input: Input::default(),
            tick: 0,
            since: None,
            armed: false,
            ended,
            launches: BTreeMap::new(),
            whole_on_resize: false,
            whole: false,
        }
    }

    /// Draw the whole transcript again, from a cleared terminal, once a
    /// resize settles.
    ///
    /// Every resize erases the pane's rows and draws the pane again, since
    /// the pane owns the screen: the tail of the transcript at the new
    /// width. The scrollback is the terminal's, and a terminal emulator
    /// re-wraps it when it is made narrower — xterm.js does — so its blocks
    /// are no longer as the console drew them. Where the terminal holds this
    /// console and nothing else, as the desktop app's does, the scrollback
    /// is cleared too and the whole transcript is drawn again, at the new
    /// width, the lines that do not fit on the screen into the scrollback.
    /// The CLI's terminal holds the user's shell above the console, which a
    /// clear would take, and draws the pane again alone.
    pub fn redraws_whole_on_resize(mut self) -> Self {
        self.whole_on_resize = true;
        self
    }

    /// Clear the terminal and draw it again: the banner, every block, and
    /// the pane under them.
    fn redraw<B: Screen>(&mut self, terminal: &mut Terminal<Anchored<B>>) -> Result<()> {
        viewport::restart(terminal)?;
        self.committed = 0;
        self.head.clear();
        if self.banner != Banner::Unshown {
            self.banner = Banner::Live;
        }
        self.show(terminal)
    }

    /// Put the attach identity at the head of the transcript, over its
    /// first block, with one blank line under it as under every block. It
    /// goes into the scrollback as it scrolls off the top of the screen.
    pub fn banner(&mut self) {
        if self.banner == Banner::Unshown {
            self.banner = Banner::Live;
        }
    }

    /// Rebuild from a fresh snapshot, keeping the finished blocks out of it.
    ///
    /// This is what a reconnect lands on. The daemon's stream has no replay,
    /// so the whole transcript arrives again. The finished items the
    /// connection that dropped drew stay as they were drawn: the ones in the
    /// scrollback are not printed twice, and the ones on the screen, which
    /// go into the scrollback as they scroll off, keep their place — a
    /// refused prompt and why it was refused among them, which no event
    /// holds. The snapshot gives the items after them.
    ///
    /// The count is all there is to go on, and the two folds do not always
    /// agree on it: a turn that streamed around a tool call is two blocks
    /// live and one block stored. A block can therefore be lost from the
    /// viewport across a reconnect. It is where it was drawn, and the next
    /// block redraws the viewport.
    pub fn snapshot(&mut self, events: &[AgentEventDto]) {
        self.snapshot_at(events, chrono::Utc::now());
    }

    /// [`Console::snapshot`], with the wall clock behind each event's turn
    /// read as `now` rather than at the moment the event is folded in. Two
    /// consoles folding the same events with the same `now` start their
    /// turn's clock at the same instant, so comparing their renders never
    /// turns on which one read the wall clock first.
    pub(crate) fn snapshot_at(
        &mut self,
        events: &[AgentEventDto],
        now: chrono::DateTime<chrono::Utc>,
    ) {
        self.input.sync_history(
            events
                .iter()
                .filter(|event| event.kind == "user_prompt_submit")
                .filter(|event| {
                    event
                        .payload
                        .get("source")
                        .and_then(|source| source.as_str())
                        == Some("console")
                })
                .filter_map(|event| {
                    event
                        .payload
                        .get("text")
                        .and_then(|text| text.as_str())
                        .map(str::to_owned)
                })
                .collect(),
        );
        // Replaying the transcript replays every turn's start and end, and
        // the clock follows: a turn that was running before the stream
        // dropped counts from its own prompt again, and a turn that began
        // while the stream was down counts from its prompt, not the old one.
        let finished = self.settled();
        let mut items = Vec::new();
        for event in events {
            absorb(&mut items, event);
            self.follow_turn(event, now);
            self.follow_usage(event);
        }
        self.items.truncate(finished);
        self.items.extend(items.into_iter().skip(finished));
        self.pending.clear();
    }

    /// Fold one streamed event into the transcript.
    pub fn apply(&mut self, event: &AgentEventDto) {
        self.apply_at(event, chrono::Utc::now());
    }

    /// [`Console::apply`], with the wall clock behind the event's turn read
    /// as `now` rather than at the moment the event is folded in. See
    /// [`Console::snapshot_at`].
    pub(crate) fn apply_at(&mut self, event: &AgentEventDto, now: chrono::DateTime<chrono::Utc>) {
        self.follow_turn(event, now);
        self.follow_usage(event);
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
        // Input posted while a turn runs is queued behind it (008): what the
        // turn says arrives after the prompt was typed, and is the turn
        // before the prompt's, so it is drawn above the prompt.
        match self
            .pending
            .front()
            .copied()
            .filter(|at| *at < self.items.len())
        {
            Some(at) => {
                let queued = self.items.split_off(at);
                absorb(&mut self.items, event);
                let grown = self.items.len() - at;
                self.items.extend(queued);
                for pending in &mut self.pending {
                    *pending += grown;
                }
            }
            None => absorb(&mut self.items, event),
        }
    }

    /// What the status line says the agent is doing, and what the session's
    /// status is.
    fn follow_turn(&mut self, event: &AgentEventDto, now: chrono::DateTime<chrono::Utc>) {
        // The session's status follows its events the way the daemon moves
        // the row on them (021): the header read at attach is what it was
        // then, and every later event the daemon stores moves it here too —
        // unless the row had ended by then. The daemon moves a live row
        // only, so a row its sweep marked exited, with no session_end
        // stored, or one marked failed, is not revived by the events
        // replayed under it. A live row's replay may hold an earlier
        // launch's session_end before this launch's session_start, and
        // follows both: a resumed session reads as running, not exited.
        if !self.ended
            && let Some(status) = status_of(&event.kind)
        {
            self.header.status = status.into();
        }
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
        self.set_turn(turn, instant_of(&event.created_at, now));
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
        // A typed prompt the daemon refused is never confirmed: it is no
        // longer queued, and holds nothing after it out of the scrollback.
        if self
            .pending
            .back()
            .is_some_and(|at| at + 1 == self.items.len())
        {
            self.pending.pop_back();
        }
        self.items.push(TranscriptItem::Error {
            meta: transcript::ItemMeta {
                created_at: chrono::Utc::now().to_rfc3339(),
                kinds: vec!["session.error".into()],
                closed_by: None,
            },
            text: text.to_string(),
        });
        self.set_turn(Turn::Idle, Instant::now());
    }

    pub fn live(&mut self) {
        self.link = Link::Live;
    }

    /// Whether the session had ended as the console last heard of it.
    pub fn has_ended(&self) -> bool {
        matches!(self.header.status.as_str(), "exited" | "failed")
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
                session_ended(&events)
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
        // Crossterm 0.29 reports Ctrl-J in raw mode as Ctrl with `j`, both
        // for its byte parser and the keyboard protocol. Accept the encoded
        // line-feed character too, as the terminal WebSocket carries keys.
        let modified_enter = key
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
        if ctrl && key.code == KeyCode::Char('o') {
            self.whole = !self.whole;
            return Action::None;
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
            KeyCode::Enter if modified_enter => {
                self.input.newline();
                Action::None
            }
            KeyCode::Char('j' | '\n') if ctrl => {
                self.input.newline();
                Action::None
            }
            KeyCode::Enter if self.input.remove_trailing_backslash() => {
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
                closed_by: None,
            },
            text: text.to_string(),
            source: Some("console".into()),
        });
        // A prompt typed while a turn runs is queued behind it: the status
        // row still says what that turn is doing.
        if !self.turn.running() {
            self.set_turn(Turn::Thinking, Instant::now());
        }
    }

    /// Text the terminal pasted as one: into the box at the cursor, line
    /// breaks and all. Nothing is sent until Enter.
    pub fn paste(&mut self, text: &str) {
        self.armed = false;
        self.input.paste(text);
    }

    /// How many leading items are finished with: their lines are stable,
    /// and go into the scrollback as they scroll off the top of the screen.
    ///
    /// While a turn runs the last item is never one of them — it is what is
    /// still being written. Between turns it is one too, so the last block
    /// ends on its blank line like every finished one — unless it is a run of
    /// chunks no whole has closed, which the next chunk would write into.
    /// Neither is an unanswered question, which is the picker, nor a
    /// prompt still waiting to be confirmed, nor a tool call that has not
    /// ended: the question about a call comes after the call, and the event
    /// that ends the call comes after the answer, so a call committed while
    /// its question waited would sit in the scrollback as pending for ever.
    ///
    /// It never goes backwards: what is in the scrollback is in the
    /// scrollback, whatever a fresh snapshot makes of the items around it.
    fn settled(&self) -> usize {
        let writing = self.turn.running()
            || self
                .items
                .last()
                .is_some_and(|last| is_open(last, true) || is_open(last, false));
        let mut end = self.items.len().saturating_sub(usize::from(writing));
        if let Some(pending) = self.pending.front() {
            // The turn a queued prompt waits behind is still writing the
            // block before the prompt, chunk by chunk: that block is the one
            // being written, not the prompt.
            end = end.min(pending.saturating_sub(1));
        }
        for (at, item) in self.items.iter().enumerate().take(end).skip(self.committed) {
            // A call the turn left open when it stopped — a cancelled turn
            // leaves one — will never end: it holds nothing back after that.
            let open = match item {
                TranscriptItem::PermissionQuestion { answer, .. } => answer.is_none(),
                TranscriptItem::ToolCall { tool, .. } => {
                    self.turn.running() && !transcript::tool_is_terminal(tool.status.as_deref())
                }
                _ => false,
            };
            if open {
                return at;
            }
        }
        end.max(self.committed)
    }

    /// The pieces of the transcript not yet wholly in the scrollback that
    /// may go into it, in order, `width` columns wide: the last lines of
    /// the piece part-way there, the banner, and the finished blocks, each
    /// with the blank line under it. A block that draws nothing is a piece
    /// of no lines, not even the blank one. With them, the first item that
    /// is not one of them.
    fn finished(&self, width: u16) -> (Vec<Vec<Line<'static>>>, usize) {
        let mut pieces = Vec::new();
        let mut next = self.committed;
        if !self.head.is_empty() {
            pieces.push(rewrap(&self.head, width));
            if self.banner != Banner::Live {
                next += 1;
            }
        } else if self.banner == Banner::Live {
            let mut lines = banner::draw(&self.header, usize::from(width));
            lines.push(Line::default());
            pieces.push(lines);
        }
        let end = self.settled().max(next);
        for item in self.items.iter().take(end).skip(next) {
            pieces.push(self.piece(item, width));
        }
        (pieces, end)
    }

    /// The lines one item takes in the scrollback: its block, and the blank
    /// line under it where the block draws anything.
    fn piece(&self, item: &TranscriptItem, width: u16) -> Vec<Line<'static>> {
        let mut lines = block(item, usize::from(width), None, self.whole);
        if !lines.is_empty() {
            lines.push(Line::default());
        }
        lines
    }

    /// Move every finished line into the terminal's own buffer, whether it
    /// fits the screen or not: what a test reads as what the scrollback
    /// gets.
    #[cfg(test)]
    pub(super) fn commit<B: Screen>(&mut self, terminal: &mut Terminal<B>) -> Result<()> {
        let (pieces, _) = self.finished(terminal.size()?.width);
        let count = pieces.iter().map(Vec::len).sum();
        self.emit(terminal, pieces, count)
    }

    /// One turn of the screen: the pane made as tall as the terminal, the
    /// finished lines that do not fit over its pinned rows moved above it,
    /// and the pane drawn.
    ///
    /// The height comes first, and is the terminal's own: the pane holds
    /// every row of the screen, so the fit has nothing to do but at the open
    /// and on a resize. The transcript then fills the rows over the pinned
    /// ones, its last line on the row over the status row: the lines that
    /// do not fit leave through the top, into the scrollback, from the
    /// first, and only where they are finished — the block being written
    /// keeps its head, which the pane does not show, until it is finished.
    /// A line leaves through a scrolling region — ratatui borrows the
    /// pane's top row for it — and no draw repaints more than the cells that
    /// changed.
    pub fn show<B: Screen>(&mut self, terminal: &mut Terminal<Anchored<B>>) -> Result<()> {
        if terminal.backend().lost() {
            return Ok(());
        }
        viewport::fit(terminal, self.rows(terminal.size()?))?;
        let size = terminal.size()?;
        let room = size.height.saturating_sub(self.pinned_rows(size.width));
        let (pieces, from) = self.finished(size.width);
        let finished: usize = pieces.iter().map(Vec::len).sum();
        let written = self.unfinished(from, size.width, room).len();
        let over = (finished + written).saturating_sub(usize::from(room));
        self.emit(terminal, pieces, over.min(finished))?;
        terminal.draw(|frame| self.render(frame))?;
        Ok(())
    }

    /// The way out: everything left goes to the scrollback, and the viewport
    /// is wiped so the shell comes back to a clean line. The pane is made as
    /// short as its pinned rows first, on the top row of the screen, so what
    /// is left is inserted over the rows it was drawn on and the shell comes
    /// back right under it.
    ///
    /// A terminal whose backend a failed fit lost has nothing to write to:
    /// the error that ended the loop is the one to report, and this is `Ok`.
    pub fn close<B: Screen>(&mut self, terminal: &mut Terminal<Anchored<B>>) -> Result<()> {
        if terminal.backend().lost() {
            return Ok(());
        }
        let width = terminal.size()?.width;
        viewport::fit(terminal, self.pinned_rows(width))?;
        // Every item is done with now, the one being written and an
        // unanswered question too: each goes as the block it is.
        let (mut pieces, from) = self.finished(width);
        for item in self.items.iter().skip(from) {
            pieces.push(self.piece(item, width));
        }
        let count = pieces.iter().map(Vec::len).sum();
        self.emit(terminal, pieces, count)?;
        // ratatui's clear puts the cursor back where the box had it, inside
        // the wiped rows: the shell comes back at their top instead.
        terminal.clear()?;
        let top = terminal.get_frame().area().as_position();
        terminal.set_cursor_position(top)?;
        Ok(())
    }

    /// Move the first `count` lines of `pieces` — what [`Console::finished`]
    /// gives, in its order — into the terminal's own buffer, above the
    /// viewport, where the scrollback keeps them. A piece that goes whole is
    /// done with; the last lines of one that goes in part are kept as they
    /// are, to go later.
    fn emit<B: Screen>(
        &mut self,
        terminal: &mut Terminal<B>,
        pieces: Vec<Vec<Line<'static>>>,
        count: usize,
    ) -> Result<()> {
        let mut left = count;
        for mut lines in pieces {
            let taken = lines.len().min(left);
            let rest = lines.split_off(taken);
            if !lines.is_empty() {
                let height = u16::try_from(lines.len()).unwrap_or(u16::MAX);
                terminal.insert_before(height, |buffer| {
                    Paragraph::new(Text::from(lines)).render(buffer.area, buffer);
                })?;
            }
            if !rest.is_empty() {
                if taken > 0 {
                    self.head = rest;
                }
                break;
            }
            left -= taken;
            self.head.clear();
            match self.banner {
                Banner::Live => self.banner = Banner::Committed,
                _ => self.committed += 1,
            }
        }
        Ok(())
    }
}

/// `lines` cut into rows of `width` columns at most, between grapheme
/// clusters: the last lines of a piece drawn at a width the terminal has
/// since been made narrower than, as a terminal re-wraps its scrollback.
fn rewrap(lines: &[Line<'static>], width: u16) -> Vec<Line<'static>> {
    let width = usize::from(width.max(1));
    let mut rows = Vec::with_capacity(lines.len());
    for line in lines {
        if line.width() <= width {
            rows.push(line.clone());
            continue;
        }
        let mut row: Vec<Span<'static>> = Vec::new();
        let mut used = 0;
        for span in &line.spans {
            let mut text = String::new();
            for grapheme in span.content.graphemes(true) {
                let columns = grapheme.width();
                if used > 0 && used + columns > width {
                    if !text.is_empty() {
                        row.push(Span::styled(std::mem::take(&mut text), span.style));
                    }
                    rows.push(Line::from(std::mem::take(&mut row)).style(line.style));
                    used = 0;
                }
                used += columns;
                text.push_str(grapheme);
            }
            if !text.is_empty() {
                row.push(Span::styled(text, span.style));
            }
        }
        rows.push(Line::from(row).style(line.style));
    }
    rows
}

/// Whether a session's events so far end with the session's end: a
/// `session_end` that no `session_start` came after. A session killed and
/// revived later holds the end of the launch before this one, followed by
/// this launch's start, and is not over.
pub fn session_ended(events: &[AgentEventDto]) -> bool {
    events
        .iter()
        .rev()
        .find(|event| matches!(event.kind.as_str(), "session_start" | "session_end"))
        .is_some_and(|event| event.kind == "session_end")
}

/// The session status an event moves the row to, as the daemon maps it
/// (021): the agent is running from its start, a prompt, a tool event or an
/// answered permission; idle once a turn stops or a compaction ends; exited
/// once the session ends. Any other event leaves the status where it was.
fn status_of(kind: &str) -> Option<&'static str> {
    match kind {
        "session_start" | "user_prompt_submit" | "pre_tool_use" | "post_tool_use"
        | "permission.replied" => Some("running"),
        "stop" | "compaction_update" => Some("idle"),
        "session_end" => Some("exited"),
        _ => None,
    }
}

/// When an event happened, on the console's own clock: a turn already
/// running at attach counts from its prompt, not from the attach. An event
/// whose time cannot be read, or that a clock ahead of this one dated in the
/// future, counts from now.
fn instant_of(created_at: &str, now: chrono::DateTime<chrono::Utc>) -> Instant {
    let anchor = Instant::now();
    chrono::DateTime::parse_from_rfc3339(created_at)
        .ok()
        .and_then(|at| (now - at.with_timezone(&chrono::Utc)).to_std().ok())
        .and_then(|age| anchor.checked_sub(age))
        .unwrap_or(anchor)
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
        // A turn that ends at once has its last chunk and its stored whole
        // ready together, and a stream can hand over the whole first. A
        // chunk that arrives after the whole of its kind, with an id below
        // that whole's, and says nothing the whole does not, is that late
        // chunk: it is folded in, not drawn again. The id is what tells it
        // from the next turn's first chunk handed over before its prompt —
        // the daemon gives the live chunks and the stored events their ids
        // from one monotonic generator, and a new turn's chunk gets its id
        // after its prompt, which is after the whole before it.
        let said = chunk_text(event);
        if let Some(
            TranscriptItem::Thought { meta, text } | TranscriptItem::AgentText { meta, text },
        ) = items.last_mut().filter(|last| is_kind(last, thought))
            && meta
                .closed_by
                .as_deref()
                .is_some_and(|whole| event.id.as_str() < whole)
            && text.contains(&said)
        {
            // Recorded under the whole that closed the block, which stays
            // its last kind: the block is not reopened for the next chunk.
            let closing = meta.kinds.len().saturating_sub(1);
            meta.kinds.insert(closing, event.kind.clone());
            return;
        }
        items.push(TranscriptItem::from(event));
        return;
    }

    // The whole the daemon stores for a run of text (021) is every chunk of
    // that run joined into one. Where chunks arrived it says again what is
    // already on the screen, so it closes the blocks the chunks opened and
    // adds no text; only a run that streamed nothing pushes a block of its
    // own.
    let mut closed = false;
    for item in items.iter_mut() {
        if is_open(item, thought)
            && let TranscriptItem::Thought { meta, .. } | TranscriptItem::AgentText { meta, .. } =
                item
        {
            meta.kinds.push(event.kind.clone());
            meta.closed_by = Some(event.id.clone());
            closed = true;
        }
    }
    if !closed {
        let mut item = TranscriptItem::from(event);
        if let TranscriptItem::Thought { meta, .. } | TranscriptItem::AgentText { meta, .. } =
            &mut item
        {
            meta.closed_by = Some(event.id.clone());
        }
        items.push(item);
    }
}

/// Whether this block is still taking chunks of the kind asked for.
fn is_open(item: &TranscriptItem, thought: bool) -> bool {
    is_kind(item, thought)
        && item
            .meta()
            .kinds
            .last()
            .is_some_and(|kind| kind.ends_with("_chunk"))
}

/// Whether this block is a thought, or the agent's text, as asked for.
fn is_kind(item: &TranscriptItem, thought: bool) -> bool {
    match item {
        TranscriptItem::Thought { .. } => thought,
        TranscriptItem::AgentText { .. } => !thought,
        _ => false,
    }
}

/// The text one chunk carries, whichever of the two kinds it is.
fn chunk_text(event: &AgentEventDto) -> String {
    match TranscriptItem::from(event) {
        TranscriptItem::Thought { text, .. } | TranscriptItem::AgentText { text, .. } => text,
        _ => String::new(),
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
    terminal: &mut Terminal<Anchored<B>>,
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
    // When the terminal last changed size, while a redraw of the whole
    // transcript waits for it to settle.
    let mut resized: Option<tokio::time::Instant> = None;

    // The stream opens with the transcript snapshot. Take it before any key,
    // so the first thing drawn is the conversation as it stands and a key
    // pressed at once answers a question that is already on the screen. A
    // session that had already ended is over here too: the caller's close
    // puts the transcript in the scrollback, and nothing more will come.
    let ended = match source.next().await {
        Some(frame) => console.frame(frame?),
        None => false,
    };
    console.banner();
    if ended {
        return Ok(());
    }

    loop {
        console.show(terminal)?;

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
            // A resize is a turn of the loop: the loop fits the pane to the
            // terminal's size on its way round, before it draws, and a
            // console that redraws whole does so once the size settles.
            Step::Key(TermEvent::Resize(..)) if console.whole_on_resize => {
                resized = Some(tokio::time::Instant::now());
            }
            Step::Key(_) => {}
            Step::Ended | Step::Closed => return Ok(()),
            Step::Tick => {
                console.tick();
                if resized.is_some_and(|at| at.elapsed() >= SETTLE) {
                    resized = None;
                    console.redraw(terminal)?;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {

    use futures_util::stream;
    use serde_json::json;

    use crate::tui::testing::*;
    use crate::tui::*;

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

    /// The seat's system prompt, as the daemon sends it ahead of every
    /// prompt (021): the one thing the console must never draw.
    const SYSTEM_PROMPT: &str = "You plan one Ariadne goal into tasks, with the user.";

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

        assert!(shown.contains("❯ Run the tests"), "{shown}");
        assert!(
            shown.contains("# Report"),
            "the heading is a heading: {shown}"
        );
        assert!(
            shown.contains("  sh\n"),
            "the code block names its language: {shown}"
        );
        assert!(
            !shown.contains("```"),
            "the code block has no literal fence: {shown}"
        );
        assert!(
            shown.contains("✓ $ cargo nextest run"),
            "a completed call is ticked, and its head is its command: {shown}"
        );
        assert!(
            shown.contains("… 2 more lines") && shown.contains("    six\n author"),
            "the output is folded to its last lines, the blank ones trimmed: {shown}"
        );
        assert!(
            shown.contains("author") && shown.contains("claude:opus") && shown.contains("running"),
            "the status line names the seat, the model and the status: {shown}"
        );
    }

    /// The daemon hosts many consoles in one process, and they share one
    /// set of grammars for fenced code.
    #[tokio::test]
    async fn two_consoles_that_draw_fenced_code_build_the_grammars_once() {
        use std::sync::atomic::Ordering;

        for text in ["```rust\nlet n = 1;\n```", "```json\n{\"n\": 1}\n```"] {
            let (shown, _, _) = console(
                Stub::new(vec![event("agent_message", text, json!({"text": text}))])
                    .deltas(vec![ended()]),
                Vec::new(),
            )
            .await;
            assert!(shown.contains("n"), "{shown}");
        }

        assert_eq!(crate::markdown::GRAMMARS_BUILT.load(Ordering::SeqCst), 1);
    }

    /// A cancelled turn leaves its call as it was, and nothing will end it:
    /// the blocks after it still go to the scrollback.
    #[test]
    fn a_call_a_stopped_turn_left_open_holds_nothing_back_from_the_scrollback() {
        let mut terminal = pane();
        let mut console = Console::new(header());
        let typed = |text: &str| {
            event(
                "user_prompt_submit",
                text,
                json!({"text": text, "source": "console"}),
            )
        };
        console.snapshot(&[
            typed("go"),
            updated("run", "in_progress", ""),
            event("stop", "cancelled", json!({"stop_reason": "cancelled"})),
            typed("next"),
            event("agent_message", "answered", json!({"text": "answered"})),
            event("stop", "stop", json!({"stop_reason": "end_turn"})),
        ]);

        console.show(&mut terminal).unwrap();
        console.commit(&mut terminal).unwrap();

        let shown = shown(&terminal);
        assert!(
            console.live_lines(72, 35).is_empty(),
            "every block is finished, and none is left out of the scrollback: {shown}"
        );
        assert!(
            rows(terminal.backend().under().scrollback()).ends_with("● answered\n"),
            "the last block is in the scrollback: {shown}"
        );
    }

    #[test]
    fn a_refused_prompt_is_no_longer_queued_and_holds_nothing_back() {
        let mut terminal = pane();
        let mut console = Console::new(header());
        type_into(&mut console, "hello");
        assert_eq!(enter(&mut console), Action::Send("hello".into()));

        console.failed("409 Conflict: the session takes no more input");
        console.show(&mut terminal).unwrap();

        let shown = shown(&terminal);
        assert!(!shown.contains("queued"), "{shown}");
        assert!(
            shown.starts_with("▌❯ hello\n\n✗ 409 Conflict"),
            "the prompt and why it was refused are on the screen: {shown}"
        );
        console.commit(&mut terminal).unwrap();
        assert!(console.live_lines(72, 35).is_empty(), "{shown}");
    }

    #[test]
    fn a_prompt_typed_while_a_tool_runs_leaves_the_tool_on_the_status_row() {
        let mut console = Console::new(header());
        let mut terminal = terminal();
        console.apply(&updated("run", "in_progress", ""));

        type_into(&mut console, "next");
        enter(&mut console);
        terminal.draw(|frame| console.render(frame)).unwrap();

        let shown = screen(&terminal);
        assert!(shown.contains("running Bash"), "{shown}");
    }

    #[tokio::test]
    async fn one_blank_line_separates_the_banner_from_the_first_block() {
        let (shown, _, _) = console(
            Stub::new(vec![event("agent_message", "hi", json!({"text": "hi"}))])
                .deltas(vec![ended()]),
            Vec::new(),
        )
        .await;

        let rows: Vec<&str> = shown.lines().collect();
        let bottom = rows
            .iter()
            .position(|row| row.starts_with('╰'))
            .expect("a framed banner");
        assert_eq!(rows[bottom + 1..bottom + 3], ["", "● hi"], "{shown}");
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
            shown.find("Let me run the tests.") < shown.find("✓ ◇ Bash")
                && shown.find("✓ ◇ Bash") < shown.find("The tests pass."),
            "in the order the turn happened: {shown}"
        );
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

    /// A turn that ends at once has its last chunks and its stored whole
    /// ready together, and a stream can hand over the whole first. The late
    /// chunks say nothing the whole did not and carry ids below it: they
    /// fold in, not drawn again, and the first of them does not reopen the
    /// block for the second. The next turn's first chunk, with an id above
    /// the whole, is a block of its own even when the stream hands it over
    /// before the prompt that began its turn, and even when it repeats the
    /// words.
    #[test]
    fn a_chunk_that_arrives_after_the_whole_of_its_turn_is_not_drawn_again() {
        let text = |words: &str| json!({"text": words});
        let mut console = Console::new(header());
        let mut terminal = terminal();
        console.apply(&numbered(
            event("agent_message", "whole", text("All three steps are done.")),
            3,
        ));
        console.apply(&numbered(
            event("agent_message_chunk", "late-1", text("All three ")),
            1,
        ));
        console.apply(&numbered(
            event("agent_message_chunk", "late-2", text("steps are done.")),
            2,
        ));
        // The next turn's first chunk, handed over before its prompt.
        console.apply(&numbered(
            event(
                "agent_message_chunk",
                "next",
                text("All three steps are done."),
            ),
            5,
        ));
        console.apply(&numbered(
            event(
                "user_prompt_submit",
                "more",
                json!({"text": "more", "source": "console"}),
            ),
            4,
        ));

        console.commit(&mut terminal).unwrap();
        terminal.draw(|frame| console.render(frame)).unwrap();
        let shown = screen(&terminal);

        assert_eq!(
            shown.matches("● All three steps are done.").count(),
            2,
            "once per turn: the late chunks fold in, the next turn's does not: {shown}"
        );
        assert!(
            !shown.contains("done.All") && !shown.contains("done.steps"),
            "no late chunk is appended to the whole: {shown}"
        );
    }

    /// Input posted while a turn runs is queued behind it (008). What the
    /// turn says arrives after the prompt was typed, and is drawn above the
    /// prompt: the prompt is the next turn's.
    #[test]
    fn what_a_running_turn_says_is_drawn_above_a_prompt_typed_while_it_ran() {
        let mut console = Console::new(header());
        let mut terminal = terminal();
        console.apply(&event(
            "user_prompt_submit",
            "one",
            json!({"text": "one", "source": "console"}),
        ));
        type_into(&mut console, "two");
        enter(&mut console);

        // The turn streams on, and the pane is redrawn between its chunks.
        console.apply(&event(
            "agent_message_chunk",
            "answer",
            json!({"text": "answer "}),
        ));
        console.commit(&mut terminal).unwrap();
        terminal.draw(|frame| console.render(frame)).unwrap();
        console.apply(&event(
            "agent_message_chunk",
            "answer-end",
            json!({"text": "to one"}),
        ));
        console.apply(&event("stop", "stop", json!({"stop_reason": "end_turn"})));
        console.apply(&event(
            "user_prompt_submit",
            "two",
            json!({"text": "two", "source": "console"}),
        ));
        console.commit(&mut terminal).unwrap();
        terminal.draw(|frame| console.render(frame)).unwrap();
        let shown = screen(&terminal);

        let answer = row_of(&shown, "answer to one").expect(&shown);
        let two = row_of(&shown, "❯ two").expect(&shown);
        assert!(
            answer < two,
            "the answer to the first prompt is above the second: {shown}"
        );
        assert_eq!(
            shown.matches("answer").count(),
            1,
            "the answer is whole, not cut at the chunk the pane was redrawn on: {shown}"
        );
        assert_eq!(
            shown.matches("❯ two").count(),
            1,
            "and the typed prompt is drawn once: {shown}"
        );
    }

    /// The question about a call comes after the call, and the event that
    /// ends the call comes after the answer. The call stays in the pane until
    /// it has ended, so the scrollback gets it as it ended — with its mark
    /// and its time — and never as pending.
    #[test]
    fn a_call_a_question_asks_about_reaches_the_scrollback_as_it_ended_not_as_pending() {
        let mut console = Console::new(header());
        let mut terminal = terminal();
        let acp = |status: &str| {
            json!({"toolCallId": "edit", "kind": "edit", "status": status,
                   "rawInput": {"file_path": "src/lib.rs"}})
        };
        console.apply(&event(
            "pre_tool_use",
            "Edit",
            json!({"tool_name": "Edit", "acp": acp("pending")}),
        ));
        console.apply(&event(
            "permission_request",
            "Permission requested for Edit",
            json!({"tool_name": "Edit", "acp": acp("pending"),
                   "options": [{"optionId": "yes", "name": "Allow"}]}),
        ));
        console.commit(&mut terminal).unwrap();
        terminal.draw(|frame| console.render(frame)).unwrap();

        console.apply(&event(
            "permission.replied",
            "answered",
            json!({"option_id": "yes"}),
        ));
        let mut ended = event(
            "post_tool_use",
            "Edit",
            json!({"tool_name": "Edit", "acp": acp("completed")}),
        );
        ended.created_at = "2026-09-12T00:00:02Z".into();
        console.apply(&ended);
        console.apply(&event("agent_message", "done", json!({"text": "done"})));
        console.commit(&mut terminal).unwrap();
        terminal.draw(|frame| console.render(frame)).unwrap();
        let shown = screen(&terminal);

        assert!(
            shown.contains("✓ ✎ src/lib.rs  2.0s"),
            "the call is drawn as it ended: {shown}"
        );
        assert!(
            !shown.contains("○ ✎ src/lib.rs"),
            "and not as pending above it: {shown}"
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
        assert_eq!(shown.matches("❯ a").count(), 1, "{shown}");
        assert_eq!(shown.matches("❯ b").count(), 1, "{shown}");
    }

    #[tokio::test]
    async fn a_resize_redraws_the_viewport_at_the_new_size() {
        let mut stub = Stub::new(Vec::new());
        let source = stub.source();
        let mut terminal = pane();
        let mut console = Console::new(header());
        terminal.backend_mut().under_mut().resize(40, 20);
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

        let shown = shown(&terminal);
        assert!(
            shown
                .lines()
                .any(|line| line == "────────────────────────────────────────"),
            "the box's rule reaches the new right edge: {shown}"
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
            shown.matches("❯ hi").count(),
            1,
            "the confirmed prompt took the pending one's place: {shown}"
        );
    }

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

        assert!(shown.contains("❯ Run the tests"), "{shown}");
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
            shown.contains("❯ (prompt text not recorded)"),
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
            shown.matches("❯ hi").count(),
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
            shown.matches("❯ tests").count(),
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
            shown.matches("❯ tests").count(),
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
            screen(&terminal).contains("❯ ship it"),
            "the prompt shows before any event comes back: {}",
            screen(&terminal)
        );
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

    /// A fresh attach starts folded, and Ctrl-O toggles it for the rest of
    /// the attach; a reconnect keeps whichever the attach had (rule 27).
    #[test]
    fn a_fresh_attach_starts_folded_and_a_reconnect_keeps_the_state_it_had() {
        let mut console = Console::new(header());
        let mut terminal = terminal();

        terminal.draw(|frame| console.render(frame)).unwrap();
        assert!(
            screen(&terminal).contains("ctrl-o unfold"),
            "a fresh attach starts folded"
        );

        console.key(ctrl('o'));
        terminal.draw(|frame| console.render(frame)).unwrap();
        assert!(
            screen(&terminal).contains("ctrl-o fold"),
            "ctrl-o sets it whole"
        );

        console.dropped();
        console.snapshot(&[]);
        console.live();
        terminal.draw(|frame| console.render(frame)).unwrap();
        assert!(
            screen(&terminal).contains("ctrl-o fold"),
            "a reconnect keeps what the attach had"
        );
    }

    /// Ctrl-O toggles the fold state during a pending permission question
    /// too, and the question with every option stays on the screen (rule
    /// 27). Every other key of the picker still works after it.
    #[test]
    fn ctrl_o_during_a_permission_question_toggles_the_state_and_keeps_it_on_screen() {
        let mut console = Console::new(header());
        let mut terminal = terminal();
        console.apply(&asked());

        terminal.draw(|frame| console.render(frame)).unwrap();
        let before = screen(&terminal);
        assert!(
            before.contains("Write") && before.contains("❯ 1. Reject"),
            "{before}"
        );

        console.key(ctrl('o'));
        terminal.draw(|frame| console.render(frame)).unwrap();
        let after = screen(&terminal);
        assert!(after.contains("ctrl-o fold"), "ctrl-o toggled: {after}");
        assert!(
            after.contains("Write") && after.contains("❯ 1. Reject") && after.contains("2. Allow"),
            "the question and every option stay on the screen: {after}"
        );

        console.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        terminal.draw(|frame| console.render(frame)).unwrap();
        assert!(
            screen(&terminal).contains("❯ 2. Allow"),
            "the arrow keys still move the pick"
        );
    }

    /// A block moved to the scrollback while the fold state is whole carries
    /// every line of it: a block already there cannot be drawn again (rule
    /// 27).
    #[test]
    fn a_block_committed_while_whole_carries_every_line_into_the_scrollback() {
        let mut console = Console::new(header());
        let mut terminal = terminal();
        let output: String = (1..=30).map(|n| format!("line {n}\n")).collect();
        console.apply(&event(
            "post_tool_use",
            "run",
            json!({"acp": {"toolCallId": "run", "kind": "execute", "status": "completed",
                           "rawInput": {"command": "cat file"}, "rawOutput": output}}),
        ));
        console.key(ctrl('o'));
        console.apply(&event("agent_message", "done", json!({"text": "done"})));

        console.commit(&mut terminal).unwrap();
        let shown = screen(&terminal);

        assert!(!shown.contains("more lines"), "{shown}");
        assert!(
            shown.contains("line 1") && shown.contains("line 30"),
            "{shown}"
        );
    }

    /// A block part-way into the scrollback keeps the fold its lines there
    /// were drawn with, since they cannot be drawn again, and Ctrl-O changes
    /// the blocks after it (rule 27).
    #[test]
    fn a_block_part_way_into_the_scrollback_keeps_its_fold_and_ctrl_o_changes_the_ones_after() {
        let mut terminal = pane();
        let mut console = Console::new(header());
        let ran = |id: &str| {
            let output: String = (1..=30).map(|n| format!("{id}-{n:02}\n")).collect();
            event(
                "post_tool_use",
                id,
                json!({"acp": {"toolCallId": id, "kind": "execute", "status": "completed",
                               "rawInput": {"command": format!("cat {id}")},
                               "rawOutput": output}}),
            )
        };
        let stop = || event("stop", "stop", json!({"stop_reason": "end_turn"}));
        let said: String = (1..=20).map(|n| format!("- said-{n:02}\n")).collect();
        console.key(ctrl('o'));
        console.apply(&ran("alpha"));
        console.apply(&event("agent_message", "said", json!({"text": said})));
        console.apply(&stop());
        console.show(&mut terminal).unwrap();
        let scrollback = rows(terminal.backend().under().scrollback());
        assert!(
            scrollback.contains("alpha-01") && !scrollback.contains("alpha-30"),
            "the first block is part-way into the scrollback: {scrollback}"
        );

        console.key(ctrl('o'));
        console.apply(&ran("beta"));
        console.apply(&stop());
        console.show(&mut terminal).unwrap();

        let shown = shown(&terminal);
        for n in 1..=30 {
            assert_eq!(
                shown.matches(&format!("alpha-{n:02}")).count(),
                1,
                "the part-way block stays whole, each line once: {shown}"
            );
        }
        assert!(
            shown.contains("beta-30") && !shown.contains("beta-01"),
            "the block after it is folded: {shown}"
        );
        assert_eq!(shown.matches("more lines").count(), 1, "{shown}");
    }

    /// A refused prompt and why it was refused are the console's own, and
    /// no event holds them: a reconnect's snapshot keeps them where they
    /// were drawn.
    #[test]
    fn a_reconnect_keeps_a_refused_prompt_on_the_screen() {
        let mut terminal = pane();
        let mut console = Console::new(header());
        let events = [
            event(
                "user_prompt_submit",
                "go",
                json!({"text": "go", "source": "console"}),
            ),
            event("agent_message", "done", json!({"text": "done"})),
            event("stop", "stop", json!({"stop_reason": "end_turn"})),
        ];
        console.snapshot(&events);
        type_into(&mut console, "more");
        enter(&mut console);
        console.failed("409 Conflict: the session takes no more input");
        console.show(&mut terminal).unwrap();

        console.dropped();
        console.snapshot(&events);
        console.live();
        console.apply(&event("agent_message", "later", json!({"text": "later"})));
        console.show(&mut terminal).unwrap();

        let shown = shown(&terminal);
        assert!(
            shown.contains("▌❯ go\n\n● done\n\n▌❯ more\n\n✗ 409 Conflict"),
            "the refused prompt is where it was drawn: {shown}"
        );
        assert_eq!(shown.matches("● done").count(), 1, "{shown}");
        assert!(
            shown.find("✗ 409 Conflict") < shown.find("● later"),
            "and what comes after the reconnect is under it: {shown}"
        );
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
        assert_eq!(shown.matches("❯ first").count(), 1, "{shown}");
        assert_eq!(shown.matches("second").count(), 1, "{shown}");
        assert_eq!(shown.matches("third").count(), 1, "{shown}");
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
        let mut terminal = pane();
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
            shown(&terminal).contains("all done"),
            "{}",
            shown(&terminal)
        );
    }

    /// A session killed and revived holds the end of the launch before, and
    /// this launch's start after it. The session is running again, and a
    /// console opened on it stays open; one whose last launch ended does not.
    #[test]
    fn a_snapshot_whose_session_started_again_after_its_end_keeps_the_console() {
        let started = || event("session_start", "session started", json!({}));
        let revived = vec![
            started(),
            ended(),
            started(),
            event("agent_message", "back", json!({"text": "back"})),
        ];

        assert!(!Console::new(header()).frame(Frame::Snapshot(revived.clone())));
        assert!(!session_ended(&revived));
        assert!(session_ended(&[started(), ended()]));
        assert!(session_ended(&[ended()]));
        assert!(!session_ended(&[]));
    }
}
