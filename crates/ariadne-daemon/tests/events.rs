//! Integration tests for the domain-event bus, the SSE stream and CORS.
//!
//! No external binaries needed: the scheduler test only asserts on the
//! transition it makes before it reaches out to git/tmux.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use http_body_util::BodyExt;
use tokio::sync::broadcast::Receiver;
use tower::ServiceExt;

use ariadne_api::stream::{DeletedDto, DomainEvent};
use ariadne_core::{
    Actor, AgentKind, AttentionReason, GoalStatus, ReviewVerdict, Role, SessionStatus, TaskStatus,
};
use ariadne_daemon::bus::{BusEvent, EventBus};
use ariadne_daemon::config::Config;
use ariadne_daemon::gitwt::GitManager;
use ariadne_daemon::http::{self, AppState};
use ariadne_daemon::launcher::Launcher;
use ariadne_daemon::logbuf::LogBuffer;
use ariadne_daemon::scheduler::{self, SchedEvent};
use ariadne_daemon::tmux::TmuxManager;
use ariadne_store::{Goal, NewGoal, NewProfile, NewRepository, NewTask, Profile, Store, Task};

/// How long a test waits for an event before giving up.
const TIMEOUT: Duration = Duration::from_secs(5);

struct Harness {
    store: Store,
    bus: EventBus,
    launcher: Arc<Launcher>,
    state: AppState,
    router: Router,
    dir: tempfile::TempDir,
}

async fn harness() -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("test.db")).await.unwrap();
    // Installed before anything writes, exactly as the daemon does at startup.
    let bus = ariadne_daemon::bus::start(store.clone());
    let cfg = Arc::new(Config::load(Some(dir.path().join("home"))).unwrap());
    let launcher = Arc::new(Launcher {
        cfg,
        store: store.clone(),
        tmux: TmuxManager::default(),
        git: GitManager,
    });
    let state = AppState {
        store: store.clone(),
        started_at: Instant::now(),
        launcher: launcher.clone(),
        sched_tx: None,
        events: bus.clone(),
        logs: LogBuffer::new(),
    };
    Harness {
        router: http::router(state.clone()),
        store,
        bus,
        launcher,
        state,
        dir,
    }
}

impl Harness {
    async fn profile(&self, name: &str, role: Role) -> Profile {
        self.store
            .create_profile(NewProfile {
                name: name.into(),
                role,
                agent_kind: Some(AgentKind::ClaudeCode),
                model: None,
                system_prompt: format!("You are {name}."),
                prompts: vec![],
            })
            .await
            .unwrap()
    }

    /// A live session of `role`, as the launcher would have created it.
    async fn session(
        &self,
        goal: &Goal,
        task: Option<&Task>,
        role: Role,
        profile_id: &str,
    ) -> ariadne_store::AgentSession {
        self.store
            .create_session(ariadne_store::NewSession {
                goal_id: goal.id.clone(),
                task_id: task.map(|t| t.id.clone()),
                role,
                profile_id: profile_id.to_string(),
                agent_kind: AgentKind::ClaudeCode,
                model: None,
                tmux_session: format!("ariadne-test-{}", role.as_str()),
                worktree_path: Some(PathBuf::from("/tmp/wt").display().to_string()),
                review_round: task.map(|t| t.review_round),
            })
            .await
            .unwrap()
    }

    /// Hand a task to its engineer, which is the state a live engineer
    /// session is actually in.
    ///
    /// Attention belongs to an agent somebody is waiting on, and nobody is
    /// waiting on the engineer of a task that has not been started: the
    /// ingestion withholds the flag there, so the tests that assert on one
    /// start the work first.
    async fn hand_to_engineer(&self, task: &Task) {
        for status in [TaskStatus::Ready, TaskStatus::InProgress] {
            self.store
                .transition_task(&task.id, status, Actor::Daemon, None, None)
                .await
                .unwrap();
        }
    }

