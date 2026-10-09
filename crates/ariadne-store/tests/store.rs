//! Store integration tests against a temp-file SQLite database.

use ariadne_core::{
    Actor, AttentionReason, GoalStatus, MessageKind, PermissionMode, Seat, SessionStatus,
    TaskStatus, TokenUsage,
};
use ariadne_store::defaults::{DEFAULT_WORKFLOW, PULL_REQUEST_WORKFLOW};
use ariadne_store::*;

async fn test_store() -> (Store, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("test.db")).await.unwrap();
    (store, dir)
}

fn bash_choice(repository_id: &str, call_id: &str, selected: &str) -> NewLearnedPermission {
    NewLearnedPermission {
        repository_id: repository_id.into(),
        tool_name: "Bash".into(),
        key: r#"{"command":"git show <HASH>"}"#.into(),
        level: "command".into(),
        family: "git show".into(),
        risk_tags: vec![],
        scope: "repository".into(),
        tool_call: serde_json::json!({
            "toolCallId": call_id, "title": "git show", "kind": "execute",
            "rawInput": {"description": "Show", "command": "git show 94f07c0b"},
        }),
        options: serde_json::json!([
            {"optionId": "yes", "kind": "allow_once"},
            {"optionId": "no", "kind": "reject_once"},
        ]),
        selected_option: selected.into(),
        target: "learn".into(),
        output: None,
    }
}

