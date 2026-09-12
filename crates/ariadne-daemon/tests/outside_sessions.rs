//! The outside-session listing: `GET /v1/outside-sessions` pages a snapshot
//! of every ACP agent's stored sessions, filtered, newest first — and follows
//! an agent's own pages when it takes the snapshot.

mod common;

use axum::http::StatusCode;
use serde_json::{Value, json};

use ariadne_api::sessions::OutsideSessionPageDto;
use ariadne_store::AgentPin;

use common::acp::{StubAcpAgent, script, stub_acp_agent};
use common::{Harness, get, harness, post_json};

/// A home whose `config.toml` registers one ACP agent per `(id, bin)`.
fn home_with_agents(agents: &[(&str, &str)]) -> std::path::PathBuf {
    let home = std::path::Path::new(agents[0].1)
        .parent()
        .unwrap()
        .join("home");
    std::fs::create_dir_all(&home).unwrap();
    let entries: String = agents
        .iter()
        .map(|(id, bin)| format!("[[acp_agents]]\nid = {id:?}\ncommand = [{bin:?}]\n"))
        .collect();
    std::fs::write(home.join("config.toml"), entries).unwrap();
    home
}

/// Five stored sessions, written oldest first so the listing's order is the
/// daemon's and not the agent's: two under `/work/alpha`, one beside it in
/// `/work/alphabet`, one in `/work/beta`, one elsewhere.
fn five_sessions() -> Value {
    json!([
        {"sessionId": "s1", "cwd": "/work/alpha", "title": "Fix the flaky test",
         "updatedAt": "2026-01-01T00:00:00Z"},
        {"sessionId": "s2", "cwd": "/work/alpha/sub", "title": "Add paging",
         "updatedAt": "2026-01-02T00:00:00Z"},
        {"sessionId": "s3", "cwd": "/work/beta", "title": "fix the login",
         "updatedAt": "2026-01-03T00:00:00Z"},
        {"sessionId": "s4", "cwd": "/work/alphabet", "title": "Rename the module",
         "updatedAt": "2026-01-04T00:00:00Z"},
        {"sessionId": "s5", "cwd": "/other", "title": "Write the docs",
         "updatedAt": "2026-01-05T00:00:00Z"},
    ])
}

/// A stub that lists `sessions` — whole, or `page_size` at a time — and can
/// load any of them.
fn listing_script(sessions: Value, page_size: Option<usize>) -> Value {
    let mut setup = script();
    setup["capabilities"] = json!({"loadSession": true, "sessionCapabilities": {"list": {}}});
    setup["stored_sessions"] = Value::Array(
        sessions
            .as_array()
            .unwrap()
            .iter()
            .map(|session| session["sessionId"].clone())
            .collect(),
    );
    setup["session_list"] = sessions;
    if let Some(size) = page_size {
        setup["session_page_size"] = json!(size);
    }
    setup
}

async fn harness_with(stub: &StubAcpAgent) -> Harness {
    harness()
        .home(home_with_agents(&[("test-agent", &stub.bin)]))
        .discover_agents()
        .await
}

/// One listing request, `query` being what follows the `?`.
async fn listing(h: &Harness, query: &str) -> OutsideSessionPageDto {
    h.get(&format!("/v1/outside-sessions?{query}")).await
}

fn ids(page: &OutsideSessionPageDto) -> Vec<&str> {
    page.sessions
        .iter()
        .map(|session| session.internal_session_id.as_str())
        .collect()
}

/// An agent that answers its list two sessions at a time, over three pages,
/// still has every one of its sessions in the listing: the daemon follows
/// each `nextCursor` it is given.
#[tokio::test]
async fn a_paging_agents_every_session_is_in_the_listing() {
    let dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(dir.path(), listing_script(five_sessions(), Some(2)));
    let h = harness_with(&stub).await;

    let page = listing(&h, "").await;

    assert_eq!(ids(&page), ["s5", "s4", "s3", "s2", "s1"]);
    let cursors: Vec<Value> = stub
        .calls_of("session/list")
        .iter()
        .map(|call| call.get("cursor").cloned().unwrap_or(Value::Null))
        .collect();
    assert_eq!(cursors, [Value::Null, json!("2"), json!("4")]);
}