    /// An active goal with one pending, dependency-free task.
    ///
    /// Returns only once the bus has published every seeding change, so a
    /// stream opened afterwards sees nothing but what the test itself does.
    async fn active_goal_with_task(&self) -> (Goal, Task) {
        let mut rx = self.bus.subscribe();
        let planner = self.profile("planner", Role::Planner).await;
        let engineer = self.profile("engineer", Role::Engineer).await;
        let reviewer = self.profile("reviewer", Role::Reviewer).await;
        // Not a git repo: the scheduler fails right after the transition we
        // assert on, which is all this needs.
        let repo = self
            .store
            .create_repository(NewRepository {
                path: self.dir.path().join("repo").display().to_string(),
                base_branch: "main".into(),
                description: None,
            })
            .await
            .unwrap();
        let goal = self
            .store
            .create_goal(NewGoal {
                title: "Ship the UI".into(),
                description: "desc".into(),
                planner_profile_id: planner.id,
                max_tasks: None,
                required_approvals: 1,
                repository_ids: vec![repo.id.clone()],
            })
            .await
            .unwrap();
        let task = self
            .store
            .create_task(NewTask {
                goal_id: goal.id.clone(),
                repo_id: repo.id,
                title: "task".into(),
                description: "do things".into(),
                engineer_profile_id: engineer.id,
                reviewer_profile_ids: vec![reviewer.id],
                depends_on: vec![],
            })
            .await
            .unwrap();
        let goal = self
            .store
            .set_goal_status(&goal.id, GoalStatus::Active)
            .await
            .unwrap();
        // The pump preserves commit order: seeing the last seeding change
        // means all the earlier ones are out too.
        next_event(
            &mut rx,
            |e| matches!(&e.event, DomainEvent::GoalUpdated(g) if g.status == GoalStatus::Active),
        )
        .await;
        (goal, task)
    }
}

/// Wait for the first event matching `pred`, skipping unrelated ones.
async fn next_event(rx: &mut Receiver<BusEvent>, pred: impl Fn(&BusEvent) -> bool) -> BusEvent {
    tokio::time::timeout(TIMEOUT, async {
        loop {
            let event = rx.recv().await.expect("event bus closed");
            if pred(&event) {
                return event;
            }
        }
    })
    .await
    .expect("timed out waiting for a matching domain event")
}

fn get(uri: &str) -> Request<Body> {
    Request::builder().uri(uri).body(Body::empty()).unwrap()
}

