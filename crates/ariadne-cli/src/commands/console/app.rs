//! `ariadne attach` on a terminal: the full-screen console of
//! [`ariadne_console::app`] on the process terminal, driven from the
//! daemon's trajectory stream (040).
//!
//! The screen, its keys and its loop are ariadne-console's. What is here is
//! what only the CLI has. The terminal: [`Screen`] takes the alternate
//! screen, raw mode and the mouse, and [`Held`] gives them back on every way
//! out. And the daemon: [`frames`] is the trajectory stream as the loop's
//! source, dialled again when it drops or skips a revision, and [`Posts`]
//! is the session's routes as its host.

use std::io::Write;
use std::time::Duration;

use anyhow::Result;
use base64::Engine;
use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    EventStream, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags,
    PushKeyboardEnhancementFlags,
};
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen};
use futures_util::{Stream, stream};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use tokio::time::{Instant, sleep_until};

use ariadne_api::events::AgentEventDto;
use ariadne_api::sessions::{AnswerQuestionRequest, ConsoleInputRequest, QuestionDto};
use ariadne_api::trajectory::{
    StepDto, ToolCallDetailDto, TrajectoryItemQuery, TrajectoryStreamQuery,
};
use ariadne_client::{Client, SseStream};
use ariadne_console::app::{App, Frame, Host, drive};
use ariadne_console::trajectory::Printer;

use super::terminal::{Held, Terminals};
use crate::commands::{follow, transcript};
use crate::output::note;

/// Open the full-screen console on the process terminal.
pub(super) async fn attach(client: &Client, id: &str) -> Result<()> {
    let mut app = App::new(id);
    // The terminal is held before the screen is drawn, so a terminal that
    // cannot be drawn on at all is still given back.
    let held = Held::take(Screen::default())?;
    let mut terminal = Terminal::new(CrosstermBackend::new(std::io::stdout()))?;
    let outcome = drive(
        &mut terminal,
        frames(client, id),
        &mut Posts { client, id },
        EventStream::new(),
        &mut app,
    )
    .await;
    drop(terminal);
    drop(held);

    // The alternate screen took its lines with it: the turns this attach
    // showed go into the scrollback, folded, on the main screen.
    print!("{}", app.farewell(&Printer::new(chrono::Local)));
    let _ = std::io::stdout().flush();
    note(&farewell(&app, id));
    outcome
}

/// What is said as the console closes: that the session is still running,
/// or that it has ended.
fn farewell(app: &App, id: &str) -> String {
    match app.has_ended() {
        true => format!("the session has ended; revive it with: ariadne session resume {id}"),
        false => format!(
            "left the console — the session is still running; attach again with: ariadne attach {id}"
        ),
    }
}

/// Where the trajectory stream stands, between one frame and the next.
enum Link {
    /// Not dialled yet. A refusal here ends the console.
    First,
    /// Open, and the revision of the last frame it gave.
    Open(SseStream, Option<u64>),
    /// Dropped: the wait before the next dial, and when it is due.
    Down { wait: Duration, at: Instant },
    /// Nothing more: the first dial was refused, or a frame could not be read.
    Ended,
}

/// The daemon's trajectory stream as the loop's source: the snapshot, then
/// each revision, dialled again on the backoff of every other follow.
///
/// A drop, a `resync` and an update that does not follow the last revision
/// are each one [`Frame::Dropped`], and the fresh stream's snapshot is what
/// says it is back.
fn frames<'a>(client: &'a Client, id: &'a str) -> impl Stream<Item = Result<Frame>> + 'a {
    stream::unfold(Link::First, move |link| step(client, id, link))
}

/// A dropped stream: the frame that says so, and the wait before the dial.
fn dropped(wait: Option<Duration>) -> Option<(Result<Frame>, Link)> {
    let wait = follow::backoff(wait);
    let at = Instant::now() + wait;
    Some((Ok(Frame::Dropped), Link::Down { wait, at }))
}

