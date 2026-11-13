//! Session pages contain only Ariadne work.
use crate::common;
use ariadne_api::sessions::{SessionEntryDto, SessionPageDto};
use ariadne_core::{AttentionReason, Seat, SessionStatus};
use ariadne_store::NewSession;
use axum::http::StatusCode;
use common::{Harness, get, harness};
use serde_json::Value;
/// One listing request, `query` being what follows the `?`.
async fn listing(h: &Harness, query: &str) -> SessionPageDto {
    h.get(&format!("/v1/sessions?{query}")).await
}

fn ids(page: &SessionPageDto) -> Vec<&str> {
    page.sessions
        .iter()
        .map(|session| session.id.as_str())
        .collect()
}

/// The one row of a page that answers a single session.
fn only(page: &SessionPageDto) -> &SessionEntryDto {
    assert_eq!(page.total, 1, "{:?}", ids(page));
    &page.sessions[0]
}

/// A directory named after a title: no space in it, since a path with one
/// cannot go in a URI.
fn slug(title: &str) -> String {
    title.replace(' ', "-")
}

/// One Ariadne session of its own, on a goal titled `title` and a model of
/// `agent`: the row the listing reads a goal, an agent, a title and a
/// directory off. Its worktree is `slug(title)` under the harness.
async fn ariadne_session(h: &Harness, title: &str, agent: &str) -> ariadne_store::AgentSession {
    let repo = h.repository(&h.at(&format!("repo-{}", slug(title)))).await;
    let goal = h
        .store
        .create_goal(ariadne_store::NewGoal {
            workflow: None,
            issue_url: None,
            title: title.into(),
            description: "desc".into(),
            repository_ids: vec![repo.id.clone()],
            pin: common::test_pin(),
        })
        .await
        .unwrap();
    h.store
        .create_session(NewSession {
            goal_id: Some(goal.id.clone()),
            task_id: None,
            seat: Some(Seat::Orchestrator),
            task_agent_id: None,
            model: format!("{agent}:test-model"),
            effort: None,
            worktree_path: Some(h.at(&slug(title)).display().to_string()),
            pull_request_id: None,
        })
        .await
        .unwrap()
}

#[tokio::test]
async fn dir_narrows_the_listing_by_an_ariadne_sessions_worktree() {
    let h = harness().await;
    let session = ariadne_session(&h, "Ship the UI", "stub").await;
    ariadne_session(&h, "Ship the API", "stub").await;

    let page = listing(&h, &format!("dir={}", h.at(&slug("Ship the UI")).display())).await;

    assert_eq!(ids(&page), [session.id.as_str()]);
}

#[tokio::test]
async fn task_narrows_the_listing_to_one_tasks_sessions() {
    let h = harness().await;
    let cast = h.cast().await;
    let agent = h.agent_session(&cast, "develop").await;
    h.orchestrator_session(&cast.goal).await;

    let page = listing(&h, &format!("task={}", cast.task.id)).await;

    assert_eq!(ids(&page), [agent.id.as_str()]);
    assert_eq!(only(&page).title.as_deref(), Some(cast.task.title.as_str()));
}

#[tokio::test]
async fn seat_narrows_the_listing_to_one_seat() {
    let h = harness().await;
    let cast = h.cast().await;
    let agent = h.agent_session(&cast, "develop").await;
    h.orchestrator_session(&cast.goal).await;

    let page = listing(&h, "seat=agent").await;

    assert_eq!(ids(&page), [agent.id.as_str()]);
    assert_eq!(only(&page).seat, Some(Seat::Agent));
}

#[tokio::test]
async fn status_narrows_the_listing_to_one_status() {
    let h = harness().await;
    let live = ariadne_session(&h, "Ship the UI", "stub").await;
    let ended = ariadne_session(&h, "Ship the API", "stub").await;
    h.set_status(&ended, SessionStatus::Exited).await;

    let page = listing(&h, "status=exited").await;
    assert_eq!(ids(&page), [ended.id.as_str()]);
    assert_eq!(only(&page).status, Some(SessionStatus::Exited));

    let live_page = listing(&h, "status=starting").await;
    assert_eq!(ids(&live_page), [live.id.as_str()]);
}