fn post_json(uri: &str, body: serde_json::Value) -> Request<Body> {
    Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

/// Read from an SSE body until one complete message (`\n\n`-terminated) is in.
async fn next_sse_message(body: &mut Body) -> String {
    tokio::time::timeout(TIMEOUT, async {
        let mut buf = String::new();
        while let Some(frame) = body.frame().await {
            let frame = frame.expect("sse body error");
            if let Some(chunk) = frame.data_ref() {
                buf.push_str(&String::from_utf8_lossy(chunk));
                if buf.contains("\n\n") {
                    return buf;
                }
            }
        }
        panic!("sse stream ended before a complete message: {buf:?}");
    })
    .await
    .expect("timed out waiting for an sse message")
}

#[tokio::test]
async fn http_mutation_emits_a_fat_event() {
    let h = harness().await;
    let mut rx = h.bus.subscribe();

    let response = h
        .router
        .clone()
        .oneshot(post_json(
            "/v1/profiles",
            serde_json::json!({
                "name": "rust-engineer",
                "role": "engineer",
                "agent_kind": "claude_code",
                "system_prompt": "You write Rust.",
            }),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    let event = next_event(&mut rx, |e| e.event.kind() == "profile_created").await;
    let DomainEvent::ProfileCreated(profile) = event.event else {
        unreachable!("matched on kind above");
    };
    // Fat payload: the whole DTO, not just an id to refetch.
    assert_eq!(profile.name, "rust-engineer");
    assert_eq!(profile.role, Role::Engineer);
    assert_eq!(profile.system_prompt, "You write Rust.");
}

#[tokio::test]
async fn http_transition_emits_task_updated_with_its_transition() {
    let h = harness().await;
    let (_goal, task) = h.active_goal_with_task().await;
    let mut rx = h.bus.subscribe();

    let response = h
        .router
        .clone()
        .oneshot(post_json(
            &format!("/v1/tasks/{}/cancel", task.id),
            serde_json::json!({}),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let event = next_event(&mut rx, |e| e.event.kind() == "task_updated").await;
    assert_eq!(event.goal_id.as_deref(), Some(task.goal_id.as_str()));
    assert_eq!(event.task_id.as_deref(), Some(task.id.as_str()));
    let DomainEvent::TaskUpdated(updated) = event.event else {
        unreachable!("matched on kind above");
    };
    assert_eq!(updated.task.status, TaskStatus::Cancelled);
    let transition = updated
        .transition
        .expect("status change carries its audit row");
    assert_eq!(transition.from_status, "pending");
    assert_eq!(transition.to_status, "cancelled");
    assert_eq!(transition.actor, "user");
}

#[tokio::test]
async fn scheduler_transition_emits_task_updated_without_http() {
    let h = harness().await;
    let (_goal, task) = h.active_goal_with_task().await;
    let mut rx = h.bus.subscribe();

    // No HTTP involved: the scheduler reconciles the task on its own and
    // finds its (empty) dependency set merged.
    // No sleep inhibition: a test has no business touching power management.
    let sched = scheduler::start(h.store.clone(), h.launcher.clone(), false);
    sched
        .send(SchedEvent::TaskChanged(task.id.clone()))
        .unwrap();

    let event = next_event(
        &mut rx,
        |e| matches!(&e.event, DomainEvent::TaskUpdated(u) if u.task.status == TaskStatus::Ready),
    )
    .await;
    let DomainEvent::TaskUpdated(updated) = event.event else {
        unreachable!("matched on the variant above");
    };
    assert_eq!(updated.task.id, task.id);
    let transition = updated
        .transition
        .expect("status change carries its audit row");
    assert_eq!(transition.to_status, "ready");
    assert_eq!(transition.actor, "daemon");
}

#[tokio::test]
async fn sse_stream_frames_events_and_honours_the_goal_filter() {
    let h = harness().await;
    let (goal, task) = h.active_goal_with_task().await;

    let response = h
        .router
        .clone()
        .oneshot(get(&format!("/v1/events/stream?goal={}", goal.id)))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some("text/event-stream")
    );
    let mut body = response.into_body();

    // Out of scope for this goal: must not reach the stream.
    h.profile("unrelated", Role::Engineer).await;
    // In scope: a task transition.
    h.store
        .transition_task(
            &task.id,
            TaskStatus::Ready,
            ariadne_core::Actor::Daemon,
            None,
            None,
        )
        .await
        .unwrap();

    let message = next_sse_message(&mut body).await;
    let mut lines = message.trim_end().lines();
    let id = lines.next().unwrap();
    assert!(id.starts_with("id: "), "first line is the event id: {id:?}");
    assert!(
        ariadne_core::id::is_valid(id.trim_start_matches("id: ")),
        "event id is a ULID: {id:?}"
    );
    assert_eq!(lines.next(), Some("event: task_updated"));
    let data = lines.next().unwrap().strip_prefix("data: ").unwrap();
    let payload: serde_json::Value = serde_json::from_str(data).unwrap();
    assert_eq!(payload["task"]["id"], task.id);
    assert_eq!(payload["task"]["status"], "ready");
    assert_eq!(payload["transition"]["to_status"], "ready");
}

/// A client that falls behind must be told, not left silently stale: it gets
/// a final `resync` event and the connection is closed, which is what drives
/// an `EventSource` to reconnect and refetch.
#[tokio::test]
async fn sse_stream_signals_resync_and_closes_when_a_client_lags() {
    let h = harness().await;
    // A tiny bus, so a handful of unread events overflows this subscriber.
    let bus = EventBus::with_capacity(2);
    let router = http::router(AppState {
        events: bus.clone(),
        ..h.state.clone()
    });

    let response = router.oneshot(get("/v1/events/stream")).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let mut body = response.into_body();

    // Nothing reads the body yet, so these pile up in the subscriber's buffer.
    for i in 0..8 {
        bus.publish(BusEvent {
            event: DomainEvent::ProfileDeleted(DeletedDto {
                id: format!("profile-{i}"),
            }),
            goal_id: None,
            task_id: None,
        });
    }

    let message = next_sse_message(&mut body).await;
    assert!(
        message.contains("event: resync"),
        "a lagged client is told to resync, got: {message:?}"
    );
    let data = message
        .lines()
        .find_map(|l| l.strip_prefix("data: "))
        .expect("resync carries a payload");
    let payload: serde_json::Value = serde_json::from_str(data).unwrap();
    assert!(
        payload["missed"].as_u64().is_some_and(|n| n > 0),
        "resync reports how many events were lost: {payload}"
    );

    // ...and the stream ends, rather than carrying on with a hole in it.
    let end = tokio::time::timeout(TIMEOUT, body.frame())
        .await
        .expect("stream must close after resync, not hang");
    assert!(end.is_none(), "no frames follow the resync event");
}

#[tokio::test]
async fn sse_stream_task_filter_excludes_other_tasks() {
    let h = harness().await;
    let (goal, task) = h.active_goal_with_task().await;

    let response = h
        .router
        .clone()
        .oneshot(get(&format!("/v1/events/stream?task={}", task.id)))
        .await
        .unwrap();
    let mut body = response.into_body();

    // A goal-level event carries no task id, so the task filter drops it.
    h.store
        .set_goal_status(&goal.id, GoalStatus::Cancelled)
        .await
        .unwrap();
    h.store.set_task_stalled(&task.id, true).await.unwrap();

    let message = next_sse_message(&mut body).await;
    assert!(
        message.contains("event: task_updated"),
        "goal event must be filtered out, got: {message:?}"
    );
    let data = message
        .lines()
        .find_map(|l| l.strip_prefix("data: "))
        .unwrap();
    let payload: serde_json::Value = serde_json::from_str(data).unwrap();
    assert_eq!(payload["task"]["stalled"], true);
}

#[tokio::test]
async fn cors_allows_preflight_and_cross_origin_calls() {
    let h = harness().await;

    let preflight = Request::builder()
        .method(Method::OPTIONS)
        .uri("/v1/profiles")
        .header(header::ORIGIN, "tauri://localhost")
        .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
        .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "content-type")
        .body(Body::empty())
        .unwrap();
    let response = h.router.clone().oneshot(preflight).await.unwrap();
    assert!(
        response.status().is_success(),
        "preflight rejected: {}",
        response.status()
    );
    let headers = response.headers().clone();
    assert_eq!(
        headers.get(header::ACCESS_CONTROL_ALLOW_ORIGIN).unwrap(),
        "*"
    );
    assert!(headers.contains_key(header::ACCESS_CONTROL_ALLOW_METHODS));
    assert!(headers.contains_key(header::ACCESS_CONTROL_ALLOW_HEADERS));

    // The actual cross-origin request, from a dev-server origin this time.
    let mut request = get("/v1/events/stream");
    request
        .headers_mut()
        .insert(header::ORIGIN, "http://localhost:1420".parse().unwrap());
    let response = h.router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .unwrap(),
        "*"
    );
}

/// The launcher writes session rows through the same store, so its spawns
/// reach the bus without knowing about it.
#[tokio::test]
async fn launcher_session_writes_emit_session_events() {
    let h = harness().await;
    let (goal, task) = h.active_goal_with_task().await;
    let mut rx = h.bus.subscribe();

    let session = h
        .store
        .create_session(ariadne_store::NewSession {
            goal_id: goal.id.clone(),
            task_id: Some(task.id.clone()),
            role: Role::Engineer,
            profile_id: task.engineer_profile_id.clone(),
            agent_kind: AgentKind::ClaudeCode,
            model: None,
            tmux_session: "ariadne-test".into(),
            worktree_path: Some(PathBuf::from("/tmp/wt").display().to_string()),
            review_round: None,
        })
        .await
        .unwrap();

    let event = next_event(&mut rx, |e| e.event.kind() == "session_created").await;
    assert_eq!(event.goal_id.as_deref(), Some(goal.id.as_str()));
    assert_eq!(event.task_id.as_deref(), Some(task.id.as_str()));

    h.store
        .set_session_status(&session.id, ariadne_core::SessionStatus::Exited)
        .await
        .unwrap();
    let event = next_event(&mut rx, |e| e.event.kind() == "session_updated").await;
    let DomainEvent::SessionUpdated(dto) = event.event else {
        unreachable!("matched on kind above");
    };
    assert_eq!(dto.id, session.id);
    assert_eq!(dto.status, ariadne_core::SessionStatus::Exited);
    assert!(dto.ended_at.is_some());
}

/// Attention rides the same ingestion path as liveness: an agent that reports
/// an error needs the user, and one that goes back to work does not — while
/// going idle, which is exactly when a prompt may be waiting, leaves the flag
/// alone. Every change reaches the bus as a `session_updated`.
#[tokio::test]
async fn ingested_events_raise_and_clear_session_attention() {
    let h = harness().await;
    let (goal, task) = h.active_goal_with_task().await;
    h.hand_to_engineer(&task).await;
    let session = h
        .store
        .create_session(ariadne_store::NewSession {
            goal_id: goal.id.clone(),
            task_id: Some(task.id.clone()),
            role: Role::Engineer,
            profile_id: task.engineer_profile_id.clone(),
            agent_kind: AgentKind::Opencode,
            model: None,
            tmux_session: "ariadne-test".into(),
            worktree_path: Some(PathBuf::from("/tmp/wt").display().to_string()),
            review_round: None,
        })
        .await
        .unwrap();
    let mut rx = h.bus.subscribe();

    let ingest = |kind: &str| {
        post_json(
            "/internal/agent-events",
            serde_json::json!({
                "session_id": session.id,
                "agent_kind": "opencode",
                "kind": kind,
                "payload": {},
            }),
        )
    };
    let post = async |kind: &str| {
        let response = h.router.clone().oneshot(ingest(kind)).await.unwrap();
        assert_eq!(response.status(), StatusCode::ACCEPTED, "{kind}");
    };

    // A failed turn: attention is raised, the lifecycle status is untouched.
    post("session.error").await;
    let flagged = h.store.get_session(&session.id).await.unwrap();
    assert_eq!(
        flagged.attention_reason(),
        Some(AttentionReason::AgentError)
    );
    assert!(flagged.attention_since.is_some());
    assert_eq!(flagged.status(), SessionStatus::Starting);

    // ...and the flag is on the bus, not just in the database.
    let event = next_event(&mut rx, |e| {
        matches!(&e.event, DomainEvent::SessionUpdated(s)
            if s.attention_reason == Some(AttentionReason::AgentError))
    })
    .await;
    assert_eq!(event.task_id.as_deref(), Some(task.id.as_str()));

    // Going idle is when a permission prompt or a question is waiting: it
    // must not clear anything.
    post("session.idle").await;
    assert_eq!(
        h.store
            .get_session(&session.id)
            .await
            .unwrap()
            .attention_reason(),
        Some(AttentionReason::AgentError)
    );

    // Back to work: the agent needs nobody now.
    post("tool.execute.before").await;
    let cleared = h.store.get_session(&session.id).await.unwrap();
    assert_eq!(cleared.attention_reason(), None);
    assert_eq!(cleared.attention_since, None);
    assert_eq!(cleared.status(), SessionStatus::Running);
    next_event(&mut rx, |e| {
        matches!(&e.event, DomainEvent::SessionUpdated(s)
            if s.id == session.id && s.attention_reason.is_none())
    })
    .await;

    // A session that ended needing attention keeps the reason: a stray event
    // arriving afterwards resurrects neither its status nor its flag.
    h.store
        .set_session_status(&session.id, SessionStatus::Exited)
        .await
        .unwrap();
    h.store
        .set_session_attention(&session.id, AttentionReason::Disconnected)
        .await
        .unwrap();
    post("tool.execute.before").await;
    let ended = h.store.get_session(&session.id).await.unwrap();
    assert_eq!(
        ended.attention_reason(),
        Some(AttentionReason::Disconnected)
    );
    assert_eq!(ended.status(), SessionStatus::Exited);
}

/// OpenCode's approval dialog reaches Ariadne as `permission.asked` on the
/// plugin's event stream — the payloads below are what opencode 1.18.15
/// actually sent during a run with `permission.bash = "ask"`. The session
/// looks no different while the dialog is up, so this is the only signal.
#[tokio::test]
async fn an_opencode_permission_ask_flags_the_session_as_blocked() {
    let h = harness().await;
    let (goal, task) = h.active_goal_with_task().await;
    h.hand_to_engineer(&task).await;
    let session = h
        .store
        .create_session(ariadne_store::NewSession {
            goal_id: goal.id.clone(),
            task_id: Some(task.id.clone()),
            role: Role::Engineer,
            profile_id: task.engineer_profile_id.clone(),
            agent_kind: AgentKind::Opencode,
            model: None,
            tmux_session: "ariadne-test".into(),
            worktree_path: Some(PathBuf::from("/tmp/wt").display().to_string()),
            review_round: None,
        })
        .await
        .unwrap();

    let post = async |kind: &str, payload: serde_json::Value| {
        let request = post_json(
            "/internal/agent-events",
            serde_json::json!({
                "session_id": session.id,
                "agent_kind": "opencode",
                "kind": kind,
                "payload": payload,
            }),
        );
        let response = h.router.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::ACCEPTED, "{kind}");
    };

    // Working, so the internal id is already known and the flag is down.
    post(
        "session.created",
        serde_json::json!({
            "sessionID": "ses_fe5cb9641ffeQPvwaIKtSsLAqP",
            "info": {"id": "ses_fe5cb9641ffeQPvwaIKtSsLAqP", "version": "1.18.15"},
        }),
    )
    .await;
    let running = h.store.get_session(&session.id).await.unwrap();
    assert_eq!(running.status(), SessionStatus::Running);
    assert_eq!(
        running.internal_session_id.as_deref(),
        Some("ses_fe5cb9641ffeQPvwaIKtSsLAqP")
    );

    // The dialog goes up: flagged, and the status is left where it was.
    post(
        "permission.asked",
        serde_json::json!({
            "id": "per_01a3575b4001aOsIrUVWB44A4e",
            "sessionID": "ses_fe5cb9641ffeQPvwaIKtSsLAqP",
            "permission": "bash",
            "patterns": ["echo hello-from-bash"],
            "metadata": {"command": "echo hello-from-bash"},
            "always": ["echo *"],
            "tool": {"messageID": "msg_01a346a620011crwCE4oJgZDqr", "callID": "call_vt3e3umm"},
        }),
    )
    .await;
    let flagged = h.store.get_session(&session.id).await.unwrap();
    assert_eq!(
        flagged.attention_reason(),
        Some(AttentionReason::WaitingPermission)
    );
    assert_eq!(flagged.status(), SessionStatus::Running);
    // The permission's own id must not be mistaken for the session's.
    assert_eq!(
        flagged.internal_session_id.as_deref(),
        Some("ses_fe5cb9641ffeQPvwaIKtSsLAqP")
    );

    // `session.updated` keeps firing while the dialog waits: it must not
    // look like the agent went back to work.
    post(
        "session.updated",
        serde_json::json!({"info": {"id": "ses_fe5cb9641ffeQPvwaIKtSsLAqP"}}),
    )
    .await;
    assert_eq!(
        h.store
            .get_session(&session.id)
            .await
            .unwrap()
            .attention_reason(),
        Some(AttentionReason::WaitingPermission)
    );

    // The user answered — rejected, here, which still hands control back.
    post(
        "permission.replied",
        serde_json::json!({
            "sessionID": "ses_fe5cb9641ffeQPvwaIKtSsLAqP",
            "requestID": "per_01a3575b4001aOsIrUVWB44A4e",
            "reply": "reject",
        }),
    )
    .await;
    let cleared = h.store.get_session(&session.id).await.unwrap();
    assert_eq!(cleared.attention_reason(), None);
    assert_eq!(cleared.status(), SessionStatus::Running);

    // A question is the other family: a wait for an answer, cleared by one.
    post(
        "question.asked",
        serde_json::json!({
            "id": "ask_01a3575b4001aOsIrUVWB44A4f",
            "sessionID": "ses_fe5cb9641ffeQPvwaIKtSsLAqP",
        }),
    )
    .await;
    assert_eq!(
        h.store
            .get_session(&session.id)
            .await
            .unwrap()
            .attention_reason(),
        Some(AttentionReason::WaitingInput)
    );
    post(
        "question.replied",
        serde_json::json!({
            "sessionID": "ses_fe5cb9641ffeQPvwaIKtSsLAqP",
            "requestID": "ask_01a3575b4001aOsIrUVWB44A4f",
            "answers": [],
        }),
    )
    .await;
    assert_eq!(
        h.store
            .get_session(&session.id)
            .await
            .unwrap()
            .attention_reason(),
        None
    );

    // An error while a dialog is up must not be traded for the wait, and
    // neither may clear the other: the flag stands until real work resumes.
    post("permission.asked", serde_json::json!({"id": "per_2"})).await;
    post("session.error", serde_json::json!({})).await;
    let errored = h.store.get_session(&session.id).await.unwrap();
    assert_eq!(
        errored.attention_reason(),
        Some(AttentionReason::AgentError)
    );
    assert_eq!(errored.status(), SessionStatus::Running);
}

/// Claude Code's `Notification` hook is the only signal that an
/// idle-looking session is actually blocked on the user: it must raise the
/// right attention reason, survive the `touch_session` of its own ingestion,
/// and be cleared only once the agent does real work again.
#[tokio::test]
async fn a_claude_notification_flags_the_session_as_blocked() {
    let h = harness().await;
    let (goal, task) = h.active_goal_with_task().await;
    h.hand_to_engineer(&task).await;
    let session = h
        .store
        .create_session(ariadne_store::NewSession {
            goal_id: goal.id.clone(),
            task_id: Some(task.id.clone()),
            role: Role::Engineer,
            profile_id: task.engineer_profile_id.clone(),
            agent_kind: AgentKind::ClaudeCode,
            model: None,
            tmux_session: "ariadne-test".into(),
            worktree_path: Some(PathBuf::from("/tmp/wt").display().to_string()),
            review_round: None,
        })
        .await
        .unwrap();

    let post = async |kind: &str, payload: serde_json::Value| {
        let request = post_json(
            "/internal/agent-events",
            serde_json::json!({
                "session_id": session.id,
                "agent_kind": "claude_code",
                "kind": kind,
                "payload": payload,
            }),
        );
        let response = h.router.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::ACCEPTED, "{kind}");
    };
    let notification = |notification_type: &str, message: &str| {
        serde_json::json!({
            "session_id": "5f3b1c8e-1234-4a2b-9d0e-0123456789ab",
            "cwd": "/tmp/wt",
            "hook_event_name": "Notification",
            "message": message,
            "notification_type": notification_type,
        })
    };

    // Blocked on a permission dialog: flagged, and still not "running" —
    // the ingestion's own touch_session must not undo the flag.
    post(
        "notification",
        notification(
            "permission_prompt",
            "Claude needs your permission to use Bash",
        ),
    )
    .await;
    let flagged = h.store.get_session(&session.id).await.unwrap();
    assert_eq!(
        flagged.attention_reason(),
        Some(AttentionReason::WaitingPermission)
    );
    assert_eq!(flagged.status(), SessionStatus::Starting);
    assert!(flagged.last_activity_at.is_some());

    // The user answered and the agent runs a tool: attention drops.
    post("pre_tool_use", serde_json::json!({"tool_name": "Bash"})).await;
    let cleared = h.store.get_session(&session.id).await.unwrap();
    assert_eq!(cleared.attention_reason(), None);
    assert_eq!(cleared.status(), SessionStatus::Running);

    // Idle at the prompt: waiting for an answer, not for permission.
    post(
        "notification",
        notification("idle_prompt", "Claude is waiting for your input"),
    )
    .await;
    assert_eq!(
        h.store
            .get_session(&session.id)
            .await
            .unwrap()
            .attention_reason(),
        Some(AttentionReason::WaitingInput)
    );

    // Submitting a prompt is the other half of the clearing rule.
    post("user_prompt_submit", serde_json::json!({"prompt": "go on"})).await;
    assert_eq!(
        h.store
            .get_session(&session.id)
            .await
            .unwrap()
            .attention_reason(),
        None
    );

    // An unrecognized notification is recorded but changes nothing.
    post(
        "notification",
        notification("auth_success", "Logged in as me@example.com"),
    )
    .await;
    let untouched = h.store.get_session(&session.id).await.unwrap();
    assert_eq!(untouched.attention_reason(), None);
    assert_eq!(untouched.status(), SessionStatus::Running);
    let events = h
        .store
        .list_events(ariadne_store::EventFilter {
            session_id: Some(session.id.clone()),
            task_id: None,
            limit: 50,
            after: None,
        })
        .await
        .unwrap();
    assert_eq!(
        events.iter().filter(|e| e.kind == "notification").count(),
        3
    );
}

