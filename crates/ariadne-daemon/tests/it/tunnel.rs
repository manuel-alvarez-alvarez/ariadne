//! The webhook tunnel against a stand-in localtunnel server (027).
//!
//! The stand-in is a python3 program, as the stub agent is. It answers the
//! new-tunnel request with a local port and a URL, holds the client's
//! connections on that port, and forwards each request its public port
//! receives through one of them.
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::common::forge::{StubForgeCli, answer, stub_forge_cli};
use crate::common::{
    Harness, QUIET, RUNS_OUT, TIMEOUT, eventually, harness, next_event, post_json, put_json, sh,
};
use ariadne_api::repositories::{ForgeTunnelDto, TunnelState};
use ariadne_daemon::forge::tunnel::Tunnel;
use ariadne_daemon::timeouts::Timeouts;
use ariadne_daemon::webhooks::WebhookListen;
use axum::http::StatusCode;
use serde_json::{Value, json};

const STAND_IN: &str = r#"
import json, os, queue, socket, sys, threading, time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

state, api_port = sys.argv[1], int(sys.argv[2])
parent = os.getppid()

def orphaned():
    # Ends with the test process, however that one ends.
    while os.getppid() == parent:
        time.sleep(0.2)
    os._exit(0)

def bound(port=0):
    s = socket.socket()
    s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    s.bind(("127.0.0.1", port))
    s.listen(64)
    return s

tunnel, public = bound(), bound()
idle = queue.Queue()

def closed(conn):
    # Logs a tunnel connection the client closed.
    try:
        if conn.recv(1, socket.MSG_PEEK) == b"":
            with open(os.path.join(state, "closed.log"), "a") as f:
                f.write("closed\n")
    except OSError:
        pass

def hold():
    while True:
        conn, _ = tunnel.accept()
        threading.Thread(target=closed, args=(conn,), daemon=True).start()
        idle.put(conn)

def message(sock):
    data = b""
    while b"\r\n\r\n" not in data:
        chunk = sock.recv(65536)
        if not chunk:
            return data
        data += chunk
    head, _, body = data.partition(b"\r\n\r\n")
    length = 0
    for line in head.split(b"\r\n")[1:]:
        name, _, value = line.partition(b":")
        if name.strip().lower() == b"content-length":
            length = int(value.strip())
    while len(body) < length:
        chunk = sock.recv(65536)
        if not chunk:
            break
        body += chunk
    return head, body

def forward(client):
    head, body = message(client)
    lines = [l for l in head.split(b"\r\n") if not l.lower().startswith(b"connection:")]
    conn = idle.get()
    conn.sendall(b"\r\n".join(lines + [b"Connection: close"]) + b"\r\n\r\n" + body)
    head, body = message(conn)
    conn.close()
    client.sendall(head + b"\r\n\r\n" + body)
    client.close()

def serve():
    while True:
        client, _ = public.accept()
        threading.Thread(target=forward, args=(client,), daemon=True).start()

class Api(BaseHTTPRequestHandler):
    def do_GET(self):
        sub = self.path.strip("/").split("?")[0]
        with open(os.path.join(state, "requests.log"), "a") as f:
            f.write(sub + "\n")
        # The test holds the answer while this file exists.
        while os.path.exists(os.path.join(state, "hold")):
            time.sleep(0.02)
        body = json.dumps({
            "id": sub,
            "port": tunnel.getsockname()[1],
            "max_conn_count": 10,
            "url": "http://%s.127.0.0.1:%d" % (sub, public.getsockname()[1]),
        }).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *args):
        pass

ThreadingHTTPServer.allow_reuse_address = True
api = ThreadingHTTPServer(("127.0.0.1", api_port), Api)
for target in (orphaned, hold, serve):
    threading.Thread(target=target, daemon=True).start()
ports = os.path.join(state, "ports.json")
with open(ports + ".tmp", "w") as f:
    json.dump({"api": api.server_address[1], "public": public.getsockname()[1]}, f)
os.replace(ports + ".tmp", ports)
api.serve_forever()
"#;

/// A running stand-in server. Dropping it kills it.
struct StandIn {
    child: tokio::process::Child,
    dir: PathBuf,
    api: u16,
    public: u16,
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
            public: bound["public"].as_u64().unwrap() as u16,
        }
    }

    /// The server goes away: every socket of it closes.
    async fn stop(mut self) {
        self.child.kill().await.unwrap();
    }

    /// The subdomain of each new-tunnel request, in order.
    fn requests(&self) -> Vec<String> {
        std::fs::read_to_string(self.dir.join("requests.log"))
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    /// How many tunnel connections the client has closed.
    fn closed(&self) -> usize {
        std::fs::read_to_string(self.dir.join("closed.log"))
            .unwrap_or_default()
            .lines()
            .count()
    }

    fn host(&self) -> String {
        format!("http://127.0.0.1:{}", self.api)
    }

    fn url(&self, subdomain: &str) -> String {
        format!("http://{subdomain}.127.0.0.1:{}", self.public)
    }
}

