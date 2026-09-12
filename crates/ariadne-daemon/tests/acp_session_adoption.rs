//! Session discovery and adoption over ACP: the stored sessions of a
//! registered ACP agent, listed through `session/list` and adopted through
//! `session/load`.

mod common;

use serde_json::json;

use ariadne_api::error::ErrorBody;
use ariadne_api::sessions::{AdoptOutsideSessionResponse, OutsideSessionPageDto};
use ariadne_core::{Actor, GoalStatus, Seat, SessionStatus, TaskStatus};
use ariadne_store::{AgentPin, NewGoal, SessionFilter};

use common::acp::{script, stub_acp_agent};
use common::{Harness, TIMEOUT, as_session, eventually, harness, post_json};

/// A home whose `config.toml` registers one custom ACP agent at `bin`.
fn home_with_agent(id: &str, bin: &str) -> std::path::PathBuf {
    let home = std::path::Path::new(bin).parent().unwrap().join("home");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(
        home.join("config.toml"),
        format!("[[acp_agents]]\nid = {id:?}\ncommand = [{bin:?}]\n"),
    )
    .unwrap();
    home
}

/// A stub that can list its stored sessions (`sessionCapabilities.list`) and
/// load one of them (`loadSession`), but does not advertise `resume` — so an
/// adopted conversation is continued through `session/load`, never
/// `session/resume`.
fn adoptable_script() -> serde_json::Value {
    let mut setup = script();
    setup["capabilities"] = json!({"loadSession": true, "sessionCapabilities": {"list": {}}});
    setup["stored_sessions"] = json!(["outside-1"]);
    setup["session_list"] = json!([{
        "sessionId": "outside-1",
        "cwd": "/work/outside",
        "title": "fix the outstanding bug",
        "updatedAt": "2026-01-01T00:00:00Z",
    }]);
    setup
}

fn adoptable_script_at(working_directory: &str, first_prompt: &str) -> serde_json::Value {
    let mut setup = adoptable_script();
    setup["session_list"][0]["cwd"] = json!(working_directory);
    setup["session_list"][0]["title"] = json!(first_prompt);
    setup
}

fn adoption_request(goal: serde_json::Value, model: &str) -> serde_json::Value {
    json!({
        "agent_id": "test-agent",
        "internal_session_id": "outside-1",
        "goal": goal,
        "description": "continue the outside work",
        "agents": [{"seat": "author", "model": model}],
        "landing": "none",
        "permission_mode": "auto",
    })
}

async fn harness_with_agent(id: &str, bin: &str) -> Harness {
    let h = harness()
        .home(home_with_agent(id, bin))
        .discover_agents()
        .await;
    eventually(TIMEOUT, "discovery to accept the adoption stub", || async {
        if h.launcher.registry.agents().await.iter().any(|agent| {
            agent.id == id && agent.status == ariadne_api::agents::AcpAgentStatus::Ready
        }) {
            return true;
        }
        h.launcher.registry.refresh().await;
        false
    })
    .await;
    h
}

/// The stub agent's stored sessions appear in `GET /v1/outside-sessions`,
/// named by the registry agent they belong to.
#[tokio::test]
async fn an_acp_agents_stored_sessions_appear_in_the_listing() {
    let dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(dir.path(), adoptable_script());
    let h = harness_with_agent("test-agent", &stub.bin).await;

    let page: OutsideSessionPageDto = h.get("/v1/outside-sessions").await;

    let sessions = page.sessions;
    let found = sessions
        .iter()
        .find(|session| session.internal_session_id == "outside-1")
        .unwrap_or_else(|| panic!("outside-1 not in {sessions:#?}"));
    assert_eq!(found.agent_id, "test-agent");
    assert_eq!(found.working_directory, "/work/outside");
    assert_eq!(found.first_prompt, "fix the outstanding bug");
    assert_eq!(found.last_activity_at, "2026-01-01T00:00:00Z");
}

