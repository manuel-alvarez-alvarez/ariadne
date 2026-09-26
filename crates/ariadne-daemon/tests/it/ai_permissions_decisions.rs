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
    /// An answer shaped as `laya-serve` returns it. Its `confidence` is the
    /// uncalibrated entropy score, set far from the calibrated one so a
    /// decision gated on the wrong field fails.
    async fn answer(label: &str, confidence: f64) -> Self {
        let mut decision = choice(label, confidence);
        decision["answer_confidence"] = json!(confidence);
        Self::start(Answer::Value(response(decision))).await
    }

    /// An answer shaped as the ONNX agent returns it: no `answer_confidence`.
    async fn onnx_answer(label: &str, confidence: f64) -> Self {
        Self::start(Answer::Value(response(choice(label, confidence)))).await
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

fn choice(label: &str, confidence: f64) -> Value {
    let other = if label == "allow" { "review" } else { "allow" };
    json!({
        "type": "choice",
        "choice": label,
        "probabilities": {label: confidence, other: 1.0 - confidence},
        "confidence": 0.05,
        "action": {"act_probability": 1.0}
    })
}

fn response(decision: Value) -> Value {
    json!({
        "model": "rl-agent",
        "answers": {"decision": decision},
        "usage": {"input_tokens": 139, "output_tokens": 0},
        "routing": {"model": "english"}
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
            "toolCall": {"toolCallId": "call-1", "title": "Bash", "kind": "execute",
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

async fn wait_for_question(h: &Harness, session: &ariadne_store::AgentSession) {
    eventually(TIMEOUT, "the permission request to wait", || async {
        h.attention(session).await == Some(AttentionReason::WaitingPermission)
    })
    .await;
}

#[tokio::test]
async fn a_confident_allow_runs_at_once_and_reports_ai() {
    let server = ModelServer::answer("allow", 0.95).await;
    let (h, cast, _agent_dir) = ai_permissions_harness(&server, 0.8, Timeouts::default()).await;

    let session = h.launcher.spawn_author(&cast.task.id).await.unwrap();
    eventually(TIMEOUT, "the AI-approved turn to finish", || async {
        h.session_status(&session).await == SessionStatus::Idle
    })
    .await;

    assert_eq!(h.attention(&session).await, None);
    assert_eq!(
        reply(&h, &session.id).await,
        json!({"session_id": "stub-session", "option_id": "yes", "decided_by": "ai",
               "label": "allow", "confidence": 0.95})
    );
    let requests = server.requests.lock().unwrap();
    assert_eq!(requests[0]["state"]["tool"], "Bash");
    assert!(
        requests[0]["state"]["input"]
            .as_str()
            .unwrap()
            .contains("cargo test")
    );
    assert_eq!(
        requests[0]["state"]["repository"],
        session.worktree_path.as_deref().unwrap()
    );
    assert_eq!(requests[0]["state"]["options"], "Reject, Allow");
    assert_eq!(
        requests[0]["questions"]["decision"]["instructions"],
        "Can this coding-agent tool call run without a person's review?"
    );
    assert_eq!(
        requests[0]["questions"]["decision"]["criteria"]["allow"],
        "reading files, searching, listing, building, running tests, editing files inside the working tree, git commands that do not delete branches or force-push"
    );
    assert_eq!(
        requests[0]["questions"]["decision"]["criteria"]["review"],
        "deleting outside the working tree, force pushes, package installs, network writes, credentials or secrets, changes to system configuration, anything unclear"
    );
}

/// The question a `PUT` sets is the `instructions` the daemon sends the model
/// on the next decision: the decision reads the settings each time, not once.
#[tokio::test]
async fn a_changed_question_reaches_the_model_on_the_next_decision() {
    let server = ModelServer::answer("allow", 0.95).await;
    let (h, cast, _agent_dir) = ai_permissions_harness(&server, 0.8, Timeouts::default()).await;
    let _: Value = h
        .json(
            put_json(
                "/v1/permissions/ai",
                json!({"question": "May this run unattended?"}),
            ),
            StatusCode::OK,
        )
        .await;

    let session = h.launcher.spawn_author(&cast.task.id).await.unwrap();
    eventually(TIMEOUT, "the AI-approved turn to finish", || async {
        h.session_status(&session).await == SessionStatus::Idle
    })
    .await;

    let requests = server.requests.lock().unwrap();
    let decision = &requests[0]["questions"]["decision"];
    assert_eq!(decision["instructions"], "May this run unattended?");
    assert_eq!(
        decision["criteria"]["allow"],
        "reading files, searching, listing, building, running tests, editing files inside the working tree, git commands that do not delete branches or force-push",
        "an unset criterion is still the built-in one"
    );
}

/// The criteria a `PUT` sets are the `criteria` of the choice, under the
/// fixed answer names `allow` and `review`.
#[tokio::test]
async fn changed_criteria_reach_the_model_under_the_fixed_answer_names() {
    let server = ModelServer::answer("allow", 0.95).await;
    let (h, cast, _agent_dir) = ai_permissions_harness(&server, 0.8, Timeouts::default()).await;
    let _: Value = h
        .json(
            put_json(
                "/v1/permissions/ai",
                json!({"allow_criteria": "tests and builds", "review_criteria": "everything else"}),
            ),
            StatusCode::OK,
        )
        .await;

    let session = h.launcher.spawn_author(&cast.task.id).await.unwrap();
    eventually(TIMEOUT, "the AI-approved turn to finish", || async {
        h.session_status(&session).await == SessionStatus::Idle
    })
    .await;

    let requests = server.requests.lock().unwrap();
    let decision = &requests[0]["questions"]["decision"];
    assert_eq!(
        decision["criteria"],
        json!({"allow": "tests and builds", "review": "everything else"})
    );
    assert_eq!(
        decision["instructions"], "Can this coding-agent tool call run without a person's review?",
        "an unset question is still the built-in one"
    );
}

/// A `laya-serve` that records its pid, loads for as long as its second
/// argument says, and then allows every request with the calibrated
/// confidence 0.95.
const SLOW_SERVER: &str = r#"#!/usr/bin/env python3
import http.server, json, os, sys, time
with open(sys.argv[1], 'w') as f:
    f.write(str(os.getpid()) + '\n')
time.sleep(float(sys.argv[2]))
ANSWER = json.dumps({"answers": {"decision": {"type": "choice", "choice": "allow",
    "probabilities": {"allow": 0.95, "review": 0.05}, "confidence": 0.05,
    "answer_confidence": 0.95}}, "usage": {}, "routing": {}}).encode()
class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        self.send_response(200 if self.path == '/health' else 404)
        self.end_headers()
    def do_POST(self):
        self.rfile.read(int(self.headers.get('content-length', 0)))
        self.send_response(200)
        self.send_header('content-type', 'application/json')
        self.end_headers()
        self.wfile.write(ANSWER)
    def log_message(self, *args): pass
http.server.HTTPServer((os.environ['LAYA_HOST'], int(os.environ['LAYA_PORT'])), Handler).serve_forever()
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
        0.8,
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
async fn an_answer_without_its_calibrated_confidence_is_gated_on_its_probability() {
    let server = ModelServer::onnx_answer("allow", 0.95).await;
    let (h, cast, _agent_dir) = ai_permissions_harness(&server, 0.8, Timeouts::default()).await;

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
    let server = ModelServer::answer("allow", 0.6).await;
    let (h, cast, _agent_dir) = ai_permissions_harness(&server, 0.8, Timeouts::default()).await;

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
    assert_eq!(reply(&h, &asked.id).await["decided_by"], "console");

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
async fn a_review_answer_waits_for_the_console() {
    let server = ModelServer::answer("review", 0.99).await;
    let (h, cast, _agent_dir) = ai_permissions_harness(&server, 0.8, Timeouts::default()).await;

    let session = h.launcher.spawn_author(&cast.task.id).await.unwrap();
    wait_for_question(&h, &session).await;
    assert_eq!(
        h.send(answer(&session.id, "no")).await.0,
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn an_allow_without_an_allowing_option_waits_for_the_console() {
    let server = ModelServer::answer("allow", 0.99).await;
    let mut scripted = permission_script();
    scripted["prompts"][0]["permission"]["options"] =
        json!([{"optionId": "no", "name": "Reject", "kind": "reject_once"}]);
    let (h, cast, _agent_dir) =
        ai_permissions_harness_with(&server, 0.8, Timeouts::default(), scripted).await;

    let session = h.launcher.spawn_author(&cast.task.id).await.unwrap();
    wait_for_question(&h, &session).await;
    assert_eq!(
        h.send(answer(&session.id, "no")).await.0,
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn a_malformed_answer_warns_and_waits_for_the_console() {
    let server = ModelServer::start(Answer::Value(json!({"answers": {}}))).await;
    let (h, cast, _agent_dir) = ai_permissions_harness(&server, 0.8, Timeouts::default()).await;
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
        h.send(answer(&session.id, "no")).await.0,
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn a_stopped_model_warns_and_waits_for_the_console() {
    let server = ModelServer::answer("allow", 0.95).await;
    let (h, cast, _agent_dir) = ai_permissions_harness(&server, 0.8, Timeouts::default()).await;
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
        h.send(answer(&session.id, "no")).await.0,
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn a_model_timeout_waits_for_the_console() {
    let server = ModelServer::hanging().await;
    let timeouts = Timeouts {
        ai_permissions_decision: RUNS_OUT,
        ..Timeouts::default()
    };
    let (h, cast, _agent_dir) = ai_permissions_harness(&server, 0.8, timeouts).await;

    let session = h.launcher.spawn_author(&cast.task.id).await.unwrap();
    wait_for_question(&h, &session).await;
    assert_eq!(
        h.send(answer(&session.id, "no")).await.0,
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn a_disabled_model_waits_for_the_console() {
    let server = ModelServer::answer("allow", 0.95).await;
    let (h, cast, _agent_dir) = ai_permissions_harness(&server, 0.8, Timeouts::default()).await;
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
        h.send(answer(&session.id, "no")).await.0,
        StatusCode::NO_CONTENT
    );
}
