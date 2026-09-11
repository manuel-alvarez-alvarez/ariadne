//! Integration tests for the domain-event bus, the SSE stream and CORS.
//!
//! No external binaries needed: the scheduler test only asserts on the
//! transition it makes before it reaches out to git or an agent.

mod common;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};

use ariadne_api::events::AgentEventDto;
use ariadne_api::goals::GoalDto;
use ariadne_api::sessions::SessionDto;
use ariadne_api::stream::{DeletedDto, DomainEvent};
use ariadne_api::tasks::TaskDto;
use ariadne_api::usage::TokenUsageDto;
use ariadne_core::{
    Actor, AttentionReason, GoalStatus, MessageKind, Seat, SessionStatus, TaskStatus,
};
use ariadne_daemon::bus::{BusEvent, EventBus};
use ariadne_daemon::http::{self, AppState};
use ariadne_daemon::scheduler::{self, SchedEvent};
use ariadne_store::{EventFilter, Task};

use common::{Harness, TIMEOUT, expect_sse, get, harness, next_event, next_sse_message, post_json};

/// Hand a task to its author, which is the state a live author session is
/// actually in.
///
/// Attention belongs to an agent somebody is waiting on, and nobody is waiting
/// on the author of a task that has not been started: the ingestion
/// withholds the flag there, so the tests that assert on one start the work
/// first.
async fn hand_to_author(h: &Harness, task: &Task) {
    for status in [TaskStatus::Ready, TaskStatus::InProgress] {
        h.store
            .transition_task(&task.id, status, Actor::Daemon, None, None)
            .await
            .unwrap();
    }
}

/// A permission request as the runtime reports it: the call the agent asks
/// about and the options it offers — what a session waiting on an approval
/// reports, whoever it belongs to.
fn permission_request() -> serde_json::Value {
    serde_json::json!({
        "session_id": "stub-session",
        "tool_name": "Bash",
        "tool_input": {"command": "touch /tmp/probe"},
        "options": [{"optionId": "yes", "name": "Allow", "kind": "allow_once"}],
    })
}

/// How many events of `kind` this session has reported.
async fn recorded(h: &Harness, session: &ariadne_store::AgentSession, kind: &str) -> usize {
    h.store
        .list_events(EventFilter {
            session_id: Some(session.id.clone()),
            task_id: None,
            limit: 50,
            after: None,
        })
        .await
        .unwrap()
        .iter()
        .filter(|e| e.kind == kind)
        .count()
}

/// How many permission requests this session has reported.
async fn permission_requests_recorded(h: &Harness, session_id: &str) -> usize {
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
        .filter(|e| e.kind == "permission_request")
        .count()
}

#[tokio::test]
async fn http_mutation_emits_a_fat_event() {
    let h = harness().await;
    let mut rx = h.bus.subscribe();

    let (status, _) = h
        .send(post_json(
            "/v1/skills",
            serde_json::json!({
                "name": "api-design",
                "document": "---\nname: api-design\ndescription: shape an API\n---\n",
            }),
        ))
        .await;
    assert_eq!(status, StatusCode::CREATED);

    let event = next_event(&mut rx, |e| e.event.kind() == "skill_created").await;
    let DomainEvent::SkillCreated(skill) = event.event else {
        unreachable!("matched on kind above");
    };
    // Fat payload: the whole DTO, not just a name to refetch.
    assert_eq!(skill.name, "api-design");
    assert_eq!(skill.summary, "shape an API");
    assert!(!skill.builtin);
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
    // finds its (empty) dependency set finished.
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

/// A browser's `EventSource` never surfaces the comment the other streams
/// keep alive with, so this one opens by saying, in a frame a client can see,
/// that the daemon is there and which daemon it is.
#[tokio::test]
async fn sse_stream_opens_with_a_heartbeat() {
    let h = harness().await;
    let mut body = h.stream(get("/v1/events/stream")).await;

    let beat = expect_sse(&mut body, "heartbeat").await;
    assert_eq!(
        beat["version"],
        env!("CARGO_PKG_VERSION"),
        "the heartbeat carries the version /v1/version reports: {beat}"
    );
    let started_at = beat["started_at"]
        .as_str()
        .unwrap_or_else(|| panic!("the heartbeat carries a start time: {beat}"));
    chrono::DateTime::parse_from_rfc3339(started_at)
        .unwrap_or_else(|e| panic!("started_at is RFC 3339: {started_at:?} ({e})"));

    // And the domain events still follow it.
    h.bus.publish(BusEvent {
        event: DomainEvent::SkillDeleted(DeletedDto {
            id: "skill-gone".into(),
        }),
        goal_id: None,
        task_id: None,
    });
    let payload = expect_sse(&mut body, "skill_deleted").await;
    assert_eq!(payload["id"], "skill-gone");
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
    // Every connection opens with a heartbeat; the domain events follow it.
    expect_sse(&mut body, "heartbeat").await;

    // Out of scope for this goal: must not reach the stream.
    h.store
        .set_skill_document("coding", "---\nname: coding\ndescription: ours\n---\n")
        .await
        .unwrap();
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
    expect_sse(&mut body, "heartbeat").await;
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
            event: DomainEvent::SkillDeleted(DeletedDto {
                id: format!("skill-{i}"),
            }),
            goal_id: None,
            task_id: None,
        });
    }

    // Past the opening heartbeat is what the connection has to say about the
    // events it lost.
    let beat = next_sse_message(&mut body).await;
    assert!(
        beat.contains("event: heartbeat"),
        "a connection opens with a heartbeat, got: {beat:?}"
    );
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
        .uri("/v1/skills")
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
        .session(&cast.goal, Some(&cast.task), Seat::Author, &cast.author.id)
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