/// An agent that answers its first page and never the second is listed with
/// what arrived: the 5 s budget ends the listing, and the pages inside it
/// are kept rather than dropped with the timeout.
#[tokio::test]
async fn the_pages_that_arrived_are_kept_when_an_agents_budget_ends() {
    let dir = tempfile::tempdir().unwrap();
    let mut setup = listing_script(five_sessions(), Some(2));
    setup["session_list_stall_from"] = json!(2);
    let stub = stub_acp_agent(dir.path(), setup);
    let h = harness_with(&stub).await;

    let page = listing(&h, "").await;

    assert_eq!(ids(&page), ["s2", "s1"]);
    assert_eq!(page.total, 2);
}

/// A cursor cut from one snapshot continues from the same row after a
/// refresh: it names the sort key of the last row, not an offset into the
/// snapshot it came from. A session newer than every row of the first page
/// appears at the agent in between, so an offset would slip by one and
/// answer `s4` again.
#[tokio::test]
async fn a_cursor_continues_from_the_same_row_after_a_refresh() {
    let dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(dir.path(), listing_script(five_sessions(), None));
    let h = harness_with(&stub).await;

    let first = listing(&h, "limit=2").await;
    assert_eq!(ids(&first), ["s5", "s4"]);
    let cursor = first.next_cursor.clone().expect("a second page");

    let mut grown = five_sessions();
    grown.as_array_mut().unwrap().push(json!({
        "sessionId": "s6", "cwd": "/work/gamma", "title": "Newest of all",
        "updatedAt": "2026-01-06T00:00:00Z",
    }));
    stub.reprogram(listing_script(grown, None));
    let second = listing(&h, &format!("limit=2&refresh=true&cursor={cursor}")).await;

    assert_ne!(second.snapshot_at, first.snapshot_at);
    assert_eq!(stub.calls_of("session/list").len(), 2);
    assert_eq!(second.total, 6);
    assert_eq!(ids(&second), ["s3", "s2"]);
}

/// Five sessions at `limit=2` are three pages through `next_cursor`, newest
/// first, each saying `total=5`, the last with no cursor.
#[tokio::test]
async fn five_sessions_at_limit_two_are_three_pages_newest_first() {
    let dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(dir.path(), listing_script(five_sessions(), None));
    let h = harness_with(&stub).await;

    let first = listing(&h, "limit=2").await;
    assert_eq!(ids(&first), ["s5", "s4"]);
    assert_eq!(first.total, 5);
    let cursor = first.next_cursor.clone().expect("a second page");

    let second = listing(&h, &format!("limit=2&cursor={cursor}")).await;
    assert_eq!(ids(&second), ["s3", "s2"]);
    assert_eq!(second.total, 5);
    let cursor = second.next_cursor.clone().expect("a third page");

    let third = listing(&h, &format!("limit=2&cursor={cursor}")).await;
    assert_eq!(ids(&third), ["s1"]);
    assert_eq!(third.total, 5);
    assert_eq!(third.next_cursor, None);
}

/// `agent` keeps the sessions of that registry agent alone.
#[tokio::test]
async fn agent_narrows_the_listing_to_one_agents_sessions() {
    let dir = tempfile::tempdir().unwrap();
    let (one_dir, other_dir) = (dir.path().join("one"), dir.path().join("other"));
    std::fs::create_dir_all(&one_dir).unwrap();
    std::fs::create_dir_all(&other_dir).unwrap();
    let one = stub_acp_agent(&one_dir, listing_script(five_sessions(), None));
    let other_sessions = json!([{"sessionId": "o1", "cwd": "/work/other", "title": "Other",
                                 "updatedAt": "2026-01-06T00:00:00Z"}]);
    let other = stub_acp_agent(&other_dir, listing_script(other_sessions, None));
    let h = harness()
        .home(home_with_agents(&[
            ("test-agent", &one.bin),
            ("other-agent", &other.bin),
        ]))
        .discover_agents()
        .await;

    let all = listing(&h, "").await;
    assert_eq!(all.total, 6);
    let page = listing(&h, "agent=other-agent").await;
    assert_eq!(ids(&page), ["o1"]);
    assert_eq!(page.total, 1);
}

/// `dir` keeps the sessions whose working directory is the path or a
/// directory under it — not a sibling that merely shares its prefix.
#[tokio::test]
async fn dir_narrows_the_listing_to_a_path_and_what_is_under_it() {
    let dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(dir.path(), listing_script(five_sessions(), None));
    let h = harness_with(&stub).await;

    let page = listing(&h, "dir=/work/alpha").await;

    assert_eq!(ids(&page), ["s2", "s1"]);
    assert_eq!(page.total, 2);
}

