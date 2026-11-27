//! Upgrade a database of the last release, with pending and closed
//! `wait_for_user` requests, to one with no `agent_requests` table.

/// A database at migration 0023, the last one the release before the drop
/// shipped: a session, its events, its usage, and three requests of it.
async fn before_the_drop(dir: &std::path::Path) -> std::path::PathBuf {
    let path = dir.join("store.db");
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(&path)
                .create_if_missing(true),
        )
        .await
        .unwrap();
    let all = sqlx::migrate::Migrator::new(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations"),
    )
    .await
    .unwrap();
    let through_0023 = sqlx::migrate::Migrator {
        migrations: std::borrow::Cow::Owned(
            all.migrations
                .iter()
                .filter(|migration| migration.version <= 23)
                .cloned()
                .collect(),
        ),
        ..sqlx::migrate::Migrator::DEFAULT
    };
    through_0023.run(&pool).await.unwrap();
    for statement in [
        "INSERT INTO agent_sessions (id, seat, model, created_at) VALUES ('s1', 'reviewer', 'stub:model', '2026-01-01')",
        "INSERT INTO agent_events (id, session_id, kind, payload, payload_codec, created_at) VALUES ('e1', 's1', 'agent_request', CAST('{\"request_id\":\"r1\",\"hint\":\"Pick a base\"}' AS BLOB), 'none', '2026-01-01'), ('e2', 's1', 'stop', CAST('{}' AS BLOB), 'none', '2026-01-02')",
        "INSERT INTO session_usage (session_id, source, input_tokens, cached_input_tokens, output_tokens, updated_at) VALUES ('s1', 'src', 10, 2, 5, '2026-01-01')",
        "INSERT INTO agent_requests (id, session_id, summary, created_at, question) VALUES ('r1', 's1', 'Pick a base', '2026-01-01', '{\"text\":\"Which base?\"}')",
        "INSERT INTO agent_requests (id, session_id, summary, created_at, answered_at, resolution, answer) VALUES ('r2', 's1', 'Done', '2026-01-01', '2026-01-02', 'answered', '{\"text\":\"yes\"}'), ('r3', 's1', 'Gone', '2026-01-01', '2026-01-02', 'expired', NULL)",
    ] {
        sqlx::query(statement).execute(&pool).await.unwrap();
    }
    pool.close().await;
    path
}

/// Count the original tables, except those that later migrations drop.
async fn row_counts(path: &std::path::Path) -> Vec<(String, i64)> {
    let pool = sqlx::SqlitePool::connect(&format!("sqlite://{}", path.display()))
        .await
        .unwrap();
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type = 'table'
         AND name NOT IN ('agent_requests', 'messages', '_sqlx_migrations', 'task_workflows', 'task_pull_requests') ORDER BY name",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    let mut counts = Vec::new();
    for table in tables {
        let count: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT count(*) FROM \"{table}\""
        )))
        .fetch_one(&pool)
        .await
        .unwrap();
        counts.push((table, count));
    }
    pool.close().await;
    counts
}

#[tokio::test]
async fn an_upgrade_drops_the_requests_and_keeps_every_other_row() {
    let dir = tempfile::tempdir().unwrap();
    let path = before_the_drop(dir.path()).await;
    let before = row_counts(&path).await;

    let store = ariadne_store::Store::open(&path).await.unwrap();
    let session = store.get_session("s1").await.unwrap();
    assert_eq!(session.model, "stub:model");
    let events = store
        .list_events(ariadne_store::EventFilter {
            session_id: Some("s1".into()),
            limit: 100,
            ..Default::default()
        })
        .await
        .unwrap();
    let kinds: Vec<&str> = events.iter().map(|event| event.kind.as_str()).collect();
    assert_eq!(kinds, ["agent_request", "stop"]);
    assert_eq!(store.session_usage("s1").await.unwrap().output_tokens, 5);
    drop(store);

    let pool = sqlx::SqlitePool::connect(&format!("sqlite://{}", path.display()))
        .await
        .unwrap();
    let left: i64 =
        sqlx::query_scalar("SELECT count(*) FROM sqlite_master WHERE name LIKE '%agent_requests%'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(left, 0, "the table and its indexes are gone");
    pool.close().await;
    // Opening a store seeds the shipped skills and workflows, so a table
    // can gain rows. No table loses one.
    let after = row_counts(&path).await;
    let tables =
        |counts: &[(String, i64)]| counts.iter().map(|(t, _)| t.clone()).collect::<Vec<_>>();
    assert_eq!(tables(&after), tables(&before));
    for ((table, was), (_, is)) in before.iter().zip(&after) {
        assert!(is >= was, "{table} had {was} rows and has {is}");
    }
}
