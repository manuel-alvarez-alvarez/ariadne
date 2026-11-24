//! Worker tasks retain source context without worker goals.
use crate::common::{
    TIMEOUT, as_session, eventually_some, get, harness, patch_json, post_json, put_json, test_pin,
    with_forge_enabled,
};
use axum::http::StatusCode;
use serde_json::{Value, json};

const TASK: &str = "01arz3ndektsv4rrffq69g5fat";
const GOAL: &str = "01arz3ndektsv4rrffq69g5fav";

fn draft(repo: &str) -> Value {
    json!({"repo_id": repo, "title": "Remote work", "description": "Keep source context",
        "source_goal": {"daemon_id": "source-daemon", "goal_id": GOAL, "title": "Source goal", "description": "Source instructions"},
        "workflow": {"name": "saved-workflow", "steps": [
            {"id":"develop", "title":"Develop", "description":"Build", "skills":["coding"], "ranks":[], "gate":null}]},
        "agents": [{"step":"develop", "model":test_pin().model}]})
}

#[tokio::test]
async fn a_remote_draft_has_context_without_a_worker_goal() {
    let h = harness().await;
    let repo = h.repository(&h.at("repo")).await;
    let protocol: Value = h.get("/v1/remote-tasks/protocol").await;
    assert_eq!(protocol["protocol"], "remote-tasks-v1");
    let path = format!("/v1/remote-tasks/{TASK}");
    let task: Value = h
        .json(put_json(&path, draft(&repo.id)), StatusCode::CREATED)
        .await;
    assert_eq!(task["id"], TASK);
    assert_eq!(task["goal_id"], GOAL);
    assert_eq!(
        task["execution_context"]["source_goal"]["title"],
        "Source goal"
    );
    assert_eq!(
        task["execution_context"]["execution_owner"],
        protocol["daemon_id"]
    );
    assert_eq!(task["execution_context"]["authorized"], false);
    let repeated: Value = h
        .json(put_json(&path, draft(&repo.id)), StatusCode::OK)
        .await;
    assert_eq!(task, repeated);
    h.json::<Value>(get(&format!("/v1/goals/{GOAL}")), StatusCode::NOT_FOUND)
        .await;
    assert!(h.store.list_goals(&[]).await.unwrap().is_empty());
    let reopened = ariadne_store::Store::open(h.at("test.db")).await.unwrap();
    assert_eq!(reopened.daemon_id().await.unwrap(), protocol["daemon_id"]);
    assert!(
        reopened
            .remote_task_context(TASK)
            .await
            .unwrap()
            .unwrap()
            .authorized_revision
            .is_none()
    );
    assert!(reopened.list_goals(&[]).await.unwrap().is_empty());
    let read: Value = h.get(&format!("/v1/tasks/{TASK}")).await;
    assert_eq!(read["execution_context"]["authorized"], false);
}