/// A Claude `Notification` payload for a permission prompt: what a session
/// blocked on a dialog reports, whoever it belongs to.
fn permission_prompt() -> serde_json::Value {
    serde_json::json!({
        "session_id": "5f3b1c8e-1234-4a2b-9d0e-0123456789ab",
        "cwd": "/tmp/wt",
        "hook_event_name": "Notification",
        "message": "Claude needs your permission to use Bash",
        "notification_type": "permission_prompt",
    })
}

/// Attention says a human must act, so it is only raised on an agent somebody
/// is still waiting on. A reviewer that has cast its verdict is finished, and
/// a dialog it puts up afterwards is nobody's to answer — the event is still
/// recorded, and the status still follows it.
#[tokio::test]
async fn a_reviewer_that_already_voted_raises_no_attention() {
    let h = harness().await;
    let (goal, task) = h.active_goal_with_task().await;
    h.hand_to_engineer(&task).await;
    h.store
        .transition_task(
            &task.id,
            TaskStatus::UnderReview,
            Actor::Engineer,
            None,
            None,
        )
        .await
        .unwrap();
    // Entering review opens the round the verdict belongs to.
    let task = h.store.get_task(&task.id).await.unwrap();
    let reviewer = h.store.list_task_reviewers(&task.id).await.unwrap()[0].clone();
    let session = h
        .session(&goal, Some(&task), Role::Reviewer, &reviewer)
        .await;

    let post = async |kind: &str, payload: serde_json::Value| {
        let request = post_json(
            "/internal/agent-events",
            serde_json::json!({
                "session_id": session.id,
                "agent_kind": "claude_code",
                "kind": kind,
                "payload": payload,
            }),
        );
        let response = h.router.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::ACCEPTED, "{kind}");
    };

    // The round is still waiting on this reviewer: the prompt is raised.
    post("notification", permission_prompt()).await;
    assert_eq!(
        h.store
            .get_session(&session.id)
            .await
            .unwrap()
            .attention_reason(),
        Some(AttentionReason::WaitingPermission)
    );
    // Back at work, so the flag comes down of its own accord...
    post("pre_tool_use", serde_json::json!({"tool_name": "Bash"})).await;

    // ...and once the verdict is in, the same prompt raises nothing.
    h.store
        .create_review(ariadne_store::NewReview {
            task_id: task.id.clone(),
            round: task.review_round,
            reviewer_profile_id: reviewer.clone(),
            session_id: Some(session.id.clone()),
            verdict: ReviewVerdict::Approve,
            body: None,
        })
        .await
        .unwrap();
    post("notification", permission_prompt()).await;
    let quiet = h.store.get_session(&session.id).await.unwrap();
    assert_eq!(
        quiet.attention_reason(),
        None,
        "a reviewer that has voted is not an agent anybody is waiting on"
    );
    assert_eq!(
        quiet.status(),
        SessionStatus::Running,
        "withholding the flag changes nothing else about the ingestion"
    );
    let events = h
        .store
        .list_events(ariadne_store::EventFilter {
            session_id: Some(session.id.clone()),
            task_id: None,
            limit: 50,
            after: None,
        })
        .await
        .unwrap();
    assert_eq!(
        events.iter().filter(|e| e.kind == "notification").count(),
        2,
        "the event itself is recorded either way"
    );
}

