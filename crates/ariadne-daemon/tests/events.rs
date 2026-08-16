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
use ariadne_core::{AgentKind, GoalStatus, Role, TaskStatus};
use ariadne_daemon::bus::{BusEvent, EventBus};
use ariadne_daemon::config::Config;
use ariadne_daemon::gitwt::GitManager;
use ariadne_daemon::http::{self, AppState};
use ariadne_daemon::launcher::Launcher;
use ariadne_daemon::scheduler::{self, SchedEvent};
use ariadne_daemon::tmux::TmuxManager;
use ariadne_store::{Goal, NewGoal, NewProfile, NewTask, Profile, Store, Task};

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
                extra_flags: vec![],
            })
            .await
            .unwrap()
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
        let goal = self
            .store
            .create_goal(NewGoal {
                title: "Ship the UI".into(),
                description: "desc".into(),
                planner_profile_id: planner.id,
                max_tasks: None,
                required_approvals: 1,
                // Not a git repo: the scheduler fails right after the
                // transition we assert on, which is all this needs.
                repos: vec![(
                    self.dir.path().join("repo").display().to_string(),
                    "main".into(),
                )],
            })
            .await
            .unwrap();
        let repo = self
            .store
            .list_goal_repos(&goal.id)
            .await
            .unwrap()
            .remove(0);
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
    let sched = scheduler::start(h.store.clone(), h.launcher.clone());
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
