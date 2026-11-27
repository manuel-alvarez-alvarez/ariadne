//! The upgrade fills the severity of the notices stored before it (040,
//! rule 6), and leaves the facts of every other event as they were.
use ariadne_store::Store;

#[tokio::test]
async fn the_upgrade_fills_the_severity_of_older_notices() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.db");
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
    let previous = sqlx::migrate::Migrator {
        migrations: std::borrow::Cow::Owned(
            all.migrations
                .iter()
                .filter(|m| m.version <= 32)
                .cloned()
                .collect(),
        ),
        ..sqlx::migrate::Migrator::DEFAULT
    };
    previous.run(&pool).await.unwrap();
    // A store that filled its facts before a notice had a severity: the
    // notices hold empty facts, and the fill is done.
    sqlx::raw_sql(
        r#"INSERT INTO agent_events (id, kind, payload, payload_codec, created_at, facts) VALUES
             ('e1', 'notice', '{"severity":"warning","title":"Slow"}', 'none', 'then', '{}'),
             ('e2', 'notice', '{"severity":"info","title":"Hi"}', 'none', 'then', '{}'),
             ('e3', 'stop', '{"stop_reason":"end_turn"}', 'none', 'then', '{"kept":true}');
           INSERT INTO store_fills (name, done_at) VALUES ('agent_event_facts', 'then');"#,
    )
    .execute(&pool)
    .await
    .unwrap();
    pool.close().await;

    let store = Store::open(&path).await.unwrap();
    drop(store);
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .connect_with(sqlx::sqlite::SqliteConnectOptions::new().filename(&path))
        .await
        .unwrap();
    let facts: Vec<(String, String)> =
        sqlx::query_as("SELECT id, facts FROM agent_events ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(
        facts,
        [
            ("e1".to_string(), r#"{"severity":"warning"}"#.to_string()),
            ("e2".to_string(), r#"{"severity":"info"}"#.to_string()),
            ("e3".to_string(), r#"{"kept":true}"#.to_string()),
        ]
    );
    let fills: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM store_fills")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(fills, 1, "the fill is done again");
}
