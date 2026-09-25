//! Starting a loose session: `POST /v1/sessions` opens a new conversation
//! with a registry agent, in a directory, with no goal, task or seat.

use crate::common;

use axum::http::StatusCode;
use serde_json::json;

use ariadne_api::error::ErrorBody;
use ariadne_api::sessions::SessionDto;
use ariadne_core::{Seat, SessionStatus};
use ariadne_store::{SessionFilter, TaskFilter};

use common::acp::{StubAcpAgent, script, stub_acp_agent};
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

/// A stub that offers `new-model` beside the model it runs, and `high`
/// beside the effort it runs at, as the agent's catalog.
fn offering_script() -> serde_json::Value {
    let mut setup = script();
    setup["config_options"][0]["options"] = json!([
        {"value": "old-model", "name": "Old"},
        {"value": "new-model", "name": "New"},
    ]);
    setup["config_options"][1]["options"] = json!([
        {"value": "low", "name": "Low"},
        {"value": "high", "name": "High"},
    ]);
    setup
}

/// The harness over the stub once discovery has accepted it, with the
/// stub's log cleared of the probe discovery ran — which opens a session of
/// its own to read the catalog.
async fn harness_with_agent(id: &str, stub: &StubAcpAgent) -> Harness {
    let h = harness()
        .home(home_with_agent(id, &stub.bin))
        .discover_agents()
        .await;
    eventually(TIMEOUT, "discovery to accept the stub", || async {
        if h.launcher.registry.agents().await.iter().any(|agent| {
            agent.id == id && agent.status == ariadne_api::agents::AcpAgentStatus::Ready
        }) {
            return true;
        }
        h.launcher.registry.refresh().await;
        false
    })
    .await;
    stub.clear_messages();
    h
}

fn start_request(working_directory: &std::path::Path) -> serde_json::Value {
    json!({
        "model": "test-agent:new-model",
        "effort": "high",
        "working_directory": working_directory.to_str().unwrap(),
    })
}

/// A new session opens a new conversation in the directory it names, on the
/// model and effort it names, and runs no scheduled work: no goal, task,
/// worktree or seat. The first prompt typed into it is its title.
#[tokio::test]
async fn a_new_session_opens_a_conversation_in_its_directory() {
    let dir = tempfile::tempdir().unwrap();
    let work = dir.path().join("work");
    std::fs::create_dir(&work).unwrap();
    let stub = stub_acp_agent(dir.path(), offering_script());
    let h = harness_with_agent("test-agent", &stub).await;

    let session: SessionDto = h
        .json(
            post_json("/v1/sessions", start_request(&work)),
            StatusCode::OK,
        )
        .await;

    assert!(session.status.is_live());
    assert_eq!(session.goal_id, None);
    assert_eq!(session.task_id, None);
    assert_eq!(session.seat, None);
    assert_eq!(session.title, None);
    assert_eq!(session.model, "test-agent:new-model");
    assert_eq!(session.effort.as_deref(), Some("high"));
    assert_eq!(session.worktree_path.as_deref(), work.to_str());
    assert_eq!(
        stub.calls_of("session/new")[0]["cwd"],
        work.to_str().unwrap()
    );
    assert!(stub.calls_of("session/load").is_empty());
    let switched: Vec<_> = stub
        .calls_of("session/set_config_option")
        .iter()
        .map(|call| call["value"].clone())
        .collect();
    assert_eq!(switched, [json!("new-model"), json!("high")]);
    assert!(h.store.list_goals(&[]).await.unwrap().is_empty());
    assert!(
        h.store
            .list_tasks(TaskFilter::default())
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        std::fs::read_dir(&h.launcher.cfg.worktree_root)
            .unwrap()
            .count(),
        0
    );

    let row = format!("/v1/sessions/{}", session.id);
    eventually(TIMEOUT, "the agent's own session id", || async {
        let row: SessionDto = h.get(&row).await;
        row.internal_session_id.as_deref() == Some("stub-session")
    })
    .await;
    let (status, _) = h
        .send(post_json(
            &format!("/v1/sessions/{}/console/input", session.id),
            json!({"text": "Tidy the release notes"}),
        ))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let titled: SessionDto = h.get(&row).await;
    assert_eq!(titled.title.as_deref(), Some("Tidy the release notes"));
    assert_eq!(
        h.store
            .list_sessions(SessionFilter::default())
            .await
            .unwrap()
            .len(),
        1
    );
}