/// A relaunch starts a new agent under the same session row, and the agent it
/// replaced still has its exit to report: for a moment the daemon hears from
/// two processes under one session, on a resumed conversation even under one
/// internal id. The launch each of them reports is the only thing that says
/// which is the agent running.
///
/// So the dead one's word moves nothing. Believed, its `session_end` retires
/// a session whose agent is working — and the goal then spends its spawn
/// budget replacing an agent it already has. The event is recorded all the
/// same: it is what happened, it is simply no longer news about the agent.
#[tokio::test]
async fn an_event_from_a_launch_the_session_has_moved_past_changes_nothing() {
    let h = harness().await;
    let cast = h.active_cast().await;
    hand_to_author(&h, &cast.task).await;
    let session = h
        .session(&cast.goal, Some(&cast.task), Seat::Author, &cast.author.id)
        .await;
    h.store
        .set_session_launch(&session.id, "01launchtwoxxxxxxxxxxxxxxx")
        .await
        .unwrap();
    h.set_status(&session, SessionStatus::Running).await;

    // The agent that was killed, still going through its exit: a question it
    // will never be answered on, and the end of a process nobody is watching.
    h.ingest_from(
        &session,
        "01launchonexxxxxxxxxxxxxxx",
        "permission_request",
        permission_request(),
    )
    .await;
    assert_eq!(h.attention(&session).await, None);
    h.ingest_from(
        &session,
        "01launchonexxxxxxxxxxxxxxx",
        "session_end",
        serde_json::json!({}),
    )
    .await;
    assert_eq!(h.session_status(&session).await, SessionStatus::Running);
    assert_eq!(
        recorded(&h, &session, "session_end").await,
        1,
        "the event still landed"
    );

    // The agent that is actually running, saying the same words.
    h.ingest_from(
        &session,
        "01launchtwoxxxxxxxxxxxxxxx",
        "session_end",
        serde_json::json!({}),
    )
    .await;
    assert_eq!(h.session_status(&session).await, SessionStatus::Exited);
    assert_eq!(recorded(&h, &session, "session_end").await, 2);
}

/// The summary the daemon builds from a payload is never stored — it is
/// built when the DTO is built — so both surfaces that hand one out have to
/// agree: the recorded snapshot `GET /v1/events` answers, and the live event
/// the SSE stream carries for the same report.
#[tokio::test]
async fn an_events_summary_reaches_the_snapshot_and_the_stream_alike() {
    let h = harness().await;
    let cast = h.active_cast().await;
    // Synced on the bus directly, so the session's own `session_created`
    // reaches the daemon's async relay before the SSE stream subscribes —
    // otherwise it can still be in flight once the stream opens.
    let mut sync = h.bus.subscribe();
    let session = h
        .session(&cast.goal, Some(&cast.task), Seat::Author, &cast.author.id)
        .await;
    next_event(
        &mut sync,
        |e| matches!(&e.event, DomainEvent::SessionCreated(s) if s.id == session.id),
    )
    .await;
    drop(sync);

    let mut body = h.stream(get("/v1/events/stream")).await;
    expect_sse(&mut body, "heartbeat").await;

    h.ingest(
        &session,
        "pre_tool_use",
        serde_json::json!({
            "cwd": "/tmp/wt",
            "tool_name": "Bash",
            "tool_input": {"command": "cargo nextest run"},
        }),
    )
    .await;

    let events: Vec<AgentEventDto> = h.get(&format!("/v1/events?session={}", session.id)).await;
    let recorded = events
        .iter()
        .find(|e| e.kind == "pre_tool_use")
        .expect("the tool call was recorded");
    assert_eq!(recorded.summary, "Bash: cargo nextest run");

    let live = expect_sse(&mut body, "agent_event").await;
    assert_eq!(live["summary"], "Bash: cargo nextest run");
}

