//! Discover sessions from CLI stores and resume one as a task author.

mod common;

use std::path::{Path, PathBuf};

use ariadne_api::sessions::{OutsideSessionDto, SessionDto};
use ariadne_core::{Actor, AgentKind, Seat, TaskStatus};
use sqlx::Connection;

use common::{as_session, harness, post_json, test_pin};

const CLAUDE: &str = include_str!("fixtures/outside-sessions/claude.jsonl");
const CODEX: &str = include_str!("fixtures/outside-sessions/codex.jsonl");
const OPENCODE: &str = include_str!("fixtures/outside-sessions/opencode.sql");

/// Build the three transcript stores from captured-format fixtures.
async fn transcript_home(root: &Path) -> PathBuf {
    let claude = root.join(".claude/projects/project/claude-outside.jsonl");
    let codex = root.join(".codex/sessions/2026/09/02/rollout-codex-outside.jsonl");
    std::fs::create_dir_all(claude.parent().unwrap()).unwrap();
    std::fs::create_dir_all(codex.parent().unwrap()).unwrap();
    std::fs::write(claude, CLAUDE).unwrap();
    std::fs::write(codex, CODEX).unwrap();

    let db = root.join(".local/share/opencode/opencode.db");
    std::fs::create_dir_all(db.parent().unwrap()).unwrap();
    let mut connection = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&db)
            .create_if_missing(true),
    )
    .await
    .unwrap();
    sqlx::raw_sql(OPENCODE)
        .execute(&mut connection)
        .await
        .unwrap();
    root.to_path_buf()
}

#[tokio::test]
async fn hand_started_sessions_from_each_cli_appear_in_the_listing() {
    let root = tempfile::tempdir().unwrap();
    let home = transcript_home(root.path()).await;
    let h = harness().agent_home(home).await;

    let sessions: Vec<OutsideSessionDto> = h.get("/v1/outside-sessions").await;

    assert_eq!(sessions.len(), 3);
    assert!(sessions.iter().any(|session| {
        session.agent_kind == AgentKind::ClaudeCode
            && session.internal_session_id == "claude-outside"
            && session.working_directory == "/work/claude"
            && session.first_prompt == "Inspect the release workflow."
    }));
    assert!(sessions.iter().any(|session| {
        session.agent_kind == AgentKind::Codex
            && session.internal_session_id == "codex-outside"
            && session.working_directory == "/work/codex"
            && session.first_prompt == "Add the API endpoint."
    }));
    assert!(sessions.iter().any(|session| {
        session.agent_kind == AgentKind::Opencode
            && session.internal_session_id == "opencode-outside"
            && session.working_directory == "/work/opencode"
            && session.first_prompt == "Write the integration test."
    }));
}

#[tokio::test]
async fn a_session_ariadne_started_does_not_appear_in_the_listing() {
    let root = tempfile::tempdir().unwrap();
    let home = transcript_home(root.path()).await;
    let h = harness().agent_home(home).await;
    let cast = h.cast().await;
    let started = h
        .session(&cast.goal, Some(&cast.task), Seat::Author, &cast.author.id)
        .await;
    h.store
        .set_session_internal_id(&started.id, "claude-outside")
        .await
        .unwrap();

    let sessions: Vec<OutsideSessionDto> = h.get("/v1/outside-sessions").await;

    assert!(
        sessions
            .iter()
            .all(|session| session.internal_session_id != "claude-outside")
    );
}

#[tokio::test]
async fn an_adopted_session_authors_the_task_through_review() {
    let root = tempfile::tempdir().unwrap();
    let home = transcript_home(root.path()).await;
    let h = harness().agent_home(home).await;
    let repo = h.repository(&h.git_repo("author-repo")).await;
    let goal = h.goal_on(&repo, test_pin(AgentKind::ClaudeCode)).await;
    let task = h
        .task_on(
            &goal,
            &repo,
            "adopted task",
            1,
            test_pin(AgentKind::ClaudeCode),
        )
        .await;
    let dependency = h
        .task_on(
            &goal,
            &repo,
            "finish setup",
            0,
            test_pin(AgentKind::ClaudeCode),
        )
        .await;
    h.store
        .set_task_dependencies(&task.id, std::slice::from_ref(&dependency.id))
        .await
        .unwrap();
    h.advance(&task, TaskStatus::Ready).await;

    let session: SessionDto = h
        .json(
            post_json(
                &format!("/v1/tasks/{}/author-session", task.id),
                serde_json::json!({
                    "agent_kind": "claude_code",
                    "internal_session_id": "claude-outside",
                }),
            ),
            axum::http::StatusCode::OK,
        )
        .await;

    assert_eq!(session.task_id.as_deref(), Some(task.id.as_str()));
    assert_eq!(session.seat, Seat::Author);
    assert_eq!(
        session.internal_session_id.as_deref(),
        Some("claude-outside")
    );
    assert!(
        session
            .worktree_path
            .as_deref()
            .is_some_and(|path| Path::new(path).is_dir())
    );
    assert!(
        h.spawn_argv(&session.id)
            .contains("--resume claude-outside")
    );
    assert!(
        h.spawn_argv(&session.id).contains(&format!(
            "- {} ({}, branch {})",
            dependency.title, dependency.status, dependency.branch
        )),
        "the adopted author receives its dependencies"
    );

    h.store
        .transition_task(&task.id, TaskStatus::InProgress, Actor::Daemon, None, None)
        .await
        .unwrap();
    h.store
        .transition_task(&task.id, TaskStatus::UnderReview, Actor::Author, None, None)
        .await
        .unwrap();
    assert_eq!(
        h.store.get_task(&task.id).await.unwrap().status(),
        TaskStatus::UnderReview
    );
}

#[tokio::test]
async fn an_agent_cannot_adopt_a_session_for_another_task() {
    let root = tempfile::tempdir().unwrap();
    let home = transcript_home(root.path()).await;
    let h = harness().agent_home(home).await;
    let repo = h.repository(&h.git_repo("author-repo")).await;
    let goal = h.goal_on(&repo, test_pin(AgentKind::ClaudeCode)).await;
    let target = h
        .task_on(
            &goal,
            &repo,
            "target task",
            1,
            test_pin(AgentKind::ClaudeCode),
        )
        .await;
    let other = h
        .task_on(
            &goal,
            &repo,
            "other task",
            1,
            test_pin(AgentKind::ClaudeCode),
        )
        .await;
    let other_author = h.store.task_author(&other.id).await.unwrap();
    let other_session = h
        .session(&goal, Some(&other), Seat::Author, &other_author.id)
        .await;
    h.advance(&target, TaskStatus::Ready).await;

    let (status, _) = h
        .send(as_session(
            &format!("/v1/tasks/{}/author-session", target.id),
            &other_session.id,
            serde_json::json!({
                "agent_kind": "claude_code",
                "internal_session_id": "claude-outside",
            }),
        ))
        .await;

    assert_eq!(status, axum::http::StatusCode::FORBIDDEN);
}
