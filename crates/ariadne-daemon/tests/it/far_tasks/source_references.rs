use super::*;

#[tokio::test]
async fn new_remote_tasks_are_worker_drafts_during_source_planning() {
    let p = pair().await;
    let source_repo = forge_repository(&p.local, "source", "owner/reference").await;
    let worker_repo = forge_repository(&p.far, "worker", "owner/reference").await;
    let goal = goal_on(&p.local, &source_repo).await;
    let created: TaskDto = p
        .local
        .json(
            post_json(
                &format!("/v1/goals/{}/tasks", goal.id),
                task_body(&goal, "Worker draft", &p.model, json!({"host":"far"})),
            ),
            StatusCode::CREATED,
        )
        .await;
    let worker: TaskDto = p.far.get(&format!("/v1/tasks/{}", created.id)).await;
    assert_eq!(worker.repo_id, worker_repo.id);
    assert_eq!(worker.goal_id.as_ref(), Some(&goal.id));
    assert!(!worker.execution_context.as_ref().unwrap().authorized);
    assert_eq!(created.id, worker.id);
    assert_eq!(created.host, "far");
    assert_eq!(created.repo_id, source_repo.id);
    assert!(p.far.store.list_goals(&[]).await.unwrap().is_empty());
    assert!(
        p.local
            .store
            .list_task_agents(&created.id)
            .await
            .unwrap()
            .is_empty()
    );
    let listed: Vec<TaskDto> = p.local.get(&format!("/v1/tasks?goal={}", goal.id)).await;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, worker.id);
}

async fn setup() -> (Pair, GoalDto) {
    let p = pair().await;
    let repo = forge_repository(&p.local, "source", "owner/reference").await;
    forge_repository(&p.far, "worker", "owner/reference").await;
    let goal = goal_on(&p.local, &repo).await;
    (p, goal)
}

async fn draft(p: &Pair, goal: &GoalDto, title: &str, dependencies: &[&str]) -> TaskDto {
    p.local
        .json(
            post_json(
                &format!("/v1/goals/{}/tasks", goal.id),
                task_body(
                    goal,
                    title,
                    &p.model,
                    json!({"host":"far", "depends_on": dependencies}),
                ),
            ),
            StatusCode::CREATED,
        )
        .await
}

#[tokio::test]
async fn source_dependencies_hold_worker_authorization_until_the_plan_and_dependencies_finish() {
    let (p, goal) = setup().await;
    let first = draft(&p, &goal, "First", &[]).await;
    let second = draft(&p, &goal, "Second", &[&first.id]).await;
    let held: TaskDto = p.far.get(&format!("/v1/tasks/{}", second.id)).await;
    assert!(!held.execution_context.unwrap().authorized);
    activate(&p.local, &goal).await;
    eventually(TIMEOUT, "first draft authorization", async || {
        let task: TaskDto = p.far.get(&format!("/v1/tasks/{}", first.id)).await;
        task.execution_context.unwrap().authorized
    })
    .await;
    let held: TaskDto = p.far.get(&format!("/v1/tasks/{}", second.id)).await;
    assert!(!held.execution_context.unwrap().authorized);
    p.far
        .store
        .transition_task(&first.id, TaskStatus::InProgress, Actor::Daemon, None, None)
        .await
        .unwrap();
    p.far
        .store
        .transition_task(
            &first.id,
            TaskStatus::Finished,
            Actor::Agent,
            None,
            Some("abcdef123"),
        )
        .await
        .unwrap();
    eventually(TIMEOUT, "dependent authorization", async || {
        let task: TaskDto = p.far.get(&format!("/v1/tasks/{}", second.id)).await;
        task.execution_context.unwrap().authorized
    })
    .await;
    assert_eq!(task(&p.local, &first.id).await.status, TaskStatus::Finished);
}

#[tokio::test]
async fn source_cancellation_waits_for_worker_acknowledgment_after_reconnection() {
    let (p, goal) = setup().await;
    let source = draft(&p, &goal, "Cancel later", &[]).await;
    go_away(&p).await;
    let cancelled: TaskDto = p
        .local
        .json(
            post(&format!("/v1/tasks/{}/cancel", source.id)),
            StatusCode::OK,
        )
        .await;
    assert_eq!(cancelled.status, TaskStatus::Pending);
    eventually(TIMEOUT, "cached source progress is stale", async || {
        task(&p.local, &source.id).await.host_unreachable
    })
    .await;
    back(&p).await;
    eventually(TIMEOUT, "worker cancellation acknowledgment", async || {
        task(&p.local, &source.id).await.status == TaskStatus::Cancelled
    })
    .await;
    assert_eq!(task(&p.far, &source.id).await.status, TaskStatus::Cancelled);
}

#[tokio::test]
async fn a_finished_worker_outcome_wins_a_source_cancellation_race() {
    let (p, goal) = setup().await;
    let source = draft(&p, &goal, "Finish while disconnected", &[]).await;
    activate(&p.local, &goal).await;
    eventually(TIMEOUT, "authorization", async || {
        task(&p.far, &source.id).await.status == TaskStatus::Ready
    })
    .await;
    go_away(&p).await;
    p.local
        .json::<TaskDto>(
            post(&format!("/v1/tasks/{}/cancel", source.id)),
            StatusCode::OK,
        )
        .await;
    p.far
        .store
        .transition_task(
            &source.id,
            TaskStatus::InProgress,
            Actor::Daemon,
            None,
            None,
        )
        .await
        .unwrap();
    p.far
        .store
        .transition_task(
            &source.id,
            TaskStatus::Finished,
            Actor::Agent,
            None,
            Some("abcdef123"),
        )
        .await
        .unwrap();
    back(&p).await;
    eventually(TIMEOUT, "authoritative completion", async || {
        task(&p.local, &source.id).await.status == TaskStatus::Finished
    })
    .await;
    assert!(!task(&p.local, &source.id).await.host_unreachable);
}

