//! The conversations on disk: `GET /v1/sessions` lists an OpenCode session
//! that `session/list` never answers with, and its row carries the model and
//! the tokens `opencode.db` holds for it.
//!
//! OpenCode answers `session/list` with its newest 100 root sessions of the
//! directory it runs in and no further page, so this reader goes straight to
//! `opencode.db` instead. The database here is written by hand with `sqlx`,
//! so no test reads the database of the machine it runs on. The stub agent
//! is registered as `opencode-acp`, which is the agent the disk reader
//! answers for.

use crate::common;
use crate::session_list::{home_with_agents, hours_before, listing_script};

use std::path::Path;

use chrono::{DateTime, TimeDelta, Utc};
use serde_json::{Value, json};
use sqlx::Executor;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

use ariadne_api::sessions::{SessionEntryDto, SessionKind, SessionPageDto};

use common::acp::{StubAcpAgent, discovery_accepted, stub_acp_agent};
use common::{Harness, harness};

/// The agent whose database the disk reader answers for.
const OPENCODE: &str = "opencode-acp";

/// A harness whose one registry agent is `stub`, under the id the disk reader
/// is registered by.
async fn harness_with(stub: &StubAcpAgent) -> Harness {
    let h = harness()
        .home(home_with_agents(&[(OPENCODE, &stub.bin)]))
        .discover_agents()
        .await;
    discovery_accepted(&h, stub, OPENCODE).await;
    h
}

/// A stub that answers `session/list` with `sessions` and nothing else.
fn stub_listing(dir: &Path, sessions: Value) -> StubAcpAgent {
    stub_acp_agent(dir, listing_script(sessions, None))
}

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

/// The row of a page with that id.
fn row<'a>(page: &'a SessionPageDto, id: &str) -> &'a SessionEntryDto {
    page.sessions
        .iter()
        .find(|session| session.id == id)
        .unwrap_or_else(|| panic!("{id} is not in the page: {:?}", ids(page)))
}

/// This harness's `opencode.db`, created and its schema laid down, ready for
/// a test to write sessions and messages into.
async fn db(h: &Harness) -> sqlx::SqlitePool {
    let path = h.transcript_homes().opencode.join("opencode.db");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let pool = SqlitePoolOptions::new()
        .connect_with(
            SqliteConnectOptions::new()
                .filename(&path)
                .create_if_missing(true),
        )
        .await
        .unwrap();
    pool.execute(
        "CREATE TABLE session ( \
            id TEXT PRIMARY KEY, parent_id TEXT, directory TEXT, title TEXT, \
            time_updated INTEGER, model TEXT, tokens_input INTEGER, \
            tokens_output INTEGER, tokens_reasoning INTEGER, cost REAL \
         )",
    )
    .await
    .unwrap();
    pool.execute("CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT)")
        .await
        .unwrap();
    pool
}

/// The JSON the `model` column holds.
fn model(provider: &str, id: &str) -> String {
    json!({"id": id, "providerID": provider}).to_string()
}