/// Attention rides the same ingestion path as liveness: an agent that reports
/// an error needs the user, and one that goes back to work does not — while
/// going idle takes down only what it disproves, which is the failed turn it
/// recovered from. Every change reaches the bus as a `session_updated`.
#[tokio::test]
async fn ingested_events_raise_and_clear_session_attention() {
    let h = harness().await;
    let cast = h.active_cast().await;
    hand_to_author(&h, &cast.task).await;
    let session = h
        .session(&cast.goal, Some(&cast.task), Seat::Author, &cast.author.id)
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

    // A turn that ends on idle rather than on another error has recovered:
    // the error goes, and the session is nobody's business again.
    h.ingest(&session, "stop", serde_json::json!({})).await;
    assert_eq!(h.attention(&session).await, None);

    // Back to work, with the error raised again: the agent needs nobody now.
    h.raise(&session, AttentionReason::AgentError).await;
    h.ingest(&session, "pre_tool_use", serde_json::json!({}))
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
    h.ingest(&session, "pre_tool_use", serde_json::json!({}))
        .await;
    let ended = h.store.get_session(&session.id).await.unwrap();
    assert_eq!(
        ended.attention_reason(),
        Some(AttentionReason::Disconnected)
    );
    assert_eq!(ended.status(), SessionStatus::Exited);

    // Nor does a late request: an approval asked for by a session already
    // recorded as ended has no agent left to hear the answer, so it neither
    // goes up nor writes over the reason the session ended with.
    h.ingest(&session, "permission_request", permission_request())
        .await;
    assert_eq!(
        h.attention(&session).await,
        Some(AttentionReason::Disconnected)
    );
}

/// A session that says it went idle is not a silent one, and one that says it
/// after a failed turn has recovered from it: those two flags come down, and
/// the task's stall column with them.
///
/// Nothing else does. Going idle is exactly when a permission request is
/// waiting, so a prompt survives it, and `waiting_user` was never the agent's
/// to take down.
#[tokio::test]
async fn an_idle_report_clears_the_stall_and_the_error_and_nothing_else() {
    let h = harness().await;
    let cast = h.active_cast().await;
    hand_to_author(&h, &cast.task).await;
    let stalled = async || h.store.get_task(&cast.task.id).await.unwrap().is_stalled();
    let session = h
        .session(&cast.goal, Some(&cast.task), Seat::Author, &cast.author.id)
        .await;

    // A stalled author that answers its nudge: the `stop` ending the turn is
    // the agent reporting, which is the one thing the flag denied.
    h.raise(&session, AttentionReason::Stalled).await;
    assert!(stalled().await, "the task says what its agent's flag says");
    h.ingest(&session, "stop", serde_json::json!({})).await;
    assert_eq!(h.attention(&session).await, None);
    assert!(!stalled().await, "and follows it back down");

    // A turn that failed and then ended on idle is a turn that recovered.
    h.raise(&session, AttentionReason::AgentError).await;
    h.ingest(&session, "stop", serde_json::json!({})).await;
    assert_eq!(h.attention(&session).await, None);

    // The request the user still has to answer stands through the idle it is
    // waiting in...
    h.raise(&session, AttentionReason::WaitingPermission).await;
    h.ingest(&session, "stop", serde_json::json!({})).await;
    assert_eq!(
        h.attention(&session).await,
        Some(AttentionReason::WaitingPermission)
    );

    // ...and so does what the daemon raised for the user, which no event of
    // the agent's has ever been allowed to clear.
    h.store.clear_session_attention(&session.id).await.unwrap();
    h.raise(&session, AttentionReason::WaitingUser).await;
    h.ingest(&session, "stop", serde_json::json!({})).await;
    assert_eq!(
        h.attention(&session).await,
        Some(AttentionReason::WaitingUser)
    );
}

