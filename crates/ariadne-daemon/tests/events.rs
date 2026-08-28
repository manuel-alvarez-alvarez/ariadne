//! Integration tests for the domain-event bus, the SSE stream and CORS.
//!
//! No external binaries needed: the scheduler test only asserts on the
//! transition it makes before it reaches out to git/tmux.

mod common;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};

use ariadne_api::goals::GoalDto;
use ariadne_api::sessions::SessionDto;
use ariadne_api::stream::{DeletedDto, DomainEvent};
use ariadne_api::tasks::TaskDto;
use ariadne_api::usage::TokenUsageDto;
use ariadne_core::{
    Actor, AgentKind, AttentionReason, GoalStatus, ReviewVerdict, Role, SessionStatus, TaskStatus,
};
use ariadne_daemon::bus::{BusEvent, EventBus};
use ariadne_daemon::http::{self, AppState};
use ariadne_daemon::scheduler::{self, SchedEvent};
use ariadne_store::{EventFilter, NewReview, Task};

use common::{Harness, TIMEOUT, get, harness, next_event, next_sse_message, post_json};

/// Hand a task to its engineer, which is the state a live engineer session is
/// actually in.
///
/// Attention belongs to an agent somebody is waiting on, and nobody is waiting
/// on the engineer of a task that has not been started: the ingestion
/// withholds the flag there, so the tests that assert on one start the work
/// first.
async fn hand_to_engineer(h: &Harness, task: &Task) {
    for status in [TaskStatus::Ready, TaskStatus::InProgress] {
        h.store
            .transition_task(&task.id, status, Actor::Daemon, None, None)
            .await
            .unwrap();
    }
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

async fn notifications_recorded(h: &Harness, session_id: &str) -> usize {
    h.store
        .list_events(EventFilter {
            session_id: Some(session_id.to_string()),
            task_id: None,
            limit: 50,
            after: None,
        })
        .await
        .unwrap()
        .iter()
        .filter(|e| e.kind == "notification")
        .count()
}

#[tokio::test]
async fn http_mutation_emits_a_fat_event() {
    let h = harness().await;
    let mut rx = h.bus.subscribe();

    let (status, _) = h
        .send(post_json(
            "/v1/profiles",
            serde_json::json!({
                "name": "rust-engineer",
                "role": "engineer",
                "agent_kind": "claude_code",
                "system_prompt": "You write Rust.",
            }),
        ))
        .await;
    assert_eq!(status, StatusCode::CREATED);

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
    let cast = h.active_cast().await;
    let mut rx = h.bus.subscribe();

    let (status, _) = h
        .send(post_json(
            &format!("/v1/tasks/{}/cancel", cast.task.id),
            serde_json::json!({}),
        ))
        .await;
    assert_eq!(status, StatusCode::OK);

    let event = next_event(&mut rx, |e| e.event.kind() == "task_updated").await;
    assert_eq!(event.goal_id.as_deref(), Some(cast.goal.id.as_str()));
    assert_eq!(event.task_id.as_deref(), Some(cast.task.id.as_str()));
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
    let cast = h.active_cast().await;
    let mut rx = h.bus.subscribe();

    // No HTTP involved: the scheduler reconciles the task on its own and
    // finds its (empty) dependency set merged.
    // No sleep inhibition: a test has no business touching power management.
    let sched = scheduler::start(h.store.clone(), h.launcher.clone(), false);
    sched
        .send(SchedEvent::TaskChanged(cast.task.id.clone()))
        .unwrap();

    let event = next_event(
        &mut rx,
        |e| matches!(&e.event, DomainEvent::TaskUpdated(u) if u.task.status == TaskStatus::Ready),
    )
    .await;
    let DomainEvent::TaskUpdated(updated) = event.event else {
        unreachable!("matched on the variant above");
    };
    assert_eq!(updated.task.id, cast.task.id);
    let transition = updated
        .transition
        .expect("status change carries its audit row");
    assert_eq!(transition.to_status, "ready");
    assert_eq!(transition.actor, "daemon");
}

#[tokio::test]
async fn sse_stream_frames_events_and_honours_its_filters() {
    let h = harness().await;
    let cast = h.active_cast().await;

    let response = h
        .response(get(&format!("/v1/events/stream?goal={}", cast.goal.id)))
        .await;
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
        .transition_task(&cast.task.id, TaskStatus::Ready, Actor::Daemon, None, None)
        .await
        .unwrap();

    let message = next_sse_message(&mut body).await;
    let mut lines = message.trim_end().lines();
    let id = lines.next().unwrap();
    assert!(id.starts_with("id: "), "first line is the event id: {id:?}");
    let event_id = id.trim_start_matches("id: ");
    assert!(
        event_id.len() == 26 && event_id.chars().all(|c| c.is_ascii_alphanumeric()),
        "event id is a ULID: {id:?}"
    );
    assert_eq!(lines.next(), Some("event: task_updated"));
    let data = lines.next().unwrap().strip_prefix("data: ").unwrap();
    let payload: serde_json::Value = serde_json::from_str(data).unwrap();
    assert_eq!(payload["task"]["id"], cast.task.id);
    assert_eq!(payload["task"]["status"], "ready");
    assert_eq!(payload["transition"]["to_status"], "ready");

    // The `task` filter is the narrower one, and a goal-level event carries no
    // task id at all, so it drops that too.
    let mut body = h
        .stream(get(&format!("/v1/events/stream?task={}", cast.task.id)))
        .await;
    h.store
        .set_goal_status(&cast.goal.id, GoalStatus::Cancelled)
        .await
        .unwrap();
    h.store
        .set_task_worktree(&cast.task.id, Some("/tmp/wt"))
        .await
        .unwrap();

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
    assert_eq!(payload["task"]["worktree_path"], "/tmp/wt");
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

    let response = tower::ServiceExt::oneshot(router, get("/v1/events/stream"))
        .await
        .unwrap();
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
    let end = tokio::time::timeout(TIMEOUT, http_body_util::BodyExt::frame(&mut body))
        .await
        .expect("stream must close after resync, not hang");
    assert!(end.is_none(), "no frames follow the resync event");
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
    let response = h.response(preflight).await;
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
    let response = h.response(request).await;
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
    let cast = h.active_cast().await;
    let mut rx = h.bus.subscribe();

    let session = h
        .session(
            &cast.goal,
            Some(&cast.task),
            Role::Engineer,
            &cast.engineer.id,
        )
        .await;

    let event = next_event(&mut rx, |e| e.event.kind() == "session_created").await;
    assert_eq!(event.goal_id.as_deref(), Some(cast.goal.id.as_str()));
    assert_eq!(event.task_id.as_deref(), Some(cast.task.id.as_str()));

    h.set_status(&session, SessionStatus::Exited).await;
    let event = next_event(&mut rx, |e| e.event.kind() == "session_updated").await;
    let DomainEvent::SessionUpdated(dto) = event.event else {
        unreachable!("matched on kind above");
    };
    assert_eq!(dto.id, session.id);
    assert_eq!(dto.status, SessionStatus::Exited);
    assert!(dto.ended_at.is_some());
}

/// Attention rides the same ingestion path as liveness: an agent that reports
/// an error needs the user, and one that goes back to work does not — while
/// going idle, which is exactly when a prompt may be waiting, leaves the flag
/// alone. Every change reaches the bus as a `session_updated`.
#[tokio::test]
async fn ingested_events_raise_and_clear_session_attention() {
    let h = harness().await;
    let cast = h.active_cast().await;
    hand_to_engineer(&h, &cast.task).await;
    let session = h
        .session_on(
            &cast.goal,
            Some(&cast.task),
            Role::Engineer,
            &cast.engineer.id,
            AgentKind::Opencode,
        )
        .await;
    let mut rx = h.bus.subscribe();

    // A failed turn: attention is raised, the lifecycle status is untouched.
    h.ingest(&session, "session.error", serde_json::json!({}))
        .await;
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
    assert_eq!(event.task_id.as_deref(), Some(cast.task.id.as_str()));

    // Going idle is when a permission prompt or a question is waiting: it
    // must not clear anything.
    h.ingest(&session, "session.idle", serde_json::json!({}))
        .await;
    assert_eq!(
        h.attention(&session).await,
        Some(AttentionReason::AgentError)
    );

    // Back to work: the agent needs nobody now.
    h.ingest(&session, "tool.execute.before", serde_json::json!({}))
        .await;
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
    h.set_status(&session, SessionStatus::Exited).await;
    h.raise(&session, AttentionReason::Disconnected).await;
    h.ingest(&session, "tool.execute.before", serde_json::json!({}))
        .await;
    let ended = h.store.get_session(&session.id).await.unwrap();
    assert_eq!(
        ended.attention_reason(),
        Some(AttentionReason::Disconnected)
    );
    assert_eq!(ended.status(), SessionStatus::Exited);

    // Nor does a late dialog: an approval asked for by a session already
    // recorded as ended has no pane the user could answer it in, so it
    // neither goes up nor writes over the reason the session ended with.
    h.ingest(&session, "permission.asked", serde_json::json!({}))
        .await;
    assert_eq!(
        h.attention(&session).await,
        Some(AttentionReason::Disconnected)
    );
}

/// OpenCode's approval dialog reaches Ariadne as `permission.asked` on the
/// plugin's event stream — the payloads below are what opencode 1.18.15
/// actually sent during a run with `permission.bash = "ask"`. The session
/// looks no different while the dialog is up, so this is the only signal.
#[tokio::test]
async fn an_opencode_permission_ask_flags_the_session_as_blocked() {
    let h = harness().await;
    let cast = h.active_cast().await;
    hand_to_engineer(&h, &cast.task).await;
    let session = h
        .session_on(
            &cast.goal,
            Some(&cast.task),
            Role::Engineer,
            &cast.engineer.id,
            AgentKind::Opencode,
        )
        .await;

    // Working, so the internal id is already known and the flag is down.
    h.ingest(
        &session,
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
    h.ingest(
        &session,
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
    h.ingest(
        &session,
        "session.updated",
        serde_json::json!({"info": {"id": "ses_fe5cb9641ffeQPvwaIKtSsLAqP"}}),
    )
    .await;
    assert_eq!(
        h.attention(&session).await,
        Some(AttentionReason::WaitingPermission)
    );

    // The user answered — rejected, here, which still hands control back.
    h.ingest(
        &session,
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
    h.ingest(
        &session,
        "question.asked",
        serde_json::json!({
            "id": "ask_01a3575b4001aOsIrUVWB44A4f",
            "sessionID": "ses_fe5cb9641ffeQPvwaIKtSsLAqP",
        }),
    )
    .await;
    assert_eq!(
        h.attention(&session).await,
        Some(AttentionReason::WaitingInput)
    );
    h.ingest(
        &session,
        "question.replied",
        serde_json::json!({
            "sessionID": "ses_fe5cb9641ffeQPvwaIKtSsLAqP",
            "requestID": "ask_01a3575b4001aOsIrUVWB44A4f",
            "answers": [],
        }),
    )
    .await;
    assert_eq!(h.attention(&session).await, None);

    // An error while a dialog is up must not be traded for the wait, and
    // neither may clear the other: the flag stands until real work resumes.
    h.ingest(
        &session,
        "permission.asked",
        serde_json::json!({"id": "per_2"}),
    )
    .await;
    h.ingest(&session, "session.error", serde_json::json!({}))
        .await;
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
    let cast = h.active_cast().await;
    hand_to_engineer(&h, &cast.task).await;
    let session = h
        .session(
            &cast.goal,
            Some(&cast.task),
            Role::Engineer,
            &cast.engineer.id,
        )
        .await;
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
    h.ingest(
        &session,
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
    h.ingest(
        &session,
        "pre_tool_use",
        serde_json::json!({"tool_name": "Bash"}),
    )
    .await;
    let cleared = h.store.get_session(&session.id).await.unwrap();
    assert_eq!(cleared.attention_reason(), None);
    assert_eq!(cleared.status(), SessionStatus::Running);

    // Idle at the prompt: waiting for an answer, not for permission.
    h.ingest(
        &session,
        "notification",
        notification("idle_prompt", "Claude is waiting for your input"),
    )
    .await;
    assert_eq!(
        h.attention(&session).await,
        Some(AttentionReason::WaitingInput)
    );

    // Submitting a prompt is the other half of the clearing rule.
    h.ingest(
        &session,
        "user_prompt_submit",
        serde_json::json!({"prompt": "go on"}),
    )
    .await;
    assert_eq!(h.attention(&session).await, None);

    // An unrecognized notification is recorded but changes nothing.
    h.ingest(
        &session,
        "notification",
        notification("auth_success", "Logged in as me@example.com"),
    )
    .await;
    let untouched = h.store.get_session(&session.id).await.unwrap();
    assert_eq!(untouched.attention_reason(), None);
    assert_eq!(untouched.status(), SessionStatus::Running);
    assert_eq!(notifications_recorded(&h, &session.id).await, 3);
}

/// Attention says a human must act, so it is only raised on an agent somebody
/// is still waiting on. A reviewer that has cast its verdict is finished, and
/// a dialog it puts up afterwards is nobody's to answer — the event is still
/// recorded, and the status still follows it.
#[tokio::test]
async fn a_reviewer_that_already_voted_raises_no_attention() {
    let h = harness().await;
    let cast = h.active_cast().await;
    hand_to_engineer(&h, &cast.task).await;
    h.store
        .transition_task(
            &cast.task.id,
            TaskStatus::UnderReview,
            Actor::Engineer,
            None,
            None,
        )
        .await
        .unwrap();
    // Entering review opens the round the verdict belongs to.
    let task = h.store.get_task(&cast.task.id).await.unwrap();
    let session = h
        .session(&cast.goal, Some(&task), Role::Reviewer, &cast.reviewer.id)
        .await;

    // The round is still waiting on this reviewer: the prompt is raised.
    h.ingest(&session, "notification", permission_prompt()).await;
    assert_eq!(
        h.attention(&session).await,
        Some(AttentionReason::WaitingPermission)
    );
    // Back at work, so the flag comes down of its own accord...
    h.ingest(
        &session,
        "pre_tool_use",
        serde_json::json!({"tool_name": "Bash"}),
    )
    .await;

    // ...and once the verdict is in, the same prompt raises nothing.
    h.store
        .create_review(NewReview {
            task_id: task.id.clone(),
            round: task.review_round,
            reviewer_profile_id: cast.reviewer.id.clone(),
            session_id: Some(session.id.clone()),
            verdict: ReviewVerdict::Approve,
            body: None,
        })
        .await
        .unwrap();
    h.ingest(&session, "notification", permission_prompt()).await;
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
    assert_eq!(
        notifications_recorded(&h, &session.id).await,
        2,
        "the event itself is recorded either way"
    );
}

/// Same for a planner once it has finalized its plan: the goal is being
/// worked on, so whatever its session asks for is not work anybody is waiting
/// on.
#[tokio::test]
async fn a_planner_past_the_approval_raises_no_attention() {
    let h = harness().await;
    // `active_cast` finalizes the plan: the goal is already active.
    let cast = h.active_cast().await;
    let session = h
        .session(&cast.goal, None, Role::Planner, &cast.planner.id)
        .await;

    h.ingest(&session, "notification", permission_prompt()).await;
    assert_eq!(
        h.attention(&session).await,
        None,
        "the plan is finalized and running, so its planner is owed nothing"
    );

    // And the goal status is what makes the difference: back in planning, the
    // very same prompt is the user's to answer.
    h.store
        .set_goal_status(&cast.goal.id, GoalStatus::Planning)
        .await
        .unwrap();
    h.ingest(&session, "notification", permission_prompt()).await;
    assert_eq!(
        h.attention(&session).await,
        Some(AttentionReason::WaitingPermission)
    );
}

// -- token usage ------------------------------------------------------------

fn tokens(input_tokens: u64, cached_input_tokens: u64, output_tokens: u64) -> TokenUsageDto {
    TokenUsageDto {
        input_tokens,
        cached_input_tokens,
        output_tokens,
    }
}

/// An event carrying the totals of one transcript, exactly as the hooks and
/// the plugin report them.
fn reports(source: &str, usage: TokenUsageDto) -> serde_json::Value {
    serde_json::json!({
        "hook_event_name": "Stop",
        "ariadne_usage": {
            "source": source,
            "input_tokens": usage.input_tokens,
            "cached_input_tokens": usage.cached_input_tokens,
            "output_tokens": usage.output_tokens,
        },
    })
}

/// Everything an agent reports lands on its own session, rolls up to the task
/// and to the goal, and every watcher of the three hears it: a report is the
/// whole of one transcript, so a second one under the same source replaces it
/// and only a second source adds.
#[tokio::test]
async fn reported_usage_rolls_up_to_the_task_and_the_goal() {
    let h = harness().await;
    let cast = h.active_cast().await;
    let engineer = h
        .session(&cast.goal, Some(&cast.task), Role::Engineer, &cast.engineer.id)
        .await;
    let reviewer = h
        .session(&cast.goal, Some(&cast.task), Role::Reviewer, &cast.reviewer.id)
        .await;
    let planner = h
        .session(&cast.goal, None, Role::Planner, &cast.planner.id)
        .await;
    let mut rx = h.bus.subscribe();

    h.ingest(&engineer, "stop", reports("/x.jsonl", tokens(100, 80, 10)))
        .await;

    // The rollup rides in all three fat events, since all three are read with
    // it: a client holding a task would otherwise never hear its figures move.
    next_event(&mut rx, |e| {
        matches!(&e.event, DomainEvent::SessionUpdated(s)
                 if s.id == engineer.id && s.usage == tokens(100, 80, 10))
    })
    .await;
    next_event(&mut rx, |e| {
        matches!(&e.event, DomainEvent::TaskUpdated(u)
                 if u.task.id == cast.task.id && u.task.usage.total == tokens(100, 80, 10))
    })
    .await;
    next_event(&mut rx, |e| {
        matches!(&e.event, DomainEvent::GoalUpdated(g)
                 if g.id == cast.goal.id && g.usage.total == tokens(100, 80, 10))
    })
    .await;

    // The same transcript, further along: the session stands at the second
    // figures, not at the sum of both.
    h.ingest(&engineer, "stop", reports("/x.jsonl", tokens(150, 120, 30)))
        .await;
    let session: SessionDto = h.get(&format!("/v1/sessions/{}", engineer.id)).await;
    assert_eq!(session.usage, tokens(150, 120, 30));

    // A resumed agent writes a transcript of its own, and that one adds.
    h.ingest(&engineer, "stop", reports("/y.jsonl", tokens(10, 0, 5)))
        .await;
    let session: SessionDto = h.get(&format!("/v1/sessions/{}", engineer.id)).await;
    assert_eq!(session.usage, tokens(160, 120, 35));

    h.ingest(&reviewer, "stop", reports("/r.jsonl", tokens(20, 10, 4)))
        .await;
    h.ingest(&planner, "stop", reports("/p.jsonl", tokens(40, 30, 8)))
        .await;

    let task: TaskDto = h.get(&format!("/v1/tasks/{}", cast.task.id)).await;
    assert_eq!(task.usage.engineer, tokens(160, 120, 35));
    let reviewers = &task.usage.reviewers;
    assert_eq!(reviewers.len(), 1);
    assert_eq!(reviewers[0].profile_id, cast.reviewer.id);
    assert_eq!(reviewers[0].profile_name.as_deref(), Some("reviewer"));
    assert_eq!(reviewers[0].usage, tokens(20, 10, 4));
    assert_eq!(
        task.usage.total,
        tokens(180, 130, 39),
        "the total is its engineer and its reviewers, and nothing else is on the task"
    );

    let goal: GoalDto = h.get(&format!("/v1/goals/{}", cast.goal.id)).await;
    assert_eq!(goal.usage.planner, tokens(40, 30, 8));
    assert_eq!(goal.usage.engineers, tokens(160, 120, 35));
    assert_eq!(goal.usage.reviewers, tokens(20, 10, 4));
    assert_eq!(
        goal.usage.total,
        tokens(220, 160, 47),
        "every session of the goal, the planner's included"
    );
}

/// A session nobody has reported for reads as zeros rather than as nothing,
/// and so do the task and the goal above it.
#[tokio::test]
async fn a_session_that_has_reported_nothing_reads_as_zeros() {
    let h = harness().await;
    let cast = h.active_cast().await;
    let engineer = h
        .session(&cast.goal, Some(&cast.task), Role::Engineer, &cast.engineer.id)
        .await;

    let session: SessionDto = h.get(&format!("/v1/sessions/{}", engineer.id)).await;
    assert_eq!(session.usage, tokens(0, 0, 0));
    let task: TaskDto = h.get(&format!("/v1/tasks/{}", cast.task.id)).await;
    assert_eq!(task.usage.total, tokens(0, 0, 0));
    assert_eq!(task.usage.engineer, tokens(0, 0, 0));
    let goal: GoalDto = h.get(&format!("/v1/goals/{}", cast.goal.id)).await;
    assert_eq!(goal.usage.total, tokens(0, 0, 0));
}

/// Figures nobody can read are dropped on their own: the event they came on
/// is recorded like any other, and everything else it carries still happens.
#[tokio::test]
async fn a_malformed_report_is_dropped_and_its_event_still_lands() {
    let h = harness().await;
    let cast = h.active_cast().await;
    let engineer = h
        .session(&cast.goal, Some(&cast.task), Role::Engineer, &cast.engineer.id)
        .await;

    h.ingest(
        &engineer,
        "stop",
        serde_json::json!({
            "hook_event_name": "Stop",
            "ariadne_usage": {"source": "/x.jsonl", "input_tokens": -5, "output_tokens": 1},
        }),
    )
    .await;

    let session: SessionDto = h.get(&format!("/v1/sessions/{}", engineer.id)).await;
    assert_eq!(session.usage, tokens(0, 0, 0));
    assert_eq!(
        session.status,
        SessionStatus::Idle,
        "the event still moved the status it was sent for"
    );
    let events = h
        .store
        .list_events(EventFilter {
            session_id: Some(engineer.id.clone()),
            task_id: None,
            limit: 50,
            after: None,
        })
        .await
        .unwrap();
    assert_eq!(events.len(), 1, "the event is recorded, payload and all");
    assert_eq!(events[0].kind, "stop");
}