#[tokio::test]
async fn authorization_starts_one_saved_workflow_without_a_source_connection() {
    let h = harness().scheduler().await;
    let repo = h.repository(&h.git_repo("repo")).await;
    with_forge_enabled(&h, &repo, false).await;
    let path = format!("/v1/remote-tasks/{TASK}");
    h.json::<Value>(put_json(&path, draft(&repo.id)), StatusCode::CREATED)
        .await;
    h.flush_scheduler().await;
    let blocked: Value = h.get(&format!("/v1/tasks/{TASK}")).await;
    assert_eq!(blocked["status"], "pending");
    assert!(blocked["worktree_path"].is_null());
    let auth = format!("{path}/authorize");
    h.json::<Value>(post_json(&auth, json!({"revision":1})), StatusCode::OK)
        .await;
    h.json::<Value>(post_json(&auth, json!({"revision":1})), StatusCode::OK)
        .await;
    let running = eventually_some(TIMEOUT, "remote agent session", async || {
        let task: Value = h.get(&format!("/v1/tasks/{TASK}")).await;
        assert_ne!(task["status"], "failed", "{task}");
        task["agents"][0]["session_id"]
            .as_str()
            .map(|_| task.clone())
    })
    .await;
    assert_eq!(running["step"], "develop");
    let session = running["agents"][0]["session_id"].as_str().unwrap();
    let observed: Value = h.get(&format!("/v1/sessions/{session}")).await;
    assert_eq!(observed["task_id"], TASK);
    assert_eq!(observed["goal_id"], GOAL);
    h.json::<Value>(as_session(&format!("/v1/tasks/{TASK}/messages"), session,
        json!({"to_actor":"orchestrator", "body":"Source is unavailable; retain this question."})), StatusCode::CREATED).await;
    h.flush_scheduler().await;
    let messages: Vec<Value> = h.get(&format!("/v1/tasks/{TASK}/messages")).await;
    assert_eq!(messages.len(), 1);
    assert!(messages[0]["delivered_at"].is_null());
    for _ in 0..2 {
        h.json::<Value>(post_json(&format!("/v1/tasks/{TASK}/relay-message"), json!({
            "id":"source-message", "origin_task_id":TASK, "column":"develop", "body":"The source reconnected."
        })), StatusCode::OK).await;
    }
    let reconnected: Vec<Value> = h.get(&format!("/v1/tasks/{TASK}/messages")).await;
    assert_eq!(reconnected.len(), 2);
    h.store
        .upsert_session_usage(
            session,
            "remote-transcript",
            ariadne_core::TokenUsage {
                input_tokens: 12,
                cached_input_tokens: 3,
                output_tokens: 4,
            },
        )
        .await
        .unwrap();
    let cost: Value = h.get(&format!("/v1/tasks/{TASK}")).await;
    assert_eq!(cost["usage"]["total"]["input_tokens"], 12);
    let (status, diff) = h.send(get(&format!("/v1/tasks/{TASK}/diff"))).await;
    assert_eq!(status, StatusCode::OK);
    assert!(diff.is_empty());
    h.json::<Value>(
        as_session(
            &format!("/v1/tasks/{TASK}/step/complete"),
            session,
            json!({"reason":"The saved column is complete."}),
        ),
        StatusCode::OK,
    )
    .await;
    let ended: Value = h.get(&format!("/v1/tasks/{TASK}")).await;
    assert_eq!(ended["status"], "finished");
    assert!(h.store.list_goals(&[]).await.unwrap().is_empty());
    let history: Vec<Value> = h.get(&format!("/v1/tasks/{TASK}/transitions")).await;
    assert_eq!(
        history
            .iter()
            .filter(|t| t["to_status"] == "in_progress")
            .count(),
        1
    );
}

#[tokio::test]
async fn draft_edits_invalidate_old_authorization_and_cancellation_is_final() {
    let h = harness().await;
    let repo = h.repository(&h.at("repo")).await;
    let path = format!("/v1/remote-tasks/{TASK}");
    h.json::<Value>(put_json(&path, draft(&repo.id)), StatusCode::CREATED)
        .await;
    h.json::<Value>(
        patch_json(
            &format!("/v1/tasks/{TASK}"),
            json!({"title":"Updated draft"}),
        ),
        StatusCode::OK,
    )
    .await;
    h.json::<Value>(
        post_json(&format!("{path}/authorize"), json!({"revision":1})),
        StatusCode::CONFLICT,
    )
    .await;
    h.json::<Value>(
        post_json(
            &format!("/v1/tasks/{TASK}/transitions"),
            json!({"to":"cancelled"}),
        ),
        StatusCode::OK,
    )
    .await;
    h.json::<Value>(
        post_json(&format!("{path}/authorize"), json!({"revision":2})),
        StatusCode::CONFLICT,
    )
    .await;
}

