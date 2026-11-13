//! Upgrade an existing database without keeping unowned sessions.

#[tokio::test]
async fn upgrade_removes_unowned_sessions_and_preserves_seated_sessions() {
    let dir = tempfile::tempdir().unwrap();
    let migrations = dir.path().join("migrations");
    std::fs::create_dir(&migrations).unwrap();
    std::fs::write(
        migrations.join("0001_init.sql"),
        include_str!("../migrations/0001_init.sql"),
    )
    .unwrap();
    let path = dir.path().join("store.db");
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(&path)
                .create_if_missing(true)
                .foreign_keys(true),
        )
        .await
        .unwrap();
    sqlx::migrate::Migrator::new(migrations)
        .await
        .unwrap()
        .run(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO agent_sessions (id, seat, model, created_at, title) VALUES ('owned', 'reviewer', 'stub:model', '2026-01-01', 'Review'), ('unowned', NULL, 'stub:model', '2026-01-01', 'Loose')")
        .execute(&pool).await.unwrap();
    pool.close().await;
    let store = ariadne_store::Store::open(&path).await.unwrap();
    let sessions = store.list_sessions(Default::default()).await.unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, "owned");
    assert_eq!(sessions[0].model, "stub:model");
    let pool = sqlx::SqlitePool::connect(&format!("sqlite://{}", path.display()))
        .await
        .unwrap();
    let columns: Vec<(i64, String, String, i64, Option<String>, i64)> =
        sqlx::query_as("PRAGMA table_info(agent_sessions)")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert!(!columns.iter().any(|column| column.1 == "title"));
    pool.close().await;
}
