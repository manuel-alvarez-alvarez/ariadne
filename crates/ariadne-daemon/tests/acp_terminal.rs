//! Integration tests for a session's console served as a terminal:
//! `GET /v1/sessions/{id}/console/terminal`, a WebSocket that draws the
//! console `ariadne attach` draws, as bytes for a terminal emulator, and
//! reads the emulator's keys and size.
//!
//! The agent is the scriptable stub from test support (`common::acp`), the
//! same one `acp_console.rs` drives the console endpoints against. The
//! emulator is `vt100`: the bytes are fed to it, and what it shows is what a
//! person would see.

mod common;

use std::net::SocketAddr;
use std::time::Instant;

use axum::http::StatusCode;
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::{self, Message};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use ariadne_api::sessions::{
    TerminalClientMessage, TerminalKey, TerminalModifier, TerminalServerMessage,
};
use ariadne_core::{Actor, AttentionReason, PermissionMode, Seat, SessionStatus, TaskStatus};
use ariadne_store::{AgentPin, NewTask, NewTaskAgent};

use common::acp::{StubAcpAgent, registry_home, script, stub_acp_agent};
use common::{Cast, Harness, TIMEOUT, eventually, harness, post, post_json};

/// A task whose author runs on the registry agent `stub`, in a real repo —
/// the same fixture `acp_console.rs` casts.
async fn acp_cast(h: &Harness) -> Cast {
    h.git_repo("repo");
    let cast = h.cast_pinned("stub:test-model", 1).await;
    h.store
        .set_agent_pin(
            &cast.author.id,
            &AgentPin {
                model: "stub:test-model".into(),
                effort: Some("high".into()),
            },
        )
        .await
        .unwrap();
    cast
}

/// Spawn the task's author against the stub and wait for the initial turn to
/// end.
async fn spawned_idle(h: &Harness, cast: &Cast) -> ariadne_store::AgentSession {
    let session = h.launcher.spawn_author(&cast.task.id).await.unwrap();
    eventually(TIMEOUT, "the prompt round trip to end", || async {
        h.session_status(&session).await == SessionStatus::Idle
    })
    .await;
    session
}

/// A daemon on the stub, with one idle author session, served on a loopback
/// port: a WebSocket needs a connection, not a `oneshot`.
async fn idle_daemon() -> (
    tempfile::TempDir,
    StubAcpAgent,
    Harness,
    ariadne_store::AgentSession,
    SocketAddr,
) {
    let agent_dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(agent_dir.path(), script());
    let h = harness().home(registry_home(&stub)).await;
    let cast = acp_cast(&h).await;
    let session = spawned_idle(&h, &cast).await;
    let address = serve(&h).await;
    (agent_dir, stub, h, session, address)
}

async fn serve(h: &Harness) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = h.router.clone();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    address
}

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

async fn connect(address: SocketAddr, id: &str) -> Result<Socket, tungstenite::Error> {
    tokio_tungstenite::connect_async(format!("ws://{address}/v1/sessions/{id}/console/terminal"))
        .await
        .map(|(socket, _)| socket)
}

/// A request whose allowing and denying answers are distinct.
fn permission_script() -> serde_json::Value {
    let mut scripted = script();
    scripted["prompts"] = json!([{
        "permission": {
            "toolCall": {"toolCallId": "call-1", "title": "Write", "kind": "write",
                         "rawInput": {"path": "src/main.rs"}},
            "options": [
                {"optionId": "no", "name": "Reject", "kind": "reject_once"},
                {"optionId": "yes", "name": "Allow", "kind": "allow_once"},
            ],
        },
        "updates": [],
        "stop_reason": "end_turn",
    }]);
    scripted
}

/// A home whose configured ACP permission policy is read as the daemon would
/// read it, with the stub registered as the agent `stub`.
fn home_with_permission_mode(
    dir: &tempfile::TempDir,
    mode: &str,
    stub: &StubAcpAgent,
) -> std::path::PathBuf {
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(
        home.join("config.toml"),
        format!(
            "permission_mode = \"{mode}\"\n\n[[acp_agents]]\nid = \"stub\"\ncommand = [{:?}]\n",
            stub.bin
        ),
    )
    .unwrap();
    home
}

/// The top border of the input box, at a width: what a row of the screen
/// reads as when the console is drawn `cols` wide.
fn border(cols: usize) -> String {
    format!("┌{}┐", "─".repeat(cols - 2))
}

