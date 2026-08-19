//! Store integration tests against a temp-file SQLite database.

use ariadne_core::{
    Actor, AgentKind, AuthorRole, GoalStatus, PromptKind, ReviewVerdict, Role, SessionStatus,
    TaskStatus,
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
            system_prompt: format!("You are {name}."),
            prompts: vec![],
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
        })
        .await
        .unwrap();
    (goal, repo)
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
            reviewer_profile_ids: vec![rev.id],
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
    assert_eq!(
        store
            .get_agent_config(AgentKind::ClaudeCode)
            .await
            .unwrap()
            .extra_flags(),
        vec!["--dangerously-skip-permissions".to_string()]
    );
    assert_eq!(
        store
            .get_agent_config(AgentKind::Codex)
            .await
            .unwrap()
            .extra_flags(),
        vec!["--dangerously-bypass-approvals-and-sandbox".to_string()]
    );
    assert!(
        store
            .get_agent_config(AgentKind::Opencode)
            .await
            .unwrap()
            .extra_flags()
            .is_empty()
    );
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
    // Re-opening the same database reads back the edit, not the seed.
    assert_eq!(
        store
            .get_agent_config(AgentKind::ClaudeCode)
            .await
            .unwrap()
            .extra_flags(),
        vec!["--permission-mode=acceptEdits".to_string()]
    );

    let emptied = store
        .update_agent_config(AgentKind::Codex, vec![])
        .await
        .unwrap();
    assert!(emptied.extra_flags().is_empty());
    // One agent's flags are its own.
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
            system_prompt: "x".into(),
            prompts: vec![],
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
        })
        .await
        .unwrap();
    assert_eq!(repo.path, "/tmp/repo");
    assert_eq!(repo.description.as_deref(), Some("the one repo"));

    // The same checkout on another branch is a different repository.
    let other = store
        .create_repository(NewRepository {
            path: "/tmp/repo".into(),
            base_branch: "next".into(),
            description: None,
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
                reviewer_profile_ids: vec![rev.id],
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
    let (store, _dir) = test_store().await;
    let planner = seed_profile(&store, "planner", Role::Planner).await;
    let (goal, repo) = seed_goal(&store, &planner, None).await;
    seed_task(&store, &goal, &repo, vec![]).await;

    let err = store.delete_repository(&repo.id).await.unwrap_err();
    let StoreError::Conflict(message) = err else {
        panic!("expected a conflict, got {err:?}");
    };
    assert!(message.contains("1 goal"), "{message}");
    assert!(message.contains("1 task"), "{message}");

    // Nothing holds it once the goal (and with it the task) is gone.
    store.delete_goal(&goal.id).await.unwrap();
    store.delete_repository(&repo.id).await.unwrap();
}

#[tokio::test]
async fn task_happy_path_to_merged() {
    let (store, _dir) = test_store().await;
    let planner = seed_profile(&store, "planner", Role::Planner).await;
    let (goal, repo) = seed_goal(&store, &planner, None).await;
    let task = seed_task(&store, &goal, &repo, vec![]).await;
    assert_eq!(task.status(), TaskStatus::Pending);
    assert_eq!(task.branch, format!("ariadne/task-{}", task.id));

    let t = store
        .transition_task(&task.id, TaskStatus::Ready, Actor::Daemon, None, None)
        .await
        .unwrap();
    let t = store
        .transition_task(&t.id, TaskStatus::InProgress, Actor::Daemon, None, None)
        .await
        .unwrap();
    let t = store
        .transition_task(
            &t.id,
            TaskStatus::UnderReview,
            Actor::Engineer,
            Some("review please"),
            None,
        )
        .await
        .unwrap();
    assert_eq!(t.review_round, 1, "review round bumps on under_review");
    let t = store
        .transition_task(&t.id, TaskStatus::Approved, Actor::Daemon, None, None)
        .await
        .unwrap();
    let t = store
        .transition_task(&t.id, TaskStatus::Merging, Actor::Daemon, None, None)
        .await
        .unwrap();
    let t = store
        .transition_task(
            &t.id,
            TaskStatus::Merged,
            Actor::Engineer,
            None,
            Some("abc123"),
        )
        .await
        .unwrap();
    assert_eq!(t.status(), TaskStatus::Merged);
    assert_eq!(t.merge_commit.as_deref(), Some("abc123"));

    let audit = store.list_task_transitions(&t.id).await.unwrap();
    assert_eq!(audit.len(), 6);
    assert_eq!(audit[0].from_status, "pending");
    assert_eq!(audit[5].to_status, "merged");
}

#[tokio::test]
async fn illegal_transitions_are_rejected_and_unaudited() {
    let (store, _dir) = test_store().await;
    let planner = seed_profile(&store, "planner", Role::Planner).await;
    let (goal, repo) = seed_goal(&store, &planner, None).await;
    let task = seed_task(&store, &goal, &repo, vec![]).await;

    // Illegal edge.
    assert!(matches!(
        store
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
        store
            .transition_task(&task.id, TaskStatus::Ready, Actor::Reviewer, None, None)
            .await,
        Err(StoreError::Transition(_))
    ));
    // Merged requires a commit.
    let t = store
        .transition_task(&task.id, TaskStatus::Ready, Actor::Daemon, None, None)
        .await
        .unwrap();
    let t = store
        .transition_task(&t.id, TaskStatus::InProgress, Actor::Daemon, None, None)
        .await
        .unwrap();
    let t = store
        .transition_task(&t.id, TaskStatus::UnderReview, Actor::Engineer, None, None)
        .await
        .unwrap();
    let t = store
        .transition_task(&t.id, TaskStatus::Approved, Actor::Daemon, None, None)
        .await
        .unwrap();
    let t = store
        .transition_task(&t.id, TaskStatus::Merging, Actor::Daemon, None, None)
        .await
        .unwrap();
    assert!(matches!(
        store
            .transition_task(&t.id, TaskStatus::Merged, Actor::Engineer, None, None)
            .await,
        Err(StoreError::Invalid(_))
    ));

    let audit = store.list_task_transitions(&task.id).await.unwrap();
    assert_eq!(audit.len(), 5, "failed transitions leave no audit rows");
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
            reviewer_profile_ids: vec![rev.id],
            depends_on: vec![],
        })
        .await;
    assert!(matches!(t2, Err(StoreError::Conflict(_))));
}