/// A permission request is the one signal that an agent is blocked on the
/// user: it raises the wait without reading as liveness, keeps the internal
/// id the session already reported, and comes down when the answer hands
/// control back — whichever way it went.
#[tokio::test]
async fn a_permission_request_flags_the_session_as_blocked() {
    let h = harness().await;
    let cast = h.active_cast().await;
    hand_to_author(&h, &cast.task).await;
    let session = h
        .session(&cast.goal, Some(&cast.task), Seat::Author, &cast.author.id)
        .await;

    // Working, so the internal id is already known and the flag is down.
    h.ingest(
        &session,
        "session_start",
        serde_json::json!({"session_id": "stub-session"}),
    )
    .await;
    let running = h.store.get_session(&session.id).await.unwrap();
    assert_eq!(running.status(), SessionStatus::Running);
    assert_eq!(running.internal_session_id.as_deref(), Some("stub-session"));

    // The request goes up: flagged, and the status is left where it was.
    h.ingest(&session, "permission_request", permission_request())
        .await;
    let flagged = h.store.get_session(&session.id).await.unwrap();
    assert_eq!(
        flagged.attention_reason(),
        Some(AttentionReason::WaitingPermission)
    );
    assert_eq!(flagged.status(), SessionStatus::Running);
    assert!(flagged.last_activity_at.is_some());

    // The user answered — rejected, here, which still hands control back.
    h.ingest(
        &session,
        "permission.replied",
        serde_json::json!({"session_id": "stub-session", "option_id": null}),
    )
    .await;
    let cleared = h.store.get_session(&session.id).await.unwrap();
    assert_eq!(cleared.attention_reason(), None);
    assert_eq!(cleared.status(), SessionStatus::Running);

    // An error while a request is up must not be traded for the wait, and
    // neither may clear the other: the flag stands until real work resumes.
    h.ingest(&session, "permission_request", permission_request())
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

/// Attention says a human must act, so it is only raised on an agent somebody
/// is still waiting on. A reviewer that has cast its verdict is finished, and
/// a dialog it puts up afterwards is nobody's to answer — the event is still
/// recorded, and the status still follows it.
#[tokio::test]
async fn a_reviewer_that_already_voted_raises_no_attention() {
    let h = harness().await;
    let cast = h.active_cast().await;
    hand_to_author(&h, &cast.task).await;
    h.store
        .transition_task(
            &cast.task.id,
            TaskStatus::UnderReview,
            Actor::Author,
            None,
            None,
        )
        .await
        .unwrap();
    // Entering review opens the round the verdict belongs to.
    let task = h.store.get_task(&cast.task.id).await.unwrap();
    let session = h
        .session(&cast.goal, Some(&task), Seat::Reviewer, &cast.reviewer.id)
        .await;

    // The round is still waiting on this reviewer: the prompt is raised.
    h.ingest(&session, "permission_request", permission_request())
        .await;
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
    h.verdict_from(&task, &session, MessageKind::Approve, "looks right")
        .await;
    h.ingest(&session, "permission_request", permission_request())
        .await;
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
        permission_requests_recorded(&h, &session.id).await,
        2,
        "the event itself is recorded either way"
    );
}