#[tokio::test]
async fn simultaneous_creation_and_authorization_keep_one_task_and_one_start() {
    let h = harness().await;
    let repo = h.repository(&h.at("repo")).await;
    let path = format!("/v1/remote-tasks/{TASK}");
    let (first, second) = tokio::join!(
        h.send(put_json(&path, draft(&repo.id))),
        h.send(put_json(&path, draft(&repo.id)))
    );
    let mut statuses = [first.0.as_u16(), second.0.as_u16()];
    statuses.sort();
    assert_eq!(statuses, [200, 201]);
    let auth = format!("{path}/authorize");
    let (first, second) = tokio::join!(
        h.send(post_json(&auth, json!({"revision":1}))),
        h.send(post_json(&auth, json!({"revision":1})))
    );
    assert_eq!(first.0, StatusCode::OK);
    assert_eq!(second.0, StatusCode::OK);
    let history: Vec<Value> = h.get(&format!("/v1/tasks/{TASK}/transitions")).await;
    assert_eq!(history.len(), 1);
    assert_eq!(h.store.remote_tasks().await.unwrap().len(), 1);
    let mut conflicting = draft(&repo.id);
    conflicting["title"] = json!("Another task");
    h.json::<Value>(put_json(&path, conflicting), StatusCode::CONFLICT)
        .await;
}

#[tokio::test]
async fn an_incomplete_draft_can_be_staffed_and_authorized_after_restart() {
    let h = harness().await;
    let repo = h.repository(&h.at("repo")).await;
    let mut body = draft(&repo.id);
    body["agents"] = json!([]);
    let path = format!("/v1/remote-tasks/{TASK}");
    h.json::<Value>(put_json(&path, body.clone()), StatusCode::CREATED)
        .await;
    h.json::<Value>(
        post_json(&format!("{path}/authorize"), json!({"revision":1})),
        StatusCode::BAD_REQUEST,
    )
    .await;
    h.json::<Value>(
        patch_json(
            &format!("/v1/tasks/{TASK}"),
            json!({"agents":draft(&repo.id)["agents"]}),
        ),
        StatusCode::OK,
    )
    .await;
    let repeated: Value = h.json(put_json(&path, body), StatusCode::OK).await;
    assert_eq!(repeated["agents"].as_array().unwrap().len(), 1);
    assert_eq!(repeated["execution_context"]["revision"], 2);
    let reopened = ariadne_store::Store::open(h.at("test.db")).await.unwrap();
    assert_eq!(
        reopened
            .authorize_remote_task(TASK, 2)
            .await
            .unwrap()
            .status,
        "ready"
    );
    let again = ariadne_store::Store::open(h.at("test.db")).await.unwrap();
    assert_eq!(
        again
            .remote_task_context(TASK)
            .await
            .unwrap()
            .unwrap()
            .authorized_revision,
        Some(2)
    );
}