#[tokio::test]
async fn source_draft_edits_advance_worker_revision_and_reject_dependency_cycles() {
    let (p, goal) = setup().await;
    let first = draft(&p, &goal, "First", &[]).await;
    let second = draft(&p, &goal, "Second", &[&first.id]).await;
    p.local
        .error(
            patch_json(
                &format!("/v1/tasks/{}", first.id),
                json!({"depends_on":[second.id]}),
            ),
            StatusCode::BAD_REQUEST,
        )
        .await;
    let edited: TaskDto = p
        .local
        .json(
            patch_json(
                &format!("/v1/tasks/{}", second.id),
                json!({"title":"Revised"}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(edited.title, "Revised");
    assert_eq!(edited.execution_context.unwrap().revision, 2);
    assert_eq!(edited.depends_on, vec![first.id]);
    assert!(task(&p.far, &second.id).await.depends_on.is_empty());
}

#[tokio::test]
async fn source_sessions_history_diffs_and_usage_resolve_the_worker_identity() {
    let (p, goal) = setup().await;
    let source = draft(&p, &goal, "Inspect worker", &[]).await;
    activate(&p.local, &goal).await;
    eventually(TIMEOUT, "worker authorization", async || {
        task(&p.far, &source.id).await.status == TaskStatus::Ready
    })
    .await;
    let worker_task = p.far.store.get_task(&source.id).await.unwrap();
    let worker_goal = p.far.store.task_goal(&worker_task).await.unwrap();
    let session = p
        .far
        .session(
            &worker_goal,
            Some(&worker_task),
            ariadne_core::Seat::Agent,
            &source.agents[0].id,
        )
        .await;
    p.far
        .store
        .upsert_session_usage(
            &session.id,
            "transcript",
            ariadne_core::TokenUsage {
                input_tokens: 20,
                cached_input_tokens: 5,
                output_tokens: 8,
            },
        )
        .await
        .unwrap();
    let projected = eventually_some(TIMEOUT, "worker session and usage at source", async || {
        let task = task(&p.local, &source.id).await;
        (task.usage.total.input_tokens == 20 && task.agents[0].session_id.is_some()).then_some(task)
    })
    .await;
    let alias = projected.agents[0].session_id.as_ref().unwrap();
    assert_eq!(alias, &format!("far~{}~{}", source.id, session.id));
    let inspected: ariadne_api::sessions::SessionDto =
        p.local.get(&format!("/v1/sessions/{alias}")).await;
    assert_eq!(inspected.id, *alias);
    assert_eq!(inspected.task_id.as_deref(), Some(source.id.as_str()));
    assert_eq!(inspected.usage.input_tokens, 20);
    let goal_read: GoalDto = p.local.get(&format!("/v1/goals/{}", goal.id)).await;
    assert_eq!(goal_read.usage.total.input_tokens, 20);
    assert_eq!(goal_read.usage.total.output_tokens, 8);
    assert_eq!(goal_read.usage.agents[0].agent_id, source.agents[0].id);
    let worker_history: Vec<Value> = p
        .far
        .get(&format!("/v1/tasks/{}/transitions", source.id))
        .await;
    let source_history: Vec<Value> = p
        .local
        .get(&format!("/v1/tasks/{}/transitions", source.id))
        .await;
    assert_eq!(source_history, worker_history);
    let repository = p
        .far
        .store
        .get_repository(&worker_task.repo_id)
        .await
        .unwrap();
    let path = std::path::Path::new(&repository.path);
    sh(path, &format!("git checkout -b {}", worker_task.branch));
    std::fs::write(path.join("reference-proof.txt"), "worker changes\n").unwrap();
    sh(path, "git add reference-proof.txt");
    sh(
        path,
        "git -c user.email=t@t -c user.name=t commit -m 'test: add worker changes'",
    );
    p.far
        .store
        .set_task_worktree(&source.id, Some(&repository.path))
        .await
        .unwrap();
    let (status, diff) = p
        .local
        .send(crate::common::get(&format!("/v1/tasks/{}/diff", source.id)))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(String::from_utf8_lossy(&diff).contains("worker changes"));
    assert!(
        p.local
            .store
            .list_task_agents(&source.id)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn deleting_a_source_goal_keeps_coordination_until_worker_cancellation_is_acknowledged() {
    let (p, goal) = setup().await;
    let source = draft(&p, &goal, "Delete while disconnected", &[]).await;
    go_away(&p).await;
    p.local
        .json::<GoalDto>(
            post(&format!("/v1/goals/{}/cancel", goal.id)),
            StatusCode::OK,
        )
        .await;
    let request = axum::http::Request::builder()
        .method("DELETE")
        .uri(format!("/v1/goals/{}", goal.id))
        .body(axum::body::Body::empty())
        .unwrap();
    assert_eq!(p.local.send(request).await.0, StatusCode::NO_CONTENT);
    assert!(
        p.local
            .store
            .source_task_reference(&source.id)
            .await
            .unwrap()
            .is_some()
    );
    back(&p).await;
    eventually(
        TIMEOUT,
        "worker cancellation after source deletion",
        async || task(&p.far, &source.id).await.status == TaskStatus::Cancelled,
    )
    .await;
    eventually(TIMEOUT, "resolved coordination removed", async || {
        p.local
            .store
            .source_task_reference(&source.id)
            .await
            .unwrap()
            .is_none()
    })
    .await;
}

#[tokio::test]
async fn a_lost_worker_creation_response_replays_one_task_identity() {
    let (p, goal) = setup().await;
    p.front.lose_next("create remote task");
    let source = draft(&p, &goal, "Recover creation", &[]).await;
    eventually(TIMEOUT, "creation reservation acknowledged", async || {
        p.local
            .store
            .source_task_reference(&source.id)
            .await
            .unwrap()
            .unwrap()
            .request
            .is_none()
    })
    .await;
    let workers: Vec<TaskDto> = p.far.get(&format!("/v1/tasks?goal={}", goal.id)).await;
    assert_eq!(workers.len(), 1);
    assert_eq!(workers[0].id, source.id);
    assert!(!workers[0].execution_context.as_ref().unwrap().authorized);
    assert!(p.far.store.list_goals(&[]).await.unwrap().is_empty());
    assert_eq!(
        task(&p.local, &source.id).await.agents[0].id,
        workers[0].agents[0].id
    );
}

#[tokio::test]
async fn incompatible_workers_require_an_upgrade_without_creating_records() {
    let (p, goal) = setup().await;
    *p.front.protocol_override.lock().unwrap() =
        Some(json!({"protocol":"old","daemon_id":"worker"}));
    let reason = refusal(
        &p.local,
        &goal,
        task_body(
            &goal,
            "Refuse incompatible worker",
            &p.model,
            json!({"host":"far"}),
        ),
        StatusCode::CONFLICT,
    )
    .await;
    assert!(reason.contains("upgrade"));
    assert!(
        p.local
            .store
            .list_tasks(Default::default())
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        p.far
            .store
            .list_tasks(Default::default())
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn stream_loss_marks_progress_stale_until_the_current_worker_is_reconciled() {
    let (p, goal) = setup().await;
    let source = draft(&p, &goal, "Follow progress", &[]).await;
    activate(&p.local, &goal).await;
    eventually(TIMEOUT, "authorized progress", async || {
        task(&p.local, &source.id).await.status == TaskStatus::Ready
    })
    .await;
    p.front.stream_cut.send_replace(true);
    eventually(TIMEOUT, "stream loss is stale", async || {
        task(&p.local, &source.id).await.host_unreachable
    })
    .await;
    assert_eq!(host(&p.local, "far").await.status, HostStatus::Ready);
    p.far
        .store
        .transition_task(
            &source.id,
            TaskStatus::InProgress,
            Actor::Daemon,
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(task(&p.local, &source.id).await.status, TaskStatus::Ready);
    p.front.stream_cut.send_replace(false);
    eventually(TIMEOUT, "reconciled current worker", async || {
        let current = task(&p.local, &source.id).await;
        current.status == TaskStatus::InProgress && !current.host_unreachable
    })
    .await;
}

#[tokio::test]
async fn dependency_failure_and_retry_preserve_one_authoritative_worker_draft() {
    let p = pair_on(true).await;
    let repo = forge_repository(&p.local, "source", "owner/control").await;
    forge_repository(&p.far, "worker", "owner/control").await;
    let goal = goal_on(&p.local, &repo).await;
    let dependency = draft(&p, &goal, "Dependency", &[]).await;
    let blocked = draft(&p, &goal, "Blocked", &[&dependency.id]).await;
    activate(&p.local, &goal).await;
    eventually(TIMEOUT, "dependency ready", async || {
        task(&p.far, &dependency.id).await.status == TaskStatus::Ready
    })
    .await;
    p.front.lose_next("dependency failure");
    p.far
        .store
        .transition_task(
            &dependency.id,
            TaskStatus::Failed,
            Actor::Daemon,
            Some("dependency failed"),
            None,
        )
        .await
        .unwrap();
    eventually(TIMEOUT, "source dependency failure", async || {
        task(&p.local, &blocked.id).await.status == TaskStatus::Failed
    })
    .await;
    let worker = task(&p.far, &blocked.id).await;
    assert_eq!(worker.status, TaskStatus::Failed);
    assert!(!worker.execution_context.unwrap().authorized);
    let failure = task(&p.local, &blocked.id).await;
    assert!(failure.reason.unwrap().contains("Dependency"));
    let history: Vec<Value> = p
        .local
        .get(&format!("/v1/tasks/{}/transitions", blocked.id))
        .await;
    assert!(history.iter().any(|t| t["to_status"] == "failed"));
    p.local
        .json::<TaskDto>(
            post(&format!("/v1/tasks/{}/retry", dependency.id)),
            StatusCode::OK,
        )
        .await;
    eventually(TIMEOUT, "dependency retried", async || {
        task(&p.far, &dependency.id).await.status == TaskStatus::Ready
    })
    .await;
    p.far
        .store
        .transition_task(
            &dependency.id,
            TaskStatus::InProgress,
            Actor::Daemon,
            None,
            None,
        )
        .await
        .unwrap();
    p.far
        .store
        .transition_task(
            &dependency.id,
            TaskStatus::Finished,
            Actor::Agent,
            None,
            Some("abcdef123"),
        )
        .await
        .unwrap();
    eventually(TIMEOUT, "dependency completion at source", async || {
        task(&p.local, &dependency.id).await.status == TaskStatus::Finished
    })
    .await;
    assert_eq!(task(&p.local, &blocked.id).await.status, TaskStatus::Failed);
    p.front.lose_next("dependency retry");
    p.local
        .json::<TaskDto>(
            post(&format!("/v1/tasks/{}/retry", blocked.id)),
            StatusCode::OK,
        )
        .await;
    eventually(
        TIMEOUT,
        "same worker draft authorized after retry",
        async || task(&p.far, &blocked.id).await.status == TaskStatus::Ready,
    )
    .await;
    assert_eq!(task(&p.local, &blocked.id).await.id, blocked.id);
    let history: Vec<Value> = p
        .far
        .get(&format!("/v1/tasks/{}/transitions", blocked.id))
        .await;
    assert_eq!(
        history
            .iter()
            .filter(|t| t["to_status"] == "failed")
            .count(),
        1
    );
    assert_eq!(
        history
            .iter()
            .filter(|t| t["to_status"] == "pending")
            .count(),
        1
    );
}

/// Feed the terminal bytes of `socket` to `emulator` until its screen
/// satisfies `done`, or fail with what it shows.
async fn draw_until(
    socket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    emulator: &mut vt100::Parser,
    what: &str,
    done: impl Fn(&vt100::Screen) -> bool,
) {
    use futures_util::StreamExt;
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    while !done(emulator.screen()) {
        let frame = tokio::time::timeout_at(deadline, socket.next())
            .await
            .unwrap_or_else(|_| {
                panic!(
                    "timed out waiting for {what}:\n{}",
                    emulator.screen().contents()
                )
            })
            .expect("the socket stays open")
            .expect("the socket does not fail");
        if let tokio_tungstenite::tungstenite::Message::Binary(bytes) = frame {
            emulator.process(&bytes);
        }
    }
}

/// The retired event routes are gone for a worker's session too: the source
/// answers `404` itself and never asks the worker, which may be an older one
/// that still serves them.
#[tokio::test]
async fn the_retired_console_routes_of_a_worker_session_are_not_forwarded() {
    let (p, goal) = setup().await;
    let source = draft(&p, &goal, "Console", &[]).await;
    let worker_task = p.far.store.get_task(&source.id).await.unwrap();
    let worker_goal = p.far.store.task_goal(&worker_task).await.unwrap();
    let session = p
        .far
        .session(
            &worker_goal,
            Some(&worker_task),
            ariadne_core::Seat::Agent,
            &source.agents[0].id,
        )
        .await;
    let alias = format!("far~{}~{}", source.id, session.id);
    let source_address = Front::default().serve(&p.local).await;
    let client = ariadne_client::Client::tcp(format!("http://{source_address}"));
    for route in ["console", "console/stream"] {
        let answer = client
            .forward(
                axum::http::Method::GET,
                &format!("/v1/sessions/{alias}/{route}"),
                None,
                Vec::new(),
            )
            .await
            .unwrap();
        assert_eq!(answer.status, 404, "{route}");
    }

    assert_eq!(
        p.front.count("retired console route"),
        0,
        "the worker was never asked for either route"
    );
}

#[tokio::test]
async fn source_trajectory_stream_and_terminal_connections_reach_the_worker_session() {
    use futures_util::SinkExt;
    let (p, goal) = setup().await;
    let source = draft(&p, &goal, "Console", &[]).await;
    let worker_task = p.far.store.get_task(&source.id).await.unwrap();
    let worker_goal = p.far.store.task_goal(&worker_task).await.unwrap();
    let session = p
        .far
        .session(
            &worker_goal,
            Some(&worker_task),
            ariadne_core::Seat::Agent,
            &source.agents[0].id,
        )
        .await;
    p.far
        .store
        .create_event(ariadne_store::NewAgentEvent {
            session_id: Some(session.id.clone()),
            task_id: Some(source.id.clone()),
            kind: "agent_message".into(),
            payload: json!({"text":"worker transcript"}),
        })
        .await
        .unwrap();
    let alias = format!("far~{}~{}", source.id, session.id);
    let source_address = Front::default().serve(&p.local).await;
    let client = ariadne_client::Client::tcp(format!("http://{source_address}"));
    let mut stream = client
        .stream(&format!("/v1/sessions/{alias}/trajectory/stream"))
        .await
        .unwrap();
    let snapshot = tokio::time::timeout(TIMEOUT, stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(snapshot.event, "snapshot");
    assert!(snapshot.data.contains("worker transcript"));
    drop(stream);
    let (mut socket, _) = tokio_tungstenite::connect_async(format!(
        "ws://{source_address}/v1/sessions/{alias}/console/terminal"
    ))
    .await
    .unwrap();
    let send =
        |message: Value| tokio_tungstenite::tungstenite::Message::Text(message.to_string().into());
    socket
        .send(send(json!({"type":"resize","cols":80,"rows":24})))
        .await
        .unwrap();
    // The worker's full-screen console draws on the emulator at the source:
    // the alternate screen, with the worker's transcript on it.
    let size = |n: u16| std::num::NonZeroU16::new(n).unwrap();
    let mut emulator = vt100::Parser::new(size(24), size(80), 0);
    draw_until(
        &mut socket,
        &mut emulator,
        "the worker transcript",
        |screen| screen.contents().contains("worker transcript"),
    )
    .await;
    assert!(emulator.screen().alternate_screen());
    // A mouse message crosses the forwarder as the worker reads it: the
    // click opens the first turn, and a resize after it draws the screen
    // again, with the bottom row on the new last row.
    for kind in ["wheel_up", "wheel_down", "click"] {
        socket
            .send(send(json!({"type":"mouse","kind":kind,"col":2,"row":1})))
            .await
            .unwrap();
    }
    socket
        .send(send(json!({"type":"resize","cols":60,"rows":20})))
        .await
        .unwrap();
    emulator.screen_mut().set_size(size(20), size(60));
    draw_until(
        &mut socket,
        &mut emulator,
        "the clicked turn open, redrawn at 60 columns",
        |screen| {
            let contents = screen.contents();
            contents.contains("▾")
                && contents
                    .lines()
                    .nth(19)
                    .is_some_and(|row| row.ends_with("? keys"))
        },
    )
    .await;
    socket.close(None).await.unwrap();
}

/// A worker session serves its trajectory through the source's forwarder:
/// the snapshot, a page of its steps, a call's detail, a blob, and the
/// stream, each naming the session by its alias.
#[tokio::test]
async fn source_trajectory_routes_reach_the_worker_session() {
    let (p, goal) = setup().await;
    let source = draft(&p, &goal, "Trajectory", &[]).await;
    let worker_task = p.far.store.get_task(&source.id).await.unwrap();
    let worker_goal = p.far.store.task_goal(&worker_task).await.unwrap();
    let session = p
        .far
        .session(
            &worker_goal,
            Some(&worker_task),
            ariadne_core::Seat::Agent,
            &source.agents[0].id,
        )
        .await;
    let prompt = p
        .far
        .store
        .create_event(ariadne_store::NewAgentEvent {
            session_id: Some(session.id.clone()),
            task_id: Some(source.id.clone()),
            kind: "user_prompt_submit".into(),
            payload: json!({"text": "Report", "source": "daemon"}),
        })
        .await
        .unwrap();
    for kind in ["agent_message", "stop"] {
        p.far
            .store
            .create_event(ariadne_store::NewAgentEvent {
                session_id: Some(session.id.clone()),
                task_id: Some(source.id.clone()),
                kind: kind.into(),
                payload: json!({"text": "worker answer", "stop_reason": "end_turn"}),
            })
            .await
            .unwrap();
    }
    // Raw data that looks like a worker blob URL and names the worker
    // session is the agent's own, and passes as it is.
    let raw_input = json!({
        "url": format!("/v1/sessions/{}/trajectory/blobs/e/0", session.id),
        "blob": format!("/v1/sessions/{}/trajectory/blobs/e/0", session.id),
        "session_id": session.id,
    });
    let call = p
        .far
        .store
        .create_event(ariadne_store::NewAgentEvent {
            session_id: Some(session.id.clone()),
            task_id: Some(source.id.clone()),
            kind: "pre_tool_use".into(),
            payload: json!({"acp": {"toolCallId": "call-1", "title": "Shot", "kind": "other",
                "status": "completed", "rawInput": raw_input,
                "content": [{"type": "content", "content": {"type": "image",
                    "mimeType": "image/png", "data": "aGVsbG8="}}]}}),
        })
        .await
        .unwrap();
    let alias = format!("far~{}~{}", source.id, session.id);

    let snapshot: Value = p
        .local
        .get(&format!("/v1/sessions/{alias}/trajectory"))
        .await;
    let turn = snapshot["turns"]
        .as_array()
        .unwrap()
        .iter()
        .find(|turn| turn["id"] == prompt.id.as_str())
        .unwrap_or_else(|| panic!("the worker's turn: {snapshot}"));
    assert_eq!(turn["state"], "completed");
    assert_eq!(turn["answer"][0]["text"], "worker answer");
    let own: Value = p
        .far
        .get(&format!("/v1/sessions/{}/trajectory", session.id))
        .await;
    assert_eq!(own["session"]["host"], "local", "the worker's own word");
    assert_eq!(snapshot["session"]["host"], "far", "the host that runs it");
    let page: Value = p
        .local
        .get(&format!(
            "/v1/sessions/{alias}/trajectory/turns/{}/steps",
            prompt.id
        ))
        .await;
    assert_eq!(page["steps"][0]["content"][0]["text"], "worker answer");
    let detail: Value = p
        .local
        .get(&format!(
            "/v1/sessions/{alias}/trajectory/items/{}",
            call.id
        ))
        .await;
    assert!(detail.get("raw").is_none(), "no raw data unasked: {detail}");
    let refused: Value = p
        .local
        .get(&format!(
            "/v1/sessions/{alias}/trajectory/items/{}?raw=false",
            call.id
        ))
        .await;
    assert!(
        refused.get("raw").is_none(),
        "no raw data refused: {refused}"
    );
    let raw: Value = p
        .local
        .get(&format!(
            "/v1/sessions/{alias}/trajectory/items/{}?raw=true",
            call.id
        ))
        .await;
    assert_eq!(raw["raw"]["input"], raw_input, "raw data passes as it is");
    let blob = detail["detail"]["content"][0]["blob"].as_str().unwrap();
    assert_eq!(
        blob,
        format!("/v1/sessions/{alias}/trajectory/blobs/{}/0", call.id),
        "the blob URL names the alias"
    );
    let response = p.local.response(crate::common::get(blob)).await;
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "image/png");
    assert_eq!(
        response.headers()["content-security-policy"],
        "sandbox; default-src 'none'"
    );
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
    let bytes = http_body_util::BodyExt::collect(response.into_body())
        .await
        .unwrap()
        .to_bytes();
    assert_eq!(&bytes[..], b"hello");

    let source_address = Front::default().serve(&p.local).await;
    let client = ariadne_client::Client::tcp(format!("http://{source_address}"));
    let mut stream = client
        .stream(&format!("/v1/sessions/{alias}/trajectory/stream"))
        .await
        .unwrap();
    let opening = tokio::time::timeout(TIMEOUT, stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(opening.event, "snapshot");
    assert!(opening.data.contains("worker answer"));
    let opened: Value = serde_json::from_str(&opening.data).unwrap();
    assert_eq!(opened["session"]["host"], "far");
    p.far
        .store
        .set_session_pin(
            &session.id,
            &ariadne_store::AgentPin {
                model: format!(
                    "{}:switched",
                    ariadne_core::models::agent_of(&session.model)
                ),
                effort: Some("low".into()),
            },
        )
        .await
        .unwrap();
    let patched = loop {
        let frame = tokio::time::timeout(TIMEOUT, stream.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let update: Value = serde_json::from_str(&frame.data).unwrap();
        if let Some(op) = update["ops"]
            .as_array()
            .and_then(|ops| ops.iter().find(|op| op["op"] == "session_patch"))
        {
            break op.clone();
        }
    };
    assert_eq!(patched["session"]["host"], "far", "a patch names it too");
    assert_eq!(patched["session"]["model"], "switched");
    assert_eq!(patched["session"]["effort"], "low");
}

#[tokio::test]
async fn changing_the_host_endpoint_cannot_replace_the_recorded_execution_owner() {
    let (p, goal) = setup().await;
    let source = draft(&p, &goal, "Keep worker ownership", &[]).await;
    *p.front.protocol_override.lock().unwrap() =
        Some(json!({"protocol":"remote-tasks-v1","daemon_id":"another-daemon"}));
    p.local
        .error(
            patch_json(
                &format!("/v1/tasks/{}", source.id),
                json!({"title":"Wrong worker"}),
            ),
            StatusCode::CONFLICT,
        )
        .await;
    p.local
        .store
        .set_goal_status(&goal.id, GoalStatus::Planning)
        .await
        .unwrap();
    eventually(TIMEOUT, "owner mismatch is stale", async || {
        task(&p.local, &source.id).await.host_unreachable
    })
    .await;
    assert_eq!(
        task(&p.far, &source.id).await.title,
        "Keep worker ownership"
    );
    assert_eq!(task(&p.local, &source.id).await.id, source.id);
}

#[tokio::test]
async fn source_dependency_edits_need_no_worker_connection_or_local_staffing() {
    let (p, goal) = setup().await;
    let first = draft(&p, &goal, "First", &[]).await;
    let second = draft(&p, &goal, "Second", &[]).await;
    go_away(&p).await;
    let edited: TaskDto = p
        .local
        .json(
            patch_json(
                &format!("/v1/tasks/{}", second.id),
                json!({"depends_on":[first.id]}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(edited.depends_on, vec![first.id]);
    assert_eq!(edited.execution_context.unwrap().revision, 1);
    assert!(task(&p.far, &second.id).await.depends_on.is_empty());
    assert!(
        p.local
            .store
            .list_task_agents(&second.id)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn worker_session_creation_refreshes_source_progress_without_a_usage_report() {
    let (p, goal) = setup().await;
    let source = draft(&p, &goal, "Session progress", &[]).await;
    let worker_task = p.far.store.get_task(&source.id).await.unwrap();
    let worker_goal = p.far.store.task_goal(&worker_task).await.unwrap();
    let session = p
        .far
        .session(
            &worker_goal,
            Some(&worker_task),
            ariadne_core::Seat::Agent,
            &source.agents[0].id,
        )
        .await;
    let alias = format!("far~{}~{}", source.id, session.id);
    eventually(TIMEOUT, "source session reference", async || {
        task(&p.local, &source.id).await.agents[0]
            .session_id
            .as_deref()
            == Some(alias.as_str())
    })
    .await;
    assert_eq!(task(&p.local, &source.id).await.usage.total.input_tokens, 0);
}

#[tokio::test]
async fn reopening_the_source_store_recovers_an_unacknowledged_creation_without_a_second_worker_task()
 {
    let p = pair_setup(false).await;
    let repo = forge_repository(&p.local, "source", "owner/restart").await;
    forge_repository(&p.far, "worker", "owner/restart").await;
    let goal = goal_on(&p.local, &repo).await;
    p.front.lose_next("create remote task");
    let source = draft(&p, &goal, "Interrupted creation", &[]).await;
    assert!(
        p.local
            .store
            .source_task_reference(&source.id)
            .await
            .unwrap()
            .unwrap()
            .request
            .is_some()
    );
    let reopened = ariadne_store::Store::open(p.local.at("test.db"))
        .await
        .unwrap();
    assert_eq!(
        reopened.daemon_id().await.unwrap(),
        p.local.store.daemon_id().await.unwrap()
    );
    let mut restarted = p.local.state.clone();
    restarted.store = reopened;
    ariadne_daemon::far::start(&restarted, p.local.timeouts);
    eventually(TIMEOUT, "restarted creation acknowledged", async || {
        p.local
            .store
            .source_task_reference(&source.id)
            .await
            .unwrap()
            .unwrap()
            .request
            .is_none()
    })
    .await;
    let workers: Vec<TaskDto> = p.far.get(&format!("/v1/tasks?goal={}", goal.id)).await;
    assert_eq!(workers.len(), 1);
    assert_eq!(workers[0].id, source.id);
    assert!(!workers[0].execution_context.as_ref().unwrap().authorized);
}

#[tokio::test]
async fn an_endpoint_change_preserves_the_worker_identity_and_task_reference() {
    let (p, goal) = setup().await;
    let source = draft(&p, &goal, "Move endpoint", &[]).await;
    let endpoint = Front::default().serve(&p.far).await;
    p.local
        .json::<HostDto>(
            patch_json(
                "/v1/hosts/far",
                json!({"endpoint":format!("http://{endpoint}")}),
            ),
            StatusCode::OK,
        )
        .await;
    until_far_is(&p.local, HostStatus::Ready).await;
    let edited: TaskDto = p
        .local
        .json(
            patch_json(
                &format!("/v1/tasks/{}", source.id),
                json!({"title":"Same worker"}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(edited.id, source.id);
    assert_eq!(
        edited.execution_context.unwrap().execution_owner,
        source.execution_context.unwrap().execution_owner
    );
    assert_eq!(task(&p.far, &source.id).await.title, "Same worker");
}

#[tokio::test]
async fn goal_completion_includes_referenced_remote_work() {
    let (p, goal) = setup().await;
    let source = draft(&p, &goal, "Finish goal work", &[]).await;
    activate(&p.local, &goal).await;
    p.local
        .error(
            post_json(&format!("/v1/goals/{}/complete", goal.id), json!({})),
            StatusCode::CONFLICT,
        )
        .await;
    eventually(TIMEOUT, "worker ready", async || {
        task(&p.far, &source.id).await.status == TaskStatus::Ready
    })
    .await;
    p.far
        .store
        .transition_task(
            &source.id,
            TaskStatus::InProgress,
            Actor::Daemon,
            None,
            None,
        )
        .await
        .unwrap();
    p.far
        .store
        .transition_task(
            &source.id,
            TaskStatus::Finished,
            Actor::Agent,
            None,
            Some("abcdef123"),
        )
        .await
        .unwrap();
    eventually(TIMEOUT, "remote completion at source", async || {
        task(&p.local, &source.id).await.status == TaskStatus::Finished
    })
    .await;
    let completed: GoalDto = p
        .local
        .json(
            post_json(&format!("/v1/goals/{}/complete", goal.id), json!({})),
            StatusCode::OK,
        )
        .await;
    assert_eq!(completed.status, GoalStatus::Completed);
    assert!(p.far.store.list_goals(&[]).await.unwrap().is_empty());
}

#[tokio::test]
async fn source_cleanup_preserves_local_branches_that_share_a_worker_task_name() {
    let (p, goal) = setup().await;
    let source = draft(&p, &goal, "Worker branch", &[]).await;
    let path = std::path::Path::new(&goal.repos[0].path);
    sh(path, &format!("git branch {}", source.branch));
    p.local
        .launcher
        .cleanup_task(&source.id, true, true, false)
        .await
        .unwrap();
    sh(
        path,
        &format!("git rev-parse --verify refs/heads/{}", source.branch),
    );
    assert_eq!(task(&p.far, &source.id).await.status, TaskStatus::Pending);
}

#[tokio::test]
async fn a_worker_with_creation_only_v1_requires_a_dependency_control_upgrade() {
    let (p, goal) = setup().await;
    let protocol: Value = p.far.get("/v1/remote-tasks/protocol").await;
    *p.front.protocol_override.lock().unwrap() =
        Some(json!({"protocol":"remote-tasks-v1","daemon_id":protocol["daemon_id"]}));
    let reason = refusal(
        &p.local,
        &goal,
        task_body(
            &goal,
            "Needs source controls",
            &p.model,
            json!({"host":"far"}),
        ),
        StatusCode::CONFLICT,
    )
    .await;
    assert!(reason.contains("upgrade"));
    assert!(reason.contains("source-dependency-controls-v1"));
    assert!(
        p.local
            .store
            .list_tasks(Default::default())
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        p.far
            .store
            .list_tasks(Default::default())
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn a_goal_less_far_task_has_no_source_goal_and_accepts_cancel_and_retry() {
    let p = pair().await;
    let repo = forge_repository(&p.local, "source", "owner/solo").await;
    forge_repository(&p.far, "worker", "owner/solo").await;
    let task: TaskDto = p
        .local
        .json(
            post_json(
                "/v1/tasks",
                json!({
                    "title":"Independent remote work", "repo_id":repo.id, "host":"far",
                    "agents":[{"model":p.model}]
                }),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert!(task.goal_id.is_none());
    assert!(task.workflow.is_none());
    assert!(task.steps.is_empty());
    eventually(TIMEOUT, "worker authorization", async || {
        let worker: TaskDto = p.far.get(&format!("/v1/tasks/{}", task.id)).await;
        assert!(worker.goal_id.is_none());
        worker.execution_context.unwrap().authorized
    })
    .await;
    p.far
        .store
        .transition_task(&task.id, TaskStatus::InProgress, Actor::Daemon, None, None)
        .await
        .unwrap();
    p.far
        .store
        .transition_task(
            &task.id,
            TaskStatus::Failed,
            Actor::Daemon,
            Some("Worker failed"),
            None,
        )
        .await
        .unwrap();
    eventually(TIMEOUT, "source failure reason", async || {
        let t: TaskDto = p.local.get(&format!("/v1/tasks/{}", task.id)).await;
        t.status == TaskStatus::Failed && t.reason.as_deref() == Some("Worker failed")
    })
    .await;
    p.local
        .json::<TaskDto>(
            post(&format!("/v1/tasks/{}/retry", task.id)),
            StatusCode::OK,
        )
        .await;
    eventually(TIMEOUT, "remote retry", async || {
        let t: TaskDto = p.far.get(&format!("/v1/tasks/{}", task.id)).await;
        t.status == TaskStatus::Ready
    })
    .await;
    p.local
        .json::<TaskDto>(
            post(&format!("/v1/tasks/{}/cancel", task.id)),
            StatusCode::OK,
        )
        .await;
    eventually(TIMEOUT, "remote cancellation", async || {
        let t: TaskDto = p.local.get(&format!("/v1/tasks/{}", task.id)).await;
        t.status == TaskStatus::Cancelled
    })
    .await;
    assert!(p.far.store.list_goals(&[]).await.unwrap().is_empty());
}

#[tokio::test]
async fn a_goal_less_far_task_wakes_no_goal_reconciliation() {
    use tracing_subscriber::layer::SubscriberExt;

    let p = pair_on(true).await;
    let _guard = tracing::subscriber::set_default(
        tracing_subscriber::registry().with(p.local.telemetry.log_layer()),
    );
    let repo = forge_repository(&p.local, "source", "owner/solo").await;
    forge_repository(&p.far, "worker", "owner/solo").await;
    let task: TaskDto = p
        .local
        .json(
            post_json(
                "/v1/tasks",
                json!({
                    "title":"Independent remote work", "repo_id":repo.id, "host":"far",
                    "agents":[{"model":p.model}]
                }),
            ),
            StatusCode::CREATED,
        )
        .await;
    eventually(TIMEOUT, "worker authorization", async || {
        let worker: TaskDto = p.far.get(&format!("/v1/tasks/{}", task.id)).await;
        worker.execution_context.unwrap().authorized
    })
    .await;
    p.far
        .store
        .transition_task(&task.id, TaskStatus::InProgress, Actor::Daemon, None, None)
        .await
        .unwrap();
    eventually(TIMEOUT, "source progress", async || {
        let t: TaskDto = p.local.get(&format!("/v1/tasks/{}", task.id)).await;
        t.status == TaskStatus::InProgress
    })
    .await;
    p.local.telemetry.flush();
    let logs: ariadne_api::logs::LogSnapshotResponse = p.local.get("/v1/logs").await;
    assert!(
        !logs
            .lines
            .iter()
            .any(|line| line.message.contains("goal reconciliation failed")),
        "{:#?}",
        logs.lines
    );
}

#[tokio::test]
async fn goal_less_admission_checks_the_host_and_saves_its_workflow() {
    let p = pair().await;
    let repo = forge_repository(&p.local, "source", "owner/solo").await;
    forge_repository(&p.far, "worker", "owner/solo").await;
    p.local
        .store
        .create_workflow(ariadne_store::NewWorkflow {
            name: "independent".into(),
            document: "workflow independent\n  build[Build]\n    Build.\n    gate: committed\n"
                .into(),
        })
        .await
        .unwrap();
    let body = json!({"title":"Remote columns", "repo_id":repo.id,"workflow":"independent",
        "host":"far","agents":[{"step":"build","model":p.model}]});
    let task: TaskDto = p
        .local
        .json(post_json("/v1/tasks", body.clone()), StatusCode::CREATED)
        .await;
    let worker: TaskDto = p.far.get(&format!("/v1/tasks/{}", task.id)).await;
    assert!(worker.goal_id.is_none());
    assert!(worker.execution_context.unwrap().source_goal.is_none());
    assert_eq!(worker.workflow.as_deref(), Some("independent"));
    assert_eq!(worker.steps.len(), 1);
    assert_eq!(worker.steps[0].id, "build");
    assert_eq!(
        worker.steps[0].gate,
        Some(ariadne_core::workflow::StepGate::Committed)
    );
    for host in ["dead", "unknown"] {
        let mut refused = body.clone();
        refused["host"] = json!(host);
        let (status, bytes) = p.local.send(post_json("/v1/tasks", refused)).await;
        assert!(status.is_client_error());
        let reason: Value = serde_json::from_slice(&bytes).unwrap();
        assert!(reason["error"]["message"].as_str().unwrap().contains(host));
    }
}

#[tokio::test]
async fn far_goal_less_admission_uses_the_local_workflow_reasons() {
    let p = pair().await;
    let repo = forge_repository(&p.local, "source", "owner/admission").await;
    forge_repository(&p.far, "worker", "owner/admission").await;
    p.local
        .store
        .create_workflow(ariadne_store::NewWorkflow {
            name: "admission".into(),
            document:
                "workflow admission\n  build[Build]\n    Build.\n  inspect[Inspect]\n    Inspect.\n"
                    .into(),
        })
        .await
        .unwrap();
    for (workflow, agents, expected) in [
        (
            "pr-review",
            json!([{"step":"review","model":p.model}]),
            "workflow pr-review runs a pull request operation; a goal cannot use it",
        ),
        (
            "pr-babysit",
            json!([{"step":"babysit","model":p.model}]),
            "workflow pr-babysit runs a pull request operation; a goal cannot use it",
        ),
        (
            "admission",
            json!([{"step":"build","model":p.model}]),
            "the goal is active, so every column needs an agent; none staffs inspect",
        ),
        (
            "admission",
            json!([{"step":"unknown","model":p.model}]),
            "the agent names an unknown column unknown; the columns are build, inspect",
        ),
    ] {
        for host in ["local", "far"] {
            let reason: Value = p
                .local
                .json(
                    post_json(
                        "/v1/tasks",
                        json!({
                            "title":"Admission", "repo_id":repo.id,"workflow":workflow,
                            "host":host,"agents":agents
                        }),
                    ),
                    StatusCode::BAD_REQUEST,
                )
                .await;
            assert_eq!(
                reason["error"]["message"],
                format!("invalid input: {expected}")
            );
        }
    }
}
