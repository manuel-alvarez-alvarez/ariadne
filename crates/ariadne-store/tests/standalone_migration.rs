//! Upgrade tasks while preserving existing goal and remote records.
use ariadne_store::Store;

#[tokio::test]
async fn an_upgrade_preserves_goal_tasks_and_accepts_a_null_goal() {
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
                .filter(|m| m.version <= 25)
                .cloned()
                .collect(),
        ),
        ..sqlx::migrate::Migrator::DEFAULT
    };
    previous.run(&pool).await.unwrap();
    sqlx::raw_sql("INSERT INTO repositories (id,path,base_branch,created_at,updated_at) VALUES ('repo','/repo','main','then','then');
        INSERT INTO goals (id,title,description,status,orchestrated,model,workflow,created_at,updated_at) VALUES ('goal','Goal','Instructions','active',1,'stub:model','develop-review-merge','then','then');
        INSERT INTO tasks (id,goal_id,repo_id,title,description,branch,created_at,updated_at) VALUES ('old','goal','repo','Old work','Instructions','old-branch','then','then');").execute(&pool).await.unwrap();
    pool.close().await;
    let store = Store::open(&path).await.unwrap();
    let old = store.get_task("old").await.unwrap();
    assert_eq!(old.goal_id.as_deref(), Some("goal"));
    assert_eq!(old.branch, "old-branch");
    let pool = sqlx::SqlitePool::connect(&format!("sqlite://{}", path.display()))
        .await
        .unwrap();
    sqlx::query("INSERT INTO tasks (id,goal_id,repo_id,title,description,branch,created_at,updated_at) VALUES ('new',NULL,'repo','New work','Instructions','new-branch','then','then')").execute(&pool).await.unwrap();
    assert!(store.get_task("new").await.unwrap().goal_id.is_none());
    let invalid = sqlx::query("INSERT INTO tasks (id,goal_id,repo_id,title,description,branch,created_at,updated_at) VALUES ('invalid','unknown','repo','Invalid','Instructions','invalid-branch','then','then')").execute(&pool).await.unwrap_err();
    assert!(invalid.to_string().contains("task needs a local goal"));
    pool.close().await;
    store.delete_goal("goal").await.unwrap();
    assert!(store.get_task("old").await.is_err());
    assert!(store.get_task("new").await.is_ok());
}