/// A new session that ended comes back through the usual revival, loading
/// the conversation it opened rather than opening another.
#[tokio::test]
async fn a_new_session_resumes_the_conversation_it_opened() {
    let dir = tempfile::tempdir().unwrap();
    let mut setup = offering_script();
    setup["stored_sessions"] = json!(["stub-session"]);
    let stub = stub_acp_agent(dir.path(), setup);
    let h = harness_with_agent("test-agent", &stub).await;
    let session: SessionDto = h
        .json(
            post_json("/v1/sessions", start_request(dir.path())),
            StatusCode::OK,
        )
        .await;
    let row = format!("/v1/sessions/{}", session.id);
    eventually(TIMEOUT, "the agent's own session id", || async {
        let row: SessionDto = h.get(&row).await;
        row.internal_session_id.is_some()
    })
    .await;

    let killed: SessionDto = h
        .json(common::post(&format!("{row}/kill")), StatusCode::OK)
        .await;
    assert_eq!(killed.status, SessionStatus::Exited);
    let revived: SessionDto = h
        .json(common::post(&format!("{row}/resume")), StatusCode::OK)
        .await;

    assert_eq!(revived.id, session.id);
    assert!(revived.status.is_live());
    assert_eq!(stub.calls_of("session/new").len(), 1);
    let loaded = stub
        .calls_of("session/load")
        .into_iter()
        .chain(stub.calls_of("session/resume"))
        .collect::<Vec<_>>();
    assert_eq!(loaded.len(), 1, "{loaded:?}");
    assert_eq!(loaded[0]["sessionId"], "stub-session");
}

/// A directory that is relative or missing, and a model whose agent is not
/// in the registry, are refused before anything starts.
#[tokio::test]
async fn a_new_session_needs_an_existing_absolute_directory_and_a_registry_model() {
    let dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(dir.path(), offering_script());
    let h = harness_with_agent("test-agent", &stub).await;

    for (body, says) in [
        (
            json!({"model": "test-agent:new-model", "working_directory": "relative/dir"}),
            "absolute path",
        ),
        (
            json!({
                "model": "test-agent:new-model",
                "working_directory": dir.path().join("missing").to_str().unwrap(),
            }),
            "absolute path",
        ),
        (
            json!({
                "model": "nobody:new-model",
                "working_directory": dir.path().to_str().unwrap(),
            }),
            "nobody",
        ),
    ] {
        let error: ErrorBody = h
            .error(post_json("/v1/sessions", body), StatusCode::BAD_REQUEST)
            .await;
        assert!(
            error.error.message.contains(says),
            "{}",
            error.error.message
        );
    }
    assert!(
        h.store
            .list_sessions(SessionFilter::default())
            .await
            .unwrap()
            .is_empty()
    );
    assert!(stub.calls_of("session/new").is_empty());
}

#[tokio::test]
async fn an_agent_session_cannot_start_a_session() {
    let h = harness().await;
    let cast = h.cast().await;
    let session = h
        .session(&cast.goal, Some(&cast.task), Seat::Author, &cast.author.id)
        .await;
    let dir = tempfile::tempdir().unwrap();

    let (status, _) = h
        .send(as_session(
            "/v1/sessions",
            &session.id,
            start_request(dir.path()),
        ))
        .await;

    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn the_start_endpoint_is_in_the_openapi_document() {
    let h = harness().await;
    let doc: serde_json::Value = h.get("/api-docs/openapi.json").await;
    let post = &doc["paths"]["/v1/sessions"]["post"];
    assert_eq!(
        post["requestBody"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/NewSessionRequest"
    );
    assert!(doc["components"]["schemas"]["NewSessionRequest"].is_object());
}