#[tokio::test]
async fn saved_columns_keep_their_gates_and_a_failed_task_can_retry() {
    let h = harness().scheduler().await;
    let repo = h.repository(&h.git_repo("repo")).await;
    with_forge_enabled(&h, &repo, false).await;
    let mut body = draft(&repo.id);
    body["workflow"]["steps"][0]["gate"] = json!("committed");
    body["workflow"]["steps"].as_array_mut().unwrap().push(json!({
        "id":"inspect", "title":"Inspect", "description":"Inspect the saved change", "skills":["code-review"], "ranks":[], "gate":null}));
    body["agents"]
        .as_array_mut()
        .unwrap()
        .push(json!({"step":"inspect", "model":test_pin().model}));
    let path = format!("/v1/remote-tasks/{TASK}");
    h.json::<Value>(put_json(&path, body), StatusCode::CREATED)
        .await;
    h.json::<Value>(
        post_json(&format!("{path}/authorize"), json!({"revision":1})),
        StatusCode::OK,
    )
    .await;
    let running = eventually_some(TIMEOUT, "first saved column", async || {
        let task: Value = h.get(&format!("/v1/tasks/{TASK}")).await;
        task["agents"][0]["session_id"]
            .as_str()
            .map(|_| task.clone())
    })
    .await;
    let session = running["agents"][0]["session_id"].as_str().unwrap();
    let complete = format!("/v1/tasks/{TASK}/step/complete");
    h.json::<Value>(
        as_session(
            &complete,
            session,
            json!({"reason":"Check the commit gate."}),
        ),
        StatusCode::CONFLICT,
    )
    .await;
    let worktree = std::path::Path::new(running["worktree_path"].as_str().unwrap());
    crate::common::sh(worktree, "echo change > remote.txt");
    crate::common::sh(worktree, "git add remote.txt");
    crate::common::sh(
        worktree,
        "git -c user.name=Test -c user.email=test@test commit -m 'feat(core): add the remote change'",
    );
    h.json::<Value>(
        as_session(
            &complete,
            session,
            json!({"reason":"The change is committed."}),
        ),
        StatusCode::OK,
    )
    .await;
    let inspect = eventually_some(TIMEOUT, "second saved column", async || {
        let task: Value = h.get(&format!("/v1/tasks/{TASK}")).await;
        task["agents"][1]["session_id"]
            .as_str()
            .map(|_| task.clone())
    })
    .await;
    assert_eq!(inspect["step"], "inspect");
    h.json::<Value>(
        as_session(
            &format!("/v1/tasks/{TASK}/step/fail"),
            inspect["agents"][1]["session_id"].as_str().unwrap(),
            json!({"reason":"Revise the change."}),
        ),
        StatusCode::OK,
    )
    .await;
    let returned: Value = h.get(&format!("/v1/tasks/{TASK}")).await;
    assert_eq!(returned["step"], "develop");
    h.json::<Value>(
        as_session(
            &format!("/v1/tasks/{TASK}/transitions"),
            session,
            json!({"to":"failed", "reason":"A retry is required."}),
        ),
        StatusCode::OK,
    )
    .await;
    h.json::<Value>(
        post_json(&format!("/v1/tasks/{TASK}/retry"), json!({})),
        StatusCode::OK,
    )
    .await;
    let retried: Value = h.get(&format!("/v1/tasks/{TASK}")).await;
    assert!(
        retried["execution_context"]["authorized"]
            .as_bool()
            .unwrap()
    );
}

#[tokio::test]
async fn the_remote_protocol_and_context_are_in_the_published_schema() {
    let h = harness().await;
    let doc: Value = h.get("/api-docs/openapi.json").await;
    assert!(doc["paths"]["/v1/remote-tasks/protocol"]["get"].is_object());
    assert!(doc["paths"]["/v1/remote-tasks/{id}"]["put"].is_object());
    assert!(doc["paths"]["/v1/remote-tasks/{id}/authorize"]["post"].is_object());
    assert!(doc["components"]["schemas"]["TaskDto"]["properties"]["execution_context"].is_object());
    let snapshot: Value =
        serde_json::from_str(include_str!("../../../../ui/openapi.json")).unwrap();
    for path in [
        "/v1/remote-tasks/protocol",
        "/v1/remote-tasks/{id}",
        "/v1/remote-tasks/{id}/authorize",
        "/v1/remote-tasks/{id}/dependency-failure",
        "/v1/remote-tasks/{id}/retry-dependency",
    ] {
        assert_eq!(doc["paths"][path], snapshot["paths"][path]);
    }
    for name in [
        "TaskDto",
        "CreateRemoteTaskRequest",
        "AuthorizeRemoteTaskRequest",
        "RemoteTaskProtocol",
        "FailRemoteDependencyRequest",
        "RetryRemoteDependencyRequest",
        "SourceGoalReference",
        "TaskExecutionContext",
        "TaskWorkflowContext",
    ] {
        assert_eq!(
            doc["components"]["schemas"][name],
            snapshot["components"]["schemas"][name]
        );
    }
}

