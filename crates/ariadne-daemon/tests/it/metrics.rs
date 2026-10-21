//! Integration tests for the metric catalog (035): each instrument read back
//! from the telemetry store the daemon's meter exports into, at the seam that
//! records it.

use crate::common;

use axum::http::StatusCode;
use chrono::{Duration as ChronoDuration, SecondsFormat, Utc};
use serde_json::json;

use ariadne_api::sessions::SessionDto;
use ariadne_core::{Actor, AttentionReason, PermissionMode, Seat, SessionStatus, TaskStatus};
use ariadne_store::AgentSession;

use common::acp::{discovery_settled, registry_home, script, stub_acp_agent};
use common::{
    Cast, Harness, RUNS_OUT, TIMEOUT, eventually, get, harness, post, post_json, test_pin,
};

const LAUNCH: &str = "01launchonexxxxxxxxxxxxxxx";
const SECOND_LAUNCH: &str = "01launchtwoxxxxxxxxxxxxxxx";

/// The name a repository is labelled with: the last part of its checkout.
fn repo_label(cast: &Cast) -> String {
    std::path::Path::new(&cast.repo.path)
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .to_string()
}

/// An RFC 3339 moment `minutes` from now.
fn in_minutes(minutes: i64) -> String {
    (Utc::now() + ChronoDuration::minutes(minutes)).to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// A usage report of `source`, as an agent's event carries it.
fn usage(source: &str, input: u64, cached: u64, output: u64) -> serde_json::Value {
    json!({"ariadne_usage": {
        "source": source,
        "input_tokens": input,
        "cached_input_tokens": cached,
        "output_tokens": output,
    }})
}

/// The labels of the develop agent's tokens of `kind`.
fn tokens<'a>(kind: &'a str, repo: &'a str) -> [(&'a str, &'a str); 5] {
    [
        ("kind", kind),
        ("model", "stub:test-model"),
        ("seat", "agent"),
        ("repo", repo),
        ("step", "develop"),
    ]
}

/// End the session's run, start it again under a second launch, and fail
/// that one: once the failure is counted, every write before it has been
/// read off the change hook, which reads changes in order.
async fn fail_a_second_launch(h: &Harness, session: &AgentSession) {
    h.store.restart_session(&session.id, None).await.unwrap();
    h.store
        .set_session_launch(&session.id, SECOND_LAUNCH)
        .await
        .unwrap();
    h.set_status(session, SessionStatus::Running).await;
    h.set_status(session, SessionStatus::Failed).await;
    eventually(TIMEOUT, "the second launch's end", || async {
        h.metric("ariadne.sessions.ended", &[("status", "failed")])
            .await
            == 1.0
    })
    .await;
}

/// A running session of the develop column's agent, its launch named.
async fn running_agent(h: &Harness) -> (Cast, AgentSession) {
    h.git_repo("repo");
    let cast = h.cast().await;
    let session = h
        .session(
            &cast.goal,
            Some(&cast.task),
            Seat::Agent,
            &cast.develop().id,
        )
        .await;
    h.store
        .set_session_launch(&session.id, LAUNCH)
        .await
        .unwrap();
    h.set_status(&session, SessionStatus::Running).await;
    (cast, session)
}

/// A task that finishes counts one ending with its column, its column's
/// model, its workflow, its repository and whether it landed, and one lead
/// time; each status it left is timed with the seconds it spent there.
#[tokio::test]
async fn a_finished_task_counts_its_ending_and_times_each_status_it_left() {
    let h = harness().await;
    let cast = h.cast().await;
    let task = &cast.task.id;
    let ready_at = in_minutes(60);
    h.store
        .transition_task_at(
            task,
            TaskStatus::Ready,
            Actor::Daemon,
            None,
            None,
            &ready_at,
        )
        .await
        .unwrap();
    h.store
        .transition_task_at(
            task,
            TaskStatus::InProgress,
            Actor::Daemon,
            None,
            None,
            &in_minutes(62),
        )
        .await
        .unwrap();
    h.store
        .transition_task(
            task,
            TaskStatus::Finished,
            Actor::Agent,
            None,
            Some("abc123"),
        )
        .await
        .unwrap();

    let repo = repo_label(&cast);
    let ended = [
        ("status", "finished"),
        ("step", "develop"),
        ("landed", "true"),
        ("workflow", cast.goal.workflow.as_str()),
        ("repo", repo.as_str()),
        ("model", "stub:test-model"),
    ];
    eventually(TIMEOUT, "the task's ending", || async {
        h.metric("ariadne.tasks.ended", &ended).await == 1.0
    })
    .await;
    assert_eq!(
        h.metric("ariadne.task.lead_time_count", &ended[..2]).await,
        1.0
    );
    // The two moments were taken a moment apart, so within a second.
    let ready = h
        .metric("ariadne.task.status_time_sum", &[("status", "ready")])
        .await;
    assert!(
        (ready - 120.0).abs() < 1.0,
        "ready from its own transition to the next one: {ready}"
    );
    for status in ["pending", "in_progress"] {
        assert_eq!(
            h.metric("ariadne.task.status_time_count", &[("status", status)])
                .await,
            1.0,
            "{status}"
        );
    }
    for labels in h.metric_labels("ariadne.tasks.ended").await {
        assert!(!labels.to_string().contains(task), "{labels}");
    }
}

