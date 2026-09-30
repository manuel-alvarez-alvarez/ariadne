//! Permission decisions made through the model in `ai` mode (022, Decisions).

use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::State;
use axum::http::{Request, StatusCode};
use axum::routing::post as route_post;
use serde_json::{Value, json};
use tracing_subscriber::layer::SubscriberExt;

use ariadne_api::logs::LogSnapshotResponse;
use ariadne_api::permissions::TestAiPermissionResponse;
use ariadne_core::{Actor, AttentionReason, PermissionMode, SessionStatus, TaskStatus};
use ariadne_daemon::timeouts::Timeouts;
use ariadne_store::{AgentPin, AiPermissionSettingsUpdate, EventFilter};

use crate::common::acp::{registry_home, script, stub_acp_agent};
use crate::common::{
    Cast, Harness, HarnessBuilder, QUIET, RUNS_OUT, TIMEOUT, eventually, harness, post_json,
    put_json, shared_script,
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
    /// An answer shaped as the winning `score` question returns it.
    async fn answer(danger: f64) -> Self {
        let score = danger * 2.0;
        let probabilities = if score <= 1.0 {
            json!({"0": 1.0 - score, "1": score, "2": 0.0})
        } else {
            json!({"0": 0.0, "1": 2.0 - score, "2": score - 1.0})
        };
        Self::start(Answer::Value(response(json!({
            "type": "score",
            "score": score,
            "probabilities": probabilities,
        }))))
        .await
    }

    async fn hanging() -> Self {
        Self::start(Answer::Hang).await
    }

    async fn malformed() -> Self {
        Self::start(Answer::Value(json!({}))).await
    }

    async fn start(answer: Answer) -> Self {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let state = ServerState {
            answer,
            requests: requests.clone(),
        };
        let app = axum::Router::new()
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

/// A call the daemon once sent to a person by rule, before asking the model.
fn ssh_key_script() -> Value {
    let mut scripted = permission_script();
    scripted["prompts"][0]["permission"]["toolCall"]["rawInput"] =
        json!({"command": "cat ~/.ssh/id_rsa"});
    scripted
}

fn home_delete_script() -> Value {
    let mut scripted = permission_script();
    scripted["prompts"][0]["permission"]["toolCall"]["rawInput"] = json!({"command": "rm -rf ~"});
    scripted
}

fn python() -> String {
    shared_script("#!/bin/sh\necho 'Python 3.12.1'\n")
        .display()
        .to_string()
}

async fn enabled_test_harness(server: &ModelServer, timeouts: Timeouts) -> Harness {
    let h = harness()
        .ai_permissions_endpoint(server.endpoint.clone())
        .python_bin(python())
        .ai_permissions_installer(vec!["/usr/bin/true".into()])
        .timeouts(timeouts)
        .await;
    let _: Value = h
        .json(
            put_json("/v1/permissions/ai", json!({"enabled": true})),
            StatusCode::OK,
        )
        .await;
    h
}

async fn test_error(h: &Harness) -> String {
    h.json::<TestAiPermissionResponse>(
        post_json(
            "/v1/permissions/ai/test",
            json!({"tool":"Bash", "input":{}}),
        ),
        StatusCode::OK,
    )
    .await
    .ai_error
    .unwrap()
}

#[tokio::test]
async fn a_test_request_scores_the_same_model_state_without_publishing_or_learning() {
    let server = ModelServer::answer(0.41).await;
    let h = harness()
        .ai_permissions_endpoint(server.endpoint.clone())
        .python_bin(python())
        .await;
    h.store
        .update_ai_permission_settings(AiPermissionSettingsUpdate {
            enabled: Some(true),
            allow_threshold: Some(0.13),
            deny_threshold: Some(0.53),
            ..Default::default()
        })
        .await
        .unwrap();
    let mut events = h.bus.subscribe();

    let response: TestAiPermissionResponse = h
        .json(
            post_json(
                "/v1/permissions/ai/test",
                json!({"tool":"Bash", "kind":"execute", "input":{"command":"git status"},
                       "options":["Allow", "Reject"], "workspace":"/repo/ariadne"}),
            ),
            StatusCode::OK,
        )
        .await;

    assert_eq!(response.label.as_deref(), Some("ask"));
    assert_eq!(response.danger, Some(0.41));
    assert_eq!(response.allow_threshold, 0.13);
    assert_eq!(response.deny_threshold, 0.53);
    assert_eq!(response.ai_error, None);
    assert_eq!(response.operation.as_deref(), Some("read_workspace"));
    assert_eq!(response.risk_tags, Some(vec![]));
    assert_eq!(response.rule, None);
    assert_eq!(response.cap, None);
    assert!(response.probabilities.is_some());
    assert!(
        h.store
            .list_learned_permissions(None)
            .await
            .unwrap()
            .is_empty(),
        "the test wrote a learned approval"
    );
    assert_eq!(
        server.requests.lock().unwrap()[0]["state"],
        json!({
            "task":{"workspace":"/repo/ariadne"},
            "request":{"tool":"Bash", "kind":"execute", "input":"{\"command\":\"git status\"}"},
            "permission_options":["Allow", "Reject"]
        })
    );
    let ruled: TestAiPermissionResponse = h
        .json(
            post_json(
                "/v1/permissions/ai/test",
                json!({"tool":"Bash", "kind":"execute", "input":{"command":"rm -rf ~"},
                       "workspace":"/repo/ariadne"}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(ruled.label.as_deref(), Some("deny"));
    assert_eq!(ruled.danger, None);
    assert_eq!(ruled.rule.as_deref(), Some("home_delete"));
    assert_eq!(server.requests.lock().unwrap().len(), 1);
    assert!(
        tokio::time::timeout(QUIET, events.recv()).await.is_err(),
        "the test published an event"
    );
}

#[tokio::test]
async fn a_test_request_reports_unavailable_or_a_model_error_without_failing_the_endpoint() {
    let h = harness().await;
    let empty: ariadne_api::error::ErrorBody = h
        .json(
            post_json("/v1/permissions/ai/test", json!({"tool":"", "input":{}})),
            StatusCode::UNPROCESSABLE_ENTITY,
        )
        .await;
    assert_eq!(empty.error.code, "invalid_request");
    let disabled: ariadne_api::error::ErrorBody = h
        .json(
            post_json(
                "/v1/permissions/ai/test",
                json!({"tool":"Bash", "input":{}}),
            ),
            StatusCode::CONFLICT,
        )
        .await;
    assert_eq!(disabled.error.code, "ai_disabled");

    let unavailable = harness()
        .python_bin(python())
        .ai_permissions_installer(vec!["/usr/bin/true".into()])
        .await;
    let _: Value = unavailable
        .json(
            put_json("/v1/permissions/ai", json!({"enabled": true})),
            StatusCode::OK,
        )
        .await;
    let response: TestAiPermissionResponse = unavailable
        .json(
            post_json(
                "/v1/permissions/ai/test",
                json!({"tool":"Bash", "input":{}}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(response.label, None);
    assert_eq!(response.danger, None);
    assert_eq!(response.ai_error.as_deref(), Some("unavailable"));
}

#[tokio::test]
async fn a_test_request_returns_each_model_call_error_in_its_response() {
    let failed_server = ModelServer::answer(0.41).await;
    let failed = enabled_test_harness(&failed_server, Timeouts::default()).await;
    failed_server.stop();
    assert_eq!(test_error(&failed).await, "failed");

    let timeout_server = ModelServer::hanging().await;
    let timed_out = enabled_test_harness(
        &timeout_server,
        Timeouts {
            ai_permissions_decision: RUNS_OUT,
            ..Timeouts::default()
        },
    )
    .await;
    assert_eq!(test_error(&timed_out).await, "timed out");

    let malformed_server = ModelServer::malformed().await;
    let malformed = enabled_test_harness(&malformed_server, Timeouts::default()).await;
    assert_eq!(test_error(&malformed).await, "malformed");

    let probability_server = ModelServer::start(Answer::Value(response(json!({
        "type": "score", "score": 0.5,
        "probabilities": {"0": 1.2, "1": -0.2, "2": 0.0}
    }))))
    .await;
    let probability = enabled_test_harness(&probability_server, Timeouts::default()).await;
    assert_eq!(test_error(&probability).await, "malformed");
}

async fn ai_permissions_harness(
    server: &ModelServer,
    allow_threshold: f64,
    timeouts: Timeouts,
) -> (Harness, Cast, tempfile::TempDir) {
    ai_permissions_harness_with(server, allow_threshold, timeouts, permission_script()).await
}

async fn ai_permissions_harness_with(
    server: &ModelServer,
    allow_threshold: f64,
    timeouts: Timeouts,
    scripted: Value,
) -> (Harness, Cast, tempfile::TempDir) {
    let endpoint = server.endpoint.clone();
    ai_permissions_harness_on(
        |builder| builder.ai_permissions_endpoint(endpoint),
        server,
        allow_threshold,
        timeouts,
        scripted,
    )
    .await
}

/// A harness whose model is reached however `ai_permissions` says: a pinned endpoint, or
/// a server the daemon starts.
async fn ai_permissions_harness_on(
    ai_permissions: impl FnOnce(HarnessBuilder) -> HarnessBuilder,
    _server: &ModelServer,
    allow_threshold: f64,
    timeouts: Timeouts,
    scripted: Value,
) -> (Harness, Cast, tempfile::TempDir) {
    let agent_dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(agent_dir.path(), scripted);
    let h = ai_permissions(harness())
        .home(registry_home(&stub))
        .ai_permissions_installer(vec!["/bin/sh".into(), "-c".into(), "exit 0".into()])
        .python_bin(python())
        .timeouts(timeouts)
        .await;
    let _: Value = h
        .json(
            put_json(
                "/v1/permissions/ai",
                json!({"enabled": true, "allow_threshold": allow_threshold,
                       "deny_threshold": 0.8}),
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

/// The model's side of a reply: its label, danger, both thresholds and error.
fn model_part(reply: &Value) -> Value {
    json!({
        "label": reply["label"],
        "danger": reply["danger"].as_f64().map(|c| (c * 1e4).round() / 1e4),
        "allow_threshold": reply["allow_threshold"],
        "deny_threshold": reply["deny_threshold"],
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
    let (h, cast, _agent_dir) = ai_permissions_harness(&server, 0.05, Timeouts::default()).await;

    let session = h.launcher.spawn_author(&cast.task.id).await.unwrap();
    eventually(TIMEOUT, "the AI-approved turn to finish", || async {
        h.session_status(&session).await == SessionStatus::Idle
    })
    .await;

    assert_eq!(h.attention(&session).await, None);
    assert_eq!(
        reply(&h, &session.id).await,
        json!({"session_id": "stub-session", "option_id": "yes", "decided_by": "ai",
               "label": "allow", "danger": 0.05, "allow_threshold": 0.05,
               "deny_threshold": 0.8,
               "ai_error": null, "operation": "build_test", "risk_tags": [],
               "rule": null, "cap": null,
               "probabilities": {"0": 0.9, "1": 0.1, "2": 0.0}})
    );
    let requests = server.requests.lock().unwrap();
    assert_eq!(
        requests[0]["state"]["request"],
        json!({
            "tool": "Bash", "kind": "execute", "input": "{\"command\":\"cargo test -p app\"}"
        }),
        "the normalized call"
    );
    assert_eq!(
        requests[0]["state"]["permission_options"],
        json!(["Reject", "Allow"])
    );
    assert!(requests[0]["state"]["task"]["workspace"].is_string());
    assert_eq!(requests[0]["model"], "kev-latest");
    assert_eq!(requests[0]["questions"]["decision"]["type"], "score");
    assert_eq!(
        requests[0]["questions"]["decision"]["instructions"],
        "How much does this coding-agent tool call put the system or the project at risk? Do not ask only because a command runs, a file changes, a project file is deleted, or git is used. Do not deny only because an operation is powerful."
    );
    assert_eq!(
        requests[0]["questions"]["decision"]["criteria"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
}

/// A `kev.serve` that records its pid, loads for as long as its second
/// argument says, reports the device its environment chose, and then allows
/// every request with the calibrated danger 0.05.
const SLOW_SERVER: &str = r#"#!/usr/bin/env python3
import http.server, json, os, sys, time
with open(sys.argv[1], 'w') as f:
    f.write(str(os.getpid()) + '\n')
time.sleep(float(sys.argv[2]))
args = sys.argv[3:]
host = args[args.index('--host') + 1]
port = int(args[args.index('--port') + 1])
ANSWER = json.dumps({"model": "kev-latest",
    "answers": {"decision": {"type": "score", "score": 0.1,
                                  "probabilities": {"0": 0.9, "1": 0.1, "2": 0.0}}},
    "usage": {}, "latency_ms": 12.3}).encode()
BACKEND = os.environ['KEV_BACKEND']
DEVICE = 'mps' if BACKEND == 'mlx' else 'cpu' if os.environ.get('KEV_DTYPE') == 'fp32' else 'cuda'
CARD = json.dumps({"models": [{"name": "kev-latest", "device": DEVICE, "backend": BACKEND}]}).encode()
class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        self.send_response(200 if self.path == '/v1/models' else 404)
        self.end_headers()
        if self.path == '/v1/models':
            self.wfile.write(CARD)
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
        0.2,
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
    assert_eq!(reply["danger"], 0.05);
    h.state.ai_permissions.shutdown().await;
}

#[tokio::test]
async fn a_score_answer_is_used_as_the_danger() {
    let server = ModelServer::answer(0.05).await;
    let (h, cast, _agent_dir) = ai_permissions_harness(&server, 0.2, Timeouts::default()).await;

    let session = h.launcher.spawn_author(&cast.task.id).await.unwrap();
    eventually(TIMEOUT, "the AI-approved turn to finish", || async {
        h.session_status(&session).await == SessionStatus::Idle
    })
    .await;

    assert_eq!(h.attention(&session).await, None);
    let reply = reply(&h, &session.id).await;
    assert_eq!(reply["decided_by"], "ai");
    assert_eq!(reply["danger"], 0.05);
}

#[tokio::test]
async fn a_score_equal_to_the_deny_threshold_is_denied() {
    let server = ModelServer::answer(0.8).await;
    let (h, cast, _agent_dir) = ai_permissions_harness(&server, 0.2, Timeouts::default()).await;

    let session = h.launcher.spawn_author(&cast.task.id).await.unwrap();
    eventually(TIMEOUT, "the boundary denial to finish", || async {
        h.session_status(&session).await == SessionStatus::Idle
    })
    .await;

    let reply = reply(&h, &session.id).await;
    assert_eq!(reply["decided_by"], "ai");
    assert_eq!(reply["label"], "deny");
    assert_eq!(reply["danger"], 0.8);
}

#[tokio::test]
async fn an_uncertain_allow_falls_to_console_and_then_to_the_learned_approval() {
    let server = ModelServer::answer(0.4).await;
    let (h, cast, _agent_dir) = ai_permissions_harness(&server, 0.2, Timeouts::default()).await;

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
        json!({"label": "ask", "danger": 0.4, "allow_threshold": 0.2,
               "deny_threshold": 0.8,
               "ai_error": null}),
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
async fn a_cap_changes_a_model_allow_to_a_console_question() {
    let server = ModelServer::answer(0.05).await;
    let (h, cast, _agent_dir) =
        ai_permissions_harness_with(&server, 0.2, Timeouts::default(), ssh_key_script()).await;

    let session = h.launcher.spawn_author(&cast.task.id).await.unwrap();
    wait_for_question(&h, &session).await;
    let request = permission_request(&h, &session.id).await;
    assert_eq!(request["label"], "ask");
    assert_eq!(request["danger"], 0.05);
    assert_eq!(request["operation"], "secrets_credentials");
    assert_eq!(request["cap"], "credential_access");
    assert_eq!(
        request["risk_tags"],
        json!(["outside_workspace", "remote", "credential_access"])
    );
    let requests = server.requests.lock().unwrap().clone();
    assert_eq!(requests.len(), 1, "the model was asked");
    assert!(
        requests[0]["state"]["request"]["input"]
            .as_str()
            .unwrap()
            .contains("cat ~/.ssh/id_rsa")
    );
    drop(requests);
    let reply = answered(&h, &session.id, "no").await;
    assert_eq!(reply["decided_by"], "console");
    assert_eq!(reply["cap"], "credential_access");
}

#[tokio::test]
async fn a_hard_rule_denies_without_the_model_attention_or_learning() {
    let server = ModelServer::answer(0.05).await;
    let (h, cast, _agent_dir) =
        ai_permissions_harness_with(&server, 0.2, Timeouts::default(), home_delete_script()).await;

    let session = h.launcher.spawn_author(&cast.task.id).await.unwrap();
    eventually(TIMEOUT, "the rule-denied turn to finish", || async {
        h.session_status(&session).await == SessionStatus::Idle
    })
    .await;

    assert!(server.requests.lock().unwrap().is_empty());
    assert_eq!(h.attention(&session).await, None);
    let reply = reply(&h, &session.id).await;
    assert_eq!(reply["decided_by"], "rule");
    assert_eq!(reply["option_id"], "no");
    assert_eq!(reply["label"], "deny");
    assert_eq!(reply["danger"], Value::Null);
    assert_eq!(reply["rule"], "home_delete");
    assert_eq!(reply["probabilities"], Value::Null);
    assert!(
        h.store
            .list_learned_permissions(Some(&cast.repo.id))
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn a_hard_rule_without_a_one_time_reject_asks_without_learning() {
    let server = ModelServer::answer(0.05).await;
    let mut scripted = home_delete_script();
    scripted["prompts"][0]["permission"]["options"] = json!([
        {"optionId": "never", "name": "Reject always", "kind": "reject_always"},
        {"optionId": "yes", "name": "Allow", "kind": "allow_once"}
    ]);
    let (h, cast, _agent_dir) =
        ai_permissions_harness_with(&server, 0.2, Timeouts::default(), scripted).await;

    let session = h.launcher.spawn_author(&cast.task.id).await.unwrap();
    wait_for_question(&h, &session).await;
    let request = permission_request(&h, &session.id).await;
    assert_eq!(request["rule"], "home_delete");
    assert_eq!(request["label"], "deny");
    let reply = answered(&h, &session.id, "yes").await;
    assert_eq!(reply["decided_by"], "console");
    assert_eq!(reply["rule"], "home_delete");
    assert!(server.requests.lock().unwrap().is_empty());
    assert!(
        h.store
            .list_learned_permissions(Some(&cast.repo.id))
            .await
            .unwrap()
            .is_empty(),
        "a rule decision was learned"
    );
}

#[tokio::test]
async fn a_confident_deny_selects_the_rejecting_option_and_reports_ai() {
    let server = ModelServer::answer(0.99).await;
    let (h, cast, _agent_dir) = ai_permissions_harness(&server, 0.2, Timeouts::default()).await;
    let _guard =
        tracing::subscriber::set_default(tracing_subscriber::registry().with(h.logs.layer()));

    let session = h.launcher.spawn_author(&cast.task.id).await.unwrap();
    eventually(TIMEOUT, "the AI-denied turn to finish", || async {
        h.session_status(&session).await == SessionStatus::Idle
    })
    .await;
    assert_eq!(h.attention(&session).await, None);
    let reply = reply(&h, &session.id).await;
    assert_eq!(reply["decided_by"], "ai");
    assert_eq!(reply["option_id"], "no");
    assert_eq!(
        model_part(&reply),
        json!({"label": "deny", "danger": 0.99, "allow_threshold": 0.2,
               "deny_threshold": 0.8,
               "ai_error": null})
    );
    assert!(
        !h.store
            .has_learned_permission(&cast.repo.id, "Bash", "execute")
            .await
            .unwrap(),
        "the denial was not learned"
    );
    let snapshot: LogSnapshotResponse = h.get("/v1/logs").await;
    assert!(
        snapshot.lines.iter().any(|line| line.level == "INFO"
            && line.message.contains("AI permission decision")
            && line.message.contains("tool=Bash decided_by=ai label=deny")
            && line.message.contains("danger=0.99")
            && line.message.contains("allow_threshold=0.2")
            && line.message.contains("deny_threshold=0.8")),
        "{snapshot:?}"
    );
}

#[tokio::test]
async fn a_deny_without_a_rejecting_option_waits_for_the_console() {
    let server = ModelServer::answer(0.99).await;
    let mut scripted = permission_script();
    scripted["prompts"][0]["permission"]["options"] = json!([
        {"optionId": "never", "name": "Reject always", "kind": "reject_always"},
        {"optionId": "yes", "name": "Allow", "kind": "allow_once"}
    ]);
    let (h, cast, _agent_dir) =
        ai_permissions_harness_with(&server, 0.2, Timeouts::default(), scripted).await;

    let session = h.launcher.spawn_author(&cast.task.id).await.unwrap();
    wait_for_question(&h, &session).await;
    assert_eq!(
        model_part(&permission_request(&h, &session.id).await),
        json!({"label": "deny", "danger": 0.99, "allow_threshold": 0.2,
               "deny_threshold": 0.8, "ai_error": null})
    );
    assert_eq!(
        answered(&h, &session.id, "yes").await["decided_by"],
        "console"
    );
    let learned = h
        .store
        .list_learned_permissions(Some(&cast.repo.id))
        .await
        .unwrap();
    assert_eq!(learned.len(), 1);
    assert_eq!(learned[0].label.as_deref(), Some("deny"));
    assert_eq!(learned[0].danger, Some(0.99));
    assert_eq!(learned[0].allow_threshold, Some(0.2));
    assert_eq!(learned[0].deny_threshold, Some(0.8));
}

#[tokio::test]
async fn an_allow_without_an_allowing_option_waits_for_the_console() {
    let server = ModelServer::answer(0.01).await;
    let mut scripted = permission_script();
    scripted["prompts"][0]["permission"]["options"] =
        json!([{"optionId": "no", "name": "Reject", "kind": "reject_once"}]);
    let (h, cast, _agent_dir) =
        ai_permissions_harness_with(&server, 0.2, Timeouts::default(), scripted).await;

    let session = h.launcher.spawn_author(&cast.task.id).await.unwrap();
    wait_for_question(&h, &session).await;
    assert_eq!(
        model_part(&answered(&h, &session.id, "no").await),
        json!({"label": "allow", "danger": 0.01, "allow_threshold": 0.2,
               "deny_threshold": 0.8,
               "ai_error": null})
    );
}

#[tokio::test]
async fn a_malformed_answer_warns_and_waits_for_the_console() {
    let server = ModelServer::start(Answer::Value(json!({"answers": {}}))).await;
    let (h, cast, _agent_dir) = ai_permissions_harness(&server, 0.2, Timeouts::default()).await;
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
        json!({"label": null, "danger": null, "allow_threshold": null,
               "deny_threshold": null,
               "ai_error": "malformed"})
    );
}

#[tokio::test]
async fn a_stopped_model_warns_and_waits_for_the_console() {
    let server = ModelServer::answer(0.05).await;
    let (h, cast, _agent_dir) = ai_permissions_harness(&server, 0.2, Timeouts::default()).await;
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
        json!({"label": null, "danger": null, "allow_threshold": null,
               "deny_threshold": null,
               "ai_error": "failed"})
    );
}

#[tokio::test]
async fn a_model_timeout_waits_for_the_console() {
    let server = ModelServer::hanging().await;
    let timeouts = Timeouts {
        ai_permissions_decision: RUNS_OUT,
        ..Timeouts::default()
    };
    let (h, cast, _agent_dir) = ai_permissions_harness(&server, 0.2, timeouts).await;

    let session = h.launcher.spawn_author(&cast.task.id).await.unwrap();
    wait_for_question(&h, &session).await;
    assert_eq!(
        model_part(&answered(&h, &session.id, "no").await),
        json!({"label": null, "danger": null, "allow_threshold": null,
               "deny_threshold": null,
               "ai_error": "timed out"})
    );
}

#[tokio::test]
async fn a_disabled_model_waits_for_the_console() {
    let server = ModelServer::answer(0.05).await;
    let (h, cast, _agent_dir) = ai_permissions_harness(&server, 0.2, Timeouts::default()).await;
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
        json!({"label": null, "danger": null, "allow_threshold": null,
               "deny_threshold": null,
               "ai_error": "unavailable"})
    );
}
