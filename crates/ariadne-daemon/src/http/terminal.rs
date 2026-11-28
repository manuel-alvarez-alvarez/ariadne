//! A session's console as a terminal, over a WebSocket.
//!
//! The desktop app shows the same full-screen console `ariadne attach
//! --full-screen` draws, in a terminal emulator (xterm.js) rather than a
//! pseudo-terminal. An emulator takes bytes in and gives keys, mouse events
//! and a size out, so the daemon hosts the console itself:
//! [`ariadne_console::app`]'s loop runs in process, on a backend that writes
//! ANSI bytes into the socket and answers the size from what the client
//! said. The trajectory comes from the hub `GET /trajectory/stream` serves,
//! and what is typed goes down the same paths `POST /console/input`,
//! `/console/cancel` and `/questions/{id}/answer` take, so a permission
//! answer, a queued prompt and the attention flag behave the same whichever
//! console it was.
//!
//! The protocol is [`TerminalClientMessage`] in, as JSON text frames, and
//! binary frames of terminal bytes out, with a [`TerminalServerMessage`]
//! text frame for the session's status as the socket opens and as it ends.
//! The console takes the alternate screen and the mouse as it opens, and
//! gives both back as it closes, with the turns it showed written to the
//! main screen. The console lives as long as the socket: the client closing
//! it ends the console, and the session ending — or Ctrl-C twice, or Ctrl-D
//! — flushes the last bytes and closes the socket.

use std::io::{self, Write};

use anyhow::Result;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::response::Response;
use base64::Engine;
use crossterm::event::{
    Event as TermEvent, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use futures_util::stream::{SplitSink, SplitStream, unfold};
use futures_util::{SinkExt, Stream, StreamExt};
use tokio::sync::mpsc;

use ariadne_api::sessions::{
    AnswerQuestionRequest, QuestionDto, TerminalClientMessage, TerminalKey, TerminalModifier,
    TerminalMouse, TerminalServerMessage,
};
use ariadne_api::trajectory::{
    StepDto, TrajectoryQuery, TrajectorySnapshotDto, TrajectoryStepsQuery,
};
use ariadne_console::ansi::{FULL_SCREEN_OFF, FULL_SCREEN_ON, TITLE_RESTORE, title};
use ariadne_console::app::{App, Frame, HISTORY_PAGE, Host, drive, reach_back, session_name};
use ariadne_console::trajectory::Printer;
use ariadne_console::{AnsiBackend, Window};
use ariadne_core::SessionStatus;

use super::AppState;
use super::console;
use super::error::ApiResult;
use super::trajectory::{self, Subscription};
use crate::http::convert::session_dto_of;
use crate::trajectory::step_id;

/// How many turns the console's snapshot carries: the most a hub gives.
const TURNS: usize = 50;

/// How many steps of a turn one read asks for: the most a route gives.
const STEP_PAGE: u32 = 200;

/// A session's full-screen console as a terminal.
///
/// Upgrades to a WebSocket. The client sends JSON text frames — a `resize`
/// with the terminal's columns and rows first and on every change, a `key`
/// per key press, a `paste` per pasted text and a `mouse` per wheel step or
/// click (`TerminalClientMessage`) — and reads binary frames of terminal
/// bytes that draw the console, plus a text frame carrying the session's
/// status as the socket opens and as it ends (`TerminalServerMessage`).
/// Closing the socket ends the console and leaves the session running; the
/// session ending, Ctrl-C twice or Ctrl-D close the socket.
#[utoipa::path(get, path = "/v1/sessions/{id}/console/terminal", tag = "sessions",
    operation_id = "console_terminal",
    params(("id" = String, Path, description = "session id")),
    responses((status = 101,
        description = "WebSocket. Client to server: JSON text frames, one \
                       `TerminalClientMessage` each — `resize` first, then `key`, \
                       `paste` and `mouse`. Server to client: binary frames of the \
                       terminal bytes that draw the full-screen console, and JSON text \
                       frames of `TerminalServerMessage`."),
        (status = 404)))]
pub(super) async fn terminal(
    State(state): State<AppState>,
    Path(id): Path<String>,
    ws: WebSocketUpgrade,
) -> ApiResult<Response> {
    // A session that does not exist is a 404 before the upgrade, in the
    // envelope every other handler answers with.
    state.store.get_session(&id).await?;
    Ok(ws.on_upgrade(move |socket| async move {
        if let Err(error) = run(state, id.clone(), socket).await {
            tracing::warn!(session = %id, error = %format!("{error:#}"), "terminal console ended");
        }
    }))
}

