//! `ariadne attach` on a terminal: the inline console of [`ariadne_console`]
//! on the process terminal, driven from the daemon over HTTP.
//!
//! The pane, its transcript and its loop are ariadne-console's. What is here
//! is what only the CLI has. The terminal: [`Held`] is what takes raw mode
//! and gives it back, on every way out. And the daemon: [`frames`] is the
//! console stream as the loop's source, with the backoff and the redial when
//! it drops, and [`Posts`] is the console input and cancel endpoints as its
//! sink.

use std::io::{IsTerminal, Write};
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{
    DisableBracketedPaste, EnableBracketedPaste, EventStream, KeyboardEnhancementFlags,
    PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use futures_util::{Stream, stream};
use ratatui::backend::CrosstermBackend;
use tokio::time::{Instant, sleep_until};

use ariadne_api::sessions::{ConsoleInputRequest, SessionDto};
use ariadne_client::{Client, SseEvent, SseStream};
use ariadne_console::{Console, Frame, Header, Sink, drive, open};

use crate::commands::follow;
use crate::output::note;

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

    // The terminal is held before the viewport is opened, so a terminal that
    // cannot be opened at all is still given back: raw mode off, cursor shown.
    let held = Held::take(Raw::default())?;
    let mut terminal = open(|| CrosstermBackend::new(std::io::stdout()))?;
    let outcome = drive(
        &mut terminal,
        frames(client, id),
        &mut Posts { client, id },
        EventStream::new(),
        &mut console,
    )
    .await;
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

/// Where the daemon's console stream stands, between one frame and the next.
enum Link {
    /// Not dialled yet. A refusal here ends the console: it is how a daemon
    /// that was never there is heard of.
    First,
    Open(SseStream),
    /// Dropped: the wait before the next dial, and when it is due.
    Down {
        wait: Duration,
        at: Instant,
    },
    /// Nothing more: the first dial was refused, or a frame could not be read.
    Ended,
}

/// The daemon's console stream as the loop's source: the snapshot, then the
/// events, dialled again on the backoff every other follow uses whenever it
/// drops.
///
/// A drop is one [`Frame::Dropped`]. The wait doubles between refused
/// redials, and the fresh stream's snapshot is what says it is back. The
/// stream never ends on its own: a daemon that stays away is dialled for as
/// long as the console is open.
fn frames<'a>(client: &'a Client, id: &'a str) -> impl Stream<Item = Result<Frame>> + 'a {
    stream::unfold(Link::First, move |link| step(client, id, link))
}

/// The next frame of the stream, and where it stands after it.
async fn step(client: &Client, id: &str, mut link: Link) -> Option<(Result<Frame>, Link)> {
    let path = format!("/v1/sessions/{id}/console/stream");
    loop {
        link = match link {
            Link::First => match client.stream(&path).await {
                Ok(stream) => Link::Open(stream),
                Err(e) => return Some((Err(e.into()), Link::Ended)),
            },
            Link::Open(mut stream) => match stream.next().await {
                Some(Ok(frame)) => match frame_of(&frame) {
                    Ok(Some(frame)) => return Some((Ok(frame), Link::Open(stream))),
                    Ok(None) => Link::Open(stream),
                    Err(e) => return Some((Err(e), Link::Ended)),
                },
                Some(Err(_)) | None => {
                    let wait = follow::backoff(None);
                    let at = Instant::now() + wait;
                    return Some((Ok(Frame::Dropped), Link::Down { wait, at }));
                }
            },
            Link::Down { wait, at } => {
                sleep_until(at).await;
                match client.stream(&path).await {
                    Ok(stream) => Link::Open(stream),
                    Err(_) => {
                        let wait = follow::backoff(Some(wait));
                        let at = Instant::now() + wait;
                        Link::Down { wait, at }
                    }
                }
            }
            Link::Ended => return None,
        };
    }
}

/// The frame one event of the daemon's stream carries, and none for an event
/// of a kind the console does not read.
fn frame_of(frame: &SseEvent) -> Result<Option<Frame>> {
    Ok(match frame.event.as_str() {
        "snapshot" => Some(Frame::Snapshot(serde_json::from_str(&frame.data)?)),
        "event" => Some(Frame::Event(serde_json::from_str(&frame.data)?)),
        _ => None,
    })
}

/// The daemon's console input and cancel endpoints as the loop's sink.
struct Posts<'a> {
    client: &'a Client,
    id: &'a str,
}