/// The client's end: the emulator the bytes draw on, and the socket they
/// come over.
struct Client {
    socket: Socket,
    parser: vt100::Parser,
    /// Every status the daemon said, in order.
    statuses: Vec<SessionStatus>,
}

impl Client {
    /// Connect and say the terminal's size, which is the first thing the
    /// daemon waits for.
    async fn open(address: SocketAddr, id: &str, cols: u16, rows: u16) -> Self {
        let mut client = Self {
            socket: connect(address, id).await.unwrap(),
            parser: vt100::Parser::new(rows, cols, 200),
            statuses: Vec::new(),
        };
        client.resize(cols, rows).await;
        client
    }

    async fn send(&mut self, message: TerminalClientMessage) {
        self.socket
            .send(Message::text(serde_json::to_string(&message).unwrap()))
            .await
            .unwrap();
    }

    async fn resize(&mut self, cols: u16, rows: u16) {
        self.parser.screen_mut().set_size(rows, cols);
        self.send(TerminalClientMessage::Resize { cols, rows })
            .await;
    }

    async fn key(&mut self, code: TerminalKey) {
        self.send(TerminalClientMessage::Key {
            code,
            modifiers: Vec::new(),
        })
        .await;
    }

    async fn ctrl(&mut self, character: char) {
        self.send(TerminalClientMessage::Key {
            code: TerminalKey::Char(character),
            modifiers: vec![TerminalModifier::Control],
        })
        .await;
    }

    /// Type `text` one key at a time, then Enter.
    async fn type_line(&mut self, text: &str) {
        for character in text.chars() {
            self.key(TerminalKey::Char(character)).await;
        }
        self.key(TerminalKey::Enter).await;
    }

    fn screen(&self) -> String {
        self.parser.screen().contents()
    }

    /// Read frames into the emulator until the screen satisfies `done`, or
    /// fail with what it shows.
    async fn read_until(&mut self, what: &str, done: impl Fn(&str) -> bool) {
        let deadline = Instant::now() + TIMEOUT;
        while !done(&self.screen()) {
            let left = deadline.saturating_duration_since(Instant::now());
            let frame = tokio::time::timeout(left, self.socket.next()).await;
            let Ok(frame) = frame else {
                panic!(
                    "timed out waiting for {what}; the screen:\n{}",
                    self.screen()
                );
            };
            match frame {
                Some(Ok(Message::Binary(bytes))) => self.parser.process(&bytes),
                Some(Ok(Message::Text(text))) => self.status(&text),
                Some(Ok(Message::Close(_))) | None => {
                    panic!(
                        "the socket closed waiting for {what}; the screen:\n{}",
                        self.screen()
                    );
                }
                Some(Ok(_)) => {}
                Some(Err(error)) => panic!("the socket failed waiting for {what}: {error}"),
            }
        }
    }

    fn status(&mut self, text: &str) {
        let TerminalServerMessage::Status { status } = serde_json::from_str(text).unwrap();
        self.statuses.push(status);
    }

    /// Read frames until the daemon closes the socket. `true` when it did
    /// within the patience of a test, `false` when it kept the socket open.
    async fn closed_by_the_daemon(&mut self) -> bool {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match tokio::time::timeout(left, self.socket.next()).await {
                Ok(Some(Ok(Message::Binary(bytes)))) => self.parser.process(&bytes),
                Ok(Some(Ok(Message::Text(text)))) => self.status(&text),
                Ok(Some(Ok(Message::Close(_))) | None) => return true,
                Ok(Some(Ok(_))) => {}
                // The daemon hung up without a close frame: closed all the same.
                Ok(Some(Err(_))) => return true,
                Err(_) => return false,
            }
        }
    }
}

/// The bytes draw the transcript — the tool call and the agent's text — and
/// the status line, laid out at the width the client said.
#[tokio::test]
async fn the_terminal_draws_the_transcript_and_the_status_line_at_the_client_size() {
    let (_dir, _stub, _h, session, address) = idle_daemon().await;
    let mut client = Client::open(address, &session.id, 100, 30).await;

    client
        .read_until("the transcript and the status line", |screen| {
            screen.contains("done") && screen.contains("idle")
        })
        .await;

    let screen = client.screen();
    assert!(screen.contains("Read"), "the tool call is drawn:\n{screen}");
    assert!(
        screen.contains("author stub:test-model"),
        "the status line names the seat and the model:\n{screen}"
    );
    assert!(
        screen.lines().any(|row| row == border(100)),
        "the input box spans the client's width:\n{screen}"
    );
    assert_eq!(client.statuses, [SessionStatus::Idle]);
}