/// Run the console on the socket until one end closes it.
async fn run(state: AppState, id: String, socket: WebSocket) -> Result<()> {
    let (outbound, inbound) = socket.split();
    let window = Window::new(0, 0);
    let mut keys = Box::pin(events(inbound, window.clone()));

    // Nothing is drawn before the client says how big the terminal is: a
    // console laid out for a guessed size would be drawn again at once.
    loop {
        match keys.next().await {
            Some(Ok(TermEvent::Resize(..))) => break,
            Some(Ok(_)) => {}
            Some(Err(_)) | None => return Ok(()),
        }
    }

    let (bytes, drain) = mpsc::unbounded_channel();
    let forwarder = tokio::spawn(forward(drain, outbound));
    let _ = bytes.send(status_frame(&state, &id).await);
    let _ = bytes.send(Message::binary(FULL_SCREEN_ON.to_vec()));
    let name = name_of(&state, &id).await;
    if !name.is_empty() {
        let _ = bytes.send(Message::binary(title(&name)));
    }

    let mut terminal = AnsiBackend::full_screen(Pipe::new(bytes.clone()), window.clone())?;
    let mut app = App::new(&id);
    app.named(&name);
    let mut posts = Posts {
        state: state.clone(),
        id: id.clone(),
        bytes: bytes.clone(),
    };
    let outcome = drive(
        &mut terminal,
        frames(state.clone(), id.clone()),
        &mut posts,
        keys,
        &mut app,
    )
    .await;
    // Whatever ended it, the screen is given back and the turns it showed go
    // to the main screen before the socket closes: the scrollback is the
    // emulator's, and it keeps what it read.
    drop(terminal);
    let _ = bytes.send(Message::binary(FULL_SCREEN_OFF.to_vec()));
    if !name.is_empty() {
        let _ = bytes.send(Message::binary(TITLE_RESTORE.to_vec()));
    }
    let farewell = app.farewell(&Printer::new(chrono::Local));
    let _ = bytes.send(Message::binary(farewell.replace('\n', "\r\n")));
    let _ = bytes.send(status_frame(&state, &id).await);
    drop(posts);
    drop(bytes);
    let _ = forwarder.await;
    outcome
}

/// The name of the session, as `ariadne session ls` shows it and the
/// console of `ariadne attach` names it: the title behind it, its seat and
/// its step. A session the store does not know has none.
async fn name_of(state: &AppState, id: &str) -> String {
    let Ok(session) = state.store.get_session(id).await else {
        return String::new();
    };
    let title = crate::acp_sessions::title_of(state, &session).await;
    match session_dto_of(&state.store, session).await {
        Ok(row) => session_name(title.as_deref(), row.seat, row.step.as_deref()),
        Err(_) => String::new(),
    }
}

/// The session's status, as the text frame that says it.
async fn status_frame(state: &AppState, id: &str) -> Message {
    let status = match state.store.get_session(id).await {
        Ok(session) => session.status(),
        Err(_) => SessionStatus::Exited,
    };
    let message = TerminalServerMessage::Status { status };
    Message::text(serde_json::to_string(&message).unwrap_or_default())
}

/// Send every frame the console wrote, then close the socket.
async fn forward(
    mut drain: mpsc::UnboundedReceiver<Message>,
    mut outbound: SplitSink<WebSocket, Message>,
) {
    while let Some(message) = drain.recv().await {
        if outbound.send(message).await.is_err() {
            return;
        }
    }
    let _ = outbound.send(Message::Close(None)).await;
    let _ = outbound.flush().await;
}

/// The writer the backend draws into: a buffer, sent as one binary frame on
/// every flush.
struct Pipe {
    bytes: mpsc::UnboundedSender<Message>,
    pending: Vec<u8>,
}

impl Pipe {
    fn new(bytes: mpsc::UnboundedSender<Message>) -> Self {
        Self {
            bytes,
            pending: Vec::new(),
        }
    }
}

impl Write for Pipe {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.pending.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.pending.is_empty() {
            return Ok(());
        }
        let frame = Message::binary(std::mem::take(&mut self.pending));
        self.bytes
            .send(frame)
            .map_err(|_| io::Error::other("the terminal socket is closed"))
    }
}

impl Drop for Pipe {
    fn drop(&mut self) {
        let _ = self.flush();
    }
}