#[tokio::test]
async fn dependencies_gate_and_reject_cycles() {
    let (store, _dir) = test_store().await;
    let planner = seed_profile(&store, "planner", Role::Planner).await;
    let (goal, repo) = seed_goal(&store, &planner, None).await;
    let a = seed_task(&store, &goal, &repo, vec![]).await;
    let b = seed_task(&store, &goal, &repo, vec![a.id.clone()]).await;

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
    let t = store
        .transition_task(&a.id, TaskStatus::Ready, Actor::Daemon, None, None)
        .await
        .unwrap();
    let t = store
        .transition_task(&t.id, TaskStatus::InProgress, Actor::Daemon, None, None)
        .await
        .unwrap();
    let t = store
        .transition_task(&t.id, TaskStatus::UnderReview, Actor::Engineer, None, None)
        .await
        .unwrap();
    let t = store
        .transition_task(&t.id, TaskStatus::Approved, Actor::Daemon, None, None)
        .await
        .unwrap();
    let t = store
        .transition_task(&t.id, TaskStatus::Merging, Actor::Daemon, None, None)
        .await
        .unwrap();
    store
        .transition_task(
            &t.id,
            TaskStatus::Merged,
            Actor::Engineer,
            None,
            Some("sha"),
        )
        .await
        .unwrap();
    assert!(store.task_dependencies_merged(&b.id).await.unwrap());
}

