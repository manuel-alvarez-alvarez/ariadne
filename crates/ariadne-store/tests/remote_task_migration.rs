//! Remote execution preserves local and linked task records during migration.
use ariadne_store::{SessionFilter, Store};

#[tokio::test]
async fn remote_execution_migration_keeps_existing_tasks_and_their_children() {
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
                .filter(|m| m.version <= 20)
                .cloned()
                .collect(),
        ),
        ..sqlx::migrate::Migrator::DEFAULT
    };
    previous.run(&pool).await.unwrap();
    sqlx::raw_sql("INSERT INTO repositories (id,path,base_branch,created_at,updated_at) VALUES ('repo','/repo','main','then','then');
        INSERT INTO goals (id,title,description,status,orchestrated,model,workflow,created_at,updated_at) VALUES ('local','Local','Instructions','active',1,'stub:model','develop-review-merge','then','then');
        INSERT INTO goals (id,title,description,status,orchestrated,origin_goal_id,model,workflow,created_at,updated_at) VALUES ('linked','Linked','Source instructions','active',0,'source-goal','stub:model','develop-review-merge','then','then');
        INSERT INTO tasks (id,goal_id,repo_id,title,description,branch,created_at,updated_at) VALUES ('local-task','local','repo','Local work','Task instructions','local-branch','then','then');
        INSERT INTO tasks (id,goal_id,repo_id,title,description,branch,origin_task_id,created_at,updated_at) VALUES ('linked-task','linked','repo','Linked work','Task instructions','linked-branch','source-task','then','then');
        INSERT INTO task_agents (id,task_id,step,ordinal,model) VALUES ('agent','linked-task','develop',0,'stub:model');
        INSERT INTO agent_sessions (id,goal_id,task_id,seat,task_agent_id,model,created_at,place) VALUES ('session','linked','linked-task','agent','agent','stub:model','then','none');
        INSERT INTO session_usage (session_id,source,input_tokens,cached_input_tokens,output_tokens,updated_at) VALUES ('session','transcript',12,3,4,'then');
        INSERT INTO task_transitions (id,task_id,from_status,to_status,actor,created_at) VALUES ('transition','linked-task','pending','ready','daemon','then');
        INSERT INTO messages (id,goal_id,task_id,kind,from_actor,to_actor,body,created_at) VALUES ('message','linked','linked-task','message','agent','orchestrator','Retained question','then');")
        .execute(&pool).await.unwrap();
    pool.close().await;
    let store = Store::open(&path).await.unwrap();
    assert_eq!(store.list_goals(&[]).await.unwrap().len(), 2);
    assert_eq!(
        store.get_task("local-task").await.unwrap().branch,
        "local-branch"
    );
    assert_eq!(
        store
            .get_task("linked-task")
            .await
            .unwrap()
            .origin_task_id
            .as_deref(),
        Some("source-task")
    );
    assert!(store.tasks_without_local_goals().await.unwrap().is_empty());
    assert_eq!(
        store
            .list_sessions(SessionFilter::default())
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        store.session_usage("session").await.unwrap().input_tokens,
        12
    );
    assert_eq!(
        store
            .list_task_transitions("linked-task")
            .await
            .unwrap()
            .len(),
        1
    );
    store.delete_goal("linked").await.unwrap();
    assert!(store.get_task("linked-task").await.is_err());
    assert!(store.get_session("session").await.is_err());
    assert!(store.get_task("local-task").await.is_ok());
}