/// An agent without the session-listing capability contributes nothing to
/// the listing, and the catalog says why.
#[tokio::test]
async fn an_agent_without_the_capability_lists_nothing_and_shows_the_reason() {
    let dir = tempfile::tempdir().unwrap();
    // No `sessionCapabilities.list` and no `listSessions`: the default
    // `script()` capabilities, minus resume — session_list stays false.
    let mut setup = script();
    setup["capabilities"] = json!({"loadSession": true});
    let stub = stub_acp_agent(dir.path(), setup);
    let h = harness_with_agent("no-listing", &stub.bin).await;

    let page: OutsideSessionPageDto = h.get("/v1/outside-sessions").await;
    assert!(
        page.sessions
            .iter()
            .all(|session| session.agent_id != "no-listing")
    );

    let agents: Vec<serde_json::Value> = h.get("/v1/acp-agents").await;
    let agent = agents
        .iter()
        .find(|agent| agent["id"] == "no-listing")
        .unwrap();
    assert_eq!(agent["capabilities"]["session_list"], false);
    assert!(
        agent["degraded"]
            .as_array()
            .unwrap()
            .iter()
            .any(|gap| gap == "no_adoption"),
        "{agent}"
    );
}

/// Only the user can use the goal-and-task adoption endpoint.
#[tokio::test]
async fn an_agent_session_cannot_adopt_an_outside_session() {
    let h = harness().await;
    let cast = h.cast().await;
    let session = h
        .session(&cast.goal, Some(&cast.task), Seat::Author, &cast.author.id)
        .await;

    let (status, _) = h
        .send(as_session(
            "/v1/outside-sessions/adopt",
            &session.id,
            serde_json::json!({
                "agent_id": "stub",
                "internal_session_id": "outside-1",
                "goal": {"id": cast.goal.id},
                "description": "continue the outside work",
                "agents": [{"seat": "author", "model": "stub:test-model"}],
            }),
        ))
        .await;

    assert_eq!(status, axum::http::StatusCode::FORBIDDEN);
}