/// Same for an orchestrator: it is the agent the user talks to about work
/// already running, so a permission it asks for is the user's to answer for
/// as long as the goal runs, and nobody's afterwards.
#[tokio::test]
async fn an_orchestrator_of_a_finished_goal_raises_no_attention() {
    let h = harness().await;
    // `active_cast` finalizes the plan: the goal is already active.
    let cast = h.active_cast().await;
    let session = h.orchestrator_session(&cast.goal).await;

    h.ingest(&session, "permission_request", permission_request())
        .await;
    assert_eq!(
        h.attention(&session).await,
        Some(AttentionReason::WaitingPermission),
        "the goal is under way, so its orchestrator is still owed an answer"
    );

    h.store.clear_session_attention(&session.id).await.unwrap();
    h.store
        .set_goal_status(&cast.goal.id, GoalStatus::Completed)
        .await
        .unwrap();
    h.ingest(&session, "permission_request", permission_request())
        .await;
    assert_eq!(
        h.attention(&session).await,
        None,
        "the goal is over, so nothing its orchestrator asks for is owed"
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

/// An event carrying the totals of one transcript in its `ariadne_usage`.
fn reports(source: &str, usage: TokenUsageDto) -> serde_json::Value {
    serde_json::json!({
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
    let author = h
        .session(&cast.goal, Some(&cast.task), Seat::Author, &cast.author.id)
        .await;
    let reviewer = h
        .session(
            &cast.goal,
            Some(&cast.task),
            Seat::Reviewer,
            &cast.reviewer.id,
        )
        .await;
    let orchestrator = h.orchestrator_session(&cast.goal).await;
    let mut rx = h.bus.subscribe();

    h.ingest(&author, "stop", reports("/x.jsonl", tokens(100, 80, 10)))
        .await;

    // The rollup rides in all three fat events, since all three are read with
    // it: a client holding a task would otherwise never hear its figures move.
    next_event(&mut rx, |e| {
        matches!(&e.event, DomainEvent::SessionUpdated(s)
                 if s.id == author.id && s.usage == tokens(100, 80, 10))
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
    h.ingest(&author, "stop", reports("/x.jsonl", tokens(150, 120, 30)))
        .await;
    let session: SessionDto = h.get(&format!("/v1/sessions/{}", author.id)).await;
    assert_eq!(session.usage, tokens(150, 120, 30));

    // A resumed agent writes a transcript of its own, and that one adds.
    h.ingest(&author, "stop", reports("/y.jsonl", tokens(10, 0, 5)))
        .await;
    let session: SessionDto = h.get(&format!("/v1/sessions/{}", author.id)).await;
    assert_eq!(session.usage, tokens(160, 120, 35));

    h.ingest(&reviewer, "stop", reports("/r.jsonl", tokens(20, 10, 4)))
        .await;
    h.ingest(
        &orchestrator,
        "stop",
        reports("/p.jsonl", tokens(40, 30, 8)),
    )
    .await;

    let task: TaskDto = h.get(&format!("/v1/tasks/{}", cast.task.id)).await;
    assert_eq!(task.usage.author, tokens(160, 120, 35));
    let reviewers = &task.usage.reviewers;
    assert_eq!(reviewers.len(), 1);
    assert_eq!(reviewers[0].agent_id, cast.reviewer.id);
    assert_eq!(reviewers[0].skills, vec!["code-review".to_string()]);
    assert_eq!(reviewers[0].usage, tokens(20, 10, 4));
    assert_eq!(
        task.usage.total,
        tokens(180, 130, 39),
        "the total is its author and its reviewers, and nothing else is on the task"
    );

    let goal: GoalDto = h.get(&format!("/v1/goals/{}", cast.goal.id)).await;
    assert_eq!(goal.usage.orchestrator, tokens(40, 30, 8));
    assert_eq!(goal.usage.authors, tokens(160, 120, 35));
    assert_eq!(goal.usage.reviewers, tokens(20, 10, 4));
    assert_eq!(
        goal.usage.total,
        tokens(220, 160, 47),
        "every session of the goal, the orchestrator's included"
    );
}

/// A session nobody has reported for reads as zeros rather than as nothing,
/// and so do the task and the goal above it.
#[tokio::test]
async fn a_session_that_has_reported_nothing_reads_as_zeros() {
    let h = harness().await;
    let cast = h.active_cast().await;
    let author = h
        .session(&cast.goal, Some(&cast.task), Seat::Author, &cast.author.id)
        .await;

    let session: SessionDto = h.get(&format!("/v1/sessions/{}", author.id)).await;
    assert_eq!(session.usage, tokens(0, 0, 0));
    let task: TaskDto = h.get(&format!("/v1/tasks/{}", cast.task.id)).await;
    assert_eq!(task.usage.total, tokens(0, 0, 0));
    assert_eq!(task.usage.author, tokens(0, 0, 0));
    let goal: GoalDto = h.get(&format!("/v1/goals/{}", cast.goal.id)).await;
    assert_eq!(goal.usage.total, tokens(0, 0, 0));
}

/// Figures nobody can read are dropped on their own: the event they came on
/// is recorded like any other, and everything else it carries still happens.
#[tokio::test]
async fn a_malformed_report_is_dropped_and_its_event_still_lands() {
    let h = harness().await;
    let cast = h.active_cast().await;
    let author = h
        .session(&cast.goal, Some(&cast.task), Seat::Author, &cast.author.id)
        .await;

    h.ingest(
        &author,
        "stop",
        serde_json::json!({
            "ariadne_usage": {"source": "/x.jsonl", "input_tokens": -5, "output_tokens": 1},
        }),
    )
    .await;

    let session: SessionDto = h.get(&format!("/v1/sessions/{}", author.id)).await;
    assert_eq!(session.usage, tokens(0, 0, 0));
    assert_eq!(
        session.status,
        SessionStatus::Idle,
        "the event still moved the status it was sent for"
    );
    let events = h
        .store
        .list_events(EventFilter {
            session_id: Some(author.id.clone()),
            task_id: None,
            limit: 50,
            after: None,
        })
        .await
        .unwrap();
    assert_eq!(events.len(), 1, "the event is recorded, payload and all");
    assert_eq!(events[0].kind, "stop");
}
