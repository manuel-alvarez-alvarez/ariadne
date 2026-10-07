//! The optional advisory diagnosis of a failed ACP session (024): a stub
//! Kev answers beside a stub ACP failure, and the original error, the
//! session's end, and the daemon's switching behavior never wait on it or
//! change because of it.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::post as route_post;
use serde_json::{Value, json};
use tokio::sync::Semaphore;

use ariadne_core::{PermissionMode, SessionStatus, TaskStatus};
use ariadne_daemon::timeouts::Timeouts;
use ariadne_store::{AgentPin, AgentSession, EventFilter, Store};

use crate::common::acp::{discovery_accepted, script, stub_acp_agent};
use crate::common::{Harness, QUIET, TIMEOUT, eventually, harness, put_json, test_pin};

#[derive(Clone)]
enum Answer {
    Value(Value),
    Hang,
    Delayed(Duration, Value),
    /// Hangs on a `diagnosis` question, answers a `decision` question at
    /// once — one worker a diagnosis in flight occupies until a permission
    /// decision preempts it (024).
    DiagnosisHangsDecisionAnswers(Value),
}

#[derive(Clone)]
struct ServerState {
    answer: Answer,
    requests: Arc<Mutex<Vec<Value>>>,
    /// One real Kev serves one request at a time. Held for the whole of a
    /// handler's work, so a second request genuinely waits behind a first
    /// that hangs — and is freed the moment that first one's connection is
    /// aborted, which is the one thing a preempting permission decision does
    /// differently from waiting its turn.
    worker: Arc<Semaphore>,
}

/// A stub Kev, answering `/v1/systemone` the way the real service does for a
/// `choice` question (`kev.api.to_answers`).
struct ModelServer {
    endpoint: String,
    requests: Arc<Mutex<Vec<Value>>>,
    task: tokio::task::JoinHandle<()>,
}

impl ModelServer {
    async fn answer(choice: &str, probabilities: Value) -> Self {
        Self::answer_from("kev-latest", choice, probabilities).await
    }

    /// An answer served from `model` — distinct from the request's own
    /// `kev-latest`, to prove the stored diagnosis names the model the
    /// response actually carries rather than a hardcoded one.
    async fn answer_from(model: &str, choice: &str, probabilities: Value) -> Self {
        Self::start(Answer::Value(diagnosis_answer(
            model,
            choice,
            probabilities,
        )))
        .await
    }

    /// An answer that takes `delay` to arrive — long enough for a test to
    /// act while it is still in flight, short enough not to be mistaken for
    /// [`Self::hanging`].
    async fn delayed(delay: Duration, choice: &str, probabilities: Value) -> Self {
        Self::start(Answer::Delayed(
            delay,
            diagnosis_answer("kev-latest", choice, probabilities),
        ))
        .await
    }

    async fn malformed() -> Self {
        Self::start(Answer::Value(json!({"answers": {}}))).await
    }

    async fn hanging() -> Self {
        Self::start(Answer::Hang).await
    }

    /// Hangs on a diagnosis request, forever occupying the one worker,
    /// until a permission decision preempts it — at which point it answers
    /// the decision with `score`.
    async fn diagnosis_hangs_decision_answers(score: Value) -> Self {
        Self::start(Answer::DiagnosisHangsDecisionAnswers(score)).await
    }