/// The client's messages as the events the console loop reads: a key, a
/// paste, a mouse event, a resize. The size is set on the window before the resize event
/// goes out, so the redraw it causes is at the new size. The socket closing,
/// or a frame that is not a message of the protocol, ends the stream — which
/// ends the console.
fn events(
    inbound: SplitStream<WebSocket>,
    window: Window,
) -> impl Stream<Item = io::Result<TermEvent>> {
    inbound.filter_map(move |message| {
        let window = window.clone();
        async move {
            let text = match message {
                Ok(Message::Text(text)) => text,
                Ok(Message::Binary(_) | Message::Ping(_) | Message::Pong(_)) => return None,
                Ok(Message::Close(_)) => {
                    return Some(Err(io::Error::other("the client closed the socket")));
                }
                Err(error) => return Some(Err(io::Error::other(error))),
            };
            let message: TerminalClientMessage = match serde_json::from_str(&text) {
                Ok(message) => message,
                Err(error) => return Some(Err(io::Error::other(error))),
            };
            Some(Ok(match message {
                TerminalClientMessage::Resize { cols, rows } => {
                    // An emulator not yet laid out reports no rows at all,
                    // and ratatui makes room under a viewport on a terminal
                    // of no rows for ever: the smallest terminal is one cell.
                    let (cols, rows) = (cols.max(1), rows.max(1));
                    window.set(cols, rows);
                    TermEvent::Resize(cols, rows)
                }
                TerminalClientMessage::Key { code, modifiers } => {
                    TermEvent::Key(KeyEvent::new(key_code(code), key_modifiers(&modifiers)))
                }
                TerminalClientMessage::Paste { text } => TermEvent::Paste(text),
                TerminalClientMessage::Mouse { kind, col, row } => TermEvent::Mouse(MouseEvent {
                    kind: match kind {
                        TerminalMouse::WheelUp => MouseEventKind::ScrollUp,
                        TerminalMouse::WheelDown => MouseEventKind::ScrollDown,
                        TerminalMouse::Click => MouseEventKind::Down(MouseButton::Left),
                    },
                    column: col,
                    row,
                    modifiers: KeyModifiers::NONE,
                }),
            }))
        }
    })
}

fn key_code(key: TerminalKey) -> KeyCode {
    match key {
        TerminalKey::Char(c) => KeyCode::Char(c),
        TerminalKey::F(n) => KeyCode::F(n),
        TerminalKey::Enter => KeyCode::Enter,
        TerminalKey::Backspace => KeyCode::Backspace,
        TerminalKey::Tab => KeyCode::Tab,
        TerminalKey::BackTab => KeyCode::BackTab,
        TerminalKey::Esc => KeyCode::Esc,
        TerminalKey::Left => KeyCode::Left,
        TerminalKey::Right => KeyCode::Right,
        TerminalKey::Up => KeyCode::Up,
        TerminalKey::Down => KeyCode::Down,
        TerminalKey::Home => KeyCode::Home,
        TerminalKey::End => KeyCode::End,
        TerminalKey::PageUp => KeyCode::PageUp,
        TerminalKey::PageDown => KeyCode::PageDown,
        TerminalKey::Delete => KeyCode::Delete,
        TerminalKey::Insert => KeyCode::Insert,
    }
}

fn key_modifiers(modifiers: &[TerminalModifier]) -> KeyModifiers {
    modifiers
        .iter()
        .map(|modifier| match modifier {
            TerminalModifier::Shift => KeyModifiers::SHIFT,
            TerminalModifier::Control => KeyModifiers::CONTROL,
            TerminalModifier::Alt => KeyModifiers::ALT,
            TerminalModifier::Super => KeyModifiers::SUPER,
            TerminalModifier::Hyper => KeyModifiers::HYPER,
            TerminalModifier::Meta => KeyModifiers::META,
        })
        .fold(KeyModifiers::NONE, |all, one| all | one)
}

/// The session's routes as the console's host: the same code the HTTP
/// handlers run, so the refusals and the side effects are the same.
struct Posts {
    state: AppState,
    id: String,
    /// The socket's frames, which the clipboard is written to.
    bytes: mpsc::UnboundedSender<Message>,
}

impl Host for Posts {
    async fn send(&mut self, text: String) -> Result<(), String> {
        console::take_input(&self.state, &self.id, text)
            .await
            .map_err(|error| error.body.error.message)
    }