/// One call opens an active goal without an orchestrator, creates its task,
/// and loads the outside conversation into that task's author seat.
#[tokio::test]
async fn adoption_into_a_new_goal_creates_and_loads_the_author_without_an_orchestrator() {
    let dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(dir.path(), adoptable_script());
    let h = harness()
        .home(home_with_agent("test-agent", &stub.bin))
        .discover_agents()
        .scheduler()
        .await;
    eventually(TIMEOUT, "discovery to accept the adoption stub", || async {
        if h.launcher.registry.agents().await.iter().any(|agent| {
            agent.id == "test-agent" && agent.status == ariadne_api::agents::AcpAgentStatus::Ready
        }) {
            return true;
        }
        h.launcher.registry.refresh().await;
        false
    })
    .await;
    let repository = h.repository(&h.git_repo("adopted-repo")).await;
    let working_directory = std::path::Path::new(&repository.path).join("nested/project");
    let first_prompt = format!("continue this work {}", "x".repeat(130));
    stub.reprogram(adoptable_script_at(
        &working_directory.display().to_string(),
        &first_prompt,
    ));
    stub.clear_messages();

    let adopted: AdoptOutsideSessionResponse = h
        .json(
            post_json(
                "/v1/outside-sessions/adopt",
                adoption_request(json!({"title": "Imported work"}), "test-agent:old-model"),
            ),
            axum::http::StatusCode::CREATED,
        )
        .await;

    assert_eq!(adopted.goal.status, GoalStatus::Active);
    assert!(!adopted.goal.orchestrated);
    assert_eq!(adopted.goal.model, "test-agent:old-model");
    assert_eq!(adopted.goal.repos.len(), 1);
    assert_eq!(adopted.goal.repos[0].id, repository.id);
    assert_eq!(adopted.task.status, TaskStatus::InProgress);
    assert_eq!(
        adopted.task.title,
        first_prompt.chars().take(120).collect::<String>()
    );
    assert_eq!(adopted.task.agents[0].model, "test-agent:old-model");
    assert_eq!(
        adopted.session.task_id.as_deref(),
        Some(adopted.task.id.as_str())
    );
    assert_eq!(
        adopted.session.internal_session_id.as_deref(),
        Some("outside-1")
    );

    eventually(
        TIMEOUT,
        "the adopted conversation to finish its turn",
        || async {
            h.store
                .get_session(&adopted.session.id)
                .await
                .unwrap()
                .status()
                == SessionStatus::Idle
        },
    )
    .await;
    assert!(stub.methods().contains(&"session/load".to_string()));
    assert!(!stub.methods().contains(&"session/new".to_string()));

    h.state.notify_scheduler_goal(&adopted.goal.id);
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let sessions = h
        .store
        .list_sessions(SessionFilter {
            goal_id: Some(adopted.goal.id.clone()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(sessions.len(), 1, "{sessions:#?}");
    assert_eq!(sessions[0].seat(), Seat::Author);

    let page: OutsideSessionPageDto = h.get("/v1/outside-sessions").await;
    assert!(
        page.sessions
            .iter()
            .all(|session| session.internal_session_id != "outside-1"),
        "{:#?}",
        page.sessions
    );
}

/// Repository inference refuses a directory outside every registered repository.
#[tokio::test]
async fn a_new_goal_without_a_matching_repository_is_refused_by_directory() {
    let dir = tempfile::tempdir().unwrap();
    let outside = dir.path().join("not-registered/project");
    let stub = stub_acp_agent(
        dir.path(),
        adoptable_script_at(&outside.display().to_string(), "continue work"),
    );
    let h = harness_with_agent("test-agent", &stub.bin).await;
    h.repository(&h.git_repo("somewhere-else")).await;

    let error: ErrorBody = h
        .error(
            post_json(
                "/v1/outside-sessions/adopt",
                adoption_request(json!({"title": "Imported work"}), "test-agent:old-model"),
            ),
            axum::http::StatusCode::CONFLICT,
        )
        .await;

    assert!(
        error.error.message.contains(&outside.display().to_string()),
        "{}",
        error.error.message
    );
}

/// An active goal receives the new task and keeps its existing orchestration policy.
#[tokio::test]
async fn adoption_into_an_active_goal_adds_the_task_to_that_goal() {
    let dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(dir.path(), adoptable_script());
    let h = harness_with_agent("test-agent", &stub.bin).await;
    let repository = h.repository(&h.git_repo("existing-goal-repo")).await;
    let other_repository = h.repository(&h.git_repo("other-goal-repo")).await;
    let goal = h
        .store
        .create_goal(NewGoal {
            title: "Existing work".into(),
            description: String::new(),
            repository_ids: vec![other_repository.id, repository.id.clone()],
            pin: AgentPin {
                model: "test-agent:old-model".into(),
                effort: None,
            },
        })
        .await
        .unwrap();
    let goal = h.activate(&goal).await;
    let working_directory = std::path::Path::new(&repository.path).join("inside");
    stub.reprogram(adoptable_script_at(
        &working_directory.display().to_string(),
        "continue work",
    ));

    let adopted: AdoptOutsideSessionResponse = h
        .json(
            post_json(
                "/v1/outside-sessions/adopt",
                adoption_request(json!({"id": goal.id}), "test-agent:old-model"),
            ),
            axum::http::StatusCode::CREATED,
        )
        .await;

    assert_eq!(adopted.goal.id, goal.id);
    assert!(adopted.goal.orchestrated);
    assert_eq!(adopted.task.goal_id, goal.id);
    assert_eq!(adopted.task.repo_id, repository.id);
}

/// A planning or completed goal cannot receive an adopted session's task.
#[tokio::test]
async fn adoption_into_a_goal_that_is_not_active_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(dir.path(), adoptable_script());
    let h = harness_with_agent("test-agent", &stub.bin).await;
    let repository = h.repository(&h.git_repo("closed-goal-repo")).await;
    let pin = AgentPin {
        model: "test-agent:old-model".into(),
        effort: None,
    };
    let planning = h.goal_on(&repository, pin.clone()).await;
    let completed = h.goal_on(&repository, pin).await;
    let completed = h
        .store
        .set_goal_status(&completed.id, GoalStatus::Completed)
        .await
        .unwrap();

    for goal in [&planning, &completed] {
        let error: ErrorBody = h
            .error(
                post_json(
                    "/v1/outside-sessions/adopt",
                    adoption_request(json!({"id": goal.id}), "test-agent:old-model"),
                ),
                axum::http::StatusCode::CONFLICT,
            )
            .await;
        assert!(error.error.message.contains(&goal.status), "{error:#?}");
    }
}

/// The outside session's registry agent must match the first author's pin.
#[tokio::test]
async fn adoption_is_refused_when_the_author_pin_names_another_agent() {
    let dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(dir.path(), adoptable_script());
    let h = harness_with_agent("test-agent", &stub.bin).await;
    let repository = h.repository(&h.git_repo("pin-repo")).await;
    let goal = h
        .goal_on(
            &repository,
            AgentPin {
                model: "test-agent:old-model".into(),
                effort: None,
            },
        )
        .await;
    let goal = h.activate(&goal).await;

    let error: ErrorBody = h
        .error(
            post_json(
                "/v1/outside-sessions/adopt",
                adoption_request(json!({"id": goal.id}), "another-agent:model"),
            ),
            axum::http::StatusCode::CONFLICT,
        )
        .await;

    assert!(error.error.message.contains("another-agent"), "{error:#?}");
}

/// A snapshot miss causes one refresh and still returns not found for that call.
#[tokio::test]
async fn a_session_missing_from_the_snapshot_is_refused_after_one_fresh_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(dir.path(), adoptable_script());
    let h = harness_with_agent("test-agent", &stub.bin).await;
    let _: OutsideSessionPageDto = h.get("/v1/outside-sessions").await;
    stub.clear_messages();

    let mut request = adoption_request(json!({"id": "unused"}), "test-agent:old-model");
    request["internal_session_id"] = json!("missing");
    let error: ErrorBody = h
        .error(
            post_json("/v1/outside-sessions/adopt", request),
            axum::http::StatusCode::NOT_FOUND,
        )
        .await;

    assert!(error.error.message.contains("missing"), "{error:#?}");
    assert_eq!(stub.calls_of("session/list").len(), 1);
}

/// A message cannot target an orchestrator on an unorchestrated goal.
#[tokio::test]
async fn a_message_to_an_unorchestrated_goals_orchestrator_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(dir.path(), adoptable_script());
    let h = harness_with_agent("test-agent", &stub.bin).await;
    let repository = h.repository(&h.git_repo("message-repo")).await;
    stub.reprogram(adoptable_script_at(&repository.path, "continue work"));
    let adopted: AdoptOutsideSessionResponse = h
        .json(
            post_json(
                "/v1/outside-sessions/adopt",
                adoption_request(json!({"title": "Imported work"}), "test-agent:old-model"),
            ),
            axum::http::StatusCode::CREATED,
        )
        .await;

    let error: ErrorBody = h
        .error(
            post_json(
                &format!("/v1/goals/{}/messages", adopted.goal.id),
                json!({
                    "kind": "message",
                    "to_actor": Actor::Orchestrator,
                    "body": "what should happen next?",
                }),
            ),
            axum::http::StatusCode::CONFLICT,
        )
        .await;

    assert_eq!(
        error.error.message,
        "this goal has no orchestrator; the user answers in the console"
    );
}

