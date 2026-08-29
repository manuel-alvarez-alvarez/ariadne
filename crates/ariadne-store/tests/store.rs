//! Store integration tests against a temp-file SQLite database.

use ariadne_core::{
    Actor, AgentKind, AttentionReason, AuthorRole, GoalStatus, MergeStrategy, PromptKind,
    ReviewVerdict, Role, SessionStatus, TaskStatus, TokenUsage,
};
use ariadne_store::defaults::{default_prompt, default_system_prompt};
use ariadne_store::*;

async fn test_store() -> (Store, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("test.db")).await.unwrap();
    (store, dir)
}

async fn seed_profile(store: &Store, name: &str, role: Role) -> Profile {
    store
        .create_profile(NewProfile {
            name: name.into(),
            role,
            agent_kind: Some(AgentKind::ClaudeCode),
            model: None,
            effort: None,
            system_prompt: Some(format!("You are {name}.")),
        })
        .await
        .unwrap()
}

/// A registered repository, on a path of its own so goals can be seeded side
/// by side (one registration per path and base branch).
async fn seed_repository(store: &Store) -> Repository {
    store
        .create_repository(NewRepository {
            path: format!("/tmp/repo-{}", ariadne_core::id::new_id()),
            base_branch: "main".into(),
            description: None,
            merge_strategy: MergeStrategy::Direct,
        })
        .await
        .unwrap()
}

async fn seed_goal(store: &Store, planner: &Profile, max_tasks: Option<i64>) -> (Goal, Repository) {
    let repo = seed_repository(store).await;
    let goal = store
        .create_goal(NewGoal {
            title: "Test goal".into(),
            description: "desc".into(),
            planner_profile_id: planner.id.clone(),
            max_tasks,
            required_approvals: 1,
            repository_ids: vec![repo.id.clone()],
            pin: None,
        })
        .await
        .unwrap();
    (goal, repo)
}

/// The ramp almost every test starts on: a fresh database holding one planner
/// profile, one goal, the repository that goal works in, and one task on it.
struct World {
    store: Store,
    planner: Profile,
    goal: Goal,
    repo: Repository,
    task: Task,
    /// Kept for its Drop: the database lives in it.
    _dir: tempfile::TempDir,
}

impl World {
    async fn new() -> Self {
        let (store, dir) = test_store().await;
        let planner = seed_profile(&store, "planner", Role::Planner).await;
        let (goal, repo) = seed_goal(&store, &planner, None).await;
        let task = seed_task(&store, &goal, &repo, vec![]).await;
        Self {
            store,
            planner,
            goal,
            repo,
            task,
            _dir: dir,
        }
    }

    /// A session in this world's goal, on its task unless `task_id` says
    /// otherwise — a planner's has none.
    async fn session(
        &self,
        tmux: &str,
        role: Role,
        profile_id: &str,
        task_id: Option<&str>,
    ) -> AgentSession {
        self.store
            .create_session(NewSession {
                goal_id: self.goal.id.clone(),
                task_id: task_id.map(str::to_string),
                role,
                profile_id: profile_id.to_string(),
                agent_kind: AgentKind::ClaudeCode,
                model: None,
                effort: None,
                tmux_session: tmux.into(),
                worktree_path: Some("/tmp/wt".into()),
                review_round: None,
            })
            .await
            .unwrap()
    }

    /// The engineer session of this world's task.
    async fn engineer_session(&self) -> AgentSession {
        let profile = self.task.engineer_profile_id.clone();
        self.session(
            "ariadne-test-eng",
            Role::Engineer,
            &profile,
            Some(&self.task.id),
        )
        .await
    }

    /// This world's task as it now stands.
    async fn task(&self) -> Task {
        self.store.get_task(&self.task.id).await.unwrap()
    }
}

/// The happy path, one move at a time: what a task does between `pending` and
/// `merged`, and who does each of them.
const HAPPY_PATH: [(TaskStatus, Actor); 5] = [
    (TaskStatus::Ready, Actor::Daemon),
    (TaskStatus::InProgress, Actor::Daemon),
    (TaskStatus::UnderReview, Actor::Engineer),
    (TaskStatus::Approved, Actor::Daemon),
    (TaskStatus::Merged, Actor::Engineer),
];

/// Walk a task up the happy path from wherever it is to `upto`.
async fn walk_to(store: &Store, task_id: &str, upto: TaskStatus) -> Task {
    let now = store.get_task(task_id).await.unwrap().status();
    let from = HAPPY_PATH
        .iter()
        .position(|(status, _)| *status == now)
        .map_or(0, |at| at + 1);
    let mut task = store.get_task(task_id).await.unwrap();
    for (status, actor) in &HAPPY_PATH[from..] {
        let merge_commit = (*status == TaskStatus::Merged).then_some("abc123");
        task = store
            .transition_task(task_id, *status, *actor, None, merge_commit)
            .await
            .unwrap();
        if *status == upto {
            break;
        }
    }
    task
}

async fn seed_task(store: &Store, goal: &Goal, repo: &Repository, deps: Vec<String>) -> Task {
    let eng = seed_profile(
        store,
        &format!("eng-{}", ariadne_core::id::new_id()),
        Role::Engineer,
    )
    .await;
    let rev = seed_profile(
        store,
        &format!("rev-{}", ariadne_core::id::new_id()),
        Role::Reviewer,
    )
    .await;
    store
        .create_task(NewTask {
            goal_id: goal.id.clone(),
            repo_id: repo.id.clone(),
            title: "task".into(),
            description: "do things".into(),
            engineer_profile_id: eng.id,
            pin: None,
            reviewers: vec![ReviewerSlot::of(rev.id)],
            depends_on: deps,
        })
        .await
        .unwrap()
}

/// A fresh database knows how to launch every agent CLI, with the flags the
/// core defaults name — nothing to configure before the first spawn.
#[tokio::test]
async fn agent_configs_are_seeded_with_the_defaults() {
    let (store, _dir) = test_store().await;
    let configs = store.list_agent_configs().await.unwrap();
    assert_eq!(
        configs.iter().map(|c| c.agent_kind()).collect::<Vec<_>>(),
        AgentKind::ALL.to_vec()
    );
    for config in configs {
        assert_eq!(config.extra_flags(), config.default_flags());
    }
    // The bypass each CLI spells its own way, spelled out: this is what an
    // unconfigured Ariadne launches them with.
    for (kind, flag) in [
        (AgentKind::ClaudeCode, "--dangerously-skip-permissions"),
        (
            AgentKind::Codex,
            "--dangerously-bypass-approvals-and-sandbox",
        ),
        (AgentKind::Opencode, "--auto"),
    ] {
        assert_eq!(
            store.get_agent_config(kind).await.unwrap().extra_flags(),
            vec![flag.to_string()]
        );
    }
}

/// The flags are the user's to replace, emptying them included, and the
/// defaults stay readable beside them so a reset needs nothing remembered.
#[tokio::test]
async fn agent_config_flags_are_replaced_whole() {
    let (store, _dir) = test_store().await;
    let updated = store
        .update_agent_config(
            AgentKind::ClaudeCode,
            vec!["--permission-mode=acceptEdits".into()],
        )
        .await
        .unwrap();
    assert_eq!(
        updated.extra_flags(),
        vec!["--permission-mode=acceptEdits".to_string()]
    );
    assert_eq!(
        updated.default_flags(),
        vec!["--dangerously-skip-permissions".to_string()]
    );
    let emptied = store
        .update_agent_config(AgentKind::Codex, vec![])
        .await
        .unwrap();
    assert!(emptied.extra_flags().is_empty());
    // The edit is read back from the database, and one agent's flags are its
    // own: emptying codex left claude alone.
    assert_eq!(
        store
            .get_agent_config(AgentKind::ClaudeCode)
            .await
            .unwrap()
            .extra_flags(),
        vec!["--permission-mode=acceptEdits".to_string()]
    );
}

#[tokio::test]
async fn profile_crud_and_delete_guard() {
    let (store, _dir) = test_store().await;
    let p = seed_profile(&store, "planner-1", Role::Planner).await;
    assert_eq!(p.role(), Role::Planner);

    // Unique name enforced.
    let dup = store
        .create_profile(NewProfile {
            name: "planner-1".into(),
            role: Role::Planner,
            agent_kind: Some(AgentKind::Codex),
            model: None,
            effort: None,
            system_prompt: Some("x".into()),
        })
        .await;
    assert!(matches!(dup, Err(StoreError::Conflict(_))));

    // Delete blocked while referenced.
    let (_goal, _repo) = seed_goal(&store, &p, None).await;
    assert!(matches!(
        store.delete_profile(&p.id).await,
        Err(StoreError::Conflict(_))
    ));

    // Unreferenced profile deletes fine.
    let q = seed_profile(&store, "planner-2", Role::Planner).await;
    store.delete_profile(&q.id).await.unwrap();
    assert!(matches!(
        store.get_profile(&q.id).await,
        Err(StoreError::NotFound { .. })
    ));
}

