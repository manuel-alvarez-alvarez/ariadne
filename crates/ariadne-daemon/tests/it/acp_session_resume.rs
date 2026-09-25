//! Session discovery and resume over ACP: the stored sessions of a
//! registered ACP agent, listed through `session/list` and resumed through
//! `session/load`.

use crate::common;

use serde_json::json;

use ariadne_api::error::ErrorBody;
use ariadne_api::sessions::{SessionDto, SessionKind, SessionPageDto};
use ariadne_core::{PermissionMode, Seat, SessionStatus};
use ariadne_store::{SessionFilter, TaskFilter};

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
/// stored conversation is continued through `session/load`, never
/// `session/resume`.
fn outside_script() -> serde_json::Value {
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

fn outside_script_at(working_directory: &str, first_prompt: &str) -> serde_json::Value {
    let mut setup = outside_script();
    setup["session_list"][0]["cwd"] = json!(working_directory);
    setup["session_list"][0]["title"] = json!(first_prompt);
    setup
}

async fn harness_with_agent(id: &str, bin: &str) -> Harness {
    let h = harness()
        .home(home_with_agent(id, bin))
        .discover_agents()
        .await;
    eventually(TIMEOUT, "discovery to accept the session stub", || async {
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

/// The stub agent's stored sessions appear in `GET /v1/sessions`, named by
/// the registry agent they belong to. The listing holds the last seven days
/// by default, and this conversation is dated further back, so the request
/// asks for all of them.
#[tokio::test]
async fn an_acp_agents_stored_sessions_appear_in_the_listing() {
    let dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(dir.path(), outside_script());
    let h = harness_with_agent("test-agent", &stub.bin).await;

    let page: SessionPageDto = h.get("/v1/sessions?all=true").await;

    let sessions = page.sessions;
    let found = sessions
        .iter()
        .find(|session| session.internal_session_id.as_deref() == Some("outside-1"))
        .unwrap_or_else(|| panic!("outside-1 not in {sessions:#?}"));
    assert_eq!(found.kind, SessionKind::Outside);
    assert_eq!(found.agent_id, "test-agent");
    assert_eq!(found.working_directory.as_deref(), Some("/work/outside"));
    assert_eq!(found.title.as_deref(), Some("fix the outstanding bug"));
    assert_eq!(
        found.last_activity_at.as_deref(),
        Some("2026-01-01T00:00:00Z")
    );
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

    let page: SessionPageDto = h.get("/v1/sessions?all=true").await;
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

fn resume_request() -> serde_json::Value {
    json!({"agent_id": "test-agent", "internal_session_id": "outside-1"})
}

/// A conversation whose directory is gone cannot start there: the resume is
/// refused with that reason, before any row is made, so the conversation
/// stays outside rather than turning into a failed session.
#[tokio::test]
async fn an_outside_session_whose_directory_is_gone_is_refused_without_a_row() {
    let dir = tempfile::tempdir().unwrap();
    let gone = dir.path().join("removed-worktree");
    let setup = outside_script_at(gone.to_str().unwrap(), "continue");
    let stub = stub_acp_agent(dir.path(), setup);
    let h = harness_with_agent("test-agent", &stub.bin).await;

    let refusal: ErrorBody = h
        .json(
            post_json("/v1/outside-sessions/resume", resume_request()),
            axum::http::StatusCode::CONFLICT,
        )
        .await;

    assert_eq!(
        refusal.error.message,
        format!(
            "the directory this conversation ran in is gone: {}",
            gone.display()
        )
    );
    assert!(
        h.store
            .list_sessions(SessionFilter::default())
            .await
            .unwrap()
            .is_empty()
    );
    let page: SessionPageDto = h.get("/v1/sessions?all=true").await;
    let row = page
        .sessions
        .iter()
        .find(|session| session.internal_session_id.as_deref() == Some("outside-1"))
        .unwrap_or_else(|| panic!("outside-1 not in {:#?}", page.sessions));
    assert_eq!(row.kind, SessionKind::Outside);
}

#[tokio::test]
async fn an_outside_session_loads_in_its_directory_without_scheduled_work() {
    let dir = tempfile::tempdir().unwrap();
    let mut setup = outside_script_at(dir.path().to_str().unwrap(), "continue");
    setup["capabilities"]["sessionCapabilities"]["resume"] = json!({});
    let stub = stub_acp_agent(dir.path(), setup.clone());
    let h = harness()
        .scheduler()
        .home(home_with_agent("test-agent", &stub.bin))
        .discover_agents()
        .await;
    setup["config_options"][0]["currentValue"] = json!("loaded-model");
    stub.reprogram(setup);
    let session: SessionDto = h
        .json(
            post_json("/v1/outside-sessions/resume", resume_request()),
            axum::http::StatusCode::OK,
        )
        .await;
    assert!(session.status.is_live());
    assert_eq!(session.goal_id, None);
    assert_eq!(session.task_id, None);
    assert_eq!(session.seat, None);
    assert_eq!(session.model, "test-agent:loaded-model");
    assert_eq!(session.internal_session_id.as_deref(), Some("outside-1"));
    let row: SessionDto = h.get(&format!("/v1/sessions/{}", session.id)).await;
    assert_eq!(row.title.as_deref(), Some("continue"));
    assert_eq!(
        stub.calls_of("session/load")[0]["cwd"],
        dir.path().to_str().unwrap()
    );
    assert_eq!(stub.calls_of("session/load")[0]["sessionId"], "outside-1");
    assert!(stub.calls_of("session/resume").is_empty());
    assert!(stub.calls_of("session/set_config_option").is_empty());
    assert_eq!(
        std::fs::read_dir(&h.launcher.cfg.worktree_root)
            .unwrap()
            .count(),
        0
    );
    h.sched
        .as_ref()
        .unwrap()
        .send(ariadne_daemon::scheduler::SchedEvent::SessionEvent(
            session.id.clone(),
        ))
        .unwrap();
    h.flush_scheduler().await;
    assert!(h.store.list_goals(&[]).await.unwrap().is_empty());
    assert!(
        h.store
            .list_tasks(TaskFilter::default())
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        h.store
            .list_sessions(SessionFilter::default())
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(h.launcher.acp.is_running(&session.id));
}

#[tokio::test]
async fn concurrent_resumes_return_one_row_and_start_one_process() {
    let dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(
        dir.path(),
        outside_script_at(dir.path().to_str().unwrap(), "continue"),
    );
    let h = harness_with_agent("test-agent", &stub.bin).await;
    let (first, second): (SessionDto, SessionDto) = tokio::join!(
        h.json(
            post_json("/v1/outside-sessions/resume", resume_request()),
            axum::http::StatusCode::OK
        ),
        h.json(
            post_json("/v1/outside-sessions/resume", resume_request()),
            axum::http::StatusCode::OK
        ),
    );
    assert_eq!(first.id, second.id);
    assert_eq!(stub.launches_for(&first.id).len(), 1);
    assert_eq!(stub.calls_of("session/load").len(), 1);
}

#[tokio::test]
async fn no_loaded_model_uses_the_agents_default_model() {
    let dir = tempfile::tempdir().unwrap();
    let mut setup = outside_script_at(dir.path().to_str().unwrap(), "continue");
    let stub = stub_acp_agent(dir.path(), setup.clone());
    let h = harness_with_agent("test-agent", &stub.bin).await;
    setup["config_options"] = json!([]);
    stub.reprogram(setup);
    let session: SessionDto = h
        .json(
            post_json("/v1/outside-sessions/resume", resume_request()),
            axum::http::StatusCode::OK,
        )
        .await;
    assert_eq!(session.model, "test-agent:old-model");
    assert!(h.launcher.acp.is_running(&session.id));
}

async fn resume_with_an_old_catalog(load_model: bool) -> SessionDto {
    let dir = tempfile::tempdir().unwrap();
    let mut setup = outside_script_at(dir.path().to_str().unwrap(), "continue");
    setup["agent_info"] = json!({"name": "stub", "version": "1.0"});
    let stub = stub_acp_agent(dir.path(), setup.clone());
    let h = harness_with_agent("test-agent", &stub.bin).await;
    let kept = h.store.list_acp_catalogs().await.unwrap();
    let cached = kept
        .iter()
        .find(|row| row.agent_id == "test-agent")
        .unwrap();
    let mut old: serde_json::Value = serde_json::from_str(&cached.catalog).unwrap();
    old.as_object_mut().unwrap().remove("default_model");
    h.store
        .put_acp_catalog(
            "test-agent",
            &cached.command(),
            &cached.version,
            &old.to_string(),
        )
        .await
        .unwrap();

    // Startup discovery must repair a catalog from before default_model existed,
    // even when the command and agent version have not changed.
    h.launcher.registry.discover().await;
    if load_model {
        setup["config_options"][0]["currentValue"] = json!("loaded-model");
    } else {
        setup["config_options"] = json!([]);
    }
    stub.reprogram(setup);
    let session: SessionDto = h
        .json(
            post_json("/v1/outside-sessions/resume", resume_request()),
            axum::http::StatusCode::OK,
        )
        .await;
    assert!(h.launcher.acp.is_running(&session.id));
    assert_eq!(stub.calls_of("session/load").len(), 1);

    // Keep the repair: the next startup can reuse the completed catalog.
    stub.clear_messages();
    h.launcher.registry.discover().await;
    assert!(stub.calls_of("session/new").is_empty());
    session
}

#[tokio::test]
async fn an_old_catalog_does_not_block_the_loaded_model() {
    let session = resume_with_an_old_catalog(true).await;
    assert_eq!(session.model, "test-agent:loaded-model");
}

#[tokio::test]
async fn an_old_catalog_recovers_the_default_when_load_has_no_model() {
    let session = resume_with_an_old_catalog(false).await;
    assert_eq!(session.model, "test-agent:old-model");
}

#[tokio::test]
async fn a_session_missing_from_the_snapshot_is_refused_after_one_fresh_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(dir.path(), outside_script());
    let h = harness_with_agent("test-agent", &stub.bin).await;
    let _: SessionPageDto = h.get("/v1/sessions?all=true").await;
    stub.clear_messages();
    let error: ErrorBody = h
        .error(
            post_json(
                "/v1/outside-sessions/resume",
                json!({"agent_id": "test-agent", "internal_session_id": "missing"}),
            ),
            axum::http::StatusCode::NOT_FOUND,
        )
        .await;
    assert!(error.error.message.contains("missing"));
    assert_eq!(stub.calls_of("session/list").len(), 1);
}

#[tokio::test]
async fn an_agent_session_cannot_resume_an_outside_session() {
    let h = harness().await;
    let cast = h.cast().await;
    let session = h
        .session(&cast.goal, Some(&cast.task), Seat::Author, &cast.author.id)
        .await;
    let (status, _) = h
        .send(as_session(
            "/v1/outside-sessions/resume",
            &session.id,
            resume_request(),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn the_resume_endpoint_replaces_adoption_in_openapi() {
    let h = harness().await;
    let doc: serde_json::Value = h.get("/api-docs/openapi.json").await;
    assert!(doc["paths"]["/v1/outside-sessions/resume"]["post"].is_object());
    assert!(doc["paths"].get("/v1/outside-sessions/adopt").is_none());
}

#[tokio::test]
async fn a_loose_console_serves_history_takes_input_and_cancels() {
    let dir = tempfile::tempdir().unwrap();
    let mut setup = outside_script_at(dir.path().to_str().unwrap(), "continue");
    setup["load_updates"] = json!([
        {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "outside history"}}
    ]);
    setup["prompts"] = json!([{
        "updates": [{"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "new answer"}}],
        "wait_for": dir.path().join("release")
    }]);
    let stub = stub_acp_agent(dir.path(), setup);
    let h = harness_with_agent("test-agent", &stub.bin).await;
    let session: SessionDto = h
        .json(
            post_json("/v1/outside-sessions/resume", resume_request()),
            axum::http::StatusCode::OK,
        )
        .await;
    let console = format!("/v1/sessions/{}/console", session.id);
    let history: Vec<serde_json::Value> = h.get(&console).await;
    assert!(
        history
            .iter()
            .any(|event| event["payload"]["text"] == "outside history"),
        "{history:?}"
    );
    let mut stream = h.stream(common::get(&format!("{console}/stream"))).await;
    let snapshot = common::expect_sse(&mut stream, "snapshot").await;
    assert!(
        snapshot
            .as_array()
            .unwrap()
            .iter()
            .any(|event| event["payload"]["text"] == "outside history")
    );
    let mut domain = h.stream(common::get("/v1/events/stream")).await;
    common::expect_sse(&mut domain, "heartbeat").await;
    let (status, _) = h
        .send(post_json(
            &format!("{console}/input"),
            json!({"text": "continue here"}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::NO_CONTENT);
    eventually(TIMEOUT, "the console turn", || async {
        dir.path().join("release.reached").exists()
    })
    .await;
    let (status, _) = h.send(common::post(&format!("{console}/cancel"))).await;
    assert_eq!(status, axum::http::StatusCode::NO_CONTENT);
    eventually(TIMEOUT, "the cancelled turn", || async {
        let row: SessionDto = h.get(&format!("/v1/sessions/{}", session.id)).await;
        row.status == SessionStatus::Idle
    })
    .await;
    let delta = common::expect_sse(&mut stream, "event").await;
    assert_eq!(delta["kind"], "user_prompt_submit");
    assert_eq!(delta["payload"]["text"], "continue here");
    let changed = common::expect_sse(&mut domain, "agent_event").await;
    assert_eq!(changed["session_id"], session.id);
    assert!(changed["task_id"].is_null());
    assert!(stub.prompts_for(&session.id)[0].ends_with("continue here"));
    let history: Vec<serde_json::Value> = h.get(&console).await;
    assert!(
        history
            .iter()
            .any(|event| event["payload"]["stop_reason"] == "cancelled")
    );
    let _: SessionDto = h
        .json(
            common::post(&format!("/v1/sessions/{}/kill", session.id)),
            axum::http::StatusCode::OK,
        )
        .await;
    let revived: SessionDto = h
        .json(
            common::post(&format!("/v1/sessions/{}/resume", session.id)),
            axum::http::StatusCode::OK,
        )
        .await;
    assert_eq!(revived.id, session.id);
    assert!(revived.status.is_live());
    let history: Vec<serde_json::Value> = h.get(&console).await;
    assert_eq!(
        history
            .iter()
            .filter(|event| event["payload"]["text"] == "outside history")
            .count(),
        1
    );
}

/// A loose session with no title takes the first prompt typed into it as
/// one, and keeps it: the next prompt does not rename it.
#[tokio::test]
async fn the_first_prompt_typed_into_an_untitled_loose_session_titles_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut setup = outside_script_at(dir.path().to_str().unwrap(), "");
    setup["prompts"] = json!([{"updates": []}, {"updates": []}]);
    let stub = stub_acp_agent(dir.path(), setup);
    let h = harness_with_agent("test-agent", &stub.bin).await;
    let session: SessionDto = h
        .json(
            post_json("/v1/outside-sessions/resume", resume_request()),
            axum::http::StatusCode::OK,
        )
        .await;
    assert_eq!(session.title, None);
    let input = format!("/v1/sessions/{}/console/input", session.id);
    let row = format!("/v1/sessions/{}", session.id);

    let (status, _) = h
        .send(post_json(
            &input,
            json!({"text": "\n  Tidy the release notes  \nand more"}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::NO_CONTENT);
    let titled: SessionDto = h.get(&row).await;
    assert_eq!(titled.title.as_deref(), Some("Tidy the release notes"));

    eventually(TIMEOUT, "the first turn to end", || async {
        let row: SessionDto = h.get(&row).await;
        row.status == SessionStatus::Idle
    })
    .await;
    let (status, _) = h
        .send(post_json(&input, json!({"text": "now the changelog"})))
        .await;
    assert_eq!(status, axum::http::StatusCode::NO_CONTENT);
    let kept: SessionDto = h.get(&row).await;
    assert_eq!(kept.title.as_deref(), Some("Tidy the release notes"));
}
/// A loose session answers its permission requests the way the registered
/// repository its directory lies in does.
#[tokio::test]
async fn a_loose_session_uses_its_repositorys_permission_mode() {
    let dir = tempfile::tempdir().unwrap();
    let mut setup = outside_script_at(dir.path().to_str().unwrap(), "continue");
    setup["prompts"] = json!([{
        "permission": {
            "toolCall": {"toolCallId": "write-1", "title": "Write", "kind": "write"},
            "options": [
                {"optionId": "no", "name": "Reject", "kind": "reject_once"},
                {"optionId": "yes", "name": "Allow", "kind": "allow_once"}
            ]
        },
        "updates": []
    }]);
    let stub = stub_acp_agent(dir.path(), setup);
    let home = home_with_agent("test-agent", &stub.bin);
    let h = harness().scheduler().home(home).discover_agents().await;
    let repo = h.repository(dir.path()).await;
    h.set_permission_mode(&repo, PermissionMode::Ask).await;
    let session: SessionDto = h
        .json(
            post_json("/v1/outside-sessions/resume", resume_request()),
            axum::http::StatusCode::OK,
        )
        .await;
    let input = format!("/v1/sessions/{}/console/input", session.id);
    h.send(post_json(&input, json!({"text": "write it"}))).await;
    eventually(TIMEOUT, "the permission request", || async {
        h.store
            .get_session(&session.id)
            .await
            .unwrap()
            .attention_reason()
            == Some(ariadne_core::AttentionReason::WaitingPermission)
    })
    .await;
    h.flush_scheduler().await;
    assert_eq!(
        h.store
            .get_session(&session.id)
            .await
            .unwrap()
            .attention_reason(),
        Some(ariadne_core::AttentionReason::WaitingPermission)
    );
    assert!(
        stub.messages()
            .iter()
            .all(|message| message.get("method").is_some())
    );
    h.send(post_json(&input, json!({"text": "yes"}))).await;
    eventually(TIMEOUT, "the permission answer", || async {
        stub.messages()
            .iter()
            .any(|message| message["result"]["outcome"]["optionId"] == "yes")
    })
    .await;
}