/// One session row, as OpenCode writes it.
#[allow(clippy::too_many_arguments)]
async fn session(
    pool: &sqlx::SqlitePool,
    id: &str,
    parent: Option<&str>,
    directory: &str,
    title: &str,
    updated: DateTime<Utc>,
    model: Option<&str>,
    input: i64,
    output: i64,
) {
    sqlx::query(
        "INSERT INTO session \
         (id, parent_id, directory, title, time_updated, model, tokens_input, tokens_output) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(parent)
    .bind(directory)
    .bind(title)
    .bind(updated.timestamp_millis())
    .bind(model)
    .bind(input)
    .bind(output)
    .execute(pool)
    .await
    .unwrap();
}

/// One message row, as OpenCode writes it. Only `session_id` matters to the
/// reader; the rest of a real message is no part of this contract.
async fn message(pool: &sqlx::SqlitePool, id: &str, session_id: &str) {
    sqlx::query("INSERT INTO message (id, session_id) VALUES (?, ?)")
        .bind(id)
        .bind(session_id)
        .execute(pool)
        .await
        .unwrap();
}

/// A root session with a message that `session/list` never answers with is in
/// the listing all the same, under the directory and the title its own row
/// holds, and it carries the model and the tokens the row holds — spelled
/// `<providerID>/<id>`, as every other OpenCode model is.
#[tokio::test]
async fn a_session_the_agent_does_not_list_is_listed_with_its_model_and_tokens() {
    let dir = tempfile::tempdir().unwrap();
    let now = Utc::now();
    let stub = stub_listing(
        dir.path(),
        json!([{"sessionId": "listed", "cwd": "/work/listed", "title": "Listed by the agent",
                "updatedAt": hours_before(now, 2)}]),
    );
    let h = harness_with(&stub).await;
    let pool = db(&h).await;
    session(
        &pool,
        "on-disk",
        None,
        "/work/on-disk",
        "Read the config",
        now - TimeDelta::hours(1),
        Some(&model("anthropic", "claude-sonnet-4")),
        900,
        300,
    )
    .await;
    message(&pool, "msg-1", "on-disk").await;

    let page = listing(&h, "").await;

    assert_eq!(ids(&page), ["on-disk", "listed"]);
    let found = row(&page, "on-disk");
    assert_eq!(found.kind, SessionKind::Outside);
    assert_eq!(found.agent_id, OPENCODE);
    assert_eq!(found.internal_session_id.as_deref(), Some("on-disk"));
    assert_eq!(found.working_directory.as_deref(), Some("/work/on-disk"));
    assert_eq!(found.title.as_deref(), Some("Read the config"));
    assert_eq!(
        found.model.as_deref(),
        Some("opencode-acp:anthropic/claude-sonnet-4")
    );
    let usage = found.usage.expect("the row's tokens");
    assert_eq!(usage.input_tokens, 900);
    assert_eq!(usage.output_tokens, 300);
    assert_eq!(
        found.last_activity_at.as_deref(),
        Some(hours_before(now, 1).as_str())
    );
}

/// A session with no provider in its `model` column goes by the id alone.
#[tokio::test]
async fn a_model_with_no_provider_goes_by_its_id_alone() {
    let dir = tempfile::tempdir().unwrap();
    let stub = stub_listing(dir.path(), json!([]));
    let h = harness_with(&stub).await;
    let pool = db(&h).await;
    session(
        &pool,
        "no-provider",
        None,
        "/work/no-provider",
        "No provider",
        Utc::now() - TimeDelta::hours(1),
        Some(&json!({"id": "local-model"}).to_string()),
        10,
        20,
    )
    .await;
    message(&pool, "msg-1", "no-provider").await;

    let page = listing(&h, "").await;

    assert_eq!(
        row(&page, "no-provider").model.as_deref(),
        Some("opencode-acp:local-model")
    );
}

/// A child session is a subagent of its parent, not a conversation of its
/// own, and it is not listed even though it holds a message.
#[tokio::test]
async fn a_child_session_is_not_listed() {
    let dir = tempfile::tempdir().unwrap();
    let stub = stub_listing(dir.path(), json!([]));
    let h = harness_with(&stub).await;
    let pool = db(&h).await;
    session(
        &pool,
        "parent",
        None,
        "/work/parent",
        "Parent",
        Utc::now() - TimeDelta::hours(2),
        None,
        0,
        0,
    )
    .await;
    message(&pool, "msg-parent", "parent").await;
    session(
        &pool,
        "child",
        Some("parent"),
        "/work/parent",
        "A subagent",
        Utc::now() - TimeDelta::hours(1),
        None,
        0,
        0,
    )
    .await;
    message(&pool, "msg-child", "child").await;

    let page = listing(&h, "").await;

    assert_eq!(ids(&page), ["parent"]);
}

/// A root session with no message in it is not a conversation anybody had,
/// and it is not listed.
#[tokio::test]
async fn a_session_with_no_message_is_not_listed() {
    let dir = tempfile::tempdir().unwrap();
    let stub = stub_listing(dir.path(), json!([]));
    let h = harness_with(&stub).await;
    let pool = db(&h).await;
    session(
        &pool,
        "empty",
        None,
        "/work/empty",
        "Opened and left",
        Utc::now() - TimeDelta::hours(1),
        None,
        0,
        0,
    )
    .await;

    let page = listing(&h, "").await;

    assert_eq!(ids(&page), [] as [&str; 0]);
    assert_eq!(page.total, 0);
}

/// A row the agent answered with stays listed whatever the disk rules say:
/// one with no row in `opencode.db` at all.
#[tokio::test]
async fn a_row_the_agent_lists_stays_listed_whatever_the_disk_says() {
    let dir = tempfile::tempdir().unwrap();
    let now = Utc::now();
    let stub = stub_listing(
        dir.path(),
        json!([{"sessionId": "no-row", "cwd": "/work/no-row", "title": "No row of its own",
                "updatedAt": hours_before(now, 1)}]),
    );
    let h = harness_with(&stub).await;
    // No `opencode.db` is written at all: the disk half contributes nothing,
    // and the row the agent listed is what the page shows.

    let page = listing(&h, "").await;

    assert_eq!(ids(&page), ["no-row"]);
    assert_eq!(
        row(&page, "no-row").title.as_deref(),
        Some("No row of its own")
    );
}

/// A missing `opencode.db` leaves the listing working: the disk half
/// contributes nothing, the row the agent listed still answers, and the
/// reader never makes the file exist — `create_if_missing` stays false, so a
/// database this daemon never creates is not one it leaves behind either.
#[tokio::test]
async fn a_missing_database_leaves_the_listing_working() {
    let dir = tempfile::tempdir().unwrap();
    let now = Utc::now();
    let stub = stub_listing(
        dir.path(),
        json!([{"sessionId": "listed", "cwd": "/work/listed", "title": "Listed by the agent",
                "updatedAt": hours_before(now, 1)}]),
    );
    let h = harness_with(&stub).await;
    let db_path = h.transcript_homes().opencode.join("opencode.db");
    assert!(!db_path.is_file());

    let page = listing(&h, "").await;

    assert_eq!(ids(&page), ["listed"]);
    assert!(
        !db_path.is_file(),
        "the reader must not create the database"
    );
}