#[tokio::test]
async fn repository_crud_and_unique_path_branch() {
    let (store, _dir) = test_store().await;
    let repo = store
        .create_repository(NewRepository {
            path: "/tmp/repo".into(),
            base_branch: "main".into(),
            description: Some("the one repo".into()),
            merge_strategy: Default::default(),
        })
        .await
        .unwrap();
    assert_eq!(repo.path, "/tmp/repo");
    assert_eq!(repo.description.as_deref(), Some("the one repo"));
    assert_eq!(
        repo.merge_strategy(),
        MergeStrategy::Direct,
        "a repository nobody said otherwise about is landed on directly"
    );

    // The same checkout on another branch is a different repository.
    let other = store
        .create_repository(NewRepository {
            path: "/tmp/repo".into(),
            base_branch: "next".into(),
            description: None,
            merge_strategy: Default::default(),
        })
        .await
        .unwrap();
    assert!(other.description.is_none());
    assert_eq!(store.list_repositories().await.unwrap().len(), 2);

    // (path, base_branch) is unique.
    let dup = store
        .create_repository(NewRepository {
            path: "/tmp/repo".into(),
            base_branch: "main".into(),
            description: None,
            merge_strategy: Default::default(),
        })
        .await;
    assert!(matches!(dup, Err(StoreError::Conflict(_))));

    // Partial update: the branch moves, the description is cleared, the path
    // stays exactly as it was.
    let edited = store
        .update_repository(
            &repo.id,
            RepositoryUpdate {
                base_branch: Some("trunk".into()),
                description: Some(None),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(edited.path, "/tmp/repo");
    assert_eq!(edited.base_branch, "trunk");
    assert!(edited.description.is_none());

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

/// A goal holds references, not copies: what it lists is whatever the
/// repositories say right now, and so is what its tasks resolve.
/// How a repository takes a change is the one thing about it an engineer has
/// to be told, and it round-trips like every other field: `direct` unless
/// somebody said otherwise, and editable either way.
#[tokio::test]
async fn a_repository_says_how_a_task_lands_on_it() {
    let (store, _dir) = test_store().await;
    let published = store
        .create_repository(NewRepository {
            path: "/tmp/published".into(),
            base_branch: "main".into(),
            description: None,
            merge_strategy: MergeStrategy::PullRequest,
        })
        .await
        .unwrap();
    assert_eq!(published.merge_strategy(), MergeStrategy::PullRequest);
    assert_eq!(
        store
            .get_repository(&published.id)
            .await
            .unwrap()
            .merge_strategy(),
        MergeStrategy::PullRequest
    );

    // Switched over, and an update that says nothing about it leaves it.
    let back = store
        .update_repository(
            &published.id,
            RepositoryUpdate {
                merge_strategy: Some(MergeStrategy::Direct),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(back.merge_strategy(), MergeStrategy::Direct);
    let renamed = store
        .update_repository(
            &published.id,
            RepositoryUpdate {
                description: Some(Some("still the one".into())),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(renamed.merge_strategy(), MergeStrategy::Direct);
    assert_eq!(renamed.description.as_deref(), Some("still the one"));
}

#[tokio::test]
async fn a_goal_reads_its_repositories_live() {
    let (store, _dir) = test_store().await;
    let planner = seed_profile(&store, "planner", Role::Planner).await;
    let api = seed_repository(&store).await;
    let ui = seed_repository(&store).await;

    let goal = store
        .create_goal(NewGoal {
            title: "Two repos".into(),
            description: "desc".into(),
            planner_profile_id: planner.id.clone(),
            max_tasks: None,
            required_approvals: 1,
            // The same repository named twice is one reference.
            repository_ids: vec![api.id.clone(), ui.id.clone(), api.id.clone()],
            pin: None,
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
    let planner = seed_profile(&store, "planner", Role::Planner).await;
    let repo = seed_repository(&store).await;
    let new_goal = |repository_ids: Vec<String>| NewGoal {
        title: "Goal".into(),
        description: "desc".into(),
        planner_profile_id: planner.id.clone(),
        max_tasks: None,
        required_approvals: 1,
        repository_ids,
        pin: None,
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
    let eng = seed_profile(&store, "eng", Role::Engineer).await;
    let rev = seed_profile(&store, "rev", Role::Reviewer).await;
    assert!(matches!(
        store
            .create_task(NewTask {
                goal_id: goal.id.clone(),
                repo_id: unrelated.id,
                title: "task".into(),
                description: "do things".into(),
                engineer_profile_id: eng.id,
                pin: None,
                reviewers: vec![ReviewerSlot::of(rev.id)],
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
    let eng = seed_profile(&w.store, "eng", Role::Engineer).await;
    let rev = seed_profile(&w.store, "rev", Role::Reviewer).await;
    let task = w
        .store
        .create_task(NewTask {
            goal_id: w.goal.id.clone(),
            repo_id: w.repo.id.clone(),
            title: "Fix the landing briefing: real fetch/rebase".into(),
            description: "d".into(),
            engineer_profile_id: eng.id,
            pin: None,
            reviewers: vec![ReviewerSlot::of(rev.id)],
            depends_on: vec![],
        })
        .await
        .unwrap();

    let tail = &task.id[task.id.len() - 6..];
    assert_eq!(
        task.branch,
        format!("fix-the-landing-briefing-real-fetch-{tail}")
    );
    assert!(!task.branch.contains("ariadne"), "{}", task.branch);
}

#[tokio::test]
async fn task_happy_path_to_merged() {
    let w = World::new().await;
    assert_eq!(w.task.status(), TaskStatus::Pending);

    let t = walk_to(&w.store, &w.task.id, TaskStatus::UnderReview).await;
    assert_eq!(t.review_round, 1, "review round bumps on under_review");

    let t = walk_to(&w.store, &w.task.id, TaskStatus::Merged).await;
    assert_eq!(t.status(), TaskStatus::Merged);
    assert_eq!(t.merge_commit.as_deref(), Some("abc123"));

    let audit = w.store.list_task_transitions(&t.id).await.unwrap();
    assert_eq!(audit.len(), 5);
    assert_eq!(audit[0].from_status, "pending");
    assert_eq!(audit[4].to_status, "merged");
}

#[tokio::test]
async fn illegal_transitions_are_rejected_and_unaudited() {
    let w = World::new().await;
    let task = &w.task;

    // Illegal edge.
    assert!(matches!(
        w.store
            .transition_task(
                &task.id,
                TaskStatus::Merged,
                Actor::Engineer,
                None,
                Some("x")
            )
            .await,
        Err(StoreError::Transition(_))
    ));
    // Legal edge, wrong actor.
    assert!(matches!(
        w.store
            .transition_task(&task.id, TaskStatus::Ready, Actor::Reviewer, None, None)
            .await,
        Err(StoreError::Transition(_))
    ));
    // Merged requires a commit.
    let t = walk_to(&w.store, &task.id, TaskStatus::Approved).await;
    assert!(matches!(
        w.store
            .transition_task(&t.id, TaskStatus::Merged, Actor::Engineer, None, None)
            .await,
        Err(StoreError::Invalid(_))
    ));

    let audit = w.store.list_task_transitions(&task.id).await.unwrap();
    assert_eq!(audit.len(), 4, "failed transitions leave no audit rows");
}

#[tokio::test]
async fn max_tasks_is_enforced() {
    let (store, _dir) = test_store().await;
    let planner = seed_profile(&store, "planner", Role::Planner).await;
    let (goal, repo) = seed_goal(&store, &planner, Some(1)).await;
    let _t1 = seed_task(&store, &goal, &repo, vec![]).await;

    let eng = seed_profile(&store, "eng-x", Role::Engineer).await;
    let rev = seed_profile(&store, "rev-x", Role::Reviewer).await;
    let t2 = store
        .create_task(NewTask {
            goal_id: goal.id.clone(),
            repo_id: repo.id.clone(),
            title: "too many".into(),
            description: "".into(),
            engineer_profile_id: eng.id,
            pin: None,
            reviewers: vec![ReviewerSlot::of(rev.id)],
            depends_on: vec![],
        })
        .await;
    assert!(matches!(t2, Err(StoreError::Conflict(_))));
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
    walk_to(store, &a.id, TaskStatus::Merged).await;
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

    // Retried, it is on its way again — and merged, it is where the task
    // waiting on it wanted it.
    store
        .transition_task(&dep.id, TaskStatus::Ready, Actor::User, None, None)
        .await
        .unwrap();
    assert_eq!(blocked().await, None, "a retried dependency blocks nothing");
    walk_to(store, &dep.id, TaskStatus::Merged).await;
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
    assert_eq!(audit[1].actor, "planner");

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

#[tokio::test]
async fn one_review_verdict_per_round() {
    let w = World::new().await;
    let (store, task) = (&w.store, &w.task);
    let reviewer = store.list_task_reviewers(&task.id).await.unwrap().remove(0);

    store
        .create_review(NewReview {
            task_id: task.id.clone(),
            round: 1,
            reviewer_profile_id: reviewer.clone(),
            session_id: None,
            verdict: ReviewVerdict::RequestChanges,
            body: Some("please fix".into()),
        })
        .await
        .unwrap();

    let dup = store
        .create_review(NewReview {
            task_id: task.id.clone(),
            round: 1,
            reviewer_profile_id: reviewer.clone(),
            session_id: None,
            verdict: ReviewVerdict::Approve,
            body: None,
        })
        .await;
    assert!(matches!(dup, Err(StoreError::Conflict(_))));

    // Next round is fine.
    store
        .create_review(NewReview {
            task_id: task.id.clone(),
            round: 2,
            reviewer_profile_id: reviewer,
            session_id: None,
            verdict: ReviewVerdict::Approve,
            body: None,
        })
        .await
        .unwrap();
    assert_eq!(
        store.list_reviews(&task.id, Some(2)).await.unwrap().len(),
        1
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
async fn messages_sessions_events_round_trip() {
    let w = World::new().await;
    let (store, goal, task) = (&w.store, &w.goal, &w.task);

    let session = w.engineer_session().await;
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
        .create_message(NewMessage {
            goal_id: goal.id.clone(),
            task_id: Some(task.id.clone()),
            author_role: AuthorRole::Engineer,
            author_session_id: Some(session.id.clone()),
            recipient: None,
            body: "starting work".into(),
        })
        .await
        .unwrap();
    let msgs = store.list_task_messages(&task.id, None, 50).await.unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].author_role(), AuthorRole::Engineer);
    // Keyset pagination: nothing after the last id.
    assert!(
        store
            .list_task_messages(&task.id, Some(&msgs[0].id), 50)
            .await
            .unwrap()
            .is_empty()
    );

    store
        .create_event(NewAgentEvent {
            session_id: Some(session.id.clone()),
            task_id: Some(task.id.clone()),
            agent_kind: Some(AgentKind::ClaudeCode),
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

/// A message can name who it is for: one profile, or the user. Saying nothing
/// keeps it addressed to the thread, the way every message was before
/// recipients existed, and whichever of the three it is survives the round
/// trip through the list a thread is read with.
#[tokio::test]
async fn a_message_can_be_addressed_to_a_profile_or_to_the_user() {
    let w = World::new().await;
    let (store, goal, task, planner) = (&w.store, &w.goal, &w.task, &w.planner);
    let engineer = Recipient::Profile(task.engineer_profile_id.clone());

    for (recipient, body) in [
        (Some(engineer.clone()), "over to you"),
        (Some(Recipient::User), "a question for you"),
        (None, "thinking out loud"),
    ] {
        store
            .create_message(NewMessage {
                goal_id: goal.id.clone(),
                task_id: Some(task.id.clone()),
                author_role: AuthorRole::Reviewer,
                author_session_id: None,
                recipient,
                body: body.into(),
            })
            .await
            .unwrap();
    }

    let msgs = store.list_task_messages(&task.id, None, 50).await.unwrap();
    assert_eq!(
        msgs.iter().map(Message::recipient).collect::<Vec<_>>(),
        vec![Some(engineer.clone()), Some(Recipient::User), None]
    );
    // The columns behind the accessor: only a profile addressee carries an id.
    assert_eq!(
        (
            msgs[0].recipient_kind.as_deref(),
            msgs[0].recipient_profile_id.as_deref()
        ),
        (Some("profile"), Some(task.engineer_profile_id.as_str()))
    );
    assert_eq!(
        (
            msgs[1].recipient_kind.as_deref(),
            msgs[1].recipient_profile_id.as_deref()
        ),
        (Some("user"), None)
    );
    assert_eq!(msgs[2].recipient_kind, None);

    // The goal thread addresses the same way, and what create_message returns
    // already carries the recipient it was given.
    let msg = store
        .create_message(NewMessage {
            goal_id: goal.id.clone(),
            task_id: None,
            author_role: AuthorRole::User,
            author_session_id: None,
            recipient: Some(Recipient::Profile(planner.id.clone())),
            body: "over to you, planner".into(),
        })
        .await
        .unwrap();
    let addressed_planner = Some(Recipient::Profile(planner.id.clone()));
    assert_eq!(msg.recipient(), addressed_planner);
    let goal_msgs = store.list_goal_messages(&goal.id, None, 50).await.unwrap();
    assert_eq!(goal_msgs[0].recipient(), addressed_planner);
}

/// A profile someone addressed is a profile the database is holding on to:
/// deleting it is refused the same way a profile in use by a goal or a task
/// is, rather than leaving the message pointing at nothing.
#[tokio::test]
async fn a_profile_a_message_addresses_cannot_be_deleted() {
    let w = World::new().await;
    let (store, goal) = (&w.store, &w.goal);
    // A profile nothing else references, so only the message can hold it.
    let bystander = seed_profile(store, "bystander", Role::Reviewer).await;

    store
        .create_message(NewMessage {
            goal_id: goal.id.clone(),
            task_id: None,
            author_role: AuthorRole::User,
            author_session_id: None,
            recipient: Some(Recipient::Profile(bystander.id.clone())),
            body: "a word with you".into(),
        })
        .await
        .unwrap();

    let err = store.delete_profile(&bystander.id).await.unwrap_err();
    let StoreError::Conflict(message) = err else {
        panic!("expected a conflict, got {err:?}");
    };
    assert!(
        message.contains("1 message as its addressee"),
        "the refusal says what holds the profile: {message}"
    );
}

/// What a launch is dated for: the one clock a watchdog reads when a session
/// has reported nothing at all. A relaunch moves the date, which is what makes
/// the silence it measures this run's rather than the row's.
#[tokio::test]
async fn every_launch_of_a_session_is_dated() {
    let w = World::new().await;
    let store = &w.store;

    let session = w.engineer_session().await;
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

/// A resumed agent conversation keeps its one session row: restarting puts
/// the row back where a spawn leaves it, so nothing downstream can tell the
/// relaunch from a first launch.
#[tokio::test]
async fn restarting_a_session_reopens_the_same_row() {
    let w = World::new().await;
    let (store, task) = (&w.store, &w.task);

    let session = w.engineer_session().await;
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
        .restart_session(&session.id, Some("/tmp/wt2"), Some(2))
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
        restarted.review_round,
        Some(2),
        "a reviewer's row names the round it is being relaunched for"
    );
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
    let again = store
        .restart_session(&session.id, None, None)
        .await
        .unwrap();
    assert_eq!(again.worktree_path.as_deref(), Some("/tmp/wt2"));
    assert_eq!(again.review_round, Some(2));

    assert!(
        store
            .restart_session("01ARZ3NDEKTSV4RRFFQ69G5FAV", None, None)
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

    let session = w.engineer_session().await;
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
    let session = w.engineer_session().await;

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
    let session = w.engineer_session().await;
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

    // A session with nothing up is a no-op; an id that names none still says so.
    store.clear_session_attention(&session.id).await.unwrap();
    store.clear_attention_after_idle(&session.id).await.unwrap();
    assert!(
        store
            .clear_attention_after_idle("01ARZ3NDEKTSV4RRFFQ69G5FAV")
            .await
            .is_err()
    );
}

/// Whether a tool that asks the user something is still waiting on them is
/// read off the session's own log, and the last event about such a call is the
/// whole of the answer: everything the turn reports in between belongs to its
/// other tool calls. Answered, dismissed by a prompt or ended with the turn,
/// the question is over — and the clear it takes with it is the narrow one.
#[tokio::test]
async fn a_pending_question_is_the_last_word_of_a_sessions_log() {
    let w = World::new().await;
    let store = &w.store;
    let session = w.engineer_session().await;
    let pending = async || {
        store
            .tool_call_is_pending(&session.id, "AskUserQuestion")
            .await
            .unwrap()
    };

    // A log with nothing in it has no question in it.
    assert!(!pending().await);
    for (kind, tool, expected) in [
        ("pre_tool_use", Some("Bash"), false),
        ("pre_tool_use", Some("AskUserQuestion"), true),
        // The rest of the turn's batch, running around the blocked call.
        ("pre_tool_use", Some("Bash"), true),
        ("post_tool_use", Some("Bash"), true),
        ("notification", None, true),
        // Answered, asked again, ended with the turn, asked again, typed over.
        ("post_tool_use", Some("AskUserQuestion"), false),
        ("pre_tool_use", Some("AskUserQuestion"), true),
        ("stop", None, false),
        ("pre_tool_use", Some("AskUserQuestion"), true),
        ("user_prompt_submit", None, false),
    ] {
        store
            .create_event(NewAgentEvent {
                session_id: Some(session.id.clone()),
                task_id: None,
                agent_kind: Some(AgentKind::ClaudeCode),
                kind: kind.into(),
                payload: match tool {
                    Some(tool) => serde_json::json!({"tool_name": tool}),
                    None => serde_json::json!({}),
                },
            })
            .await
            .unwrap();
        assert_eq!(pending().await, expected, "{kind} {tool:?}");
    }

    // What an answered question takes down is its own flag and no other: the
    // dialog and the message to the user are answered somewhere else.
    for (raised, left) in [
        (AttentionReason::WaitingInput, None),
        (
            AttentionReason::WaitingPermission,
            Some(AttentionReason::WaitingPermission),
        ),
        (
            AttentionReason::WaitingUser,
            Some(AttentionReason::WaitingUser),
        ),
    ] {
        store.clear_session_attention(&session.id).await.unwrap();
        store
            .set_session_attention(&session.id, raised)
            .await
            .unwrap();
        store.clear_question_attention(&session.id).await.unwrap();
        assert_eq!(
            store
                .get_session(&session.id)
                .await
                .unwrap()
                .attention_reason(),
            left,
            "{raised:?}"
        );
    }
}

/// The user speaking in a thread is the answer to whatever was waiting for
/// them there, and to nothing else: `waiting_user` comes down across the whole
/// thread, every other reason stays exactly where it was, and the threads
/// beside it are not touched.
#[tokio::test]
async fn a_user_message_takes_only_waiting_user_down_and_only_in_its_own_thread() {
    let w = World::new().await;
    let store = &w.store;
    let engineer = w.engineer_session().await;
    let reviewer = w
        .session(
            "ariadne-test-rev",
            Role::Reviewer,
            &w.planner.id.clone(),
            Some(&w.task.id),
        )
        .await;
    // The goal's own thread, which is where its planner works, and a second
    // task of the same goal: two threads the message is not written in.
    let planner = w
        .session("ariadne-test-plan", Role::Planner, &w.planner.id.clone(), None)
        .await;
    let other = seed_task(store, &w.goal, &w.repo, vec![]).await;
    let elsewhere = w
        .session(
            "ariadne-test-other",
            Role::Engineer,
            &other.engineer_profile_id.clone(),
            Some(&other.id),
        )
        .await;
    for session in [&engineer, &planner, &elsewhere] {
        store
            .set_session_attention(&session.id, AttentionReason::WaitingUser)
            .await
            .unwrap();
    }
    // The one flag a message answers nothing about: a dialog is on a pane.
    store
        .set_session_attention(&reviewer.id, AttentionReason::WaitingPermission)
        .await
        .unwrap();

    store
        .clear_user_attention_in_thread(&w.goal.id, Some(&w.task.id))
        .await
        .unwrap();

    let reason = async |session: &AgentSession| {
        store
            .get_session(&session.id)
            .await
            .unwrap()
            .attention_reason()
    };
    assert_eq!(reason(&engineer).await, None, "the thread it was written in");
    assert_eq!(
        reason(&reviewer).await,
        Some(AttentionReason::WaitingPermission),
        "a dialog on a pane is not answered from a conversation"
    );
    assert_eq!(
        reason(&planner).await,
        Some(AttentionReason::WaitingUser),
        "the goal's own thread is not this task's"
    );
    assert_eq!(
        reason(&elsewhere).await,
        Some(AttentionReason::WaitingUser),
        "and neither is another task's"
    );

    // The goal thread reaches the sessions sitting on no task, and only them.
    store
        .clear_user_attention_in_thread(&w.goal.id, None)
        .await
        .unwrap();
    assert_eq!(reason(&planner).await, None);
    assert_eq!(reason(&elsewhere).await, Some(AttentionReason::WaitingUser));
}

/// What an agent's own detectors may raise over, and what they may not.
///
/// `waiting_user` is the one flag no agent put up: it says a person owes this
/// task something — a message written to them, a request that is theirs to
/// merge — and a prompt, a disconnect or a stall neither settles that nor is
/// more use to whoever is reading the strip. So a raise from any of them is
/// withheld, clock included, and the write says nothing to the watchers
/// either: nothing about the session changed.
#[tokio::test]
async fn an_agents_own_reason_does_not_replace_the_attention_raised_for_the_user() {
    let w = World::new().await;
    let store = &w.store;
    let session = w.engineer_session().await;
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

/// The summary a round was asked for review with is the round's own, read off
/// the transition that opened it — the latest one, so a second round answers
/// with what was submitted for it and not for the first.
#[tokio::test]
async fn the_review_summary_is_the_reason_of_the_latest_review_request() {
    let w = World::new().await;
    let (store, task) = (&w.store, &w.task);
    assert_eq!(store.review_summary(&task.id).await.unwrap(), None);

    let ask = async |summary: &str| {
        store
            .transition_task(
                &task.id,
                TaskStatus::UnderReview,
                Actor::Engineer,
                Some(summary),
                None,
            )
            .await
            .unwrap();
    };
    walk_to(store, &task.id, TaskStatus::InProgress).await;
    ask("the first pass, with a test per lane").await;
    assert_eq!(
        store.review_summary(&task.id).await.unwrap().as_deref(),
        Some("the first pass, with a test per lane")
    );

    // A round of changes, and a second request with its own summary.
    for (status, actor) in [
        (TaskStatus::ChangesRequested, Actor::Daemon),
        (TaskStatus::InProgress, Actor::Daemon),
    ] {
        store
            .transition_task(&task.id, status, actor, None, None)
            .await
            .unwrap();
    }
    ask("the lane widths, as asked").await;
    assert_eq!(
        store.review_summary(&task.id).await.unwrap().as_deref(),
        Some("the lane widths, as asked")
    );
}

/// A stalled task is a task with a stalled agent on it: the flag on the
/// session is where that is decided, and the task's own column is the
/// projection of it, written by the attention change and by nothing else.
#[tokio::test]
async fn a_task_is_stalled_while_one_of_its_agents_is() {
    let w = World::new().await;
    let (store, task) = (&w.store, &w.task);
    let engineer = w.engineer_session().await;
    let reviewer = w
        .session(
            "ariadne-test-rev",
            Role::Reviewer,
            &w.planner.id.clone(),
            Some(&task.id),
        )
        .await;
    assert!(!w.task().await.is_stalled());

    store
        .set_session_attention(&engineer.id, AttentionReason::Stalled)
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
    store.clear_agent_attention(&engineer.id).await.unwrap();
    assert!(
        !store.get_task(&task.id).await.unwrap().is_stalled(),
        "an agent that is working again leaves no stall behind"
    );

    // And so does the relaunch that puts a stuck one back on its feet.
    store
        .set_session_attention(&engineer.id, AttentionReason::Stalled)
        .await
        .unwrap();
    assert!(store.get_task(&task.id).await.unwrap().is_stalled());
    store
        .restart_session(&engineer.id, None, None)
        .await
        .unwrap();
    assert!(!store.get_task(&task.id).await.unwrap().is_stalled());

    // A planner has no task to project onto, and says so on its own row.
    let alone = w
        .session(
            "ariadne-test-plan",
            Role::Planner,
            &w.planner.id.clone(),
            None,
        )
        .await;
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

/// A prompt is a dialog on the agent's terminal, so it cannot outlive the
/// session it was raised on: retiring one takes `waiting_permission` /
/// `waiting_input` down with it, and leaves every reason a session ends
/// *carrying* exactly where it is.
#[tokio::test]
async fn retiring_a_session_drops_the_prompt_it_can_no_longer_answer() {
    let w = World::new().await;
    let store = &w.store;

    let session = w.engineer_session().await;
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
    let failed = w.engineer_session().await;
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
    let session = w.engineer_session().await;
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
        let racing = w.engineer_session().await;
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

/// Every status a goal can be in survives the round trip through SQLite,
/// whose `CHECK` on the column is a second copy of the enum: a status the
/// constraint has not been told about is not a wrong answer but a write that
/// fails.
#[tokio::test]
async fn every_goal_status_round_trips_through_the_database() {
    let (store, _dir) = test_store().await;
    let planner = seed_profile(&store, "planner", Role::Planner).await;
    let (goal, _) = seed_goal(&store, &planner, None).await;

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
    let planner = seed_profile(&store, "planner", Role::Planner).await;
    let (planning, _) = seed_goal(&store, &planner, None).await;
    let (active, _) = seed_goal(&store, &planner, None).await;
    let (cancelled, _) = seed_goal(&store, &planner, None).await;
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

/// A fresh database holds three profiles and not one prompt: what they are
/// briefed with is the code's, which is what an empty `profile_prompts` and a
/// NULL `system_prompt` mean.
#[tokio::test]
async fn a_fresh_database_is_seeded_with_the_built_in_profiles_on_every_default() {
    let (store, _dir) = test_store().await;
    for (name, role) in [
        ("Planner", Role::Planner),
        ("Engineer", Role::Engineer),
        ("Reviewer", Role::Reviewer),
    ] {
        let p = store.get_profile_by_name(name).await.unwrap();
        assert_eq!(p.role(), role);
        assert!(p.agent_kind().is_none(), "{name} must have no agent kind");
        assert!(p.model.is_none(), "{name} must have no model");
        assert!(
            p.system_prompt.is_none(),
            "{name} stores no system prompt of its own"
        );
        assert_eq!(
            p.effective_system_prompt(),
            default_system_prompt(role),
            "{name} is briefed with the role default system prompt"
        );
        assert!(
            p.effective_system_prompt().contains("Ariadne"),
            "{name}'s system prompt says what it is the role of"
        );
        // Exactly the role's prompt kinds, each of them the default itself.
        let prompts = store.list_profile_prompts(&p.id).await.unwrap();
        assert_eq!(
            prompts.iter().map(|p| p.kind()).collect::<Vec<_>>(),
            PromptKind::for_role(role),
            "{name} owns the prompts of its role"
        );
        for prompt in &prompts {
            assert_eq!(prompt.content, default_prompt(role, prompt.kind()).unwrap());
            assert!(prompt.is_default, "{name} stored a {} prompt", prompt.kind);
            assert!(prompt.updated_at.is_none());
        }
    }

    // Not one prompt row was written for any of them.
    assert_eq!(prompt_rows(&_dir).await, 0);

    // Fixed, recognizable ids; the reviewer carries the newer persona.
    let reviewer = store.get_profile_by_name("Reviewer").await.unwrap();
    assert_eq!(reviewer.id, "00000000000000000000000003");
    assert!(
        reviewer
            .effective_system_prompt()
            .contains("install dependencies and run the build"),
        "reviewers are told to install dependencies and verify"
    );
    // User edits stick.
    let engineer = store.get_profile_by_name("Engineer").await.unwrap();
    assert_eq!(engineer.id, "00000000000000000000000002");
    store
        .update_profile(
            &engineer.id,
            ProfileUpdate {
                system_prompt: Some("custom".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let engineer = store.get_profile_by_name("Engineer").await.unwrap();
    assert_eq!(engineer.system_prompt.as_deref(), Some("custom"));
    assert_eq!(engineer.effective_system_prompt(), "custom");
}

/// How many prompts the database is actually holding, read from the file
/// itself: what the store answers is the effective prompt, which says nothing
/// about whether a row is behind it.
async fn prompt_rows(dir: &tempfile::TempDir) -> i64 {
    prompt_rows_at(&dir.path().join("test.db")).await
}

async fn prompt_rows_at(path: &std::path::Path) -> i64 {
    let pool = sqlx::SqlitePool::connect(&format!("sqlite://{}", path.display()))
        .await
        .unwrap();
    let rows = sqlx::query_scalar("SELECT COUNT(*) FROM profile_prompts")
        .fetch_one(&pool)
        .await
        .unwrap();
    pool.close().await;
    rows
}

/// Seeding keys off an empty `profiles` table only: deleting a built-in is
/// permanent, and a reopened database is not re-seeded behind the user's back.
#[tokio::test]
async fn built_ins_are_not_recreated_on_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");

    let store = Store::open(&path).await.unwrap();
    let planner = store.get_profile_by_name("Planner").await.unwrap();
    store.delete_profile(&planner.id).await.unwrap();
    store
        .update_profile_prompt(
            &store.get_profile_by_name("Engineer").await.unwrap().id,
            PromptKind::EngineerBriefing,
            "mine",
        )
        .await
        .unwrap();
    drop(store);

    let store = Store::open(&path).await.unwrap();
    assert!(matches!(
        store.get_profile_by_name("Planner").await,
        Err(StoreError::NotFound { .. })
    ));
    let engineer = store.get_profile_by_name("Engineer").await.unwrap();
    assert_eq!(
        store
            .get_profile_prompt(&engineer.id, PromptKind::EngineerBriefing)
            .await
            .unwrap()
            .content,
        "mine"
    );
}

#[tokio::test]
async fn a_new_profile_starts_on_the_role_defaults_and_stores_none_of_them() {
    let (store, _dir) = test_store().await;
    let reviewer = seed_profile(&store, "rev-strict", Role::Reviewer).await;

    let prompts = store.list_profile_prompts(&reviewer.id).await.unwrap();
    assert_eq!(
        prompts.iter().map(|p| p.kind()).collect::<Vec<_>>(),
        PromptKind::for_role(Role::Reviewer)
    );
    assert_eq!(
        prompts[0].content,
        default_prompt(Role::Reviewer, PromptKind::ReviewerBriefing).unwrap()
    );
    assert!(prompts.iter().all(|p| p.is_default));
    assert_eq!(prompt_rows(&_dir).await, 0);
    // Its system prompt is the one it was created with, not the role default.
    assert_eq!(
        reviewer.system_prompt.as_deref(),
        Some("You are rev-strict.")
    );
    assert!(!reviewer.system_prompt_is_default());

    // Created without one, it follows its role's instead.
    let plain = store
        .create_profile(NewProfile {
            name: "rev-plain".into(),
            role: Role::Reviewer,
            agent_kind: None,
            model: None,
            effort: None,
            system_prompt: None,
        })
        .await
        .unwrap();
    assert!(plain.system_prompt.is_none());
    assert_eq!(
        plain.effective_system_prompt(),
        default_system_prompt(Role::Reviewer)
    );
}

/// Setting a prompt is what writes a row, and resetting is what takes it away
/// again: what is left over is the default, which nothing stores.
#[tokio::test]
async fn a_prompt_is_stored_only_while_it_is_set_and_a_reset_deletes_it() {
    let (store, _dir) = test_store().await;
    let engineer = store.get_profile_by_name("Engineer").await.unwrap();

    let updated = store
        .update_profile_prompt(&engineer.id, PromptKind::ChangesRequested, "fix it")
        .await
        .unwrap();
    assert_eq!(updated.content, "fix it");
    assert_eq!(updated.kind(), PromptKind::ChangesRequested);
    assert!(!updated.is_default);
    assert!(updated.updated_at.is_some());
    // One prompt was set, so one row exists.
    assert_eq!(prompt_rows(&_dir).await, 1);
    assert_eq!(
        store
            .get_profile_prompt(&engineer.id, PromptKind::ChangesRequested)
            .await
            .unwrap()
            .content,
        "fix it"
    );
    // Only the prompt that was written changed.
    assert_eq!(
        store
            .get_profile_prompt(&engineer.id, PromptKind::EngineerBriefing)
            .await
            .unwrap()
            .content,
        default_prompt(Role::Engineer, PromptKind::EngineerBriefing).unwrap()
    );

    let reset = store
        .reset_profile_prompt(&engineer.id, PromptKind::ChangesRequested)
        .await
        .unwrap();
    assert_eq!(
        reset.content,
        default_prompt(Role::Engineer, PromptKind::ChangesRequested).unwrap()
    );
    assert!(reset.is_default);
    // The row is gone with it: a default is stored nowhere.
    assert_eq!(prompt_rows(&_dir).await, 0);

    // The system prompt resets the same way.
    store
        .update_profile(
            &engineer.id,
            ProfileUpdate {
                system_prompt: Some("custom".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let restored = store.reset_system_prompt(&engineer.id).await.unwrap();
    assert!(restored.system_prompt.is_none());
    assert_eq!(
        restored.effective_system_prompt(),
        default_system_prompt(Role::Engineer)
    );
}

/// A briefing may drop placeholders, keep braces that are only braces, and say
/// nothing at all — but a token nothing will ever substitute is refused, on
/// both write paths, with the offender and the allowed set in the message.
#[tokio::test]
async fn a_template_naming_a_placeholder_its_kind_cannot_fill_in_is_refused() {
    let (store, _dir) = test_store().await;
    let engineer = store.get_profile_by_name("Engineer").await.unwrap();

    let Err(StoreError::Invalid(message)) = store
        .update_profile_prompt(
            &engineer.id,
            PromptKind::EngineerBriefing,
            "# {task_titel}\n\n{task_description}",
        )
        .await
    else {
        panic!("a typo'd placeholder was stored");
    };
    assert!(
        message.contains("{task_titel}") && message.contains("{task_title}"),
        "unhelpful message: {message}"
    );
    // ...and nothing was written.
    assert_eq!(
        store
            .get_profile_prompt(&engineer.id, PromptKind::EngineerBriefing)
            .await
            .unwrap()
            .content,
        default_prompt(Role::Engineer, PromptKind::EngineerBriefing).unwrap()
    );

    // What renders as itself still saves: literal braces, JSON, no
    // placeholders at all.
    let engineer = store.get_profile_by_name("Engineer").await.unwrap();
    for content in [
        "Land {branch} on {base_branch}, then answer {\"merged\": true}.",
        "Do it yourself.",
        "{unclosed and {branch}",
    ] {
        store
            .update_profile_prompt(&engineer.id, PromptKind::LandingDirect, content)
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn prompt_kinds_are_checked_against_the_profile_role() {
    let (store, _dir) = test_store().await;
    let engineer = store.get_profile_by_name("Engineer").await.unwrap();

    // A kind of another role is rejected on every access path.
    assert!(matches!(
        store
            .get_profile_prompt(&engineer.id, PromptKind::ReviewerResume)
            .await,
        Err(StoreError::Invalid(_))
    ));
    assert!(matches!(
        store
            .update_profile_prompt(&engineer.id, PromptKind::PlannerBriefing, "nope")
            .await,
        Err(StoreError::Invalid(_))
    ));
    assert!(matches!(
        store
            .reset_profile_prompt(&engineer.id, PromptKind::PlannerBriefing)
            .await,
        Err(StoreError::Invalid(_))
    ));
    // ...and nothing was written.
    assert_eq!(
        store
            .list_profile_prompts(&engineer.id)
            .await
            .unwrap()
            .len(),
        PromptKind::for_role(Role::Engineer).len()
    );

    // Unknown profiles and unknown kinds are proper errors, not panics.
    assert!(matches!(
        store
            .get_profile_prompt("01ARZ3NDEKTSV4RRFFQ69G5FAV", PromptKind::EngineerBriefing)
            .await,
        Err(StoreError::NotFound { .. })
    ));
    assert!(matches!(
        parse_prompt_kind("engineer_playbook"),
        Err(StoreError::Invalid(_))
    ));
    assert_eq!(
        parse_prompt_kind("engineer_briefing").unwrap(),
        PromptKind::EngineerBriefing
    );
}

/// The delete itself is the assertion: foreign keys are on, so without
/// `ON DELETE CASCADE` the seeded prompt rows would make it fail.
#[tokio::test]
async fn deleting_a_profile_takes_its_prompts_with_it() {
    let (store, _dir) = test_store().await;
    let profile = seed_profile(&store, "planner-x", Role::Planner).await;
    assert_eq!(
        store.list_profile_prompts(&profile.id).await.unwrap().len(),
        PromptKind::for_role(Role::Planner).len()
    );

    store.delete_profile(&profile.id).await.unwrap();
    assert!(matches!(
        store.list_profile_prompts(&profile.id).await,
        Err(StoreError::NotFound { .. })
    ));
}

/// A profile with an agent and model of its own, for the pinning tests.
async fn seed_pinned_profile(
    store: &Store,
    name: &str,
    role: Role,
    agent_kind: Option<AgentKind>,
    model: Option<&str>,
) -> Profile {
    store
        .create_profile(NewProfile {
            name: name.into(),
            role,
            agent_kind,
            model: model.map(str::to_string),
            effort: None,
            system_prompt: Some(format!("You are {name}.")),
        })
        .await
        .unwrap()
}

/// Creation snapshots the agent and model off the profiles; editing a profile
/// afterwards moves nothing that already exists. The goal, the task and every
/// reviewer slot pin separately, from the profile each of them names.
#[tokio::test]
async fn creation_pins_the_agent_and_model_of_every_profile() {
    let (store, _dir) = test_store().await;
    let planner = seed_pinned_profile(
        &store,
        "planner-pin",
        Role::Planner,
        Some(AgentKind::ClaudeCode),
        Some("opus"),
    )
    .await;
    let engineer = seed_pinned_profile(
        &store,
        "engineer-pin",
        Role::Engineer,
        Some(AgentKind::Codex),
        Some("gpt-5"),
    )
    .await;
    let reviewer = seed_pinned_profile(
        &store,
        "reviewer-pin",
        Role::Reviewer,
        Some(AgentKind::Opencode),
        Some("sonnet"),
    )
    .await;

    let (goal, repo) = seed_goal(&store, &planner, None).await;
    let task = store
        .create_task(NewTask {
            goal_id: goal.id.clone(),
            repo_id: repo.id.clone(),
            title: "task".into(),
            description: "do things".into(),
            engineer_profile_id: engineer.id.clone(),
            pin: None,
            reviewers: vec![ReviewerSlot::of(reviewer.id.clone())],
            depends_on: vec![],
        })
        .await
        .unwrap();

    assert_eq!(goal.agent_kind(), Some(AgentKind::ClaudeCode));
    assert_eq!(goal.model.as_deref(), Some("opus"));
    assert_eq!(task.agent_kind(), Some(AgentKind::Codex));
    assert_eq!(task.model.as_deref(), Some("gpt-5"));
    let pins = store.list_task_reviewer_pins(&task.id).await.unwrap();
    assert_eq!(pins.len(), 1);
    assert_eq!(pins[0].profile_id, reviewer.id);
    assert_eq!(pins[0].position, 0);
    assert_eq!(pins[0].agent_kind(), Some(AgentKind::Opencode));
    assert_eq!(pins[0].model.as_deref(), Some("sonnet"));

    // Every profile is moved to a different agent and a different model.
    for profile in [&planner, &engineer, &reviewer] {
        store
            .update_profile(
                &profile.id,
                ProfileUpdate {
                    agent_kind: Some(Some(AgentKind::ClaudeCode)),
                    model: Some(Some("haiku".into())),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
    }
    assert_eq!(
        store
            .get_profile(&engineer.id)
            .await
            .unwrap()
            .model
            .as_deref(),
        Some("haiku"),
        "the edit did land on the profile"
    );

    // And nothing already created followed it.
    let goal = store.get_goal(&goal.id).await.unwrap();
    assert_eq!(goal.agent_kind(), Some(AgentKind::ClaudeCode));
    assert_eq!(goal.model.as_deref(), Some("opus"));
    let task = store.get_task(&task.id).await.unwrap();
    assert_eq!(task.agent_kind(), Some(AgentKind::Codex));
    assert_eq!(task.model.as_deref(), Some("gpt-5"));
    let pins = store.list_task_reviewer_pins(&task.id).await.unwrap();
    assert_eq!(pins[0].agent_kind(), Some(AgentKind::Opencode));
    assert_eq!(pins[0].model.as_deref(), Some("sonnet"));
}

/// Auto and CLI-default are pin values like any other: a task created off an
/// unpinned profile stays auto even after the profile picks an agent, rather
/// than reading as "not pinned yet" and resolving live.
#[tokio::test]
async fn auto_and_default_are_pinned_as_such() {
    let (store, _dir) = test_store().await;
    let planner = seed_pinned_profile(&store, "planner-auto", Role::Planner, None, None).await;
    let engineer = seed_pinned_profile(&store, "engineer-auto", Role::Engineer, None, None).await;
    let reviewer = seed_pinned_profile(&store, "reviewer-auto", Role::Reviewer, None, None).await;

    let (goal, repo) = seed_goal(&store, &planner, None).await;
    let task = store
        .create_task(NewTask {
            goal_id: goal.id.clone(),
            repo_id: repo.id.clone(),
            title: "task".into(),
            description: "do things".into(),
            engineer_profile_id: engineer.id.clone(),
            pin: None,
            reviewers: vec![ReviewerSlot::of(reviewer.id.clone())],
            depends_on: vec![],
        })
        .await
        .unwrap();

    assert_eq!(goal.agent_kind(), None);
    assert_eq!(goal.model, None);
    assert_eq!(task.agent_kind(), None);
    assert_eq!(task.model, None);

    for profile in [&planner, &engineer, &reviewer] {
        store
            .update_profile(
                &profile.id,
                ProfileUpdate {
                    agent_kind: Some(Some(AgentKind::Codex)),
                    model: Some(Some("gpt-5".into())),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
    }

    let goal = store.get_goal(&goal.id).await.unwrap();
    assert_eq!(goal.agent_kind(), None, "still auto");
    assert_eq!(goal.model, None, "still the CLI default");
    let task = store.get_task(&task.id).await.unwrap();
    assert_eq!(task.agent_kind(), None, "still auto");
    assert_eq!(task.model, None, "still the CLI default");
    let pins = store.list_task_reviewer_pins(&task.id).await.unwrap();
    assert_eq!(pins[0].agent_kind(), None);
    assert_eq!(pins[0].model, None);
}

/// Reassigning reviewers writes new slots, so each one pins the profile as it
/// stands at that moment — not what it was when the task was created.
#[tokio::test]
async fn reassigned_reviewers_pin_the_profile_they_are_assigned_from() {
    let (store, _dir) = test_store().await;
    let planner = seed_pinned_profile(&store, "planner-re", Role::Planner, None, None).await;
    let engineer = seed_pinned_profile(&store, "engineer-re", Role::Engineer, None, None).await;
    let first = seed_pinned_profile(
        &store,
        "reviewer-re-1",
        Role::Reviewer,
        Some(AgentKind::ClaudeCode),
        Some("opus"),
    )
    .await;
    let second = seed_pinned_profile(
        &store,
        "reviewer-re-2",
        Role::Reviewer,
        Some(AgentKind::Codex),
        Some("gpt-5"),
    )
    .await;

    let (goal, repo) = seed_goal(&store, &planner, None).await;
    let task = store
        .create_task(NewTask {
            goal_id: goal.id.clone(),
            repo_id: repo.id.clone(),
            title: "task".into(),
            description: "do things".into(),
            engineer_profile_id: engineer.id.clone(),
            pin: None,
            reviewers: vec![ReviewerSlot::of(first.id.clone())],
            depends_on: vec![],
        })
        .await
        .unwrap();

    store
        .update_profile(
            &second.id,
            ProfileUpdate {
                model: Some(Some("gpt-5-codex".into())),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    store
        .update_task(
            &task.id,
            TaskUpdate {
                reviewers: Some(vec![
                    ReviewerSlot::of(&second.id),
                    ReviewerSlot::of(&first.id),
                ]),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    let pins = store.list_task_reviewer_pins(&task.id).await.unwrap();
    assert_eq!(
        pins.iter()
            .map(|p| p.profile_id.as_str())
            .collect::<Vec<_>>(),
        vec![second.id.as_str(), first.id.as_str()]
    );
    assert_eq!(pins[0].agent_kind(), Some(AgentKind::Codex));
    assert_eq!(
        pins[0].model.as_deref(),
        Some("gpt-5-codex"),
        "as it is now"
    );
    assert_eq!(pins[1].agent_kind(), Some(AgentKind::ClaudeCode));
    assert_eq!(pins[1].model.as_deref(), Some("opus"));
}

/// An agent and model chosen for a goal, a task or a slot are what gets
/// pinned — the profile's own pins are what a slot with no choice on it falls
/// back to, not a floor the choice is merged into. A pin naming only its agent
/// writes a null model, which is that CLI's own default.
#[tokio::test]
async fn a_chosen_pin_is_written_in_place_of_the_profiles() {
    let (store, _dir) = test_store().await;
    let planner = seed_pinned_profile(
        &store,
        "planner-chosen",
        Role::Planner,
        Some(AgentKind::ClaudeCode),
        Some("claude-opus-5"),
    )
    .await;
    let engineer = seed_pinned_profile(
        &store,
        "engineer-chosen",
        Role::Engineer,
        Some(AgentKind::ClaudeCode),
        Some("claude-opus-5"),
    )
    .await;
    let chosen = seed_pinned_profile(
        &store,
        "reviewer-chosen",
        Role::Reviewer,
        Some(AgentKind::ClaudeCode),
        Some("claude-opus-5"),
    )
    .await;
    let untouched = seed_pinned_profile(
        &store,
        "reviewer-untouched",
        Role::Reviewer,
        Some(AgentKind::Opencode),
        Some("ollama/llama3:8b"),
    )
    .await;

    let repo = seed_repository(&store).await;
    let goal = store
        .create_goal(NewGoal {
            title: "Chosen".into(),
            description: "desc".into(),
            planner_profile_id: planner.id.clone(),
            max_tasks: None,
            required_approvals: 1,
            repository_ids: vec![repo.id.clone()],
            pin: Some(AgentPin {
                agent_kind: AgentKind::Codex,
                model: Some("gpt-5.3-codex".into()),
                effort: None,
            }),
        })
        .await
        .unwrap();
    assert_eq!(goal.agent_kind(), Some(AgentKind::Codex));
    assert_eq!(goal.model.as_deref(), Some("gpt-5.3-codex"));

    let task = store
        .create_task(NewTask {
            goal_id: goal.id.clone(),
            repo_id: repo.id.clone(),
            title: "task".into(),
            description: "do things".into(),
            engineer_profile_id: engineer.id.clone(),
            pin: Some(AgentPin {
                agent_kind: AgentKind::Codex,
                model: Some("gpt-5.6-sol".into()),
                effort: None,
            }),
            reviewers: vec![
                ReviewerSlot {
                    profile_id: chosen.id.clone(),
                    pin: Some(AgentPin {
                        agent_kind: AgentKind::Opencode,
                        model: Some("ollama/llama3:8b".into()),
                        effort: None,
                    }),
                },
                ReviewerSlot::of(&untouched.id),
            ],
            depends_on: vec![],
        })
        .await
        .unwrap();

    assert_eq!(task.agent_kind(), Some(AgentKind::Codex));
    assert_eq!(task.model.as_deref(), Some("gpt-5.6-sol"));
    let pins = store.list_task_reviewer_pins(&task.id).await.unwrap();
    assert_eq!(pins[0].agent_kind(), Some(AgentKind::Opencode));
    assert_eq!(pins[0].model.as_deref(), Some("ollama/llama3:8b"));
    assert_eq!(
        pins[1].agent_kind(),
        Some(AgentKind::Opencode),
        "the slot nobody chose for took its profile's"
    );
    assert_eq!(pins[1].model.as_deref(), Some("ollama/llama3:8b"));

    // A goal created without a choice is the case that must not have moved.
    let plain = store
        .create_goal(NewGoal {
            title: "Plain".into(),
            description: "desc".into(),
            planner_profile_id: planner.id.clone(),
            max_tasks: None,
            required_approvals: 1,
            repository_ids: vec![repo.id.clone()],
            pin: None,
        })
        .await
        .unwrap();
    assert_eq!(plain.agent_kind(), Some(AgentKind::ClaudeCode));
    assert_eq!(plain.model.as_deref(), Some("claude-opus-5"));

    // An agent with no model of its own: the CLI is pinned and the model is
    // left null, which is the CLI's default rather than the profile's model.
    let agent_only = store
        .create_goal(NewGoal {
            title: "Agent only".into(),
            description: "desc".into(),
            planner_profile_id: planner.id.clone(),
            max_tasks: None,
            required_approvals: 1,
            repository_ids: vec![repo.id.clone()],
            pin: Some(AgentPin {
                agent_kind: AgentKind::Opencode,
                model: None,
                effort: None,
            }),
        })
        .await
        .unwrap();
    assert_eq!(agent_only.agent_kind(), Some(AgentKind::Opencode));
    assert_eq!(agent_only.model, None);
}

/// Editing a pending task moves its pins and puts them back: cleared, they
/// return to the engineer profile's as it stands at that moment, which is the
/// same rule reassigning a reviewer follows.
#[tokio::test]
async fn a_task_pin_can_be_moved_and_cleared_back_to_the_profiles() {
    let (store, _dir) = test_store().await;
    let planner = seed_pinned_profile(&store, "planner-edit", Role::Planner, None, None).await;
    let engineer = seed_pinned_profile(
        &store,
        "engineer-edit",
        Role::Engineer,
        Some(AgentKind::ClaudeCode),
        Some("claude-opus-5"),
    )
    .await;
    let reviewer = seed_pinned_profile(
        &store,
        "reviewer-edit",
        Role::Reviewer,
        Some(AgentKind::ClaudeCode),
        Some("claude-opus-5"),
    )
    .await;
    let (goal, repo) = seed_goal(&store, &planner, None).await;
    let task = store
        .create_task(NewTask {
            goal_id: goal.id.clone(),
            repo_id: repo.id.clone(),
            title: "task".into(),
            description: "do things".into(),
            engineer_profile_id: engineer.id.clone(),
            pin: None,
            reviewers: vec![ReviewerSlot::of(&reviewer.id)],
            depends_on: vec![],
        })
        .await
        .unwrap();

    let moved = store
        .update_task(
            &task.id,
            TaskUpdate {
                pin: Some(Some(AgentPin {
                    agent_kind: AgentKind::Codex,
                    model: Some("gpt-5.3-codex".into()),
                    effort: None,
                })),
                reviewers: Some(vec![ReviewerSlot {
                    profile_id: reviewer.id.clone(),
                    pin: Some(AgentPin {
                        agent_kind: AgentKind::Codex,
                        model: Some("o3".into()),
                        effort: None,
                    }),
                }]),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(moved.agent_kind(), Some(AgentKind::Codex));
    assert_eq!(moved.model.as_deref(), Some("gpt-5.3-codex"));
    let pins = store.list_task_reviewer_pins(&task.id).await.unwrap();
    assert_eq!(pins[0].agent_kind(), Some(AgentKind::Codex));
    assert_eq!(pins[0].model.as_deref(), Some("o3"));

    // An edit that says nothing about the model leaves the choice standing.
    let renamed = store
        .update_task(
            &task.id,
            TaskUpdate {
                title: Some("renamed".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(renamed.title, "renamed");
    assert_eq!(renamed.model.as_deref(), Some("gpt-5.3-codex"));

    // Cleared, the task is back on the profile — the profile as it is now.
    store
        .update_profile(
            &engineer.id,
            ProfileUpdate {
                agent_kind: Some(Some(AgentKind::Opencode)),
                model: Some(Some("ollama/llama3:8b".into())),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let cleared = store
        .update_task(
            &task.id,
            TaskUpdate {
                pin: Some(None),
                reviewers: Some(vec![ReviewerSlot::of(&reviewer.id)]),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(cleared.agent_kind(), Some(AgentKind::Opencode));
    assert_eq!(cleared.model.as_deref(), Some("ollama/llama3:8b"));
    let pins = store.list_task_reviewer_pins(&task.id).await.unwrap();
    assert_eq!(pins[0].agent_kind(), Some(AgentKind::ClaudeCode));
    assert_eq!(pins[0].model.as_deref(), Some("claude-opus-5"));
}

/// A profile with an agent, a model and an effort of its own, for the effort
/// tests: what a goal, a task and a slot pin off it.
async fn seed_profile_at(
    store: &Store,
    name: &str,
    role: Role,
    agent_kind: Option<AgentKind>,
    model: Option<&str>,
    effort: Option<&str>,
) -> Profile {
    store
        .create_profile(NewProfile {
            name: name.into(),
            role,
            agent_kind,
            model: model.map(str::to_string),
            effort: effort.map(str::to_string),
            system_prompt: Some(format!("You are {name}.")),
        })
        .await
        .unwrap()
}

/// A profile keeps the effort it was created at, an edit moves it, and
/// clearing it puts the profile back on whatever the CLI runs its model at —
/// the same three things `model` does, in its own column.
#[tokio::test]
async fn a_profile_keeps_moves_and_clears_the_effort_it_runs_at() {
    let (store, _dir) = test_store().await;
    let profile = seed_profile_at(
        &store,
        "engineer-effort",
        Role::Engineer,
        Some(AgentKind::ClaudeCode),
        Some("claude-opus-5"),
        Some("xhigh"),
    )
    .await;
    assert_eq!(profile.effort.as_deref(), Some("xhigh"));
    let read = store.get_profile(&profile.id).await.unwrap();
    assert_eq!(read.effort.as_deref(), Some("xhigh"), "and it round-trips");

    // An edit about something else leaves it exactly where it was.
    let renamed = store
        .update_profile(
            &profile.id,
            ProfileUpdate {
                name: Some("renamed".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(renamed.effort.as_deref(), Some("xhigh"));

    let moved = store
        .update_profile(
            &profile.id,
            ProfileUpdate {
                effort: Some(Some("max".into())),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(moved.effort.as_deref(), Some("max"));

    let cleared = store
        .update_profile(
            &profile.id,
            ProfileUpdate {
                effort: Some(None),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(cleared.effort, None);
    assert_eq!(
        cleared.model.as_deref(),
        Some("claude-opus-5"),
        "clearing the effort leaves the model it was run at alone"
    );
}

/// The effort is pinned everywhere the model is: off the profile behind a
/// goal, a task and every reviewer slot at creation, moved and handed back by
/// an edit, and copied onto the session the launcher opens.
#[tokio::test]
async fn creation_pins_the_effort_beside_the_model() {
    let (store, _dir) = test_store().await;
    let planner = seed_profile_at(
        &store,
        "planner-effort",
        Role::Planner,
        Some(AgentKind::ClaudeCode),
        Some("claude-opus-5"),
        Some("high"),
    )
    .await;
    let engineer = seed_profile_at(
        &store,
        "engineer-effort",
        Role::Engineer,
        Some(AgentKind::Codex),
        Some("gpt-5.6-sol"),
        Some("ultra"),
    )
    .await;
    let inherits = seed_profile_at(
        &store,
        "reviewer-inherits",
        Role::Reviewer,
        Some(AgentKind::ClaudeCode),
        Some("claude-sonnet-5"),
        Some("low"),
    )
    .await;
    let chosen = seed_profile_at(
        &store,
        "reviewer-chosen",
        Role::Reviewer,
        Some(AgentKind::ClaudeCode),
        Some("claude-sonnet-5"),
        Some("low"),
    )
    .await;

    let (goal, repo) = seed_goal(&store, &planner, None).await;
    assert_eq!(goal.effort.as_deref(), Some("high"));

    let task = store
        .create_task(NewTask {
            goal_id: goal.id.clone(),
            repo_id: repo.id.clone(),
            title: "task".into(),
            description: "do things".into(),
            engineer_profile_id: engineer.id.clone(),
            pin: None,
            reviewers: vec![
                ReviewerSlot::of(&inherits.id),
                ReviewerSlot {
                    profile_id: chosen.id.clone(),
                    pin: Some(AgentPin {
                        agent_kind: AgentKind::Codex,
                        model: Some("gpt-5.6-luna".into()),
                        effort: Some("max".into()),
                    }),
                },
            ],
            depends_on: vec![],
        })
        .await
        .unwrap();
    assert_eq!(task.effort.as_deref(), Some("ultra"));
    let pins = store.list_task_reviewer_pins(&task.id).await.unwrap();
    assert_eq!(pins[0].effort.as_deref(), Some("low"), "the profile's own");
    assert_eq!(pins[1].model.as_deref(), Some("gpt-5.6-luna"));
    assert_eq!(pins[1].effort.as_deref(), Some("max"), "the slot's own");

    // An edit moves the pair, and handing it back hands back the profile's.
    let moved = store
        .update_task(
            &task.id,
            TaskUpdate {
                pin: Some(Some(AgentPin {
                    agent_kind: AgentKind::ClaudeCode,
                    model: Some("claude-opus-5".into()),
                    effort: Some("xhigh".into()),
                })),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(moved.model.as_deref(), Some("claude-opus-5"));
    assert_eq!(moved.effort.as_deref(), Some("xhigh"));

    let back = store
        .update_task(
            &task.id,
            TaskUpdate {
                pin: Some(None),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(back.model.as_deref(), Some("gpt-5.6-sol"));
    assert_eq!(back.effort.as_deref(), Some("ultra"));

    // An effort of its own moves alone, and clears alone: the model the task
    // is pinned to is what it is run at, and it stays where it is.
    let raised = store
        .update_task(
            &task.id,
            TaskUpdate {
                effort: Some(Some("low".into())),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(raised.model.as_deref(), Some("gpt-5.6-sol"));
    assert_eq!(raised.effort.as_deref(), Some("low"));
    let dropped = store
        .update_task(
            &task.id,
            TaskUpdate {
                effort: Some(None),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(dropped.model.as_deref(), Some("gpt-5.6-sol"));
    assert_eq!(dropped.effort, None);

    // A pin that moves carries its own effort, and the field beside it is not
    // read: one edit says one thing about what the task runs at.
    let both = store
        .update_task(
            &task.id,
            TaskUpdate {
                pin: Some(Some(AgentPin {
                    agent_kind: AgentKind::Codex,
                    model: Some("gpt-5.6-luna".into()),
                    effort: Some("max".into()),
                })),
                effort: Some(Some("low".into())),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(both.model.as_deref(), Some("gpt-5.6-luna"));
    assert_eq!(both.effort.as_deref(), Some("max"));

    // Back where the session below expects it.
    let back = store
        .update_task(
            &task.id,
            TaskUpdate {
                pin: Some(None),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    // And the session the launcher opens carries what the task was pinned to.
    let session = store
        .create_session(NewSession {
            goal_id: goal.id.clone(),
            task_id: Some(task.id.clone()),
            role: Role::Engineer,
            profile_id: engineer.id.clone(),
            agent_kind: AgentKind::Codex,
            model: back.model.clone(),
            effort: back.effort.clone(),
            tmux_session: "ariadne-effort".into(),
            worktree_path: None,
            review_round: None,
        })
        .await
        .unwrap();
    assert_eq!(session.effort.as_deref(), Some("ultra"));
    let read = store.get_session(&session.id).await.unwrap();
    assert_eq!(read.effort.as_deref(), Some("ultra"), "and it round-trips");
}

/// The rule an override is resolved by: the model is the override's, and the
/// effort is the override's or else the profile's — but the profile's only
/// where the model is the profile's too, since the effort it was on may not
/// exist on the model the row was moved to.
#[tokio::test]
async fn an_override_takes_the_profiles_effort_only_on_the_profiles_model() {
    let (store, _dir) = test_store().await;
    let planner = seed_profile_at(&store, "planner-rule", Role::Planner, None, None, None).await;
    let (goal, repo) = seed_goal(&store, &planner, None).await;

    let sol = || Some("gpt-5.6-sol".to_string());
    // Each case is what it is called, the override it writes, and the model
    // and effort the row must come out on.
    let cases = vec![
        (
            "an override naming both takes both",
            Some(AgentPin {
                agent_kind: AgentKind::Codex,
                model: Some("gpt-5.6-luna".into()),
                effort: Some("max".into()),
            }),
            Some("gpt-5.6-luna"),
            Some("max"),
        ),
        (
            "an override on the profile's own model keeps its effort",
            Some(AgentPin {
                agent_kind: AgentKind::Codex,
                model: sol(),
                effort: None,
            }),
            Some("gpt-5.6-sol"),
            Some("ultra"),
        ),
        (
            "an override onto another model runs at the CLI's own default",
            Some(AgentPin {
                agent_kind: AgentKind::Codex,
                model: Some("gpt-5.6-luna".into()),
                effort: None,
            }),
            Some("gpt-5.6-luna"),
            None,
        ),
        (
            "and another CLI's model of the same name is another model",
            Some(AgentPin {
                agent_kind: AgentKind::ClaudeCode,
                model: sol(),
                effort: None,
            }),
            Some("gpt-5.6-sol"),
            None,
        ),
        (
            "no override at all is the profile's, effort and all",
            None,
            Some("gpt-5.6-sol"),
            Some("ultra"),
        ),
    ];

    for (n, (case, pin, model, effort)) in cases.into_iter().enumerate() {
        // A profile apiece: a slot is one row per profile, so every case needs
        // its own pair to pin off.
        let engineer = seed_profile_at(
            &store,
            &format!("engineer-rule-{n}"),
            Role::Engineer,
            Some(AgentKind::Codex),
            Some("gpt-5.6-sol"),
            Some("ultra"),
        )
        .await;
        let reviewer = seed_profile_at(
            &store,
            &format!("reviewer-rule-{n}"),
            Role::Reviewer,
            Some(AgentKind::Codex),
            Some("gpt-5.6-sol"),
            Some("ultra"),
        )
        .await;
        let task = store
            .create_task(NewTask {
                goal_id: goal.id.clone(),
                repo_id: repo.id.clone(),
                title: case.into(),
                description: "do things".into(),
                engineer_profile_id: engineer.id.clone(),
                pin: pin.clone(),
                reviewers: vec![ReviewerSlot {
                    profile_id: reviewer.id.clone(),
                    pin: pin.clone(),
                }],
                depends_on: vec![],
            })
            .await
            .unwrap();
        assert_eq!(task.model.as_deref(), model, "{case}");
        assert_eq!(task.effort.as_deref(), effort, "{case}");
        let pins = store.list_task_reviewer_pins(&task.id).await.unwrap();
        assert_eq!(pins[0].model.as_deref(), model, "the slot too: {case}");
        assert_eq!(pins[0].effort.as_deref(), effort, "the slot too: {case}");
    }

    // A profile on no effort of its own has none to hand down, whatever the
    // override leaves open.
    let plain = seed_profile_at(
        &store,
        "engineer-rule-plain",
        Role::Engineer,
        Some(AgentKind::Codex),
        Some("gpt-5.6-sol"),
        None,
    )
    .await;
    let reviewer = seed_profile_at(
        &store,
        "reviewer-rule-plain",
        Role::Reviewer,
        Some(AgentKind::Codex),
        Some("gpt-5.6-sol"),
        None,
    )
    .await;
    let task = store
        .create_task(NewTask {
            goal_id: goal.id.clone(),
            repo_id: repo.id.clone(),
            title: "no effort to inherit".into(),
            description: "do things".into(),
            engineer_profile_id: plain.id.clone(),
            pin: Some(AgentPin {
                agent_kind: AgentKind::Codex,
                model: sol(),
                effort: None,
            }),
            reviewers: vec![ReviewerSlot::of(&reviewer.id)],
            depends_on: vec![],
        })
        .await
        .unwrap();
    assert_eq!(task.model.as_deref(), Some("gpt-5.6-sol"));
    assert_eq!(task.effort, None);
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
    let session = w.engineer_session().await;
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

/// What a task spent, by the profile that spent it: its engineer once, and
/// each reviewer with every round it sat summed into one entry — a reviewer
/// runs a session per round, and nobody reads them round by round.
#[tokio::test]
async fn a_tasks_usage_groups_every_round_of_a_reviewer_together() {
    let w = World::new().await;
    let engineer = w.engineer_session().await;
    let reviewer_id = w.store.list_task_reviewers(&w.task.id).await.unwrap()[0].clone();
    let first_round = w
        .session(
            "rev-round-1",
            Role::Reviewer,
            &reviewer_id,
            Some(&w.task.id),
        )
        .await;
    let second_round = w
        .session(
            "rev-round-2",
            Role::Reviewer,
            &reviewer_id,
            Some(&w.task.id),
        )
        .await;

    for (session, spent) in [
        (&engineer, usage(100, 80, 10)),
        (&first_round, usage(20, 10, 4)),
        (&second_round, usage(5, 1, 2)),
    ] {
        w.store
            .upsert_session_usage(&session.id, "/x.jsonl", spent)
            .await
            .unwrap();
    }

    let grouped = w.store.task_usage(&w.task.id).await.unwrap();
    assert_eq!(
        grouped,
        vec![
            ProfileUsage {
                role: Role::Engineer,
                profile_id: w.task.engineer_profile_id.clone(),
                usage: usage(100, 80, 10),
            },
            ProfileUsage {
                role: Role::Reviewer,
                profile_id: reviewer_id,
                usage: usage(25, 11, 6),
            },
        ]
    );
}

/// A session that has reported nothing is still one of the task's: it reads
/// as zeros rather than dropping out, so the reviewer nobody has spent
/// anything on is still listed.
#[tokio::test]
async fn a_session_that_has_reported_nothing_reads_as_zeros() {
    let w = World::new().await;
    let _engineer = w.engineer_session().await;
    let grouped = w.store.task_usage(&w.task.id).await.unwrap();
    assert_eq!(
        grouped,
        vec![ProfileUsage {
            role: Role::Engineer,
            profile_id: w.task.engineer_profile_id.clone(),
            usage: TokenUsage::default(),
        }]
    );
}

/// A goal's usage is grouped by role rather than by profile, and its planner
/// counts: a planner session belongs to no task, so nothing under a task
/// would ever have found it.
#[tokio::test]
async fn a_goals_usage_is_grouped_by_role_and_counts_its_planner() {
    let w = World::new().await;
    let planner = w.session("plan", Role::Planner, &w.planner.id, None).await;
    let engineer = w.engineer_session().await;
    let reviewer_id = w.store.list_task_reviewers(&w.task.id).await.unwrap()[0].clone();
    let reviewer = w
        .session("rev", Role::Reviewer, &reviewer_id, Some(&w.task.id))
        .await;

    for (session, spent) in [
        (&planner, usage(40, 30, 8)),
        (&engineer, usage(100, 80, 10)),
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
            RoleUsage {
                role: Role::Engineer,
                usage: usage(100, 80, 10),
            },
            RoleUsage {
                role: Role::Planner,
                usage: usage(40, 30, 8),
            },
            RoleUsage {
                role: Role::Reviewer,
                usage: usage(20, 10, 4),
            },
        ]
    );
    assert_eq!(
        grouped.iter().map(|r| r.usage).sum::<TokenUsage>(),
        usage(160, 120, 22),
        "the goal's total is every session under it, the planner included"
    );
}

/// Usage belongs to the session that spent it: deleting the goal takes the
/// sessions with it, and the rows keyed on them go too rather than outliving
/// the id that names them.
#[tokio::test]
async fn usage_goes_when_the_session_it_belonged_to_does() {
    let w = World::new().await;
    let session = w.engineer_session().await;
    w.store
        .upsert_session_usage(&session.id, "/x.jsonl", usage(100, 80, 10))
        .await
        .unwrap();
    assert_eq!(usage_rows(&w._dir).await, 1);

    w.store.delete_goal(&w.goal.id).await.unwrap();
    assert_eq!(usage_rows(&w._dir).await, 0);
}
