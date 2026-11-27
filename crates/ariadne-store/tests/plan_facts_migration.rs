//! The upgrade fills the plan facts of the plan events stored before it
//! (040, rule 6), and leaves the facts of every other event as they were.
use ariadne_store::Store;

#[tokio::test]
async fn the_upgrade_fills_the_plan_facts_of_older_plan_events() {
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
                .filter(|m| m.version <= 31)
                .cloned()
                .collect(),
        ),
        ..sqlx::migrate::Migrator::DEFAULT
    };
    previous.run(&pool).await.unwrap();
    // A store that filled its facts before plans had one: the plan events
    // hold empty facts, and the fill is done.
    sqlx::raw_sql(
        r#"INSERT INTO agent_events (id, kind, payload, payload_codec, created_at, facts) VALUES
             ('e1', 'plan_update', '{"plan":{"type":"items","plan_id":"plan-1","entries":[]}}', 'none', 'then', '{}'),
             ('e2', 'plan_removed', '{"plan_id":"plan-1"}', 'none', 'then', '{}'),
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
            ("e1".to_string(), r#"{"plan":"plan-1"}"#.to_string()),
            ("e2".to_string(), r#"{"plan":"plan-1"}"#.to_string()),
            ("e3".to_string(), r#"{"kept":true}"#.to_string()),
        ]
    );
    let fills: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM store_fills")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(fills, 1, "the fill is done again");
}