#[tokio::test]
async fn attention_narrows_the_listing_to_the_sessions_waiting_on_somebody() {
    let h = harness().await;
    let waiting = ariadne_session(&h, "Ship the UI", "stub").await;
    ariadne_session(&h, "Ship the API", "stub").await;
    h.raise(&waiting, AttentionReason::WaitingInput).await;

    let page = listing(&h, "attention=true").await;

    assert_eq!(ids(&page), [waiting.id.as_str()]);
    assert_eq!(
        only(&page).attention_reason,
        Some(AttentionReason::WaitingInput)
    );
}

#[tokio::test]
async fn all_lists_an_ended_ariadne_session() {
    let h = harness().await;
    let live = ariadne_session(&h, "Ship the UI", "stub").await;
    let ended = ariadne_session(&h, "Ship the API", "stub").await;
    h.set_status(&ended, SessionStatus::Exited).await;

    let page = listing(&h, "").await;
    assert_eq!(ids(&page), [live.id.as_str()]);

    let all = listing(&h, "all=true").await;

    assert_eq!(all.total, 2);
    assert!(ids(&all).contains(&ended.id.as_str()));
}

#[tokio::test]
async fn an_unreadable_cursor_is_refused() {
    let h = harness().await;

    let error = h
        .error(
            get("/v1/sessions?cursor=not-a-cursor"),
            StatusCode::BAD_REQUEST,
        )
        .await;

    assert_eq!(error.error.code, "invalid_cursor");
}

