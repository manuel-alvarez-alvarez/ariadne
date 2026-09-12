//! A session's console as a terminal, over a WebSocket.
//!
//! The desktop app shows the same console `ariadne attach` draws, in a
//! terminal emulator (xterm.js) rather than a pseudo-terminal. An emulator
//! takes bytes in and gives keys and a size out, so the daemon hosts the
//! console itself: [`ariadne_console`]'s loop runs in process, on a backend
//! that writes ANSI bytes into the socket and answers the cursor position
//! and the size from what it wrote and what the client said. The events come
//! from the same snapshot and live channels `GET /console/stream` serves,
//! and what is typed goes down the same path `POST /console/input` and
//! `/console/cancel` take, so a permission answer, a queued prompt and the
//! attention flag behave the same whichever console it was.
//!
//! The protocol is [`TerminalClientMessage`] in, as JSON text frames, and
//! binary frames of terminal bytes out, with a [`TerminalServerMessage`]
//! text frame for the session's status as the socket opens and as it ends.
//! The console lives as long as the socket: the client closing it ends the
//! console, and the session ending — or Ctrl-C twice, or Ctrl-D — flushes
//! the last bytes and closes the socket.

use std::io::{self, Write};

use anyhow::Result;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::response::Response;
use crossterm::event::{Event as TermEvent, KeyCode, KeyEvent, KeyModifiers};
use futures_util::stream::{SplitSink, SplitStream, unfold};
use futures_util::{SinkExt, Stream, StreamExt};
use tokio::sync::mpsc;

use ariadne_api::sessions::{
    TerminalClientMessage, TerminalKey, TerminalModifier, TerminalServerMessage,
};
use ariadne_console::{AnsiBackend, Console, Frame, Header, Sink, Window, drive, open};
use ariadne_core::SessionStatus;

use super::AppState;
use super::console::{self, Merge, open_console};
use super::convert::session_dto_of;
use super::error::ApiResult;

/// A session's console as a terminal.
///
/// Upgrades to a WebSocket. The client sends JSON text frames — a `resize`
/// with the terminal's columns and rows first and on every change, a `key`
/// per key press and a `paste` per pasted text (`TerminalClientMessage`) —
/// and reads binary frames of terminal bytes that draw the console, plus a
/// text frame carrying the session's status as the socket opens and as it
/// ends (`TerminalServerMessage`). Closing the socket ends the console and
/// leaves the session running; the session ending, Ctrl-C twice or Ctrl-D
/// close the socket.
#[utoipa::path(get, path = "/v1/sessions/{id}/console/terminal", tag = "sessions",
    operation_id = "console_terminal",
    params(("id" = String, Path, description = "session id")),
    responses((status = 101,
        description = "WebSocket. Client to server: JSON text frames, one \
                       `TerminalClientMessage` each — `resize` first, then `key` and \
                       `paste`. Server to client: binary frames of the terminal bytes \
                       that draw the console, and JSON text frames of \
                       `TerminalServerMessage`."),
        (status = 404)))]
pub async fn terminal(
    State(state): State<AppState>,
    Path(id): Path<String>,
    ws: WebSocketUpgrade,
) -> ApiResult<Response> {
    // A session that does not exist is a 404 before the upgrade, in the
    // envelope every other handler answers with.
    let session = state.store.get_session(&id).await?;
    let header = Header::of(Some(&session_dto_of(&state.store, session).await?));
    Ok(ws.on_upgrade(move |socket| async move {
        if let Err(error) = run(state, id.clone(), header, socket).await {
            tracing::warn!(session = %id, error = %format!("{error:#}"), "terminal console ended");
        }
    }))
}

/// Run the console on the socket until one end closes it.
async fn run(state: AppState, id: String, header: Header, socket: WebSocket) -> Result<()> {
    let (outbound, inbound) = socket.split();
    let window = Window::new(0, 0);
    let mut keys = Box::pin(events(inbound, window.clone()));

    // Nothing is drawn before the client says how big the terminal is: a
    // console laid out for a guessed size would be redrawn whole at once.
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

    let mut terminal = open(|| AnsiBackend::new(Pipe::new(bytes.clone()), window.clone()))?;
    let mut console = Console::new(header);
    let mut posts = Posts {
        state: state.clone(),
        id: id.clone(),
    };
    let outcome = drive(
        &mut terminal,
        frames(state.clone(), id.clone()),
        &mut posts,
        keys,
        &mut console,
    )
    .await;
    // Whatever ended it, the transcript goes out whole before the socket
    // closes: the scrollback is the emulator's, and it keeps what it read.
    let _ = console.close(&mut terminal);
    drop(terminal);
    let _ = bytes.send(status_frame(&state, &id).await);
    drop(bytes);
    let _ = forwarder.await;
    outcome
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
/// paste, a resize. The size is set on the window before the resize event
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

/// The console input and cancel paths as the loop's sink: the same code the
/// HTTP handlers run, so the refusals and the side effects are the same.
struct Posts {
    state: AppState,
    id: String,
}

impl Sink for Posts {
    async fn send(&mut self, text: String) -> Result<(), String> {
        console::take_input(&self.state, &self.id, text)
            .await
            .map_err(|error| error.body.error.message)
    }

    async fn cancel(&mut self) {
        // A turn that ended between the key and the cancel has nothing to
        // cancel, and says so with a conflict: not a failure of the console.
        let _ = console::cancel_turn(&self.state, &self.id).await;
    }
}

/// Where the source stands, between one frame and the next.
enum Link {
    /// Not subscribed yet, or subscribed again after a lag: the next frame
    /// is a fresh snapshot.
    Fresh,
    /// Subscribed: the stored and the live channels, read in id order.
    Open(Merge),
    Ended,
}

/// The session's console as the loop's source: the snapshot `GET /console`
/// answers, then every stored and live event `GET /console/stream` sends,
/// read from the same channels through the same [`Merge`], in id order.
///
/// There is no replay. A receiver that fell behind says the stream dropped
/// — `Frame::Dropped`, which the status line shows as reconnecting — and
/// subscribes again, and the fresh snapshot is what says it is back. The
/// channels closing is the daemon going away, and ends the source.
fn frames(state: AppState, id: String) -> impl Stream<Item = Result<Frame>> {
    unfold(Link::Fresh, move |link| {
        let state = state.clone();
        let id = id.clone();
        async move {
            match link {
                Link::Fresh => match open_console(&state, id).await {
                    Ok((snapshot, merge)) => {
                        Some((Ok(Frame::Snapshot(snapshot)), Link::Open(merge)))
                    }
                    Err(error) => Some((Err(error.into()), Link::Ended)),
                },
                Link::Open(mut merge) => match merge.next().await {
                    Some(Ok(dto)) => Some((Ok(Frame::Event(dto)), Link::Open(merge))),
                    Some(Err(_)) => Some((Ok(Frame::Dropped), Link::Fresh)),
                    None => None,
                },
                Link::Ended => None,
            }
        }
    })
}