/// The new adoption endpoint and its request and response are in OpenAPI,
/// and the retired task-first endpoint is gone from it.
#[tokio::test]
async fn the_goal_and_task_adoption_endpoint_is_in_the_openapi_document() {
    let h = harness().await;

    let document: serde_json::Value = h.get("/api-docs/openapi.json").await;

    let post = &document["paths"]["/v1/outside-sessions/adopt"]["post"];
    assert_eq!(
        post["requestBody"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/AdoptOutsideSessionRequest"
    );
    assert_eq!(
        post["responses"]["201"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/AdoptOutsideSessionResponse"
    );
    assert_eq!(
        document["components"]["schemas"]["GoalDto"]["properties"]["orchestrated"]["type"],
        "boolean"
    );
    assert!(document["paths"]["/v1/tasks/{id}/author-session"].is_null());
}

/// Adoption refuses an empty task title instead of creating an unnamed task.
#[tokio::test]
async fn adoption_with_an_empty_task_title_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(dir.path(), adoptable_script());
    let h = harness_with_agent("test-agent", &stub.bin).await;
    let repository = h.repository(&h.git_repo("empty-title-repo")).await;
    let goal = h
        .goal_on(
            &repository,
            AgentPin {
                model: "test-agent:old-model".into(),
                effort: None,
            },
        )
        .await;
    let goal = h.activate(&goal).await;
    let mut request = adoption_request(json!({"id": goal.id}), "test-agent:old-model");
    request["title"] = json!("   ");

    let error: ErrorBody = h
        .error(
            post_json("/v1/outside-sessions/adopt", request),
            axum::http::StatusCode::BAD_REQUEST,
        )
        .await;

    assert_eq!(error.error.message, "a task needs a title");
}

/// Adoption rejects fields outside its request and either goal shape.
#[tokio::test]
async fn the_adoption_request_denies_unknown_fields() {
    let h = harness().await;
    let mut top = adoption_request(json!({"id": "goal"}), "stub:test-model");
    top["unexpected"] = json!(true);
    let mut nested = adoption_request(json!({"id": "goal"}), "stub:test-model");
    nested["goal"]["unexpected"] = json!(true);

    for request in [top, nested] {
        let error: ErrorBody = h
            .error(
                post_json("/v1/outside-sessions/adopt", request),
                axum::http::StatusCode::UNPROCESSABLE_ENTITY,
            )
            .await;
        assert_eq!(error.error.code, "invalid_request");
    }
}