#[tokio::test]
async fn source_dependency_controls_are_audited_repeatable_and_fenced_by_draft_revision() {
    let h = harness().await;
    let repo = h.repository(&h.at("repo")).await;
    let path = format!("/v1/remote-tasks/{TASK}");
    h.json::<Value>(put_json(&path, draft(&repo.id)), StatusCode::CREATED)
        .await;
    let failure =
        json!({"revision":1,"operation_id":"dependency-attempt-1","reason":"dependency failed"});
    let failed: Value = h
        .json(
            post_json(&format!("{path}/dependency-failure"), failure.clone()),
            StatusCode::OK,
        )
        .await;
    assert_eq!(failed["status"], "failed");
    assert_eq!(failed["reason"], "dependency failed");
    assert_eq!(failed["execution_context"]["revision"], 2);
    h.json::<Value>(
        post_json(&format!("{path}/dependency-failure"), failure.clone()),
        StatusCode::OK,
    )
    .await;
    let history: Vec<Value> = h.get(&format!("/v1/tasks/{TASK}/transitions")).await;
    assert_eq!(history.len(), 1);
    h.json::<Value>(
        post_json(&format!("{path}/authorize"), json!({"revision":1})),
        StatusCode::CONFLICT,
    )
    .await;
    let retry = json!({"revision":2,"operation_id":"dependency-attempt-1"});
    let pending: Value = h
        .json(
            post_json(&format!("{path}/retry-dependency"), retry.clone()),
            StatusCode::OK,
        )
        .await;
    assert_eq!(pending["status"], "pending");
    assert_eq!(pending["execution_context"]["revision"], 3);
    assert_eq!(pending["execution_context"]["authorized"], false);
    h.json::<Value>(
        post_json(&format!("{path}/retry-dependency"), retry.clone()),
        StatusCode::OK,
    )
    .await;
    h.json::<Value>(
        post_json(
            &format!("{path}/dependency-failure"),
            json!({"revision":1,"operation_id":"stale-operation","reason":"stale"}),
        ),
        StatusCode::CONFLICT,
    )
    .await;
    h.json::<Value>(
        post_json(&format!("{path}/authorize"), json!({"revision":3})),
        StatusCode::OK,
    )
    .await;
    h.json::<Value>(
        post_json(&format!("{path}/dependency-failure"), failure),
        StatusCode::OK,
    )
    .await;
    h.json::<Value>(
        post_json(&format!("{path}/retry-dependency"), retry),
        StatusCode::OK,
    )
    .await;
    h.json::<Value>(
        post_json(
            &format!("{path}/dependency-failure"),
            json!({"revision":3,"operation_id":"after-authorization","reason":"late"}),
        ),
        StatusCode::CONFLICT,
    )
    .await;
    let history: Vec<Value> = h.get(&format!("/v1/tasks/{TASK}/transitions")).await;
    assert_eq!(history.len(), 3);
    assert_eq!(history[1]["from_status"], "failed");
    assert_eq!(history[1]["to_status"], "pending");
}

#[tokio::test]
async fn dependency_controls_keep_agent_restrictions_and_generic_user_permissions() {
    let h = harness().await;
    let repo = h.repository(&h.at("repo")).await;
    let path = format!("/v1/remote-tasks/{TASK}");
    h.json::<Value>(put_json(&path, draft(&repo.id)), StatusCode::CREATED)
        .await;
    let failure = json!({"revision":1,"operation_id":"failure","reason":"dependency failed"});
    h.json::<Value>(
        as_session(
            &format!("{path}/dependency-failure"),
            "agent-session",
            failure.clone(),
        ),
        StatusCode::FORBIDDEN,
    )
    .await;
    h.json::<Value>(
        post_json(
            &format!("/v1/tasks/{TASK}/transitions"),
            json!({"to":"failed"}),
        ),
        StatusCode::CONFLICT,
    )
    .await;
    h.json::<Value>(
        post_json(&format!("{path}/dependency-failure"), failure),
        StatusCode::OK,
    )
    .await;
    h.json::<Value>(
        post_json(&format!("/v1/tasks/{TASK}/retry"), json!({})),
        StatusCode::CONFLICT,
    )
    .await;
    h.json::<Value>(
        as_session(
            &format!("{path}/retry-dependency"),
            "agent-session",
            json!({"revision":2,"operation_id":"failure"}),
        ),
        StatusCode::FORBIDDEN,
    )
    .await;
}