/// A task cancelled before its first column counts an ending that names no
/// column and no model, and that did not land.
#[tokio::test]
async fn a_task_cancelled_before_its_first_column_names_no_column() {
    let h = harness().await;
    let cast = h.cast().await;
    h.store
        .transition_task(
            &cast.task.id,
            TaskStatus::Cancelled,
            Actor::User,
            None,
            None,
        )
        .await
        .unwrap();
    let ended = [
        ("status", "cancelled"),
        ("step", "none"),
        ("model", "none"),
        ("landed", "false"),
    ];
    eventually(TIMEOUT, "the task's ending", || async {
        h.metric("ariadne.tasks.ended", &ended).await == 1.0
    })
    .await;
}

/// A finished task's ending names the model of its committed-gate column —
/// the one that wrote the change — even where it finishes on a later one on
/// another model.
#[tokio::test]
async fn a_finished_task_names_the_model_of_its_committed_gate_column() {
    let h = harness().await;
    let cast = h.active_cast().await;
    h.move_agent(&cast.agent("merge").id, "stub:sonnet").await;
    h.advance_to(&cast.task, "merge").await;
    h.store
        .transition_task(
            &cast.task.id,
            TaskStatus::Finished,
            Actor::Daemon,
            None,
            Some("cafe1234"),
        )
        .await
        .unwrap();
    eventually(TIMEOUT, "the task's ending", || async {
        h.metric(
            "ariadne.tasks.ended",
            &[
                ("status", "finished"),
                ("step", "merge"),
                ("model", "stub:test-model"),
            ],
        )
        .await
            == 1.0
    })
    .await;
}

