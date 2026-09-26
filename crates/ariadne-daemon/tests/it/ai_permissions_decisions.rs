//! Permission decisions made through the model in `ai` mode (022, Decisions).

use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::State;
use axum::http::{Request, StatusCode};
use axum::routing::{get, post as route_post};
use serde_json::{Value, json};
use tracing_subscriber::layer::SubscriberExt;

use ariadne_api::logs::LogSnapshotResponse;
use ariadne_core::{Actor, AttentionReason, PermissionMode, SessionStatus, TaskStatus};
use ariadne_daemon::timeouts::Timeouts;
use ariadne_store::{AgentPin, EventFilter};

use crate::common::acp::{registry_home, script, stub_acp_agent};
use crate::common::{
    Cast, Harness, HarnessBuilder, RUNS_OUT, TIMEOUT, eventually, harness, post_json, put_json,
    shared_script,
};

#[derive(Clone)]
enum Answer {
    Value(Value),
    Hang,
}

#[derive(Clone)]
struct ServerState {
    answer: Answer,
    requests: Arc<Mutex<Vec<Value>>>,
}

struct ModelServer {
    endpoint: String,
    requests: Arc<Mutex<Vec<Value>>>,
    task: tokio::task::JoinHandle<()>,
}

impl ModelServer {
    /// An answer shaped exactly as `kev.serve` returns it: a `noul` answer
    /// carries no `answer_confidence` or any other field beside its type and
    /// probability.
    async fn answer(needs_review_probability: f64) -> Self {
        Self::start(Answer::Value(response(json!({
            "type": "noul",
            "noul": needs_review_probability
        }))))
        .await
    }

    async fn hanging() -> Self {
        Self::start(Answer::Hang).await
    }