/// Keys typed into the socket and Enter become a prompt the agent receives.
#[tokio::test]
async fn typed_keys_and_enter_reach_the_stub_agent_as_a_prompt() {
    let (_dir, stub, _h, session, address) = idle_daemon().await;
    let mut client = Client::open(address, &session.id, 100, 30).await;
    client
        .read_until("the console to be idle", |screen| screen.contains("idle"))
        .await;

    client.type_line("hello from the terminal").await;

    eventually(TIMEOUT, "the typed prompt to reach the agent", || async {
        stub.calls_of("session/prompt").len() == 2
    })
    .await;
    let prompts = stub.calls_of("session/prompt");
    assert!(
        prompts[1]["prompt"][0]["text"]
            .as_str()
            .unwrap()
            .contains("hello from the terminal"),
        "{prompts:?}"
    );
    client
        .read_until("the prompt on the transcript", |screen| {
            screen.contains("> hello from the terminal")
        })
        .await;
}

/// A pending permission question is drawn as the picker, and the keys that
/// choose an option answer it: the agent gets the option, and the flag comes
/// down.
#[tokio::test]
async fn a_key_answers_a_pending_permission_question() {
    let root = tempfile::tempdir().unwrap();
    let agent_dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(agent_dir.path(), permission_script());
    let h = harness()
        .home(home_with_permission_mode(&root, "auto", &stub))
        .await;
    let cast = acp_cast(&h).await;
    let task = h
        .store
        .create_task(NewTask {
            goal_id: cast.goal.id.clone(),
            repo_id: cast.repo.id.clone(),
            title: "Ask before writing".into(),
            description: "do things".into(),
            agents: vec![
                NewTaskAgent::new(Seat::Author, ["coding"], common::test_pin()),
                NewTaskAgent::new(Seat::Reviewer, ["code-review"], common::test_pin()),
            ],
            depends_on: vec![],
            landing: None,
            permission_mode: Some(PermissionMode::Ask),
        })
        .await
        .unwrap();
    h.store
        .transition_task(&task.id, TaskStatus::Ready, Actor::Daemon, None, None)
        .await
        .unwrap();
    let session = h.launcher.spawn_author(&task.id).await.unwrap();
    eventually(TIMEOUT, "the permission attention to rise", || async {
        h.attention(&session).await == Some(AttentionReason::WaitingPermission)
    })
    .await;
    let address = serve(&h).await;
    let mut client = Client::open(address, &session.id, 100, 30).await;
    client
        .read_until("the question to be drawn", |screen| {
            screen.contains("Allow") && screen.contains("Reject")
        })
        .await;

    // The second option is Allow: down once, then Enter.
    client.key(TerminalKey::Down).await;
    client.key(TerminalKey::Enter).await;

    eventually(TIMEOUT, "the answered turn to finish", || async {
        h.session_status(&session).await == SessionStatus::Idle
    })
    .await;
    assert_eq!(h.attention(&session).await, None);
    let reply = stub
        .messages()
        .into_iter()
        .find(|message| {
            message.get("id").and_then(serde_json::Value::as_str) == Some("permission-1")
        })
        .expect("the answer reached the agent");
    assert_eq!(reply["result"]["outcome"]["optionId"], "yes");
}