/// Two choices with one repository, tool name, level and key keep one row:
/// the second replaces the first, scope included, and keeps its id and
/// `created_at`; the row carries the key, the level, the family, the tags
/// and the scope.
/// The raw input is kept whole, its keys sorted.
#[tokio::test]
async fn learned_permissions_keep_one_row_per_repository_tool_level_and_key() {
    let (store, _dir) = test_store().await;
    let repo = store
        .create_repository(NewRepository {
            default_workflow: None,
            path: "/tmp/learned-repo".into(),
            base_branch: "main".into(),
            description: None,
            permission_mode: Some(PermissionMode::Learn),
        })
        .await
        .unwrap();
    let mut changes = store.watch_changes().expect("the only watcher");
    let mut widened = bash_choice(&repo.id, "call-1", "yes");
    widened.scope = "all".into();
    let first = store.record_learned_permission(widened).await.unwrap();
    assert_eq!(first.scope, "all");
    assert!(matches!(
        changes.recv().await.unwrap(),
        Change::LearnedPermissionCreated(_)
    ));
    let mut again = bash_choice(&repo.id, "call-2", "no");
    again.tool_call["rawInput"] =
        serde_json::json!({"command": "git show ff3c04a5 2>&1", "description": "Show it"});
    again.risk_tags = vec!["remote".into(), "force".into()];
    again.target = "ai".into();
    again.output = Some(serde_json::json!({"label": "deny", "danger": 0.9}));
    let first_updated_at = chrono::DateTime::parse_from_rfc3339(&first.updated_at)
        .unwrap()
        .with_timezone(&chrono::Utc);
    let next_millisecond = first_updated_at + chrono::Duration::milliseconds(20);
    while chrono::Utc::now() < next_millisecond {
        tokio::task::yield_now().await;
    }
    let second = store.record_learned_permission(again).await.unwrap();
    assert!(matches!(
        changes.recv().await.unwrap(),
        Change::LearnedPermissionUpdated(_)
    ));

    assert_eq!(second.id, first.id);
    assert_eq!(second.created_at, first.created_at);
    assert!(second.updated_at > first.updated_at);
    assert_eq!(second.key, r#"{"command":"git show <HASH>"}"#);
    assert_eq!(second.level, "command");
    assert_eq!(second.family, "git show");
    assert_eq!(second.risk_tags, r#"["remote","force"]"#);
    assert_eq!(second.scope, "repository", "a later choice sets the scope");
    assert_eq!(second.selected_option, "no");
    assert_eq!(second.target, "ai");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(second.output.as_deref().unwrap()).unwrap(),
        serde_json::json!({"label": "deny", "danger": 0.9})
    );
    let tool_call: serde_json::Value = serde_json::from_str(&second.tool_call).unwrap();
    assert_eq!(tool_call["toolCallId"], "call-2");
    assert!(
        second
            .tool_call
            .contains(r#""rawInput":{"command":"git show ff3c04a5 2>&1","description":"Show it"}"#),
        "rawInput is stored whole with sorted keys: {}",
        second.tool_call
    );
    assert_eq!(
        store
            .list_learned_permissions(Some(&repo.id))
            .await
            .unwrap(),
        vec![second.clone()]
    );

    let find = |level: &'static str, key: &'static str| {
        let store = store.clone();
        let repo_id = repo.id.clone();
        async move {
            store
                .find_learned_permission(&repo_id, "Bash", level, key)
                .await
                .unwrap()
        }
    };
    assert_eq!(
        find("command", r#"{"command":"git log"}"#).await,
        None,
        "another key is another row"
    );
    assert_eq!(
        find("family", r#"{"command":"git show <HASH>"}"#).await,
        None,
        "another level is another row"
    );
    assert_eq!(
        find("command", r#"{"command":"git show <HASH>"}"#).await,
        Some(second.clone())
    );

    let deleted = store.delete_learned_permission(&first.id).await.unwrap();
    assert_eq!(deleted.id, first.id);
    assert!(
        store
            .list_learned_permissions(None)
            .await
            .unwrap()
            .is_empty()
    );
}

/// A fresh database holds the learned permissions table with exactly its
/// fifteen columns.
#[tokio::test]
async fn a_fresh_database_holds_the_fifteen_learned_permission_columns() {
    use sqlx::Connection;
    let expected = [
        "id",
        "repository_id",
        "tool_name",
        "key",
        "level",
        "family",
        "risk_tags",
        "scope",
        "tool_call",
        "options",
        "selected_option",
        "target",
        "output",
        "created_at",
        "updated_at",
    ];
    let (store, dir) = test_store().await;
    drop(store);
    let mut fresh = sqlx::SqliteConnection::connect(&format!(
        "sqlite://{}",
        dir.path().join("test.db").display()
    ))
    .await
    .unwrap();
    let columns: Vec<String> =
        sqlx::query_scalar("SELECT name FROM pragma_table_info('learned_permissions')")
            .fetch_all(&mut fresh)
            .await
            .unwrap();
    assert_eq!(columns, expected);
}

/// A repository's own row for a key wins over an `all` row of another
/// repository with the same tool name, level and key; an `all` row answers
/// for a repository that holds none of its own.
#[tokio::test]
async fn a_repository_row_wins_over_an_all_row_of_another_repository() {
    let (store, _dir) = test_store().await;
    let repo_a = store
        .create_repository(NewRepository {
            default_workflow: None,
            path: "/tmp/learned-repo-a".into(),
            base_branch: "main".into(),
            description: None,
            permission_mode: Some(PermissionMode::Learn),
        })
        .await
        .unwrap();
    let repo_b = store
        .create_repository(NewRepository {
            default_workflow: None,
            path: "/tmp/learned-repo-b".into(),
            base_branch: "main".into(),
            description: None,
            permission_mode: Some(PermissionMode::Learn),
        })
        .await
        .unwrap();
    let repo_c = store
        .create_repository(NewRepository {
            default_workflow: None,
            path: "/tmp/learned-repo-c".into(),
            base_branch: "main".into(),
            description: None,
            permission_mode: Some(PermissionMode::Learn),
        })
        .await
        .unwrap();

    assert_eq!(
        store
            .find_learned_permission(
                &repo_b.id,
                "Bash",
                "command",
                r#"{"command":"git show <HASH>"}"#
            )
            .await
            .unwrap(),
        None,
        "no row anywhere yet"
    );

    let mut widened = bash_choice(&repo_a.id, "call-1", "yes");
    widened.scope = "all".into();
    let all_row = store.record_learned_permission(widened).await.unwrap();

    assert_eq!(
        store
            .find_learned_permission(
                &repo_b.id,
                "Bash",
                "command",
                r#"{"command":"git show <HASH>"}"#
            )
            .await
            .unwrap(),
        Some(all_row.clone()),
        "the all-scoped row answers a repository that holds none of its own"
    );

    let own = store
        .record_learned_permission(bash_choice(&repo_b.id, "call-2", "no"))
        .await
        .unwrap();
    assert_eq!(own.scope, "repository");

    assert_eq!(
        store
            .find_learned_permission(
                &repo_b.id,
                "Bash",
                "command",
                r#"{"command":"git show <HASH>"}"#
            )
            .await
            .unwrap(),
        Some(own),
        "the repository's own row wins over another repository's all-scoped row"
    );
    assert_eq!(
        store
            .find_learned_permission(
                &repo_c.id,
                "Bash",
                "command",
                r#"{"command":"git show <HASH>"}"#
            )
            .await
            .unwrap(),
        Some(all_row),
        "a third repository with no row of its own still gets the all-scoped row"
    );
}

/// A fresh database seeds the AI permission settings with the winner's
/// threshold pair, the `4b` flavour and a `NULL` device for the daemon to fill
/// at startup, and has no schedule columns.
#[tokio::test]
async fn a_fresh_database_seeds_the_ai_permission_defaults() {
    use sqlx::Connection;
    let (store, dir) = test_store().await;
    drop(store);
    let mut connection = sqlx::SqliteConnection::connect(&format!(
        "sqlite://{}",
        dir.path().join("test.db").display()
    ))
    .await
    .unwrap();

    let (allow, deny, flavour, device): (f64, f64, String, Option<String>) = sqlx::query_as(
        "SELECT allow_threshold, deny_threshold, flavour, device
         FROM ai_permission_settings WHERE id = 1",
    )
    .fetch_one(&mut connection)
    .await
    .unwrap();
    assert_eq!((allow, deny), (0.0201, 0.6321));
    assert_eq!(flavour, "4b");
    assert_eq!(device, None, "the daemon fills it at startup");

    let columns: Vec<String> =
        sqlx::query_scalar("SELECT name FROM pragma_table_info('ai_permission_settings')")
            .fetch_all(&mut connection)
            .await
            .unwrap();
    assert!(!columns.contains(&"schedule".to_string()));
    assert!(!columns.contains(&"last_scheduled_refresh".to_string()));
}

#[tokio::test]
async fn a_loose_session_round_trips_without_a_goal_task_or_seat() {
    let (store, dir) = test_store().await;
    let session = store
        .create_session(NewSession {
            goal_id: None,
            task_id: None,
            seat: None,
            task_agent_id: None,
            model: "stub:test-model".into(),
            effort: None,
            worktree_path: Some("/work/outside".into()),
            pull_request_id: None,
        })
        .await
        .unwrap();
    store
        .set_session_internal_id(&session.id, "outside-1")
        .await
        .unwrap();
    store.close().await;
    let store = Store::open(dir.path().join("test.db")).await.unwrap();
    let session = store.get_session(&session.id).await.unwrap();
    assert_eq!(session.model, "stub:test-model");
    assert_eq!(session.goal_id, None);
    assert_eq!(session.task_id, None);
    assert_eq!(session.seat(), None);
    assert_eq!(session.internal_session_id.as_deref(), Some("outside-1"));
    assert_eq!(session.worktree_path.as_deref(), Some("/work/outside"));
}

#[tokio::test]
async fn model_ranks_survive_reopen_and_clear_independently_of_enabled() {
    use ariadne_core::models::ModelRank;

    let (store, dir) = test_store().await;
    let id = "stub:provider/model:1";
    assert!(store.model_ranks().await.unwrap().is_empty());
    store
        .set_model_rank(id, Some(ModelRank::Frontier))
        .await
        .unwrap();
    store
        .set_model_rank(id, Some(ModelRank::Fast))
        .await
        .unwrap();
    store.set_model_enabled(id, false).await.unwrap();
    store.close().await;
    let store = Store::open(dir.path().join("test.db")).await.unwrap();
    let ranks = store.model_ranks().await.unwrap();
    assert_eq!(ranks.len(), 1);
    assert_eq!(ranks.get(id), Some(&ModelRank::Fast));
    assert!(store.disabled_models().await.unwrap().contains(id));
    store.set_model_rank(id, None).await.unwrap();
    store.set_model_rank(id, None).await.unwrap();
    store.close().await;
    let store = Store::open(dir.path().join("test.db")).await.unwrap();
    assert!(store.model_ranks().await.unwrap().is_empty());
    assert!(store.disabled_models().await.unwrap().contains(id));
}

#[tokio::test]
async fn one_acp_index_survives_reopen_and_each_download_replaces_it() {
    let (store, dir) = test_store().await;
    assert!(store.acp_registry_index().await.unwrap().is_none());
    store
        .put_acp_registry_index("https://first.example/index", r#"{"agents":[]}"#)
        .await
        .unwrap();
    let first = store.acp_registry_index().await.unwrap().unwrap();
    store.close().await;
    let store = Store::open(dir.path().join("test.db")).await.unwrap();
    assert_eq!(store.acp_registry_index().await.unwrap().unwrap(), first);
    let document = r#"{"agents":[{"id":"new"}]}"#;
    store
        .put_acp_registry_index("https://second.example/index", document)
        .await
        .unwrap();
    let second = store.acp_registry_index().await.unwrap().unwrap();
    assert_eq!(second.url, "https://second.example/index");
    assert_eq!(second.document, document);
    assert!(chrono::DateTime::parse_from_rfc3339(&second.fetched_at).is_ok());
}

/// The schema names an agent only by the registry id at the head of a pin:
/// no table keeps an agent kind beside it, and a session is its row, its
/// agent and its conversation — nothing names a terminal it runs in.
#[tokio::test]
async fn the_schema_names_agents_by_registry_id_alone() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    let _store = Store::open(&path).await.unwrap();
    let pool = sqlx::SqlitePool::connect(&format!("sqlite://{}", path.display()))
        .await
        .unwrap();
    let schemas: Vec<(String, String)> =
        sqlx::query_as("SELECT name, sql FROM sqlite_master WHERE type = 'table'")
            .fetch_all(&pool)
            .await
            .unwrap();
    for (table, schema) in schemas {
        assert!(!schema.contains("agent_kind"), "{table}: {schema}");
    }
    let columns: Vec<String> = sqlx::query_scalar("SELECT name FROM pragma_table_info(?)")
        .bind("agent_sessions")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(
        columns,
        [
            "id",
            "goal_id",
            "task_id",
            "seat",
            "task_agent_id",
            "internal_session_id",
            "worktree_path",
            "status",
            "last_activity_at",
            "created_at",
            "ended_at",
            "attention_reason",
            "attention_since",
            "model",
            "effort",
            "context_used",
            "context_size",
            "launched_at",
            "launch_id",
            "title",
            "switched_from",
            "pull_request_id",
        ]
    );
    let config: Vec<String> = sqlx::query_scalar("SELECT name FROM pragma_table_info(?)")
        .bind("agent_configs")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(config, ["agent_id", "extra_flags", "updated_at"]);
}

/// The pin every seeded agent runs on: a model is required everywhere, so the
/// fixtures name one and the tests that care name their own.
fn pin(model: &str) -> AgentPin {
    AgentPin {
        model: model.into(),
        effort: None,
    }
}

/// The pin the fixtures default to.
fn default_pin() -> AgentPin {
    pin("stub:test-model")
}

/// A registered repository, on a path of its own so goals can be seeded side
/// by side (one registration per path and base branch).
async fn seed_repository(store: &Store) -> Repository {
    store
        .create_repository(NewRepository {
            default_workflow: None,
            path: format!("/tmp/repo-{}", ariadne_core::id::new_id()),
            base_branch: "main".into(),
            description: None,
            permission_mode: None,
        })
        .await
        .unwrap()
}

async fn seed_goal(store: &Store) -> (Goal, Repository) {
    let repo = seed_repository(store).await;
    let goal = store
        .create_goal(NewGoal {
            workflow: None,
            issue_url: None,
            title: "Test goal".into(),
            description: "desc".into(),
            repository_ids: vec![repo.id.clone()],
            pin: default_pin(),
        })
        .await
        .unwrap();
    (goal, repo)
}

/// The ramp almost every test starts on: a fresh database holding one goal,
/// the repository that goal works in, and one task on it.
struct World {
    store: Store,
    goal: Goal,
    repo: Repository,
    task: Task,
    /// Kept for its Drop: the database lives in it.
    _dir: tempfile::TempDir,
}

impl World {
    async fn new() -> Self {
        let (store, dir) = test_store().await;
        let (goal, repo) = seed_goal(&store).await;
        let task = seed_task(&store, &goal, &repo, vec![]).await;
        Self {
            store,
            goal,
            repo,
            task,
            _dir: dir,
        }
    }

    /// A session in this world's goal, on its task unless `task_id` says
    /// otherwise — an orchestrator's has none.
    async fn session(
        &self,
        seat: Seat,
        agent_id: Option<&str>,
        task_id: Option<&str>,
    ) -> AgentSession {
        self.store
            .create_session(NewSession {
                goal_id: Some(self.goal.id.clone()),
                task_id: task_id.map(str::to_string),
                seat: Some(seat),
                task_agent_id: agent_id.map(str::to_string),
                model: "stub:test-model".into(),
                effort: None,
                worktree_path: Some("/tmp/wt".into()),
                pull_request_id: None,
            })
            .await
            .unwrap()
    }

    /// A session of the agent of this world's task's `develop` column: the
    /// one that works first, and the one most tests mean by "the agent".
    async fn agent_session(&self) -> AgentSession {
        let agent = agent_of(&self.store, &self.task, "develop").await;
        self.session(Seat::Agent, Some(&agent.id), Some(&self.task.id))
            .await
    }

    /// A session of the agent of this world's task's `review` column.
    async fn review_session(&self) -> AgentSession {
        let agent = agent_of(&self.store, &self.task, "review").await;
        self.session(Seat::Agent, Some(&agent.id), Some(&self.task.id))
            .await
    }

    /// This world's task as it now stands.
    async fn task(&self) -> Task {
        self.store.get_task(&self.task.id).await.unwrap()
    }
}

/// The commit every task walked to `finished` by [`walk_to`] lands as.
const LANDED_AS: &str = "abc123";

/// Walk a task up the happy path from wherever it is to `upto`: the daemon
/// readies and starts it at its first column, and from there the agent of
/// each column moves it to the next one and the last column's agent ends it
/// as finished, landed as [`LANDED_AS`].
///
/// `upto` is one of `ready`, `in_progress` and `finished`; a task already at
/// or past it is left where it is.
async fn walk_to(store: &Store, task_id: &str, upto: TaskStatus) -> Task {
    let mut task = store.get_task(task_id).await.unwrap();
    if task.status() == TaskStatus::Pending {
        task = store
            .transition_task(task_id, TaskStatus::Ready, Actor::Daemon, None, None)
            .await
            .unwrap();
    }
    if upto == TaskStatus::Ready {
        return task;
    }
    if task.status() == TaskStatus::Ready {
        task = store.start_first_step(task_id).await.unwrap();
    }
    if upto == TaskStatus::InProgress {
        return task;
    }
    let steps = store.goal_steps(&task.goal_id).await.unwrap();
    let at = steps
        .iter()
        .position(|step| Some(&step.id) == task.step.as_ref())
        .expect("a task in progress is in one of its goal's columns");
    for next in &steps[at + 1..] {
        store
            .move_step(task_id, &next.id, Actor::Agent, "done here", None)
            .await
            .unwrap();
    }
    let last = &steps[steps.len() - 1].id;
    store
        .end_step(
            task_id,
            last,
            TaskStatus::Finished,
            "landed",
            Some(LANDED_AS),
        )
        .await
        .unwrap()
}

/// The agent of one column of a task.
async fn agent_of(store: &Store, task: &Task, step: &str) -> TaskAgent {
    store
        .list_task_agents(&task.id)
        .await
        .unwrap()
        .into_iter()
        .find(|agent| agent.step == step)
        .unwrap_or_else(|| panic!("task {} has no agent on {step}", task.id))
}

/// One agent per column of `goal`'s workflow, each on its column's own
/// skills and on the default pin.
async fn column_agents(store: &Store, goal: &Goal) -> Vec<NewTaskAgent> {
    store
        .goal_steps(&goal.id)
        .await
        .unwrap()
        .into_iter()
        .map(|step| NewTaskAgent::new(step.id, Vec::<String>::new(), default_pin()))
        .collect()
}

async fn seed_task(store: &Store, goal: &Goal, repo: &Repository, deps: Vec<String>) -> Task {
    store
        .create_task(NewTask {
            goal_id: goal.id.clone(),
            repo_id: repo.id.clone(),
            title: "task".into(),
            description: "do things".into(),
            agents: column_agents(store, goal).await,
            depends_on: deps,
        })
        .await
        .unwrap()
}

/// A fresh database holds no flags: an agent nobody configured is launched
/// with its registry command alone.
#[tokio::test]
async fn an_agent_nobody_configured_has_no_flags() {
    let (store, _dir) = test_store().await;
    assert!(store.list_agent_configs().await.unwrap().is_empty());
    assert!(store.agent_flags("codex-acp").await.unwrap().is_empty());
}

/// The flags are the user's to replace, emptying them included, and one
/// agent's flags are its own.
#[tokio::test]
async fn agent_config_flags_are_replaced_whole() {
    let (store, _dir) = test_store().await;
    let updated = store
        .update_agent_config("claude-agent-acp", vec!["--verbose".into()])
        .await
        .unwrap();
    assert_eq!(updated.agent_id, "claude-agent-acp");
    assert_eq!(updated.extra_flags(), vec!["--verbose".to_string()]);
    store
        .update_agent_config("codex-acp", vec!["--quiet".into()])
        .await
        .unwrap();
    let emptied = store
        .update_agent_config("codex-acp", vec![])
        .await
        .unwrap();
    assert!(emptied.extra_flags().is_empty());
    // The edit is read back from the database, and one agent's flags are its
    // own: emptying codex left claude alone.
    assert_eq!(
        store.agent_flags("claude-agent-acp").await.unwrap(),
        vec!["--verbose".to_string()]
    );
    assert!(store.agent_flags("codex-acp").await.unwrap().is_empty());
}

/// One catalog per agent: a newer read replaces the older one whole, and one
/// agent's catalog is its own.
#[tokio::test]
async fn an_acp_catalog_is_kept_per_agent_and_replaced_by_a_newer_read() {
    let (store, _dir) = test_store().await;
    assert!(store.list_acp_catalogs().await.unwrap().is_empty());
    let command = vec!["codex-acp".to_string()];
    store
        .put_acp_catalog("codex-acp", &command, "1.0", r#"{"models":[]}"#)
        .await
        .unwrap();
    store
        .put_acp_catalog(
            "opencode-acp",
            &["opencode".into(), "acp".into()],
            "1.18",
            "{}",
        )
        .await
        .unwrap();
    let replaced = store
        .put_acp_catalog("codex-acp", &command, "1.1", r#"{"models":["m"]}"#)
        .await
        .unwrap();
    assert_eq!(replaced.version, "1.1");
    assert_eq!(replaced.command(), command);

    let kept = store.list_acp_catalogs().await.unwrap();
    assert_eq!(
        kept.iter()
            .map(|row| (
                row.agent_id.as_str(),
                row.version.as_str(),
                row.catalog.as_str()
            ))
            .collect::<Vec<_>>(),
        [
            ("codex-acp", "1.1", r#"{"models":["m"]}"#),
            ("opencode-acp", "1.18", "{}"),
        ]
    );
}

#[tokio::test]
async fn repository_crud_and_unique_path_branch() {
    let (store, _dir) = test_store().await;
    let repo = store
        .create_repository(NewRepository {
            default_workflow: None,
            path: "/tmp/repo".into(),
            base_branch: "main".into(),
            description: Some("the one repo".into()),
            permission_mode: None,
        })
        .await
        .unwrap();
    assert_eq!(repo.path, "/tmp/repo");
    assert_eq!(repo.description.as_deref(), Some("the one repo"));
    // A repository registered without a mode approves on its own, and one
    // registered without a workflow lands its goals on the base branch.
    assert_eq!(repo.permission_mode(), PermissionMode::Auto);
    assert_eq!(repo.default_workflow, DEFAULT_WORKFLOW);

    // The same checkout on another branch is a different repository.
    let other = store
        .create_repository(NewRepository {
            default_workflow: Some(PULL_REQUEST_WORKFLOW.into()),
            path: "/tmp/repo".into(),
            base_branch: "next".into(),
            description: None,
            permission_mode: Some(PermissionMode::Learn),
        })
        .await
        .unwrap();
    assert!(other.description.is_none());
    assert_eq!(other.permission_mode(), PermissionMode::Learn);
    assert_eq!(other.default_workflow, PULL_REQUEST_WORKFLOW);
    assert_eq!(store.list_repositories().await.unwrap().len(), 2);

    // A default workflow is a reference into the catalog, so a name nothing
    // answers to is refused, at registration and at an edit alike.
    assert!(matches!(
        store
            .create_repository(NewRepository {
                default_workflow: Some("no-such-workflow".into()),
                path: "/tmp/elsewhere".into(),
                base_branch: "main".into(),
                description: None,
                permission_mode: None,
            })
            .await,
        Err(StoreError::NotFound {
            entity: "workflow",
            ..
        })
    ));
    assert!(matches!(
        store
            .update_repository(
                &repo.id,
                RepositoryUpdate {
                    default_workflow: Some("no-such-workflow".into()),
                    ..Default::default()
                },
            )
            .await,
        Err(StoreError::NotFound {
            entity: "workflow",
            ..
        })
    ));

    // (path, base_branch) is unique.
    let dup = store
        .create_repository(NewRepository {
            default_workflow: None,
            path: "/tmp/repo".into(),
            base_branch: "main".into(),
            description: None,
            permission_mode: None,
        })
        .await;
    assert!(matches!(dup, Err(StoreError::Conflict(_))));

    // Partial update: the branch moves, the description is cleared, the
    // mode and the workflow change, the path stays exactly as it was.
    let edited = store
        .update_repository(
            &repo.id,
            RepositoryUpdate {
                base_branch: Some("trunk".into()),
                description: Some(None),
                permission_mode: Some(PermissionMode::Ask),
                default_workflow: Some(PULL_REQUEST_WORKFLOW.into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(edited.path, "/tmp/repo");
    assert_eq!(edited.base_branch, "trunk");
    assert!(edited.description.is_none());
    assert_eq!(edited.permission_mode(), PermissionMode::Ask);
    assert_eq!(edited.default_workflow, PULL_REQUEST_WORKFLOW);

    // An update that names neither the mode nor the workflow keeps them.
    let renamed = store
        .update_repository(
            &repo.id,
            RepositoryUpdate {
                description: Some(Some("renamed".into())),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(renamed.permission_mode(), PermissionMode::Ask);
    assert_eq!(renamed.default_workflow, PULL_REQUEST_WORKFLOW);

    // An update onto a taken (path, base_branch) conflicts like a create.
    assert!(matches!(
        store
            .update_repository(
                &edited.id,
                RepositoryUpdate {
                    base_branch: Some("next".into()),
                    ..Default::default()
                },
            )
            .await,
        Err(StoreError::Conflict(_))
    ));

    // Nothing holds this one, so it goes.
    store.delete_repository(&edited.id).await.unwrap();
    assert!(matches!(
        store.get_repository(&edited.id).await,
        Err(StoreError::NotFound { .. })
    ));
    assert!(matches!(
        store.delete_repository(&edited.id).await,
        Err(StoreError::NotFound { .. })
    ));
}

/// How a goal's tasks run is its workflow, chosen once for the whole goal
/// and snapshotted into its columns. A goal created with no workflow takes
/// the `default_workflow` of its first repository — the first in the order
/// goals show their repositories — and a goal that names one runs on that,
/// whatever its repositories default to.
#[tokio::test]
async fn a_goal_created_with_no_workflow_takes_its_first_repositorys_default() {
    async fn register(store: &Store, path: String, default_workflow: Option<&str>) -> Repository {
        store
            .create_repository(NewRepository {
                default_workflow: default_workflow.map(str::to_string),
                path,
                base_branch: "main".into(),
                description: None,
                permission_mode: None,
            })
            .await
            .unwrap()
    }
    let (store, _dir) = test_store().await;
    let suffix = ariadne_core::id::new_id();
    // Two repositories, named so the one that lands by request sorts first.
    let by_request = register(
        &store,
        format!("/tmp/a-{suffix}"),
        Some(PULL_REQUEST_WORKFLOW),
    )
    .await;
    let by_merge = register(&store, format!("/tmp/b-{suffix}"), None).await;
    let goal_on = |workflow: Option<&str>, repository_ids: Vec<String>| NewGoal {
        workflow: workflow.map(str::to_string),
        issue_url: None,
        title: "Ship it".into(),
        description: String::new(),
        repository_ids,
        pin: default_pin(),
    };
    let column_ids = |steps: Vec<GoalStep>| steps.into_iter().map(|s| s.id).collect::<Vec<_>>();

    // Nothing said, one repository: the goal runs on that repository's
    // default, and its columns are that workflow's.
    let goal = store
        .create_goal(goal_on(None, vec![by_merge.id.clone()]))
        .await
        .unwrap();
    assert_eq!(goal.workflow, DEFAULT_WORKFLOW);
    assert_eq!(
        column_ids(store.goal_steps(&goal.id).await.unwrap()),
        ["develop", "review", "merge"]
    );

    // Nothing said, two repositories: the first one's default wins, whatever
    // order the request listed them in.
    let goal = store
        .create_goal(goal_on(
            None,
            vec![by_merge.id.clone(), by_request.id.clone()],
        ))
        .await
        .unwrap();
    assert_eq!(goal.workflow, PULL_REQUEST_WORKFLOW);
    assert_eq!(
        column_ids(store.goal_steps(&goal.id).await.unwrap()),
        ["develop", "review", "pr"]
    );

    // A workflow named on the goal beats every repository default.
    let goal = store
        .create_goal(goal_on(Some(DEFAULT_WORKFLOW), vec![by_request.id.clone()]))
        .await
        .unwrap();
    assert_eq!(goal.workflow, DEFAULT_WORKFLOW);
    assert_eq!(
        column_ids(store.goal_steps(&goal.id).await.unwrap()).last(),
        Some(&"merge".to_string())
    );

    // And a name nothing answers to refuses the whole creation.
    assert!(matches!(
        store
            .create_goal(goal_on(Some("no-such-workflow"), vec![by_merge.id.clone()]))
            .await,
        Err(StoreError::NotFound {
            entity: "workflow",
            ..
        })
    ));
    assert_eq!(store.list_goals(&[]).await.unwrap().len(), 3);
}

#[tokio::test]
async fn a_goal_reads_its_repositories_live() {
    let (store, _dir) = test_store().await;
    let api = seed_repository(&store).await;
    let ui = seed_repository(&store).await;

    let goal = store
        .create_goal(NewGoal {
            workflow: None,
            issue_url: None,
            title: "Two repos".into(),
            description: "desc".into(),
            // The same repository named twice is one reference.
            repository_ids: vec![api.id.clone(), ui.id.clone(), api.id.clone()],
            pin: default_pin(),
        })
        .await
        .unwrap();
    let repos = store.list_goal_repositories(&goal.id).await.unwrap();
    assert_eq!(repos.len(), 2);
    let task = seed_task(&store, &goal, &api, vec![]).await;

    // Editing the repository moves the goal and the task with it.
    store
        .update_repository(
            &api.id,
            RepositoryUpdate {
                base_branch: Some("trunk".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let listed = store.list_goal_repositories(&goal.id).await.unwrap();
    assert_eq!(
        listed
            .iter()
            .find(|r| r.id == api.id)
            .map(|r| r.base_branch.as_str()),
        Some("trunk")
    );
    let of_task = store.get_repository(&task.repo_id).await.unwrap();
    assert_eq!(of_task.base_branch, "trunk");
}

#[tokio::test]
async fn a_goal_needs_repositories_that_exist() {
    let (store, _dir) = test_store().await;
    let repo = seed_repository(&store).await;
    let new_goal = |repository_ids: Vec<String>| NewGoal {
        workflow: None,
        issue_url: None,
        title: "Goal".into(),
        description: "desc".into(),
        repository_ids,
        pin: default_pin(),
    };

    assert!(matches!(
        store.create_goal(new_goal(vec![])).await,
        Err(StoreError::Invalid(_))
    ));
    assert!(matches!(
        store.create_goal(new_goal(vec!["nosuchrepo".into()])).await,
        Err(StoreError::NotFound {
            entity: "repository",
            ..
        })
    ));
    // The unknown id refused the whole creation: no half-written goal.
    assert!(store.list_goals(&[]).await.unwrap().is_empty());

    // A task can only work in a repository its goal references.
    let goal = store.create_goal(new_goal(vec![repo.id])).await.unwrap();
    let unrelated = seed_repository(&store).await;
    assert!(matches!(
        store
            .create_task(NewTask {
                goal_id: goal.id.clone(),
                repo_id: unrelated.id,
                title: "task".into(),
                description: "do things".into(),
                agents: column_agents(&store, &goal).await,
                depends_on: vec![],
            })
            .await,
        Err(StoreError::Invalid(_))
    ));
}

/// Deleting a repository out from under a goal would leave it pointing at
/// nothing, so the refusal names who is holding it.
#[tokio::test]
async fn a_repository_a_goal_holds_cannot_be_deleted() {
    let w = World::new().await;

    let err = w.store.delete_repository(&w.repo.id).await.unwrap_err();
    let StoreError::Conflict(message) = err else {
        panic!("expected a conflict, got {err:?}");
    };
    assert!(message.contains("1 goal"), "{message}");
    assert!(message.contains("1 task"), "{message}");

    // Nothing holds it once the goal (and with it the task) is gone.
    w.store.delete_goal(&w.goal.id).await.unwrap();
    w.store.delete_repository(&w.repo.id).await.unwrap();
}

/// A task branch reads like a contributor's: the title, slugged and clipped to
/// a word boundary, with the tail of the id to tell two of them apart. Nothing
/// in it says Ariadne — this name is what shows on a published request.
#[tokio::test]
async fn task_branch_is_named_after_the_title() {
    let w = World::new().await;
    let task = w
        .store
        .create_task(NewTask {
            goal_id: w.goal.id.clone(),
            repo_id: w.repo.id.clone(),
            title: "Fix the merging briefing: real fetch/rebase".into(),
            description: "d".into(),
            agents: column_agents(&w.store, &w.goal).await,
            depends_on: vec![],
        })
        .await
        .unwrap();

    let tail = &task.id[task.id.len() - 6..];
    assert_eq!(
        task.branch,
        format!("fix-the-merging-briefing-real-fetch-{tail}")
    );
    assert!(!task.branch.contains("ariadne"), "{}", task.branch);
}

/// The happy path of a stepped task, one move at a time: the daemon readies
/// it and starts it at its first column, the agent of each column moves it
/// to the next one, and the last column's agent ends it as finished with the
/// commit it landed as. The status is `in_progress` from the first column to
/// the last; only the column moves, and every move is audited with the
/// column it left and the one it entered.
#[tokio::test]
async fn task_happy_path_to_finished() {
    let w = World::new().await;
    let store = &w.store;
    assert_eq!(w.task.status(), TaskStatus::Pending);
    assert_eq!(w.task.step, None, "no column before the first one");

    let ready = store
        .transition_task(&w.task.id, TaskStatus::Ready, Actor::Daemon, None, None)
        .await
        .unwrap();
    assert_eq!(ready.status(), TaskStatus::Ready);
    assert_eq!(ready.step, None);

    let started = store.start_first_step(&w.task.id).await.unwrap();
    assert_eq!(started.status(), TaskStatus::InProgress);
    assert_eq!(started.step.as_deref(), Some("develop"));

    let reviewing = store
        .move_step(&w.task.id, "review", Actor::Agent, "committed", None)
        .await
        .unwrap();
    assert_eq!(reviewing.status(), TaskStatus::InProgress);
    assert_eq!(reviewing.step.as_deref(), Some("review"));

    let merging = store
        .move_step(&w.task.id, "merge", Actor::Agent, "passed", None)
        .await
        .unwrap();
    assert_eq!(merging.status(), TaskStatus::InProgress);
    assert_eq!(merging.step.as_deref(), Some("merge"));
    assert_eq!(merging.merge_commit, None, "nothing has landed yet");

    let t = store
        .end_step(
            &w.task.id,
            "merge",
            TaskStatus::Finished,
            "fast-forwarded",
            Some("abc123"),
        )
        .await
        .unwrap();
    assert_eq!(t.status(), TaskStatus::Finished);
    assert_eq!(t.merge_commit.as_deref(), Some("abc123"));

    let audit = store.list_task_transitions(&t.id).await.unwrap();
    assert_eq!(
        audit
            .iter()
            .map(|t| {
                (
                    t.from_status.as_str(),
                    t.to_status.as_str(),
                    t.from_step.as_deref(),
                    t.to_step.as_deref(),
                    t.actor.as_str(),
                )
            })
            .collect::<Vec<_>>(),
        [
            ("pending", "ready", None, None, "daemon"),
            ("ready", "in_progress", None, Some("develop"), "daemon"),
            (
                "in_progress",
                "in_progress",
                Some("develop"),
                Some("review"),
                "agent"
            ),
            (
                "in_progress",
                "in_progress",
                Some("review"),
                Some("merge"),
                "agent"
            ),
            (
                "in_progress",
                "finished",
                Some("merge"),
                Some("merge"),
                "agent"
            ),
        ]
    );
    assert_eq!(audit[4].reason.as_deref(), Some("fast-forwarded"));
}

/// A refused move writes nothing: not the status, not the column, and no
/// audit row. The status table is the six statuses and who moves between
/// them; the columns move one at a time, forward or back, by the agent of
/// the column the task is in or by the daemon.
#[tokio::test]
async fn illegal_transitions_are_rejected_and_unaudited() {
    let w = World::new().await;
    let (store, task) = (&w.store, &w.task);
    let refused = |result: Result<Task>, what: &str| {
        let err = result
            .err()
            .unwrap_or_else(|| panic!("{what} was accepted"));
        assert!(
            matches!(err, StoreError::Transition(_)),
            "{what}: expected a transition error, got {err:?}"
        );
    };

    // Illegal edges: a pending task has no last column to finish from, and
    // only a failed task is retried.
    refused(
        store
            .transition_task(
                &task.id,
                TaskStatus::Finished,
                Actor::Agent,
                None,
                Some("x"),
            )
            .await,
        "pending -> finished",
    );
    refused(
        store
            .transition_task(&task.id, TaskStatus::InProgress, Actor::Daemon, None, None)
            .await,
        "pending -> in_progress",
    );
    // Legal edges, wrong actor: the daemon readies a task, and the daemon
    // alone starts it.
    refused(
        store
            .transition_task(&task.id, TaskStatus::Ready, Actor::Agent, None, None)
            .await,
        "pending -> ready by the agent",
    );
    let audit = store.list_task_transitions(&task.id).await.unwrap();
    assert!(audit.is_empty(), "nothing was written: {audit:?}");

    // In progress at its first column: a status cannot go back to ready, an
    // agent cannot finish it from anywhere but the last column, and a column
    // move skips none and needs a reason.
    walk_to(store, &task.id, TaskStatus::InProgress).await;
    refused(
        store
            .transition_task(&task.id, TaskStatus::Ready, Actor::User, None, None)
            .await,
        "in_progress -> ready",
    );
    refused(
        store
            .transition_task(&task.id, TaskStatus::Finished, Actor::User, None, Some("x"))
            .await,
        "in_progress -> finished by the user",
    );
    assert!(matches!(
        store
            .end_step(&task.id, "merge", TaskStatus::Finished, "done", Some("x"))
            .await,
        Err(StoreError::Conflict(_))
    ));
    assert!(matches!(
        store
            .move_step(&task.id, "merge", Actor::Agent, "skip the review", None)
            .await,
        Err(StoreError::Conflict(_))
    ));
    assert!(matches!(
        store
            .move_step(&task.id, "review", Actor::Agent, "   ", None)
            .await,
        Err(StoreError::Conflict(_))
    ));
    assert!(matches!(
        store
            .move_step(&task.id, "review", Actor::User, "I say so", None)
            .await,
        Err(StoreError::Conflict(_))
    ));
    assert!(matches!(
        store
            .move_step(&task.id, "nowhere", Actor::Agent, "lost", None)
            .await,
        Err(StoreError::Invalid(_))
    ));
    let still = store.get_task(&task.id).await.unwrap();
    assert_eq!(still.status(), TaskStatus::InProgress);
    assert_eq!(still.step.as_deref(), Some("develop"));

    // Failed: only the user or the orchestrator retries it, and it cannot be
    // failed again.
    store
        .transition_task(&task.id, TaskStatus::Failed, Actor::Agent, Some("no"), None)
        .await
        .unwrap();
    refused(
        store
            .transition_task(&task.id, TaskStatus::Ready, Actor::Daemon, None, None)
            .await,
        "failed -> ready by the daemon",
    );
    refused(
        store
            .transition_task(&task.id, TaskStatus::Failed, Actor::Daemon, None, None)
            .await,
        "failed -> failed",
    );
    // Terminal: a finished or cancelled task moves nowhere.
    store
        .transition_task(&task.id, TaskStatus::Cancelled, Actor::User, None, None)
        .await
        .unwrap();
    for (to, actor) in [
        (TaskStatus::Ready, Actor::User),
        (TaskStatus::Failed, Actor::Daemon),
        (TaskStatus::Cancelled, Actor::User),
    ] {
        refused(
            store.transition_task(&task.id, to, actor, None, None).await,
            &format!("cancelled -> {}", to.as_str()),
        );
    }

    let audit = store.list_task_transitions(&task.id).await.unwrap();
    assert_eq!(
        audit
            .iter()
            .map(|t| t.to_status.as_str())
            .collect::<Vec<_>>(),
        ["ready", "in_progress", "failed", "cancelled"],
        "refused transitions leave no audit rows"
    );
}

/// Nothing caps how many tasks a goal takes. How a goal breaks down is what
/// the orchestrator settles with the user before it writes any of them, so a
/// number enforced here could only refuse a plan they had already agreed.
#[tokio::test]
async fn a_goal_takes_as_many_tasks_as_its_plan_calls_for() {
    let (store, _dir) = test_store().await;
    let (goal, repo) = seed_goal(&store).await;
    for _ in 0..12 {
        seed_task(&store, &goal, &repo, vec![]).await;
    }
    assert_eq!(
        store
            .list_tasks(TaskFilter {
                goal_id: Some(goal.id.clone()),
                ..Default::default()
            })
            .await
            .unwrap()
            .len(),
        12
    );
}

#[tokio::test]
async fn dependencies_gate_and_reject_cycles() {
    let w = World::new().await;
    let (store, a) = (&w.store, &w.task);
    let b = seed_task(store, &w.goal, &w.repo, vec![a.id.clone()]).await;

    assert!(store.task_dependencies_merged(&a.id).await.unwrap());
    assert!(!store.task_dependencies_merged(&b.id).await.unwrap());

    // a -> b would close the cycle a <- b.
    assert!(matches!(
        store
            .set_task_dependencies(&a.id, std::slice::from_ref(&b.id))
            .await,
        Err(StoreError::Invalid(_))
    ));
    // Self-dependency.
    assert!(matches!(
        store
            .set_task_dependencies(&a.id, std::slice::from_ref(&a.id))
            .await,
        Err(StoreError::Invalid(_))
    ));

    // Merge a; b's deps become satisfied.
    walk_to(store, &a.id, TaskStatus::Finished).await;
    assert!(store.task_dependencies_merged(&b.id).await.unwrap());
}

/// A dependency that ended without merging is one nothing behind it can go on
/// waiting for; every other status is still on its way there.
#[tokio::test]
async fn a_dependency_that_ended_unmerged_is_reported_as_blocking() {
    let w = World::new().await;
    let (store, dep) = (&w.store, &w.task);
    let task = seed_task(store, &w.goal, &w.repo, vec![dep.id.clone()]).await;
    let blocked = async || {
        store
            .task_dependencies_blocked(&task.id)
            .await
            .unwrap()
            .map(|t| t.id)
    };

    assert_eq!(blocked().await, None, "a pending dependency is on its way");
    walk_to(store, &dep.id, TaskStatus::InProgress).await;
    assert_eq!(blocked().await, None, "and so is one in progress");

    store
        .transition_task(&dep.id, TaskStatus::Failed, Actor::Daemon, None, None)
        .await
        .unwrap();
    assert_eq!(blocked().await, Some(dep.id.clone()), "a failed one is not");

    // Retried, it is on its way again — and finished, it is where the task
    // waiting on it wanted it.
    store
        .transition_task(&dep.id, TaskStatus::Ready, Actor::User, None, None)
        .await
        .unwrap();
    assert_eq!(blocked().await, None, "a retried dependency blocks nothing");
    walk_to(store, &dep.id, TaskStatus::Finished).await;
    assert_eq!(blocked().await, None);

    // And a cancelled dependency is as final as a failed one.
    let cancelled = seed_task(store, &w.goal, &w.repo, vec![]).await;
    let waiting = seed_task(store, &w.goal, &w.repo, vec![cancelled.id.clone()]).await;
    store
        .transition_task(
            &cancelled.id,
            TaskStatus::Cancelled,
            Actor::User,
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        store
            .task_dependencies_blocked(&waiting.id)
            .await
            .unwrap()
            .map(|t| t.id),
        Some(cancelled.id)
    );
}

#[tokio::test]
async fn setting_the_dependencies_of_a_ready_task_downgrades_it_with_audit() {
    let w = World::new().await;
    let (store, dep) = (&w.store, &w.task);
    let task = seed_task(store, &w.goal, &w.repo, vec![]).await;

    store
        .transition_task(&task.id, TaskStatus::Ready, Actor::Daemon, None, None)
        .await
        .unwrap();

    // Adding a dependency to a ready task sends it back to pending...
    store
        .set_task_dependencies(&task.id, std::slice::from_ref(&dep.id))
        .await
        .unwrap();
    let task = store.get_task(&task.id).await.unwrap();
    assert_eq!(task.status(), TaskStatus::Pending);
    assert_eq!(
        store.list_task_dependencies(&task.id).await.unwrap(),
        vec![dep.id.clone()]
    );

    // ...and the downgrade is audited like any other transition.
    let audit = store.list_task_transitions(&task.id).await.unwrap();
    assert_eq!(audit.len(), 2);
    assert_eq!(audit[1].from_status, "ready");
    assert_eq!(audit[1].to_status, "pending");
    assert_eq!(audit[1].actor, "orchestrator");

    // Clearing the dependencies of a pending task leaves the status alone.
    store.set_task_dependencies(&task.id, &[]).await.unwrap();
    assert_eq!(
        store.get_task(&task.id).await.unwrap().status(),
        TaskStatus::Pending
    );
    assert_eq!(
        store.list_task_transitions(&task.id).await.unwrap().len(),
        2
    );
}

/// A message goes to exactly one recipient and is delivered once: the stamp is
/// what says which of them have reached their agent, and the claim that puts
/// it there answers true once and false after.
#[tokio::test]
async fn a_message_is_delivered_once_and_the_stamp_says_so() {
    let w = World::new().await;
    let (store, task) = (&w.store, &w.task);
    let developer = agent_of(store, task, "develop").await.id;
    let reviewer = agent_of(store, task, "review").await.id;

    let asked = store
        .send_message(NewMessage {
            goal_id: task.goal_id.clone(),
            task_id: Some(task.id.clone()),
            kind: MessageKind::Message,
            from_actor: Actor::Agent,
            from_agent_id: Some(reviewer),
            from_session: None,
            to_actor: Actor::Agent,
            to_agent_id: Some(developer),
            body: "why the retry?".into(),
        })
        .await
        .unwrap();
    assert!(!asked.is_delivered());

    let waiting = store
        .list_messages(MessageFilter {
            task_id: Some(task.id.clone()),
            undelivered_only: true,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(waiting.len(), 1);

    assert!(
        store.mark_message_delivered(&asked.id).await.unwrap(),
        "the first claim is the one that stamps"
    );
    let delivered = store.get_message(&asked.id).await.unwrap();
    let at = delivered.delivered_at.clone().expect("stamped");

    // A second claim loses and keeps the first time: the prompt and the read
    // race for one row, and only one of them may hand it over.
    assert!(
        !store.mark_message_delivered(&asked.id).await.unwrap(),
        "a claim on a stamped row answers that it lost"
    );
    assert_eq!(
        store.get_message(&asked.id).await.unwrap().delivered_at,
        Some(at)
    );
    assert!(
        store
            .list_messages(MessageFilter {
                task_id: Some(task.id.clone()),
                undelivered_only: true,
                ..Default::default()
            })
            .await
            .unwrap()
            .is_empty()
    );
}

/// The pull or merge request a published task was recorded as: the URL, which
/// is what the user is pointed at, and nothing else.
#[tokio::test]
async fn a_task_remembers_the_request_it_was_published_as() {
    let w = World::new().await;
    let (store, task) = (&w.store, &w.task);
    assert_eq!(store.get_task(&task.id).await.unwrap().pr_url, None);

    let url = "https://github.com/ariadne/ariadne/pull/12";
    store.set_task_pull_request(&task.id, url).await.unwrap();
    assert_eq!(
        store.get_task(&task.id).await.unwrap().pr_url.as_deref(),
        Some(url)
    );

    // Re-reporting the same request writes the same row; a different one
    // replaces it.
    store.set_task_pull_request(&task.id, url).await.unwrap();
    let other = "https://github.com/ariadne/ariadne/pull/13";
    store.set_task_pull_request(&task.id, other).await.unwrap();
    assert_eq!(
        store.get_task(&task.id).await.unwrap().pr_url.as_deref(),
        Some(other)
    );

    // A task that is retried starts over, and the request it was published as
    // does not come with it: nobody is going to merge that one now.
    store.clear_task_pull_request(&task.id).await.unwrap();
    assert_eq!(store.get_task(&task.id).await.unwrap().pr_url, None);
    // And a task that was never published is left exactly as it was.
    store.clear_task_pull_request(&task.id).await.unwrap();
    assert_eq!(store.get_task(&task.id).await.unwrap().pr_url, None);
}

#[tokio::test]
async fn sessions_and_events_round_trip() {
    let w = World::new().await;
    let (store, task) = (&w.store, &w.task);

    let session = w.agent_session().await;
    store
        .set_session_internal_id(&session.id, "uuid-1234")
        .await
        .unwrap();
    let session = store.get_session(&session.id).await.unwrap();
    assert_eq!(session.internal_session_id.as_deref(), Some("uuid-1234"));

    let live = store
        .list_sessions(SessionFilter {
            task_id: Some(task.id.clone()),
            live_only: true,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(live.len(), 1);

    store
        .create_event(NewAgentEvent {
            session_id: Some(session.id.clone()),
            task_id: Some(task.id.clone()),
            kind: "post_tool_use".into(),
            payload: serde_json::json!({"tool_name": "Bash"}),
        })
        .await
        .unwrap();
    let events = store
        .list_events(EventFilter {
            session_id: Some(session.id.clone()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, "post_tool_use");
}

/// Record `count` events on `session`, and answer their ids in the order they
/// were recorded — which, ids being monotonic, is the order of the ids
/// themselves.
async fn record_events(w: &World, session: &AgentSession, count: usize) -> Vec<String> {
    let mut ids = Vec::with_capacity(count);
    for _ in 0..count {
        let event = w
            .store
            .create_event(NewAgentEvent {
                session_id: Some(session.id.clone()),
                task_id: Some(w.task.id.clone()),
                kind: "stop".into(),
                payload: serde_json::json!({}),
            })
            .await
            .unwrap();
        ids.push(event.id);
    }
    ids
}

/// A descending page answers the events recorded last, newest first. It is
/// what a reader of a long-running database wants: the ascending page it
/// would otherwise get holds the oldest rows ever recorded, and never moves.
#[tokio::test]
async fn a_descending_page_answers_the_newest_events_newest_first() {
    let w = World::new().await;
    let session = w.agent_session().await;
    let recorded = record_events(&w, &session, 201).await;

    let newest = w
        .store
        .list_events(EventFilter {
            order: EventOrder::Desc,
            limit: 200,
            ..Default::default()
        })
        .await
        .unwrap();

    let expected: Vec<String> = recorded.iter().rev().take(200).cloned().collect();
    assert_eq!(
        newest.iter().map(|e| e.id.clone()).collect::<Vec<_>>(),
        expected
    );
    assert_eq!(
        newest.last().map(|e| e.id.as_str()),
        Some(recorded[1].as_str()),
        "the 201st event back is the oldest one a page of 200 reaches"
    );
}

/// `before` is where a descending page goes on from: the id under the last
/// row it answered takes the reader one page further back.
#[tokio::test]
async fn a_before_cursor_pages_back_past_the_newest_page() {
    let w = World::new().await;
    let session = w.agent_session().await;
    let recorded = record_events(&w, &session, 201).await;

    let newest = w
        .store
        .list_events(EventFilter {
            order: EventOrder::Desc,
            limit: 200,
            ..Default::default()
        })
        .await
        .unwrap();
    let older = w
        .store
        .list_events(EventFilter {
            before: Some(newest.last().unwrap().id.clone()),
            order: EventOrder::Desc,
            limit: 200,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(
        older.iter().map(|e| e.id.clone()).collect::<Vec<_>>(),
        [recorded[0].clone()],
        "one event was recorded before the page of 200"
    );
}

/// What a launch is dated for: the one clock a watchdog reads when a session
/// has reported nothing at all. A relaunch moves the date, which is what makes
/// the silence it measures this run's rather than the row's.
#[tokio::test]
async fn every_launch_of_a_session_is_dated() {
    let w = World::new().await;
    let store = &w.store;

    let session = w.agent_session().await;
    assert_eq!(
        session.launched_at, None,
        "a row that was created but never launched is dated by nothing"
    );

    store.mark_session_launched(&session.id).await.unwrap();
    let first = store
        .get_session(&session.id)
        .await
        .unwrap()
        .launched_at
        .expect("the launch is dated");

    // Launched again — a resume.
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    store.mark_session_launched(&session.id).await.unwrap();
    let second = store
        .get_session(&session.id)
        .await
        .unwrap()
        .launched_at
        .expect("the relaunch is dated too");
    assert!(second > first, "every launch moves the date");
}

/// The catalog is code, so what the database holds is the exception: the
/// models the user turned off. Turning one off twice is not an error, nor is
/// turning on one nothing ever turned off — both answer "nothing changed",
/// which is what a caller checks to know whether to say anything.
#[tokio::test]
async fn a_model_is_available_until_it_is_turned_off() {
    let w = World::new().await;
    let store = &w.store;
    let id = "opencode:anthropic/claude-sonnet-4";

    assert!(
        store.disabled_models().await.unwrap().is_empty(),
        "a fresh store subtracts nothing from the catalog"
    );
    assert!(
        !store.set_model_enabled(id, true).await.unwrap(),
        "turning on what was never off changes nothing"
    );

    assert!(store.set_model_enabled(id, false).await.unwrap());
    assert!(store.disabled_models().await.unwrap().contains(id));
    assert!(
        !store.set_model_enabled(id, false).await.unwrap(),
        "and off twice is off once"
    );

    // An id with a `/` in it round-trips whole: that is how opencode names
    // the models it discovers, and it is the id a pin is refused by.
    assert_eq!(
        store
            .disabled_models()
            .await
            .unwrap()
            .into_iter()
            .collect::<Vec<_>>(),
        vec![id.to_string()]
    );

    assert!(store.set_model_enabled(id, true).await.unwrap());
    assert!(store.disabled_models().await.unwrap().is_empty());
}

/// A resumed agent conversation keeps its one session row: restarting puts
/// the row back where a spawn leaves it, so nothing downstream can tell the
/// relaunch from a first launch.
#[tokio::test]
async fn restarting_a_session_reopens_the_same_row() {
    let w = World::new().await;
    let (store, task) = (&w.store, &w.task);

    let session = w.agent_session().await;
    store
        .set_session_internal_id(&session.id, "uuid-1234")
        .await
        .unwrap();
    store
        .set_session_status(&session.id, SessionStatus::Exited)
        .await
        .unwrap();
    store
        .set_session_attention(&session.id, AttentionReason::Disconnected)
        .await
        .unwrap();
    assert!(
        store
            .get_session(&session.id)
            .await
            .unwrap()
            .ended_at
            .is_some()
    );

    let restarted = store
        .restart_session(&session.id, Some("/tmp/wt2"))
        .await
        .unwrap();
    assert_eq!(
        restarted.attention_reason(),
        None,
        "a relaunch is the recovery: what it needed the user for goes with it"
    );
    assert_eq!(restarted.attention_since, None);
    assert_eq!(restarted.id, session.id, "the same row is reused");
    assert_eq!(restarted.status(), SessionStatus::Starting);
    assert_eq!(restarted.ended_at, None, "it has not ended after all");
    assert!(restarted.last_activity_at.is_some());
    assert_eq!(restarted.worktree_path.as_deref(), Some("/tmp/wt2"));
    assert_eq!(
        restarted.internal_session_id.as_deref(),
        Some("uuid-1234"),
        "the agent conversation carries over"
    );
    // One session, not two: the task's list is unchanged in length.
    assert_eq!(
        store
            .list_sessions(SessionFilter {
                task_id: Some(task.id.clone()),
                ..Default::default()
            })
            .await
            .unwrap()
            .len(),
        1
    );
    // Omitted values leave the stored ones alone.
    let again = store.restart_session(&session.id, None).await.unwrap();
    assert_eq!(again.worktree_path.as_deref(), Some("/tmp/wt2"));

    assert!(
        store
            .restart_session("01ARZ3NDEKTSV4RRFFQ69G5FAV", None)
            .await
            .is_err()
    );
}

/// Attention is orthogonal to the lifecycle status: it is raised and cleared
/// on its own, and re-raising the same reason keeps the clock running rather
/// than restarting it.
#[tokio::test]
async fn session_attention_is_raised_kept_and_cleared() {
    let w = World::new().await;
    let store = &w.store;

    let session = w.agent_session().await;
    let fresh = store.get_session(&session.id).await.unwrap();
    assert_eq!(fresh.attention_reason(), None);
    assert_eq!(fresh.attention_since, None);

    store
        .set_session_attention(&session.id, AttentionReason::WaitingPermission)
        .await
        .unwrap();
    let flagged = store.get_session(&session.id).await.unwrap();
    assert_eq!(
        flagged.attention_reason(),
        Some(AttentionReason::WaitingPermission)
    );
    let since = flagged
        .attention_since
        .clone()
        .expect("raising attention stamps when it started");
    assert_eq!(
        flagged.status(),
        SessionStatus::Starting,
        "attention leaves the lifecycle status alone"
    );

    // The same reason again: the clock keeps running from the first sighting.
    store
        .set_session_attention(&session.id, AttentionReason::WaitingPermission)
        .await
        .unwrap();
    assert_eq!(
        store
            .get_session(&session.id)
            .await
            .unwrap()
            .attention_since,
        Some(since.clone())
    );

    // A different reason replaces it, clock included.
    store
        .set_session_attention(&session.id, AttentionReason::AgentError)
        .await
        .unwrap();
    let changed = store.get_session(&session.id).await.unwrap();
    assert_eq!(
        changed.attention_reason(),
        Some(AttentionReason::AgentError)
    );
    assert!(changed.attention_since.is_some());

    // Only flagged sessions come back under the filter...
    let flagged = store
        .list_sessions(SessionFilter {
            attention_only: true,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(flagged.len(), 1);
    assert_eq!(flagged[0].id, session.id);

    store.clear_session_attention(&session.id).await.unwrap();
    let cleared = store.get_session(&session.id).await.unwrap();
    assert_eq!(cleared.attention_reason(), None);
    assert_eq!(cleared.attention_since, None);
    assert!(
        store
            .list_sessions(SessionFilter {
                attention_only: true,
                ..Default::default()
            })
            .await
            .unwrap()
            .is_empty()
    );

    // Clearing an unflagged session is a no-op, not an error.
    store.clear_session_attention(&session.id).await.unwrap();
    // An id that names no session still reports it.
    assert!(
        store
            .set_session_attention("01ARZ3NDEKTSV4RRFFQ69G5FAV", AttentionReason::Stalled)
            .await
            .is_err()
    );
    assert!(
        store
            .clear_session_attention("01ARZ3NDEKTSV4RRFFQ69G5FAV")
            .await
            .is_err()
    );
}

/// What an agent's own event may take down, and what it may not.
///
/// Every reason an agent raises for itself goes when it is working again —
/// and `waiting_user` is not one of those: nobody raised it on the agent's
/// behalf, so the agent getting on with something else is not the user having
/// dealt with it. Only the clear the sweep makes drops that one.
#[tokio::test]
async fn an_agents_own_event_does_not_clear_the_attention_raised_for_the_user() {
    let w = World::new().await;
    let store = &w.store;
    let session = w.agent_session().await;

    // What the agent raised for itself, the agent takes back down.
    store
        .set_session_attention(&session.id, AttentionReason::WaitingPermission)
        .await
        .unwrap();
    store.clear_agent_attention(&session.id).await.unwrap();
    assert_eq!(
        store
            .get_session(&session.id)
            .await
            .unwrap()
            .attention_reason(),
        None
    );

    // What was raised for the user stays up through every event it works
    // through...
    store
        .set_session_attention(&session.id, AttentionReason::WaitingUser)
        .await
        .unwrap();
    for _ in 0..3 {
        store.clear_agent_attention(&session.id).await.unwrap();
    }
    assert_eq!(
        store
            .get_session(&session.id)
            .await
            .unwrap()
            .attention_reason(),
        Some(AttentionReason::WaitingUser)
    );

    // ...until the user, or the sweep that decides nobody is owed it any
    // more, takes it down.
    store.clear_session_attention(&session.id).await.unwrap();
    assert_eq!(
        store
            .get_session(&session.id)
            .await
            .unwrap()
            .attention_reason(),
        None
    );
}

/// What a session reporting itself idle may take down: the silence it just
/// broke and the failed turn it recovered from, and nothing else.
///
/// Going idle is exactly when a permission dialog or a question is up, so the
/// prompts survive it — and `waiting_user` is no more the agent's here than it
/// is anywhere else.
#[tokio::test]
async fn an_idle_report_clears_only_the_silence_and_the_error() {
    let w = World::new().await;
    let store = &w.store;
    let session = w.agent_session().await;
    let reason = async || {
        store
            .get_session(&session.id)
            .await
            .unwrap()
            .attention_reason()
    };
    for raised in [AttentionReason::Stalled, AttentionReason::AgentError] {
        store
            .set_session_attention(&session.id, raised)
            .await
            .unwrap();
        store.clear_attention_after_idle(&session.id).await.unwrap();
        assert_eq!(reason().await, None, "{raised:?}");
    }

    for kept in [
        AttentionReason::WaitingPermission,
        AttentionReason::WaitingInput,
        AttentionReason::WaitingUser,
    ] {
        store
            .set_session_attention(&session.id, kept)
            .await
            .unwrap();
        store.clear_attention_after_idle(&session.id).await.unwrap();
        assert_eq!(reason().await, Some(kept), "{kept:?}");
    }

    // A session with nothing up is a no-op; an id that names none still says
    // so.
    store.clear_session_attention(&session.id).await.unwrap();
    store.clear_attention_after_idle(&session.id).await.unwrap();
    assert!(
        store
            .clear_attention_after_idle("01ARZ3NDEKTSV4RRFFQ69G5FAV")
            .await
            .is_err()
    );
}

/// What an agent's own detectors may raise over, and what they may not.
///
/// `waiting_user` is the one flag no agent put up: it says a person owes this
/// task something — a request that is theirs to merge — and a prompt, a
/// disconnect or a stall neither settles that nor is
/// more use to whoever is reading the strip. So a raise from any of them is
/// withheld, clock included, and the write says nothing to the watchers
/// either: nothing about the session changed.
#[tokio::test]
async fn an_agents_own_reason_does_not_replace_the_attention_raised_for_the_user() {
    let w = World::new().await;
    let store = &w.store;
    let session = w.agent_session().await;
    // Installed after the seeding, so what it holds is this test's writes.
    let mut changes = store.watch_changes().expect("the only watcher");

    store
        .set_session_attention(&session.id, AttentionReason::WaitingUser)
        .await
        .unwrap();
    let owed = store.get_session(&session.id).await.unwrap();
    let since = owed.attention_since.clone().expect("a clock on the flag");
    changes.recv().await.expect("the raise is announced");

    // Every reason an agent raises for itself, over the one it did not.
    for raised in [
        AttentionReason::Disconnected,
        AttentionReason::Stalled,
        AttentionReason::WaitingPermission,
        AttentionReason::WaitingInput,
        AttentionReason::AgentError,
    ] {
        store
            .set_session_attention(&session.id, raised)
            .await
            .expect("a withheld raise is not an error");
        let still = store.get_session(&session.id).await.unwrap();
        assert_eq!(
            still.attention_reason(),
            Some(AttentionReason::WaitingUser),
            "{raised:?} does not replace what the user is owed"
        );
        assert_eq!(
            still.attention_since, owed.attention_since,
            "{raised:?} does not restart the clock on it either"
        );
    }
    assert!(
        changes.try_recv().is_err(),
        "a withheld raise changed nothing, so it announces nothing"
    );

    // The other way round it does replace: what the user is owed is news
    // whatever the agent had up.
    store.clear_session_attention(&session.id).await.unwrap();
    store
        .set_session_attention(&session.id, AttentionReason::AgentError)
        .await
        .unwrap();
    store
        .set_session_attention(&session.id, AttentionReason::WaitingUser)
        .await
        .unwrap();
    let replaced = store.get_session(&session.id).await.unwrap();
    assert_eq!(
        replaced.attention_reason(),
        Some(AttentionReason::WaitingUser)
    );
    assert_ne!(
        replaced.attention_since,
        Some(since),
        "and it is a raise of its own, with a clock of its own"
    );

    // And the sweep's clear takes it down like any other.
    store.clear_session_attention(&session.id).await.unwrap();
    assert_eq!(
        store
            .get_session(&session.id)
            .await
            .unwrap()
            .attention_reason(),
        None
    );
}

/// Why an ended task ended, read off the transition that ended it: the
/// agent's own `fail_task` reason, or whatever cancelled it.
///
/// Only an ending has one. A task still being worked on carries nothing,
/// however much has been said in its transitions — the reason a column was
/// completed with is that move's, not the task's — and a retry that puts it
/// back to work takes the answer away with it.
#[tokio::test]
async fn an_ended_task_carries_the_reason_the_transition_that_ended_it_gave() {
    let w = World::new().await;
    let (store, task) = (&w.store, &w.task);
    let reason = async || {
        let task = store.get_task(&task.id).await.unwrap();
        store.ended_reason(&task).await.unwrap()
    };
    assert_eq!(reason().await, None, "a pending task has not ended");

    walk_to(store, &task.id, TaskStatus::InProgress).await;
    store
        .move_step(
            &task.id,
            "review",
            Actor::Agent,
            "the first pass, with a test per lane",
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        reason().await,
        None,
        "a column move's reason is not why the task ended"
    );

    store
        .transition_task(
            &task.id,
            TaskStatus::Failed,
            Actor::Agent,
            Some("the crate it names was deleted upstream"),
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        reason().await.as_deref(),
        Some("the crate it names was deleted upstream")
    );

    // Retried: the task is being worked on again, so there is no ending to
    // explain.
    store
        .transition_task(&task.id, TaskStatus::Ready, Actor::User, None, None)
        .await
        .unwrap();
    assert_eq!(reason().await, None);

    // And an ending nobody gave a reason for says nothing rather than
    // answering with the one before it.
    store
        .transition_task(&task.id, TaskStatus::Cancelled, Actor::User, None, None)
        .await
        .unwrap();
    assert_eq!(reason().await, None);
}

/// A stalled task is a task with a stalled agent on it: the flag on the
/// session is where that is decided, and the task's own column is the
/// projection of it, written by the attention change and by nothing else.
#[tokio::test]
async fn a_task_is_stalled_while_one_of_its_agents_is() {
    let w = World::new().await;
    let (store, task) = (&w.store, &w.task);
    let author = w.agent_session().await;
    let reviewer = w.review_session().await;
    assert!(!w.task().await.is_stalled());

    store
        .set_session_attention(&author.id, AttentionReason::Stalled)
        .await
        .unwrap();
    assert!(
        store.get_task(&task.id).await.unwrap().is_stalled(),
        "the task says what its agent's flag says"
    );

    // A status change is not news about the agent, so it does not take the
    // stall down behind its back.
    store
        .transition_task(&task.id, TaskStatus::Ready, Actor::Daemon, None, None)
        .await
        .unwrap();
    assert!(store.get_task(&task.id).await.unwrap().is_stalled());

    // A second agent stalling and unstalling changes nothing while the first
    // one is still stuck.
    store
        .set_session_attention(&reviewer.id, AttentionReason::Stalled)
        .await
        .unwrap();
    store.clear_session_attention(&reviewer.id).await.unwrap();
    assert!(store.get_task(&task.id).await.unwrap().is_stalled());

    // The clear an agent's own event makes ends it, since an agent that is
    // reporting again is not one that stopped working.
    store.clear_agent_attention(&author.id).await.unwrap();
    assert!(
        !store.get_task(&task.id).await.unwrap().is_stalled(),
        "an agent that is working again leaves no stall behind"
    );

    // And so does the relaunch that puts a stuck one back on its feet.
    store
        .set_session_attention(&author.id, AttentionReason::Stalled)
        .await
        .unwrap();
    assert!(store.get_task(&task.id).await.unwrap().is_stalled());
    store.restart_session(&author.id, None).await.unwrap();
    assert!(!store.get_task(&task.id).await.unwrap().is_stalled());

    // An orchestrator has no task to project onto, and says so on its own row.
    // An orchestrator is staffed on no task, so its session carries no agent.
    let alone = w.session(Seat::Orchestrator, None, None).await;
    store
        .set_session_attention(&alone.id, AttentionReason::Stalled)
        .await
        .unwrap();
    assert_eq!(
        store
            .get_session(&alone.id)
            .await
            .unwrap()
            .attention_reason(),
        Some(AttentionReason::Stalled)
    );
    assert!(!store.get_task(&task.id).await.unwrap().is_stalled());
}

/// A prompt is a question the live agent waits on, so it cannot outlive the
/// session it was raised on: retiring one takes `waiting_permission` /
/// `waiting_input` down with it, and leaves every reason a session ends
/// *carrying* exactly where it is.
#[tokio::test]
async fn retiring_a_session_drops_the_prompt_it_can_no_longer_answer() {
    let w = World::new().await;
    let store = &w.store;

    let session = w.agent_session().await;
    store
        .set_session_attention(&session.id, AttentionReason::WaitingPermission)
        .await
        .unwrap();

    // A status the session is still live in leaves the dialog alone: going
    // idle is exactly what an agent waiting on an answer looks like.
    store
        .set_session_status(&session.id, SessionStatus::Idle)
        .await
        .unwrap();
    assert_eq!(
        store
            .get_session(&session.id)
            .await
            .unwrap()
            .attention_reason(),
        Some(AttentionReason::WaitingPermission)
    );

    store
        .set_session_status(&session.id, SessionStatus::Exited)
        .await
        .unwrap();
    let ended = store.get_session(&session.id).await.unwrap();
    assert_eq!(ended.attention_reason(), None);
    assert_eq!(ended.attention_since, None);
    assert!(
        ended.ended_at.is_some(),
        "and it is retired as it always was"
    );

    // What a session ended reporting is not a dialog: it stays up, and stays
    // up through a further status write.
    let failed = w.agent_session().await;
    store
        .set_session_attention(&failed.id, AttentionReason::AgentError)
        .await
        .unwrap();
    let raised_at = store.get_session(&failed.id).await.unwrap().attention_since;
    store
        .set_session_status(&failed.id, SessionStatus::Failed)
        .await
        .unwrap();
    let ended = store.get_session(&failed.id).await.unwrap();
    assert_eq!(ended.attention_reason(), Some(AttentionReason::AgentError));
    assert_eq!(ended.attention_since, raised_at);
}

/// Raising a prompt and retiring the session are two writes that can arrive
/// in either order: the daemon reads a session, decides an approval dialog is
/// up, and by the time it says so the agent may have gone. What keeps the
/// dead row clean is the raise itself refusing — the liveness test rides in
/// the `UPDATE`, not in whatever its caller last read.
#[tokio::test]
async fn a_prompt_is_only_ever_raised_on_a_session_that_is_still_live() {
    let w = World::new().await;
    let store = &w.store;

    // The interleaving spelled out: a caller holding a session it read while
    // it was live, and the retirement landing before it gets to the raise.
    let session = w.agent_session().await;
    let as_read = store.get_session(&session.id).await.unwrap();
    assert!(as_read.status().is_live());
    store
        .set_session_status(&session.id, SessionStatus::Exited)
        .await
        .unwrap();
    store
        .set_session_attention(&as_read.id, AttentionReason::WaitingPermission)
        .await
        .unwrap();
    let row = store.get_session(&session.id).await.unwrap();
    assert_eq!(
        row.attention_reason(),
        None,
        "a session that has ended is not sitting on a dialog"
    );
    assert_eq!(row.attention_since, None);

    // Withholding it is not an error, but an id that names no session still
    // is — whichever reason it was raising.
    assert!(
        store
            .set_session_attention("01ARZ3NDEKTSV4RRFFQ69G5FAV", AttentionReason::WaitingInput)
            .await
            .is_err()
    );

    // What a dead agent can be flagged with is unchanged: `disconnected` is
    // for exactly this session.
    store
        .set_session_attention(&session.id, AttentionReason::Disconnected)
        .await
        .unwrap();
    assert_eq!(
        store
            .get_session(&session.id)
            .await
            .unwrap()
            .attention_reason(),
        Some(AttentionReason::Disconnected)
    );

    // And with the two writes actually racing, either order is fine: the
    // raise loses, or it wins and the retirement takes it down after it.
    for _ in 0..5 {
        let racing = w.agent_session().await;
        let (retired, raised) = tokio::join!(
            store.set_session_status(&racing.id, SessionStatus::Exited),
            store.set_session_attention(&racing.id, AttentionReason::WaitingInput),
        );
        retired.unwrap();
        raised.unwrap();
        assert_eq!(
            store
                .get_session(&racing.id)
                .await
                .unwrap()
                .attention_reason(),
            None,
            "an ended session never comes out of the race waiting on a dialog"
        );
    }
}

/// An ingested event's status can land after the session it names has been
/// killed: the daemon reads the row, decides `running` or `idle` from it,
/// and the agent may be retired before it gets to write that down. What
/// keeps the dead row clean is the write itself refusing — the liveness
/// test, and the "already there" test, ride in the `UPDATE`, not in
/// whatever the caller last read.
#[tokio::test]
async fn a_status_is_only_ever_written_while_the_session_is_still_live() {
    let w = World::new().await;
    let store = &w.store;

    // The interleaving spelled out: a caller holding a session it read
    // while it was live, and the retirement landing before it gets to write
    // the status its read decided.
    let session = w.agent_session().await;
    let as_read = store.get_session(&session.id).await.unwrap();
    assert!(as_read.status().is_live());
    store
        .set_session_status(&session.id, SessionStatus::Exited)
        .await
        .unwrap();
    store
        .set_session_status_if_live(&as_read.id, SessionStatus::Idle, None)
        .await
        .unwrap();
    let row = store.get_session(&session.id).await.unwrap();
    assert_eq!(
        row.status(),
        SessionStatus::Exited,
        "a session that has ended does not come back for a status decided before it did"
    );

    // Withholding it is not an error, but an id that names no session still
    // is.
    assert!(
        store
            .set_session_status_if_live("01ARZ3NDEKTSV4RRFFQ69G5FAV", SessionStatus::Idle, None)
            .await
            .is_err()
    );

    // And with the two writes actually racing, either order is fine: the
    // status write loses, or it wins and the retirement takes it down after
    // it.
    for _ in 0..5 {
        let racing = w.agent_session().await;
        let (retired, written) = tokio::join!(
            store.set_session_status(&racing.id, SessionStatus::Exited),
            store.set_session_status_if_live(&racing.id, SessionStatus::Idle, None),
        );
        retired.unwrap();
        written.unwrap();
        assert_eq!(
            store.get_session(&racing.id).await.unwrap().status(),
            SessionStatus::Exited,
            "an ended session never comes out of the race running again"
        );
    }
}

/// A status decided from one launch can land after the session has moved
/// past it: an event believes the launch it read, and by the time it writes
/// the status that belief decided, a relaunch may have moved the row on to
/// the next one. What keeps the new launch's `starting` row clean is the
/// write itself refusing — the launch it was decided for rides in the
/// `UPDATE` too, on the same terms as liveness.
#[tokio::test]
async fn a_status_is_only_written_for_the_launch_it_was_decided_for() {
    let w = World::new().await;
    let store = &w.store;

    let session = w.agent_session().await;
    store
        .set_session_launch(&session.id, "01launchonexxxxxxxxxxxxxxx")
        .await
        .unwrap();

    // The interleaving spelled out: an event's superseded check passed
    // against the launch it read, and the relaunch that moves the row past
    // it lands before the event gets to write the status that check decided.
    store.restart_session(&session.id, None).await.unwrap();
    store
        .set_session_status_if_live(
            &session.id,
            SessionStatus::Idle,
            Some("01launchonexxxxxxxxxxxxxxx"),
        )
        .await
        .unwrap();
    let restarted = store.get_session(&session.id).await.unwrap();
    assert_eq!(
        restarted.status(),
        SessionStatus::Starting,
        "the new launch's starting row is not moved by the one it replaced"
    );

    // The launch that follows believes the row, since it is now its own.
    let current = restarted.launch_id.clone();
    store
        .set_session_status_if_live(&session.id, SessionStatus::Running, current.as_deref())
        .await
        .unwrap();
    assert_eq!(
        store.get_session(&session.id).await.unwrap().status(),
        SessionStatus::Running
    );

    // An event that names no launch, or a row never launched, have neither
    // to compare, and both are believed — matching `ingest_event`'s own
    // superseded check.
    let never_launched = w.agent_session().await;
    store
        .set_session_status_if_live(
            &never_launched.id,
            SessionStatus::Running,
            Some("01somelaunchxxxxxxxxxxxxxx"),
        )
        .await
        .unwrap();
    assert_eq!(
        store
            .get_session(&never_launched.id)
            .await
            .unwrap()
            .status(),
        SessionStatus::Running,
        "a row never launched has no launch to compare against"
    );
    store
        .set_session_status_if_live(&never_launched.id, SessionStatus::Idle, None)
        .await
        .unwrap();
    assert_eq!(
        store
            .get_session(&never_launched.id)
            .await
            .unwrap()
            .status(),
        SessionStatus::Idle,
        "an event that names no launch is believed regardless"
    );
}

/// Every status a goal can be in survives the round trip through SQLite,
/// whose `CHECK` on the column is a second copy of the enum: a status the
/// constraint has not been told about is not a wrong answer but a write that
/// fails.
#[tokio::test]
async fn every_goal_status_round_trips_through_the_database() {
    let (store, _dir) = test_store().await;
    let (goal, _) = seed_goal(&store).await;

    for status in GoalStatus::ALL {
        assert_eq!(
            store
                .set_goal_status(&goal.id, status)
                .await
                .unwrap()
                .status(),
            status,
            "{}",
            status.as_str()
        );
    }
}

#[tokio::test]
async fn list_goals_filters_by_any_of_the_given_statuses() {
    let (store, _dir) = test_store().await;
    let (planning, _) = seed_goal(&store).await;
    let (active, _) = seed_goal(&store).await;
    let (cancelled, _) = seed_goal(&store).await;
    store
        .set_goal_status(&active.id, GoalStatus::Active)
        .await
        .unwrap();
    store
        .set_goal_status(&cancelled.id, GoalStatus::Cancelled)
        .await
        .unwrap();

    let ids = |goals: Vec<Goal>| goals.into_iter().map(|g| g.id).collect::<Vec<_>>();

    // No statuses is no filter at all.
    assert_eq!(ids(store.list_goals(&[]).await.unwrap()).len(), 3);
    assert_eq!(
        ids(store.list_goals(&[GoalStatus::Active]).await.unwrap()),
        vec![active.id.clone()]
    );
    // Several statuses match a goal in any of them, still ordered by id.
    let mut expected = vec![active.id.clone(), cancelled.id.clone()];
    expected.sort();
    assert_eq!(
        ids(store
            .list_goals(&[GoalStatus::Active, GoalStatus::Cancelled])
            .await
            .unwrap()),
        expected
    );
    assert_eq!(
        ids(store.list_goals(&[GoalStatus::Completed]).await.unwrap()),
        Vec::<String>::new()
    );
    assert_eq!(
        ids(store.list_goals(&[GoalStatus::Planning]).await.unwrap()),
        vec![planning.id]
    );
}

#[tokio::test]
async fn goal_cascade_delete_cleans_children() {
    let w = World::new().await;

    w.store.delete_goal(&w.goal.id).await.unwrap();
    assert!(matches!(
        w.store.get_task(&w.task.id).await,
        Err(StoreError::NotFound { .. })
    ));
}

/// Seeding is by name and overwrites no document the database holds: an
/// edited document is not seeded back over on a reopen, and a deleted skill
/// of the user's own stays deleted — nothing ships under its name to bring
/// it back.
#[tokio::test]
async fn a_reopen_reseeds_no_row_the_database_already_holds() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");

    let store = Store::open(&path).await.unwrap();
    store
        .set_skill_document("coding", "---\nname: coding\ndescription: mine\n---\n")
        .await
        .unwrap();
    store
        .create_skill(NewSkill {
            name: "api-design".into(),
            document: "---\nname: api-design\ndescription: mine too\n---\n".into(),
        })
        .await
        .unwrap();
    store.delete_skill("api-design").await.unwrap();
    drop(store);

    let store = Store::open(&path).await.unwrap();
    let coding = store.get_skill("coding").await.unwrap();
    assert_eq!(coding.summary(), "mine", "the edit survived the reopen");
    assert!(
        !coding.document_is_default(),
        "and was not seeded back over"
    );
    assert!(
        matches!(
            store.get_skill("api-design").await,
            Err(StoreError::NotFound { .. })
        ),
        "a deleted skill of the user's own stays deleted"
    );
}

/// A release that ships a new skill reaches a database seeded before it:
/// seeding runs by name on every open, so the one row the database lacks is
/// added on the shipped text, and every row it holds stays as it was. A
/// database that kept its sixteen-skill catalog would otherwise refuse every
/// orchestrator launch, whose skill the launcher reads by name.
#[tokio::test]
async fn a_new_shipped_skill_reaches_an_existing_database_on_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");

    let store = Store::open(&path).await.unwrap();
    store
        .set_skill_document("coding", "---\nname: coding\ndescription: mine\n---\n")
        .await
        .unwrap();
    drop(store);

    // The sixteen-skill era, reproduced: the row this release ships is taken
    // back out, the way a database written before it never had it.
    let pool = sqlx::SqlitePool::connect(&format!("sqlite://{}", path.display()))
        .await
        .unwrap();
    sqlx::query("DELETE FROM skills WHERE name = 'orchestration'")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;

    let store = Store::open(&path).await.unwrap();
    let orchestration = store.get_skill("orchestration").await.unwrap();
    assert!(
        orchestration.is_builtin() && orchestration.document_is_default(),
        "the missing skill arrived on the shipped text"
    );
    assert_eq!(
        store.list_skills().await.unwrap().len(),
        ariadne_store::defaults::BUILTIN_SKILLS.len()
    );
    let coding = store.get_skill("coding").await.unwrap();
    assert_eq!(
        coding.summary(),
        "mine",
        "and nothing the database held was touched"
    );
}

/// A release that drops a skill reaches a database seeded before it the way
/// one that adds a skill does: the catalog is the whole of what an agent can
/// be, so a name it no longer holds is offered to nobody.
///
/// What goes is only what holds nothing of anybody's. A document written over
/// a dropped built-in is the user's text, so the row stays and becomes theirs
/// — theirs to delete, and no longer resettable to a default that is gone. A
/// row a staffed agent still loads stays a built-in, because the foreign key
/// that keeps an old task readable holds it there.
#[tokio::test]
async fn a_dropped_shipped_skill_leaves_an_existing_database_on_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");

    let store = Store::open(&path).await.unwrap();
    let (goal, repo) = seed_goal(&store).await;
    let task = seed_task(&store, &goal, &repo, vec![]).await;
    drop(store);

    // The era of a larger catalog, reproduced: three built-ins this release
    // no longer ships, one of them written over and one of them still loaded
    // by the agent of the task's first column.
    let pool = sqlx::SqlitePool::connect(&format!("sqlite://{}", path.display()))
        .await
        .unwrap();
    for (name, document) in [
        ("release", None),
        (
            "triage",
            Some("---\nname: triage\ndescription: mine\n---\n"),
        ),
        ("dependency-upgrade", None),
    ] {
        sqlx::query(
            "INSERT INTO skills (name, document, builtin, created_at, updated_at)
             VALUES (?, ?, 1, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        )
        .bind(name)
        .bind(document)
        .execute(&pool)
        .await
        .unwrap();
    }
    sqlx::query(
        "INSERT INTO task_agent_skills (agent_id, skill_name, ordinal)
         SELECT id, 'dependency-upgrade', 1 FROM task_agents
          WHERE task_id = ? AND step = 'develop'",
    )
    .bind(&task.id)
    .execute(&pool)
    .await
    .unwrap();
    pool.close().await;

    let store = Store::open(&path).await.unwrap();
    assert!(
        matches!(
            store.get_skill("release").await,
            Err(StoreError::NotFound { .. })
        ),
        "a dropped built-in on the shipped text is taken out"
    );

    let theirs = store.get_skill("triage").await.unwrap();
    assert!(
        !theirs.is_builtin(),
        "a dropped built-in somebody wrote over becomes a skill of their own"
    );
    assert_eq!(theirs.summary(), "mine", "on the text they wrote");
    assert!(matches!(
        store.reset_skill("triage").await,
        Err(StoreError::Conflict(_))
    ));
    store.delete_skill("triage").await.unwrap();

    let loaded = store.get_skill("dependency-upgrade").await.unwrap();
    assert!(
        loaded.is_builtin(),
        "a dropped built-in an agent still loads stays, so the task still reads"
    );
    let author = agent_of(&store, &task, "develop").await;
    let names: Vec<String> = store
        .agent_skills(&author.id)
        .await
        .unwrap()
        .into_iter()
        .map(|s| s.name)
        .collect();
    assert_eq!(
        names,
        ["coding", "dependency-upgrade"],
        "and the task still names it"
    );
    assert_eq!(
        store.list_skills().await.unwrap().len(),
        ariadne_store::defaults::BUILTIN_SKILLS.len() + 1,
        "the catalog, plus the one row the task holds in place"
    );
}

/// `pull-request` left the catalog once opening a request became the
/// daemon's own tool rather than an agent's skill. An old staffing on it is
/// the kept-row case any dropped skill proves: the row stays a built-in,
/// because the task that staffed it still names it, and it reads as an empty
/// skill since nothing ships under its name any more.
#[tokio::test]
async fn an_old_staffing_on_pull_request_keeps_its_row_and_reads_as_empty() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");

    let store = Store::open(&path).await.unwrap();
    let (goal, repo) = seed_goal(&store).await;
    let task = seed_task(&store, &goal, &repo, vec![]).await;
    drop(store);

    // The era that shipped `pull-request`, reproduced: a row this release no
    // longer ships, staffed on the agent of the task's first column.
    let pool = sqlx::SqlitePool::connect(&format!("sqlite://{}", path.display()))
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO skills (name, document, builtin, created_at, updated_at)
         VALUES ('pull-request', NULL, 1, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO task_agent_skills (agent_id, skill_name, ordinal)
         SELECT id, 'pull-request', 1 FROM task_agents
          WHERE task_id = ? AND step = 'develop'",
    )
    .bind(&task.id)
    .execute(&pool)
    .await
    .unwrap();
    pool.close().await;

    let store = Store::open(&path).await.unwrap();
    let loaded = store.get_skill("pull-request").await.unwrap();
    assert!(
        loaded.is_builtin(),
        "an old staffing on pull-request keeps its row"
    );
    assert_eq!(
        loaded.document_text(),
        "",
        "the dropped skill reads as empty"
    );

    let author = agent_of(&store, &task, "develop").await;
    let names: Vec<String> = store
        .agent_skills(&author.id)
        .await
        .unwrap()
        .into_iter()
        .map(|s| s.name)
        .collect();
    assert!(
        names.contains(&"pull-request".to_string()),
        "the task still names it: {names:?}"
    );
}

/// A skill that merged into another takes its staffings with it: the rows
/// that named it name the skill that does its work now, so a task staffed
/// before the merge still reads as the work it did. An agent staffed on both
/// keeps one row, not two.
#[tokio::test]
async fn a_merged_skill_hands_its_staffings_to_the_skill_that_absorbed_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");

    let store = Store::open(&path).await.unwrap();
    let (goal, repo) = seed_goal(&store).await;
    let both = seed_task(&store, &goal, &repo, vec![]).await;
    let alone = seed_task(&store, &goal, &repo, vec![]).await;
    drop(store);

    // The era before the merge, reproduced: `testing` staffed beside `coding`
    // on one task's first agent, and on its own on the other's.
    let pool = sqlx::SqlitePool::connect(&format!("sqlite://{}", path.display()))
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO skills (name, document, builtin, created_at, updated_at)
         VALUES ('testing', NULL, 1, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO task_agent_skills (agent_id, skill_name, ordinal)
         SELECT id, 'testing', 1 FROM task_agents WHERE task_id = ? AND step = 'develop'",
    )
    .bind(&both.id)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE task_agent_skills SET skill_name = 'testing'
          WHERE skill_name = 'coding'
            AND agent_id IN (SELECT id FROM task_agents WHERE task_id = ?)",
    )
    .bind(&alone.id)
    .execute(&pool)
    .await
    .unwrap();
    pool.close().await;

    let store = Store::open(&path).await.unwrap();
    for task in [&both, &alone] {
        let author = agent_of(&store, task, "develop").await;
        let names: Vec<String> = store
            .agent_skills(&author.id)
            .await
            .unwrap()
            .into_iter()
            .map(|s| s.name)
            .collect();
        assert_eq!(names, ["coding"], "the staffing reads as the merged skill");
    }
    assert!(
        matches!(
            store.get_skill("testing").await,
            Err(StoreError::NotFound { .. })
        ),
        "and nothing holds the merged skill's row in place any more"
    );
}

/// A database from before a release can hold a skill of the user's own under
/// the name that release ships. The seed adopts it: the row becomes a
/// built-in, its text stays on it as the override — so the orchestrator runs
/// on the user's text the way it runs on an edited built-in — and a reset
/// goes to the shipped document instead of being refused.
#[tokio::test]
async fn a_user_skill_under_a_shipped_name_becomes_a_built_in_on_its_own_text() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    drop(Store::open(&path).await.unwrap());

    // The old era, reproduced: no shipped `orchestration`, and a skill of the
    // user's own under that name — which `create_skill` can no longer write,
    // since the seeded row now takes it.
    let pool = sqlx::SqlitePool::connect(&format!("sqlite://{}", path.display()))
        .await
        .unwrap();
    sqlx::query("DELETE FROM skills WHERE name = 'orchestration'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO skills (name, document, builtin, created_at, updated_at)
         VALUES ('orchestration', '---\nname: orchestration\ndescription: mine\n---\n', 0,
                 '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
    )
    .execute(&pool)
    .await
    .unwrap();
    pool.close().await;

    let store = Store::open(&path).await.unwrap();
    let adopted = store.get_skill("orchestration").await.unwrap();
    assert!(adopted.is_builtin(), "the row was adopted into the catalog");
    assert_eq!(adopted.summary(), "mine", "on the user's text, kept whole");
    assert!(
        !adopted.document_is_default(),
        "the text stands as an override, not as the shipped document"
    );

    // Which is what buys the reset: the override drops, the shipped text is
    // what is left.
    let reset = store.reset_skill("orchestration").await.unwrap();
    assert!(reset.document_is_default());
    assert_eq!(
        reset.document_text(),
        ariadne_store::defaults::default_skill_document("orchestration").unwrap()
    );
}

/// A pin is written exactly as it was given, whole: the agent CLI, the model,
/// and the effort where one was chosen.
///
/// There is nothing behind a pin to inherit from and no auto to fall back to:
/// what the orchestrator sized an agent at, or what the user chose instead,
/// is the whole of the answer, and every agent names its registry agent and
/// its model.
#[tokio::test]
async fn an_agent_is_written_on_the_pin_it_was_given_whole() {
    let (store, _dir) = test_store().await;
    let (goal, repo) = seed_goal(&store).await;

    let pinned = AgentPin {
        model: "codex-acp:gpt-5.6-luna".into(),
        effort: Some("max".into()),
    };
    let task = store
        .create_task(NewTask {
            goal_id: goal.id.clone(),
            repo_id: repo.id.clone(),
            title: "Staffed".into(),
            description: "do things".into(),
            agents: vec![
                NewTaskAgent {
                    pin: pinned.clone(),
                    ..NewTaskAgent::new("develop", ["coding"], default_pin())
                },
                NewTaskAgent::new("review", ["code-review"], default_pin()),
                NewTaskAgent::new("merge", ["merge"], default_pin()),
            ],
            depends_on: vec![],
        })
        .await
        .unwrap();

    let author = agent_of(&store, &task, "develop").await;
    assert_eq!(author.model, "codex-acp:gpt-5.6-luna");
    assert_eq!(author.effort.as_deref(), Some("max"));

    let reviewer = agent_of(&store, &task, "review").await;
    assert_eq!(reviewer.model, "stub:test-model");
    assert_eq!(reviewer.effort, None);

    // And the user's later choice replaces it whole, with no half left behind.
    let moved = store
        .set_agent_pin(
            &author.id,
            &AgentPin {
                model: "claude-agent-acp:claude-opus-5".into(),
                effort: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(moved.model, "claude-agent-acp:claude-opus-5");
    assert_eq!(
        moved.effort, None,
        "the effort belonged to the model that was left behind"
    );
}

/// A database written by a release from before the schema was squashed into
/// one migration: it holds migrations this release does not ship, sqlx refuses
/// to run over it, and there is no upgrade from it — so what the user is owed
/// is the file to delete, by name.
///
/// The row is planted rather than the old chain replayed: what the check reads
/// is `_sqlx_migrations`, and a version this release has no migration for is
/// exactly what every database of that era has.
#[tokio::test]
async fn a_database_from_before_the_squash_says_which_file_to_delete() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy.db");
    // A current database first, so that the only thing wrong with it is the
    // migration history.
    drop(Store::open(&path).await.unwrap());

    let pool = sqlx::SqlitePool::connect(&format!("sqlite://{}", path.display()))
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO _sqlx_migrations (version, description, installed_on, success,
                                       checksum, execution_time)
         VALUES (29, 'repositories', '2025-01-01 00:00:00', 1, x'00', 0)",
    )
    .execute(&pool)
    .await
    .unwrap();
    pool.close().await;

    let message = match Store::open(&path).await {
        Err(StoreError::Invalid(message)) => message,
        Err(e) => panic!("opened with the wrong error: {e}"),
        Ok(_) => panic!("a database from before the squash opened"),
    };
    assert!(
        message.contains(&path.display().to_string()) && message.contains("Delete"),
        "the message names the file to delete: {message}"
    );
    assert!(
        message.contains("predates the squashed schema"),
        "and says why: {message}"
    );

    // And `ariadne doctor` answers the same, which is what the user asks when
    // the daemon it belongs to will not start.
    assert_eq!(
        ariadne_store::pre_squash_database(&path).await.as_deref(),
        Some(message.as_str())
    );
    // A database this release wrote is not called old.
    let fresh = dir.path().join("fresh.db");
    drop(Store::open(&fresh).await.unwrap());
    assert_eq!(ariadne_store::pre_squash_database(&fresh).await, None);
    // Neither is a path with nothing on it.
    assert_eq!(
        ariadne_store::pre_squash_database(dir.path().join("nothing.db")).await,
        None
    );
}

/// A database that only ever ran the squashed `0001_init.sql` — every
/// database written before a later migration touched `tasks` — opens on this
/// release the ordinary way: a later migration edits the column rather than
/// the squashed one, so its checksum, and every database already recorded
/// against it, stay as they were.
///
/// The old chain is run with sqlx's own migrator over a directory holding
/// only that one file, the same one this release still ships, rather than a
/// row planted by hand: what a real database of that era has is the
/// bookkeeping sqlx itself would have written for it.
#[tokio::test]
async fn a_database_that_only_ran_the_squashed_migration_upgrades_in_place() {
    const OLD_INIT: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/migrations/0001_init.sql"
    ));

    let dir = tempfile::tempdir().unwrap();
    let old_migrations = dir.path().join("old_migrations");
    std::fs::create_dir(&old_migrations).unwrap();
    std::fs::write(old_migrations.join("0001_init.sql"), OLD_INIT).unwrap();

    let path = dir.path().join("old.db");
    let pool = sqlx::SqlitePool::connect(&format!("sqlite://{}?mode=rwc", path.display()))
        .await
        .unwrap();
    sqlx::migrate::Migrator::new(old_migrations)
        .await
        .unwrap()
        .run(&pool)
        .await
        .unwrap();
    pool.close().await;

    // This release's own migrations run over it: the squashed one is
    // unchanged and already recorded, so only the later ones apply.
    let store = Store::open(&path)
        .await
        .expect("a pre-squash database failed to upgrade in place");

    let (goal, repo) = seed_goal(&store).await;
    let task = seed_task(&store, &goal, &repo, vec![]).await;
    assert_eq!(store.get_task(&task.id).await.unwrap().pr_url, None);
}

/// A copy of the previous schema keeps an existing goal when the nullable
/// issue URL column is added. The old migration files remain unchanged.
#[tokio::test]
async fn the_issue_url_migration_keeps_existing_goals() {
    let dir = tempfile::tempdir().unwrap();
    let old_migrations = dir.path().join("old_migrations");
    std::fs::create_dir(&old_migrations).unwrap();
    for name in [
        "0001_init.sql",
        "0002_task_pr_ready.sql",
        "0003_ai_permission_thresholds_hand_set.sql",
        "0004_forge_integrations.sql",
    ] {
        let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("migrations")
            .join(name);
        std::fs::copy(source, old_migrations.join(name)).unwrap();
    }
    let path = dir.path().join("old.db");
    let pool = sqlx::SqlitePool::connect(&format!("sqlite://{}?mode=rwc", path.display()))
        .await
        .unwrap();
    sqlx::migrate::Migrator::new(old_migrations)
        .await
        .unwrap()
        .run(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO goals (id, title, description, created_at, updated_at, model) VALUES ('old-goal', 'Fix widgets', 'Old body', '2026-01-01', '2026-01-01', 'stub:model')")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;

    let store = Store::open(&path).await.unwrap();
    let goal = store.get_goal("old-goal").await.unwrap();
    assert_eq!(goal.title, "Fix widgets");
    assert_eq!(goal.description, "Old body");
    assert_eq!(goal.issue_url, None);
}

/// A database from before `thresholds_hand_set` (it ran `0001` and `0002`)
/// with the threshold pair at `allow` and `deny`, opened on this release.
async fn upgraded_with_thresholds(allow: f64, deny: f64) -> (Store, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let old_migrations = dir.path().join("old_migrations");
    std::fs::create_dir(&old_migrations).unwrap();
    for name in ["0001_init.sql", "0002_task_pr_ready.sql"] {
        let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("migrations")
            .join(name);
        std::fs::copy(source, old_migrations.join(name)).unwrap();
    }
    let path = dir.path().join("old.db");
    let pool = sqlx::SqlitePool::connect(&format!("sqlite://{}?mode=rwc", path.display()))
        .await
        .unwrap();
    sqlx::migrate::Migrator::new(old_migrations)
        .await
        .unwrap()
        .run(&pool)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE ai_permission_settings SET allow_threshold = ?, deny_threshold = ?, \
         flavour = '9b' WHERE id = 1",
    )
    .bind(allow)
    .bind(deny)
    .execute(&pool)
    .await
    .unwrap();
    pool.close().await;
    let store = Store::open(&path).await.unwrap();
    (store, dir)
}

/// The migration that adds `thresholds_hand_set` reads a row still at the
/// seeded pair 0.0201 / 0.6321 as a default pair, which then follows the
/// flavour, and keeps it as it was.
#[tokio::test]
async fn an_upgrade_marks_the_seeded_threshold_pair_as_the_default() {
    let (store, _dir) = upgraded_with_thresholds(0.0201, 0.6321).await;

    let row = store.ai_permission_settings().await.unwrap();
    assert!(!row.thresholds_hand_set);
    assert_eq!((row.allow_threshold, row.deny_threshold), (0.0201, 0.6321));
    assert_eq!(row.flavour, "9b");
}

/// The same migration reads any other pair as one the user set by hand, and
/// keeps it.
#[tokio::test]
async fn an_upgrade_marks_any_other_threshold_pair_as_hand_set() {
    let (store, _dir) = upgraded_with_thresholds(0.2, 0.6321).await;

    let row = store.ai_permission_settings().await.unwrap();
    assert!(row.thresholds_hand_set);
    assert_eq!((row.allow_threshold, row.deny_threshold), (0.2, 0.6321));
}

// -- token usage ------------------------------------------------------------

fn usage(input_tokens: u64, cached_input_tokens: u64, output_tokens: u64) -> TokenUsage {
    TokenUsage {
        input_tokens,
        cached_input_tokens,
        output_tokens,
    }
}

/// The rows `session_usage` is actually holding, read from the file itself:
/// what the store answers about a session that is gone is a zero either way,
/// so only the table can say whether anything was left behind.
async fn usage_rows(dir: &tempfile::TempDir) -> i64 {
    usage_rows_at(&dir.path().join("test.db")).await
}

async fn usage_rows_at(path: &std::path::Path) -> i64 {
    let pool = sqlx::SqlitePool::connect(&format!("sqlite://{}", path.display()))
        .await
        .unwrap();
    let rows = sqlx::query_scalar("SELECT COUNT(*) FROM session_usage")
        .fetch_one(&pool)
        .await
        .unwrap();
    pool.close().await;
    rows
}

/// A report is the whole of one transcript, not an increment: reporting the
/// same source again leaves the session at the new figures rather than at the
/// sum of both, and only a second source adds to it.
#[tokio::test]
async fn a_source_replaces_its_own_totals_and_sources_add_up() {
    let w = World::new().await;
    let session = w.agent_session().await;
    // Nothing reported is a zero, not an absence.
    assert_eq!(
        w.store.session_usage(&session.id).await.unwrap(),
        TokenUsage::default()
    );

    assert!(
        w.store
            .upsert_session_usage(&session.id, "/x.jsonl", usage(100, 80, 10))
            .await
            .unwrap()
    );
    // Agents re-report their totals on every event: the same figures again
    // are no change, and say so.
    assert!(
        !w.store
            .upsert_session_usage(&session.id, "/x.jsonl", usage(100, 80, 10))
            .await
            .unwrap()
    );
    assert!(
        w.store
            .upsert_session_usage(&session.id, "/x.jsonl", usage(150, 120, 30))
            .await
            .unwrap()
    );
    assert_eq!(
        w.store.session_usage(&session.id).await.unwrap(),
        usage(150, 120, 30),
        "the second report replaces the first, it does not add to it"
    );

    // A resumed agent writes a transcript of its own, and that one does add.
    w.store
        .upsert_session_usage(&session.id, "/y.jsonl", usage(10, 0, 5))
        .await
        .unwrap();
    assert_eq!(
        w.store.session_usage(&session.id).await.unwrap(),
        usage(160, 120, 35)
    );
    assert_eq!(usage_rows(&w._dir).await, 2, "one row per source");
}

/// What a task spent, by the agent that spent it: one entry per column agent
/// that has a session on the task, with every session of it summed into one
/// — an agent returned to its column runs a session per return, and nobody
/// reads them return by return. The entries come in agent id order, so a
/// reader sees the same order twice running.
#[tokio::test]
async fn a_tasks_usage_groups_every_session_of_an_agent_together() {
    let w = World::new().await;
    let developer = w.agent_session().await;
    let first_return = w.review_session().await;
    let second_return = w.review_session().await;

    for (session, spent) in [
        (&developer, usage(100, 80, 10)),
        (&first_return, usage(20, 10, 4)),
        (&second_return, usage(5, 1, 2)),
    ] {
        w.store
            .upsert_session_usage(&session.id, "/x.jsonl", spent)
            .await
            .unwrap();
    }

    let grouped = w.store.task_usage(&w.task.id).await.unwrap();
    let mut expected = vec![
        AgentUsage {
            agent_id: agent_of(&w.store, &w.task, "develop").await.id,
            usage: usage(100, 80, 10),
        },
        AgentUsage {
            agent_id: agent_of(&w.store, &w.task, "review").await.id,
            usage: usage(25, 11, 6),
        },
    ];
    expected.sort_by(|a, b| a.agent_id.cmp(&b.agent_id));
    assert_eq!(grouped, expected);
}

/// A session that has reported nothing is still one of the task's: it reads
/// as zeros rather than dropping out, so the agent nobody has spent anything
/// on is still listed — and an agent with no session at all is not.
#[tokio::test]
async fn a_session_that_has_reported_nothing_reads_as_zeros() {
    let w = World::new().await;
    let _developer = w.agent_session().await;
    let grouped = w.store.task_usage(&w.task.id).await.unwrap();
    assert_eq!(
        grouped,
        vec![AgentUsage {
            agent_id: agent_of(&w.store, &w.task, "develop").await.id,
            usage: TokenUsage::default(),
        }]
    );
}

/// A goal's usage is grouped by seat rather than by agent — its orchestrator,
/// and the agents of every column of every task in one entry — and its
/// orchestrator counts: an orchestrator session belongs to no task, so
/// nothing under a task would ever have found it.
#[tokio::test]
async fn a_goals_usage_is_grouped_by_seat_and_counts_its_orchestrator() {
    let w = World::new().await;
    let orchestrator = w.session(Seat::Orchestrator, None, None).await;
    let developer = w.agent_session().await;
    let reviewer = w.review_session().await;

    for (session, spent) in [
        (&orchestrator, usage(40, 30, 8)),
        (&developer, usage(100, 80, 10)),
        (&reviewer, usage(20, 10, 4)),
    ] {
        w.store
            .upsert_session_usage(&session.id, "/x.jsonl", spent)
            .await
            .unwrap();
    }

    let grouped = w.store.goal_usage(&w.goal.id).await.unwrap();
    assert_eq!(
        grouped,
        vec![
            SeatUsage {
                seat: Seat::Agent,
                usage: usage(120, 90, 14),
            },
            SeatUsage {
                seat: Seat::Orchestrator,
                usage: usage(40, 30, 8),
            },
        ]
    );
    assert_eq!(
        grouped.iter().map(|r| r.usage).sum::<TokenUsage>(),
        usage(160, 120, 22),
        "the goal's total is every session under it, the orchestrator included"
    );
}

/// Usage belongs to the session that spent it: deleting the goal takes the
/// sessions with it, and the rows keyed on them go too rather than outliving
/// the id that names them.
#[tokio::test]
async fn usage_goes_when_the_session_it_belonged_to_does() {
    let w = World::new().await;
    let session = w.agent_session().await;
    w.store
        .upsert_session_usage(&session.id, "/x.jsonl", usage(100, 80, 10))
        .await
        .unwrap();
    assert_eq!(usage_rows(&w._dir).await, 1);

    w.store.delete_goal(&w.goal.id).await.unwrap();
    assert_eq!(usage_rows(&w._dir).await, 0);
}

/// A skill is created, read, written over and deleted by name; what Ariadne
/// ships is refused deletion, and what the user wrote is refused a reset.
#[tokio::test]
async fn skill_crud_and_the_two_refusals_that_tell_them_apart() {
    let (store, _dir) = test_store().await;

    let shipped = store.get_skill("coding").await.unwrap();
    assert!(shipped.is_builtin());
    assert!(shipped.document_is_default());
    assert_eq!(
        shipped.document_text(),
        ariadne_store::defaults::default_skill_document("coding").unwrap()
    );

    let mine = store
        .create_skill(NewSkill {
            name: "api-design".into(),
            document: "---\nname: api-design\ndescription: shape an API\n---\n".into(),
        })
        .await
        .unwrap();
    assert!(!mine.is_builtin());
    assert_eq!(mine.summary(), "shape an API");

    // A name already taken is a conflict, whoever holds it.
    assert!(matches!(
        store
            .create_skill(NewSkill {
                name: "coding".into(),
                document: "---\nname: coding\n---\n".into(),
            })
            .await,
        Err(StoreError::Conflict(_))
    ));

    // A built-in is reset, never deleted; one of the user's own is deleted,
    // never reset — there is nothing behind it to go back to.
    assert!(matches!(
        store.delete_skill("coding").await,
        Err(StoreError::Conflict(_))
    ));
    assert!(matches!(
        store.reset_skill("api-design").await,
        Err(StoreError::Conflict(_))
    ));
    store.delete_skill("api-design").await.unwrap();
    assert!(matches!(
        store.get_skill("api-design").await,
        Err(StoreError::NotFound { .. })
    ));
}

/// Every shipped skill is seeded into a fresh database on the text Ariadne
/// ships, storing none of it — so a reworded default reaches a database
/// nobody has touched, and a reset drops what was written rather than copying
/// a default in.
#[tokio::test]
async fn a_fresh_database_is_seeded_with_every_shipped_skill_on_its_own_text() {
    let (store, _dir) = test_store().await;

    let skills = store.list_skills().await.unwrap();
    assert_eq!(skills.len(), ariadne_store::defaults::BUILTIN_SKILLS.len());
    assert!(
        skills
            .iter()
            .all(|s| s.is_builtin() && s.document_is_default()),
        "every seeded skill runs on the shipped text"
    );

    let edited = store
        .set_skill_document("coding", "---\nname: coding\ndescription: ours\n---\n")
        .await
        .unwrap();
    assert!(!edited.document_is_default());
    assert_eq!(edited.summary(), "ours");

    let reset = store.reset_skill("coding").await.unwrap();
    assert!(
        reset.document_is_default(),
        "the reset dropped the text rather than copying the default in"
    );
    assert_eq!(
        reset.document_text(),
        ariadne_store::defaults::default_skill_document("coding").unwrap()
    );
}

/// A skill nothing loads is deleted; one an agent still carries is refused,
/// so a task cannot be left naming a skill that is gone.
#[tokio::test]
async fn a_skill_an_agent_still_loads_cannot_be_deleted() {
    let (store, _dir) = test_store().await;
    let (goal, repo) = seed_goal(&store).await;
    store
        .create_skill(NewSkill {
            name: "api-design".into(),
            document: "---\nname: api-design\ndescription: shape an API\n---\n".into(),
        })
        .await
        .unwrap();
    store
        .create_task(NewTask {
            goal_id: goal.id.clone(),
            repo_id: repo.id.clone(),
            title: "Shape it".into(),
            description: "do things".into(),
            agents: vec![NewTaskAgent::new("develop", ["api-design"], default_pin())],
            depends_on: vec![],
        })
        .await
        .unwrap();

    let refused = store.delete_skill("api-design").await;
    assert!(
        matches!(refused, Err(StoreError::Conflict(_))),
        "{refused:?}"
    );
}

/// A skill a workflow's own column still names cannot be deleted, even where
/// no goal and no agent has ever loaded it: a goal created on that workflow
/// later would snapshot the column onto a skill already gone.
#[tokio::test]
async fn a_skill_a_workflow_column_still_names_cannot_be_deleted() {
    let (store, _dir) = test_store().await;
    store
        .create_skill(NewSkill {
            name: "custom-check".into(),
            document: "---\nname: custom-check\ndescription: check it\n---\n".into(),
        })
        .await
        .unwrap();
    store
        .create_workflow(NewWorkflow {
            name: "custom-wf".into(),
            document:
                "workflow custom-wf\n  build[Build]\n    Do the work.\n    skills: custom-check\n"
                    .into(),
        })
        .await
        .unwrap();

    let refused = store.delete_skill("custom-check").await;
    assert!(
        matches!(refused, Err(StoreError::Conflict(_))),
        "{refused:?}"
    );
}

/// A skill no workflow names any more, but a goal already snapshotted onto
/// its columns, still cannot be deleted: staffing a task on that goal reads
/// the snapshot, not the catalog, and would fail naming a skill nobody can
/// explain the loss of.
#[tokio::test]
async fn a_skill_a_goals_snapshot_still_names_cannot_be_deleted() {
    let (store, _dir) = test_store().await;
    let repo = seed_repository(&store).await;
    store
        .create_skill(NewSkill {
            name: "custom-check".into(),
            document: "---\nname: custom-check\ndescription: check it\n---\n".into(),
        })
        .await
        .unwrap();
    store
        .create_workflow(NewWorkflow {
            name: "custom-wf".into(),
            document:
                "workflow custom-wf\n  build[Build]\n    Do the work.\n    skills: custom-check\n"
                    .into(),
        })
        .await
        .unwrap();
    store
        .create_goal(NewGoal {
            workflow: Some("custom-wf".into()),
            issue_url: None,
            title: "Test goal".into(),
            description: "desc".into(),
            repository_ids: vec![repo.id.clone()],
            pin: default_pin(),
        })
        .await
        .unwrap();

    // The workflow no longer names it, but the goal's own snapshot still
    // does, and no task has staffed it yet.
    store
        .set_workflow_document(
            "custom-wf",
            "workflow custom-wf\n  build[Build]\n    Do the work.\n",
        )
        .await
        .unwrap();

    let refused = store.delete_skill("custom-check").await;
    assert!(
        matches!(refused, Err(StoreError::Conflict(_))),
        "{refused:?}"
    );
}

/// An agent can only be staffed on a skill that exists: the name is a
/// reference, and the refusal says which name it was.
#[tokio::test]
async fn an_agent_cannot_be_staffed_on_a_skill_nothing_answers_to() {
    let (store, _dir) = test_store().await;
    let (goal, repo) = seed_goal(&store).await;

    let refused = store
        .create_task(NewTask {
            goal_id: goal.id.clone(),
            repo_id: repo.id.clone(),
            title: "Guess".into(),
            description: "do things".into(),
            agents: vec![NewTaskAgent::new("develop", ["telepathy"], default_pin())],
            depends_on: vec![],
        })
        .await;
    let message = format!("{:?}", refused.expect_err("no such skill"));
    assert!(message.contains("telepathy"), "{message}");
}

/// The `orchestration` skill is the orchestrator's own playbook, marked by
/// name and nothing stored: a task agent staffed on it — at creation, or by a
/// later edit of its skills — is refused, and the refusal says whose the
/// skill is.
#[tokio::test]
async fn a_task_agent_cannot_be_staffed_on_the_orchestrators_skill() {
    let (store, _dir) = test_store().await;
    let (goal, repo) = seed_goal(&store).await;

    let refused = store
        .create_task(NewTask {
            goal_id: goal.id.clone(),
            repo_id: repo.id.clone(),
            title: "Plan it".into(),
            description: "do things".into(),
            agents: vec![NewTaskAgent::new(
                "develop",
                ["orchestration"],
                default_pin(),
            )],
            depends_on: vec![],
        })
        .await;
    let message = format!("{:?}", refused.expect_err("the orchestrator's skill"));
    assert!(message.contains("orchestration"), "{message}");
    assert!(message.contains("orchestrator"), "{message}");

    // The same door is closed on a re-staff: a column's agent cannot be
    // moved onto it either.
    let task = seed_task(&store, &goal, &repo, vec![]).await;
    let reviewer = agent_of(&store, &task, "review").await;
    let refused = store
        .set_agent_skills(&reviewer.id, &["orchestration".to_string()])
        .await;
    assert!(
        matches!(refused, Err(StoreError::Conflict(_))),
        "{refused:?}"
    );
}

/// `pr-reviewer` is the daemon's own, loaded onto the session that reviews a
/// request the user was asked to review (029): no staffing names it, at
/// creation or by a later edit. `pr-babysit` is a task agent's: the `pr`
/// column of the shipped request workflow stages it (030), so an agent may
/// be staffed on it.
#[tokio::test]
async fn a_task_agent_cannot_be_staffed_on_the_reviewer_sessions_skill() {
    let (store, _dir) = test_store().await;
    let (goal, repo) = seed_goal(&store).await;
    let refused = store
        .create_task(NewTask {
            goal_id: goal.id.clone(),
            repo_id: repo.id.clone(),
            title: "Watch it".into(),
            description: "do things".into(),
            agents: vec![NewTaskAgent::new("develop", ["pr-reviewer"], default_pin())],
            depends_on: vec![],
        })
        .await;
    let message = format!("{:?}", refused.expect_err("the reviewer session's skill"));
    assert!(message.contains("loaded by Ariadne itself"), "{message}");
    let task = seed_task(&store, &goal, &repo, vec![]).await;
    let author = agent_of(&store, &task, "develop").await;
    assert!(matches!(
        store
            .set_agent_skills(&author.id, &["pr-reviewer".to_string()])
            .await,
        Err(StoreError::Conflict(_))
    ));

    store
        .set_agent_skills(&author.id, &["pr-babysit".to_string()])
        .await
        .expect("pr-babysit is a task agent's skill");
    assert_eq!(
        store.agent_skills(&author.id).await.unwrap()[0].name,
        "pr-babysit"
    );
}

/// An edit replaces the column staffing whole: every column is staffed
/// with the skills and the pin the edit gave it. An agent already on its
/// column keeps its row, and with it the sessions and messages that name
/// it; a column staffed for the first time gets a new row, and a column the
/// edit leaves out loses its agent. A task is edited while it is not running
/// — pending, ready, or failed and waiting for a retry, which is when the
/// orchestrator staffs a column the task lacked an agent on — and never while
/// it is in progress, where a live session would be replaced under its agent.
#[tokio::test]
async fn an_edit_replaces_the_whole_column_staffing() {
    let w = World::new().await;
    let store = &w.store;
    let before = store.list_task_agents(&w.task.id).await.unwrap();
    assert_eq!(before.len(), 3);
    // A session of the develop agent, and a message to it: what a re-staffing
    // has to keep.
    let develop_session = w.agent_session().await;
    let said = store
        .send_message(NewMessage {
            goal_id: w.goal.id.clone(),
            task_id: Some(w.task.id.clone()),
            kind: MessageKind::Message,
            from_actor: Actor::Orchestrator,
            from_agent_id: None,
            from_session: None,
            to_actor: Actor::Agent,
            to_agent_id: Some(before[0].id.clone()),
            body: "Kept across the edit.".into(),
        })
        .await
        .unwrap();
    let restaffed = || {
        vec![
            NewTaskAgent::new(
                "develop",
                ["coding", "debugging"],
                pin("codex-acp:gpt-5.6-terra"),
            ),
            NewTaskAgent::new("review", Vec::<String>::new(), default_pin()),
            NewTaskAgent::new("merge", Vec::<String>::new(), default_pin()),
        ]
    };
    let staffing = async |task_id: &str| {
        let mut rows = Vec::new();
        for agent in store.list_task_agents(task_id).await.unwrap() {
            let skills: Vec<String> = store
                .agent_skills(&agent.id)
                .await
                .unwrap()
                .into_iter()
                .map(|s| s.name)
                .collect();
            rows.push((agent.step, agent.model, skills));
        }
        rows
    };

    // Pending: replaced whole.
    store
        .update_task(
            &w.task.id,
            TaskUpdate {
                agents: Some(restaffed()),
                title: Some("Edited".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let after = store.list_task_agents(&w.task.id).await.unwrap();
    assert_eq!(
        after.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(),
        before.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(),
        "an agent already on its column keeps its row"
    );
    assert_eq!(
        store
            .get_session(&develop_session.id)
            .await
            .unwrap()
            .task_agent_id
            .as_deref(),
        Some(before[0].id.as_str()),
        "and the session that names it stays"
    );
    assert_eq!(
        store.get_message(&said.id).await.unwrap().body,
        "Kept across the edit."
    );
    assert_eq!(
        staffing(&w.task.id).await,
        [
            (
                "develop".to_string(),
                "codex-acp:gpt-5.6-terra".to_string(),
                vec!["coding".to_string(), "debugging".to_string()]
            ),
            (
                "review".to_string(),
                "stub:test-model".to_string(),
                vec!["code-review".to_string()]
            ),
            (
                "merge".to_string(),
                "stub:test-model".to_string(),
                vec!["merge".to_string()]
            ),
        ]
    );
    assert_eq!(w.task().await.title, "Edited");

    // An edit that names no agents leaves the staffing alone.
    store
        .update_task(
            &w.task.id,
            TaskUpdate {
                description: Some("still the same agents".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        store
            .list_task_agents(&w.task.id)
            .await
            .unwrap()
            .iter()
            .map(|a| a.id.as_str())
            .collect::<Vec<_>>(),
        after.iter().map(|a| a.id.as_str()).collect::<Vec<_>>()
    );

    // Ready: still editable. In progress: refused, and the staffing untouched.
    walk_to(store, &w.task.id, TaskStatus::Ready).await;
    store
        .update_task(
            &w.task.id,
            TaskUpdate {
                agents: Some(column_agents(store, &w.goal).await),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let ready_staffing = store.list_task_agents(&w.task.id).await.unwrap();
    assert_eq!(ready_staffing[0].model, "stub:test-model");
    walk_to(store, &w.task.id, TaskStatus::InProgress).await;
    let refused = store
        .update_task(
            &w.task.id,
            TaskUpdate {
                agents: Some(restaffed()),
                ..Default::default()
            },
        )
        .await;
    assert!(
        matches!(refused, Err(StoreError::Conflict(_))),
        "{refused:?}"
    );
    assert_eq!(
        store
            .list_task_agents(&w.task.id)
            .await
            .unwrap()
            .iter()
            .map(|a| a.id.as_str())
            .collect::<Vec<_>>(),
        ready_staffing
            .iter()
            .map(|a| a.id.as_str())
            .collect::<Vec<_>>()
    );

    // Failed: editable again, which is how a column a failed task lacks an
    // agent on is staffed before the retry.
    store
        .transition_task(&w.task.id, TaskStatus::Failed, Actor::Daemon, None, None)
        .await
        .unwrap();
    store
        .update_task(
            &w.task.id,
            TaskUpdate {
                agents: Some(vec![NewTaskAgent::new(
                    "develop",
                    Vec::<String>::new(),
                    default_pin(),
                )]),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let failed = w.task().await;
    assert_eq!(
        store.unstaffed_columns(&failed).await.unwrap(),
        ["review", "merge"]
    );
    // The develop agent kept its row through the edit that dropped the other
    // two columns; the message to it is still there.
    assert_eq!(
        store.list_task_agents(&w.task.id).await.unwrap()[0].id,
        before[0].id
    );
    assert_eq!(
        store.get_message(&said.id).await.unwrap().to_agent_id,
        Some(before[0].id.clone())
    );
    store
        .update_task(
            &w.task.id,
            TaskUpdate {
                agents: Some(restaffed()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(store.unstaffed_columns(&failed).await.unwrap().is_empty());
    assert_eq!(
        agent_of(store, &failed, "develop").await.model,
        "codex-acp:gpt-5.6-terra"
    );
    // The columns staffed again are new rows: their agents had gone with the
    // edit that left them out.
    let again = store.list_task_agents(&w.task.id).await.unwrap();
    assert_eq!(again[0].id, before[0].id);
    assert!(
        again[1..]
            .iter()
            .all(|a| before.iter().all(|b| b.id != a.id))
    );

    // A column whose agent has a session or a message behind it cannot be
    // left out: dropping the agent would drop them.
    let review_session = w
        .session(Seat::Agent, Some(&again[1].id), Some(&w.task.id))
        .await;
    let refused = store
        .update_task(
            &w.task.id,
            TaskUpdate {
                agents: Some(vec![NewTaskAgent::new(
                    "develop",
                    Vec::<String>::new(),
                    default_pin(),
                )]),
                ..Default::default()
            },
        )
        .await;
    assert!(
        matches!(&refused, Err(StoreError::Conflict(message)) if message.contains("column review")),
        "{refused:?}"
    );
    assert_eq!(store.list_task_agents(&w.task.id).await.unwrap().len(), 3);
    assert!(store.get_session(&review_session.id).await.is_ok());
}

/// An agent event takes its id and is published under one lock. A writer
/// that has to wait for the lock takes its id after the one that held it, so
/// a higher id is never taken by a writer that publishes before a lower one.
#[tokio::test]
async fn an_event_that_waits_for_the_event_lock_takes_its_id_after_the_one_that_held_it() {
    let (store, _dir) = test_store().await;
    let held = store.event_order().lock().await;
    let writer = {
        let store = store.clone();
        tokio::spawn(async move {
            store
                .create_event(NewAgentEvent {
                    session_id: None,
                    task_id: None,
                    kind: "stop".into(),
                    payload: serde_json::json!({}),
                })
                .await
                .unwrap()
        })
    };
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
    // Taken while the writer waits: the id the writer would have taken
    // already, had it not waited.
    let meanwhile = ariadne_core::id::new_id();
    drop(held);

    let written = writer.await.unwrap();

    assert!(
        written.id > meanwhile,
        "the waiting writer took its id after the lock was released: {} > {meanwhile}",
        written.id
    );
}

/// Two events written at once are published in the order of their ids,
/// whichever of them reached the write pool first.
#[tokio::test]
async fn events_written_at_once_are_published_in_id_order() {
    let (store, _dir) = test_store().await;
    let mut changes = store.watch_changes().expect("the only watcher");
    let held = store.event_order().lock().await;
    let writers: Vec<_> = ["first", "second"]
        .into_iter()
        .map(|kind| {
            let store = store.clone();
            tokio::spawn(async move {
                store
                    .create_event(NewAgentEvent {
                        session_id: None,
                        task_id: None,
                        kind: kind.into(),
                        payload: serde_json::json!({}),
                    })
                    .await
                    .unwrap()
            })
        })
        .collect();
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
    drop(held);
    for writer in writers {
        writer.await.unwrap();
    }

    let mut published = Vec::new();
    while published.len() < 2 {
        if let Change::AgentEventCreated(event) = changes.recv().await.expect("the store is open") {
            published.push(event.id);
        }
    }

    assert!(
        published[0] < published[1],
        "published in id order: {published:?}"
    );
}

/// The payload of every event row, as the row holds it: the store answers
/// the text an agent reported, so only the table itself can say what the
/// bytes under it are and which codec packed them.
async fn stored_payloads(dir: &tempfile::TempDir) -> Vec<(Vec<u8>, String)> {
    let path = dir.path().join("test.db");
    let pool = sqlx::SqlitePool::connect(&format!("sqlite://{}", path.display()))
        .await
        .unwrap();
    let rows = sqlx::query_as("SELECT payload, payload_codec FROM agent_events ORDER BY id")
        .fetch_all(&pool)
        .await
        .unwrap();
    pool.close().await;
    rows
}

/// An event of a deleted goal is readable by nobody, so none of them is
/// kept. The orchestrator's is the one this turns on: its session carries no
/// task, so a task cascade never reached it and the row outlived its goal
/// for good.
#[tokio::test]
async fn deleting_a_goal_leaves_no_event_of_its_sessions_or_its_tasks() {
    let w = World::new().await;
    let orchestrator = w.session(Seat::Orchestrator, None, None).await;
    let author = w.agent_session().await;
    for session in [&orchestrator, &author] {
        w.store
            .create_event(NewAgentEvent {
                session_id: Some(session.id.clone()),
                task_id: session.task_id.clone(),
                kind: "stop".into(),
                payload: serde_json::json!({}),
            })
            .await
            .unwrap();
    }
    // One reported against the task alone, which no session cascade reaches.
    w.store
        .create_event(NewAgentEvent {
            session_id: None,
            task_id: Some(w.task.id.clone()),
            kind: "stop".into(),
            payload: serde_json::json!({}),
        })
        .await
        .unwrap();
    assert_eq!(stored_payloads(&w._dir).await.len(), 3);

    w.store.delete_goal(&w.goal.id).await.unwrap();

    assert!(
        stored_payloads(&w._dir).await.is_empty(),
        "the goal took every event of its sessions and its tasks with it"
    );
}

/// A payload is packed on the way in and unpacked on the way out, so what an
/// agent reported is what every reader of the event gets back — a long one
/// included, which is where packing is worth anything at all.
#[tokio::test]
async fn a_hundred_kilobyte_payload_reads_back_word_for_word() {
    let w = World::new().await;
    let session = w.agent_session().await;
    let long: String = (0..2000)
        .map(|n| format!("line {n}: the agent read a file and said something about it\n"))
        .collect();
    assert!(long.len() > 100 * 1024, "{} bytes", long.len());
    let payload = serde_json::json!({"tool_name": "Read", "tool_response": long});

    let written = w
        .store
        .create_event(NewAgentEvent {
            session_id: Some(session.id.clone()),
            task_id: Some(w.task.id.clone()),
            kind: "post_tool_use".into(),
            payload: payload.clone(),
        })
        .await
        .unwrap();

    assert_eq!(written.payload, payload.to_string());
    let read_back = w
        .store
        .list_session_events(&session.id)
        .await
        .unwrap()
        .pop()
        .expect("the event that was written");
    assert_eq!(read_back.payload, payload.to_string());
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&read_back.payload).unwrap(),
        payload
    );
}

/// What the row holds is smaller than what it reads, which is the whole
/// point: an event of any size worth storing packs. A payload too short for
/// deflate to shrink is stored as it came, under the codec that says so, so
/// no row is ever bigger for being packed.
#[tokio::test]
async fn a_payload_above_a_kilobyte_is_stored_smaller_than_it_reads() {
    let w = World::new().await;
    let session = w.agent_session().await;
    let long = "cargo nextest run -p ariadne-store ".repeat(50);
    let long = serde_json::json!({"tool_name": "Bash", "tool_input": {"command": long}});
    assert!(long.to_string().len() > 1024, "a payload above a kilobyte");
    let short = serde_json::json!({});

    for payload in [&long, &short] {
        w.store
            .create_event(NewAgentEvent {
                session_id: Some(session.id.clone()),
                task_id: Some(w.task.id.clone()),
                kind: "post_tool_use".into(),
                payload: payload.clone(),
            })
            .await
            .unwrap();
    }

    let stored = stored_payloads(&w._dir).await;
    let (packed, codec) = &stored[0];
    assert_eq!(codec, "deflate");
    assert!(
        packed.len() < long.to_string().len(),
        "{} bytes stored against {} read",
        packed.len(),
        long.to_string().len()
    );
    let (plain, codec) = &stored[1];
    assert_eq!(codec, "none");
    assert_eq!(plain.len(), short.to_string().len());
}

/// The commit path no longer checkpoints — `Store::open` turns SQLite's
/// automatic one off, because it runs on the connection that committed and
/// cannot reset the log while a reader is on an older snapshot. What keeps
/// the log from growing without end is `checkpoint`, so it has to actually
/// fold it back in.
#[tokio::test]
async fn a_checkpoint_folds_the_write_ahead_log_back_in() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ariadne.db");
    let store = Store::open(&path).await.unwrap();
    let wal = path.with_extension("db-wal");

    // Enough writes to put pages in the log. Nothing has folded them in:
    // the commit path does not, and nothing has asked yet.
    for n in 0..200 {
        store
            .create_repository(ariadne_store::NewRepository {
                default_workflow: None,
                path: dir.path().join(format!("repo-{n}")).display().to_string(),
                base_branch: "main".into(),
                description: None,
                permission_mode: None,
            })
            .await
            .unwrap();
    }
    let before = std::fs::metadata(&wal).map(|f| f.len()).unwrap_or(0);
    assert!(before > 0, "the writes went to the log, not the database");

    store.checkpoint().await.unwrap();

    let after = std::fs::metadata(&wal).map(|f| f.len()).unwrap_or(0);
    assert!(
        after < before,
        "the log was folded back in: {before} bytes before, {after} after"
    );
}

/// The AI permission settings are one row, seeded with the defaults a fresh daemon
/// answers with. A write moves the columns it names and leaves the rest, and
/// the row survives a reopen: an install that ran for minutes must not be
/// forgotten because the daemon restarted.
#[tokio::test]
async fn the_ai_permission_settings_are_one_row_that_takes_partial_writes() {
    let (store, dir) = test_store().await;

    let defaults = store.ai_permission_settings().await.unwrap();
    assert!(!defaults.enabled);
    assert_eq!(defaults.allow_threshold, 0.0201);
    assert_eq!(defaults.deny_threshold, 0.6321);
    assert!(
        !defaults.thresholds_hand_set,
        "a fresh pair follows the flavour"
    );
    assert_eq!(defaults.flavour, "4b");
    assert_eq!(defaults.device, None);
    assert_eq!(defaults.state, "disabled");
    assert_eq!(defaults.installed_release, None);
    assert_eq!(defaults.latest_release, None);
    assert!(!defaults.weights_present);
    assert_eq!(defaults.last_refresh_at, None);
    assert_eq!(defaults.last_error, None);

    // What the user chose.
    let partly_chosen = store
        .update_ai_permission_settings(AiPermissionSettingsUpdate {
            enabled: Some(true),
            allow_threshold: Some(0.2),
            flavour: Some("9b".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(partly_chosen.allow_threshold, 0.2);
    assert_eq!(partly_chosen.deny_threshold, 0.6321);
    let chosen = store
        .update_ai_permission_settings(AiPermissionSettingsUpdate {
            deny_threshold: Some(0.8),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(chosen.enabled);
    assert_eq!(chosen.allow_threshold, 0.2);
    assert_eq!(chosen.deny_threshold, 0.8);
    assert!(
        !chosen.thresholds_hand_set,
        "the flag moves only when written"
    );
    assert_eq!(chosen.flavour, "9b");
    let hand_set = store
        .update_ai_permission_settings(AiPermissionSettingsUpdate {
            thresholds_hand_set: Some(true),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(hand_set.thresholds_hand_set);
    assert_eq!(hand_set.allow_threshold, 0.2);
    assert_eq!(chosen.state, "disabled", "a choice is not an install");

    // What the installer found, written without touching what the user chose.
    let installed = store
        .update_ai_permission_settings(AiPermissionSettingsUpdate {
            state: Some("ready".into()),
            installed_release: Some(Some("v0.1.4".into())),
            latest_release: Some(Some("v0.1.4".into())),
            weights_present: Some(true),
            last_refresh_at: Some(Some("2026-09-26T10:00:00.000Z".into())),
            last_error: Some(None),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(installed.state, "ready");
    assert_eq!(installed.installed_release.as_deref(), Some("v0.1.4"));
    assert!(installed.weights_present);
    assert_eq!(installed.allow_threshold, 0.2, "the user's choice stayed");
    assert_eq!(installed.deny_threshold, 0.8, "the user's choice stayed");

    // The device the daemon fills in at startup, written without touching
    // the flavour the user chose.
    let with_device = store
        .update_ai_permission_settings(AiPermissionSettingsUpdate {
            device: Some("mlx".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(with_device.device.as_deref(), Some("mlx"));
    assert_eq!(with_device.flavour, "9b");
    assert_eq!(with_device.installed_release.as_deref(), Some("v0.1.4"));

    // A state written only while enabled lands on an enabled row, and leaves
    // a row turned off at `disabled`: what an install that ends after the model
    // was turned off writes.
    let moved = store
        .update_ai_permission_settings(AiPermissionSettingsUpdate {
            state: Some("installing".into()),
            state_while_enabled: true,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(moved.state, "installing");
    store
        .update_ai_permission_settings(AiPermissionSettingsUpdate {
            enabled: Some(false),
            state: Some("disabled".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    let kept_off = store
        .update_ai_permission_settings(AiPermissionSettingsUpdate {
            state: Some("ready".into()),
            state_while_enabled: true,
            installed_release: Some(Some("v0.1.5".into())),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(kept_off.state, "disabled", "a row turned off stays off");
    assert_eq!(
        kept_off.installed_release.as_deref(),
        Some("v0.1.5"),
        "and the rest of the write still lands"
    );
    store
        .update_ai_permission_settings(AiPermissionSettingsUpdate {
            enabled: Some(true),
            state: Some("ready".into()),
            installed_release: Some(Some("v0.1.4".into())),
            ..Default::default()
        })
        .await
        .unwrap();

    store.close().await;
    let reopened = Store::open(dir.path().join("test.db")).await.unwrap();
    let kept = reopened.ai_permission_settings().await.unwrap();
    assert!(kept.enabled);
    assert_eq!(kept.state, "ready");
    assert_eq!(kept.allow_threshold, 0.2);
    assert_eq!(kept.deny_threshold, 0.8);
    assert!(kept.thresholds_hand_set);
    assert_eq!(kept.installed_release.as_deref(), Some("v0.1.4"));
}

/// The `ai` mode is one a repository can be registered and edited into: the
/// column takes all four spellings and reads back as the mode itself.
#[tokio::test]
async fn a_repository_takes_the_ai_permission_mode() {
    let (store, _dir) = test_store().await;
    let repo = store
        .create_repository(NewRepository {
            default_workflow: None,
            path: "/tmp/ai-repo".into(),
            base_branch: "main".into(),
            description: None,
            permission_mode: Some(PermissionMode::Ai),
        })
        .await
        .unwrap();
    assert_eq!(repo.permission_mode(), PermissionMode::Ai);

    let edited = store
        .update_repository(
            &repo.id,
            RepositoryUpdate {
                permission_mode: Some(PermissionMode::Learn),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(edited.permission_mode(), PermissionMode::Learn);
}

/// A `session_ended` fact about one run of an agent, on `repo_id`.
fn ended(repo_id: &str, launch_id: &str, data: serde_json::Value) -> NewStatFact {
    NewStatFact {
        kind: "session_ended".into(),
        repo_id: Some(repo_id.into()),
        goal_id: Some("01GOAL".into()),
        task_id: Some("01TASK".into()),
        session_id: Some("01SESSION".into()),
        launch_id: Some(launch_id.into()),
        seat: Some("agent".into()),
        model: Some("stub:test-model".into()),
        effort: Some("high".into()),
        skills: vec!["coding".into(), "migration".into()],
        data,
    }
}

/// The data of a run that ended `status`, flagged `reason`, after
/// `lifetime_secs`, having spent 1000 in, 800 of it cached, and 100 out.
fn run(status: &str, reason: Option<&str>, lifetime_secs: i64) -> serde_json::Value {
    serde_json::json!({
        "status": status, "attention_reason": reason, "lifetime_secs": lifetime_secs,
        "turns": 3, "input_tokens": 1000, "cached_input_tokens": 800, "output_tokens": 100,
    })
}

/// Every fact of `kind` in the ledger of the database under `dir`, oldest
/// first, as its model and its data. Straight SQL: the store reads the
/// ledger only as aggregates, and a test of the fact itself needs the row.
async fn ledger(dir: &std::path::Path, kind: &str) -> Vec<(Option<String>, serde_json::Value)> {
    let pool = sqlx::SqlitePool::connect(&format!("sqlite://{}", dir.join("test.db").display()))
        .await
        .unwrap();
    let rows: Vec<(Option<String>, String)> =
        sqlx::query_as("SELECT model, data FROM stat_facts WHERE kind = ? ORDER BY id")
            .bind(kind)
            .fetch_all(&pool)
            .await
            .unwrap();
    pool.close().await;
    rows.into_iter()
        .map(|(model, data)| (model, serde_json::from_str(&data).unwrap()))
        .collect()
}

/// A fact names its goal by id and holds no key to it, so deleting the goal
/// leaves the fact where it was.
#[tokio::test]
async fn a_fact_outlives_the_goal_it_is_about() {
    let w = World::new().await;
    let mut fact = ended(&w.repo.id, "launch-1", run("exited", None, 10));
    fact.goal_id = Some(w.goal.id.clone());
    fact.task_id = Some(w.task.id.clone());
    w.store.record_fact(fact).await.unwrap();
    w.store.delete_goal(&w.goal.id).await.unwrap();
    assert_eq!(ledger(w._dir.path(), "session_ended").await.len(), 1);
}

/// One run of a session ends once in the ledger: a second fact of the same
/// kind for the same launch is refused, and a fact for the next launch is
/// kept.
#[tokio::test]
async fn a_fact_is_recorded_once_per_launch() {
    let (store, dir) = test_store().await;
    let first = ended("01REPO", "launch-1", run("exited", None, 10));
    assert!(
        store
            .record_fact_once_per_launch(first.clone())
            .await
            .unwrap()
    );
    assert!(!store.record_fact_once_per_launch(first).await.unwrap());
    let next = ended("01REPO", "launch-2", run("exited", None, 10));
    assert!(store.record_fact_once_per_launch(next).await.unwrap());
    assert_eq!(ledger(dir.path(), "session_ended").await.len(), 2);
}

/// The status write that ends a session is announced before its fact exists,
/// so the fact announces the session again once it is readable: a client
/// that refetches its stats on that event reads the fact. The same run
/// refused a second fact announces nothing.
#[tokio::test]
async fn a_fact_announces_its_session_once_it_is_readable() {
    let w = World::new().await;
    let session = w.agent_session().await;
    let mut changes = w.store.watch_changes().expect("the only watcher");
    w.store
        .set_session_status(&session.id, SessionStatus::Exited)
        .await
        .unwrap();
    while changes.try_recv().is_ok() {}

    let mut fact = ended(&w.repo.id, "launch-1", run("exited", None, 10));
    fact.session_id = Some(session.id.clone());
    assert!(
        w.store
            .record_fact_once_per_launch(fact.clone())
            .await
            .unwrap()
    );

    match changes.try_recv() {
        Ok(Change::SessionUpdated(updated)) => assert_eq!(updated.id, session.id),
        other => panic!("expected the session announced after its fact: {other:?}"),
    }
    assert_eq!(
        ledger(w._dir.path(), "session_ended").await.len(),
        1,
        "the fact is readable on that event"
    );

    assert!(!w.store.record_fact_once_per_launch(fact).await.unwrap());
    assert!(
        changes.try_recv().is_err(),
        "a refused fact announces nothing"
    );
}

/// A session's end answers the row as that write left it: ended, its prompt
/// flag gone, and the reason it ended carrying kept. The row is read in the
/// same transaction as the write, so a resume that writes the row next
/// cannot change what the caller is told.
#[tokio::test]
async fn ending_a_session_answers_the_row_its_write_left() {
    let w = World::new().await;
    let prompted = w.agent_session().await;
    w.store
        .set_session_attention(&prompted.id, AttentionReason::WaitingPermission)
        .await
        .unwrap();
    let ended = w
        .store
        .set_session_status(&prompted.id, SessionStatus::Exited)
        .await
        .unwrap();
    assert_eq!(ended.status(), SessionStatus::Exited);
    assert!(ended.ended_at.is_some());
    assert_eq!(ended.attention_reason(), None);

    let stalled = w.review_session().await;
    w.store
        .set_session_attention(&stalled.id, AttentionReason::Stalled)
        .await
        .unwrap();
    let ended = w
        .store
        .set_session_status(&stalled.id, SessionStatus::Failed)
        .await
        .unwrap();
    assert_eq!(ended.status(), SessionStatus::Failed);
    assert_eq!(ended.attention_reason(), Some(AttentionReason::Stalled));
}

/// A `task_ended` fact names its goal by id and holds no key to it, so
/// deleting the goal leaves the fact where it was. It is filled from the
/// agent of the column the task ended in: the last column's, for a task that
/// finished, with the column it ended in and whether the change landed.
#[tokio::test]
async fn an_outcome_fact_outlives_the_goal_it_is_about() {
    let w = World::new().await;
    walk_to(&w.store, &w.task.id, TaskStatus::Finished).await;
    w.store.delete_goal(&w.goal.id).await.unwrap();
    let facts = ledger(w._dir.path(), "task_ended").await;
    assert_eq!(facts.len(), 1, "{facts:?}");
    assert_eq!(facts[0].0.as_deref(), Some("stub:test-model"));
    assert_eq!(facts[0].1["status"], "finished");
    assert_eq!(facts[0].1["step"], "merge");
    assert_eq!(facts[0].1["landed"], true);
}

/// A clear is a completed spell of attention: the agent's event and an idle
/// report each record the reason and wait, while a no-op adds nothing.
#[tokio::test]
async fn attention_clears_write_facts_only_when_a_flag_falls() {
    let w = World::new().await;
    let session = w.agent_session().await;
    w.store
        .set_session_attention(&session.id, AttentionReason::WaitingPermission)
        .await
        .unwrap();
    w.store.clear_agent_attention(&session.id).await.unwrap();
    w.store
        .set_session_attention(&session.id, AttentionReason::Stalled)
        .await
        .unwrap();
    w.store
        .clear_attention_after_idle(&session.id)
        .await
        .unwrap();
    w.store.clear_session_attention(&session.id).await.unwrap();

    let facts = ledger(w._dir.path(), "attention").await;
    assert_eq!(facts.len(), 2, "a clear with no flag is not a fact");
    assert_eq!(facts[0].1["reason"], "waiting_permission");
    assert_eq!(facts[1].1["reason"], "stalled");
    assert!(facts.iter().all(|(_, data)| data["wait_secs"].is_number()));
}

/// The aggregate counts every attention source and applies both fact filters.
#[tokio::test]
async fn attention_stats_groups_facts_and_honours_the_filter() {
    let (store, _dir) = test_store().await;
    let fact = |kind: &str, repo_id: &str, data| NewStatFact {
        kind: kind.into(),
        repo_id: Some(repo_id.into()),
        goal_id: None,
        task_id: None,
        session_id: None,
        launch_id: None,
        seat: None,
        model: None,
        effort: None,
        skills: vec![],
        data,
    };
    for data in [
        serde_json::json!({"decided_by":"console","answer":"allow","wait_ms":10}),
        serde_json::json!({"decided_by":"console","answer":"deny","wait_ms":30}),
        serde_json::json!({"decided_by":"auto","answer":"cancelled","wait_ms":5}),
    ] {
        store
            .record_fact(fact("permission", "kept", data))
            .await
            .unwrap();
    }
    store
        .record_fact(fact(
            "attention",
            "kept",
            serde_json::json!({"reason":"waiting_input","wait_secs":4}),
        ))
        .await
        .unwrap();
    store
        .record_fact(fact(
            "attention",
            "kept",
            serde_json::json!({"reason":"waiting_input","wait_secs":8}),
        ))
        .await
        .unwrap();
    store
        .record_fact(fact(
            "session_ended",
            "kept",
            serde_json::json!({"status":"failed","attention_reason":"stalled"}),
        ))
        .await
        .unwrap();
    store
        .record_fact(fact(
            "switch",
            "kept",
            serde_json::json!({"reason":"exhausted"}),
        ))
        .await
        .unwrap();
    store
        .record_fact(fact(
            "permission",
            "other",
            serde_json::json!({"decided_by":"console","answer":"allow","wait_ms":1}),
        ))
        .await
        .unwrap();

    let stats = store
        .attention_stats(&StatsFilter {
            repo_id: Some("kept".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(stats.permissions.total, 3);
    assert_eq!(stats.permissions.person_share, 2.0 / 3.0);
    assert_eq!(stats.permissions.by_decider[0].decided_by, "console");
    assert_eq!(stats.permissions.by_decider[0].allowed, 1);
    assert_eq!(stats.permissions.by_decider[0].denied, 1);
    assert_eq!(stats.permissions.by_decider[0].mean_wait_ms, 20.0);
    let asked = stats
        .flags
        .iter()
        .find(|row| row.reason == "waiting_input")
        .unwrap();
    assert_eq!((asked.raised, asked.mean_wait_secs), (2, 6.0));
    assert_eq!(
        (
            stats.sessions_failed,
            stats.sessions_stalled,
            stats.exhaustions
        ),
        (1, 1, 1)
    );
    let none = store
        .attention_stats(&StatsFilter {
            since: Some(chrono::Utc::now() + chrono::Duration::seconds(1)),
            repo_id: Some("kept".into()),
        })
        .await
        .unwrap();
    assert_eq!(none.permissions.total, 0);
}

/// Remaining aggregates ignore an old `tool_call` fact in the append-only ledger.
#[tokio::test]
async fn remaining_stats_reads_ignore_legacy_tool_call_facts() {
    let (store, _dir) = test_store().await;
    let mut legacy = ended("01REPO", "legacy", serde_json::json!({"tool_name": "Bash"}));
    legacy.kind = "tool_call".into();
    store.record_fact(legacy).await.unwrap();

    let filter = StatsFilter::default();
    store.work_stats(&filter).await.unwrap();
    store.time_stats(&filter).await.unwrap();
    store.spend_stats(&filter).await.unwrap();
    store.model_stats(&filter).await.unwrap();
    store.attention_stats(&filter).await.unwrap();
}

/// A forge row as a detection writes it: `acme/widgets` on github.com, not
/// enabled.
fn widgets(repository_id: &str) -> SetForgeIntegration {
    SetForgeIntegration {
        repository_id: repository_id.into(),
        kind: ariadne_core::ForgeKind::Github,
        host: "GitHub.com".into(),
        owner: "Acme".into(),
        name: "Widgets".into(),
        remote: "origin".into(),
        enabled: false,
        login: None,
        review_model: None,
        review_effort: None,
    }
}

/// How the last fetch went is written only on a change (026): a failure and
/// its error, then the fetch that works again. A fetch that works on and on
/// writes nothing, so a timer fetch every 5 minutes publishes nothing. A
/// disabled integration fetches nothing, and records nothing.
#[tokio::test]
async fn the_last_fetch_error_is_written_only_when_it_changes() {
    let (store, _dir) = test_store().await;
    let repo = seed_repository(&store).await;
    store
        .set_forge_integration(widgets(&repo.id))
        .await
        .unwrap();
    assert!(
        !store
            .set_forge_fetch_error(&repo.id, Some("gh: HTTP 502"))
            .await
            .unwrap()
    );
    store
        .set_forge_integration(SetForgeIntegration {
            enabled: true,
            login: Some("me".into()),
            ..widgets(&repo.id)
        })
        .await
        .unwrap();
    assert!(
        store
            .set_forge_fetch_error(&repo.id, Some("gh: HTTP 502"))
            .await
            .unwrap()
    );
    assert!(
        !store
            .set_forge_fetch_error(&repo.id, Some("gh: HTTP 502"))
            .await
            .unwrap()
    );
    let read = store.forge_integration(&repo.id).await.unwrap().unwrap();
    assert_eq!(read.fetch_error.as_deref(), Some("gh: HTTP 502"));
    assert!(store.set_forge_fetch_error(&repo.id, None).await.unwrap());
    assert!(!store.set_forge_fetch_error(&repo.id, None).await.unwrap());
    let read = store.forge_integration(&repo.id).await.unwrap().unwrap();
    assert_eq!(read.fetch_error, None);
}

/// The forge row is read with its repository, lower-cased, in every list a
/// repository is read through, and goes with the repository.
#[tokio::test]
async fn a_forge_integration_is_read_with_its_repository_and_goes_with_it() {
    let (store, _dir) = test_store().await;
    let repo = seed_repository(&store).await;
    assert!(store.forge_integration(&repo.id).await.unwrap().is_none());

    let written = store
        .set_forge_integration(widgets(&repo.id))
        .await
        .unwrap();
    let forge = written.forge.expect("the write answers the row");
    assert_eq!(forge.kind(), ariadne_core::ForgeKind::Github);
    assert_eq!(
        (
            forge.host.as_str(),
            forge.owner.as_str(),
            forge.name.as_str()
        ),
        ("github.com", "acme", "widgets")
    );
    assert_eq!(
        store.get_repository(&repo.id).await.unwrap().forge,
        Some(forge.clone())
    );
    assert_eq!(
        store.list_repositories().await.unwrap()[0].forge,
        Some(forge)
    );
    assert!(store.enabled_forge_integrations().await.unwrap().is_empty());

    store.delete_repository(&repo.id).await.unwrap();
    assert!(store.forge_integration(&repo.id).await.unwrap().is_none());
}

/// One forge repository is enabled on one repository row at a time; the
/// refusal names the row that holds it.
#[tokio::test]
async fn one_forge_repository_is_enabled_on_one_row() {
    let (store, _dir) = test_store().await;
    let (first, second) = (seed_repository(&store).await, seed_repository(&store).await);
    let enabled = |id: &str| SetForgeIntegration {
        enabled: true,
        login: Some("octocat".into()),
        ..widgets(id)
    };
    store
        .set_forge_integration(enabled(&first.id))
        .await
        .unwrap();
    store
        .set_forge_integration(widgets(&second.id))
        .await
        .unwrap();

    let refused = store
        .set_forge_integration(enabled(&second.id))
        .await
        .unwrap_err();
    assert!(
        matches!(&refused, StoreError::Conflict(m) if m.contains(&first.id)),
        "{refused}"
    );
    assert!(
        store
            .forge_enabled_elsewhere(&second.id, "github.com", "acme", "widgets")
            .await
            .is_err()
    );
    assert!(
        store
            .forge_enabled_elsewhere(&first.id, "github.com", "acme", "widgets")
            .await
            .is_ok()
    );

    store
        .set_forge_integration(widgets(&first.id))
        .await
        .unwrap();
    store
        .set_forge_integration(enabled(&second.id))
        .await
        .unwrap();
    let rows = store.enabled_forge_integrations().await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].repository_id, second.id);
}

/// A row is Ariadne's bookkeeping of a request it works on (026): one per
/// repository and number, the first task that opened it staying its origin.
/// Starting and stopping the work says so per repository, and the sessions
/// that ran on a row outlive it, let go of it.
#[tokio::test]
async fn a_pull_request_row_keeps_its_identity_and_origin_and_its_sessions_outlive_it() {
    let (store, _dir, first) = store_with_my_pull_request().await;
    assert_eq!(first.role, "author");
    assert_eq!(first.origin_task_id, None);
    let mut changes = store.watch_changes().unwrap();
    let (second, created) = store
        .upsert_pull_request(NewPullRequest {
            repository_id: first.repository_id.clone(),
            number: 7,
            url: first.url.clone(),
            role: "author".into(),
            origin_task_id: None,
        })
        .await
        .unwrap();
    assert!(!created);
    assert_eq!(second.id, first.id);
    assert!(matches!(
        store
            .upsert_pull_request(NewPullRequest {
                repository_id: first.repository_id.clone(),
                number: 8,
                url: "https://github.com/acme/widgets/pull/8".into(),
                role: "maintainer".into(),
                origin_task_id: None,
            })
            .await,
        Err(StoreError::Invalid(_))
    ));
    assert_eq!(
        store
            .pull_request_by_number(&first.repository_id, 7)
            .await
            .unwrap()
            .map(|row| row.id),
        Some(first.id.clone())
    );
    let session = store
        .create_session(NewSession {
            goal_id: None,
            task_id: None,
            seat: Some(ariadne_core::Seat::Reviewer),
            task_agent_id: None,
            model: "stub:m".into(),
            effort: None,
            worktree_path: None,
            pull_request_id: Some(first.id.clone()),
        })
        .await
        .unwrap();
    while let Ok(change) = changes.try_recv() {
        drop(change);
    }
    let gone = store.delete_pull_request(&first.id).await.unwrap();
    assert_eq!(gone.map(|row| row.id), Some(first.id.clone()));
    loop {
        match changes.recv().await.unwrap() {
            Change::PullRequestsChanged(repository) => {
                assert_eq!(repository, first.repository_id);
                break;
            }
            _ => continue,
        }
    }
    let kept = store.get_session(&session.id).await.unwrap();
    assert_eq!(
        kept.pull_request_id, None,
        "the session is let go of the row"
    );
    assert!(
        store
            .delete_pull_request(&first.id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .list_pull_requests(PullRequestFilter::default())
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn pull_request_migration_preserves_existing_rows_and_a_recoverable_backup() {
    let dir = tempfile::tempdir().unwrap();
    let old_migrations = dir.path().join("migrations");
    std::fs::create_dir(&old_migrations).unwrap();
    for entry in std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/migrations")).unwrap() {
        let entry = entry.unwrap();
        if entry.file_name().to_string_lossy().as_ref() < "0007" {
            std::fs::copy(entry.path(), old_migrations.join(entry.file_name())).unwrap();
        }
    }
    let path = dir.path().join("old.db");
    let db = sqlx::sqlite::SqlitePoolOptions::new()
        .connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(&path)
                .create_if_missing(true),
        )
        .await
        .unwrap();
    sqlx::migrate::Migrator::new(old_migrations.as_path())
        .await
        .unwrap()
        .run(&db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO repositories (id,path,base_branch,created_at,updated_at) VALUES ('old-repo','/work/widgets','main','2026-10-01','2026-10-01')").execute(&db).await.unwrap();
    let backup = dir.path().join("backup.db");
    sqlx::query("VACUUM INTO ?")
        .bind(backup.to_str().unwrap())
        .execute(&db)
        .await
        .unwrap();
    db.close().await;
    let upgraded = Store::open(&path).await.unwrap();
    let row = upgraded.get_repository("old-repo").await.unwrap();
    assert_eq!(row.path, "/work/widgets");
    assert_eq!(row.base_branch, "main");
    assert!(
        upgraded
            .list_pull_requests(PullRequestFilter::default())
            .await
            .unwrap()
            .is_empty()
    );
    upgraded.close().await;
    let recovered = Store::open(&backup).await.unwrap();
    assert_eq!(
        recovered
            .get_repository("old-repo")
            .await
            .unwrap()
            .created_at,
        "2026-10-01"
    );
    assert_eq!(recovered.list_repositories().await.unwrap().len(), 1);
}

/// A store with one enabled GitHub repository, signed in as `me`, and one
/// open request of mine on it.
async fn store_with_my_pull_request() -> (Store, tempfile::TempDir, PullRequestRow) {
    let (store, dir) = test_store().await;
    let repo = store
        .create_repository(NewRepository {
            default_workflow: None,
            path: "/tmp/pull-request-comments".into(),
            base_branch: "main".into(),
            description: None,
            permission_mode: None,
        })
        .await
        .unwrap();
    store
        .set_forge_integration(SetForgeIntegration {
            repository_id: repo.id.clone(),
            kind: ariadne_core::ForgeKind::Github,
            host: "github.com".into(),
            owner: "acme".into(),
            name: "widgets".into(),
            remote: "origin".into(),
            enabled: true,
            login: Some("me".into()),
            review_model: None,
            review_effort: None,
        })
        .await
        .unwrap();
    let (row, _) = store
        .upsert_pull_request(NewPullRequest {
            repository_id: repo.id.clone(),
            number: 7,
            url: "https://github.com/acme/widgets/pull/7".into(),
            role: "author".into(),
            origin_task_id: None,
        })
        .await
        .unwrap();
    (store, dir, row)
}

/// A comment is the forge's; what the store keeps of one is a mark (026):
/// told, once a claim of the news that names it held, and given back by a
/// release; posted by a review, once a review session posted it (029). A
/// claim is a compare and set over the row's told mark and the comments'
/// marks, so the same news is not claimed twice.
#[tokio::test]
async fn comment_marks_are_claimed_once_released_whole_and_keep_the_review_mark() {
    let (store, _dir, row) = store_with_my_pull_request().await;
    let told = PullRequestTold {
        checks: vec!["lint".into()],
        behind_base: true,
        review_decision: "approved".into(),
        state: "open".into(),
        check_state: "failure".into(),
        head_sha: String::new(),
    };
    let before = PullRequestTold {
        checks: Vec::new(),
        behind_base: false,
        review_decision: "none".into(),
        state: "open".into(),
        check_state: "none".into(),
        head_sha: String::new(),
    };
    let comment = ["rc-1".to_string()];
    assert!(
        store
            .claim_pull_request_news(&row.id, &comment, &before, &told)
            .await
            .unwrap()
    );
    let claimed = store.get_pull_request(&row.id).await.unwrap();
    assert!(claimed.news_told_at.is_some());
    assert_eq!(claimed.told_checks, r#"["lint"]"#);
    assert_eq!(claimed.told_check_state.as_deref(), Some("failure"));
    let marks = store.pull_request_comment_marks(&row.id).await.unwrap();
    assert_eq!(marks.len(), 1);
    assert!(marks[0].told_at.is_some());
    assert!(
        !store
            .claim_pull_request_news(&row.id, &comment, &told, &told)
            .await
            .unwrap(),
        "a told comment is not claimed twice"
    );
    // A claim whose prompt never went out is given back whole.
    store
        .release_pull_request_news(&row.id, &comment, &before)
        .await
        .unwrap();
    let released = store.get_pull_request(&row.id).await.unwrap();
    assert_eq!(released.told_checks, "[]");
    assert!(!released.told_behind_base);
    assert!(
        store.pull_request_comment_marks(&row.id).await.unwrap()[0]
            .told_at
            .is_none()
    );
    // A news computed from a mark that moved since is refused.
    let stale = PullRequestTold {
        review_decision: "changes_requested".into(),
        ..before.clone()
    };
    assert!(
        !store
            .claim_pull_request_news(&row.id, &[], &stale, &told)
            .await
            .unwrap()
    );
    store
        .mark_review_comments(&row.id, &["rc-1".into(), "ic-2".into()])
        .await
        .unwrap();
    let mut marks = store.pull_request_comment_marks(&row.id).await.unwrap();
    marks.sort_by(|a, b| a.forge_id.cmp(&b.forge_id));
    assert!(marks.iter().all(|m| m.from_review));
    assert!(
        store
            .claim_pull_request_news(&row.id, &comment, &before, &told)
            .await
            .unwrap(),
        "a review's mark leaves the comment untold"
    );
    store.delete_pull_request(&row.id).await.unwrap();
    assert!(
        store
            .pull_request_comment_marks(&row.id)
            .await
            .unwrap()
            .is_empty(),
        "the marks go with the row"
    );
}

/// The ready flag a session reports moves the row, and a repeated ready
/// report says nothing moved.
#[tokio::test]
async fn a_pull_request_reports_ready_once() {
    let (store, _dir, row) = store_with_my_pull_request().await;
    let (ready, moved) = store.set_pull_request_ready(&row.id, true).await.unwrap();
    assert!(ready.ready && moved);
    let (_, moved) = store.set_pull_request_ready(&row.id, true).await.unwrap();
    assert!(!moved, "a repeat moves nothing");
    let (ready, moved) = store.set_pull_request_ready(&row.id, false).await.unwrap();
    assert!(!ready.ready && moved);
}

/// A reviewer row keeps the head its session last reviewed, and says once
/// whether that moved (029).
#[tokio::test]
async fn a_reviewed_sha_moves_once() {
    let (store, _dir, row) = store_with_my_pull_request().await;
    assert_eq!(row.reviewed_sha, None);
    let (reviewed, moved) = store
        .set_pull_request_reviewed(&row.id, "abc")
        .await
        .unwrap();
    assert!(moved);
    assert_eq!(reviewed.reviewed_sha.as_deref(), Some("abc"));
    let (_, moved) = store
        .set_pull_request_reviewed(&row.id, "abc")
        .await
        .unwrap();
    assert!(!moved, "the same sha moves nothing");
    let (_, moved) = store
        .set_pull_request_reviewed(&row.id, "def")
        .await
        .unwrap();
    assert!(moved, "a later sha moves it again");
}

/// A task's author keeps the request it opened (005), so the migration that
/// says so takes away what an older release gave a request of the user's
/// own: the session of its own, the row of a request no task opened, and the
/// pin that session ran on. A request a task opened, a request the user
/// reviews and its session, and every other forge setting stay.
#[tokio::test]
async fn the_migration_that_gives_requests_to_their_authors_keeps_task_and_review_rows() {
    let dir = tempfile::tempdir().unwrap();
    let old_migrations = dir.path().join("migrations");
    std::fs::create_dir(&old_migrations).unwrap();
    for entry in std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/migrations")).unwrap() {
        let entry = entry.unwrap();
        if entry.file_name().to_string_lossy().as_ref() < "0012" {
            std::fs::copy(entry.path(), old_migrations.join(entry.file_name())).unwrap();
        }
    }
    let path = dir.path().join("old.db");
    let db = sqlx::sqlite::SqlitePoolOptions::new()
        .connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(&path)
                .create_if_missing(true),
        )
        .await
        .unwrap();
    sqlx::migrate::Migrator::new(old_migrations.as_path())
        .await
        .unwrap()
        .run(&db)
        .await
        .unwrap();
    for statement in [
        "INSERT INTO repositories (id,path,base_branch,created_at,updated_at) VALUES ('r','/work/widgets','main','2026-10-01','2026-10-01')",
        "INSERT INTO forge_integrations (repository_id,kind,host,owner,name,remote,enabled,login,babysit_model,review_model,detected_at,updated_at) VALUES ('r','github','github.com','acme','widgets','origin',1,'me','stub:m','stub:r','2026-10-01','2026-10-01')",
        "INSERT INTO goals (id,title,description,created_at,updated_at,model) VALUES ('g','Ship','Body','2026-10-01','2026-10-01','stub:m')",
        "INSERT INTO tasks (id,goal_id,repo_id,title,description,branch,created_at,updated_at) VALUES ('t','g','r','Fix','Body','ariadne/fix','2026-10-01','2026-10-01')",
        "INSERT INTO pull_requests (id,repository_id,number,url,title,author_login,tracked_by,state,draft,head_branch,head_sha,base_branch,checks,review_decision,unanswered_comments,origin_task_id,opened_at,role,ready,last_seen_at,created_at,updated_at) VALUES ('mine','r',1,'https://github.com/acme/widgets/pull/1','Fix','me','forge','open',0,'fix-1','abc','main','none','none',0,NULL,'2026-10-01','author',0,'2026-10-01','2026-10-01','2026-10-01')",
        "INSERT INTO pull_requests (id,repository_id,number,url,title,author_login,tracked_by,state,draft,head_branch,head_sha,base_branch,checks,review_decision,unanswered_comments,origin_task_id,opened_at,role,ready,last_seen_at,created_at,updated_at) VALUES ('tasks','r',2,'https://github.com/acme/widgets/pull/2','Fix','me','forge','open',0,'fix-2','abc','main','none','none',0,'t','2026-10-01','author',0,'2026-10-01','2026-10-01','2026-10-01')",
        "INSERT INTO pull_requests (id,repository_id,number,url,title,author_login,tracked_by,state,draft,head_branch,head_sha,base_branch,checks,review_decision,unanswered_comments,origin_task_id,opened_at,role,ready,last_seen_at,created_at,updated_at) VALUES ('review','r',3,'https://github.com/acme/widgets/pull/3','Fix','someone','forge','open',0,'fix-3','abc','main','none','none',0,NULL,'2026-10-01','reviewer',0,'2026-10-01','2026-10-01','2026-10-01')",
        "INSERT INTO agent_sessions (id,model,status,created_at,seat,pull_request_id) VALUES ('babysit','stub:m','idle','2026-10-01','author','tasks')",
        "INSERT INTO agent_sessions (id,model,status,created_at,seat,pull_request_id) VALUES ('reviewing','stub:r','idle','2026-10-01','reviewer','review')",
    ] {
        sqlx::query(statement).execute(&db).await.unwrap();
    }
    db.close().await;

    let upgraded = Store::open(&path).await.unwrap();
    assert!(matches!(
        upgraded.get_pull_request("mine").await,
        Err(StoreError::NotFound { .. })
    ));
    let kept = upgraded.get_pull_request("tasks").await.unwrap();
    assert_eq!(kept.origin_task_id.as_deref(), Some("t"));
    assert_eq!(
        upgraded
            .pull_request_of_task("t")
            .await
            .unwrap()
            .map(|p| p.id),
        Some("tasks".to_string())
    );
    assert_eq!(
        upgraded.get_pull_request("review").await.unwrap().role,
        "reviewer"
    );
    assert!(matches!(
        upgraded.get_session("babysit").await,
        Err(StoreError::NotFound { .. })
    ));
    assert_eq!(
        upgraded
            .get_session("reviewing")
            .await
            .unwrap()
            .pull_request_id
            .as_deref(),
        Some("review")
    );
    let forge = upgraded.forge_integration("r").await.unwrap().unwrap();
    assert!(forge.enabled);
    assert_eq!(forge.review_model.as_deref(), Some("stub:r"));
    upgraded.close().await;
}

/// The pull request session migration adds beside what is there: an old
/// session keeps every value and reads no request, an old request reads no
/// failed check and a head level with its base, and the copy taken before
/// the upgrade opens with every row.
#[tokio::test]
async fn pull_request_session_migration_preserves_sessions_and_requests() {
    let dir = tempfile::tempdir().unwrap();
    let old_migrations = dir.path().join("migrations");
    std::fs::create_dir(&old_migrations).unwrap();
    for entry in std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/migrations")).unwrap() {
        let entry = entry.unwrap();
        if entry.file_name().to_string_lossy().as_ref() < "0010" {
            std::fs::copy(entry.path(), old_migrations.join(entry.file_name())).unwrap();
        }
    }
    let path = dir.path().join("old.db");
    let db = sqlx::sqlite::SqlitePoolOptions::new()
        .connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(&path)
                .create_if_missing(true),
        )
        .await
        .unwrap();
    sqlx::migrate::Migrator::new(old_migrations.as_path())
        .await
        .unwrap()
        .run(&db)
        .await
        .unwrap();
    for statement in [
        "INSERT INTO repositories (id,path,base_branch,created_at,updated_at) VALUES ('r','/work/widgets','main','2026-10-01','2026-10-01')",
        "INSERT INTO agent_sessions (id,model,status,created_at,worktree_path) VALUES ('s','stub:m','exited','2026-10-01','/work/wt')",
        "INSERT INTO pull_requests (id,repository_id,number,url,title,author_login,tracked_by,state,draft,head_branch,head_sha,base_branch,checks,review_decision,unanswered_comments,opened_at,role,ready,last_seen_at,created_at,updated_at) VALUES ('p','r',1,'https://github.com/acme/widgets/pull/1','Fix','someone','forge','open',0,'fix','abc','main','none','none',2,'2026-10-01','reviewer',0,'2026-10-01','2026-10-01','2026-10-01')",
        "INSERT INTO pull_requests (id,repository_id,number,url,title,author_login,tracked_by,state,draft,head_branch,head_sha,base_branch,checks,review_decision,unanswered_comments,opened_at,role,ready,last_seen_at,created_at,updated_at) VALUES ('q','r',2,'https://github.com/acme/widgets/pull/2','Old','someone','forge','merged',0,'old','abd','main','none','none',0,'2026-09-01','reviewer',0,'2026-09-01','2026-09-01','2026-09-02')",
    ] {
        sqlx::query(statement).execute(&db).await.unwrap();
    }
    let backup = dir.path().join("backup.db");
    sqlx::query("VACUUM INTO ?")
        .bind(backup.to_str().unwrap())
        .execute(&db)
        .await
        .unwrap();
    db.close().await;

    let upgraded = Store::open(&path).await.unwrap();
    let session = upgraded.get_session("s").await.unwrap();
    assert_eq!(session.worktree_path.as_deref(), Some("/work/wt"));
    assert_eq!(session.pull_request_id, None);
    // Nothing works on either request — no integration reads them, and one
    // ended long ago — so neither keeps a row (0020).
    assert!(
        upgraded
            .list_pull_requests(PullRequestFilter::default())
            .await
            .unwrap()
            .is_empty()
    );
    upgraded.close().await;

    let recovered = Store::open(&backup).await.unwrap();
    assert_eq!(
        recovered
            .get_session("s")
            .await
            .unwrap()
            .worktree_path
            .as_deref(),
        Some("/work/wt")
    );
}

/// The migration that takes the forge's content out of the database (0020)
/// keeps the row of each request Ariadne works on, with its marks: the
/// request a task opened, an open one that asks for the user's review on a
/// repository with a review pin, and one of the user's they asked Ariadne
/// to review. Every other row goes, and so does an ended one whose work was
/// taken down. A comment keeps its told and review marks alone, and a
/// session that ran on a row that goes stays, let go of it.
#[tokio::test]
async fn the_migration_that_drops_forge_content_keeps_the_rows_of_work_and_their_marks() {
    let dir = tempfile::tempdir().unwrap();
    let old_migrations = dir.path().join("migrations");
    std::fs::create_dir(&old_migrations).unwrap();
    for entry in std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/migrations")).unwrap() {
        let entry = entry.unwrap();
        if entry.file_name().to_string_lossy().as_ref() < "0020" {
            std::fs::copy(entry.path(), old_migrations.join(entry.file_name())).unwrap();
        }
    }
    let path = dir.path().join("old.db");
    let db = sqlx::sqlite::SqlitePoolOptions::new()
        .connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(&path)
                .create_if_missing(true),
        )
        .await
        .unwrap();
    sqlx::migrate::Migrator::new(old_migrations.as_path())
        .await
        .unwrap()
        .run(&db)
        .await
        .unwrap();
    let pull = |id: &str,
                number: i64,
                author: &str,
                role: &str,
                state: &str,
                task: &str,
                asked: i64,
                requested: i64,
                cleaned: &str| {
        format!(
            "INSERT INTO pull_requests (id,repository_id,number,url,title,author_login,tracked_by,state,draft,head_branch,head_sha,base_branch,checks,review_decision,unanswered_comments,origin_task_id,opened_at,role,ready,last_seen_at,created_at,updated_at,review_asked,review_requested,cleaned_at) VALUES ('{id}','r',{number},'https://github.com/acme/widgets/pull/{number}','Fix','{author}','forge','{state}',0,'fix-{number}','abc','main','none','none',0,{task},'2026-10-01','{role}',0,'2026-10-01','2026-10-01','2026-10-01',{asked},{requested},{cleaned})"
        )
    };
    for statement in [
        "INSERT INTO repositories (id,path,base_branch,created_at,updated_at) VALUES ('r','/work/widgets','main','2026-10-01','2026-10-01')".to_string(),
        "INSERT INTO forge_integrations (repository_id,kind,host,owner,name,remote,enabled,login,review_model,detected_at,updated_at) VALUES ('r','github','github.com','acme','widgets','origin',1,'me','stub:r','2026-10-01','2026-10-01')".to_string(),
        "INSERT INTO goals (id,title,description,created_at,updated_at,model) VALUES ('g','Ship','Body','2026-10-01','2026-10-01','stub:m')".to_string(),
        "INSERT INTO tasks (id,goal_id,repo_id,title,description,branch,created_at,updated_at) VALUES ('t','g','r','Fix','Body','ariadne/fix','2026-10-01','2026-10-01')".to_string(),
        pull("tasks", 1, "me", "author", "open", "'t'", 0, 0, "NULL"),
        pull("review", 2, "someone", "reviewer", "open", "NULL", 0, 1, "NULL"),
        pull("asked", 3, "me", "author", "open", "NULL", 1, 0, "NULL"),
        pull("listed", 4, "someone", "reviewer", "open", "NULL", 0, 0, "NULL"),
        pull("mine", 5, "me", "author", "open", "NULL", 0, 0, "NULL"),
        pull("ended", 6, "someone", "reviewer", "merged", "NULL", 0, 1, "'2026-10-02'"),
        pull("owed", 7, "me", "author", "merged", "'t'", 0, 0, "NULL"),
        "INSERT INTO agent_sessions (id,model,status,created_at,seat,pull_request_id) VALUES ('reviewing','stub:r','idle','2026-10-01','reviewer','review')".to_string(),
        "INSERT INTO agent_sessions (id,model,status,created_at,seat,pull_request_id) VALUES ('reviewed','stub:r','exited','2026-10-01','reviewer','ended')".to_string(),
        "INSERT INTO pull_request_comments (id,pull_request_id,forge_id,thread_id,kind,author_login,author_is_bot,body,created_at,fetched_at,told_at) VALUES ('c1','tasks','rc-1','T1','review_comment','alice',0,'Rename it.','2026-10-02','2026-10-02','2026-10-02')".to_string(),
        "INSERT INTO pull_request_comments (id,pull_request_id,forge_id,thread_id,kind,author_login,author_is_bot,body,created_at,fetched_at,from_review) VALUES ('c2','asked','rc-2','T2','review_comment','me',0,'[P1] Untested','2026-10-02','2026-10-02',1)".to_string(),
        "INSERT INTO pull_request_comments (id,pull_request_id,forge_id,thread_id,kind,author_login,author_is_bot,body,created_at,fetched_at) VALUES ('c3','tasks','rc-3','T3','review_comment','bob',0,'Untold.','2026-10-02','2026-10-02')".to_string(),
    ] {
        sqlx::query(sqlx::AssertSqlSafe(statement)).execute(&db).await.unwrap();
    }
    db.close().await;

    let upgraded = Store::open(&path).await.unwrap();
    let mut kept: Vec<String> = upgraded
        .list_pull_requests(PullRequestFilter::default())
        .await
        .unwrap()
        .into_iter()
        .map(|row| row.id)
        .collect();
    kept.sort();
    assert_eq!(kept, ["asked", "owed", "review", "tasks"]);
    assert_eq!(
        upgraded
            .get_pull_request("tasks")
            .await
            .unwrap()
            .origin_task_id
            .as_deref(),
        Some("t")
    );
    let told = upgraded.pull_request_comment_marks("tasks").await.unwrap();
    assert_eq!(told.len(), 1, "an untold comment leaves no mark");
    assert_eq!(told[0].forge_id, "rc-1");
    assert!(told[0].told_at.is_some());
    let review = upgraded.pull_request_comment_marks("asked").await.unwrap();
    assert!(review[0].from_review);
    assert_eq!(
        upgraded
            .get_session("reviewing")
            .await
            .unwrap()
            .pull_request_id
            .as_deref(),
        Some("review")
    );
    let let_go = upgraded.get_session("reviewed").await.unwrap();
    assert_eq!(let_go.pull_request_id, None, "a session outlives its row");
    let db = sqlx::SqlitePool::connect(&format!("sqlite://{}", path.display()))
        .await
        .unwrap();
    let columns: Vec<String> =
        sqlx::query_scalar("SELECT name FROM pragma_table_info('pull_requests')")
            .fetch_all(&db)
            .await
            .unwrap();
    for gone in [
        "title",
        "body",
        "author_login",
        "head_sha",
        "checks",
        "state",
    ] {
        assert!(!columns.iter().any(|c| c == gone), "{gone} is the forge's");
    }
    db.close().await;
    upgraded.close().await;
}

#[tokio::test]
async fn webhook_migration_preserves_existing_integrations_and_a_recoverable_backup() {
    let dir = tempfile::tempdir().unwrap();
    let old_migrations = dir.path().join("migrations");
    std::fs::create_dir(&old_migrations).unwrap();
    for entry in std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/migrations")).unwrap() {
        let entry = entry.unwrap();
        if entry.file_name().to_string_lossy().as_ref() < "0008" {
            std::fs::copy(entry.path(), old_migrations.join(entry.file_name())).unwrap();
        }
    }
    let path = dir.path().join("old.db");
    let db = sqlx::sqlite::SqlitePoolOptions::new()
        .connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(&path)
                .create_if_missing(true),
        )
        .await
        .unwrap();
    sqlx::migrate::Migrator::new(old_migrations.as_path())
        .await
        .unwrap()
        .run(&db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO repositories (id,path,base_branch,created_at,updated_at) VALUES ('old-repo','/work/widgets','main','2026-10-01','2026-10-01')").execute(&db).await.unwrap();
    sqlx::query("INSERT INTO forge_integrations (repository_id,kind,host,owner,name,remote,enabled,login,detected_at,updated_at) VALUES ('old-repo','github','github.com','acme','widgets','origin',1,'me','2026-10-01','2026-10-01')").execute(&db).await.unwrap();
    let backup = dir.path().join("backup.db");
    sqlx::query("VACUUM INTO ?")
        .bind(backup.to_str().unwrap())
        .execute(&db)
        .await
        .unwrap();
    db.close().await;
    let upgraded = Store::open(&path).await.unwrap();
    let row = upgraded.get_repository("old-repo").await.unwrap();
    let forge = row.forge.unwrap();
    assert_eq!(forge.login.as_deref(), Some("me"));
    assert_eq!(forge.webhook_state, "polling");
    assert!(forge.webhook_id.is_none());
    assert!(forge.webhook_secret.is_none());
    assert!(forge.webhook_last_delivery_at.is_none());
    assert_eq!(row.path, "/work/widgets");
    assert_eq!(row.base_branch, "main");
    assert!(
        upgraded
            .list_pull_requests(PullRequestFilter::default())
            .await
            .unwrap()
            .is_empty()
    );
    upgraded.close().await;
    let recovered = Store::open(&backup).await.unwrap();
    assert_eq!(
        recovered
            .get_repository("old-repo")
            .await
            .unwrap()
            .created_at,
        "2026-10-01"
    );
    assert_eq!(recovered.list_repositories().await.unwrap().len(), 1);
}

#[tokio::test]
async fn forge_settings_migration_turns_the_tunnel_on_and_keeps_the_first_subdomain() {
    let dir = tempfile::tempdir().unwrap();
    let old_migrations = dir.path().join("migrations");
    std::fs::create_dir(&old_migrations).unwrap();
    for entry in std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/migrations")).unwrap() {
        let entry = entry.unwrap();
        if entry.file_name().to_string_lossy().as_ref() < "0009" {
            std::fs::copy(entry.path(), old_migrations.join(entry.file_name())).unwrap();
        }
    }
    let path = dir.path().join("old.db");
    let db = sqlx::sqlite::SqlitePoolOptions::new()
        .connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(&path)
                .create_if_missing(true),
        )
        .await
        .unwrap();
    sqlx::migrate::Migrator::new(old_migrations.as_path())
        .await
        .unwrap()
        .run(&db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO repositories (id,path,base_branch,created_at,updated_at) VALUES ('old-repo','/work/widgets','main','2026-10-01','2026-10-01')").execute(&db).await.unwrap();
    sqlx::query("INSERT INTO forge_integrations (repository_id,kind,host,owner,name,remote,enabled,login,detected_at,updated_at,webhook_state) VALUES ('old-repo','github','github.com','acme','widgets','origin',1,'me','2026-10-01','2026-10-01','live')").execute(&db).await.unwrap();
    db.close().await;

    let upgraded = Store::open(&path).await.unwrap();
    let settings = upgraded.forge_settings().await.unwrap();
    assert!(settings.tunnel_enabled);
    assert_eq!(settings.tunnel_subdomain, None);
    let forge = upgraded
        .get_repository("old-repo")
        .await
        .unwrap()
        .forge
        .unwrap();
    assert_eq!(forge.webhook_state, "live");

    assert!(
        !upgraded
            .set_tunnel_enabled(false)
            .await
            .unwrap()
            .tunnel_enabled
    );
    let kept = upgraded
        .keep_tunnel_subdomain("amber-104233")
        .await
        .unwrap();
    assert_eq!(kept.tunnel_subdomain.as_deref(), Some("amber-104233"));
    let again = upgraded
        .keep_tunnel_subdomain("other-000001")
        .await
        .unwrap();
    assert_eq!(again.tunnel_subdomain.as_deref(), Some("amber-104233"));
    upgraded.close().await;

    let reopened = Store::open(&path).await.unwrap();
    let settings = reopened.forge_settings().await.unwrap();
    assert!(!settings.tunnel_enabled);
    assert_eq!(settings.tunnel_subdomain.as_deref(), Some("amber-104233"));
}

/// Every shipped workflow is seeded into a fresh database on the text
/// Ariadne ships, storing none of it, the same way a skill is.
#[tokio::test]
async fn a_fresh_database_is_seeded_with_every_shipped_workflow_on_its_own_text() {
    let (store, _dir) = test_store().await;

    let workflows = store.list_workflows().await.unwrap();
    assert_eq!(
        workflows.len(),
        ariadne_store::defaults::BUILTIN_WORKFLOWS.len()
    );
    assert!(
        workflows
            .iter()
            .all(|w| w.is_builtin() && w.document_is_default()),
        "every seeded workflow runs on the shipped text"
    );

    let merge = store.get_workflow("develop-review-merge").await.unwrap();
    assert_eq!(merge.steps().len(), 3);
    assert_eq!(merge.steps()[0].id, "develop");

    let pr = store.get_workflow("develop-review-pr").await.unwrap();
    assert_eq!(pr.steps().len(), 3);
    assert_eq!(pr.steps()[2].id, "pr");
}

/// Seeding a workflow is by name and overwrites no document the database
/// holds: an edit survives a reopen, and nothing is seeded back over it.
#[tokio::test]
async fn a_reopen_reseeds_no_workflow_row_the_database_already_holds() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");

    let edited =
        "workflow develop-review-merge\n  solo[Solo]\n    Do it all.\n    skills: coding\n";
    let store = Store::open(&path).await.unwrap();
    store
        .set_workflow_document("develop-review-merge", edited)
        .await
        .unwrap();
    store
        .create_workflow(NewWorkflow {
            name: "api-design".into(),
            document: "workflow api-design\n  build[Build]\n    Do the work.\n    skills: coding\n"
                .into(),
        })
        .await
        .unwrap();
    store.delete_workflow("api-design").await.unwrap();
    drop(store);

    let store = Store::open(&path).await.unwrap();
    let reopened = store.get_workflow("develop-review-merge").await.unwrap();
    assert!(
        !reopened.document_is_default(),
        "the edit survived the reopen"
    );
    assert_eq!(reopened.steps()[0].id, "solo");
    assert!(
        matches!(
            store.get_workflow("api-design").await,
            Err(StoreError::NotFound { .. })
        ),
        "a deleted workflow of the user's own stays deleted"
    );
}

/// A workflow is created, read, written over and deleted by name; a built-in
/// is reset rather than deleted, and a workflow of the user's own is deleted
/// rather than reset — there is nothing behind it to go back to.
#[tokio::test]
async fn workflow_crud_and_the_two_refusals_that_tell_them_apart() {
    let (store, _dir) = test_store().await;

    let shipped = store.get_workflow("develop-review-merge").await.unwrap();
    assert!(shipped.is_builtin());
    assert!(shipped.document_is_default());

    let mine = store
        .create_workflow(NewWorkflow {
            name: "api-design".into(),
            document: "workflow api-design\n  build[Build]\n    Do the work.\n    skills: coding\n"
                .into(),
        })
        .await
        .unwrap();
    assert!(!mine.is_builtin());

    assert!(matches!(
        store
            .create_workflow(NewWorkflow {
                name: "develop-review-merge".into(),
                document: "workflow develop-review-merge\n  a[A]\n    Do it.\n    skills: coding\n"
                    .into(),
            })
            .await,
        Err(StoreError::Conflict(_))
    ));

    assert!(matches!(
        store.delete_workflow("develop-review-merge").await,
        Err(StoreError::Conflict(_))
    ));
    assert!(matches!(
        store.reset_workflow("api-design").await,
        Err(StoreError::Conflict(_))
    ));
    store.delete_workflow("api-design").await.unwrap();
    assert!(matches!(
        store.get_workflow("api-design").await,
        Err(StoreError::NotFound { .. })
    ));

    let edited = store
        .set_workflow_document(
            "develop-review-merge",
            "workflow develop-review-merge\n  solo[Solo]\n    Do it all.\n    skills: coding\n",
        )
        .await
        .unwrap();
    assert!(!edited.document_is_default());
    let reset = store.reset_workflow("develop-review-merge").await.unwrap();
    assert!(reset.document_is_default());
    assert_eq!(reset.steps()[0].id, "develop");
}

/// A workflow column can only name a skill that exists: the name is a
/// reference, and the refusal says which name it was.
#[tokio::test]
async fn a_workflow_save_refuses_an_unknown_skill_naming_it() {
    let (store, _dir) = test_store().await;
    let refused = store
        .create_workflow(NewWorkflow {
            name: "api-design".into(),
            document:
                "workflow api-design\n  build[Build]\n    Do the work.\n    skills: telepathy\n"
                    .into(),
        })
        .await;
    let message = format!("{:?}", refused.expect_err("no such skill"));
    assert!(message.contains("telepathy"), "{message}");
}

/// A workflow column cannot staff the orchestrator's skill or the one a
/// reviewer pull request session loads; `pr-babysit` is allowed, since the
/// shipped `develop-review-pr` workflow stages it on its last column.
#[tokio::test]
async fn a_workflow_save_refuses_the_orchestrators_skill_and_the_reviewer_sessions_skill() {
    let (store, _dir) = test_store().await;

    for forbidden in ["orchestration", "pr-reviewer"] {
        let refused = store
            .create_workflow(NewWorkflow {
                name: "api-design".into(),
                document: format!(
                    "workflow api-design\n  build[Build]\n    Do the work.\n    skills: {forbidden}\n"
                ),
            })
            .await;
        let message = format!("{:?}", refused.expect_err("forbidden skill"));
        assert!(message.contains(forbidden), "{message}");
    }

    store
        .create_workflow(NewWorkflow {
            name: "api-design".into(),
            document:
                "workflow api-design\n  pr[Pull request]\n    Keep it.\n    skills: pr-babysit\n"
                    .into(),
        })
        .await
        .unwrap();
}

/// A goal that names the shipped `develop-review-merge` workflow outright,
/// on a repository of its own.
async fn stepped_goal(store: &Store) -> (Goal, Repository) {
    let repo = seed_repository(store).await;
    let goal = store
        .create_goal(NewGoal {
            workflow: Some(DEFAULT_WORKFLOW.into()),
            title: "Run columns".into(),
            description: "Build a change.".into(),
            issue_url: None,
            repository_ids: vec![repo.id.clone()],
            pin: default_pin(),
        })
        .await
        .unwrap();
    (goal, repo)
}

/// One agent per column of `develop-review-merge`, each on its column's own
/// skills.
fn step_agents() -> Vec<NewTaskAgent> {
    ["develop", "review", "merge"]
        .into_iter()
        .map(|step| NewTaskAgent::new(step, Vec::<String>::new(), default_pin()))
        .collect()
}

/// A staffing is checked against the goal's columns: an agent on a column
/// the workflow has not got is refused, naming the column and listing the
/// ones there are, and so is a column staffed twice — and a refused staffing
/// writes no task. An agent staffed with no skills of its own takes its
/// column's. While the goal is planned a column may be left unstaffed: the
/// orchestrator staffs a plan task by task, so `create_task` accepts a
/// partial staffing and `unstaffed_columns` names what is missing, in column
/// order, for the plan and the retry that refuse to start on it. Once the
/// goal runs, a task is runnable as soon as it is written or edited, so a
/// create or an edit that leaves a column unstaffed is refused by name.
#[tokio::test]
async fn a_staffing_names_the_goals_columns_once_each_and_may_leave_some_for_later() {
    let (store, _dir) = test_store().await;
    let (goal, repo) = stepped_goal(&store).await;
    let new = |agents| NewTask {
        goal_id: goal.id.clone(),
        repo_id: repo.id.clone(),
        title: "Build".into(),
        description: "Build it.".into(),
        agents,
        depends_on: vec![],
    };

    let mut unknown = step_agents();
    unknown[1].step = "publish".into();
    let refused = format!("{:?}", store.create_task(new(unknown)).await.unwrap_err());
    assert!(refused.contains("unknown column publish"), "{refused}");
    assert!(refused.contains("develop, review, merge"), "{refused}");
    let mut duplicate = step_agents();
    duplicate[1].step = "develop".into();
    let refused = format!("{:?}", store.create_task(new(duplicate)).await.unwrap_err());
    assert!(refused.contains("develop is staffed twice"), "{refused}");
    assert!(
        store
            .list_tasks(TaskFilter {
                goal_id: Some(goal.id.clone()),
                ..Default::default()
            })
            .await
            .unwrap()
            .is_empty(),
        "a refused staffing writes no task"
    );

    // Empty skills inherit the column's; named skills are the agent's own.
    let mut named = step_agents();
    named[0].skills = vec!["coding".into(), "debugging".into()];
    let task = store.create_task(new(named)).await.unwrap();
    let skills_of = async |step: &str| -> Vec<String> {
        let agent = agent_of(&store, &task, step).await;
        store
            .agent_skills(&agent.id)
            .await
            .unwrap()
            .into_iter()
            .map(|s| s.name)
            .collect()
    };
    assert_eq!(skills_of("develop").await, ["coding", "debugging"]);
    assert_eq!(skills_of("review").await, ["code-review"]);
    assert_eq!(skills_of("merge").await, ["merge"]);
    assert!(store.unstaffed_columns(&task).await.unwrap().is_empty());

    // A partial staffing is accepted, and what it lacks is named in column
    // order, whatever order the agents were given in.
    let partial = store
        .create_task(new(vec![NewTaskAgent::new(
            "review",
            Vec::<String>::new(),
            default_pin(),
        )]))
        .await
        .unwrap();
    assert_eq!(
        store.unstaffed_columns(&partial).await.unwrap(),
        ["develop", "merge"]
    );
    let mut reversed = step_agents();
    reversed.reverse();
    reversed.remove(1);
    let gapped = store.create_task(new(reversed)).await.unwrap();
    assert_eq!(store.unstaffed_columns(&gapped).await.unwrap(), ["review"]);
    let empty = store.create_task(new(vec![])).await.unwrap();
    assert_eq!(
        store.unstaffed_columns(&empty).await.unwrap(),
        ["develop", "review", "merge"]
    );

    // On an active goal nothing may be left for later: a task written then
    // starts as soon as its dependencies allow.
    store
        .set_goal_status(&goal.id, GoalStatus::Active)
        .await
        .unwrap();
    let refused = format!(
        "{:?}",
        store
            .create_task(new(vec![NewTaskAgent::new(
                "review",
                Vec::<String>::new(),
                default_pin(),
            )]))
            .await
            .unwrap_err()
    );
    assert!(refused.contains("none staffs develop, merge"), "{refused}");
    assert!(refused.contains("active"), "{refused}");
    let refused = format!(
        "{:?}",
        store
            .update_task(
                &task.id,
                TaskUpdate {
                    agents: Some(vec![NewTaskAgent::new(
                        "develop",
                        Vec::<String>::new(),
                        default_pin(),
                    )]),
                    ..Default::default()
                },
            )
            .await
            .unwrap_err()
    );
    assert!(refused.contains("none staffs review, merge"), "{refused}");
    assert!(store.unstaffed_columns(&task).await.unwrap().is_empty());
    store
        .update_task(
            &task.id,
            TaskUpdate {
                agents: Some(step_agents()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(store.unstaffed_columns(&task).await.unwrap().is_empty());
}

#[tokio::test]
async fn workflow_snapshots_keep_their_columns_and_references_prevent_deletion() {
    let (store, _dir) = test_store().await;
    store
        .create_workflow(NewWorkflow {
            name: "custom".into(),
            document: "workflow custom\n build[Build]\n skills: pr-babysit\n".into(),
        })
        .await
        .unwrap();
    let repo = seed_repository(&store).await;
    store
        .update_repository(
            &repo.id,
            RepositoryUpdate {
                default_workflow: Some("custom".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        store.delete_workflow("custom").await,
        Err(StoreError::WorkflowInUse(_))
    ));
    let make = || NewGoal {
        workflow: None,
        title: "Keep columns".into(),
        description: "Use the default.".into(),
        issue_url: None,
        repository_ids: vec![repo.id.clone()],
        pin: default_pin(),
    };
    let goal = store.create_goal(make()).await.unwrap();
    assert_eq!(goal.workflow, "custom");
    store
        .set_workflow_document("custom", "workflow custom\n other[Other]\n")
        .await
        .unwrap();
    assert_eq!(store.goal_steps(&goal.id).await.unwrap()[0].id, "build");
    let later = store.create_goal(make()).await.unwrap();
    assert_eq!(store.goal_steps(&later.id).await.unwrap()[0].id, "other");
    store
        .update_repository(
            &repo.id,
            RepositoryUpdate {
                default_workflow: Some(DEFAULT_WORKFLOW.into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        store.delete_workflow("custom").await,
        Err(StoreError::WorkflowInUse(_))
    ));
    let agent = NewTaskAgent::new("build", Vec::<String>::new(), default_pin());
    let task = store
        .create_task(NewTask {
            goal_id: goal.id,
            repo_id: repo.id,
            title: "Keep request".into(),
            description: "Wait for merge.".into(),
            agents: vec![agent],
            depends_on: vec![],
        })
        .await
        .unwrap();
    let agent = &store.list_task_agents(&task.id).await.unwrap()[0];
    assert_eq!(
        store.agent_skills(&agent.id).await.unwrap()[0].name,
        "pr-babysit"
    );
}

/// A database written on the old schema, as the release before workflows
/// left it: one goal per landing, a task in every status, a task of a
/// `pull_request` goal with an author alone, a review with two reviewers and
/// their verdicts, a request review session, and a goal branch. Written at
/// the schema before the catalog of workflows existed, so the open runs the
/// three migrations that lead to the stepped schema in one go.
async fn old_pipeline_database(dir: &tempfile::TempDir) -> std::path::PathBuf {
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    let path = dir.path().join("old.db");
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(&path)
                .create_if_missing(true)
                .foreign_keys(true),
        )
        .await
        .unwrap();
    let migrator = sqlx::migrate::Migrator::new(std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/migrations"
    )))
    .await
    .unwrap();
    migrator.run_to(20, &pool).await.unwrap();
    sqlx::raw_sql(
        "INSERT INTO skills (name,builtin,created_at,updated_at) VALUES
            ('coding',1,'now','now'),('code-review',1,'now','now');
         INSERT INTO repositories (id,path,base_branch,created_at,updated_at,default_landing) VALUES
            ('r_merge','/tmp/merge','main','now','now','merge'),
            ('r_pr','/tmp/pr','main','now','now','pull_request'),
            ('r_fb','/tmp/fb','main','now','now','feature_branch'),
            ('r_none','/tmp/none','main','now','now','none');
         INSERT INTO goals (id,title,description,status,model,landing,created_at,updated_at) VALUES
            ('g_merge','Merge','Text','active','stub:test','merge','now','now'),
            ('g_pr','Request','Text','active','stub:test','pull_request','now','now'),
            ('g_fb','Branch','Text','active','stub:test','feature_branch','now','now'),
            ('g_none','Nothing','Text','active','stub:test','none','now','now');
         INSERT INTO goal_repositories (goal_id,repository_id,goal_branch) VALUES
            ('g_merge','r_merge',NULL),('g_pr','r_pr',NULL),('g_fb','r_fb','branch-fb'),('g_none','r_none',NULL);
         INSERT INTO tasks (id,goal_id,repo_id,title,description,status,branch,created_at,updated_at) VALUES
            ('t_pending','g_merge','r_merge','Pending','Text','pending','t-pending','now','now'),
            ('t_ready','g_merge','r_merge','Ready','Text','ready','t-ready','now','now'),
            ('t_progress','g_merge','r_merge','Progress','Text','in_progress','t-progress','now','now'),
            ('t_review','g_merge','r_merge','Review','Text','under_review','t-review','now','now'),
            ('t_changes','g_merge','r_merge','Changes','Text','changes_requested','t-changes','now','now'),
            ('t_approved','g_merge','r_merge','Approved','Text','approved','t-approved','now','now'),
            ('t_finished','g_merge','r_merge','Finished','Text','finished','t-finished','now','now'),
            ('t_cancelled','g_merge','r_merge','Cancelled','Text','cancelled','t-cancelled','now','now'),
            ('t_failed','g_merge','r_merge','Failed','Text','failed','t-failed','now','now'),
            ('t_pr','g_pr','r_pr','Publish','Text','in_progress','t-pr','now','now');
         INSERT INTO task_agents (id,task_id,seat,ordinal,model,brief) VALUES
            ('a_review','t_review','author',0,'stub:author','Keep this brief.'),
            ('r_one','t_review','reviewer',0,'stub:first',NULL),
            ('r_two','t_review','reviewer',1,'stub:second',NULL),
            ('a_pr','t_pr','author',0,'stub:author',NULL),
            ('a_finished','t_finished','author',0,'stub:author',NULL);
         INSERT INTO task_agent_skills VALUES ('a_review','coding',0),('r_one','code-review',0),('a_pr','coding',0);
         UPDATE tasks SET picked_agent_id = 'a_review' WHERE id = 't_review';
         INSERT INTO task_picks VALUES ('t_review','r_one','a_review','now');
         INSERT INTO pull_requests (id,repository_id,number,url,role,ready,created_at,updated_at) VALUES
            ('pr_theirs','r_pr',7,'https://github.com/acme/widgets/pull/7','reviewer',0,'now','now');
         INSERT INTO agent_sessions (id,goal_id,task_id,seat,task_agent_id,model,created_at,pull_request_id) VALUES
            ('s_author','g_merge','t_review','author','a_review','stub:author','now',NULL),
            ('s_reviewer','g_merge','t_review','reviewer','r_one','stub:first','now',NULL),
            ('s_orchestrator','g_merge',NULL,'orchestrator',NULL,'stub:test','now',NULL),
            ('s_request',NULL,NULL,'reviewer',NULL,'stub:test','now','pr_theirs');
         INSERT INTO messages (id,goal_id,task_id,kind,from_actor,from_agent_id,from_session,to_actor,to_agent_id,body,created_at) VALUES
            ('m_request','g_merge','t_review','review_request','author','a_review','s_author','reviewer','r_one','Please review.','now'),
            ('m_approve','g_merge','t_review','approve','reviewer','r_one','s_reviewer','author','a_review','Looks right.','now'),
            ('m_changes','g_merge','t_review','request_changes','reviewer','r_two',NULL,'author','a_review','Add a test.','now'),
            ('m_message','g_merge','t_review','message','orchestrator',NULL,NULL,'author','a_review','How far along?','now'),
            ('m_user','g_merge',NULL,'message','user',NULL,NULL,'orchestrator',NULL,'Carry on.','now');
         INSERT INTO task_transitions (id,task_id,from_status,to_status,actor,reason,created_at) VALUES
            ('01AAAAAAAA0000000000000001','t_review','ready','in_progress','daemon','Started.','now'),
            ('01AAAAAAAA0000000000000002','t_review','in_progress','under_review','author','Done.','now'),
            ('01AAAAAAAA0000000000000003','t_changes','under_review','changes_requested','reviewer',NULL,'now');",
    )
    .execute(&pool)
    .await
    .unwrap();
    pool.close().await;
    path
}

/// A second connection to the database file a store holds open, for the
/// reads of the schema itself that no store method makes.
async fn raw_pool(path: &std::path::Path) -> sqlx::SqlitePool {
    sqlx::SqlitePool::connect(&format!("sqlite://{}", path.display()))
        .await
        .unwrap()
}

/// A `PRAGMA table_info` read as the column names of `table`.
async fn columns_of(pool: &sqlx::SqlitePool, table: &str) -> Vec<String> {
    let rows: Vec<(i64, String, String, i64, Option<String>, i64)> =
        sqlx::query_as(sqlx::AssertSqlSafe(format!("PRAGMA table_info({table})")))
            .fetch_all(pool)
            .await
            .unwrap();
    rows.into_iter().map(|(_, name, ..)| name).collect()
}

#[tokio::test]
async fn the_workflows_only_migration_maps_every_old_row_as_agreed() {
    let dir = tempfile::tempdir().unwrap();
    let path = old_pipeline_database(&dir).await;
    let backup = dir.path().join("backup.db");
    std::fs::copy(&path, &backup).unwrap();
    let store = Store::open(&path).await.unwrap();

    // A goal's landing becomes a workflow, and a goal with no columns gets
    // the columns of the shipped document.
    for (goal, workflow, last) in [
        ("g_merge", "develop-review-merge", "merge"),
        ("g_fb", "develop-review-merge", "merge"),
        ("g_none", "develop-review-merge", "merge"),
        ("g_pr", "develop-review-pr", "pr"),
    ] {
        assert_eq!(
            store.get_goal(goal).await.unwrap().workflow,
            workflow,
            "{goal}"
        );
        let steps = store.goal_steps(goal).await.unwrap();
        assert_eq!(
            steps.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            ["develop", "review", last],
            "{goal}"
        );
        assert_eq!(steps[0].skills, r#"["coding"]"#);
        assert_eq!(steps[0].gate.as_deref(), Some("committed"));
        assert_eq!(steps[1].skills, r#"["code-review"]"#);
    }
    let pr = store.goal_steps("g_pr").await.unwrap().pop().unwrap();
    assert_eq!(pr.skills, r#"["pr-babysit"]"#);
    assert_eq!(pr.gate.as_deref(), Some("request_merged"));
    let merge = store.goal_steps("g_merge").await.unwrap().pop().unwrap();
    assert_eq!(merge.skills, r#"["merge"]"#);
    assert_eq!(merge.gate.as_deref(), Some("merged"));

    // A repository's default landing becomes its default workflow.
    for (repo, workflow) in [
        ("r_merge", "develop-review-merge"),
        ("r_fb", "develop-review-merge"),
        ("r_none", "develop-review-merge"),
        ("r_pr", "develop-review-pr"),
    ] {
        assert_eq!(
            store.get_repository(repo).await.unwrap().default_workflow,
            workflow,
            "{repo}"
        );
    }

    // A task that stood in a review status, or had an author on it, is
    // failed with one transition by the daemon; the rest keep their status.
    for (task, from) in [
        ("t_ready", "ready"),
        ("t_progress", "in_progress"),
        ("t_review", "under_review"),
        ("t_changes", "changes_requested"),
        ("t_approved", "approved"),
        ("t_pr", "in_progress"),
    ] {
        assert_eq!(
            store.get_task(task).await.unwrap().status(),
            TaskStatus::Failed,
            "{task}"
        );
        let last = store
            .list_task_transitions(task)
            .await
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(
            (last.from_status.as_str(), last.to_status.as_str()),
            (from, "failed"),
            "{task}"
        );
        assert_eq!(last.actor, "daemon");
        assert_eq!(
            last.reason.as_deref(),
            Some("replaced by workflows, retry it")
        );
        assert_eq!(last.id.len(), 26, "the daemon's transition carries a ULID");
    }
    for (task, status) in [
        ("t_pending", TaskStatus::Pending),
        ("t_finished", TaskStatus::Finished),
        ("t_cancelled", TaskStatus::Cancelled),
        ("t_failed", TaskStatus::Failed),
    ] {
        assert_eq!(
            store.get_task(task).await.unwrap().status(),
            status,
            "{task}"
        );
        assert!(
            store.list_task_transitions(task).await.unwrap().is_empty(),
            "{task}"
        );
    }
    // The daemon's transition sorts after the ones before it and before one
    // written now: it is read as the task's latest, and a retry's is read
    // after it.
    let transitions = store.list_task_transitions("t_review").await.unwrap();
    assert_eq!(
        transitions
            .iter()
            .map(|t| t.to_status.as_str())
            .collect::<Vec<_>>(),
        ["in_progress", "under_review", "failed"]
    );
    assert!(transitions[2].id > transitions[1].id);
    assert!(transitions[2].id < ariadne_core::id::new_id());
    assert!(transitions.iter().all(|t| t.actor != "author"));
    assert_eq!(transitions[1].actor, "agent");
    assert_eq!(
        store.list_task_transitions("t_changes").await.unwrap()[0].actor,
        "agent"
    );

    // An author is the agent of `develop`; a reviewer the agent of `review`,
    // every reviewer kept on an ordinal of its own; a task of a request goal
    // has no agent on `pr`, and the merge goal's tasks none on `merge`.
    let agents = store.list_task_agents("t_review").await.unwrap();
    assert_eq!(
        agents
            .iter()
            .map(|a| (a.id.as_str(), a.step.as_str(), a.ordinal))
            .collect::<Vec<_>>(),
        [
            ("a_review", "develop", 0),
            ("r_one", "review", 1),
            ("r_two", "review", 2)
        ]
    );
    assert_eq!(agents[0].brief.as_deref(), Some("Keep this brief."));
    assert_eq!(
        store.agent_skills("r_one").await.unwrap()[0].name,
        "code-review"
    );
    assert_eq!(
        store
            .unstaffed_columns(&store.get_task("t_review").await.unwrap())
            .await
            .unwrap(),
        ["merge"]
    );
    // Staffing the column t_review lacks keeps its agents' rows, and with
    // them the sessions, the messages and the usage that name them; the
    // retry then takes the task.
    store
        .upsert_session_usage(
            "s_author",
            "prompt",
            ariadne_core::TokenUsage {
                input_tokens: 10,
                cached_input_tokens: 2,
                output_tokens: 5,
            },
        )
        .await
        .unwrap();
    store
        .update_task(
            "t_review",
            TaskUpdate {
                agents: Some(vec![
                    NewTaskAgent::new("develop", Vec::<String>::new(), default_pin()),
                    NewTaskAgent::new("review", Vec::<String>::new(), default_pin()),
                    NewTaskAgent::new("merge", Vec::<String>::new(), default_pin()),
                ]),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let staffed = store.list_task_agents("t_review").await.unwrap();
    assert_eq!(
        staffed
            .iter()
            .map(|a| (a.id.as_str(), a.step.as_str()))
            .collect::<Vec<_>>(),
        [
            ("a_review", "develop"),
            ("r_one", "review"),
            (staffed[2].id.as_str(), "merge")
        ],
        "the agents on their columns keep their rows; the second reviewer folds into the first"
    );
    assert!(staffed[2].id != "r_two");
    for (session, agent) in [("s_author", "a_review"), ("s_reviewer", "r_one")] {
        let kept = store.get_session(session).await.unwrap();
        assert_eq!(kept.task_agent_id.as_deref(), Some(agent), "{session}");
    }
    for message in ["m_request", "m_approve", "m_changes", "m_message"] {
        assert!(
            store.get_message(message).await.is_ok(),
            "{message} went with the staffing"
        );
    }
    assert_eq!(
        store
            .get_message("m_changes")
            .await
            .unwrap()
            .from_agent_id
            .as_deref(),
        Some("r_one"),
        "the second reviewer's message is the review column's now"
    );
    let usage = store.task_usage("t_review").await.unwrap();
    assert!(
        usage
            .iter()
            .any(|u| u.agent_id == "a_review" && u.usage.input_tokens == 10),
        "{usage:?}"
    );
    assert!(
        store
            .unstaffed_columns(&store.get_task("t_review").await.unwrap())
            .await
            .unwrap()
            .is_empty()
    );
    store
        .transition_task(
            "t_review",
            TaskStatus::Ready,
            Actor::User,
            Some("retried"),
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        store
            .start_first_step("t_review")
            .await
            .unwrap()
            .step
            .as_deref(),
        Some("develop")
    );

    assert_eq!(
        store.list_task_agents("t_pr").await.unwrap()[0].step,
        "develop"
    );
    // An author alone staffs `develop`: the task had no reviewer, so `review`
    // is unstaffed too, and nothing ever staffs `pr`.
    assert_eq!(
        store
            .unstaffed_columns(&store.get_task("t_pr").await.unwrap())
            .await
            .unwrap(),
        ["review", "pr"]
    );
    // Staffed, the task is one a retry takes.
    let mut pin = default_pin();
    pin.model = "stub:keeper".into();
    store
        .update_task(
            "t_pr",
            TaskUpdate {
                agents: Some(vec![
                    NewTaskAgent::new("develop", ["coding"], default_pin()),
                    NewTaskAgent::new("review", Vec::<String>::new(), default_pin()),
                    NewTaskAgent::new("pr", Vec::<String>::new(), pin),
                ]),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(
        store
            .unstaffed_columns(&store.get_task("t_pr").await.unwrap())
            .await
            .unwrap()
            .is_empty()
    );
    let retried = store
        .transition_task(
            "t_pr",
            TaskStatus::Ready,
            Actor::User,
            Some("retried"),
            None,
        )
        .await
        .unwrap();
    assert_eq!(retried.status(), TaskStatus::Ready);
    assert_eq!(
        store
            .start_first_step("t_pr")
            .await
            .unwrap()
            .step
            .as_deref(),
        Some("develop")
    );

    // Every author session and every reviewer session of a task is the
    // session of a column's agent; a request review session keeps its seat.
    for (session, seat) in [
        ("s_author", Seat::Agent),
        ("s_reviewer", Seat::Agent),
        ("s_orchestrator", Seat::Orchestrator),
        ("s_request", Seat::Reviewer),
    ] {
        assert_eq!(
            store.get_session(session).await.unwrap().seat(),
            Some(seat),
            "{session}"
        );
    }
    assert_eq!(
        store
            .get_session("s_request")
            .await
            .unwrap()
            .pull_request_id
            .as_deref(),
        Some("pr_theirs")
    );

    // A review request and the two verdicts are messages that open with
    // their old kind; the author and the reviewer that said them are agents.
    for (message, body, from, to) in [
        (
            "m_request",
            "[review_request] Please review.",
            Actor::Agent,
            Actor::Agent,
        ),
        (
            "m_approve",
            "[approve] Looks right.",
            Actor::Agent,
            Actor::Agent,
        ),
        (
            "m_changes",
            "[request_changes] Add a test.",
            Actor::Agent,
            Actor::Agent,
        ),
        (
            "m_message",
            "How far along?",
            Actor::Orchestrator,
            Actor::Agent,
        ),
        ("m_user", "Carry on.", Actor::User, Actor::Orchestrator),
    ] {
        let read = store.get_message(message).await.unwrap();
        assert_eq!(read.kind(), Some(MessageKind::Message), "{message}");
        assert_eq!(read.body, body, "{message}");
        assert_eq!(
            (read.from_actor(), read.to_actor()),
            (Some(from), Some(to)),
            "{message}"
        );
    }
    assert_eq!(
        store
            .get_message("m_approve")
            .await
            .unwrap()
            .from_session
            .as_deref(),
        Some("s_reviewer")
    );

    // The picks, the picked author, the goal branch and the landings leave
    // the schema.
    let raw = raw_pool(&path).await;
    let tables: Vec<String> =
        sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
            .fetch_all(&raw)
            .await
            .unwrap();
    assert!(!tables.iter().any(|t| t == "task_picks"), "{tables:?}");
    assert!(
        !columns_of(&raw, "tasks")
            .await
            .contains(&"picked_agent_id".to_string())
    );
    assert!(
        !columns_of(&raw, "goal_repositories")
            .await
            .contains(&"goal_branch".to_string())
    );
    assert!(
        !columns_of(&raw, "goals")
            .await
            .contains(&"landing".to_string())
    );
    assert!(
        !columns_of(&raw, "repositories")
            .await
            .contains(&"default_landing".to_string())
    );
    assert!(
        !columns_of(&raw, "task_agents")
            .await
            .contains(&"seat".to_string())
    );
    for (table, column, word) in [
        ("tasks", "status", "under_review"),
        ("task_agents", "step", "author"),
        ("agent_sessions", "seat", "author"),
        ("messages", "kind", "approve"),
        ("task_transitions", "actor", "reviewer"),
    ] {
        let refused = sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE {table} SET {column} = ? WHERE 0"
        )))
        .bind(word)
        .execute(&raw)
        .await;
        assert!(refused.is_ok(), "{table}.{column}");
        let sql: String = sqlx::query_scalar("SELECT sql FROM sqlite_master WHERE name = ?")
            .bind(table)
            .fetch_one(&raw)
            .await
            .unwrap();
        assert!(
            !sql.contains(&format!("'{word}'")),
            "{table}.{column} still allows {word}: {sql}"
        );
    }
    // What was kept is kept whole: the request row, and the first reviewer's
    // pin until the re-staffing above moved it.
    assert_eq!(
        store.get_pull_request("pr_theirs").await.unwrap().role,
        "reviewer"
    );
    assert_eq!(
        store.get_task_agent("r_one").await.unwrap().model,
        "stub:test-model"
    );

    // The backup opens on the new schema too, and a fresh database is on it
    // from the start.
    let recovered = Store::open(&backup).await.unwrap();
    assert_eq!(
        recovered.get_goal("g_pr").await.unwrap().workflow,
        "develop-review-pr"
    );
    let (fresh, fresh_dir) = test_store().await;
    drop(fresh);
    let fresh = raw_pool(&fresh_dir.path().join("test.db")).await;
    assert!(
        !columns_of(&fresh, "tasks")
            .await
            .contains(&"picked_agent_id".to_string())
    );
    assert!(
        columns_of(&fresh, "task_agents")
            .await
            .contains(&"step".to_string())
    );
    assert!(
        columns_of(&fresh, "goals")
            .await
            .contains(&"workflow".to_string())
    );
}

/// The 0023 migration folded into `old_pipeline_database`'s run drops whole
/// tables and columns. Opening it backs the database up beside itself
/// first, on its own — not on a copy the caller happened to make — and the
/// backup recovers what 0023 went on to drop.
#[tokio::test]
async fn opening_a_database_with_a_destructive_migration_pending_backs_it_up_first() {
    let dir = tempfile::tempdir().unwrap();
    let path = old_pipeline_database(&dir).await;
    let backup = std::path::PathBuf::from(format!("{}.backup-schema-20", path.display()));
    assert!(!backup.is_file(), "no backup before the open");

    let store = Store::open(&path).await.unwrap();
    store.close().await;
    assert!(
        backup.is_file(),
        "the open backed the database up on its own"
    );

    // Read before `Store::open` below migrates the backup file itself in
    // place: it is what 0023 would have dropped from `path`, undone.
    let raw = raw_pool(&backup).await;
    assert!(
        columns_of(&raw, "tasks")
            .await
            .contains(&"picked_agent_id".to_string()),
        "the backup still has what 0023 went on to drop"
    );
    raw.close().await;

    let recovered = Store::open(&backup).await.unwrap();
    assert_eq!(recovered.get_goal("g_merge").await.unwrap().title, "Merge");
}

#[tokio::test]
async fn invalid_stored_column_skills_refuse_staffing_without_writing_a_task() {
    use sqlx::Connection;

    let (store, dir) = test_store().await;
    let (goal, repo) = stepped_goal(&store).await;
    let mut db = sqlx::SqliteConnection::connect(&format!(
        "sqlite://{}",
        dir.path().join("test.db").display()
    ))
    .await
    .unwrap();
    sqlx::query("UPDATE goal_steps SET skills = '{}' WHERE goal_id = ? AND id = 'develop'")
        .bind(&goal.id)
        .execute(&mut db)
        .await
        .unwrap();
    let error = store
        .create_task(NewTask {
            goal_id: goal.id.clone(),
            repo_id: repo.id,
            title: "Build".into(),
            description: "Use valid skills.".into(),
            agents: step_agents(),
            depends_on: vec![],
        })
        .await
        .unwrap_err();
    assert!(matches!(error, StoreError::Invalid(_)));
    assert!(error.to_string().contains("develop"));
    assert!(
        store
            .list_tasks(TaskFilter {
                goal_id: Some(goal.id),
                ..Default::default()
            })
            .await
            .unwrap()
            .is_empty()
    );
}