    async fn start(answer: Answer) -> Self {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let state = ServerState {
            answer,
            requests: requests.clone(),
        };
        let app = axum::Router::new()
            .route("/release.json", get(release))
            .route("/v1/systemone", route_post(decision))
            .with_state(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self {
            endpoint,
            requests,
            task,
        }
    }

    fn stop(&self) {
        self.task.abort();
    }
}

impl Drop for ModelServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn response(decision: Value) -> Value {
    json!({
        "model": "kev-latest",
        "answers": {"decision": decision},
        "usage": {"input_tokens": 139, "output_tokens": 0},
        "latency_ms": 200.0
    })
}

async fn release() -> axum::Json<Value> {
    axum::Json(json!({
        "tag_name": "v0.1.0",
        "assets": [{"browser_download_url": "https://example.test/model.whl"}]
    }))
}

async fn decision(
    State(state): State<ServerState>,
    axum::Json(request): axum::Json<Value>,
) -> axum::Json<Value> {
    state.requests.lock().unwrap().push(request);
    match state.answer {
        Answer::Value(answer) => axum::Json(answer),
        Answer::Hang => {
            std::future::pending::<()>().await;
            unreachable!()
        }
    }
}

fn permission_script() -> Value {
    let mut scripted = script();
    scripted["prompts"] = json!([{
        "permission": {
            "toolCall": {"toolCallId": "call-1", "name": "Bash", "title": "Bash", "kind": "execute",
                         "rawInput": {"command": "cargo test -p app"}},
            "options": [
                {"optionId": "no", "name": "Reject", "kind": "reject_once"},
                {"optionId": "yes", "name": "Allow", "kind": "allow_once"}
            ]
        },
        "updates": [],
        "stop_reason": "end_turn"
    }]);
    scripted
}

fn guardrail_script() -> Value {
    let mut scripted = permission_script();
    scripted["prompts"][0]["permission"]["toolCall"]["rawInput"] =
        json!({"command": "cat ~/.ssh/id_rsa"});
    scripted
}

fn python() -> String {
    shared_script("#!/bin/sh\necho 'Python 3.12.1'\n")
        .display()
        .to_string()
}

async fn ai_permissions_harness(
    server: &ModelServer,
    threshold: f64,
    timeouts: Timeouts,
) -> (Harness, Cast, tempfile::TempDir) {
    ai_permissions_harness_with(server, threshold, timeouts, permission_script()).await
}

async fn ai_permissions_harness_with(
    server: &ModelServer,
    threshold: f64,
    timeouts: Timeouts,
    scripted: Value,
) -> (Harness, Cast, tempfile::TempDir) {
    let endpoint = server.endpoint.clone();
    ai_permissions_harness_on(
        |builder| builder.ai_permissions_endpoint(endpoint),
        server,
        threshold,
        timeouts,
        scripted,
    )
    .await
}

/// A harness whose model is reached however `ai_permissions` says: a pinned endpoint, or
/// a server the daemon starts.
async fn ai_permissions_harness_on(
    ai_permissions: impl FnOnce(HarnessBuilder) -> HarnessBuilder,
    server: &ModelServer,
    threshold: f64,
    timeouts: Timeouts,
    scripted: Value,
) -> (Harness, Cast, tempfile::TempDir) {
    let agent_dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(agent_dir.path(), scripted);
    let h = ai_permissions(harness())
        .home(registry_home(&stub))
        .ai_permissions_release_url(format!("{}/release.json", server.endpoint))
        .ai_permissions_installer(vec!["/bin/sh".into(), "-c".into(), "exit 0".into()])
        .python_bin(python())
        .timeouts(timeouts)
        .await;
    let _: Value = h
        .json(
            put_json(
                "/v1/permissions/ai",
                json!({"enabled": true, "threshold": threshold}),
            ),
            StatusCode::OK,
        )
        .await;
    h.git_repo("repo");
    let cast = h.cast_pinned("stub:test-model", 1).await;
    h.store
        .set_agent_pin(
            &cast.author.id,
            &AgentPin {
                model: "stub:test-model".into(),
                effort: None,
            },
        )
        .await
        .unwrap();
    h.set_permission_mode(&cast.repo, PermissionMode::Ai).await;
    h.store
        .transition_task(&cast.task.id, TaskStatus::Ready, Actor::Daemon, None, None)
        .await
        .unwrap();
    (h, cast, agent_dir)
}

fn answer(session_id: &str, text: &str) -> Request<Body> {
    post_json(
        &format!("/v1/sessions/{session_id}/console/input"),
        json!({"text": text}),
    )
}

async fn reply(h: &Harness, session_id: &str) -> Value {
    h.store
        .list_events(EventFilter {
            session_id: Some(session_id.to_string()),
            ..Default::default()
        })
        .await
        .unwrap()
        .into_iter()
        .find(|event| event.kind == "permission.replied")
        .map(|event| serde_json::from_str(&event.payload).unwrap())
        .expect("the permission reply was stored")
}

/// Answer the waiting question in the console, and the reply it was stored as.
async fn answered(h: &Harness, session_id: &str, text: &str) -> Value {
    assert_eq!(
        h.send(answer(session_id, text)).await.0,
        StatusCode::NO_CONTENT
    );
    eventually(TIMEOUT, "the console reply to be stored", || async {
        h.store
            .list_events(EventFilter {
                session_id: Some(session_id.to_string()),
                ..Default::default()
            })
            .await
            .unwrap()
            .iter()
            .any(|event| event.kind == "permission.replied")
    })
    .await;
    reply(h, session_id).await
}

/// The model's side of a reply: its label, its allow score to four places,
/// the threshold, the guardrail and why it gave no answer.
fn model_part(reply: &Value) -> Value {
    json!({
        "label": reply["label"],
        "confidence": reply["confidence"].as_f64().map(|c| (c * 1e4).round() / 1e4),
        "threshold": reply["threshold"],
        "guardrail": reply["guardrail"],
        "ai_error": reply["ai_error"],
    })
}

async fn permission_request(h: &Harness, session_id: &str) -> Value {
    h.store
        .list_events(EventFilter {
            session_id: Some(session_id.to_string()),
            ..Default::default()
        })
        .await
        .unwrap()
        .into_iter()
        .find(|event| event.kind == "permission_request")
        .map(|event| serde_json::from_str(&event.payload).unwrap())
        .expect("the permission request was stored")
}

async fn wait_for_question(h: &Harness, session: &ariadne_store::AgentSession) {
    eventually(TIMEOUT, "the permission request to wait", || async {
        h.attention(session).await == Some(AttentionReason::WaitingPermission)
    })
    .await;
}

#[tokio::test]
async fn a_confident_allow_runs_at_once_and_reports_ai() {
    let server = ModelServer::answer(0.05).await;
    let (h, cast, _agent_dir) = ai_permissions_harness(&server, 0.7, Timeouts::default()).await;

    let session = h.launcher.spawn_author(&cast.task.id).await.unwrap();
    eventually(TIMEOUT, "the AI-approved turn to finish", || async {
        h.session_status(&session).await == SessionStatus::Idle
    })
    .await;

    assert_eq!(h.attention(&session).await, None);
    assert!(
        permission_request(&h, &session.id)
            .await
            .get("guardrail")
            .is_none()
    );
    assert_eq!(
        reply(&h, &session.id).await,
        json!({"session_id": "stub-session", "option_id": "yes", "decided_by": "ai",
               "label": "allow", "confidence": 0.95, "threshold": 0.7,
               "guardrail": null, "ai_error": null})
    );
    let requests = server.requests.lock().unwrap();
    assert_eq!(
        requests[0]["state"],
        json!({
            "tool": "Bash", "kind": "execute", "input": "{\"command\":\"cargo test -p app\"}",
            "options": "Reject, Allow", "operates_inside_repo": true, "writes_files": false,
            "writes_outside_repo": false, "uses_network": false, "network_hosts": [],
            "reads_sensitive_paths": false, "destructive_operation": false,
            "uses_privilege_escalation": false, "modifies_git_remote": false,
            "potentially_exfiltrates_data": false
        })
    );
    assert_eq!(requests[0]["model"], "kev-latest");
    assert_eq!(requests[0]["questions"]["decision"]["type"], "noul");
    assert_eq!(
        requests[0]["questions"]["decision"]["instructions"],
        "Does this coding-agent tool call need a person's review?"
    );
    assert_eq!(
        requests[0]["questions"]["decision"]["criteria"]["false"],
        "git status, diff, log, show, add or commit; cargo, npm, make, tsc, pytest, eslint or prettier builds, tests and lints; ls, find, cat or grep; reading or editing files under the repository path; fetching documentation"
    );
    assert_eq!(
        requests[0]["questions"]["decision"]["criteria"]["true"],
        "anything not listed as safe"
    );
}

/// A `kev.serve` that records its pid, loads for as long as its second
/// argument says, and then allows every request with the calibrated
/// confidence 0.95.
const SLOW_SERVER: &str = r#"#!/usr/bin/env python3
import http.server, json, os, sys, time
with open(sys.argv[1], 'w') as f:
    f.write(str(os.getpid()) + '\n')
time.sleep(float(sys.argv[2]))
args = sys.argv[3:]
host = args[args.index('--host') + 1]
port = int(args[args.index('--port') + 1])
ANSWER = json.dumps({"model": "kev-latest",
    "answers": {"decision": {"type": "noul", "noul": 0.05}},
    "usage": {}, "latency_ms": 12.3}).encode()
class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        self.send_response(200 if self.path == '/v1/models' else 404)
        self.end_headers()
    def do_POST(self):
        self.rfile.read(int(self.headers.get('content-length', 0)))
        self.send_response(200)
        self.send_header('content-type', 'application/json')
        self.end_headers()
        self.wfile.write(ANSWER)
    def log_message(self, *args): pass
http.server.HTTPServer((host, port), Handler).serve_forever()
"#;

#[tokio::test]
async fn a_request_made_while_the_server_loads_waits_for_it() {
    let release = ModelServer::hanging().await;
    let record = tempfile::NamedTempFile::new().unwrap();
    let command = vec![
        shared_script(SLOW_SERVER).display().to_string(),
        record.path().display().to_string(),
        "3".to_string(),
    ];
    let (h, cast, _agent_dir) = ai_permissions_harness_on(
        |builder| builder.ai_permissions_serve_command(command),
        &release,
        0.7,
        Timeouts::default(),
        permission_script(),
    )
    .await;
    eventually(TIMEOUT, "the server to start loading", || async {
        std::fs::read_to_string(record.path()).is_ok_and(|pid| pid.ends_with('\n'))
    })
    .await;
    assert_eq!(
        h.state.ai_permissions.live().await,
        None,
        "the server is still loading"
    );

    let session = h.launcher.spawn_author(&cast.task.id).await.unwrap();
    eventually(TIMEOUT, "the AI-approved turn to finish", || async {
        h.session_status(&session).await == SessionStatus::Idle
    })
    .await;

    assert_eq!(h.attention(&session).await, None);
    let reply = reply(&h, &session.id).await;
    assert_eq!(reply["decided_by"], "ai");
    assert_eq!(reply["confidence"], 0.95);
    h.state.ai_permissions.shutdown().await;
}

#[tokio::test]
async fn a_noul_answer_is_gated_on_its_allow_probability() {
    let server = ModelServer::answer(0.05).await;
    let (h, cast, _agent_dir) = ai_permissions_harness(&server, 0.7, Timeouts::default()).await;

    let session = h.launcher.spawn_author(&cast.task.id).await.unwrap();
    eventually(TIMEOUT, "the AI-approved turn to finish", || async {
        h.session_status(&session).await == SessionStatus::Idle
    })
    .await;

    assert_eq!(h.attention(&session).await, None);
    let reply = reply(&h, &session.id).await;
    assert_eq!(reply["decided_by"], "ai");
    assert_eq!(reply["confidence"], 0.95);
}

#[tokio::test]
async fn an_uncertain_allow_falls_to_console_and_then_to_the_learned_approval() {
    let server = ModelServer::answer(0.4).await;
    let (h, cast, _agent_dir) = ai_permissions_harness(&server, 0.7, Timeouts::default()).await;

    let asked = h.launcher.spawn_author(&cast.task.id).await.unwrap();
    wait_for_question(&h, &asked).await;
    assert_eq!(
        h.send(answer(&asked.id, "yes")).await.0,
        StatusCode::NO_CONTENT
    );
    eventually(TIMEOUT, "the console-approved turn to finish", || async {
        h.session_status(&asked).await == SessionStatus::Idle
    })
    .await;
    let asked_reply = reply(&h, &asked.id).await;
    assert_eq!(asked_reply["decided_by"], "console");
    assert_eq!(
        model_part(&asked_reply),
        json!({"label": "allow", "confidence": 0.6, "threshold": 0.7,
               "guardrail": null, "ai_error": null}),
        "the reply keeps the score that fell short"
    );

    let again = h
        .task_on(
            &cast.goal,
            &cast.repo,
            "Again",
            1,
            crate::common::test_pin(),
        )
        .await;
    h.store
        .transition_task(&again.id, TaskStatus::Ready, Actor::Daemon, None, None)
        .await
        .unwrap();
    let remembered = h.launcher.spawn_author(&again.id).await.unwrap();
    eventually(TIMEOUT, "the learned turn to finish", || async {
        h.session_status(&remembered).await == SessionStatus::Idle
    })
    .await;
    assert_eq!(h.attention(&remembered).await, None);
    assert_eq!(reply(&h, &remembered.id).await["decided_by"], "learned");
    assert_eq!(
        server.requests.lock().unwrap().len(),
        2,
        "the model decides first each time"
    );
}

#[tokio::test]
async fn a_guardrail_asks_the_console_without_calling_the_model_and_names_the_rule() {
    let server = ModelServer::answer(0.05).await;
    let (h, cast, _agent_dir) =
        ai_permissions_harness_with(&server, 0.7, Timeouts::default(), guardrail_script()).await;
    let _guard =
        tracing::subscriber::set_default(tracing_subscriber::registry().with(h.logs.layer()));

    let session = h.launcher.spawn_author(&cast.task.id).await.unwrap();
    wait_for_question(&h, &session).await;

    assert!(server.requests.lock().unwrap().is_empty());
    let snapshot: LogSnapshotResponse = h.get("/v1/logs").await;
    assert!(
        snapshot
            .lines
            .iter()
            .any(|line| line.level == "WARN" && line.message.contains("credential-paths")),
        "{snapshot:?}"
    );
    assert_eq!(
        permission_request(&h, &session.id).await["guardrail"],
        "credential-paths"
    );
    assert_eq!(
        model_part(&answered(&h, &session.id, "no").await),
        json!({"label": null, "confidence": null, "threshold": null,
               "guardrail": "credential-paths", "ai_error": null})
    );
}

#[tokio::test]
async fn a_learned_approval_does_not_answer_a_guardrail_request() {
    let server = ModelServer::answer(0.05).await;
    let (h, cast, _agent_dir) =
        ai_permissions_harness_with(&server, 0.7, Timeouts::default(), guardrail_script()).await;
    h.store
        .learn_permission(&cast.repo.id, "Bash", "execute")
        .await
        .unwrap();

    let session = h.launcher.spawn_author(&cast.task.id).await.unwrap();
    wait_for_question(&h, &session).await;

    assert!(server.requests.lock().unwrap().is_empty());
    assert_eq!(
        h.send(answer(&session.id, "no")).await.0,
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn an_answer_that_needs_review_waits_for_the_console() {
    let server = ModelServer::answer(0.99).await;
    let (h, cast, _agent_dir) = ai_permissions_harness(&server, 0.7, Timeouts::default()).await;
    let _guard =
        tracing::subscriber::set_default(tracing_subscriber::registry().with(h.logs.layer()));

    let session = h.launcher.spawn_author(&cast.task.id).await.unwrap();
    wait_for_question(&h, &session).await;
    assert_eq!(
        model_part(&permission_request(&h, &session.id).await),
        json!({"label": "escalate", "confidence": 0.01, "threshold": 0.7,
               "guardrail": null, "ai_error": null}),
        "the question carries the model's answer while it waits"
    );
    let reply = answered(&h, &session.id, "no").await;
    assert_eq!(reply["decided_by"], "console");
    assert_eq!(
        model_part(&reply),
        json!({"label": "escalate", "confidence": 0.01, "threshold": 0.7,
               "guardrail": null, "ai_error": null})
    );
    let snapshot: LogSnapshotResponse = h.get("/v1/logs").await;
    assert!(
        snapshot.lines.iter().any(|line| line.level == "INFO"
            && line.message.contains("AI permission decision")
            && line
                .message
                .contains("tool=Bash decided_by=console label=escalate")
            && line.message.contains("threshold=0.7")),
        "{snapshot:?}"
    );
}

#[tokio::test]
async fn an_allow_without_an_allowing_option_waits_for_the_console() {
    let server = ModelServer::answer(0.01).await;
    let mut scripted = permission_script();
    scripted["prompts"][0]["permission"]["options"] =
        json!([{"optionId": "no", "name": "Reject", "kind": "reject_once"}]);
    let (h, cast, _agent_dir) =
        ai_permissions_harness_with(&server, 0.7, Timeouts::default(), scripted).await;

    let session = h.launcher.spawn_author(&cast.task.id).await.unwrap();
    wait_for_question(&h, &session).await;
    assert_eq!(
        model_part(&answered(&h, &session.id, "no").await),
        json!({"label": "allow", "confidence": 0.99, "threshold": 0.7,
               "guardrail": null, "ai_error": null})
    );
}

#[tokio::test]
async fn a_malformed_answer_warns_and_waits_for_the_console() {
    let server = ModelServer::start(Answer::Value(json!({"answers": {}}))).await;
    let (h, cast, _agent_dir) = ai_permissions_harness(&server, 0.7, Timeouts::default()).await;
    let _guard =
        tracing::subscriber::set_default(tracing_subscriber::registry().with(h.logs.layer()));

    let session = h.launcher.spawn_author(&cast.task.id).await.unwrap();
    wait_for_question(&h, &session).await;
    let snapshot: LogSnapshotResponse = h.get("/v1/logs").await;
    assert!(
        snapshot.lines.iter().any(|line| line.level == "WARN"
            && line
                .message
                .contains("AI permission model decision was malformed")),
        "{snapshot:?}"
    );
    assert_eq!(
        model_part(&answered(&h, &session.id, "no").await),
        json!({"label": null, "confidence": null, "threshold": null,
               "guardrail": null, "ai_error": "malformed"})
    );
}

#[tokio::test]
async fn a_stopped_model_warns_and_waits_for_the_console() {
    let server = ModelServer::answer(0.05).await;
    let (h, cast, _agent_dir) = ai_permissions_harness(&server, 0.7, Timeouts::default()).await;
    server.stop();
    let _guard =
        tracing::subscriber::set_default(tracing_subscriber::registry().with(h.logs.layer()));

    let session = h.launcher.spawn_author(&cast.task.id).await.unwrap();
    wait_for_question(&h, &session).await;
    let snapshot: LogSnapshotResponse = h.get("/v1/logs").await;
    assert!(
        snapshot.lines.iter().any(|line| line.level == "WARN"
            && line.message.contains("AI permission model decision failed")),
        "{snapshot:?}"
    );
    assert_eq!(
        permission_request(&h, &session.id).await["ai_error"],
        "failed"
    );
    assert_eq!(
        model_part(&answered(&h, &session.id, "no").await),
        json!({"label": null, "confidence": null, "threshold": null,
               "guardrail": null, "ai_error": "failed"})
    );
}

#[tokio::test]
async fn a_model_timeout_waits_for_the_console() {
    let server = ModelServer::hanging().await;
    let timeouts = Timeouts {
        ai_permissions_decision: RUNS_OUT,
        ..Timeouts::default()
    };
    let (h, cast, _agent_dir) = ai_permissions_harness(&server, 0.7, timeouts).await;

    let session = h.launcher.spawn_author(&cast.task.id).await.unwrap();
    wait_for_question(&h, &session).await;
    assert_eq!(
        model_part(&answered(&h, &session.id, "no").await),
        json!({"label": null, "confidence": null, "threshold": null,
               "guardrail": null, "ai_error": "timed out"})
    );
}

#[tokio::test]
async fn a_disabled_model_waits_for_the_console() {
    let server = ModelServer::answer(0.05).await;
    let (h, cast, _agent_dir) = ai_permissions_harness(&server, 0.7, Timeouts::default()).await;
    let _: Value = h
        .json(
            put_json("/v1/permissions/ai", json!({"enabled": false})),
            StatusCode::OK,
        )
        .await;

    let session = h.launcher.spawn_author(&cast.task.id).await.unwrap();
    wait_for_question(&h, &session).await;
    assert!(server.requests.lock().unwrap().is_empty());
    assert_eq!(
        model_part(&answered(&h, &session.id, "no").await),
        json!({"label": null, "confidence": null, "threshold": null,
               "guardrail": null, "ai_error": "unavailable"})
    );
}