/// Same for a planner once its goal has left planning: the plan is finalized,
/// so whatever its session asks for is not work anybody is waiting on.
#[tokio::test]
async fn a_planner_past_planning_raises_no_attention() {
    let h = harness().await;
    // `active_goal_with_task` finalizes the plan: the goal is already active.
    let (goal, _task) = h.active_goal_with_task().await;
    let session = h
        .session(&goal, None, Role::Planner, &goal.planner_profile_id)
        .await;

    let post = async || {
        let request = post_json(
            "/internal/agent-events",
            serde_json::json!({
                "session_id": session.id,
                "agent_kind": "claude_code",
                "kind": "notification",
                "payload": permission_prompt(),
            }),
        );
        let response = h.router.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::ACCEPTED);
    };

    post().await;
    assert_eq!(
        h.store
            .get_session(&session.id)
            .await
            .unwrap()
            .attention_reason(),
        None,
        "the goal is past planning, so its planner is owed nothing"
    );

    // And the goal status is what makes the difference: back in planning, the
    // very same prompt is the user's to answer.
    h.store
        .set_goal_status(&goal.id, GoalStatus::Planning)
        .await
        .unwrap();
    post().await;
    assert_eq!(
        h.store
            .get_session(&session.id)
            .await
            .unwrap()
            .attention_reason(),
        Some(AttentionReason::WaitingPermission)
    );
}
