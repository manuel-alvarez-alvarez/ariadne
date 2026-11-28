//! Reconstruct ACP request ownership at the durable store boundary.
use ariadne_daemon::acp_recovery::RecoveryHistory;
use ariadne_store::{NewSession, Store};
use serde_json::json;

#[tokio::test]
async fn recovery_preserves_an_unanswered_turn_and_a_sent_permission_answer() {
    let home = tempfile::tempdir().unwrap();
    let store = Store::open(home.path().join("store.db")).await.unwrap();
    let session = store
        .create_session(NewSession {
            goal_id: None,
            task_id: None,
            seat: None,
            task_agent_id: None,
            model: "stub:model".into(),
            effort: None,
            worktree_path: None,
            pull_request_id: None,
        })
        .await
        .unwrap();
    let launch = "launch";
    for message in [
        json!({"id":1,"method":"initialize","params":{"protocolVersion":1}}),
        json!({"id":2,"method":"session/new","params":{"cwd":"/work"}}),
        json!({"id":3,"method":"session/prompt","params":{"sessionId":"conversation","prompt":[{"type":"text","text":"continue"}]}}),
        json!({"id":"permission","result":{"outcome":{"outcome":"selected","optionId":"yes"}}}),
    ] {
        store
            .enqueue_relay_input(&session.id, launch, &message.to_string())
            .await
            .unwrap();
    }
    for (seq, message) in [
        (1, json!({"id":1,"result":{"protocolVersion":1}})),
        (
            2,
            json!({"id":2,"result":{"sessionId":"conversation","configOptions":[]}}),
        ),
        (
            3,
            json!({"method":"session/update","params":{"sessionId":"conversation","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"kept"}}}}),
        ),
        (
            4,
            json!({"id":"permission","method":"session/request_permission","params":{"sessionId":"conversation"}}),
        ),
    ] {
        store
            .save_relay_output(
                &session.id,
                launch,
                seq,
                &json!({"type":"message","seq":seq,"stream":"stdout","data":message.to_string()})
                    .to_string(),
            )
            .await
            .unwrap();
    }
    store
        .mark_relay_output_processed(&session.id, launch, 1)
        .await
        .unwrap();
    store
        .mark_relay_output_processed(&session.id, launch, 2)
        .await
        .unwrap();
    let history = RecoveryHistory::load(&store, &session.id, launch)
        .await
        .unwrap();
    let prompt = history
        .pending_prompt()
        .expect("restore the active request");
    assert_eq!(prompt.message["id"], 3);
    assert_eq!(prompt.input_seq, 3);
    assert_eq!(history.last_input_seq, 4);
    assert_eq!(
        history.requests[1].response.as_ref().unwrap()["result"]["sessionId"],
        "conversation"
    );
    assert!(history.requests[1].response_processed);
    assert_eq!(
        history.answered_permissions["\"permission\""]["outcome"]["optionId"],
        "yes"
    );
}

#[tokio::test]
async fn a_received_prompt_response_stays_pending_until_its_effects_are_stored() {
    let home = tempfile::tempdir().unwrap();
    let store = Store::open(home.path().join("store.db")).await.unwrap();
    let session = store
        .create_session(NewSession {
            goal_id: None,
            task_id: None,
            seat: None,
            task_agent_id: None,
            model: "stub:model".into(),
            effort: None,
            worktree_path: None,
            pull_request_id: None,
        })
        .await
        .unwrap();
    store.enqueue_relay_input(&session.id, "launch", &json!({"id":7,"method":"session/prompt","params":{"sessionId":"conversation","prompt":[]}}).to_string()).await.unwrap();
    store.save_relay_output(&session.id, "launch", 1, &json!({"type":"message","seq":1,"stream":"stdout",
        "data":json!({"id":7,"result":{"stopReason":"end_turn","usage":{"inputTokens":12,"outputTokens":3}}}).to_string()}).to_string()).await.unwrap();
    let history = RecoveryHistory::load(&store, &session.id, "launch")
        .await
        .unwrap();
    let pending = history
        .pending_prompt()
        .expect("usage still needs runtime processing");
    assert_eq!(
        pending.response.as_ref().unwrap()["result"]["usage"]["inputTokens"],
        12
    );
    store
        .mark_relay_output_processed(&session.id, "launch", 1)
        .await
        .unwrap();
    let history = RecoveryHistory::load(&store, &session.id, "launch")
        .await
        .unwrap();
    assert!(history.pending_prompt().is_none());
}

#[tokio::test]
async fn replaying_one_relay_effect_keeps_one_event_and_one_usage_record() {
    use ariadne_api::events::IngestEventRequest;
    use ariadne_daemon::http::ingest_relay_event;
    let home = tempfile::tempdir().unwrap();
    let store = Store::open(home.path().join("store.db")).await.unwrap();
    let session = store
        .create_session(NewSession {
            goal_id: None,
            task_id: None,
            seat: None,
            task_agent_id: None,
            model: "stub:model".into(),
            effort: None,
            worktree_path: None,
            pull_request_id: None,
        })
        .await
        .unwrap();
    let request = IngestEventRequest {
        session_id: session.id.clone(),
        launch: Some("launch".into()),
        kind: "stop".into(),
        payload: json!({"session_id":"conversation", "stop_reason":"end_turn",
            "ariadne_usage":{"source":"launch","input_tokens":12,"cached_input_tokens":4,"output_tokens":3}}),
    };
    let first = ingest_relay_event(&store, &request, "output:9", 0)
        .await
        .unwrap()
        .unwrap();
    let replayed = ingest_relay_event(&store, &request, "output:9", 0)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        store.list_session_events(&session.id).await.unwrap().len(),
        1
    );
    assert_eq!(first.id, replayed.id);
    let usage = store.session_usage(&session.id).await.unwrap();
    assert_eq!(
        (
            usage.input_tokens,
            usage.cached_input_tokens,
            usage.output_tokens
        ),
        (12, 4, 3)
    );
}

#[tokio::test]
async fn a_replayed_relay_effect_does_not_move_the_session_status_again() {
    use ariadne_api::events::IngestEventRequest;
    use ariadne_core::SessionStatus;
    use ariadne_daemon::http::ingest_relay_event;
    let home = tempfile::tempdir().unwrap();
    let store = Store::open(home.path().join("store.db")).await.unwrap();
    let session = store
        .create_session(NewSession {
            goal_id: None,
            task_id: None,
            seat: None,
            task_agent_id: None,
            model: "stub:model".into(),
            effort: None,
            worktree_path: None,
            pull_request_id: None,
        })
        .await
        .unwrap();
    let event = |kind: &str| IngestEventRequest {
        session_id: session.id.clone(),
        launch: Some("launch".into()),
        kind: kind.into(),
        payload: json!({"session_id": "conversation"}),
    };
    ingest_relay_event(&store, &event("session_start"), "setup", 1)
        .await
        .unwrap();
    ingest_relay_event(&store, &event("stop"), "output:9", 0)
        .await
        .unwrap();
    let idle = store.get_session(&session.id).await.unwrap();
    assert_eq!(idle.status(), SessionStatus::Idle);

    // A reconnect replays the launch's setup.
    ingest_relay_event(&store, &event("session_start"), "setup", 1)
        .await
        .unwrap();
    assert_eq!(
        store.get_session(&session.id).await.unwrap().status(),
        SessionStatus::Idle
    );
}

#[tokio::test]
async fn a_replayed_stop_left_half_applied_still_moves_the_session_to_idle() {
    use ariadne_api::events::IngestEventRequest;
    use ariadne_core::SessionStatus;
    use ariadne_daemon::http::ingest_relay_event;
    use ariadne_store::NewAgentEvent;
    let home = tempfile::tempdir().unwrap();
    let store = Store::open(home.path().join("store.db")).await.unwrap();
    let session = store
        .create_session(NewSession {
            goal_id: None,
            task_id: None,
            seat: None,
            task_agent_id: None,
            model: "stub:model".into(),
            effort: None,
            worktree_path: None,
            pull_request_id: None,
        })
        .await
        .unwrap();
    let event = |kind: &str| IngestEventRequest {
        session_id: session.id.clone(),
        launch: Some("launch".into()),
        kind: kind.into(),
        payload: json!({"session_id": "conversation"}),
    };
    ingest_relay_event(&store, &event("session_start"), "setup", 1)
        .await
        .unwrap();
    assert_eq!(
        store.get_session(&session.id).await.unwrap().status(),
        SessionStatus::Running
    );

    // The daemon stored the stop and went down before it moved the row.
    store
        .create_event_once(
            NewAgentEvent {
                session_id: Some(session.id.clone()),
                task_id: None,
                kind: "stop".into(),
                payload: json!({"session_id": "conversation"}),
            },
            &format!("{}:launch:output:9", session.id),
        )
        .await
        .unwrap();
    ingest_relay_event(&store, &event("stop"), "output:9", 0)
        .await
        .unwrap();
    assert_eq!(
        store.get_session(&session.id).await.unwrap().status(),
        SessionStatus::Idle
    );
}