#[tokio::test]
async fn failure_racing_authorization_admits_exactly_one_draft_outcome() {
    let h = harness().await;
    let repo = h.repository(&h.at("repo")).await;
    let path = format!("/v1/remote-tasks/{TASK}");
    h.json::<Value>(put_json(&path, draft(&repo.id)), StatusCode::CREATED)
        .await;
    let (failure, authorization) = tokio::join!(
        h.send(post_json(
            &format!("{path}/dependency-failure"),
            json!({"revision":1,"operation_id":"failure","reason":"dependency failed"})
        )),
        h.send(post_json(
            &format!("{path}/authorize"),
            json!({"revision":1})
        ))
    );
    let mut statuses = [failure.0.as_u16(), authorization.0.as_u16()];
    statuses.sort();
    assert_eq!(statuses, [200, 409]);
    let history: Vec<Value> = h.get(&format!("/v1/tasks/{TASK}/transitions")).await;
    assert_eq!(history.len(), 1);
}

#[tokio::test]
async fn old_dependency_operation_replays_cannot_reset_a_newer_failure() {
    let h = harness().await;
    let repo = h.repository(&h.at("repo")).await;
    let path = format!("/v1/remote-tasks/{TASK}");
    h.json::<Value>(put_json(&path, draft(&repo.id)), StatusCode::CREATED)
        .await;
    let first = json!({"revision":1,"operation_id":"first","reason":"first dependency failed"});
    let retry = json!({"revision":2,"operation_id":"first"});
    h.json::<Value>(
        post_json(&format!("{path}/dependency-failure"), first.clone()),
        StatusCode::OK,
    )
    .await;
    h.json::<Value>(
        post_json(&format!("{path}/retry-dependency"), retry.clone()),
        StatusCode::OK,
    )
    .await;
    h.json::<Value>(
        post_json(
            &format!("{path}/dependency-failure"),
            json!({"revision":3,"operation_id":"second","reason":"second dependency failed"}),
        ),
        StatusCode::OK,
    )
    .await;
    h.json::<Value>(
        post_json(&format!("{path}/dependency-failure"), first),
        StatusCode::OK,
    )
    .await;
    let replayed: Value = h
        .json(
            post_json(&format!("{path}/retry-dependency"), retry),
            StatusCode::OK,
        )
        .await;
    assert_eq!(replayed["status"], "failed");
    assert_eq!(replayed["reason"], "second dependency failed");
    assert_eq!(replayed["execution_context"]["revision"], 4);
    let history: Vec<Value> = h.get(&format!("/v1/tasks/{TASK}/transitions")).await;
    assert_eq!(history.len(), 3);
}

#[tokio::test]
async fn a_dependency_retry_remains_blocked_without_new_source_authorization() {
    let h = harness().scheduler().await;
    let repo = h.repository(&h.git_repo("repo")).await;
    with_forge_enabled(&h, &repo, false).await;
    let path = format!("/v1/remote-tasks/{TASK}");
    h.json::<Value>(put_json(&path, draft(&repo.id)), StatusCode::CREATED)
        .await;
    h.json::<Value>(
        post_json(
            &format!("{path}/dependency-failure"),
            json!({"revision":1,"operation_id":"failure","reason":"dependency failed"}),
        ),
        StatusCode::OK,
    )
    .await;
    h.json::<Value>(
        post_json(
            &format!("{path}/retry-dependency"),
            json!({"revision":2,"operation_id":"failure"}),
        ),
        StatusCode::OK,
    )
    .await;
    h.flush_scheduler().await;
    let task: Value = h.get(&format!("/v1/tasks/{TASK}")).await;
    assert_eq!(task["status"], "pending");
    assert_eq!(task["execution_context"]["authorized"], false);
    assert!(task["worktree_path"].is_null());
    assert!(task["agents"][0]["session_id"].is_null());
}