    async fn answer(
        &mut self,
        question_id: &str,
        answer: AnswerQuestionRequest,
    ) -> Result<(), String> {
        super::questions::take_answer(&self.state, &self.id, question_id, &answer)
            .await
            .map_err(|error| error.body.error.message)
    }

    async fn cancel(&mut self) {
        // A turn that ended between the key and the cancel has nothing to
        // cancel, and says so with a conflict: not a failure of the console.
        let _ = console::cancel_turn(&self.state, &self.id).await;
    }

    async fn steps(&mut self, turn_id: &str) -> Result<Vec<StepDto>, String> {
        let mut steps = Vec::new();
        let mut before = None;
        loop {
            let query = TrajectoryStepsQuery {
                before,
                limit: Some(STEP_PAGE),
                subagent: None,
            };
            let page = trajectory::steps(
                State(self.state.clone()),
                Path((self.id.clone(), turn_id.to_string())),
                Query(query),
            )
            .await
            .map_err(|error| {
                format!(
                    "the steps of the turn could not be read: {}",
                    error.body.error.message
                )
            })?;
            let page = page.0;
            let Some(first) = page.steps.first() else {
                break;
            };
            before = Some(step_id(first).to_string());
            steps.splice(0..0, page.steps);
            if page.earlier_steps == 0 {
                break;
            }
        }
        Ok(steps)
    }

    async fn questions(&mut self) -> Result<Vec<QuestionDto>, String> {
        Ok(self.state.launcher.acp.open_questions(&self.id).await)
    }

    fn copy(&mut self, text: String) {
        // OSC 52 reaches the clipboard of the machine the emulator runs on.
        let sequence = format!(
            "\x1b]52;c;{}\x07",
            base64::engine::general_purpose::STANDARD.encode(text)
        );
        let _ = self.bytes.send(Message::binary(sequence));
    }
}

/// Where the source stands, between one frame and the next.
enum Link {
    /// Not subscribed yet, or subscribed again after a lag: the next frame
    /// is a fresh snapshot.
    Fresh,
    /// Subscribed, and the revision of the last frame it gave.
    Open(Box<Subscription>, u64),
    Ended,
}

/// The session's trajectory as the loop's source: the snapshot the hub
/// opens a subscriber on, then each revision, as `GET /trajectory/stream`
/// sends them.
///
/// There is no replay. A receiver that fell behind, or an update that does
/// not follow the last revision, says the stream dropped — `Frame::Dropped`,
/// which the footer shows as reconnecting — and subscribes again, and the
/// fresh snapshot is what says it is back. The hub ending is the daemon
/// going away, and ends the source.
fn frames(state: AppState, id: String) -> impl Stream<Item = Result<Frame>> {
    unfold(Link::Fresh, move |link| {
        let state = state.clone();
        let id = id.clone();
        async move {
            match link {
                Link::Fresh => match state.trajectories.subscribe(&state, &id, TURNS).await {
                    Ok(mut subscription) => {
                        let snapshot = subscription
                            .opening()
                            .expect("a subscription opens on a snapshot");
                        let rev = snapshot.rev;
                        let history = match snapshot.has_older {
                            true => Some(prompt_history(&state, &id, &snapshot).await),
                            false => None,
                        };
                        Some((
                            Ok(Frame::Snapshot(Box::new(snapshot), history)),
                            Link::Open(Box::new(subscription), rev),
                        ))
                    }
                    Err(error) => {
                        Some((Err(anyhow::anyhow!(error.body.error.message)), Link::Ended))
                    }
                },
                Link::Open(mut subscription, rev) => match subscription.revision().await {
                    Some(Ok(update)) if update.rev == rev + 1 => {
                        let rev = update.rev;
                        Some((
                            Ok(Frame::Update((*update).clone())),
                            Link::Open(subscription, rev),
                        ))
                    }
                    Some(Ok(_) | Err(_)) => Some((Ok(Frame::Dropped), Link::Fresh)),
                    None => None,
                },
                Link::Ended => None,
            }
        }
    })
}

/// The prompts typed at a console, read back through the pages of turns
/// older than the snapshot's.
async fn prompt_history(
    state: &AppState,
    id: &str,
    snapshot: &TrajectorySnapshotDto,
) -> Vec<String> {
    reach_back(snapshot, |before| async move {
        let query = TrajectoryQuery {
            before: Some(before),
            limit: Some(HISTORY_PAGE),
            subagent: None,
        };
        trajectory::snapshot(State(state.clone()), Path(id.to_string()), Query(query))
            .await
            .ok()
            .map(|page| page.0)
    })
    .await
}