/// `since` keeps the sessions last active at or after the moment.
#[tokio::test]
async fn since_narrows_the_listing_to_activity_at_or_after_it() {
    let dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(dir.path(), listing_script(five_sessions(), None));
    let h = harness_with(&stub).await;

    let page = listing(&h, "since=2026-01-03T00:00:00Z").await;

    assert_eq!(ids(&page), ["s5", "s4", "s3"]);
}

/// `until` keeps the sessions last active at or before the moment.
#[tokio::test]
async fn until_narrows_the_listing_to_activity_at_or_before_it() {
    let dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(dir.path(), listing_script(five_sessions(), None));
    let h = harness_with(&stub).await;

    let page = listing(&h, "until=2026-01-02T00:00:00Z").await;

    assert_eq!(ids(&page), ["s2", "s1"]);
}

/// `q` keeps the sessions whose first prompt contains the text, whatever
/// the case of either.
#[tokio::test]
async fn q_narrows_the_listing_by_first_prompt_case_insensitively() {
    let dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(dir.path(), listing_script(five_sessions(), None));
    let h = harness_with(&stub).await;

    let page = listing(&h, "q=FIX").await;

    assert_eq!(ids(&page), ["s3", "s1"]);
}

/// The snapshot serves a second request inside a minute without asking any
/// agent again; `refresh=true` asks them at once.
#[tokio::test]
async fn a_second_request_asks_no_agent_again_but_a_refresh_does() {
    let dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(dir.path(), listing_script(five_sessions(), None));
    let h = harness_with(&stub).await;

    let first = listing(&h, "").await;
    assert_eq!(stub.calls_of("session/list").len(), 1);

    let second = listing(&h, "limit=2").await;
    assert_eq!(stub.calls_of("session/list").len(), 1);
    assert_eq!(second.snapshot_at, first.snapshot_at);

    listing(&h, "refresh=true").await;
    assert_eq!(stub.calls_of("session/list").len(), 2);
}

/// A session adopted after the snapshot was taken is gone from the next
/// page without a refresh: what a row already holds is subtracted at query
/// time, not at snapshot time.
#[tokio::test]
async fn a_session_adopted_after_the_snapshot_is_absent_without_a_refresh() {
    let dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(dir.path(), listing_script(five_sessions(), None));
    let h = harness_with(&stub).await;
    let repo = h.repository(&h.git_repo("author-repo")).await;
    let pin = AgentPin {
        model: "test-agent:old-model".into(),
        effort: None,
    };
    let goal = h.goal_on(&repo, pin).await;
    let goal = h.activate(&goal).await;

    let before = listing(&h, "").await;
    assert!(ids(&before).contains(&"s3"));

    h.json::<Value>(
        post_json(
            "/v1/outside-sessions/adopt",
            json!({
                "agent_id": "test-agent",
                "internal_session_id": "s3",
                "goal": {"id": goal.id},
                "agents": [{"seat": "author", "model": "test-agent:old-model"}],
            }),
        ),
        StatusCode::CREATED,
    )
    .await;
    stub.clear_messages();

    let after = listing(&h, "").await;

    assert_eq!(ids(&after), ["s5", "s4", "s2", "s1"]);
    assert_eq!(after.total, 4);
    assert_eq!(after.snapshot_at, before.snapshot_at);
    assert!(stub.calls_of("session/list").is_empty());
}

/// A cursor the daemon did not write is refused as `invalid_cursor`.
#[tokio::test]
async fn an_unreadable_cursor_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(dir.path(), listing_script(five_sessions(), None));
    let h = harness_with(&stub).await;

    let error = h
        .error(
            get("/v1/outside-sessions?cursor=not-a-cursor"),
            StatusCode::BAD_REQUEST,
        )
        .await;

    assert_eq!(error.error.code, "invalid_cursor");
}

/// The query parameters and the page DTO are in the OpenAPI document.
#[tokio::test]
async fn the_query_and_the_page_are_in_the_openapi_document() {
    let h = harness().await;

    let doc: Value = h.get("/api-docs/openapi.json").await;

    let operation = &doc["paths"]["/v1/outside-sessions"]["get"];
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
            "agent", "cursor", "dir", "limit", "q", "refresh", "since", "until"
        ]
    );
    assert_eq!(
        operation["responses"]["200"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/OutsideSessionPageDto"
    );
    let page = &doc["components"]["schemas"]["OutsideSessionPageDto"];
    let mut fields: Vec<&str> = page["properties"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    fields.sort_unstable();
    assert_eq!(fields, ["next_cursor", "sessions", "snapshot_at", "total"]);
}