#[tokio::test]
async fn the_query_and_the_page_are_in_the_openapi_document() {
    let h = harness().await;

    let doc: Value = h.get("/api-docs/openapi.json").await;

    let operation = &doc["paths"]["/v1/sessions"]["get"];
    let mut names: Vec<&str> = operation["parameters"]
        .as_array()
        .unwrap()
        .iter()
        .map(|parameter| parameter["name"].as_str().unwrap())
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "agent",
            "all",
            "attention",
            "cursor",
            "dir",
            "goal",
            "limit",
            "pull_request",
            "q",
            "seat",
            "since",
            "status",
            "task",
            "until"
        ]
    );
    assert_eq!(
        operation["responses"]["200"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/SessionPageDto"
    );
    let page = &doc["components"]["schemas"]["SessionPageDto"];
    let mut fields: Vec<&str> = page["properties"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    fields.sort_unstable();
    assert_eq!(fields, ["next_cursor", "sessions", "total"]);
    assert!(doc["paths"]["/v1/sessions"]["post"].is_null());
    assert!(doc["paths"]["/v1/outside-sessions/resume"].is_null());
    let schemas = doc["components"]["schemas"].as_object().unwrap();
    for name in [
        "OutsideSessionDto",
        "NewSessionRequest",
        "ResumeOutsideSessionRequest",
        "SessionKind",
    ] {
        assert!(!schemas.contains_key(name));
    }
    assert!(schemas["SessionDto"]["properties"].get("title").is_none());
}

#[tokio::test]
async fn five_sessions_at_limit_two_are_three_pages_newest_first() {
    let h = harness().await;
    let mut expected = Vec::new();
    for i in 0..5 {
        let session = ariadne_session(&h, &format!("Session {i}"), "stub").await;
        h.idle_for(&session, (5 - i) * 3600).await;
        expected.insert(0, session.id);
    }
    let mut actual = Vec::new();
    let mut query = "limit=2".to_string();
    let mut sizes = Vec::new();
    loop {
        let page = listing(&h, &query).await;
        assert_eq!(page.total, 5);
        sizes.push(page.sessions.len());
        actual.extend(page.sessions.into_iter().map(|row| row.id));
        match page.next_cursor {
            Some(cursor) => query = format!("limit=2&cursor={cursor}"),
            None => break,
        }
    }
    assert_eq!(sizes, [2, 2, 1]);
    assert_eq!(actual, expected);
}

#[tokio::test]
async fn agent_goal_directory_and_title_narrow_ariadne_sessions() {
    let h = harness().await;
    let wanted = ariadne_session(&h, "Fix Login", "one").await;
    ariadne_session(&h, "Ship docs", "two").await;
    for query in [
        "agent=one".to_string(),
        "q=LOGIN".to_string(),
        format!("goal={}", wanted.goal_id.as_deref().unwrap()),
        format!("dir={}", wanted.worktree_path.as_deref().unwrap()),
    ] {
        let page = listing(&h, &query).await;
        assert_eq!(ids(&page), [wanted.id.as_str()], "{query}");
    }
}

#[tokio::test]
async fn the_default_page_holds_the_last_seven_days_and_all_widens_it() {
    let h = harness().await;
    let recent = ariadne_session(&h, "Recent", "stub").await;
    let old = ariadne_session(&h, "Old", "stub").await;
    h.idle_for(&old, 8 * 24 * 3600).await;
    assert_eq!(ids(&listing(&h, "").await), [recent.id.as_str()]);
    assert_eq!(listing(&h, "all=true").await.total, 2);
    assert_eq!(listing(&h, "since=2000-01-01T00:00:00Z").await.total, 2);
    assert_eq!(listing(&h, "until=2000-01-01T00:00:00Z").await.total, 0);
}

#[tokio::test]
async fn removed_session_creation_routes_refuse_posts() {
    let h = harness().await;
    for (path, expected) in [
        ("/v1/sessions", StatusCode::METHOD_NOT_ALLOWED),
        ("/v1/outside-sessions/resume", StatusCode::NOT_FOUND),
    ] {
        let (status, _) = h.send(common::post_json(path, serde_json::json!({}))).await;
        assert_eq!(status, expected);
    }
}

#[tokio::test]
async fn cursor_preserves_precision_and_breaks_ties_by_agent_then_session() {
    let h = harness().await;
    let cast = h.cast().await;
    let db = sqlx::SqlitePool::connect(&format!("sqlite://{}", h.at("test.db").display()))
        .await
        .unwrap();
    for (id, agent, at, dir) in [
        ("z", "b", "2026-01-01T00:00:00.001500Z", "/work/alphabet"),
        ("b", "a", "2026-01-01T00:00:00.001500Z", "/work/alpha/sub"),
        ("a", "a", "2026-01-01T00:00:00.001500Z", "/work/alpha"),
        ("older", "a", "2026-01-01T00:00:00.001200Z", "/work/alpha"),
    ] {
        sqlx::query("INSERT INTO agent_sessions (id, goal_id, seat, model, status, created_at, last_activity_at, worktree_path) VALUES (?, ?, 'orchestrator', ?, 'idle', ?, ?, ?)")
            .bind(id).bind(&cast.goal.id).bind(format!("{agent}:model")).bind(at).bind(at).bind(dir).execute(&db).await.unwrap();
    }
    db.close().await;
    let mut actual = Vec::new();
    let mut query = "all=true&limit=1".to_string();
    loop {
        let page = listing(&h, &query).await;
        actual.extend(page.sessions.into_iter().map(|row| row.id));
        match page.next_cursor {
            Some(cursor) => query = format!("all=true&limit=1&cursor={cursor}"),
            None => break,
        }
    }
    assert_eq!(actual, ["a", "b", "z", "older"]);
    let bounded = listing(&h, "all=true&dir=/work/alpha&since=2026-01-01T00:00:00.001500Z&until=2026-01-01T00:00:00.001500Z").await;
    assert_eq!(ids(&bounded), ["a", "b"]);
}
