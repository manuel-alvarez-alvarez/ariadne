//! The webhook tunnel against a stand-in smee.io server (027).
//!
//! The stand-in is a python3 program, as the stub agent is. It streams each
//! channel as server-sent events: `ready`, a `ping` every 50 ms, and each
//! delivery posted to the channel, re-serialized the way smee.io does it —
//! the body parsed and written back as compact JSON, beside the lowercased
//! headers and the query.
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::common::forge::{Invocation, StubForgeCli, answer, stub_forge_cli};
use crate::common::{
    Harness, QUIET, RUNS_OUT, TIMEOUT, eventually, git_repo_copy, harness, next_event, post_json,
    put_json, sh, store_copy,
};
use ariadne_api::repositories::{ForgeTunnelDto, TunnelState};
use ariadne_daemon::forge::tunnel::Tunnel;
use ariadne_daemon::timeouts::Timeouts;
use ariadne_daemon::webhooks::WebhookListen;
use axum::http::StatusCode;
use serde_json::{Value, json};

const STAND_IN: &str = r#"
import json, os, queue, socketserver, sys, threading, time
from http.server import BaseHTTPRequestHandler
from urllib.parse import parse_qsl, urlsplit

state, api_port = sys.argv[1], int(sys.argv[2])
parent = os.getppid()
streams, lock = {}, threading.Lock()

def orphaned():
    # Ends with the test process, however that one ends.
    while os.getppid() == parent:
        time.sleep(0.2)
    os._exit(0)

def log(name, line):
    with open(os.path.join(state, name), "a") as f:
        f.write(line + "\n")

class Api(BaseHTTPRequestHandler):
    def do_GET(self):
        channel = urlsplit(self.path).path.strip("/")
        log("requests.log", channel)
        # The test holds the answer while this file exists.
        while os.path.exists(os.path.join(state, "hold")):
            time.sleep(0.02)
        events = queue.Queue()
        with lock:
            streams.setdefault(channel, []).append(events)
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()
        try:
            self.wfile.write(b"event: ready\ndata: {}\n\n")
            self.wfile.flush()
            while True:
                try:
                    frame = "data: %s\n\n" % events.get(timeout=0.05)
                except queue.Empty:
                    # The test mutes the pings while this file exists.
                    if os.path.exists(os.path.join(state, "mute")):
                        continue
                    frame = "event: ping\ndata: {}\n\n"
                self.wfile.write(frame.encode())
                self.wfile.flush()
        except OSError:
            log("closed.log", channel)
        finally:
            with lock:
                streams[channel].remove(events)

    def do_POST(self):
        url = urlsplit(self.path)
        raw = self.rfile.read(int(self.headers.get("Content-Length", 0)))
        delivery = {name.lower(): value for name, value in self.headers.items()}
        delivery["body"] = json.loads(raw)
        delivery["query"] = dict(parse_qsl(url.query))
        delivery["timestamp"] = int(time.time() * 1000)
        # JSON.stringify's output: compact, and UTF-8 left unescaped.
        data = json.dumps(delivery, separators=(",", ":"), ensure_ascii=False)
        with lock:
            for events in streams.get(url.path.strip("/"), []):
                events.put(data)
        self.send_response(200)
        self.send_header("Content-Length", "2")
        self.end_headers()
        self.wfile.write(b"OK")

    def log_message(self, *args):
        pass

class Server(socketserver.ThreadingMixIn, socketserver.TCPServer):
    # Not `http.server.HTTPServer`: it looks up the name of its host as it
    # binds, and that lookup takes 35 s on a GitHub macOS runner.
    daemon_threads = True
    allow_reuse_address = True

api = Server(("127.0.0.1", api_port), Api)
threading.Thread(target=orphaned, daemon=True).start()
ports = os.path.join(state, "ports.json")
with open(ports + ".tmp", "w") as f:
    json.dump({"api": api.server_address[1]}, f)
os.replace(ports + ".tmp", ports)
api.serve_forever()
"#;

/// A running stand-in server. Dropping it kills it.
struct StandIn {
    child: tokio::process::Child,
    dir: PathBuf,
    api: u16,
}

impl StandIn {
    /// Start the stand-in in `dir`, on `api` or on a free port where 0.
    async fn start(dir: &Path, api: u16) -> Self {
        std::fs::create_dir_all(dir).unwrap();
        let script = dir.join("stand-in.py");
        std::fs::write(&script, STAND_IN).unwrap();
        let ports = dir.join("ports.json");
        let _ = std::fs::remove_file(&ports);
        let child = tokio::process::Command::new("python3")
            .arg(&script)
            .arg(dir)
            .arg(api.to_string())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        eventually(TIMEOUT, "the stand-in to bind", async || ports.exists()).await;
        let bound: Value = serde_json::from_str(&std::fs::read_to_string(&ports).unwrap()).unwrap();
        Self {
            child,
            dir: dir.to_path_buf(),
            api: bound["api"].as_u64().unwrap() as u16,
        }
    }

    /// The server goes away: every socket of it closes.
    async fn stop(mut self) {
        self.child.kill().await.unwrap();
    }