fn script(list: &[Value]) -> Value {
    json!([
        answer(&["auth", "status"], 0, ""),
        answer(&["api", "user"], 0, "me"),
        answer(&["pr", "list"], 0, &serde_json::to_string(list).unwrap()),
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
    (h, listener)
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

/// Fetches so far: each one lists authored requests once.
fn fetches(stub: &StubForgeCli) -> usize {
    stub.invocations()
        .iter()
        .filter(|call| call.args.starts_with(&["pr".into(), "list".into()]))
        .filter(|call| call.args.iter().any(|a| a == "--author"))
        .count()
}

/// Every hook request: an `api` call other than the login's.
fn hook_calls(stub: &StubForgeCli) -> usize {
    stub.invocations()
        .iter()
        .filter(|call| call.args.first().is_some_and(|a| a == "api"))
        .filter(|call| call.args.get(1).is_none_or(|a| a != "user"))
        .count()
}

/// Fetches that began after the last hook request.
fn fetches_after_the_last_hook_call(stub: &StubForgeCli) -> usize {
    let calls = stub.invocations();
    let last = calls
        .iter()
        .rposition(|call| call.args.first().is_some_and(|a| a == "api") && call.args[1] != "user")
        .expect("a hook request");
    calls[last..]
        .iter()
        .filter(|call| call.args.starts_with(&["pr".into(), "list".into()]))
        .filter(|call| call.args.iter().any(|a| a == "--author"))
        .count()
}

async fn quiet(stub: &StubForgeCli, count: usize) {
    assert!(
        tokio::time::timeout(QUIET, async {
            loop {
                assert_eq!(fetches(stub), count);
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

/// Wait for the hook to go live at `url`, and for the one fetch after it.
async fn live_at(h: &Harness, stub: &StubForgeCli, id: &str, url: &str) -> usize {
    let hook = format!("{url}/webhooks/github/{id}");
    eventually(TIMEOUT, "the hook to go live at the tunnel", async || {
        let row = h.store.forge_integration(id).await.unwrap().unwrap();
        row.webhook_state == "live" && row.webhook_url.as_deref() == Some(hook.as_str())
    })
    .await;
    eventually(TIMEOUT, "the catch-up fetch", async || {
        fetches_after_the_last_hook_call(stub) >= 1
    })
    .await;
    // A fetch that was running when the hook went live coalesces the
    // catch-up wake into one more fetch (026): wait for the two to settle.
    let mut count = fetches(stub);
    eventually(TIMEOUT, "the fetches to settle", async || {
        tokio::time::sleep(QUIET).await;
        let settled = fetches(stub) == count;
        count = fetches(stub);
        settled
    })
    .await;
    quiet(stub, count).await;
    let after = fetches_after_the_last_hook_call(stub);
    assert!(
        (1..=2).contains(&after),
        "one catch-up fetch, or a running one and the wake it coalesced: {after}"
    );
    count
}

/// Post a signed delivery to `url`, as the forge would.
async fn deliver(h: &Harness, id: &str, url: &str) -> StatusCode {
    let secret = h
        .store
        .forge_integration(id)
        .await
        .unwrap()
        .unwrap()
        .webhook_secret
        .unwrap();
    let body = br#"{"action":"created"}"#;
    // Python provides an independent signature implementation.
    let output = std::process::Command::new("python3")
        .args([
            "-c",
            "import hmac,hashlib,sys;print('sha256='+hmac.new(sys.argv[1].encode(),sys.argv[2].encode(),hashlib.sha256).hexdigest())",
            &secret,
            std::str::from_utf8(body).unwrap(),
        ])
        .output()
        .unwrap();
    let signature = String::from_utf8(output.stdout).unwrap().trim().to_string();
    reqwest::Client::new()
        .post(url)
        .header("X-Hub-Signature-256", signature)
        .header("X-Github-Event", "issue_comment")
        .body(body.to_vec())
        .send()
        .await
        .unwrap()
        .status()
}

async fn subdomain(h: &Harness) -> String {
    eventually(TIMEOUT, "a stored subdomain", async || {
        h.store
            .forge_settings()
            .await
            .unwrap()
            .tunnel_subdomain
            .is_some()
    })
    .await;
    h.store
        .forge_settings()
        .await
        .unwrap()
        .tunnel_subdomain
        .unwrap()
}

#[tokio::test]
async fn a_tunnel_to_the_listener_registers_the_hook_and_a_delivery_through_it_fetches_once() {
    let dir = tempfile::tempdir().unwrap();
    let server = StandIn::start(dir.path(), 0).await;
    let home = home(&format!("tunnel_host = \"{}\"\n", server.host()));
    let stub = stub_forge_cli(script(&[]));
    let (h, listener) = daemon(home.path(), &stub, Timeouts::default()).await;
    let id = enable(&h).await;
    let sub = subdomain(&h).await;
    let count = live_at(&h, &stub, &id, &server.url(&sub)).await;
    assert_eq!(server.requests(), std::slice::from_ref(&sub));
    let create = stub
        .invocations()
        .into_iter()
        .find(|c| c.args.iter().any(|a| a == "POST"))
        .unwrap();
    assert!(
        create
            .args
            .iter()
            .any(|a| a.contains(&format!("{}/webhooks/github/{id}", server.url(&sub))))
    );
    let state = tunnel(&h).await;
    assert!(state.enabled);
    assert_eq!(state.state, TunnelState::Up);
    assert_eq!(state.url, Some(server.url(&sub)));
    assert_eq!(state.listen, Some(listener.address().to_string()));
    assert_ne!(listener.address().port(), 0);

    // The stand-in's public port is the URL's; the subdomain needs no DNS.
    let url = format!("http://127.0.0.1:{}/webhooks/github/{id}", server.public);
    assert_eq!(deliver(&h, &id, &url).await, StatusCode::ACCEPTED);
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
    let stub = stub_forge_cli(script(&[]));
    let timeouts = Timeouts {
        forge_poll: RUNS_OUT,
        tunnel_connect: RUNS_OUT,
        tunnel_retry: Duration::from_millis(100),
        ..Timeouts::default()
    };
    let (h, _listener) = daemon(home.path(), &stub, timeouts).await;
    let id = enable(&h).await;
    let sub = subdomain(&h).await;
    live_at(&h, &stub, &id, &server.url(&sub)).await;
    let other = other_repository(&h).await;

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
    let polling = fetches(&stub);
    eventually(TIMEOUT, "the timer to fetch", async || {
        fetches(&stub) >= polling + 2
    })
    .await;

    let server = StandIn::start(dir.path(), api).await;
    live_at(&h, &stub, &id, &server.url(&sub)).await;
    let row = h.store.forge_integration(&id).await.unwrap().unwrap();
    assert_eq!(row.webhook_error, None);
    assert_eq!(row.webhook_id, Some(17));
    assert_eq!(tunnel(&h).await.state, TunnelState::Up);
    assert!(server.requests().iter().all(|r| *r == sub));
}

#[tokio::test]
async fn the_switch_moves_the_fetch_without_touching_the_hooks_and_survives_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let server = StandIn::start(dir.path(), 0).await;
    let home = home(&format!("tunnel_host = \"{}\"\n", server.host()));
    let stub = stub_forge_cli(script(&[]));
    let timeouts = Timeouts {
        forge_poll: RUNS_OUT,
        ..Timeouts::default()
    };
    let (h, _listener) = daemon(home.path(), &stub, timeouts).await;
    let id = enable(&h).await;
    let sub = subdomain(&h).await;
    live_at(&h, &stub, &id, &server.url(&sub)).await;
    let hooks = hook_calls(&stub);

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
    let polling = fetches(&stub);
    eventually(TIMEOUT, "the timer to fetch", async || {
        fetches(&stub) >= polling + 2
    })
    .await;
    assert_eq!(hook_calls(&stub), hooks, "a switch makes no hook call");

    let on: ForgeTunnelDto = h
        .json(
            put_json("/v1/forge/tunnel", json!({"enabled": true})),
            StatusCode::OK,
        )
        .await;
    assert!(on.enabled);
    live_at(&h, &stub, &id, &server.url(&sub)).await;
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
async fn the_configured_subdomain_is_asked_for_and_the_stored_one_again_after_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let server = StandIn::start(dir.path(), 0).await;
    let configured = home(&format!(
        "tunnel_host = \"{}\"\ntunnel_subdomain = \"widgets-hooks\"\n",
        server.host()
    ));
    let stub = stub_forge_cli(script(&[]));
    let (h, _listener) = daemon(configured.path(), &stub, Timeouts::default()).await;
    let id = enable(&h).await;
    live_at(&h, &stub, &id, &server.url("widgets-hooks")).await;
    assert_eq!(server.requests(), ["widgets-hooks"]);
    assert_eq!(
        h.store.forge_settings().await.unwrap().tunnel_subdomain,
        None
    );
    h.state.tunnel.shutdown().await;
    drop(h);

    let dir = tempfile::tempdir().unwrap();
    let server = StandIn::start(dir.path(), 0).await;
    let stored = home(&format!("tunnel_host = \"{}\"\n", server.host()));
    let stub = stub_forge_cli(script(&[]));
    let (h, _listener) = daemon(stored.path(), &stub, Timeouts::default()).await;
    let id = enable(&h).await;
    let sub = subdomain(&h).await;
    let word = sub.split_once('-');
    assert!(
        word.is_some_and(|(word, digits)| !word.is_empty()
            && word.bytes().all(|b| b.is_ascii_lowercase())
            && digits.len() == 6
            && digits.bytes().all(|b| b.is_ascii_digit())),
        "{sub}"
    );
    live_at(&h, &stub, &id, &server.url(&sub)).await;
    h.state.tunnel.shutdown().await;

    let restarted = Tunnel::new(
        h.store.clone(),
        h.launcher.cfg.clone(),
        h.bus.clone(),
        &h.state.forge_poll,
        Timeouts::default(),
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
    assert_eq!(server.requests(), [sub.clone(), sub]);
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
    let stub = stub_forge_cli(script(&[]));
    let (h, _listener) = daemon(public.path(), &stub, Timeouts::default()).await;
    let id = enable(&h).await;
    live_at(&h, &stub, &id, "https://hooks.example").await;
    assert_eq!(tunnel(&h).await.state, TunnelState::Off);
    assert!(server.requests().is_empty());

    let none = home(&format!("tunnel_host = \"{}\"\n", server.host()));
    let stub = stub_forge_cli(script(&[]));
    let (h, _listener) = daemon(none.path(), &stub, Timeouts::default()).await;
    let path = h.git_repo("plain");
    let _: Value = h
        .json(
            post_json("/v1/repositories", json!({"path": path})),
            StatusCode::CREATED,
        )
        .await;
    tokio::time::sleep(QUIET).await;
    assert_eq!(tunnel(&h).await.state, TunnelState::Off);
    assert!(server.requests().is_empty());
}

/// The pull request rows a fetch recorded, without what names the daemon's
/// own copy of them.
async fn rows(h: &Harness) -> Vec<Value> {
    let mut rows: Vec<Value> = h.get("/v1/pull-requests").await;
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
    let stub = stub_forge_cli(script(&before));
    let (h, _listener) = daemon(tunnelled.path(), &stub, Timeouts::default()).await;
    let id = enable(&h).await;
    let sub = subdomain(&h).await;
    let count = live_at(&h, &stub, &id, &server.url(&sub)).await;
    stub.reprogram(script(&after));
    let url = format!("http://127.0.0.1:{}/webhooks/github/{id}", server.public);
    assert_eq!(deliver(&h, &id, &url).await, StatusCode::ACCEPTED);
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
    let stub = stub_forge_cli(script(&before));
    let (h, _listener) = daemon(
        timed.path(),
        &stub,
        Timeouts {
            forge_poll: RUNS_OUT,
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
    stub.reprogram(script(&after));
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
    let stub = stub_forge_cli(script(&[]));
    let (h, _listener) = daemon(home.path(), &stub, Timeouts::default()).await;
    let hold = dir.path().join("hold");
    std::fs::write(&hold, "").unwrap();
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
    std::fs::remove_file(&hold).unwrap();
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
    time.sleep(0.2)
";

/// A running `ariadned`. Dropping it kills it, the wrapper with it.
struct Daemon {
    wrapper: std::process::Child,
    pid_file: PathBuf,
}

impl Daemon {
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
    let stub = stub_forge_cli(script(&[]));
    let repo = dir.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    sh(
        &repo,
        "git init -q -b main && echo v1 > file.txt && git add . && \
         git -c user.email=t@t -c user.name=t commit -qm init && \
         git remote add origin https://github.com/acme/widgets.git",
    );
    let port = {
        let free = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        free.local_addr().unwrap().port()
    };
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(
        home.join("config.toml"),
        format!(
            "tcp_listen = \"127.0.0.1:{port}\"\ntunnel_host = \"{}\"\ngh_bin = \"{}\"\nglab_bin = \"{}\"\n",
            server.host(),
            stub.gh,
            stub.glab
        ),
    )
    .unwrap();
    let daemon = Daemon {
        wrapper: std::process::Command::new("python3")
            .args(["-c", WRAPPER, env!("CARGO_BIN_EXE_ariadned"), "--home"])
            .arg(&home)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap(),
        pid_file: ariadne_client::endpoint::pid_file(&home),
    };
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
    let created = client
        .post(format!("{api}/v1/repositories"))
        .header("Content-Type", "application/json")
        .body(json!({"path": repo, "forge": {"enabled": true}}).to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(created.status(), StatusCode::CREATED);
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
