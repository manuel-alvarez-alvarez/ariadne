//! The upgrade turns each pull request operation goal into its goal-less
//! task (029, 032) and leaves every ordinary goal as it was.
use ariadne_core::workflow::PrOperation;
use ariadne_store::{SessionFilter, Store};

#[tokio::test]
async fn the_upgrade_turns_operation_goals_into_goal_less_tasks() {
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
                .filter(|m| m.version <= 28)
                .cloned()
                .collect(),
        ),
        ..sqlx::migrate::Migrator::DEFAULT
    };
    previous.run(&pool).await.unwrap();
    sqlx::raw_sql("INSERT INTO repositories (id,path,base_branch,created_at,updated_at) VALUES ('repo','/repo','main','then','then');
        INSERT INTO goals (id,title,description,status,orchestrated,model,workflow,created_at,updated_at) VALUES ('ordinary','Ordinary','d','active',1,'stub:model','develop-review-merge','then','then');
        INSERT INTO goals (id,title,description,status,orchestrated,model,workflow,created_at,updated_at,pr_operation,pr_repository_id,pr_number,pr_url) VALUES ('open-review','Review PR #7','d','active',0,'stub:model','pr-review','then','then','review','repo',7,'https://forge/pull/7');
        INSERT INTO goals (id,title,description,status,orchestrated,model,workflow,created_at,updated_at,pr_operation,pr_repository_id,pr_number,pr_url) VALUES ('done-babysit','Keep PR #7','d','completed',0,'stub:model','pr-babysit','then','then','babysit','repo',7,'https://forge/pull/7');
        INSERT INTO goal_repositories (goal_id,repository_id) VALUES ('ordinary','repo'),('open-review','repo'),('done-babysit','repo');
        INSERT INTO goal_steps (goal_id,ordinal,id,title,description,skills,ranks,gate) VALUES ('open-review',0,'review','Review','d','[\"pr-reviewer\"]','[\"balanced\",\"frontier\"]','request_merged');
        INSERT INTO goal_steps (goal_id,ordinal,id,title,description,skills,ranks,gate) VALUES ('done-babysit',0,'babysit','Babysit','d','[\"pr-babysit\"]','[\"balanced\",\"frontier\"]','request_merged');
        INSERT INTO tasks (id,goal_id,repo_id,title,description,status,branch,step,created_at,updated_at) VALUES ('review-task','open-review','repo','Review PR #7','d','in_progress','b1','review','then','then');
        INSERT INTO tasks (id,goal_id,repo_id,title,description,status,branch,created_at,updated_at) VALUES ('babysit-task','done-babysit','repo','Keep PR #7','d','finished','b2','then','then');
        INSERT INTO tasks (id,goal_id,repo_id,title,description,branch,created_at,updated_at) VALUES ('ordinary-task','ordinary','repo','Work','d','b3','then','then');
        INSERT INTO task_agents (id,task_id,step,ordinal,model) VALUES ('agent','review-task','review',0,'stub:model');
        INSERT INTO agent_sessions (id,goal_id,task_id,seat,task_agent_id,model,created_at,place) VALUES ('session','open-review','review-task','agent','agent','stub:model','then','none');")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;

    let store = Store::open(&path).await.unwrap();
    let goals: Vec<String> = store
        .list_goals(&[])
        .await
        .unwrap()
        .into_iter()
        .map(|goal| goal.id)
        .collect();
    assert_eq!(goals, ["ordinary"]);
    assert_eq!(
        store
            .get_task("ordinary-task")
            .await
            .unwrap()
            .goal_id
            .as_deref(),
        Some("ordinary")
    );

    let review = store.get_task("review-task").await.unwrap();
    assert_eq!(review.goal_id, None);
    assert_eq!(review.step.as_deref(), Some("review"));
    let open = store
        .active_pr_task("repo", 7, PrOperation::Review)
        .await
        .unwrap()
        .map(|task| task.id);
    assert_eq!(open.as_deref(), Some("review-task"));
    assert_eq!(
        store.task_workflow("review-task").await.unwrap().as_deref(),
        Some("pr-review")
    );
    let steps = store.task_steps(&review).await.unwrap();
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0].id, "review");
    let session = store.get_session("session").await.unwrap();
    assert_eq!(session.goal_id, None);
    assert_eq!(session.task_id.as_deref(), Some("review-task"));
    assert_eq!(
        store
            .list_sessions(SessionFilter::default())
            .await
            .unwrap()
            .len(),
        1
    );

    let babysit = store
        .task_pull_request("babysit-task")
        .await
        .unwrap()
        .expect("the request link");
    assert_eq!(babysit.operation(), Some(PrOperation::Babysit));
    assert_eq!(babysit.url, "https://forge/pull/7");
    assert!(!babysit.open);
    assert!(
        store
            .active_pr_task("repo", 7, PrOperation::Babysit)
            .await
            .unwrap()
            .is_none()
    );
}