/// The next frame of the stream, and where it stands after it.
async fn step(client: &Client, id: &str, mut link: Link) -> Option<(Result<Frame>, Link)> {
    let query = TrajectoryStreamQuery {
        limit: Some(transcript::TURN_PAGE),
    };
    loop {
        link = match link {
            Link::First => match client.trajectory_stream(id, &query).await {
                Ok(stream) => Link::Open(stream, None),
                Err(e) => return Some((Err(e.into()), Link::Ended)),
            },
            Link::Open(mut stream, rev) => match stream.next().await {
                Some(Ok(event)) => match super::frame(&event) {
                    Ok(super::Frame::Snapshot(snapshot)) => {
                        let rev = Some(snapshot.rev);
                        let history = if snapshot.has_older {
                            prompt_history(client, id).await
                        } else {
                            None
                        };
                        return Some((
                            Ok(Frame::Snapshot(snapshot, history)),
                            Link::Open(stream, rev),
                        ));
                    }
                    Ok(super::Frame::Update(update))
                        if rev.is_some_and(|rev| update.rev == rev + 1) =>
                    {
                        let rev = Some(update.rev);
                        return Some((Ok(Frame::Update(update)), Link::Open(stream, rev)));
                    }
                    Ok(super::Frame::Update(_) | super::Frame::Resync) => return dropped(None),
                    Ok(super::Frame::Other) => Link::Open(stream, rev),
                    Err(e) => return Some((Err(e), Link::Ended)),
                },
                Some(Err(_)) | None => return dropped(None),
            },
            Link::Down { wait, at } => {
                sleep_until(at).await;
                match client.trajectory_stream(id, &query).await {
                    Ok(stream) => Link::Open(stream, None),
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

/// Use the inline console's event window when the trajectory omits older turns.
async fn prompt_history(client: &Client, id: &str) -> Option<Vec<String>> {
    let events: Vec<AgentEventDto> = client
        .get_json(&format!("/v1/sessions/{id}/console"))
        .await
        .ok()?;
    Some(
        events
            .into_iter()
            .filter(|event| event.kind == "user_prompt_submit")
            .filter(|event| event.payload["source"].as_str() == Some("console"))
            .filter_map(|event| event.payload["text"].as_str().map(str::to_owned))
            .collect(),
    )
}

/// The session's routes as the console's host.
struct Posts<'a> {
    client: &'a Client,
    id: &'a str,
}

impl Host for Posts<'_> {
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

    async fn answer(
        &mut self,
        question_id: &str,
        answer: AnswerQuestionRequest,
    ) -> Result<(), String> {
        self.client
            .send_no_content(
                http::Method::POST,
                &format!("/v1/sessions/{}/questions/{question_id}/answer", self.id),
                Some(&answer),
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

    async fn steps(&mut self, turn_id: &str) -> Result<Vec<StepDto>, String> {
        transcript::all_steps(self.client, self.id, turn_id)
            .await
            .map_err(|e| format!("the steps of the turn could not be read: {e}"))
    }

    async fn subagent_steps(
        &mut self,
        turn_id: &str,
        subagent_id: &str,
    ) -> Result<Vec<StepDto>, String> {
        transcript::all_steps_of(self.client, self.id, turn_id, Some(subagent_id))
            .await
            .map_err(|e| format!("the steps of the subagent could not be read: {e}"))
    }

    async fn detail(&mut self, step_id: &str, raw: bool) -> Result<ToolCallDetailDto, String> {
        self.client
            .trajectory_item(self.id, step_id, &TrajectoryItemQuery { raw })
            .await
            .map_err(|e| format!("the detail of the step could not be read: {}", e.human()))
    }

    async fn questions(&mut self) -> Result<Vec<QuestionDto>, String> {
        self.client
            .get_json(&format!("/v1/sessions/{}/questions", self.id))
            .await
            .map_err(|e| e.human())
    }

    fn copy(&mut self, text: String) {
        let mut out = std::io::stdout();
        let _ = out.write_all(osc52(&text).as_bytes());
        let _ = out.flush();
    }
}

/// The sequence that puts `text` on the terminal's clipboard: OSC 52, which
/// reaches the clipboard of the machine the terminal runs on, over SSH too.
fn osc52(text: &str) -> String {
    format!(
        "\x1b]52;c;{}\x07",
        base64::engine::general_purpose::STANDARD.encode(text)
    )
}

/// The process terminal as the full-screen console takes it: raw mode, the
/// alternate screen, the mouse, bracketed paste, and the key protocol that
/// tells Shift+Enter from Enter where the terminal can report it.
///
/// The modes are switched by escape sequences written to `out`, which is
/// stdout on the real terminal and a buffer in a test.
pub(super) struct Screen<W: Write = std::io::Stdout> {
    out: W,
    enhanced: bool,
}

impl Default for Screen {
    fn default() -> Self {
        Self::on(std::io::stdout())
    }
}

impl<W: Write> Screen<W> {
    fn on(out: W) -> Self {
        Self {
            out,
            enhanced: false,
        }
    }

    /// Write the modes the console needs, raw mode aside.
    fn modes(&mut self) -> std::io::Result<()> {
        crossterm::execute!(
            self.out,
            EnterAlternateScreen,
            EnableMouseCapture,
            EnableBracketedPaste
        )
    }
}

impl<W: Write> Terminals for Screen<W> {
    fn enter(&mut self) -> Result<()> {
        crossterm::terminal::enable_raw_mode()?;
        self.modes()?;
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
        let _ = crossterm::execute!(self.out, DisableMouseCapture);
        let _ = crossterm::execute!(self.out, LeaveAlternateScreen);
        let _ = crossterm::terminal::disable_raw_mode();
        let _ = crossterm::execute!(self.out, crossterm::cursor::Show);
        let _ = self.out.flush();
    }
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use axum::Router;
    use axum::extract::State;
    use axum::response::sse::{Event as SseFrame, Sse};
    use axum::routing::get;
    use crossterm::event::{Event as TermEvent, KeyCode, KeyEvent, KeyModifiers};
    use futures_util::StreamExt;
    use ratatui::backend::TestBackend;
    use serde_json::json;

    use super::*;

    /// The sequences that take the alternate screen and the mouse, and give
    /// them back.
    const ALTERNATE_ON: &[u8] = b"\x1b[?1049h";
    const ALTERNATE_OFF: &[u8] = b"\x1b[?1049l";
    const MOUSE_ON: &[u8] = b"\x1b[?1000h";
    const MOUSE_OFF: &[u8] = b"\x1b[?1000l";
    const PASTE_OFF: &[u8] = b"\x1b[?2004l";

    fn holds(bytes: &[u8], sequence: &[u8]) -> bool {
        bytes
            .windows(sequence.len())
            .any(|window| window == sequence)
    }

    #[test]
    fn the_console_takes_the_alternate_screen_and_the_mouse_and_gives_both_back() {
        let mut screen = Screen::on(Vec::new());
        screen.modes().unwrap();
        assert!(holds(&screen.out, ALTERNATE_ON), "{:?}", screen.out);
        assert!(holds(&screen.out, MOUSE_ON), "{:?}", screen.out);

        let mut screen = Screen::on(Vec::new());
        screen.leave();
        for sequence in [ALTERNATE_OFF, MOUSE_OFF, PASTE_OFF] {
            assert!(
                holds(&screen.out, sequence),
                "{sequence:?} in {:?}",
                String::from_utf8_lossy(&screen.out)
            );
        }
    }

    /// A terminal that only counts how often it was given back.
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

    /// A host that takes everything and reads nothing.
    struct Quiet;

    impl Host for Quiet {
        async fn send(&mut self, _: String) -> Result<(), String> {
            Ok(())
        }

        async fn answer(&mut self, _: &str, _: AnswerQuestionRequest) -> Result<(), String> {
            Ok(())
        }

        async fn cancel(&mut self) {}

        async fn steps(&mut self, _: &str) -> Result<Vec<StepDto>, String> {
            Ok(Vec::new())
        }

        async fn questions(&mut self) -> Result<Vec<QuestionDto>, String> {
            Ok(Vec::new())
        }

        fn copy(&mut self, _: String) {}
    }

    fn snapshot(rev: u64) -> serde_json::Value {
        json!({
            "rev": rev,
            "session": {
                "status": "running", "attention_reason": null, "running_turn_id": null,
                "title": null, "mode": null, "config_options": [], "context": null,
                "usage": {"input_tokens": 0, "cached_input_tokens": 0, "output_tokens": 0},
                "cost": null, "commands": []
            },
            "turns": [], "has_older": false, "subagents": [], "plans": []
        })
    }

    #[tokio::test]
    async fn the_terminal_is_given_back_when_ctrl_c_leaves_the_console_and_on_an_error() {
        let left = Recorder::default();
        let ctrl_c = TermEvent::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        {
            let _held = Held::take(left.clone()).unwrap();
            let mut terminal = Terminal::new(TestBackend::new(40, 12)).unwrap();
            let source = stream::once(async {
                Ok(Frame::Snapshot(
                    Box::new(serde_json::from_value(snapshot(1)).unwrap()),
                    None,
                ))
            })
            .chain(stream::pending());
            let keys = stream::iter([Ok(ctrl_c.clone()), Ok(ctrl_c)]).chain(stream::pending());
            drive(
                &mut terminal,
                source,
                &mut Quiet,
                Box::pin(keys),
                &mut App::new("s1"),
            )
            .await
            .unwrap();
        }
        assert_eq!(left.0.load(Ordering::SeqCst), 1, "Ctrl-C");

        let failed = Recorder::default();
        let outcome: Result<()> = async {
            let _held = Held::take(failed.clone())?;
            let mut terminal = Terminal::new(TestBackend::new(40, 12)).unwrap();
            let source = stream::iter([Err(anyhow::anyhow!("the daemon went away"))]);
            drive(
                &mut terminal,
                source,
                &mut Quiet,
                Box::pin(stream::pending()),
                &mut App::new("s1"),
            )
            .await
        }
        .await;
        assert!(outcome.is_err());
        assert_eq!(failed.0.load(Ordering::SeqCst), 1, "an error");
    }

    #[tokio::test]
    async fn the_terminal_is_given_back_when_setup_or_drive_panics() {
        use futures_util::FutureExt;
        use std::panic::{AssertUnwindSafe, catch_unwind};

        struct PanicDuringSetup(Recorder);
        impl Terminals for PanicDuringSetup {
            fn enter(&mut self) -> Result<()> {
                panic!("after changing terminal modes");
            }

            fn leave(&mut self) {
                self.0.leave();
            }
        }

        let setup = Recorder::default();
        assert!(catch_unwind(|| Held::take(PanicDuringSetup(setup.clone()))).is_err());

        let running = Recorder::default();
        let outcome = AssertUnwindSafe(async {
            let _held = Held::take(running.clone()).unwrap();
            let mut terminal = Terminal::new(TestBackend::new(40, 12)).unwrap();
            let source = stream::poll_fn(|_| panic!("while driving the console"));
            drive(
                &mut terminal,
                source,
                &mut Quiet,
                Box::pin(stream::pending()),
                &mut App::new("s1"),
            )
            .await
        })
        .catch_unwind()
        .await;
        assert!(outcome.is_err());
        assert_eq!(running.0.load(Ordering::SeqCst), 1, "drive panic");
        assert_eq!(setup.0.load(Ordering::SeqCst), 1, "setup panic");
    }

    #[test]
    fn terminal_restore_continues_after_a_failed_write() {
        #[derive(Default)]
        struct FailFirstWrite(Vec<u8>, bool);

        impl Write for FailFirstWrite {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                if !std::mem::replace(&mut self.1, true) {
                    return Err(std::io::Error::other("first write failed"));
                }
                self.0.write(bytes)
            }

            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let mut screen = Screen::on(FailFirstWrite::default());
        screen.leave();
        assert!(holds(&screen.out.0, MOUSE_OFF));
        assert!(holds(&screen.out.0, ALTERNATE_OFF));
    }

    #[tokio::test]
    async fn prompt_history_reaches_past_the_trajectory_page() {
        let history = (1..=51)
            .map(|n| {
                json!({
                    "id": n.to_string(),
                    "kind": "user_prompt_submit",
                    "payload": {"source": "console", "text": format!("prompt {n}")},
                    "summary": "", "created_at": ""
                })
            })
            .collect::<Vec<_>>();
        let router = Router::new()
            .route(
                "/v1/sessions/s1/trajectory/stream",
                get(|| async {
                    let mut first = snapshot(1);
                    first["has_older"] = json!(true);
                    Sse::new(stream::iter([Ok::<_, Infallible>(
                        SseFrame::default()
                            .event("snapshot")
                            .data(first.to_string()),
                    )]))
                }),
            )
            .route(
                "/v1/sessions/s1/console",
                get(move || async { axum::Json(history) }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let client = Client::tcp(format!("http://{address}"));
        let mut source = Box::pin(frames(&client, "s1"));
        let mut console = App::new("s1");
        console.frame(source.next().await.unwrap().unwrap());
        server.abort();

        for _ in 0..51 {
            console.key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        }
        assert!(matches!(
            console.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            ariadne_console::app::Action::Send(text) if text == "prompt 1"
        ));
    }

    /// The host reads a step's detail from the detail route, raw where the
    /// console asks for it, and a subagent's steps from the steps route of
    /// its turn, by its lane.
    #[tokio::test]
    async fn the_host_reads_a_detail_and_the_steps_of_a_subagent_from_their_routes() {
        let asked: Arc<Mutex<Vec<String>>> = Arc::default();
        let seen = asked.clone();
        let router = Router::new()
            .route(
                "/v1/sessions/s1/trajectory/items/c1",
                get(move |request: axum::extract::Request| async move {
                    seen.lock()
                        .unwrap()
                        .push(request.uri().query().unwrap_or_default().to_string());
                    axum::Json(json!({
                        "id": "c1", "tool_call_id": "c1", "title": "Run", "kind": "execute",
                        "status": "completed", "detail_rev": 2,
                        "detail": {"type": "command", "command": "ls", "cwd": null,
                                   "output": "a", "exit_code": 0, "truncated": false}
                    }))
                }),
            )
            .route(
                "/v1/sessions/s1/trajectory/turns/t1/steps",
                get(|request: axum::extract::Request| async move {
                    let query = request.uri().query().unwrap_or_default().to_string();
                    let steps = match query.contains("subagent=sub1") {
                        true => vec![json!({
                            "type": "note", "id": "n1", "turn_id": "t1", "lane": "sub1",
                            "created_at": "2026-11-27T10:00:00Z", "origin": "x",
                            "severity": "info", "text": "inside"
                        })],
                        false => vec![],
                    };
                    axum::Json(json!({"steps": steps, "earlier_steps": 0}))
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let client = Client::tcp(format!("http://{address}"));
        let mut host = Posts {
            client: &client,
            id: "s1",
        };

        let detail = host.detail("c1", true).await.unwrap();
        let steps = host.subagent_steps("t1", "sub1").await.unwrap();
        server.abort();

        assert_eq!(detail.detail_rev, 2);
        assert_eq!(*asked.lock().unwrap(), vec!["raw=true".to_string()]);
        assert_eq!(steps.len(), 1);
    }

    #[test]
    fn y_puts_the_text_on_the_clipboard_through_osc_52() {
        assert_eq!(osc52("hi"), "\x1b]52;c;aGk=\x07");
    }

    /// A daemon that sends a snapshot, then an update of each of `revs`,
    /// on every stream it opens, and counts the streams.
    #[derive(Clone)]
    struct Stub {
        revs: Vec<u64>,
        opened: Arc<Mutex<usize>>,
    }

    async fn trajectory(
        State(stub): State<Stub>,
    ) -> Sse<impl futures_util::Stream<Item = Result<SseFrame, Infallible>>> {
        *stub.opened.lock().unwrap() += 1;
        let first = SseFrame::default()
            .event("snapshot")
            .data(snapshot(1).to_string());
        let updates = stub.revs.into_iter().map(|rev| {
            Ok(SseFrame::default()
                .event("update")
                .data(json!({"rev": rev, "ops": []}).to_string()))
        });
        Sse::new(
            stream::once(async move { Ok(first) })
                .chain(stream::iter(updates))
                .chain(stream::pending()),
        )
    }

    /// What a frame reads as.
    fn kind_of(frame: &Frame) -> String {
        match frame {
            Frame::Snapshot(snapshot, _) => format!("snapshot {}", snapshot.rev),
            Frame::Update(update) => format!("update {}", update.rev),
            Frame::Dropped => "dropped".into(),
        }
    }

    /// An update that skips a revision drops the stream, and the stream is
    /// dialled again after the backoff. Real time, as the inline console's
    /// test of its own redial.
    #[tokio::test]
    async fn an_update_that_skips_a_revision_dials_the_stream_again() {
        let stub = Stub {
            revs: vec![2, 4],
            opened: Arc::new(Mutex::new(0)),
        };
        let app = Router::new()
            .route("/v1/sessions/s1/trajectory/stream", get(trajectory))
            .with_state(stub.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = Client::tcp(format!("http://{address}"));

        let frames: Vec<String> = frames(&client, "s1")
            .take(4)
            .map(|frame| kind_of(&frame.unwrap()))
            .collect()
            .await;
        server.abort();

        assert_eq!(frames, ["snapshot 1", "update 2", "dropped", "snapshot 1"]);
        assert_eq!(*stub.opened.lock().unwrap(), 2);
    }

    #[test]
    fn the_console_closing_says_whether_the_session_still_runs() {
        let mut ended = snapshot(1);
        ended["session"]["status"] = json!("exited");
        let mut gone = App::new("s1");
        gone.frame(Frame::Snapshot(
            Box::new(serde_json::from_value(ended).unwrap()),
            None,
        ));
        let mut live = App::new("s1");
        live.frame(Frame::Snapshot(
            Box::new(serde_json::from_value(snapshot(1)).unwrap()),
            None,
        ));

        assert!(farewell(&live, "s1").contains("still running"));
        assert!(farewell(&gone, "s1").contains("has ended"));
    }
}