    /// The channel of each stream opened, in order.
    fn requests(&self) -> Vec<String> {
        std::fs::read_to_string(self.dir.join("requests.log"))
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    /// How many streams the client has closed.
    fn closed(&self) -> usize {
        std::fs::read_to_string(self.dir.join("closed.log"))
            .unwrap_or_default()
            .lines()
            .count()
    }

    /// Hold each stream's answer until [`StandIn::release`].
    fn hold(&self) {
        std::fs::write(self.dir.join("hold"), "").unwrap();
    }

    fn release(&self) {
        std::fs::remove_file(self.dir.join("hold")).unwrap();
    }

    /// Stop the pings: an open stream that says nothing.
    fn mute(&self) {
        std::fs::write(self.dir.join("mute"), "").unwrap();
    }

    fn host(&self) -> String {
        format!("http://127.0.0.1:{}", self.api)
    }

    fn url(&self, channel: &str) -> String {
        format!("{}/{channel}", self.host())
    }
}

/// The hook URL of repository `id` on a channel at `url`.
fn hook(url: &str, id: &str) -> String {
    format!("{url}?route=/webhooks/github/{id}")
}

/// The last call of each fetch: it reads the open issues.
const ISSUES: [&str; 2] = ["api", "repos/acme/widgets/issues?state=open&per_page=100"];

/// Two gates each fetch passes: one in front of its list of requests,
/// one in front of its read of issues. A call that meets a shut gate stands
/// there until the gate opens.
///
/// The stub logs a call before it reads its gate, so a gate shut while a
/// call may be passing it leaves the test unsure where that call stands.
/// The test therefore shuts a gate only while the fetch stands at the
/// other gate, or while no fetch can start a call, and always knows where
/// the fetch stands.
struct Gates {
    lists: PathBuf,
    issues: PathBuf,
    _dir: tempfile::TempDir,
}

impl Gates {
    fn new(open: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let gates = Self {
            lists: dir.path().join("lists"),
            issues: dir.path().join("issues"),
            _dir: dir,
        };
        if open {
            gates.open();
        }
        gates
    }

    /// Both gates open: every fetch runs through.
    fn opened() -> Self {
        Self::new(true)
    }

    /// Both gates shut: the first fetch stands at its first list.
    fn shut() -> Self {
        Self::new(false)
    }

    fn open(&self) {
        std::fs::write(&self.lists, "").unwrap();
        std::fs::write(&self.issues, "").unwrap();
    }