/// Cancelling a goal counts one ending and one lead time; a second cancel is
/// refused and counts nothing.
#[tokio::test]
async fn a_cancelled_goal_counts_one_ending() {
    let h = harness().await;
    let cast = h.cast().await;
    let path = format!("/v1/goals/{}/cancel", cast.goal.id);
    let (status, _) = h.send(post(&path)).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = h.send(post(&path)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let ended = [
        ("status", "cancelled"),
        ("workflow", cast.goal.workflow.as_str()),
    ];
    assert_eq!(h.metric("ariadne.goals.ended", &ended).await, 1.0);
    assert_eq!(h.metric("ariadne.goal.lead_time_count", &ended).await, 1.0);
}

/// An active goal whose one task walked every column and finished.
async fn finished_goal(h: &Harness) -> Cast {
    let cast = h.active_cast().await;
    h.advance_to(&cast.task, "merge").await;
    h.store
        .transition_task(
            &cast.task.id,
            TaskStatus::Finished,
            Actor::Daemon,
            None,
            Some("cafe1234"),
        )
        .await
        .unwrap();
    cast
}

/// Completing a goal counts one ending, with its repository.
#[tokio::test]
async fn a_completed_goal_counts_one_ending() {
    let h = harness().await;
    let cast = finished_goal(&h).await;
    let (status, _) = h
        .send(post_json(
            &format!("/v1/goals/{}/complete", cast.goal.id),
            json!({}),
        ))
        .await;
    assert_eq!(status, StatusCode::OK);
    let repo = repo_label(&cast);
    assert_eq!(
        h.metric(
            "ariadne.goals.ended",
            &[("status", "completed"), ("repo", repo.as_str())],
        )
        .await,
        1.0
    );
}

/// Of a cancel and a completion racing for one goal, one lands and the
/// other is refused, and one ending is counted.
#[tokio::test]
async fn racing_endings_of_one_goal_count_one() {
    let h = harness().await;
    let cast = finished_goal(&h).await;
    let cancel = format!("/v1/goals/{}/cancel", cast.goal.id);
    let complete = format!("/v1/goals/{}/complete", cast.goal.id);
    let ((a, _), (b, _), (c, _)) = tokio::join!(
        h.send(post(&cancel)),
        h.send(post_json(&complete, json!({}))),
        h.send(post(&cancel)),
    );
    let mut statuses = [a, b, c];
    statuses.sort();
    assert_eq!(
        statuses,
        [StatusCode::OK, StatusCode::CONFLICT, StatusCode::CONFLICT]
    );
    assert_eq!(h.metric("ariadne.goals.ended", &[]).await, 1.0);
    assert_eq!(h.metric("ariadne.goal.lead_time_count", &[]).await, 1.0);
}

/// Each usage write counts what it added to each kind of token, by model,
/// seat and repository; a total reported smaller than the last counts
/// nothing for that kind.
#[tokio::test]
async fn usage_writes_count_their_deltas_and_never_go_down() {
    let h = harness().await;
    let (cast, session) = running_agent(&h).await;
    h.ingest_from(&session, LAUNCH, "stop", usage(LAUNCH, 100, 20, 10))
        .await;
    h.ingest_from(&session, LAUNCH, "stop", usage(LAUNCH, 150, 20, 5))
        .await;

    let repo = repo_label(&cast);
    assert_eq!(
        h.metric("ariadne.tokens", &tokens("input", &repo)).await,
        150.0
    );
    assert_eq!(
        h.metric("ariadne.tokens", &tokens("cached_input", &repo))
            .await,
        20.0
    );
    assert_eq!(
        h.metric("ariadne.tokens", &tokens("output", &repo)).await,
        10.0
    );
}

/// Reports racing on one source each count their own step, and a second
/// source adds its own: whichever order the two reports of one source land
/// in, the count is the most that source ever held, plus the other.
#[tokio::test]
async fn racing_usage_reports_count_each_step_once() {
    let h = harness().await;
    let (cast, session) = running_agent(&h).await;
    tokio::join!(
        h.ingest_from(&session, LAUNCH, "stop", usage(LAUNCH, 100, 0, 0)),
        h.ingest_from(&session, LAUNCH, "stop", usage(LAUNCH, 200, 0, 0)),
        h.ingest_from(&session, LAUNCH, "stop", usage("transcript", 50, 0, 0)),
    );
    let repo = repo_label(&cast);
    assert_eq!(
        h.metric("ariadne.tokens", &tokens("input", &repo)).await,
        250.0
    );
}

/// A run that ends counts one ending by status, seat, model and flag, and
/// one lifetime by seat and model, however often its end is reported; the
/// next launch of the same session ends on its own.
#[tokio::test]
async fn a_run_counts_one_ending_however_often_it_is_reported() {
    let h = harness().await;
    let (_cast, session) = running_agent(&h).await;
    h.ingest_from(&session, LAUNCH, "session_end", json!({}))
        .await;
    h.ingest_from(&session, LAUNCH, "session_end", json!({}))
        .await;
    fail_a_second_launch(&h, &session).await;
    let ended = [
        ("status", "exited"),
        ("seat", "agent"),
        ("model", "stub:test-model"),
        ("step", "develop"),
        ("attention_reason", "none"),
    ];
    assert_eq!(h.metric("ariadne.sessions.ended", &ended).await, 1.0);
    assert_eq!(
        h.metric("ariadne.session.lifetime_count", &ended[1..4])
            .await,
        2.0,
        "one lifetime for each launch"
    );
}

/// A kill and the end its agent reports while the kill lands are one end.
#[tokio::test]
async fn a_kill_and_the_end_its_agent_reports_count_once() {
    let h = harness().await;
    let (_cast, session) = running_agent(&h).await;
    let (killed, ()) = tokio::join!(
        h.launcher.kill_session(&session.id),
        h.ingest_from(&session, LAUNCH, "session_end", json!({})),
    );
    killed.unwrap();
    fail_a_second_launch(&h, &session).await;
    assert_eq!(
        h.metric("ariadne.sessions.ended", &[("status", "exited")])
            .await,
        1.0
    );
}

/// A flag raised and cleared counts one raise, one clear and one wait, by
/// its reason; while it is up, the pending gauge counts it.
#[tokio::test]
async fn a_raised_and_cleared_flag_counts_the_raise_the_clear_and_the_wait() {
    let h = harness().await;
    let (_cast, session) = running_agent(&h).await;
    h.raise(&session, AttentionReason::Stalled).await;
    let stalled = [("reason", "stalled")];
    eventually(TIMEOUT, "the raise and the pending flag", || async {
        h.metric("ariadne.attention.raised", &stalled).await == 1.0
            && h.metric("ariadne.attention.pending", &stalled).await == 1.0
    })
    .await;
    h.store.clear_session_attention(&session.id).await.unwrap();
    eventually(TIMEOUT, "the clear", || async {
        h.metric("ariadne.attention.cleared", &stalled).await == 1.0
    })
    .await;
    assert_eq!(
        h.metric("ariadne.attention.wait_count", &stalled).await,
        1.0
    );
    assert_eq!(h.metric("ariadne.attention.raised", &stalled).await, 1.0);
}

/// The live-state gauges read the store on each collection: a change shows
/// in the very next one, and a status that empties reads zero. The database
/// size and the process metrics are read then too.
#[tokio::test]
async fn the_live_state_gauges_read_the_store_on_collection() {
    let h = harness().await;
    let (cast, session) = running_agent(&h).await;
    let running = [
        ("status", "running"),
        ("seat", "agent"),
        ("step", "develop"),
    ];
    assert_eq!(
        h.metric("ariadne.tasks.active", &[("status", "pending")])
            .await,
        1.0
    );
    assert_eq!(
        h.metric(
            "ariadne.goals.active",
            &[("status", cast.goal.status.as_str())]
        )
        .await,
        1.0
    );
    assert_eq!(h.metric("ariadne.sessions.live", &running).await, 1.0);

    h.store
        .transition_task(&cast.task.id, TaskStatus::Ready, Actor::Daemon, None, None)
        .await
        .unwrap();
    h.set_status(&session, SessionStatus::Exited).await;
    assert_eq!(
        h.metric("ariadne.tasks.active", &[("status", "pending")])
            .await,
        0.0
    );
    assert_eq!(
        h.metric("ariadne.tasks.active", &[("status", "ready")])
            .await,
        1.0
    );
    assert_eq!(h.metric("ariadne.sessions.live", &running).await, 0.0);

    assert!(
        h.metric("ariadne.db.size", &[("file", "telemetry.db")])
            .await
            > 0.0
    );
    assert!(h.metric("process.memory.usage", &[]).await > 0.0);
    assert!(h.metric("process.cpu.time", &[]).await > 0.0);
}

/// A session on no column — the orchestrator's — carries no step.
#[tokio::test]
async fn a_session_on_no_column_carries_no_step() {
    let h = harness().await;
    let goal = h.planning_goal().await;
    let orchestrator = h.orchestrator_session(&goal).await;
    h.set_status(&orchestrator, SessionStatus::Running).await;
    assert_eq!(
        h.metric(
            "ariadne.sessions.live",
            &[("seat", "orchestrator"), ("step", "none")],
        )
        .await,
        1.0
    );
}

/// The bucket bounds of one histogram series, as exported.
async fn bounds(h: &Harness, name: &str) -> Vec<String> {
    let mut bounds: Vec<String> = h
        .metric_labels(&format!("{name}_bucket"))
        .await
        .into_iter()
        .filter_map(|labels| labels["le"].as_str().map(str::to_string))
        .collect();
    bounds.sort();
    bounds.dedup();
    bounds
}

fn sorted(bounds: &[&str]) -> Vec<String> {
    let mut bounds: Vec<String> = bounds.iter().map(|b| b.to_string()).collect();
    bounds.push("+Inf".into());
    bounds.sort();
    bounds
}

/// Each histogram range has buckets of its own, from the provider's View: a
/// lead time up to a day, a request from a millisecond, an agent's work
/// from half a second to an hour.
#[tokio::test]
async fn each_histogram_range_has_its_own_buckets() {
    let h = harness().await;
    let cast = h.cast().await;
    h.store
        .transition_task(
            &cast.task.id,
            TaskStatus::Cancelled,
            Actor::User,
            None,
            None,
        )
        .await
        .unwrap();
    let (status, _) = h.send(get("/v1/goals")).await;
    assert_eq!(status, StatusCode::OK);
    ariadne_daemon::metrics::record_forge_poll("repo", "ok", 1.0);
    eventually(TIMEOUT, "the task's lead time", || async {
        !bounds(&h, "ariadne.task.lead_time").await.is_empty()
    })
    .await;
    assert_eq!(
        bounds(&h, "ariadne.task.lead_time").await,
        sorted(&[
            "1", "10", "60", "300", "900", "1800", "3600", "7200", "14400", "28800", "86400"
        ])
    );
    assert_eq!(
        bounds(&h, "http.server.request.duration").await,
        sorted(&[
            "0.001", "0.005", "0.01", "0.025", "0.05", "0.1", "0.25", "0.5", "1", "2.5", "5", "10",
            "30"
        ])
    );
    assert_eq!(
        bounds(&h, "ariadne.forge.poll.duration").await,
        sorted(&[
            "0.5", "1", "2", "5", "10", "30", "60", "120", "300", "600", "1800", "3600"
        ])
    );
}

/// A scheduler pass is timed.
#[tokio::test]
async fn a_scheduler_pass_is_timed() {
    let h = harness().scheduler().await;
    eventually(TIMEOUT, "a timed scheduler pass", || async {
        h.metric("ariadne.scheduler.tick.duration_count", &[]).await >= 1.0
    })
    .await;
}

/// A message is counted by its kind and the actors at its two ends.
#[tokio::test]
async fn a_message_is_counted_by_its_actors() {
    let h = harness().await;
    let cast = h.cast().await;
    let (status, _) = h
        .send(post_json(
            &format!("/v1/goals/{}/messages", cast.goal.id),
            json!({"to_actor": "orchestrator", "body": "Look at the plan."}),
        ))
        .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(
        h.metric(
            "ariadne.messages",
            &[
                ("kind", "message"),
                ("from_actor", "user"),
                ("to_actor", "orchestrator"),
            ],
        )
        .await,
        1.0
    );
}

/// A follower of the event stream is counted while its connection is open.
#[tokio::test]
async fn an_event_stream_follower_is_counted_while_it_follows() {
    let h = harness().await;
    let response = tower::ServiceExt::oneshot(h.router.clone(), get("/v1/events/stream"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(h.metric("ariadne.events.stream.clients", &[]).await, 1.0);
    drop(response);
    assert_eq!(h.metric("ariadne.events.stream.clients", &[]).await, 0.0);
}

/// The retention pass counts the rows it deletes, by table.
#[tokio::test]
async fn the_retention_pass_counts_what_it_deletes() {
    let h = harness().await;
    h.telemetry_store
        .write_metrics(&[ariadne_store::telemetry::MetricPoint {
            name: "old.metric".into(),
            kind: ariadne_store::telemetry::MetricKind::Gauge,
            unit: None,
            description: None,
            labels: json!({}),
            timestamp: "2001-01-01T00:00:00.000Z".into(),
            value: 1.0,
        }])
        .await
        .unwrap();
    ariadne_daemon::telemetry::retention::start(h.telemetry_store.clone(), RUNS_OUT, 1, 1, 1);
    eventually(TIMEOUT, "the retention pass", || async {
        h.metric("ariadne.telemetry.deleted", &[("table", "metric_samples")])
            .await
            >= 1.0
    })
    .await;
}

/// A request is timed under its route template, method and status, never
/// its path; a stream route is not timed.
#[tokio::test]
async fn a_request_is_timed_under_its_route_template() {
    let h = harness().await;
    let cast = h.cast().await;
    let (status, _) = h.send(get(&format!("/v1/goals/{}", cast.goal.id))).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = h.send(get("/v1/goals/nope")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let of = |status| {
        [
            ("http.route", "/v1/goals/{id}"),
            ("http.request.method", "GET"),
            ("http.response.status_code", status),
        ]
    };
    assert_eq!(
        h.metric("http.server.request.duration_count", &of("200"))
            .await,
        1.0
    );
    assert_eq!(
        h.metric("http.server.request.duration_count", &of("404"))
            .await,
        1.0
    );
    for labels in h.metric_labels("http.server.request.duration_count").await {
        assert!(!labels.to_string().contains(&cast.goal.id), "{labels}");
        assert_ne!(labels["http.route"], "/v1/events/stream", "{labels}");
    }
}

/// A webhook delivery with no valid signature is counted as refused.
#[tokio::test]
async fn a_refused_webhook_is_counted() {
    let h = harness().await;
    let listen = ariadne_daemon::webhooks::WebhookListen::bind(
        &h.launcher.cfg,
        h.store.clone(),
        h.state.forge_poll.clone(),
    )
    .await
    .unwrap();
    let status = reqwest::Client::new()
        .post(format!("http://{}/webhooks/github/nope", listen.address()))
        .body("{}")
        .send()
        .await
        .unwrap()
        .status();
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        h.metric("ariadne.forge.webhooks", &[("outcome", "refused")])
            .await,
        1.0
    );
}

/// An agent the scheduler starts counts its launch as ready, its turns and
/// their durations, its tool call by name, and a console permission answer with its
/// wait.
#[tokio::test]
async fn an_agent_run_counts_its_launch_turns_tools_and_permission() {
    let mut scripted = script();
    scripted["prompts"] = json!([{
        "updates": [
            {"sessionUpdate": "tool_call", "toolCallId": "call-1", "name": "Read",
             "title": "Read Cargo.toml", "kind": "read", "status": "pending"},
        ],
        "permission": {
            "toolCall": {"toolCallId": "call-2", "title": "Write Cargo.toml", "kind": "write"},
            "options": [
                {"optionId": "no", "name": "Reject", "kind": "reject_once"},
                {"optionId": "yes", "name": "Allow", "kind": "allow_once"},
            ],
        },
    }]);
    let agent_dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(agent_dir.path(), scripted);
    let h = harness()
        .home(registry_home(&stub))
        .discover_agents()
        .scheduler()
        .await;
    discovery_settled(&h, &stub).await;
    h.git_repo("repo");
    let repo = h.repository(&h.at("repo")).await;
    h.set_permission_mode(&repo, PermissionMode::Ask).await;
    let goal = h.goal_running(&repo, test_pin(), None).await;
    let task = h.task_on(&goal, &repo, "task", test_pin()).await;
    h.activate(&goal).await;
    h.advance(&task, TaskStatus::InProgress).await;
    h.notify(&task.id);
    eventually(TIMEOUT, "the column agent to start", || async {
        h.running_session(&task.id, Seat::Agent).await.is_some()
    })
    .await;
    let session = h.running_session(&task.id, Seat::Agent).await.unwrap();
    eventually(TIMEOUT, "the permission request", || async {
        h.store
            .count_session_events(&session.id, "permission_request")
            .await
            .unwrap()
            == 1
    })
    .await;
    let (status, _) = h
        .send(post_json(
            &format!("/v1/sessions/{}/console/input", session.id),
            json!({"text": "yes"}),
        ))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let agent = [
        ("seat", "agent"),
        ("model", session.model.as_str()),
        ("step", "develop"),
    ];
    eventually(TIMEOUT, "the turn and the answer", || async {
        h.metric("ariadne.turns", &agent).await >= 1.0
            && h.metric(
                "ariadne.permissions",
                &[
                    ("decided_by", "console"),
                    ("answer", "allow"),
                    ("tool_name", "write"),
                ],
            )
            .await
                == 1.0
    })
    .await;
    assert!(h.metric("ariadne.turn.duration_count", &agent).await >= 1.0);
    assert_eq!(
        h.metric(
            "ariadne.tool_calls",
            &[("tool_name", "Read"), ("model", session.model.as_str())],
        )
        .await,
        1.0
    );
    assert_eq!(
        h.metric(
            "ariadne.permission.wait_count",
            &[("decided_by", "console")]
        )
        .await,
        1.0
    );
    assert!(
        h.metric("ariadne.acp.launches", &[("outcome", "ready")])
            .await
            >= 1.0
    );
    assert!(
        h.metric("ariadne.acp.launch.duration_count", &[("outcome", "ready")])
            .await
            >= 1.0
    );
}

/// A requested cross-agent switch counts one switch from the model left to
/// the model entered.
#[tokio::test]
async fn a_requested_switch_counts_the_models_left_and_entered() {
    let h = harness().second_agent().await;
    h.git_repo("repo");
    let cast = h.cast_pinned("stub:a").await;
    h.activate(&cast.goal).await;
    h.advance(&cast.task, TaskStatus::InProgress).await;
    let task = h.store.get_task(&cast.task.id).await.unwrap();
    let old = h
        .launcher
        .start_step_agent(&task, cast.develop())
        .await
        .unwrap();
    h.launcher
        .acp
        .send_prompt(&old.id, "Begin the task.".into())
        .unwrap();
    eventually(TIMEOUT, "the first turn to end", || async {
        h.session_status(&old).await == SessionStatus::Idle
    })
    .await;
    let _: SessionDto = h
        .json(
            post_json(
                &format!("/v1/sessions/{}/switch", old.id),
                json!({ "model": "other:b" }),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(
        h.metric(
            "ariadne.switches",
            &[
                ("from_model", "stub:a"),
                ("to_model", "other:b"),
                ("reason", "requested"),
                ("automatic", "false"),
                ("same_agent", "false"),
            ],
        )
        .await,
        1.0
    );
}
