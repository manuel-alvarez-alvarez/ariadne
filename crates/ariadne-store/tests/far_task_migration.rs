//! Upgrade a database from before far tasks (037) with what its routes let
//! it hold.

/// A database at migration 0018 that holds two active linked goals of one
/// origin goal: the linked goal route of 0018 made a new goal on each create.
async fn before_far_tasks(dir: &std::path::Path) -> std::path::PathBuf {
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
    let through_0018 = sqlx::migrate::Migrator {
        migrations: std::borrow::Cow::Owned(
            all.migrations
                .iter()
                .filter(|migration| migration.version <= 18)
                .cloned()
                .collect(),
        ),
        ..sqlx::migrate::Migrator::DEFAULT
    };
    through_0018.run(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO goals (id, title, description, status, orchestrated, created_at,
                            updated_at, model, workflow, origin_goal_id)
         VALUES ('01first', 'Fed', '', 'active', 0, '2026-01-01', '2026-01-01',
                 'stub:model', 'default', 'origin'),
                ('01second', 'Fed again', '', 'active', 0, '2026-01-02', '2026-01-02',
                 'stub:model', 'default', 'origin')",
    )
    .execute(&pool)
    .await
    .unwrap();
    pool.close().await;
    path
}

#[tokio::test]
async fn an_upgrade_keeps_two_active_linked_goals_of_one_origin() {
    let dir = tempfile::tempdir().unwrap();
    let path = before_far_tasks(dir.path()).await;

    let store = ariadne_store::Store::open(&path).await.unwrap();
    for id in ["01first", "01second"] {
        let goal = store.get_goal(id).await.unwrap();
        assert_eq!(goal.origin_goal_id.as_deref(), Some("origin"));
        assert_eq!(goal.status, "active");
    }
    // The oldest one answers a create for the origin goal, and makes none.
    let repo = store
        .create_repository(ariadne_store::NewRepository {
            default_workflow: None,
            path: dir.path().join("repo").display().to_string(),
            base_branch: "main".into(),
            description: None,
            permission_mode: None,
        })
        .await
        .unwrap();
    let (answered, created) = store
        .create_linked_goal(
            ariadne_store::NewGoal {
                workflow: None,
                title: "Fed once more".into(),
                description: String::new(),
                issue_url: None,
                repository_ids: vec![repo.id.clone()],
                pin: ariadne_store::AgentPin {
                    model: "stub:model".into(),
                    effort: None,
                },
            },
            "origin",
        )
        .await
        .unwrap();
    assert_eq!(answered.id, "01first");
    assert!(!created);
    let repos = store.list_goal_repositories("01first").await.unwrap();
    assert!(repos.iter().any(|r| r.id == repo.id));
}