impl Sink for Posts<'_> {
    async fn send(&mut self, text: String) -> Result<(), String> {
        self.client
            .send_no_content(
                http::Method::POST,
                &format!("/v1/sessions/{}/console/input", self.id),
                Some(&ConsoleInputRequest { text }),
            )
            .await
            .map_err(|e| e.human())
    }

    async fn cancel(&mut self) {
        // A turn that ended between the key and the post has nothing to
        // cancel, and says so with a 409: not a failure of the console.
        let _ = self
            .client
            .send_no_content::<()>(
                http::Method::POST,
                &format!("/v1/sessions/{}/console/cancel", self.id),
                None,
            )
            .await;
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

/// The process terminal: raw mode, bracketed paste — so a paste arrives as
/// one event and not as keys — and the key protocol that tells Shift+Enter
/// from Enter where the terminal can report it.
///
/// The modes are switched by escape sequences written to `out`, which is
/// stdout on the real terminal and a buffer in a test that reads what was
/// written on the way out.
pub struct Raw<W: Write = std::io::Stdout> {
    out: W,
    enhanced: bool,
}

impl Default for Raw {
    fn default() -> Self {
        Self::on(std::io::stdout())
    }
}

impl<W: Write> Raw<W> {
    fn on(out: W) -> Self {
        Self {
            out,
            enhanced: false,
        }
    }
}

impl<W: Write> Terminals for Raw<W> {
    fn enter(&mut self) -> Result<()> {
        crossterm::terminal::enable_raw_mode()?;
        // Nothing holds the terminal yet: a failure here gives raw mode
        // back itself, since no `leave` will.
        if let Err(e) = crossterm::execute!(self.out, EnableBracketedPaste) {
            let _ = crossterm::terminal::disable_raw_mode();
            return Err(e.into());
        }
        // Only a terminal that speaks the keyboard protocol can report
        // Shift+Enter at all; Alt+Enter is the newline everywhere else.
        self.enhanced = crossterm::terminal::supports_keyboard_enhancement().unwrap_or(false)
            && crossterm::execute!(
                self.out,
                PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
            )
            .is_ok();
        Ok(())
    }

    fn leave(&mut self) {
        if self.enhanced {
            let _ = crossterm::execute!(self.out, PopKeyboardEnhancementFlags);
        }
        let _ = crossterm::execute!(self.out, DisableBracketedPaste);
        let _ = crossterm::terminal::disable_raw_mode();
        let _ = crossterm::execute!(self.out, crossterm::cursor::Show);
        let _ = self.out.flush();
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
    use crossterm::event::{Event as TermEvent, KeyCode, KeyEvent, KeyModifiers};
    use futures_util::StreamExt;
    use ratatui::backend::TestBackend;
    use ratatui::{Terminal, TerminalOptions, Viewport};
    use serde_json::json;

    use ariadne_api::events::AgentEventDto;

    use super::*;

    /// A daemon console, as far as the link can tell: a snapshot and deltas
    /// on every stream it opens, and a record of every post.
    #[derive(Clone)]
    struct Stub {
        snapshot: Vec<AgentEventDto>,
        deltas: Vec<AgentEventDto>,
        /// Whether the stream stays open after its deltas, or ends — which
        /// is the drop a daemon restart is.
        stays_open: bool,
        prompts: Arc<Mutex<Vec<String>>>,
        cancels: Arc<AtomicUsize>,
        /// A session that will take no more input, which is the `409` a
        /// finished one gives.
        refuses: bool,
    }

    async fn open(
        State(stub): State<Stub>,
    ) -> Sse<impl futures_util::Stream<Item = Result<SseFrame, Infallible>>> {
        let snapshot = SseFrame::default()
            .event("snapshot")
            .data(serde_json::to_string(&stub.snapshot).unwrap());
        let deltas = stub.deltas.into_iter().map(|event| {
            Ok(SseFrame::default()
                .event("event")
                .data(serde_json::to_string(&event).unwrap()))
        });
        let rest = if stub.stays_open { usize::MAX } else { 0 };
        Sse::new(
            stream::once(async move { Ok(snapshot) })
                .chain(stream::iter(deltas))
                .chain(stream::pending().take(rest)),
        )
    }

    async fn input(State(stub): State<Stub>, Json(body): Json<ConsoleInputRequest>) -> StatusCode {
        stub.prompts.lock().unwrap().push(body.text);
        match stub.refuses {
            true => StatusCode::CONFLICT,
            false => StatusCode::NO_CONTENT,
        }
    }

    async fn cancel(State(stub): State<Stub>) -> StatusCode {
        stub.cancels.fetch_add(1, Ordering::SeqCst);
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

    fn stub(stays_open: bool, refuses: bool) -> Stub {
        Stub {
            snapshot: vec![event("agent_message", "hello")],
            deltas: vec![event("stop", "stopped")],
            stays_open,
            prompts: Arc::new(Mutex::new(Vec::new())),
            cancels: Arc::new(AtomicUsize::new(0)),
            refuses,
        }
    }

    fn event(kind: &str, summary: &str) -> AgentEventDto {
        AgentEventDto {
            id: format!("event-{kind}"),
            session_id: Some("session".into()),
            task_id: None,
            kind: kind.into(),
            payload: json!({}),
            summary: summary.into(),
            created_at: "2026-09-12T00:00:00Z".into(),
        }
    }

    /// What a frame reads as: the kinds a snapshot holds, an event's kind,
    /// or `dropped`.
    fn kind_of(frame: &Frame) -> String {
        match frame {
            Frame::Snapshot(events) => {
                let kinds: Vec<_> = events.iter().map(|event| event.kind.as_str()).collect();
                format!("snapshot:{}", kinds.join(","))
            }
            Frame::Event(event) => event.kind.clone(),
            Frame::Dropped => "dropped".into(),
        }
    }

    /// The daemon's stream ends after its deltas — a restart — and is dialled
    /// again after the backoff. Real time, and so the first backoff's second:
    /// a paused clock fires the client's own request timeout before the
    /// socket answers.
    #[tokio::test]
    async fn the_stream_is_read_as_frames_and_dialled_again_when_it_drops() {
        let (client, server) = serve(stub(false, false)).await;

        let frames: Vec<String> = frames(&client, "session")
            .take(6)
            .map(|frame| kind_of(&frame.unwrap()))
            .collect()
            .await;
        server.abort();

        assert_eq!(
            frames,
            [
                "snapshot:agent_message",
                "stop",
                "dropped",
                "snapshot:agent_message",
                "stop",
                "dropped"
            ]
        );
    }

    #[tokio::test]
    async fn a_first_dial_the_daemon_refuses_ends_the_stream_with_the_error() {
        let client = Client::tcp("http://127.0.0.1:1");

        let frames: Vec<Result<Frame>> = frames(&client, "session").collect().await;

        assert_eq!(frames.len(), 1, "{frames:?}");
        assert!(frames[0].is_err(), "{frames:?}");
    }

    #[tokio::test]
    async fn the_sink_posts_input_and_cancel_and_says_a_refusal() {
        let taking = stub(true, false);
        let (client, server) = serve(taking.clone()).await;
        let mut posts = Posts {
            client: &client,
            id: "session",
        };

        assert_eq!(posts.send("hi".into()).await, Ok(()));
        posts.cancel().await;
        server.abort();
        assert_eq!(*taking.prompts.lock().unwrap(), ["hi"]);
        assert_eq!(taking.cancels.load(Ordering::SeqCst), 1);

        let (client, server) = serve(stub(true, true)).await;
        let mut refused = Posts {
            client: &client,
            id: "session",
        };
        let outcome = refused.send("again".into()).await;
        server.abort();

        assert!(outcome.is_err(), "the 409 is said: {outcome:?}");
    }

    /// A sink that swallows what it gets, and a source with the empty
    /// transcript and nothing after it: enough to run the loop until a key
    /// ends it.
    struct Quiet;

    impl Sink for Quiet {
        async fn send(&mut self, _: String) -> Result<(), String> {
            Ok(())
        }

        async fn cancel(&mut self) {}
    }

    fn silence() -> impl Stream<Item = Result<Frame>> {
        stream::once(async { Ok(Frame::Snapshot(Vec::new())) }).chain(stream::pending())
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
        let ctrl_c = TermEvent::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        {
            let _held = Held::take(interrupted.clone()).unwrap();
            let mut terminal = Terminal::with_options(
                TestBackend::new(72, 40),
                TerminalOptions {
                    viewport: Viewport::Inline(12),
                },
            )
            .unwrap();
            let mut console = Console::new(Header::default());
            let keys = stream::iter(vec![Ok(ctrl_c.clone()), Ok(ctrl_c)]).chain(stream::pending());
            drive(
                &mut terminal,
                silence(),
                &mut Quiet,
                Box::pin(keys),
                &mut console,
            )
            .await
            .unwrap();
        }
        assert_eq!(interrupted.0.load(Ordering::SeqCst), 1, "Ctrl-C");
    }

    /// The sequence that turns bracketed paste off, as the terminal reads it.
    const PASTE_OFF: &[u8] = b"\x1b[?2004l";

    #[test]
    fn the_terminal_is_given_back_with_bracketed_paste_off() {
        let mut raw = Raw::on(Vec::new());

        raw.leave();

        assert!(
            raw.out
                .windows(PASTE_OFF.len())
                .any(|bytes| bytes == PASTE_OFF),
            "{:?}",
            String::from_utf8_lossy(&raw.out)
        );
    }

    #[test]
    fn only_a_terminal_on_both_ends_gets_the_inline_console() {
        assert!(interactive(true, true));
        assert!(!interactive(false, true));
        assert!(!interactive(true, false));
        assert!(!interactive(false, false));
    }
}