/// The client hanging up ends its console and nothing else: the session
/// takes input as before. The session ending is what closes a socket from
/// the daemon's side, after the last bytes and the final status.
#[tokio::test]
async fn closing_the_socket_leaves_the_session_alive_and_the_session_ending_closes_it() {
    let (_dir, stub, h, session, address) = idle_daemon().await;
    let mut client = Client::open(address, &session.id, 100, 30).await;
    client
        .read_until("the console to be idle", |screen| screen.contains("idle"))
        .await;

    client.socket.close(None).await.unwrap();

    assert!(h.agent_is_running(&session));
    assert_eq!(h.session_status(&session).await, SessionStatus::Idle);
    let (status, _) = h
        .send(post_json(
            &format!("/v1/sessions/{}/console/input", session.id),
            json!({ "text": "still here" }),
        ))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    eventually(
        TIMEOUT,
        "the session to take input after the close",
        || async { stub.calls_of("session/prompt").len() == 2 },
    )
    .await;

    let mut client = Client::open(address, &session.id, 100, 30).await;
    client
        .read_until("the console to be idle again", |screen| {
            screen.contains("still here")
        })
        .await;
    let (status, _) = h
        .send(post(&format!("/v1/sessions/{}/kill", session.id)))
        .await;
    assert_eq!(status, StatusCode::OK);

    assert!(
        client.closed_by_the_daemon().await,
        "the session ending closes the socket; the screen:\n{}",
        client.screen()
    );
    assert_eq!(
        client.statuses.last(),
        Some(&SessionStatus::Exited),
        "{:?}",
        client.statuses
    );
}

/// A socket opened after the session ended draws the transcript there was,
/// and closes on it: the end is in the opening snapshot, and nothing more
/// will come.
#[tokio::test]
async fn a_socket_opened_after_the_session_ended_gets_the_transcript_and_closes() {
    let (_dir, _stub, h, session, address) = idle_daemon().await;
    let (status, _) = h
        .send(post(&format!("/v1/sessions/{}/kill", session.id)))
        .await;
    assert_eq!(status, StatusCode::OK);
    eventually(TIMEOUT, "the session to be exited", || async {
        h.session_status(&session).await == SessionStatus::Exited
    })
    .await;

    let mut client = Client::open(address, &session.id, 100, 30).await;

    assert!(
        client.closed_by_the_daemon().await,
        "the ended session closes the socket; the screen:\n{}",
        client.screen()
    );
    let screen = client.screen();
    assert!(
        screen.contains("done"),
        "the transcript was drawn:\n{screen}"
    );
    assert_eq!(
        client.statuses,
        [SessionStatus::Exited, SessionStatus::Exited]
    );
}

/// Ctrl-C twice, or Ctrl-D, leave the console: the daemon closes the socket
/// and the session stays alive.
#[tokio::test]
async fn ctrl_c_twice_or_ctrl_d_closes_the_socket_and_the_session_stays_alive() {
    let (_dir, _stub, h, session, address) = idle_daemon().await;

    let mut client = Client::open(address, &session.id, 100, 30).await;
    client
        .read_until("the console to be idle", |screen| screen.contains("idle"))
        .await;
    client.ctrl('c').await;
    client.ctrl('c').await;
    assert!(client.closed_by_the_daemon().await, "Ctrl-C twice");
    assert!(h.agent_is_running(&session));

    let mut client = Client::open(address, &session.id, 100, 30).await;
    client
        .read_until("the console to be idle", |screen| screen.contains("idle"))
        .await;
    client.ctrl('d').await;
    assert!(client.closed_by_the_daemon().await, "Ctrl-D");
    assert!(h.agent_is_running(&session));
    assert_eq!(h.session_status(&session).await, SessionStatus::Idle);
}

/// A resize sent after the first draw redraws the console at the new size.
#[tokio::test]
async fn a_resize_redraws_at_the_new_size() {
    let (_dir, _stub, _h, session, address) = idle_daemon().await;
    let mut client = Client::open(address, &session.id, 100, 30).await;
    client
        .read_until("the console at 100 columns", |screen| {
            screen.contains("idle") && screen.lines().any(|row| row == border(100))
        })
        .await;

    client.resize(60, 20).await;

    client
        .read_until("the console redrawn at 60 columns", |screen| {
            screen.contains("idle") && screen.lines().any(|row| row == border(60))
        })
        .await;
}

/// An id that names no session is refused as any other read of it is, before
/// any upgrade.
#[tokio::test]
async fn a_terminal_for_an_unknown_session_is_refused_before_the_upgrade() {
    let h = harness().await;
    let address = serve(&h).await;

    let refused = connect(address, "no-such-session").await;

    match refused {
        Err(tungstenite::Error::Http(response)) => {
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
        }
        other => panic!("expected a 404 before the upgrade, got {other:?}"),
    }
}

#[tokio::test]
async fn the_terminal_endpoint_is_in_the_openapi_document() {
    let h = harness().await;
    let doc: serde_json::Value = h.get("/api-docs/openapi.json").await;
    assert!(doc["paths"]["/v1/sessions/{id}/console/terminal"]["get"].is_object());
}
