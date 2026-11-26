//! Tasks that own a workflow without a goal.
use crate::common::{harness, patch_json, post_json, test_pin};
use axum::http::StatusCode;
use serde_json::{Value, json};

#[tokio::test]
async fn a_user_creates_a_ready_task_without_a_goal() {
    let h = harness().await;
    h.store
        .create_workflow(ariadne_store::NewWorkflow {
            name: "solo".into(),
            document: "workflow solo\n  develop[Develop]\n    Build.\n    skills: coding\n".into(),
        })
        .await
        .unwrap();
    let repo = h.repository(&h.at("repo")).await;
    let mut events = h.bus.subscribe();
    let task: Value = h
        .json(
            post_json(
                "/v1/tasks",
                json!({
                    "title": "Independent work", "repo_id": repo.id, "workflow": "solo",
                    "agents": [{"step": "develop", "model": test_pin().model}]
                }),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert!(task["goal_id"].is_null());
    assert_eq!(task["status"], "ready");
    assert_eq!(task["workflow"], "solo");
    assert!(!task["steps"].as_array().unwrap().is_empty());
    assert!(h.store.list_goals(&[]).await.unwrap().is_empty());
    tokio::time::timeout(crate::common::TIMEOUT, async {
        loop {
            let event = events.recv().await.unwrap();
            if let ariadne_api::stream::DomainEvent::TaskCreated(dto) = event.event {
                assert_eq!(dto.id, task["id"].as_str().unwrap());
                assert!(dto.goal_id.is_none());
                assert!(event.goal_id.is_none());
                break;
            }
        }
    })
    .await
    .unwrap();
    let goal = h.goal_on(&repo, test_pin()).await;
    let goal_task = h.task_on(&goal, &repo, "Goal work", test_pin()).await;
    let ordinary: Value = h.get(&format!("/v1/tasks/{}", goal_task.id)).await;
    assert_eq!(ordinary["goal_id"], goal.id);
    assert!(ordinary["workflow"].is_null());
    assert_eq!(ordinary["steps"], json!([]));
    let listed: Vec<Value> = h.get("/v1/tasks?no_goal=true").await;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0]["id"], task["id"]);
}

