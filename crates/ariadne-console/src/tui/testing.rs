//! Test doubles and helpers the modules of the pane share.

use futures_util::stream;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::{TerminalOptions, Viewport};
use serde_json::json;
use tokio::sync::mpsc;
use unicode_width::UnicodeWidthStr;

use super::viewport::VIEWPORT;
use super::*;

/// A daemon console, as far as the pane can tell: a snapshot, deltas of
/// its own, and deltas it answers a post with — which is what makes a
/// test that types deterministic, since nothing arrives that the typing
/// did not cause. In memory: the sink it is, and the source it feeds.
pub(super) struct Stub {
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
    pub(super) fn new(snapshot: Vec<AgentEventDto>) -> Self {
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

    pub(super) fn refuses(mut self) -> Self {
        self.refuses = true;
        self
    }

    pub(super) fn deltas(mut self, deltas: Vec<AgentEventDto>) -> Self {
        self.deltas = deltas;
        self
    }

    pub(super) fn on_input(mut self, events: Vec<AgentEventDto>) -> Self {
        self.on_input = events;
        self
    }

    pub(super) fn on_cancel(mut self, events: Vec<AgentEventDto>) -> Self {
        self.on_cancel = events;
        self
    }

    pub(super) fn after(mut self, frames: Vec<Frame>) -> Self {
        self.after = frames;
        self
    }

    /// The stream as the loop reads it: the snapshot, the deltas, then
    /// whatever a post causes. It never ends while the stub is alive, so
    /// only the daemon or a Ctrl-C stops the loop — as in the real one.
    pub(super) fn source(&mut self) -> impl Stream<Item = Result<Frame>> + use<> {
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

pub(super) fn header() -> Header {
    Header {
        seat: "author".into(),
        model: Some("claude:opus".into()),
        effort: Some("high".into()),
        id: Some("01m2x2gbzj5c1234".into()),
        task: Some("Input box".into()),
        repository: Some("ariadne".into()),
        status: "running".into(),
        usage: Default::default(),
    }
}

pub(super) fn terminal() -> Terminal<TestBackend> {
    Terminal::with_options(
        TestBackend::new(72, 40),
        TerminalOptions {
            viewport: Viewport::Inline(VIEWPORT),
        },
    )
    .unwrap()
}

/// The terminal the loop draws on: the viewport as [`open`] makes it, on the
/// top row of an empty screen, which [`Console::show`] then fits.
pub(super) fn pane() -> Terminal<Anchored<TestBackend>> {
    open(|| TestBackend::new(72, 40)).unwrap()
}

/// Everything the terminal [`pane`] makes has shown: its scrollback, where
/// the finished blocks are, and then its screen, which the pane holds
/// whole. The blank rows the bottom-aligned pane leaves between them are
/// left out, so a test reads the transcript as one block of text.
pub(super) fn shown(terminal: &Terminal<Anchored<TestBackend>>) -> String {
    let backend = terminal.backend().under();
    let mut text = String::new();
    if backend.scrollback().area.height > 0 {
        text.push_str(&rows(backend.scrollback()));
        text.push('\n');
    }
    text.push_str(&filled(backend.buffer()));
    text
}

/// Everything the terminal shows: the scrollback above the viewport and
/// the viewport itself, as one block of text.
pub(super) fn screen(terminal: &Terminal<TestBackend>) -> String {
    rows(terminal.backend().buffer())
}

/// The rows of `buffer` from its first row that holds something: the run of
/// blank rows a bottom-aligned pane leaves over the block it is writing is
/// not part of what the transcript says.
pub(super) fn filled(buffer: &Buffer) -> String {
    let shown = rows(buffer);
    let kept: Vec<&str> = shown.lines().skip_while(|row| row.is_empty()).collect();
    kept.join("\n").trim_end().to_string()
}

/// The cell after a wide character is the blank the buffer leaves under
/// its second column, and is not read as a space.
pub(super) fn rows(buffer: &Buffer) -> String {
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

pub(super) fn key(code: KeyCode) -> TermEvent {
    TermEvent::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

pub(super) fn ctrl(character: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(character), KeyModifiers::CONTROL)
}

pub(super) fn paste(text: &str) -> TermEvent {
    TermEvent::Paste(text.into())
}

/// Type `text` into the console, one key per character.
pub(super) fn type_into(console: &mut Console, text: &str) {
    for character in text.chars() {
        console.key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }
}

pub(super) fn enter(console: &mut Console) -> Action {
    console.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
}

pub(super) fn typed(text: &str) -> Vec<TermEvent> {
    text.chars().map(|c| key(KeyCode::Char(c))).collect()
}

/// Drive the console against the stub until the session ends, then read
/// the screen. No server anywhere: the stub is the source and the sink.
pub(super) async fn console(mut stub: Stub, keys: Vec<TermEvent>) -> (String, Vec<String>, usize) {
    let source = stub.source();
    let mut terminal = pane();
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

    (shown(&terminal), stub.prompts, stub.cancels)
}

pub(super) fn event(kind: &str, summary: &str, payload: serde_json::Value) -> AgentEventDto {
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

pub(super) fn ended() -> AgentEventDto {
    event("session_end", "session ended", json!({}))
}

pub(super) fn event_at(
    kind: &str,
    summary: &str,
    payload: serde_json::Value,
    at: &str,
) -> AgentEventDto {
    let mut event = event(kind, summary, payload);
    event.created_at = at.into();
    event
}

/// An event with the id the daemon's monotonic generator would have given
/// it at that point of the stream.
pub(super) fn numbered(mut event: AgentEventDto, id: u32) -> AgentEventDto {
    event.id = format!("{id:04}");
    event
}

pub(super) fn asked() -> AgentEventDto {
    event(
        "permission_request",
        "Permission requested for Write",
        json!({"tool_name": "Write", "options": [
            {"optionId": "no", "name": "Reject"},
            {"optionId": "yes", "name": "Allow"}
        ]}),
    )
}

/// The row a line of text is on, in a screen `rows` read.
pub(super) fn row_of(shown: &str, text: &str) -> Option<usize> {
    shown.lines().position(|line| line.contains(text))
}