#[tokio::test]
async fn set_dependencies_on_ready_task_downgrades_with_audit() {
    let (store, _dir) = test_store().await;
    let planner = seed_profile(&store, "planner", Role::Planner).await;
    let (goal, repo) = seed_goal(&store, &planner, None).await;
    let dep = seed_task(&store, &goal, &repo, vec![]).await;
    let task = seed_task(&store, &goal, &repo, vec![]).await;

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
    let (store, _dir) = test_store().await;
    let planner = seed_profile(&store, "planner", Role::Planner).await;
    let (goal, repo) = seed_goal(&store, &planner, None).await;
    let task = seed_task(&store, &goal, &repo, vec![]).await;
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

#[tokio::test]
async fn messages_sessions_events_round_trip() {
    let (store, _dir) = test_store().await;
    let planner = seed_profile(&store, "planner", Role::Planner).await;
    let (goal, repo) = seed_goal(&store, &planner, None).await;
    let task = seed_task(&store, &goal, &repo, vec![]).await;

    let session = store
        .create_session(NewSession {
            goal_id: goal.id.clone(),
            task_id: Some(task.id.clone()),
            role: Role::Engineer,
            profile_id: task.engineer_profile_id.clone(),
            agent_kind: AgentKind::ClaudeCode,
            tmux_session: "ariadne-test-eng".into(),
            worktree_path: Some("/tmp/wt".into()),
            review_round: None,
        })
        .await
        .unwrap();
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

/// A resumed agent conversation keeps its one session row: restarting puts
/// the row back where a spawn leaves it, so nothing downstream can tell the
/// relaunch from a first launch.
#[tokio::test]
async fn restarting_a_session_reopens_the_same_row() {
    let (store, _dir) = test_store().await;
    let planner = seed_profile(&store, "planner", Role::Planner).await;
    let (goal, repo) = seed_goal(&store, &planner, None).await;
    let task = seed_task(&store, &goal, &repo, vec![]).await;

    let session = store
        .create_session(NewSession {
            goal_id: goal.id.clone(),
            task_id: Some(task.id.clone()),
            role: Role::Engineer,
            profile_id: task.engineer_profile_id.clone(),
            agent_kind: AgentKind::ClaudeCode,
            tmux_session: "ariadne-test-eng".into(),
            worktree_path: Some("/tmp/wt".into()),
            review_round: None,
        })
        .await
        .unwrap();
    store
        .set_session_internal_id(&session.id, "uuid-1234")
        .await
        .unwrap();
    store
        .set_session_status(&session.id, SessionStatus::Exited)
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
    let (store, _dir) = test_store().await;
    let planner = seed_profile(&store, "planner", Role::Planner).await;
    let (goal, repo) = seed_goal(&store, &planner, None).await;
    let task = seed_task(&store, &goal, &repo, vec![]).await;

    store.delete_goal(&goal.id).await.unwrap();
    assert!(matches!(
        store.get_task(&task.id).await,
        Err(StoreError::NotFound { .. })
    ));
}

#[tokio::test]
async fn a_fresh_database_is_seeded_with_the_built_in_profiles_and_their_prompts() {
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
        assert_eq!(
            p.system_prompt,
            default_system_prompt(role),
            "{name} ships the role default system prompt"
        );
        assert!(
            p.system_prompt.contains("## Ariadne orchestration"),
            "{name}'s system prompt folds in the shared playbook"
        );
        // Exactly the role's prompt kinds, each at its default.
        let prompts = store.list_profile_prompts(&p.id).await.unwrap();
        assert_eq!(
            prompts.iter().map(|p| p.kind()).collect::<Vec<_>>(),
            PromptKind::for_role(role),
            "{name} owns the prompts of its role"
        );
        for prompt in &prompts {
            assert_eq!(prompt.content, default_prompt(role, prompt.kind()).unwrap());
        }
    }

    // Fixed, recognizable ids; the reviewer carries the newer persona.
    let reviewer = store.get_profile_by_name("Reviewer").await.unwrap();
    assert_eq!(reviewer.id, "00000000000000000000000003");
    assert!(
        reviewer
            .system_prompt
            .contains("install the project's dependencies"),
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
    assert_eq!(
        store
            .get_profile_by_name("Engineer")
            .await
            .unwrap()
            .system_prompt,
        "custom"
    );
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
async fn a_new_profile_starts_from_the_role_defaults() {
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
    // Its system prompt is the one it was created with, not the role default.
    assert_eq!(reviewer.system_prompt, "You are rev-strict.");
}

#[tokio::test]
async fn prompts_update_and_reset_to_their_defaults() {
    let (store, _dir) = test_store().await;
    let engineer = store.get_profile_by_name("Engineer").await.unwrap();

    let updated = store
        .update_profile_prompt(&engineer.id, PromptKind::MergeInstructions, "just merge it")
        .await
        .unwrap();
    assert_eq!(updated.content, "just merge it");
    assert_eq!(updated.kind(), PromptKind::MergeInstructions);
    assert_eq!(
        store
            .get_profile_prompt(&engineer.id, PromptKind::MergeInstructions)
            .await
            .unwrap()
            .content,
        "just merge it"
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
        .reset_profile_prompt(&engineer.id, PromptKind::MergeInstructions)
        .await
        .unwrap();
    assert_eq!(
        reset.content,
        default_prompt(Role::Engineer, PromptKind::MergeInstructions).unwrap()
    );

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
    assert_eq!(
        restored.system_prompt,
        default_system_prompt(Role::Engineer)
    );
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
        3
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
        1
    );

    store.delete_profile(&profile.id).await.unwrap();
    assert!(matches!(
        store.list_profile_prompts(&profile.id).await,
        Err(StoreError::NotFound { .. })
    ));
}

/// A database written before repositories existed: its goals keep the repos
/// they had, its tasks resolve the same checkout and base branch, and the
/// children of the rebuilt `tasks` table survive the rebuild.
///
/// The old shape is produced by running the migrations up to (not including)
/// the one under test, which is the only way to get a real pre-migration
/// database rather than a hand-written imitation of one.
#[tokio::test]
async fn a_pre_repositories_database_migrates_its_goals_and_tasks() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy.db");
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(&path)
        .create_if_missing(true)
        .foreign_keys(true);
    let pool = sqlx::SqlitePool::connect_with(options).await.unwrap();

    let mut migrator = sqlx::migrate::Migrator::new(std::path::Path::new("./migrations"))
        .await
        .unwrap();
    migrator.migrations = migrator
        .migrations
        .iter()
        .filter(|m| m.version < 3)
        .cloned()
        .collect::<Vec<_>>()
        .into();
    migrator.run(&pool).await.unwrap();

    for (id, name, role) in [
        ("legacyplanner", "Legacy planner", "planner"),
        ("legacyengineer", "Legacy engineer", "engineer"),
    ] {
        sqlx::query(
            "INSERT INTO profiles (id, name, role, system_prompt, created_at, updated_at)
             VALUES (?, ?, ?, 'sys', 't', 't')",
        )
        .bind(id)
        .bind(name)
        .bind(role)
        .execute(&pool)
        .await
        .unwrap();
    }
    sqlx::query(
        "INSERT INTO goals (id, title, description, planner_profile_id, created_at, updated_at)
         VALUES ('legacygoal', 'Legacy goal', 'desc', 'legacyplanner', 't', 't')",
    )
    .execute(&pool)
    .await
    .unwrap();
    for (id, path) in [
        ("legacyapi", "/tmp/legacy-api"),
        ("legacyui", "/tmp/legacy-ui"),
    ] {
        sqlx::query("INSERT INTO goal_repos (id, goal_id, path, base_branch) VALUES (?, ?, ?, ?)")
            .bind(id)
            .bind("legacygoal")
            .bind(path)
            .bind("main")
            .execute(&pool)
            .await
            .unwrap();
    }
    // One of them is already registered globally: the migration must reuse
    // that row rather than register the checkout a second time.
    sqlx::query(
        "INSERT INTO repositories (id, path, base_branch, description, created_at, updated_at)
         VALUES ('registeredui', '/tmp/legacy-ui', 'main', 'the ui', 't', 't')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO tasks (id, goal_id, repo_id, title, description, engineer_profile_id,
                            branch, created_at, updated_at)
         VALUES ('legacytask', 'legacygoal', 'legacyui', 'Legacy task', 'd', 'legacyengineer',
                 'ariadne/task-legacytask', 't', 't')",
    )
    .execute(&pool)
    .await
    .unwrap();
    // Children of `tasks`: dropping the table with foreign keys on would
    // cascade these away.
    sqlx::query("INSERT INTO task_reviewers (task_id, profile_id, position) VALUES (?, ?, 0)")
        .bind("legacytask")
        .bind("legacyengineer")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO messages (id, goal_id, task_id, author_role, body, created_at)
         VALUES ('legacymsg', 'legacygoal', 'legacytask', 'engineer', 'hi', 't')",
    )
    .execute(&pool)
    .await
    .unwrap();
    pool.close().await;

    // Opening the store runs the migration under test.
    let store = Store::open(&path).await.unwrap();

    let repos = store.list_goal_repositories("legacygoal").await.unwrap();
    assert_eq!(
        repos
            .iter()
            .map(|r| (r.path.as_str(), r.base_branch.as_str()))
            .collect::<Vec<_>>(),
        vec![("/tmp/legacy-api", "main"), ("/tmp/legacy-ui", "main")]
    );

    let task = store.get_task("legacytask").await.unwrap();
    let repo = store.get_repository(&task.repo_id).await.unwrap();
    assert_eq!(repo.id, "registeredui", "the registered row was reused");
    assert_eq!(repo.path, "/tmp/legacy-ui");
    assert_eq!(repo.base_branch, "main");
    assert_eq!(repo.description.as_deref(), Some("the ui"));

    // The rebuild kept what hung off the task.
    assert_eq!(
        store.list_task_reviewers("legacytask").await.unwrap(),
        vec!["legacyengineer".to_string()]
    );
    assert_eq!(
        store
            .list_task_messages("legacytask", None, 10)
            .await
            .unwrap()
            .len(),
        1
    );

    // A database that predates the agent configs is given them on the way up,
    // with the same defaults a fresh one is seeded with.
    let configs = store.list_agent_configs().await.unwrap();
    assert_eq!(configs.len(), AgentKind::ALL.len());
    for config in configs {
        assert_eq!(config.extra_flags(), config.default_flags());
    }

    // And the goal still holds them, so the repositories cannot be deleted.
    assert!(matches!(
        store.delete_repository("registeredui").await,
        Err(StoreError::Conflict(_))
    ));

    // Foreign keys are back on after the rebuild: deleting the goal still
    // cascades, which is the whole reason the migration turned them off.
    store.delete_goal("legacygoal").await.unwrap();
    assert!(matches!(
        store.get_task("legacytask").await,
        Err(StoreError::NotFound { .. })
    ));
    store.delete_repository("registeredui").await.unwrap();
}
