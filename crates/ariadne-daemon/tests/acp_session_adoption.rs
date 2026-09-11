//! Session discovery and adoption over ACP: the stored sessions of a
//! registered ACP agent, listed through `session/list` and adopted through
//! `session/load` — the ACP counterpart of `session_adoption.rs`, which
//! covers the CLI transcript stores.

mod common;

use serde_json::json;

use ariadne_api::sessions::{AdoptOutsideSessionRequest, OutsideSessionDto};
use ariadne_core::{AgentKind, SessionStatus, TaskStatus};
use ariadne_store::AgentPin;

use common::acp::{script, stub_acp_agent};
use common::{Harness, TIMEOUT, eventually, harness, post_json};

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

async fn harness_with_agent(id: &str, bin: &str) -> Harness {
    let home = home_with_agent(id, bin);
    // No CLI transcripts live under this fixture home: `discover` would
    // otherwise fall back to scanning the real user's, which is slow and not
    // what these tests are about.
    let agent_home = home.join("no-transcripts");
    std::fs::create_dir_all(&agent_home).unwrap();
    harness()
        .home(home)
        .agent_home(agent_home)
        .acp_bin(bin)
        .discover_agents()
        .await
}

/// A ready task whose author is pinned to `<agent_id>:old-model`, the model
/// the stub reports.
async fn ready_task(h: &Harness, agent_id: &str) -> ariadne_store::Task {
    let repo = h.repository(&h.git_repo("author-repo")).await;
    let pin = AgentPin {
        agent_kind: AgentKind::Acp,
        model: format!("{agent_id}:old-model"),
        effort: None,
    };
    let goal = h.goal_on(&repo, pin.clone()).await;
    let task = h.task_on(&goal, &repo, "adopted acp task", 1, pin).await;
    h.advance(&task, TaskStatus::Ready).await;
    task
}

/// The stub agent's stored sessions appear in `GET /v1/outside-sessions`,
/// named by the registry agent they belong to.
#[tokio::test]
async fn an_acp_agents_stored_sessions_appear_in_the_listing() {
    let dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(dir.path(), adoptable_script());
    let h = harness_with_agent("test-agent", &stub.bin).await;

    let sessions: Vec<OutsideSessionDto> = h.get("/v1/outside-sessions").await;

    let found = sessions
        .iter()
        .find(|session| session.internal_session_id == "outside-1")
        .unwrap_or_else(|| panic!("outside-1 not in {sessions:#?}"));
    assert_eq!(found.agent_kind, AgentKind::Acp);
    assert_eq!(found.agent_id.as_deref(), Some("test-agent"));
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

    let sessions: Vec<OutsideSessionDto> = h.get("/v1/outside-sessions").await;
    assert!(
        sessions
            .iter()
            .all(|session| session.agent_id.as_deref() != Some("no-listing"))
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

/// Adopting a listed session binds it to the task's author seat, resumes it
/// through `session/load` (never `session/resume`, which this agent does not
/// advertise), and a follow-up console prompt reaches the same agent.
#[tokio::test]
async fn an_adopted_session_binds_the_seat_and_a_follow_up_prompt_reaches_it() {
    let dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(dir.path(), adoptable_script());
    let h = harness_with_agent("test-agent", &stub.bin).await;
    let task = ready_task(&h, "test-agent").await;

    let session: ariadne_api::sessions::SessionDto = h
        .json(
            post_json(
                &format!("/v1/tasks/{}/author-session", task.id),
                serde_json::to_value(AdoptOutsideSessionRequest {
                    agent_kind: AgentKind::Acp,
                    agent_id: Some("test-agent".into()),
                    internal_session_id: "outside-1".into(),
                })
                .unwrap(),
            ),
            axum::http::StatusCode::OK,
        )
        .await;

    assert_eq!(session.task_id.as_deref(), Some(task.id.as_str()));
    assert_eq!(session.internal_session_id.as_deref(), Some("outside-1"));
    assert_eq!(
        h.tmux_calls_of("new-session"),
        Vec::<String>::new(),
        "an acp author claims no pane"
    );

    eventually(TIMEOUT, "the adoption's briefing turn to end", || async {
        h.store.get_session(&session.id).await.unwrap().status() == SessionStatus::Idle
    })
    .await;

    // Continued through session/load, since this agent advertises no resume
    // capability at all.
    assert!(stub.methods().contains(&"session/load".to_string()));
    assert!(!stub.methods().contains(&"session/resume".to_string()));
    assert_eq!(stub.calls_of("session/load")[0]["sessionId"], "outside-1");

    // A follow-up prompt sent through the console reaches the same agent.
    let (status, _) = h
        .send(post_json(
            &format!("/v1/sessions/{}/console/input", session.id),
            serde_json::json!({"text": "keep going"}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::NO_CONTENT);

    // Discovery's own probe (`discover_agents`) shares this stub's log, so the
    // console prompt is found by its content rather than by position.
    eventually(
        TIMEOUT,
        "the follow-up prompt to reach the agent",
        || async {
            stub.calls_of("session/prompt").iter().any(|call| {
                call["prompt"][0]["text"]
                    .as_str()
                    .is_some_and(|text| text.contains("keep going"))
            })
        },
    )
    .await;
}

/// A session already bound to an Ariadne row is not listed as outside again,
/// the same way a hand-started CLI session is not once Ariadne adopts it.
#[tokio::test]
async fn an_adopted_acp_session_no_longer_appears_in_the_listing() {
    let dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(dir.path(), adoptable_script());
    let h = harness_with_agent("test-agent", &stub.bin).await;
    let task = ready_task(&h, "test-agent").await;

    h.json::<ariadne_api::sessions::SessionDto>(
        post_json(
            &format!("/v1/tasks/{}/author-session", task.id),
            serde_json::to_value(AdoptOutsideSessionRequest {
                agent_kind: AgentKind::Acp,
                agent_id: Some("test-agent".into()),
                internal_session_id: "outside-1".into(),
            })
            .unwrap(),
        ),
        axum::http::StatusCode::OK,
    )
    .await;

    let sessions: Vec<OutsideSessionDto> = h.get("/v1/outside-sessions").await;
    assert!(
        sessions
            .iter()
            .all(|session| session.internal_session_id != "outside-1"),
        "{sessions:#?}"
    );
}

/// Adoption is refused for a session belonging to a different ACP agent than
/// the one the task's author is pinned to.
#[tokio::test]
async fn adoption_is_refused_across_acp_agents() {
    let dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(dir.path(), adoptable_script());
    let h = harness_with_agent("test-agent", &stub.bin).await;
    let task = ready_task(&h, "other-agent").await;

    let (status, _) = h
        .send(post_json(
            &format!("/v1/tasks/{}/author-session", task.id),
            serde_json::json!({
                "agent_kind": "acp",
                "agent_id": "test-agent",
                "internal_session_id": "outside-1",
            }),
        ))
        .await;

    assert_eq!(status, axum::http::StatusCode::CONFLICT);
}
