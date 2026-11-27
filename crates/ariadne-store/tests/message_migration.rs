//! Upgrade a database of the last release, with agent messages on a goal, on
//! a local task and on a source task, to one with no `messages` table.

/// A database at migration 0024, the last one the release before the drop
/// shipped: two goals, a local task on each, a source task, their agents and
/// sessions, and messages on all of them.
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
    let through_0024 = sqlx::migrate::Migrator {
        migrations: std::borrow::Cow::Owned(
            all.migrations
                .iter()
                .filter(|migration| migration.version <= 24)
                .cloned()
                .collect(),
        ),
        ..sqlx::migrate::Migrator::DEFAULT
    };
    through_0024.run(&pool).await.unwrap();
    sqlx::raw_sql("INSERT INTO repositories (id,path,base_branch,created_at,updated_at) VALUES ('repo','/repo','main','then','then');
        INSERT INTO goals (id,title,description,status,orchestrated,model,workflow,created_at,updated_at) VALUES ('kept','Kept','Instructions','active',1,'stub:model','develop-review-merge','then','then'), ('gone','Gone','Instructions','active',1,'stub:model','develop-review-merge','then','then');
        INSERT INTO tasks (id,goal_id,repo_id,title,description,branch,created_at,updated_at) VALUES ('kept-task','kept','repo','Kept work','Task instructions','kept-branch','then','then'), ('dropped-task','kept','repo','Dropped work','Task instructions','dropped-branch','then','then'), ('gone-task','gone','repo','Gone work','Task instructions','gone-branch','then','then');
        INSERT INTO task_dependencies (task_id,depends_on_task_id) VALUES ('kept-task','dropped-task');
        INSERT INTO task_agents (id,task_id,step,ordinal,model) VALUES ('kept-agent','kept-task','develop',0,'stub:model'), ('dropped-agent','dropped-task','develop',0,'stub:model'), ('gone-agent','gone-task','develop',0,'stub:model');
        INSERT INTO agent_sessions (id,goal_id,task_id,seat,task_agent_id,model,created_at,place) VALUES ('kept-session','kept','kept-task','agent','kept-agent','stub:model','then','none'), ('dropped-session','kept','dropped-task','agent','dropped-agent','stub:model','then','none'), ('gone-session','gone','gone-task','agent','gone-agent','stub:model','then','none'), ('orchestrator','kept',NULL,'orchestrator',NULL,'stub:model','then','none');
        INSERT INTO source_task_references (id,goal_id,repo_id,host,worker_id,cache,created_at,updated_at) VALUES ('source-task','kept','repo','far','worker','{\"title\":\"Far work\",\"status\":\"pending\"}','then','then');
        INSERT INTO messages (id,goal_id,task_id,kind,from_actor,from_agent_id,from_session,to_actor,to_agent_id,body,delivered_at,created_at,relay_id,relayed_at) VALUES
            ('m1','kept',NULL,'message','agent',NULL,'orchestrator','orchestrator',NULL,'On the goal',NULL,'then',NULL,NULL),
            ('m2','kept','kept-task','message','orchestrator',NULL,NULL,'agent','kept-agent','To the kept agent','then','then',NULL,NULL),
            ('m3','kept','dropped-task','message','agent','dropped-agent','dropped-session','orchestrator',NULL,'From the dropped agent',NULL,'then',NULL,NULL),
            ('m4','gone','gone-task','message','orchestrator',NULL,NULL,'agent','gone-agent','To the gone agent',NULL,'then',NULL,NULL),
            ('m5','kept','source-task','message','orchestrator',NULL,NULL,'agent',NULL,'To the far agent',NULL,'then','r5','then');")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    path
}

async fn pool(path: &std::path::Path) -> sqlx::SqlitePool {
    sqlx::SqlitePool::connect(&format!("sqlite://{}", path.display()))
        .await
        .unwrap()
}

/// Count the original tables, except the messages that this upgrade drops.
async fn row_counts(path: &std::path::Path) -> Vec<(String, i64)> {
    let pool = pool(path).await;
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type = 'table'
         AND name NOT IN ('messages', '_sqlx_migrations', 'task_workflows', 'task_pull_requests') ORDER BY name",
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
async fn an_upgrade_drops_the_messages_and_keeps_every_other_row() {
    let dir = tempfile::tempdir().unwrap();
    let path = before_the_drop(dir.path()).await;
    let before = row_counts(&path).await;

    let store = ariadne_store::Store::open(&path).await.unwrap();
    assert_eq!(
        store.get_task("kept-task").await.unwrap().title,
        "Kept work"
    );
    assert_eq!(
        store.get_task("source-task").await.unwrap().title,
        "Far work"
    );
    drop(store);

    let pool = pool(&path).await;
    let left: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE name LIKE '%message%' OR sql LIKE '%messages%'",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(left.is_empty(), "nothing names the table: {left:?}");
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

#[tokio::test]
async fn a_goal_and_a_task_still_delete_after_the_upgrade() {
    let dir = tempfile::tempdir().unwrap();
    let path = before_the_drop(dir.path()).await;
    let store = ariadne_store::Store::open(&path).await.unwrap();

    store.delete_goal("gone").await.unwrap();
    assert!(store.get_goal("gone").await.is_err());
    assert!(store.get_task("gone-task").await.is_err());
    assert!(store.get_session("gone-session").await.is_err());

    // No store call deletes one local task, so the row goes by hand: the
    // triggers on `tasks` and `task_agents` run as they would for any delete.
    let pool = pool(&path).await;
    sqlx::query("DELETE FROM tasks WHERE id = 'dropped-task'")
        .execute(&pool)
        .await
        .unwrap();
    let dependencies: i64 = sqlx::query_scalar("SELECT count(*) FROM task_dependencies")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(dependencies, 0, "the dependency on the task went with it");
    pool.close().await;
    assert!(store.get_task("dropped-task").await.is_err());
    assert!(store.get_session("dropped-session").await.is_err());

    store
        .remove_source_task_reference("source-task")
        .await
        .unwrap();
    assert!(store.get_task("source-task").await.is_err());

    assert_eq!(
        store.get_task("kept-task").await.unwrap().title,
        "Kept work"
    );
    assert!(store.get_session("kept-session").await.is_ok());
    assert!(store.get_session("orchestrator").await.is_ok());
}