    /// Shut both gates while no fetch can start a call: the hook is live,
    /// so only a wake starts one, and every call has its answer. Answers
    /// the calls so far.
    fn close(&self, stub: &StubForgeCli) -> usize {
        let calls = stub.invocations().len();
        assert_eq!(stub.completed(), calls, "no call under way");
        std::fs::remove_file(&self.lists).unwrap();
        std::fs::remove_file(&self.issues).unwrap();
        calls
    }
}

fn script(list: &[Value], gates: &Gates) -> Value {
    let mut lists = answer(&["pr", "list"], 0, &serde_json::to_string(list).unwrap());
    lists["wait_for"] = json!(gates.lists);
    let mut issues = answer(&ISSUES, 0, "[]");
    issues["wait_for"] = json!(gates.issues);
    json!([
        answer(&["auth", "status"], 0, ""),
        answer(&["api", "user"], 0, "me"),
        lists,
        issues,
        answer(&["api"], 0, r#"{"id":17}"#),
    ])
}

fn pull(title: &str, review: &str) -> Value {
    json!({"number": 1, "url": "https://github.com/acme/widgets/pull/1",
        "title": title, "author": {"login": "me"}, "state": "OPEN", "isDraft": false,
        "headRefName": "fix", "headRefOid": "abc", "headRepository": {"url": "https://github.com/acme/widgets"},
        "baseRefName": "main", "statusCheckRollup": [], "reviewDecision": review, "comments": [],
        "createdAt": "2026-10-01T00:00:00Z"})
}

/// A home whose `config.toml` holds `toml`.
fn home(toml: &str) -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    std::fs::write(home.path().join("config.toml"), toml).unwrap();
    home
}

async fn daemon(home: &Path, stub: &StubForgeCli, timeouts: Timeouts) -> (Harness, WebhookListen) {
    let h = harness()
        .home(home.to_path_buf())
        .forge_cli(stub)
        .timeouts(timeouts)
        .await;
    let listener =
        WebhookListen::bind(&h.launcher.cfg, h.store.clone(), h.state.forge_poll.clone())
            .await
            .unwrap();
    h.state.tunnel.start(&listener);
    // A fresh database starts the switch off (027 rule 11); this file is
    // about the tunnel itself, so every test starts the way a user who
    // wants one would, by turning it on, and a test proving the off default
    // flips it back itself.
    let _: ForgeTunnelDto = h
        .json(
            put_json("/v1/forge/tunnel", json!({"enabled": true})),
            StatusCode::OK,
        )
        .await;
    (h, listener)
}

/// Poll and debounce timings that tests do not mean to exercise. A test that
/// does names its shorter timeout at the call site instead.
fn ordinary_timeouts() -> Timeouts {
    Timeouts {
        forge_wake_debounce: RUNS_OUT / 10,
        ..Timeouts::default()
    }
}

async fn enable(h: &Harness) -> String {
    let path = h.git_repo("tunnel");
    sh(
        &path,
        "git remote add origin https://github.com/acme/widgets.git",
    );
    let repo: Value = h
        .json(
            post_json(
                "/v1/repositories",
                json!({"path": path, "forge": {"enabled": true}}),
            ),
            StatusCode::CREATED,
        )
        .await;
    repo["id"].as_str().unwrap().into()
}

/// A repository with no forge, whose edits are unrelated to the tunnel.
async fn other_repository(h: &Harness) -> String {
    let path = h.git_repo("other");
    let repo: Value = h
        .json(
            post_json("/v1/repositories", json!({"path": path})),
            StatusCode::CREATED,
        )
        .await;
    repo["id"].as_str().unwrap().into()
}

/// Fetches so far: each one lists the open requests once.
fn fetches(stub: &StubForgeCli) -> usize {
    stub.invocations()
        .iter()
        .filter(|call| call.args.starts_with(&["pr".into(), "list".into()]))
        .count()
}

/// Whether a call is a hook request: an `api` call on a `hooks` path.
fn is_hook_call(call: &Invocation) -> bool {
    call.args.first().is_some_and(|a| a == "api")
        && call.args.get(1).is_some_and(|path| path.contains("/hooks"))
}

/// Every hook request.
fn hook_calls(stub: &StubForgeCli) -> usize {
    stub.invocations()
        .iter()
        .filter(|call| is_hook_call(call))
        .count()
}

/// Fetches that began after the last hook request.
fn fetches_after_the_last_hook_call(stub: &StubForgeCli) -> usize {
    let calls = stub.invocations();
    let last = calls
        .iter()
        .rposition(is_hook_call)
        .expect("a hook request");
    calls[last..]
        .iter()
        .filter(|call| call.args.starts_with(&["pr".into(), "list".into()]))
        .count()
}

/// Listen for [`QUIET`]: the fetch count stays at `count`. A fetch that never
/// comes publishes nothing to wait for, so only its absence proves it.
async fn quiet(stub: &StubForgeCli, count: usize) {
    assert!(
        tokio::time::timeout(QUIET, async {
            loop {
                assert_eq!(fetches(stub), count);
                // The pace of the check inside the QUIET listen.
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .is_err()
    );
}

async fn tunnel(h: &Harness) -> ForgeTunnelDto {
    h.get("/v1/forge/tunnel").await
}

/// Whether a call is a fetch's last: the read of the open issues.
fn is_issues_call(call: &Invocation) -> bool {
    call.args.starts_with(&ISSUES.map(String::from))
}

/// Whether a call is a list of requests.
fn is_list_call(call: &Invocation) -> bool {
    call.args.starts_with(&["pr".into(), "list".into()])
}

/// Wait for a call that `matches` after the first `since` calls.
async fn call_after(
    stub: &StubForgeCli,
    since: usize,
    what: &str,
    matches: fn(&Invocation) -> bool,
) {
    eventually(TIMEOUT, what, async || {
        stub.invocations().iter().skip(since).any(matches)
    })
    .await;
}

/// Let the fetch that stands at the lists gate run to the issues gate, then
/// on, and wait for the next fetch to stand at the lists gate. Nothing
/// woke the worker, so the timer started that fetch.
async fn timer_fetch(stub: &StubForgeCli, gates: &Gates) {
    let since = stub.invocations().len();
    std::fs::write(&gates.lists, "").unwrap();
    // The issues gate is shut: the fetch stands there.
    call_after(stub, since, "the fetch at the issues gate", is_issues_call).await;
    std::fs::remove_file(&gates.lists).unwrap();
    let since = stub.invocations().len();
    std::fs::write(&gates.issues, "").unwrap();
    // The lists gate is shut: the next fetch stands there.
    call_after(stub, since, "the timer's fetch", is_list_call).await;
    std::fs::remove_file(&gates.issues).unwrap();
}

/// Wait for the hook to go live at `url`, and for the one fetch after it.
///
/// A fetch stands at the shut lists gate from before the hook can go live,
/// under way on its one list of open requests. The catch-up wake therefore
/// finds it running, and coalesces into exactly one fetch after it (026).
/// Answers the fetches so far.
async fn live_at(h: &Harness, stub: &StubForgeCli, gates: &Gates, id: &str, url: &str) -> usize {
    let hook = hook(url, id);
    eventually(TIMEOUT, "the hook to go live at the tunnel", async || {
        let row = h.store.forge_integration(id).await.unwrap().unwrap();
        row.webhook_state == "live" && row.webhook_url.as_deref() == Some(hook.as_str())
    })
    .await;
    let calls = stub.invocations();
    let since_issues = calls.iter().rposition(is_issues_call).map_or(0, |i| i + 1);
    assert!(
        calls[since_issues..]
            .iter()
            .filter(|c| is_list_call(c))
            .count()
            == 1
            && stub.completed() == calls.len() - 1,
        "a fetch stands at the lists gate while the hook goes live"
    );
    // The held fetch has listed its open requests, so it counts already.
    let done = fetches(stub);
    let answered = calls.iter().filter(|c| is_issues_call(c)).count();
    gates.open();
    eventually(
        TIMEOUT,
        "the held fetch and the catch-up fetch",
        async || {
            let calls = stub.invocations();
            fetches(stub) == done + 1
                && calls.iter().filter(|c| is_issues_call(c)).count() == answered + 2
                && stub.completed() == calls.len()
        },
    )
    .await;
    // `WakeOnly` runs no timer fetch, and nothing the test waits for later
    // would show one: listen for it.
    quiet(stub, done + 1).await;
    let calls = stub.invocations();
    let last_hook = calls
        .iter()
        .rposition(is_hook_call)
        .expect("a hook request");
    assert_eq!(
        calls[last_hook..]
            .iter()
            .filter(|c| is_issues_call(c))
            .count(),
        2,
        "the held fetch, and one catch-up fetch"
    );
    done + 1
}

/// A compact body with non-ASCII text, as a forge sends it: smee.io writes
/// it back byte for byte, so its signature holds.
const COMPACT: &str = r#"{"action":"created","comment":{"body":"café ✓"}}"#;

/// Post a delivery of `body` to `url`, as the forge would, signed with the
/// integration's secret.
async fn deliver(h: &Harness, id: &str, url: &str, body: &str) -> StatusCode {
    let secret = h
        .store
        .forge_integration(id)
        .await
        .unwrap()
        .unwrap()
        .webhook_secret
        .unwrap();
    deliver_signed(&secret, url, body).await
}

/// Post a delivery of `body` to `url`, signed with `secret`.
async fn deliver_signed(secret: &str, url: &str, body: &str) -> StatusCode {
    // Python provides an independent signature implementation.
    let output = std::process::Command::new("python3")
        .args([
            "-c",
            "import hmac,hashlib,sys;print('sha256='+hmac.new(sys.argv[1].encode(),sys.argv[2].encode(),hashlib.sha256).hexdigest())",
            secret,
            body,
        ])
        .output()
        .unwrap();
    let signature = String::from_utf8(output.stdout).unwrap().trim().to_string();
    reqwest::Client::new()
        .post(url)
        .header("Content-Type", "application/json")
        .header("X-Hub-Signature-256", signature)
        .header("X-Github-Event", "issue_comment")
        .body(body.to_owned())
        .send()
        .await
        .unwrap()
        .status()
}

async fn channel(h: &Harness) -> String {
    eventually(TIMEOUT, "a stored channel", async || {
        h.store
            .forge_settings()
            .await
            .unwrap()
            .tunnel_channel
            .is_some()
    })
    .await;
    h.store
        .forge_settings()
        .await
        .unwrap()
        .tunnel_channel
        .unwrap()
}

#[tokio::test]
async fn a_tunnel_to_the_listener_registers_the_hook_and_a_delivery_through_it_fetches_once() {
    let dir = tempfile::tempdir().unwrap();
    let server = StandIn::start(dir.path(), 0).await;
    let home = home(&format!("tunnel_host = \"{}\"\n", server.host()));
    let gates = Gates::shut();
    let stub = stub_forge_cli(script(&[], &gates));
    let (h, listener) = daemon(home.path(), &stub, ordinary_timeouts()).await;
    server.hold();
    let id = enable(&h).await;
    call_after(&stub, 0, "the first fetch at the lists gate", is_list_call).await;
    server.release();
    let channel = channel(&h).await;
    let count = live_at(&h, &stub, &gates, &id, &server.url(&channel)).await;
    assert_eq!(server.requests(), std::slice::from_ref(&channel));
    let create = stub
        .invocations()
        .into_iter()
        .find(|c| c.args.iter().any(|a| a == "POST"))
        .unwrap();
    let body: Value = serde_json::from_str(create.input.as_deref().unwrap()).unwrap();
    assert_eq!(body["config"]["url"], hook(&server.url(&channel), &id));
    let state = tunnel(&h).await;
    assert!(state.enabled);
    assert_eq!(state.state, TunnelState::Up);
    assert_eq!(state.url, Some(server.url(&channel)));
    assert_eq!(state.listen, Some(listener.address().to_string()));
    assert_ne!(listener.address().port(), 0);

    // smee.io writes a spaced body back compact: the signature over the
    // forge's bytes no longer holds, and the listener refuses it.
    let url = hook(&server.url(&channel), &id);
    let spaced = r#"{"action": "created"}"#;
    assert_eq!(deliver(&h, &id, &url, spaced).await, StatusCode::OK);
    quiet(&stub, count).await;
    let row = h.store.forge_integration(&id).await.unwrap().unwrap();
    assert_eq!(row.webhook_last_delivery_at, None);

    assert_eq!(deliver(&h, &id, &url, COMPACT).await, StatusCode::OK);
    eventually(TIMEOUT, "the delivery's fetch", async || {
        fetches(&stub) == count + 1
    })
    .await;
    quiet(&stub, count + 1).await;
    assert!(
        h.store
            .forge_integration(&id)
            .await
            .unwrap()
            .unwrap()
            .webhook_last_delivery_at
            .is_some()
    );
}

#[tokio::test]
async fn a_server_that_goes_away_restores_the_timer_and_one_that_returns_goes_live_with_one_catch_up_fetch()
 {
    let dir = tempfile::tempdir().unwrap();
    let server = StandIn::start(dir.path(), 0).await;
    let home = home(&format!("tunnel_host = \"{}\"\n", server.host()));
    let gates = Gates::shut();
    let stub = stub_forge_cli(script(&[], &gates));
    let timeouts = Timeouts {
        forge_poll: RUNS_OUT / 2,
        forge_wake_debounce: RUNS_OUT / 10,
        tunnel_connect: RUNS_OUT / 2,
        tunnel_retry: RUNS_OUT / 100,
        ..Timeouts::default()
    };
    let (h, _listener) = daemon(home.path(), &stub, timeouts).await;
    server.hold();
    let id = enable(&h).await;
    call_after(&stub, 0, "the first fetch at the lists gate", is_list_call).await;
    server.release();
    let channel = channel(&h).await;
    live_at(&h, &stub, &gates, &id, &server.url(&channel)).await;
    let other = other_repository(&h).await;

    let since = gates.close(&stub);
    let api = server.api;
    server.stop().await;
    // Another repository changes faster than the outage grace: the outage
    // clock must run on regardless.
    let mut edits = 0;
    eventually(TIMEOUT, "every integration to poll", async || {
        edits += 1;
        let _: Value = h
            .json(
                put_json(
                    &format!("/v1/repositories/{other}"),
                    json!({"description": format!("edit {edits}")}),
                ),
                StatusCode::OK,
            )
            .await;
        let row = h.store.forge_integration(&id).await.unwrap().unwrap();
        row.webhook_state == "polling"
            && row.webhook_url.is_none()
            && row
                .webhook_error
                .as_deref()
                .is_some_and(|e| e.starts_with("tunnel down since "))
    })
    .await;
    assert_eq!(tunnel(&h).await.state, TunnelState::Down);
    call_after(&stub, since, "the outage's fetch", is_list_call).await;
    timer_fetch(&stub, &gates).await;

    let server = StandIn::start(dir.path(), api).await;
    live_at(&h, &stub, &gates, &id, &server.url(&channel)).await;
    let row = h.store.forge_integration(&id).await.unwrap().unwrap();
    assert_eq!(row.webhook_error, None);
    assert_eq!(row.webhook_id, Some(17));
    assert_eq!(tunnel(&h).await.state, TunnelState::Up);
    assert!(server.requests().iter().all(|r| *r == channel));
}

#[tokio::test]
async fn a_silent_server_counts_as_down() {
    let dir = tempfile::tempdir().unwrap();
    let server = StandIn::start(dir.path(), 0).await;
    let home = home(&format!("tunnel_host = \"{}\"\n", server.host()));
    let gates = Gates::opened();
    let stub = stub_forge_cli(script(&[], &gates));
    let timeouts = Timeouts {
        forge_wake_debounce: RUNS_OUT / 10,
        tunnel_idle: RUNS_OUT / 2,
        ..Timeouts::default()
    };
    let (h, _listener) = daemon(home.path(), &stub, timeouts).await;
    let id = enable(&h).await;
    let channel = channel(&h).await;
    let hook = hook(&server.url(&channel), &id);
    eventually(TIMEOUT, "the hook to go live at the tunnel", async || {
        let row = h.store.forge_integration(&id).await.unwrap().unwrap();
        row.webhook_state == "live" && row.webhook_url.as_deref() == Some(hook.as_str())
    })
    .await;

    // The stream stays open, but the pings stop.
    server.mute();
    eventually(TIMEOUT, "the integration to poll", async || {
        let row = h.store.forge_integration(&id).await.unwrap().unwrap();
        row.webhook_state == "polling"
            && row
                .webhook_error
                .as_deref()
                .is_some_and(|e| e.starts_with("tunnel down since "))
    })
    .await;
    let state = tunnel(&h).await;
    assert_eq!(state.state, TunnelState::Down);
    assert!(
        state
            .error
            .as_deref()
            .is_some_and(|e| e.starts_with("no word from the tunnel server")),
        "{state:?}"
    );
}

#[tokio::test]
async fn the_switch_moves_the_fetch_without_touching_the_hooks_and_survives_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let server = StandIn::start(dir.path(), 0).await;
    let home = home(&format!("tunnel_host = \"{}\"\n", server.host()));
    let gates = Gates::shut();
    let stub = stub_forge_cli(script(&[], &gates));
    let timeouts = Timeouts {
        forge_poll: RUNS_OUT,
        forge_wake_debounce: RUNS_OUT / 10,
        ..Timeouts::default()
    };
    let (h, _listener) = daemon(home.path(), &stub, timeouts).await;
    server.hold();
    let id = enable(&h).await;
    call_after(&stub, 0, "the first fetch at the lists gate", is_list_call).await;
    server.release();
    let channel = channel(&h).await;
    live_at(&h, &stub, &gates, &id, &server.url(&channel)).await;
    let hooks = hook_calls(&stub);

    let since = gates.close(&stub);
    let mut events = h.bus.subscribe();
    let off: ForgeTunnelDto = h
        .json(
            put_json("/v1/forge/tunnel", json!({"enabled": false})),
            StatusCode::OK,
        )
        .await;
    assert!(!off.enabled);
    next_event(&mut events, |e| {
        e.event.kind() == "forge_settings_updated" && e.event.payload()["enabled"] == false
    })
    .await;
    eventually(TIMEOUT, "the tunnel to close", async || {
        let row = h.store.forge_integration(&id).await.unwrap().unwrap();
        row.webhook_state == "polling" && row.webhook_error.as_deref() == Some("tunnel off")
    })
    .await;
    assert_eq!(tunnel(&h).await.state, TunnelState::Off);
    call_after(&stub, since, "the switch's fetch", is_list_call).await;
    timer_fetch(&stub, &gates).await;
    assert_eq!(hook_calls(&stub), hooks, "a switch makes no hook call");

    let on: ForgeTunnelDto = h
        .json(
            put_json("/v1/forge/tunnel", json!({"enabled": true})),
            StatusCode::OK,
        )
        .await;
    assert!(on.enabled);
    live_at(&h, &stub, &gates, &id, &server.url(&channel)).await;
    assert_eq!(tunnel(&h).await.state, TunnelState::Up);

    // Off, and the daemon restarts on the same store.
    let _: ForgeTunnelDto = h
        .json(
            put_json("/v1/forge/tunnel", json!({"enabled": false})),
            StatusCode::OK,
        )
        .await;
    eventually(TIMEOUT, "the tunnel to close", async || {
        tunnel(&h).await.state == TunnelState::Off
    })
    .await;
    h.state.tunnel.shutdown().await;
    let requests = server.requests().len();
    let restarted = Tunnel::new(
        h.store.clone(),
        h.launcher.cfg.clone(),
        h.bus.clone(),
        &h.state.forge_poll,
        timeouts,
    );
    let listener =
        WebhookListen::bind(&h.launcher.cfg, h.store.clone(), h.state.forge_poll.clone())
            .await
            .unwrap();
    restarted.start(&listener);
    let status = restarted.status().await.unwrap();
    assert!(!status.enabled);
    // A tunnel that opened anyway would ask the stand-in, and a switch that
    // stays off publishes no event that would show it.
    tokio::time::sleep(QUIET).await;
    assert_eq!(restarted.status().await.unwrap().state, TunnelState::Off);
    assert_eq!(
        server.requests().len(),
        requests,
        "an off switch opens nothing"
    );
    restarted.shutdown().await;
}

#[tokio::test]
async fn the_configured_channel_is_asked_for_and_the_stored_one_again_after_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let server = StandIn::start(dir.path(), 0).await;
    let configured = home(&format!(
        "tunnel_host = \"{}\"\ntunnel_channel = \"widgets-hooks\"\n",
        server.host()
    ));
    let gates = Gates::shut();
    let stub = stub_forge_cli(script(&[], &gates));
    let (h, _listener) = daemon(configured.path(), &stub, ordinary_timeouts()).await;
    server.hold();
    let id = enable(&h).await;
    call_after(&stub, 0, "the first fetch at the lists gate", is_list_call).await;
    server.release();
    live_at(&h, &stub, &gates, &id, &server.url("widgets-hooks")).await;
    assert_eq!(server.requests(), ["widgets-hooks"]);
    assert_eq!(h.store.forge_settings().await.unwrap().tunnel_channel, None);
    h.state.tunnel.shutdown().await;
    drop(h);

    let dir = tempfile::tempdir().unwrap();
    let server = StandIn::start(dir.path(), 0).await;
    let gates = Gates::shut();
    let stored = home(&format!("tunnel_host = \"{}\"\n", server.host()));
    let stub = stub_forge_cli(script(&[], &gates));
    let (h, _listener) = daemon(stored.path(), &stub, ordinary_timeouts()).await;
    server.hold();
    let id = enable(&h).await;
    call_after(&stub, 0, "the first fetch at the lists gate", is_list_call).await;
    server.release();
    let channel = channel(&h).await;
    assert!(
        channel.len() == 22 && channel.bytes().all(|b| b.is_ascii_alphanumeric()),
        "{channel}"
    );
    live_at(&h, &stub, &gates, &id, &server.url(&channel)).await;
    h.state.tunnel.shutdown().await;

    let restarted = Tunnel::new(
        h.store.clone(),
        h.launcher.cfg.clone(),
        h.bus.clone(),
        &h.state.forge_poll,
        ordinary_timeouts(),
    );
    let listener =
        WebhookListen::bind(&h.launcher.cfg, h.store.clone(), h.state.forge_poll.clone())
            .await
            .unwrap();
    restarted.start(&listener);
    eventually(TIMEOUT, "the restarted tunnel", async || {
        server.requests().len() == 2
    })
    .await;
    assert_eq!(server.requests(), [channel.clone(), channel]);
    restarted.shutdown().await;
}

#[tokio::test]
async fn a_public_url_or_no_enabled_integration_opens_no_tunnel() {
    let dir = tempfile::tempdir().unwrap();
    let server = StandIn::start(dir.path(), 0).await;

    let public = home(&format!(
        "tunnel_host = \"{}\"\nwebhook_public_url = \"https://hooks.example\"\n",
        server.host()
    ));
    let gates = Gates::opened();
    let stub = stub_forge_cli(script(&[], &gates));
    let (h, _listener) = daemon(public.path(), &stub, ordinary_timeouts()).await;
    let id = enable(&h).await;
    // The hook goes live before the fetch worker starts, so its first fetch
    // is the only one.
    let row = h.store.forge_integration(&id).await.unwrap().unwrap();
    assert_eq!(row.webhook_state, "live");
    assert_eq!(
        row.webhook_url,
        Some(format!("https://hooks.example/webhooks/github/{id}"))
    );
    eventually(TIMEOUT, "the first fetch", async || {
        let calls = stub.invocations();
        fetches(&stub) == 1 && calls.iter().any(is_issues_call) && stub.completed() == calls.len()
    })
    .await;
    // `WakeOnly` runs no timer fetch, and nothing later would show one.
    quiet(&stub, 1).await;
    assert_eq!(fetches_after_the_last_hook_call(&stub), 1);
    assert_eq!(tunnel(&h).await.state, TunnelState::Off);
    assert!(server.requests().is_empty());

    let none = home(&format!("tunnel_host = \"{}\"\n", server.host()));
    let stub = stub_forge_cli(script(&[], &gates));
    let (h, _listener) = daemon(none.path(), &stub, ordinary_timeouts()).await;
    let path = h.git_repo("plain");
    let _: Value = h
        .json(
            post_json("/v1/repositories", json!({"path": path})),
            StatusCode::CREATED,
        )
        .await;
    // A tunnel that opened anyway would ask the stand-in, and no event the
    // daemon publishes after a repository without a forge would show it.
    tokio::time::sleep(QUIET).await;
    assert_eq!(tunnel(&h).await.state, TunnelState::Off);
    assert!(server.requests().is_empty());
}

/// The pull request rows a fetch recorded, without what names the daemon's
/// own copy of them.
async fn rows(h: &Harness) -> Vec<Value> {
    let mut rows: Vec<Value> = h.pull_requests("/v1/pull-requests").await;
    for row in &mut rows {
        let row = row.as_object_mut().unwrap();
        for own in [
            "id",
            "repository_id",
            "created_at",
            "updated_at",
            "last_seen_at",
        ] {
            row.remove(own);
        }
    }
    rows
}

#[tokio::test]
async fn a_delivery_and_a_timer_fetch_record_the_same_pull_request_rows() {
    let before = [pull("Fix widgets", "")];
    let after = [pull("Fix widgets, reviewed", "CHANGES_REQUESTED")];

    // A delivery through the tunnel brings the fetch.
    let dir = tempfile::tempdir().unwrap();
    let server = StandIn::start(dir.path(), 0).await;
    let tunnelled = home(&format!("tunnel_host = \"{}\"\n", server.host()));
    let gates = Gates::shut();
    let stub = stub_forge_cli(script(&before, &gates));
    let (h, _listener) = daemon(tunnelled.path(), &stub, ordinary_timeouts()).await;
    server.hold();
    let id = enable(&h).await;
    call_after(&stub, 0, "the first fetch at the lists gate", is_list_call).await;
    server.release();
    let channel = channel(&h).await;
    let count = live_at(&h, &stub, &gates, &id, &server.url(&channel)).await;
    stub.reprogram(script(&after, &gates));
    let url = hook(&server.url(&channel), &id);
    assert_eq!(deliver(&h, &id, &url, COMPACT).await, StatusCode::OK);
    eventually(TIMEOUT, "the delivery's fetch", async || {
        fetches(&stub) == count + 1
    })
    .await;
    eventually(TIMEOUT, "the delivered row", async || {
        rows(&h)
            .await
            .first()
            .is_some_and(|r| r["review_decision"] == "changes_requested")
    })
    .await;
    let delivered = rows(&h).await;

    // The tunnel is off, and the timer brings the fetch.
    // No server answers here: the switch is off before anything asks.
    let timed = home("tunnel_host = \"http://127.0.0.1:9\"\n");
    let stub = stub_forge_cli(script(&before, &gates));
    let (h, _listener) = daemon(
        timed.path(),
        &stub,
        Timeouts {
            forge_poll: RUNS_OUT,
            forge_wake_debounce: RUNS_OUT / 10,
            ..Timeouts::default()
        },
    )
    .await;
    h.state.tunnel.set_enabled(false).await.unwrap();
    enable(&h).await;
    eventually(TIMEOUT, "the first row", async || {
        !rows(&h).await.is_empty()
    })
    .await;
    // The row above lands mid-round, from `start_work`, well before the
    // round's own trailing issues call: wait for the round itself to finish
    // before its script changes under it.
    eventually(TIMEOUT, "the first fetch to finish", async || {
        stub.fetches() >= 1
    })
    .await;
    stub.reprogram(script(&after, &gates));
    eventually(TIMEOUT, "the timer's row", async || {
        rows(&h)
            .await
            .first()
            .is_some_and(|r| r["review_decision"] == "changes_requested")
    })
    .await;
    assert_eq!(rows(&h).await, delivered);
}

#[tokio::test]
async fn a_switch_off_during_registration_cancels_it_and_publishes_no_url() {
    let dir = tempfile::tempdir().unwrap();
    let server = StandIn::start(dir.path(), 0).await;
    let home = home(&format!("tunnel_host = \"{}\"\n", server.host()));
    let gates = Gates::opened();
    let stub = stub_forge_cli(script(&[], &gates));
    let (h, _listener) = daemon(home.path(), &stub, ordinary_timeouts()).await;
    server.hold();
    let id = enable(&h).await;
    eventually(TIMEOUT, "the registration to be held", async || {
        server.requests().len() == 1
    })
    .await;

    let _: ForgeTunnelDto = h
        .json(
            put_json("/v1/forge/tunnel", json!({"enabled": false})),
            StatusCode::OK,
        )
        .await;
    eventually(TIMEOUT, "the switch to take hold", async || {
        let row = h.store.forge_integration(&id).await.unwrap().unwrap();
        row.webhook_error.as_deref() == Some("tunnel off")
    })
    .await;
    server.release();
    // The released answer goes to an attempt the switch already dropped,
    // so no event follows it to show a URL or a hook it should not bring.
    tokio::time::sleep(QUIET).await;

    let row = h.store.forge_integration(&id).await.unwrap().unwrap();
    assert_eq!(row.webhook_state, "polling");
    assert_eq!(row.webhook_url, None);
    assert_eq!(hook_calls(&stub), 0, "no hook after the switch off");
    let state = tunnel(&h).await;
    assert_eq!(state.state, TunnelState::Off);
    assert_eq!(state.url, None);
}

/// Runs `ariadned` under a python3 parent that kills it if the test process
/// dies, so no daemon outlives the test however that ends.
const WRAPPER: &str = "import os, subprocess, sys, time
parent = os.getppid()
child = subprocess.Popen(sys.argv[1:])
while child.poll() is None:
    if os.getppid() != parent:
        child.kill()
        break
    # Stand-in poll: the pace of the parent check.
    time.sleep(0.2)
";

/// A running `ariadned`. Dropping it kills it, the wrapper with it.
struct Daemon {
    wrapper: std::process::Child,
    pid_file: PathBuf,
}

impl Daemon {
    /// Start `ariadned` on `home`, with `env` beside the narrow `PATH`.
    fn spawn(home: &Path, env: &[(&str, &str)]) -> Self {
        Self {
            wrapper: std::process::Command::new("python3")
                .args(["-c", WRAPPER, env!("CARGO_BIN_EXE_ariadned"), "--home"])
                .arg(home)
                // The test starts no agent. Keep the daemon from probing any
                // coding-agent command the machine happens to have installed;
                // it still has the system `git` and the stub `gh` by absolute
                // path through the home configuration.
                .env("PATH", "/usr/bin:/bin")
                .envs(env.iter().copied())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap(),
            pid_file: ariadne_client::endpoint::pid_file(home),
        }
    }

    fn pid(&self) -> Option<String> {
        std::fs::read_to_string(&self.pid_file)
            .ok()
            .map(|pid| pid.trim().to_string())
    }

    fn signal(&self, signal: &str) -> bool {
        self.pid().is_some_and(|pid| {
            std::process::Command::new("kill")
                .args([signal, &pid])
                .status()
                .unwrap()
                .success()
        })
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        self.signal("-KILL");
        let _ = self.wrapper.kill();
        let _ = self.wrapper.wait();
    }
}

#[tokio::test]
async fn shutdown_closes_the_tunnel_and_the_listener_while_an_event_stream_holds_the_drain() {
    let dir = tempfile::tempdir().unwrap();
    let server = StandIn::start(&dir.path().join("stand-in"), 0).await;
    let gates = Gates::opened();
    let stub = stub_forge_cli(script(&[], &gates));
    let repo = git_repo_copy(dir.path(), "repo").await;
    sh(
        &repo,
        "git remote add origin https://github.com/acme/widgets.git",
    );
    let port = {
        let free = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        free.local_addr().unwrap().port()
    };
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    store_copy(&home.join("ariadne.db")).await;
    std::fs::write(
        home.join("config.toml"),
        format!(
            "tcp_listen = \"127.0.0.1:{port}\"\ntunnel_host = \"{}\"\ngh_bin = \"{}\"\n",
            server.host(),
            stub.gh
        ),
    )
    .unwrap();
    let daemon = Daemon::spawn(&home, &[]);
    let api = format!("http://127.0.0.1:{port}");
    let client = reqwest::Client::new();
    eventually(TIMEOUT, "the daemon to answer", async || {
        client
            .get(format!("{api}/v1/health"))
            .send()
            .await
            .is_ok_and(|r| r.status().is_success())
    })
    .await;
    // This daemon is a separate process, so its forge child has `ariadned`
    // rather than the test as its direct parent. Bind the stub's lifetime to
    // that process before its first forge call.
    let daemon_pid = std::fs::read_to_string(&daemon.pid_file)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    stub.set_parent_pid(daemon_pid);
    let created = client
        .post(format!("{api}/v1/repositories"))
        .header("Content-Type", "application/json")
        .body(json!({"path": repo, "forge": {"enabled": true}}).to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(created.status(), StatusCode::CREATED);
    // A fresh database starts the switch off (027 rule 11).
    let switched = client
        .put(format!("{api}/v1/forge/tunnel"))
        .header("Content-Type", "application/json")
        .body(json!({"enabled": true}).to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(switched.status(), StatusCode::OK);
    let mut listen = None;
    eventually(TIMEOUT, "the tunnel to come up", async || {
        let body = client
            .get(format!("{api}/v1/forge/tunnel"))
            .send()
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap();
        let state: ForgeTunnelDto = serde_json::from_slice(&body).unwrap();
        listen = state.listen;
        state.state == TunnelState::Up
    })
    .await;
    let listen = listen.unwrap();
    assert!(tokio::net::TcpStream::connect(&listen).await.is_ok());

    // An open event stream holds the HTTP drain for as long as it stays.
    let stream = client
        .get(format!("{api}/v1/events/stream"))
        .send()
        .await
        .unwrap();
    assert_eq!(stream.status(), StatusCode::OK);
    assert!(daemon.signal("-TERM"));
    eventually(
        TIMEOUT,
        "the tunnel and the listener to close",
        async || server.closed() > 0 && tokio::net::TcpStream::connect(&listen).await.is_err(),
    )
    .await;
    assert!(daemon.signal("-0"), "the drain still waits on the stream");
    drop(stream);
}

#[tokio::test]
async fn a_proxy_in_the_environment_never_carries_the_post_to_the_listener() {
    let dir = tempfile::tempdir().unwrap();
    let server = StandIn::start(&dir.path().join("stand-in"), 0).await;
    let gates = Gates::opened();
    let stub = stub_forge_cli(script(&[], &gates));
    let repo = git_repo_copy(dir.path(), "repo").await;
    sh(
        &repo,
        "git remote add origin https://github.com/acme/widgets.git",
    );
    let free = || {
        let free = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        free.local_addr().unwrap().port()
    };
    let (port, dead) = (free(), free());
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    store_copy(&home.join("ariadne.db")).await;
    // The channel host is `localhost`, which `NO_PROXY` exempts; the
    // listener is `127.0.0.1`, which it does not, as a user's would be.
    let host = format!("http://localhost:{}", server.api);
    std::fs::write(
        home.join("config.toml"),
        format!(
            "tcp_listen = \"127.0.0.1:{port}\"\ntunnel_host = \"{host}\"\ngh_bin = \"{}\"\n",
            stub.gh
        ),
    )
    .unwrap();
    let proxy = format!("http://127.0.0.1:{dead}");
    let daemon = Daemon::spawn(
        &home,
        &[
            ("HTTP_PROXY", &proxy),
            ("http_proxy", &proxy),
            ("ALL_PROXY", &proxy),
            ("NO_PROXY", "localhost"),
            ("no_proxy", "localhost"),
        ],
    );
    let api = format!("http://127.0.0.1:{port}");
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    eventually(TIMEOUT, "the daemon to answer", async || {
        client
            .get(format!("{api}/v1/health"))
            .send()
            .await
            .is_ok_and(|r| r.status().is_success())
    })
    .await;
    stub.set_parent_pid(daemon.pid().unwrap().parse().unwrap());
    let created = client
        .post(format!("{api}/v1/repositories"))
        .header("Content-Type", "application/json")
        .body(json!({"path": repo, "forge": {"enabled": true}}).to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(created.status(), StatusCode::CREATED);
    let created: Value = serde_json::from_slice(&created.bytes().await.unwrap()).unwrap();
    let id = created["id"].as_str().unwrap().to_string();
    let switched = client
        .put(format!("{api}/v1/forge/tunnel"))
        .header("Content-Type", "application/json")
        .body(json!({"enabled": true}).to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(switched.status(), StatusCode::OK);

    let store = ariadne_store::Store::open_read_only(home.join("ariadne.db"))
        .await
        .unwrap();
    let mut live = None;
    eventually(TIMEOUT, "the hook to go live", async || {
        live = store
            .forge_integration(&id)
            .await
            .unwrap()
            .filter(|row| row.webhook_state == "live");
        live.is_some()
    })
    .await;
    let row = live.unwrap();
    let url = row.webhook_url.unwrap();
    assert!(url.starts_with(&host), "{url}");
    let secret = row.webhook_secret.unwrap();
    assert_eq!(deliver_signed(&secret, &url, COMPACT).await, StatusCode::OK);
    eventually(TIMEOUT, "the delivery to reach the listener", async || {
        store
            .forge_integration(&id)
            .await
            .unwrap()
            .unwrap()
            .webhook_last_delivery_at
            .is_some()
    })
    .await;
}
