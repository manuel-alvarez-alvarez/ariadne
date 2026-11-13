//! Upgrade an existing database without keeping unowned sessions.

/// Writes the named migrations, applies them with foreign keys as given, and
/// seeds an owned and an unowned session, each with rows that reference it.
async fn seeded(
    dir: &std::path::Path,
    applied: &[(&str, &str)],
    foreign_keys: bool,
) -> std::path::PathBuf {
    let migrations = dir.join("migrations");
    std::fs::create_dir(&migrations).unwrap();
    for (name, sql) in applied {
        std::fs::write(migrations.join(name), sql).unwrap();
    }
    let path = dir.join("store.db");
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(&path)
                .create_if_missing(true)
                .foreign_keys(foreign_keys),
        )
        .await
        .unwrap();
    // Seed between 0001 and the rest, so a later migration sees the rows.
    let init = sqlx::migrate::Migrator::new(migrations.clone())
        .await
        .unwrap();
    let first = &init.migrations[..1];
    let only_init = sqlx::migrate::Migrator {
        migrations: std::borrow::Cow::Owned(first.to_vec()),
        ..sqlx::migrate::Migrator::DEFAULT
    };
    only_init.run(&pool).await.unwrap();
    for statement in [
        "INSERT INTO agent_sessions (id, seat, model, created_at, title) VALUES ('owned', 'reviewer', 'stub:model', '2026-01-01', 'Review'), ('unowned', NULL, 'stub:model', '2026-01-01', 'Loose')",
        "INSERT INTO agent_events (id, session_id, kind, payload, payload_codec, created_at) VALUES ('e-owned', 'owned', 'stop', x'7b7d', 'none', '2026-01-01'), ('e-unowned', 'unowned', 'stop', x'7b7d', 'none', '2026-01-01')",
        "INSERT INTO session_usage (session_id, source, input_tokens, cached_input_tokens, output_tokens, updated_at) VALUES ('owned', 's', 1, 0, 1, '2026-01-01'), ('unowned', 's', 1, 0, 1, '2026-01-01')",
        "INSERT INTO agent_requests (id, session_id, summary, created_at) VALUES ('r-owned', 'owned', 'q', '2026-01-01'), ('r-unowned', 'unowned', 'q', '2026-01-01')",
        "UPDATE agent_sessions SET switched_from = 'unowned' WHERE id = 'owned'",
    ] {
        sqlx::query(statement).execute(&pool).await.unwrap();
    }
    init.run(&pool).await.unwrap();
    pool.close().await;
    path
}

async fn assert_only_owned_remains(path: &std::path::Path) {
    let store = ariadne_store::Store::open(path).await.unwrap();
    let sessions = store.list_sessions(Default::default()).await.unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, "owned");
    assert_eq!(sessions[0].model, "stub:model");
    drop(store);
    let pool = sqlx::SqlitePool::connect(&format!("sqlite://{}", path.display()))
        .await
        .unwrap();
    let columns: Vec<(i64, String, String, i64, Option<String>, i64)> =
        sqlx::query_as("PRAGMA table_info(agent_sessions)")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert!(!columns.iter().any(|column| column.1 == "title"));
    for (table, expected) in [
        ("SELECT session_id FROM agent_events", "owned"),
        ("SELECT session_id FROM session_usage", "owned"),
        ("SELECT session_id FROM agent_requests", "owned"),
    ] {
        let rows: Vec<(String,)> = sqlx::query_as(table).fetch_all(&pool).await.unwrap();
        assert_eq!(rows, vec![(expected.to_string(),)], "{table}");
    }
    let switched: (Option<String>,) =
        sqlx::query_as("SELECT switched_from FROM agent_sessions WHERE id = 'owned'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(switched.0, None);
    pool.close().await;
}

#[tokio::test]
async fn upgrade_removes_unowned_sessions_and_their_rows() {
    let dir = tempfile::tempdir().unwrap();
    let path = seeded(
        dir.path(),
        &[("0001_init.sql", include_str!("../migrations/0001_init.sql"))],
        true,
    )
    .await;
    assert_only_owned_remains(&path).await;
}

/// A database that already applied 0002 the way `Store::open` runs it, with
/// foreign keys off, holds the removed sessions' rows; 0003 clears them.
#[tokio::test]
async fn upgrade_clears_rows_an_earlier_session_removal_left_behind() {
    let dir = tempfile::tempdir().unwrap();
    let path = seeded(
        dir.path(),
        &[
            ("0001_init.sql", include_str!("../migrations/0001_init.sql")),
            (
                "0002_remove_loose_sessions.sql",
                include_str!("../migrations/0002_remove_loose_sessions.sql"),
            ),
        ],
        false,
    )
    .await;
    assert_only_owned_remains(&path).await;
}