#[tokio::test]
async fn admission_refuses_missing_context_dependencies_and_reserved_workflows() {
    let h = harness().await;
    let repo = h.repository(&h.at("repo")).await;
    let valid = json!({"title":"Work", "repo_id":repo.id,
        "workflow":"develop-review-merge", "agents":[
            {"step":"develop","model":test_pin().model},
            {"step":"review","model":test_pin().model},
            {"step":"merge","model":test_pin().model}]});
    for (field, value, status, reason) in [
        (
            "repo_id",
            json!("unknown-repository"),
            StatusCode::NOT_FOUND,
            "repository",
        ),
        (
            "workflow",
            json!("unknown-workflow"),
            StatusCode::NOT_FOUND,
            "workflow",
        ),
        (
            "workflow",
            json!("pr-review"),
            StatusCode::BAD_REQUEST,
            "pull request operation",
        ),
        (
            "workflow",
            json!("pr-babysit"),
            StatusCode::BAD_REQUEST,
            "pull request operation",
        ),
        (
            "host",
            json!("worker"),
            StatusCode::BAD_REQUEST,
            "local host",
        ),
        (
            "depends_on",
            json!([]),
            StatusCode::UNPROCESSABLE_ENTITY,
            "unknown field",
        ),
        ("agents", json!([]), StatusCode::BAD_REQUEST, "column"),
        (
            "agents",
            json!([{"step":"develop","model":"unknown:model"}]),
            StatusCode::BAD_REQUEST,
            "agent",
        ),
    ] {
        let mut body = valid.clone();
        body[field] = value;
        let (actual, bytes) = h.send(post_json("/v1/tasks", body)).await;
        let text = String::from_utf8_lossy(&bytes);
        assert_eq!(actual, status, "{field}: {text}");
        assert!(text.contains(reason), "{field}: {text}");
    }
    for field in ["repo_id", "workflow"] {
        let mut body = valid.clone();
        body.as_object_mut().unwrap().remove(field);
        let (status, _) = h.send(post_json("/v1/tasks", body)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    }
    assert!(
        h.store
            .list_tasks(Default::default())
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn a_refused_dependency_edit_leaves_the_rest_of_the_patch_unsaved() {
    let h = harness().await;
    let repo = h.repository(&h.at("repo")).await;
    let body = json!({"title":"Original", "repo_id":repo.id,
        "workflow":"develop-review-merge", "agents":[
            {"step":"develop","model":test_pin().model},
            {"step":"review","model":test_pin().model},
            {"step":"merge","model":test_pin().model}]});
    let created: Value = h
        .json(post_json("/v1/tasks", body), StatusCode::CREATED)
        .await;
    let path = format!("/v1/tasks/{}", created["id"].as_str().unwrap());
    let (status, _) = h
        .send(patch_json(
            &path,
            json!({"title":"Changed", "depends_on":["other"]}),
        ))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let task: Value = h.get(&path).await;
    assert_eq!(task["title"], "Original");
}

#[tokio::test]
async fn saved_columns_run_without_a_goal_and_cleanup_after_finish() {
    use crate::common::{
        TIMEOUT, as_session, eventually, eventually_some, get, sh, with_forge_enabled,
    };
    let h = harness().scheduler().await;
    h.store.create_workflow(ariadne_store::NewWorkflow {
        name: "solo".into(),
        document: "workflow solo\n  develop[Develop]\n    Build.\n    skills: coding\n    gate: committed\n  inspect[Inspect]\n    Check.\n    skills: code-review\n".into(),
    }).await.unwrap();
    let repo = h.repository(&h.git_repo("repo")).await;
    with_forge_enabled(&h, &repo, false).await;
    let body = json!({"title":"Independent work", "repo_id":repo.id, "workflow":"solo", "host":"local",
        "agents":[{"step":"develop","model":test_pin().model},{"step":"inspect","model":test_pin().model}]});
    let created: Value = h
        .json(post_json("/v1/tasks", body.clone()), StatusCode::CREATED)
        .await;
    let id = created["id"].as_str().unwrap();
    let path = format!("/v1/tasks/{id}");
    let running = eventually_some(TIMEOUT, "independent agent", async || {
        let task: Value = h.get(&path).await;
        assert_ne!(task["status"], "failed", "{task}");
        (task["agents"][0]["session_id"].is_string() && task["worktree_path"].is_string())
            .then_some(task)
    })
    .await;
    assert_eq!(h.send(get(&format!("{path}/diff"))).await.0, StatusCode::OK);
    let session = running["agents"][0]["session_id"].as_str().unwrap();
    let observed: Value = h.get(&format!("/v1/sessions/{session}")).await;
    assert!(observed["goal_id"].is_null());
    let worktree = std::path::Path::new(running["worktree_path"].as_str().unwrap());
    assert_eq!(worktree.parent().unwrap().file_name().unwrap(), "tasks");
    h.json::<Value>(
        as_session("/v1/tasks", session, body),
        StatusCode::FORBIDDEN,
    )
    .await;
    let (status, _) = h
        .send(as_session(
            &format!("{path}/posts"),
            session,
            json!({"to":"orchestrator","body":"Need guidance."}),
        ))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    h.json::<Value>(
        as_session(
            &format!("{path}/step/complete"),
            session,
            json!({"reason":"Check gate."}),
        ),
        StatusCode::CONFLICT,
    )
    .await;
    sh(worktree, "echo change > independent.txt");
    sh(worktree, "git add independent.txt");
    sh(
        worktree,
        "git -c user.name=Test -c user.email=test@test commit -m 'feat(core): add independent work'",
    );
    h.json::<Value>(
        as_session(
            &format!("{path}/step/complete"),
            session,
            json!({"reason":"Committed."}),
        ),
        StatusCode::OK,
    )
    .await;
    let inspect = eventually_some(TIMEOUT, "independent review", async || {
        let task: Value = h.get(&path).await;
        task["agents"][1]["session_id"].is_string().then_some(task)
    })
    .await;
    h.json::<Value>(
        as_session(
            &format!("{path}/step/fail"),
            inspect["agents"][1]["session_id"].as_str().unwrap(),
            json!({"reason":"Revise."}),
        ),
        StatusCode::OK,
    )
    .await;
    h.flush_scheduler().await;
    let returned: Value = h.get(&path).await;
    assert_eq!(returned["step"], "develop");
    assert_eq!(returned["status"], "in_progress", "{returned}");
    let developer = returned["agents"][0]["session_id"].as_str().unwrap();
    h.json::<Value>(
        as_session(
            &format!("{path}/step/complete"),
            developer,
            json!({"reason":"Ready."}),
        ),
        StatusCode::OK,
    )
    .await;
    h.flush_scheduler().await;
    let last: Value = h.get(&path).await;
    h.json::<Value>(
        as_session(
            &format!("{path}/step/complete"),
            last["agents"][1]["session_id"].as_str().unwrap(),
            json!({"reason":"Finished."}),
        ),
        StatusCode::OK,
    )
    .await;
    eventually(TIMEOUT, "independent cleanup", async || !worktree.exists()).await;
    eventually(TIMEOUT, "independent branch cleanup", async || {
        let output = std::process::Command::new("git")
            .args([
                "show-ref",
                "--verify",
                &format!("refs/heads/{}", running["branch"].as_str().unwrap()),
            ])
            .current_dir(&repo.path)
            .output()
            .unwrap();
        !output.status.success()
    })
    .await;
    let ended: Value = h.get(&path).await;
    assert_eq!(ended["status"], "finished");
    assert!(h.store.list_goals(&[]).await.unwrap().is_empty());
    let sessions = h.store.list_sessions(Default::default()).await.unwrap();
    assert!(sessions.iter().all(|s| s.goal_id.is_none()));
}

#[tokio::test]
async fn retry_cancel_and_saved_columns_survive_a_restart() {
    use ariadne_core::{Actor, TaskStatus};
    let h = harness().await;
    h.store
        .create_workflow(ariadne_store::NewWorkflow {
            name: "solo".into(),
            document: "workflow solo\n  develop[Develop]\n    Build.\n    skills: coding\n".into(),
        })
        .await
        .unwrap();
    let repo = h.repository(&h.at("repo")).await;
    let task: Value = h.json(post_json("/v1/tasks", json!({"title":"Durable work",
        "repo_id":repo.id,"workflow":"solo","agents":[{"step":"develop","model":test_pin().model}]})), StatusCode::CREATED).await;
    let id = task["id"].as_str().unwrap();
    h.store
        .set_workflow_document(
            "solo",
            "workflow solo\n  changed[Changed]\n    Inspect.\n    skills: code-review\n",
        )
        .await
        .unwrap();
    let reopened = ariadne_store::Store::open(h.at("test.db")).await.unwrap();
    let saved = reopened.get_task(id).await.unwrap();
    assert!(saved.goal_id.is_none());
    assert_eq!(reopened.task_steps(&saved).await.unwrap()[0].id, "develop");
    assert!(
        reopened
            .tasks_without_local_goals()
            .await
            .unwrap()
            .iter()
            .any(|t| t.id == id)
    );
    reopened
        .transition_task(
            id,
            TaskStatus::Failed,
            Actor::Daemon,
            Some("Test retry."),
            None,
        )
        .await
        .unwrap();
    let retried: Value = h
        .json(
            post_json(&format!("/v1/tasks/{id}/retry"), json!({})),
            StatusCode::OK,
        )
        .await;
    assert_eq!(retried["status"], "ready");
    assert_eq!(retried["steps"], task["steps"]);
    let cancelled: Value = h
        .json(
            post_json(&format!("/v1/tasks/{id}/cancel"), json!({"reason":"Stop."})),
            StatusCode::OK,
        )
        .await;
    assert_eq!(cancelled["status"], "cancelled");
    assert!(h.store.list_goals(&[]).await.unwrap().is_empty());
}

#[tokio::test]
async fn openapi_describes_goal_less_creation_and_task_columns() {
    let h = harness().await;
    let doc: Value = h.get("/api-docs/openapi.json").await;
    assert!(doc["paths"]["/v1/tasks"]["post"].is_object());
    let properties = &doc["components"]["schemas"]["TaskDto"]["properties"];
    assert_eq!(properties["goal_id"]["type"], json!(["string", "null"]));
    assert!(properties["workflow"].is_object());
    assert_eq!(properties["steps"]["type"], "array");
    let request = &doc["components"]["schemas"]["CreateStandaloneTaskRequest"];
    for field in ["title", "repo_id", "workflow", "agents"] {
        assert!(
            request["required"]
                .as_array()
                .unwrap()
                .contains(&json!(field))
        );
    }
    assert!(request["properties"].get("depends_on").is_none());
    assert!(
        doc["paths"]["/v1/tasks"]["get"]["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["name"] == "no_goal")
    );
}