    async fn start(answer: Answer) -> Self {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let state = ServerState {
            answer,
            requests: requests.clone(),
            worker: Arc::new(Semaphore::new(1)),
        };
        let app = axum::Router::new()
            .route("/v1/systemone", route_post(respond))
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
}

impl Drop for ModelServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// An answer to a `diagnosis` question, shaped as `kev.api.to_answers`
/// shapes a `choice` question's.
fn diagnosis_answer(model: &str, choice: &str, probabilities: Value) -> Value {
    json!({
        "model": model,
        "answers": {"diagnosis": {
            "type": "choice", "choice": choice, "confidence": 0.8,
            "probabilities": probabilities,
        }},
    })
}

async fn respond(
    State(state): State<ServerState>,
    axum::Json(request): axum::Json<Value>,
) -> axum::Json<Value> {
    // Acquired for the whole of this handler, including a hang: dropped
    // only when this future ends, whichever way — its own answer, or the
    // client aborting the connection this request came in on.
    let _worker = state.worker.clone().acquire_owned().await.unwrap();
    state.requests.lock().unwrap().push(request.clone());
    match state.answer {
        Answer::Value(answer) => axum::Json(answer),
        Answer::Hang => {
            std::future::pending::<()>().await;
            unreachable!()
        }
        Answer::Delayed(delay, answer) => {
            tokio::time::sleep(delay).await;
            axum::Json(answer)
        }
        Answer::DiagnosisHangsDecisionAnswers(score) => {
            if request["questions"].get("decision").is_some() {
                axum::Json(json!({"model": "kev-latest", "answers": {"decision": score}}))
            } else {
                std::future::pending::<()>().await;
                unreachable!()
            }
        }
    }
}

fn python() -> String {
    crate::common::shared_script("#!/bin/sh\necho 'Python 3.12.1'\n")
        .display()
        .to_string()
}

/// A daemon with the AI permission model enabled against `server`, and one
/// registered stub agent whose first (and only) prompt fails with `error`.
/// `ai_failure_diagnosis` turns the feature itself on or leaves it at its
/// off-by-default.
async fn world(
    error: Value,
    ai_failure_diagnosis: bool,
    server: &ModelServer,
) -> (Harness, AgentSession) {
    world_with(
        error,
        ai_failure_diagnosis,
        server,
        Timeouts::default(),
        true,
    )
    .await
}

/// [`world`], with the daemon's timeouts of its own — for a test about one
/// of them running out — and whether the AI permission model is turned on
/// at all: off, `ai_failure_diagnosis` finds it absent exactly as it would a
/// model still loading, since [`FailureDiagnosis::consider`] reads
/// [`AiPermissions::live`] rather than waiting on either.
async fn world_with(
    error: Value,
    ai_failure_diagnosis: bool,
    server: &ModelServer,
    timeouts: Timeouts,
    ai_permissions_enabled: bool,
) -> (Harness, AgentSession) {
    let root = tempfile::tempdir().unwrap();
    let agent_dir = root.path().join("agent");
    std::fs::create_dir_all(&agent_dir).unwrap();
    let mut agent_script = script();
    agent_script["prompts"] = json!([{"error": error}]);
    let agent = stub_acp_agent(&agent_dir, agent_script);

    let home = root.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let config = format!(
        "ai_failure_diagnosis = {ai_failure_diagnosis}\n[[acp_agents]]\nid = \"stub\"\ncommand = [{:?}]\n",
        agent.bin
    );
    std::fs::write(home.join("config.toml"), config).unwrap();

    let h = harness()
        .home(home)
        .discover_agents()
        .ai_permissions_endpoint(server.endpoint.clone())
        .python_bin(python())
        .timeouts(timeouts)
        .await;
    discovery_accepted(&h, &agent, "stub").await;

    if ai_permissions_enabled {
        let _: Value = h
            .json(
                put_json("/v1/permissions/ai", json!({"enabled": true})),
                StatusCode::OK,
            )
            .await;
    }

    h.git_repo("repo");
    let repo = h.repository(&h.at("repo")).await;
    let goal = h.goal_on(&repo, test_pin()).await;
    let task = h.task_on(&goal, &repo, "task", 0, test_pin()).await;
    h.activate(&goal).await;
    h.advance(&task, TaskStatus::InProgress).await;
    let author = h.launcher.spawn_author(&task.id).await.unwrap();
    eventually(TIMEOUT, "the failed author to end", || async {
        h.session_status(&author).await == SessionStatus::Exited
    })
    .await;
    (h, author)
}

/// Every event of `kind` a session reported, as parsed JSON payloads.
async fn events_of(h: &Harness, session_id: &str, kind: &str) -> Vec<Value> {
    h.store
        .list_events(EventFilter {
            session_id: Some(session_id.to_string()),
            limit: 50,
            ..Default::default()
        })
        .await
        .unwrap()
        .iter()
        .filter(|event| event.kind == kind)
        .map(|event| serde_json::from_str(&event.payload).unwrap())
        .collect()
}

fn quota_error() -> Value {
    json!({"code": -32603, "message": "You have exceeded your quota for this model"})
}

/// The whole path: a session's own `session.error` is recorded before the
/// diagnosis is even asked for, and the diagnosis that follows carries the
/// category Kev answered, its probabilities, the serving model, and the
/// id of the error it is about.
#[tokio::test]
async fn a_failed_sessions_diagnosis_is_correlated_with_its_own_error() {
    // Served from a model distinct from the request's own `kev-latest`, so
    // the assertion below proves the stored identity is read off the
    // response rather than the constant the daemon asked with.
    let server = ModelServer::answer_from(
        "kev-8b@deadbeef",
        "exhausted",
        json!({"exhausted": 0.81, "temporary": 0.05, "auth_config": 0.05, "task_error": 0.05, "insufficient": 0.04}),
    )
    .await;
    let (h, author) = world(quota_error(), true, &server).await;

    let errors = events_of(&h, &author.id, "session.error").await;
    assert_eq!(errors.len(), 1, "{errors:?}");
    let error_event_id = h
        .store
        .list_events(EventFilter {
            session_id: Some(author.id.clone()),
            limit: 50,
            ..Default::default()
        })
        .await
        .unwrap()
        .into_iter()
        .find(|event| event.kind == "session.error")
        .unwrap()
        .id;

    eventually(TIMEOUT, "the advisory diagnosis to arrive", || async {
        !events_of(&h, &author.id, "session.diagnosis")
            .await
            .is_empty()
    })
    .await;
    let diagnosis = events_of(&h, &author.id, "session.diagnosis")
        .await
        .remove(0);

    assert_eq!(diagnosis["error_event_id"], json!(error_event_id));
    assert_eq!(diagnosis["category"], json!("exhausted"));
    assert_eq!(diagnosis["model"], json!("kev-8b@deadbeef"));
    assert_eq!(diagnosis["probabilities"]["exhausted"], json!(0.81));

    // The original error is still there, untouched by the advisory.
    assert_eq!(errors[0]["error"]["message"], quota_error()["message"]);
}

/// Disabled is the default: even with a model ready to answer, nothing asks
/// it, and no advisory ever appears.
#[tokio::test]
async fn disabled_by_default_produces_no_diagnosis() {
    let server = ModelServer::answer("exhausted", json!({})).await;
    let (h, author) = world(quota_error(), false, &server).await;

    tokio::time::sleep(QUIET).await;
    assert!(
        events_of(&h, &author.id, "session.diagnosis")
            .await
            .is_empty()
    );
    assert!(server.requests.lock().unwrap().is_empty());
}

/// A malformed answer — no usable category at all — leaves no advisory
/// behind: the original error stands on its own, exactly as it would with
/// the feature off.
#[tokio::test]
async fn a_malformed_answer_produces_no_diagnosis() {
    let server = ModelServer::malformed().await;
    let (h, author) = world(quota_error(), true, &server).await;

    eventually(TIMEOUT, "the model to have been asked", || async {
        !server.requests.lock().unwrap().is_empty()
    })
    .await;
    tokio::time::sleep(QUIET).await;
    assert!(
        events_of(&h, &author.id, "session.diagnosis")
            .await
            .is_empty()
    );
}

/// Kev's own opinion never starts or prevents what the protocol fields and
/// the configured patterns already decided: an error that matches a
/// configured pattern is still exhausted on the record, whatever Kev makes
/// of it.
#[tokio::test]
async fn model_disagreement_never_moves_the_recorded_exhaustion() {
    let server = ModelServer::answer(
        "insufficient",
        json!({"exhausted": 0.05, "temporary": 0.05, "auth_config": 0.05, "task_error": 0.05, "insufficient": 0.8}),
    )
    .await;
    let (h, author) = world(quota_error(), true, &server).await;

    let errors = events_of(&h, &author.id, "session.error").await;
    assert_eq!(errors[0]["error"]["exhausted"], json!(true));
    assert_eq!(errors[0]["error"]["exhausted_reason"], json!("quota"));

    eventually(TIMEOUT, "the advisory diagnosis to arrive", || async {
        !events_of(&h, &author.id, "session.diagnosis")
            .await
            .is_empty()
    })
    .await;
    let diagnosis = events_of(&h, &author.id, "session.diagnosis")
        .await
        .remove(0);
    assert_eq!(diagnosis["category"], json!("insufficient"));
}

/// A diagnosis still in flight at shutdown is cancelled rather than left to
/// run on: `shutdown` returns once it is, however long the request itself
/// would otherwise have hung.
#[tokio::test]
async fn shutdown_cancels_a_diagnosis_in_flight() {
    let server = ModelServer::hanging().await;
    let (h, _author) = world(quota_error(), true, &server).await;

    eventually(TIMEOUT, "the hanging request to have started", || async {
        !server.requests.lock().unwrap().is_empty()
    })
    .await;

    tokio::time::timeout(TIMEOUT, h.failure_diagnosis.shutdown())
        .await
        .expect("shutdown cancels the hanging request rather than waiting on it");
}

/// The model absent — never turned on — leaves no advisory behind, the same
/// as a model still loading its weights: both read as `AiPermissions::live`
/// finding nothing, which this never waits out.
#[tokio::test]
async fn an_absent_model_produces_no_diagnosis_the_same_as_one_still_loading() {
    let server = ModelServer::answer("exhausted", json!({})).await;
    let (h, author) = world_with(
        quota_error(),
        true,
        &server,
        Timeouts::default(),
        /* ai_permissions_enabled */ false,
    )
    .await;

    tokio::time::sleep(QUIET).await;
    assert!(
        events_of(&h, &author.id, "session.diagnosis")
            .await
            .is_empty()
    );
    assert!(server.requests.lock().unwrap().is_empty());
}

/// At most one diagnosis request runs at a time: a second failed session,
/// while the first's diagnosis is still hanging, is skipped rather than
/// queued behind it.
#[tokio::test]
async fn a_second_failure_while_one_is_in_flight_is_skipped_not_queued() {
    let server = ModelServer::hanging().await;

    let root = tempfile::tempdir().unwrap();
    let first_dir = root.path().join("first");
    let second_dir = root.path().join("second");
    std::fs::create_dir_all(&first_dir).unwrap();
    std::fs::create_dir_all(&second_dir).unwrap();

    let mut first_script = script();
    first_script["prompts"] = json!([{"error": quota_error()}]);
    let first_agent = stub_acp_agent(&first_dir, first_script);

    let mut second_script = script();
    second_script["prompts"] = json!([{"error": quota_error()}]);
    let second_agent = stub_acp_agent(&second_dir, second_script);

    let home = root.path().join("home");
    std::fs::create_dir(&home).unwrap();
    std::fs::write(
        home.join("config.toml"),
        format!(
            "ai_failure_diagnosis = true\n[[acp_agents]]\nid = \"first\"\ncommand = [{:?}]\n[[acp_agents]]\nid = \"second\"\ncommand = [{:?}]\n",
            first_agent.bin, second_agent.bin,
        ),
    )
    .unwrap();

    let h = harness()
        .home(home)
        .discover_agents()
        .ai_permissions_endpoint(server.endpoint.clone())
        .python_bin(python())
        .await;
    discovery_accepted(&h, &first_agent, "first").await;
    discovery_accepted(&h, &second_agent, "second").await;

    let _: Value = h
        .json(
            put_json("/v1/permissions/ai", json!({"enabled": true})),
            StatusCode::OK,
        )
        .await;

    h.git_repo("repo");
    let repo = h.repository(&h.at("repo")).await;
    let goal = h
        .goal_on(
            &repo,
            AgentPin {
                model: "first:model".into(),
                effort: None,
            },
        )
        .await;
    let first_task = h
        .task_on(
            &goal,
            &repo,
            "first-task",
            0,
            AgentPin {
                model: "first:model".into(),
                effort: None,
            },
        )
        .await;
    let second_task = h
        .task_on(
            &goal,
            &repo,
            "second-task",
            0,
            AgentPin {
                model: "second:model".into(),
                effort: None,
            },
        )
        .await;
    h.activate(&goal).await;
    h.advance(&first_task, TaskStatus::InProgress).await;
    h.advance(&second_task, TaskStatus::InProgress).await;

    let first_author = h.launcher.spawn_author(&first_task.id).await.unwrap();
    eventually(TIMEOUT, "the first author to end", || async {
        h.session_status(&first_author).await == SessionStatus::Exited
    })
    .await;
    eventually(TIMEOUT, "the first request to reach the model", || async {
        !server.requests.lock().unwrap().is_empty()
    })
    .await;

    let second_author = h.launcher.spawn_author(&second_task.id).await.unwrap();
    eventually(TIMEOUT, "the second author to end", || async {
        h.session_status(&second_author).await == SessionStatus::Exited
    })
    .await;

    tokio::time::sleep(QUIET).await;
    assert_eq!(
        server.requests.lock().unwrap().len(),
        1,
        "the second failure was skipped rather than queued"
    );

    // The first diagnosis is still hanging on `server`. Cancel it rather
    // than leave it to its own several-second timeout: every test in this
    // binary shares one process, and a wait left running is a wait the
    // rest of the suite pays for.
    h.failure_diagnosis.shutdown().await;
}

/// A diagnosis that takes longer than the daemon's own bound times out, and
/// leaves no advisory behind — the same as one Kev never answers at all.
#[tokio::test]
async fn a_diagnosis_that_runs_past_its_bound_times_out_and_produces_none() {
    let server = ModelServer::hanging().await;
    let (h, author) = world_with(
        quota_error(),
        true,
        &server,
        Timeouts {
            failure_diagnosis_decision: Duration::from_millis(200),
            ..Timeouts::default()
        },
        true,
    )
    .await;

    eventually(TIMEOUT, "the request to reach the model", || async {
        !server.requests.lock().unwrap().is_empty()
    })
    .await;
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert!(
        events_of(&h, &author.id, "session.diagnosis")
            .await
            .is_empty()
    );
}

/// A diagnosis correlated with a launch a session has since moved past is
/// still stored, under that launch — not the one that replaced it — and
/// disturbs neither the session's current launch nor its status.
#[tokio::test]
async fn a_diagnosis_for_a_replaced_launch_keeps_that_launchs_id_and_disturbs_nothing_current() {
    let server = ModelServer::delayed(
        Duration::from_millis(300),
        "exhausted",
        json!({"exhausted": 0.9, "temporary": 0.03, "auth_config": 0.03, "task_error": 0.02, "insufficient": 0.02}),
    )
    .await;
    let (h, author) = world(quota_error(), true, &server).await;
    let original_launch_id = h
        .store
        .get_session(&author.id)
        .await
        .unwrap()
        .launch_id
        .expect("the failed launch is on the row");

    eventually(
        TIMEOUT,
        "the delayed request to reach the model",
        || async { !server.requests.lock().unwrap().is_empty() },
    )
    .await;
    // A replacement launch takes the row over before the diagnosis answers.
    h.store
        .set_session_launch(&author.id, "launch-replacement")
        .await
        .unwrap();

    eventually(TIMEOUT, "the advisory diagnosis to arrive", || async {
        !events_of(&h, &author.id, "session.diagnosis")
            .await
            .is_empty()
    })
    .await;
    let diagnosis = events_of(&h, &author.id, "session.diagnosis")
        .await
        .remove(0);
    assert_eq!(diagnosis["launch_id"], json!(original_launch_id));

    let current = h.store.get_session(&author.id).await.unwrap();
    assert_eq!(current.launch_id.as_deref(), Some("launch-replacement"));
    assert_eq!(current.status(), SessionStatus::Exited);
}

/// The stored event still identifies the correct error after a restart: the
/// ids are the database's own, so a daemon that comes back up reads exactly
/// the same correlation a running one would.
#[tokio::test]
async fn the_stored_correlation_survives_a_restart() {
    let server = ModelServer::answer(
        "exhausted",
        json!({"exhausted": 0.75, "temporary": 0.1, "auth_config": 0.05, "task_error": 0.05, "insufficient": 0.05}),
    )
    .await;
    let (h, author) = world(quota_error(), true, &server).await;

    eventually(TIMEOUT, "the advisory diagnosis to arrive", || async {
        !events_of(&h, &author.id, "session.diagnosis")
            .await
            .is_empty()
    })
    .await;

    // A second connection to the same database file, as a daemon that
    // restarted would open: nothing here is read through `h.store`.
    let db_path = h.dir.path().join("test.db");
    let reopened = Store::open(&db_path).await.unwrap();
    let events = reopened
        .list_events(EventFilter {
            session_id: Some(author.id.clone()),
            limit: 50,
            ..Default::default()
        })
        .await
        .unwrap();
    let error_id = events
        .iter()
        .find(|event| event.kind == "session.error")
        .unwrap()
        .id
        .clone();
    let diagnosis: Value = events
        .iter()
        .find(|event| event.kind == "session.diagnosis")
        .map(|event| serde_json::from_str(&event.payload).unwrap())
        .unwrap();
    assert_eq!(diagnosis["error_event_id"], json!(error_id));
}

/// A permission decision needs the same model a diagnosis is hanging on:
/// it aborts that diagnosis first, so it is decided on its own and never
/// waits behind it — unlike the diagnosis, which is free to be skipped or
/// to wait its turn, since nothing but an advisory depends on it.
#[tokio::test]
async fn a_permission_decision_preempts_a_hanging_diagnosis_and_is_not_delayed_by_it() {
    let server = ModelServer::diagnosis_hangs_decision_answers(json!({
        "type": "score", "score": 0.0, "probabilities": {"0": 1.0, "1": 0.0, "2": 0.0},
    }))
    .await;

    let root = tempfile::tempdir().unwrap();
    let failing_dir = root.path().join("failing");
    let asking_dir = root.path().join("asking");
    std::fs::create_dir_all(&failing_dir).unwrap();
    std::fs::create_dir_all(&asking_dir).unwrap();

    let mut failing_script = script();
    failing_script["prompts"] = json!([{"error": quota_error()}]);
    let failing_agent = stub_acp_agent(&failing_dir, failing_script);

    let mut asking_script = script();
    asking_script["prompts"] = json!([{
        "permission": {
            "toolCall": {"toolCallId": "call-1", "name": "Bash", "title": "Bash",
                         "kind": "execute", "rawInput": {"command": "git status"}},
            "options": [
                {"optionId": "no", "name": "Reject", "kind": "reject_once"},
                {"optionId": "yes", "name": "Allow", "kind": "allow_once"},
            ],
        },
        "updates": [],
        "stop_reason": "end_turn",
    }]);
    let asking_agent = stub_acp_agent(&asking_dir, asking_script);

    let home = root.path().join("home");
    std::fs::create_dir(&home).unwrap();
    std::fs::write(
        home.join("config.toml"),
        format!(
            "ai_failure_diagnosis = true\n[[acp_agents]]\nid = \"failing\"\ncommand = [{:?}]\n[[acp_agents]]\nid = \"asking\"\ncommand = [{:?}]\n",
            failing_agent.bin, asking_agent.bin,
        ),
    )
    .unwrap();

    let h = harness()
        .home(home)
        .discover_agents()
        .ai_permissions_endpoint(server.endpoint.clone())
        .python_bin(python())
        .timeouts(Timeouts {
            ai_permissions_decision: Duration::from_millis(600),
            failure_diagnosis_decision: Duration::from_secs(30),
            ..Timeouts::default()
        })
        .await;
    discovery_accepted(&h, &failing_agent, "failing").await;
    discovery_accepted(&h, &asking_agent, "asking").await;

    let _: Value = h
        .json(
            put_json("/v1/permissions/ai", json!({"enabled": true})),
            StatusCode::OK,
        )
        .await;

    h.git_repo("repo");
    let repo = h.repository(&h.at("repo")).await;
    h.set_permission_mode(&repo, PermissionMode::Ai).await;

    let goal = h
        .goal_on(
            &repo,
            AgentPin {
                model: "failing:model".into(),
                effort: None,
            },
        )
        .await;
    let failing_task = h
        .task_on(
            &goal,
            &repo,
            "failing-task",
            0,
            AgentPin {
                model: "failing:model".into(),
                effort: None,
            },
        )
        .await;
    let asking_task = h
        .task_on(
            &goal,
            &repo,
            "asking-task",
            0,
            AgentPin {
                model: "asking:model".into(),
                effort: None,
            },
        )
        .await;
    h.activate(&goal).await;
    h.advance(&failing_task, TaskStatus::InProgress).await;
    h.advance(&asking_task, TaskStatus::InProgress).await;

    let failing_author = h.launcher.spawn_author(&failing_task.id).await.unwrap();
    eventually(TIMEOUT, "the failed author to end", || async {
        h.session_status(&failing_author).await == SessionStatus::Exited
    })
    .await;
    eventually(
        TIMEOUT,
        "the hanging diagnosis request to reach the model",
        || async { !server.requests.lock().unwrap().is_empty() },
    )
    .await;

    let asking_author = h.launcher.spawn_author(&asking_task.id).await.unwrap();
    // Without preemption this decision waits behind the hanging diagnosis
    // on the one worker and never gets its confident allow; with it, the
    // turn reaches `stop` well within this bound, with no console asked.
    eventually(
        TIMEOUT,
        "the ai-decided turn to finish without a console",
        || async { h.session_status(&asking_author).await == SessionStatus::Idle },
    )
    .await;
}
