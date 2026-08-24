//! Store integration tests against a temp-file SQLite database.

use ariadne_core::{
    Actor, AgentKind, AttentionReason, AuthorRole, GoalStatus, PromptKind, ReviewVerdict, Role,
    SessionStatus, TaskStatus,
};
use ariadne_store::defaults::{INTEGRATOR_ID, default_prompt, default_system_prompt};
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
            integrator_profile_id: INTEGRATOR_ID.into(),
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
    assert_eq!(
        store
            .get_agent_config(AgentKind::Opencode)
            .await
            .unwrap()
            .extra_flags(),
        vec!["--auto".to_string()]
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
                integrator_profile_id: INTEGRATOR_ID.into(),
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
        .transition_task(&t.id, TaskStatus::Integrating, Actor::Daemon, None, None)
        .await
        .unwrap();
    let t = store
        .transition_task(
            &t.id,
            TaskStatus::Merged,
            Actor::Integrator,
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
        .transition_task(&t.id, TaskStatus::Integrating, Actor::Daemon, None, None)
        .await
        .unwrap();
    assert!(matches!(
        store
            .transition_task(&t.id, TaskStatus::Merged, Actor::Integrator, None, None)
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
            integrator_profile_id: INTEGRATOR_ID.into(),
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
        .transition_task(&t.id, TaskStatus::Integrating, Actor::Daemon, None, None)
        .await
        .unwrap();
    store
        .transition_task(
            &t.id,
            TaskStatus::Merged,
            Actor::Integrator,
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
            model: None,
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
    let (store, _dir) = test_store().await;
    let planner = seed_profile(&store, "planner", Role::Planner).await;
    let (goal, repo) = seed_goal(&store, &planner, None).await;
    let task = seed_task(&store, &goal, &repo, vec![]).await;
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
    let (store, _dir) = test_store().await;
    let planner = seed_profile(&store, "planner", Role::Planner).await;
    let (goal, _repo) = seed_goal(&store, &planner, None).await;
    // A profile nothing else references, so only the message can hold it.
    let bystander = seed_profile(&store, "bystander", Role::Reviewer).await;

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

/// What a launch is dated for: asking whether *this* run of an agent has
/// reported anything of a given kind since it started. A relaunch moves the
/// date, which is what makes the question about the run rather than the row.
#[tokio::test]
async fn a_launch_is_dated_and_what_followed_it_can_be_asked_for() {
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
            model: None,
            tmux_session: "ariadne-test-eng".into(),
            worktree_path: Some("/tmp/wt".into()),
            review_round: None,
        })
        .await
        .unwrap();
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
    let event = |kind: &str| NewAgentEvent {
        session_id: Some(session.id.clone()),
        task_id: Some(task.id.clone()),
        agent_kind: Some(AgentKind::ClaudeCode),
        kind: kind.into(),
        payload: serde_json::json!({}),
    };
    store.create_event(event("session_start")).await.unwrap();
    store.create_event(event("pre_tool_use")).await.unwrap();

    assert!(
        store
            .session_reported_since(&session.id, &first, &["pre_tool_use", "user_prompt_submit"])
            .await
            .unwrap()
    );
    assert!(
        !store
            .session_reported_since(&session.id, &first, &["user_prompt_submit"])
            .await
            .unwrap(),
        "kinds outside the asked-for set are not an answer"
    );
    assert!(
        !store
            .session_reported_since(&session.id, &first, &[])
            .await
            .unwrap(),
        "and asking for nothing finds nothing"
    );

    // Launched again — a resume — with everything above now behind it.
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    store.mark_session_launched(&session.id).await.unwrap();
    let second = store
        .get_session(&session.id)
        .await
        .unwrap()
        .launched_at
        .expect("the relaunch is dated too");
    assert!(second > first, "every launch moves the date");
    assert!(
        !store
            .session_reported_since(&session.id, &second, &["pre_tool_use"])
            .await
            .unwrap(),
        "what the previous run did says nothing about this one"
    );
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
            model: None,
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
            model: None,
            tmux_session: "ariadne-test-eng".into(),
            worktree_path: Some("/tmp/wt".into()),
            review_round: None,
        })
        .await
        .unwrap();
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

/// A prompt is a dialog on the agent's terminal, so it cannot outlive the
/// session it was raised on: retiring one takes `waiting_permission` /
/// `waiting_input` down with it, and leaves every reason a session ends
/// *carrying* exactly where it is.
#[tokio::test]
async fn retiring_a_session_drops_the_prompt_it_can_no_longer_answer() {
    let (store, _dir) = test_store().await;
    let planner = seed_profile(&store, "planner", Role::Planner).await;
    let (goal, repo) = seed_goal(&store, &planner, None).await;
    let task = seed_task(&store, &goal, &repo, vec![]).await;
    let new_session = || NewSession {
        goal_id: goal.id.clone(),
        task_id: Some(task.id.clone()),
        role: Role::Engineer,
        profile_id: task.engineer_profile_id.clone(),
        agent_kind: AgentKind::ClaudeCode,
        model: None,
        tmux_session: "ariadne-test-eng".into(),
        worktree_path: Some("/tmp/wt".into()),
        review_round: None,
    };

    let session = store.create_session(new_session()).await.unwrap();
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
    let failed = store.create_session(new_session()).await.unwrap();
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
    let (store, _dir) = test_store().await;
    let planner = seed_profile(&store, "planner", Role::Planner).await;
    let (goal, repo) = seed_goal(&store, &planner, None).await;
    let task = seed_task(&store, &goal, &repo, vec![]).await;
    let new_session = || NewSession {
        goal_id: goal.id.clone(),
        task_id: Some(task.id.clone()),
        role: Role::Engineer,
        profile_id: task.engineer_profile_id.clone(),
        agent_kind: AgentKind::ClaudeCode,
        model: None,
        tmux_session: "ariadne-test-eng".into(),
        worktree_path: Some("/tmp/wt".into()),
        review_round: None,
    };

    // The interleaving spelled out: a caller holding a session it read while
    // it was live, and the retirement landing before it gets to the raise.
    let session = store.create_session(new_session()).await.unwrap();
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
        let racing = store.create_session(new_session()).await.unwrap();
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
        ("Integrator", Role::Integrator),
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
            p.system_prompt.contains("`ariadne` MCP tools"),
            "{name}'s system prompt says how to reach the orchestrator"
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
    let integrator = store.get_profile_by_name("Integrator").await.unwrap();
    assert_eq!(integrator.id, "00000000000000000000000004");

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
        .update_profile_prompt(&engineer.id, PromptKind::ChangesRequested, "fix it")
        .await
        .unwrap();
    assert_eq!(updated.content, "fix it");
    assert_eq!(updated.kind(), PromptKind::ChangesRequested);
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

    // The same rule seeds a profile: the row must not survive its bad prompt.
    assert!(matches!(
        store
            .create_profile(NewProfile {
                name: "eng-typo".into(),
                role: Role::Engineer,
                agent_kind: None,
                model: None,
                system_prompt: "You are eng-typo.".into(),
                prompts: vec![(PromptKind::ChangesRequested, "{feedbcak}".into())],
            })
            .await,
        Err(StoreError::Invalid(_))
    ));
    assert!(matches!(
        store.get_profile_by_name("eng-typo").await,
        Err(StoreError::NotFound { .. })
    ));

    // What renders as itself still saves: literal braces, JSON, no
    // placeholders at all.
    let integrator = store.get_profile_by_name("Integrator").await.unwrap();
    for content in [
        "Land {branch} on {base_branch}, then answer {\"merged\": true}.",
        "Do it yourself.",
        "{unclosed and {branch}",
    ] {
        store
            .update_profile_prompt(&integrator.id, PromptKind::IntegrationInstructions, content)
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
        2
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
    let legacy_messages = store
        .list_task_messages("legacytask", None, 10)
        .await
        .unwrap();
    assert_eq!(legacy_messages.len(), 1);
    assert_eq!(
        legacy_messages[0].recipient(),
        None,
        "a message written before recipients existed is addressed to the thread"
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
            system_prompt: format!("You are {name}."),
            prompts: vec![],
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
            integrator_profile_id: INTEGRATOR_ID.into(),
            reviewer_profile_ids: vec![reviewer.id.clone()],
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
            integrator_profile_id: INTEGRATOR_ID.into(),
            reviewer_profile_ids: vec![reviewer.id.clone()],
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
            integrator_profile_id: INTEGRATOR_ID.into(),
            reviewer_profile_ids: vec![first.id.clone()],
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
                reviewer_profile_ids: Some(vec![second.id.clone(), first.id.clone()]),
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

/// A database written before agents and models were pinned: every goal, task
/// and reviewer slot comes up pinned to what it resolves to today, so the
/// upgrade itself changes nothing and only the next profile edit is felt.
#[tokio::test]
async fn a_pre_pinning_database_backfills_from_the_profiles_it_references() {
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
        .filter(|m| m.version < 7)
        .cloned()
        .collect::<Vec<_>>()
        .into();
    migrator.run(&pool).await.unwrap();

    for (id, name, role, agent_kind, model) in [
        (
            "legacyplanner",
            "Legacy planner",
            "planner",
            Some("claude_code"),
            Some("opus"),
        ),
        (
            "legacyengineer",
            "Legacy engineer",
            "engineer",
            Some("codex"),
            Some("gpt-5"),
        ),
        // Auto and CLI-default, which must back-fill as NULL rather than as
        // some resolved-now value.
        ("legacyreviewer", "Legacy reviewer", "reviewer", None, None),
    ] {
        sqlx::query(
            "INSERT INTO profiles (id, name, role, agent_kind, model, system_prompt,
                                   created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, 'sys', 't', 't')",
        )
        .bind(id)
        .bind(name)
        .bind(role)
        .bind(agent_kind)
        .bind(model)
        .execute(&pool)
        .await
        .unwrap();
    }
    sqlx::query(
        "INSERT INTO repositories (id, path, base_branch, created_at, updated_at)
         VALUES ('legacyrepo', '/tmp/legacy-pin', 'main', 't', 't')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO goals (id, title, description, planner_profile_id, created_at, updated_at)
         VALUES ('legacygoal', 'Legacy goal', 'desc', 'legacyplanner', 't', 't')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO goal_repositories (goal_id, repository_id) VALUES (?, ?)")
        .bind("legacygoal")
        .bind("legacyrepo")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO tasks (id, goal_id, repo_id, title, description, engineer_profile_id,
                            branch, created_at, updated_at)
         VALUES ('legacytask', 'legacygoal', 'legacyrepo', 'Legacy task', 'd', 'legacyengineer',
                 'ariadne/task-legacytask', 't', 't')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO task_reviewers (task_id, profile_id, position) VALUES (?, ?, 0)")
        .bind("legacytask")
        .bind("legacyreviewer")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;

    // Opening the store runs the migration under test.
    let store = Store::open(&path).await.unwrap();

    let goal = store.get_goal("legacygoal").await.unwrap();
    assert_eq!(goal.agent_kind(), Some(AgentKind::ClaudeCode));
    assert_eq!(goal.model.as_deref(), Some("opus"));
    let task = store.get_task("legacytask").await.unwrap();
    assert_eq!(task.agent_kind(), Some(AgentKind::Codex));
    assert_eq!(task.model.as_deref(), Some("gpt-5"));
    let pins = store.list_task_reviewer_pins("legacytask").await.unwrap();
    assert_eq!(pins.len(), 1);
    assert_eq!(
        pins[0].agent_kind(),
        None,
        "an auto profile backfills to auto"
    );
    assert_eq!(pins[0].model, None);

    // The backfilled rows are pins like any other: the profiles can move on
    // without them.
    store
        .update_profile(
            "legacyengineer",
            ProfileUpdate {
                agent_kind: Some(Some(AgentKind::Opencode)),
                model: Some(None),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let task = store.get_task("legacytask").await.unwrap();
    assert_eq!(task.agent_kind(), Some(AgentKind::Codex));
    assert_eq!(task.model.as_deref(), Some("gpt-5"));
}

/// A database written before the integrator existed: its `merging` task is
/// `integrating` afterwards — in the row and in its audit trail — the built-in
/// Integrator profile is there to be named, and the rebuilt tables keep what
/// hung off them.
#[tokio::test]
async fn a_pre_integrator_database_renames_merging_and_gains_the_builtin() {
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
        .filter(|m| m.version < 11)
        .cloned()
        .collect::<Vec<_>>()
        .into();
    migrator.run(&pool).await.unwrap();

    for (id, name, role) in [
        ("legacyplanner", "Legacy planner", "planner"),
        ("legacyengineer", "Legacy engineer", "engineer"),
        ("legacyreviewer", "Legacy reviewer", "reviewer"),
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
        "INSERT INTO repositories (id, path, base_branch, created_at, updated_at)
         VALUES ('legacyrepo', '/tmp/legacy-integrator', 'main', 't', 't')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO goals (id, title, description, planner_profile_id, created_at, updated_at)
         VALUES ('legacygoal', 'Legacy goal', 'desc', 'legacyplanner', 't', 't')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO goal_repositories (goal_id, repository_id) VALUES (?, ?)")
        .bind("legacygoal")
        .bind("legacyrepo")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO tasks (id, goal_id, repo_id, title, description, status,
                            engineer_profile_id, branch, created_at, updated_at)
         VALUES ('legacytask', 'legacygoal', 'legacyrepo', 'Legacy task', 'd', 'merging',
                 'legacyengineer', 'ariadne/task-legacytask', 't', 't')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO task_transitions (id, task_id, from_status, to_status, actor, created_at)
         VALUES ('legacytrans', 'legacytask', 'approved', 'merging', 'daemon', 't')",
    )
    .execute(&pool)
    .await
    .unwrap();
    // Children of the rebuilt tables: dropping them with foreign keys on
    // would cascade these away.
    sqlx::query("INSERT INTO task_reviewers (task_id, profile_id, position) VALUES (?, ?, 0)")
        .bind("legacytask")
        .bind("legacyreviewer")
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

    let task = store.get_task("legacytask").await.unwrap();
    assert_eq!(task.status(), TaskStatus::Integrating);
    assert_eq!(
        task.integrator_profile_id, INTEGRATOR_ID,
        "a task created before the column is backfilled with the Integrator"
    );
    let transitions = store.list_task_transitions("legacytask").await.unwrap();
    assert_eq!(transitions.len(), 1);
    assert_eq!(transitions[0].to_status, "integrating");

    // The built-in the seeding path could not reach, because this database
    // already had profiles of its own.
    let integrator = store.get_profile_by_name("Integrator").await.unwrap();
    assert_eq!(integrator.id, INTEGRATOR_ID);
    assert_eq!(integrator.role(), Role::Integrator);
    the_integrator_playbook(&integrator.system_prompt);
    // With the whole of both forge playbooks in the briefings migrations 0013
    // to 0016 left it with, and nothing beside it: the two forge built-ins
    // 0013 and 0014 added were merged back into this one by 0016.
    for kind in PromptKind::for_role(Role::Integrator) {
        assert!(
            !store
                .get_profile_prompt(&integrator.id, *kind)
                .await
                .unwrap()
                .content
                .trim()
                .is_empty(),
            "the {} briefing the migrations wrote",
            kind.as_str()
        );
    }
    assert_eq!(
        store
            .list_profiles(Some(Role::Integrator))
            .await
            .unwrap()
            .len(),
        1
    );

    // The rebuilds kept what hung off the tables they replaced...
    assert_eq!(
        store.list_task_reviewers("legacytask").await.unwrap(),
        vec!["legacyreviewer".to_string()]
    );
    assert_eq!(
        store
            .list_task_messages("legacytask", None, 10)
            .await
            .unwrap()
            .len(),
        1
    );

    // And the column the backfill filled in will not go back to NULL: every
    // task names an integrator, the way every task names an engineer.
    let raw = sqlx::SqlitePool::connect(&format!("sqlite://{}", path.display()))
        .await
        .unwrap();
    assert!(
        sqlx::query("UPDATE tasks SET integrator_profile_id = NULL WHERE id = 'legacytask'")
            .execute(&raw)
            .await
            .is_err(),
        "tasks.integrator_profile_id is NOT NULL"
    );
    raw.close().await;

    // The rebuilt CHECK is the new vocabulary's: `merging` is not a status the
    // table will take back.
    let raw = sqlx::SqlitePool::connect(&format!("sqlite://{}", path.display()))
        .await
        .unwrap();
    assert!(
        sqlx::query("UPDATE tasks SET status = 'merging' WHERE id = 'legacytask'")
            .execute(&raw)
            .await
            .is_err()
    );
    // So is `integrator`, on every role and actor column that gained it.
    for (sql, what) in [
        (
            "INSERT INTO profiles (id, name, role, system_prompt, created_at, updated_at)
             VALUES ('newintegrator', 'Another integrator', 'integrator', 'sys', 't', 't')",
            "profiles.role",
        ),
        (
            "INSERT INTO task_transitions (id, task_id, from_status, to_status, actor, created_at)
             VALUES ('newtrans', 'legacytask', 'integrating', 'merged', 'integrator', 't')",
            "task_transitions.actor",
        ),
        (
            "INSERT INTO messages (id, goal_id, task_id, author_role, body, created_at)
             VALUES ('newmsg', 'legacygoal', 'legacytask', 'integrator', 'landed', 't')",
            "messages.author_role",
        ),
    ] {
        sqlx::query(sql)
            .execute(&raw)
            .await
            .unwrap_or_else(|e| panic!("{what} refused an integrator: {e}"));
    }
    raw.close().await;

    // Foreign keys are back on after the rebuilds, so the goal still cascades.
    store.delete_goal("legacygoal").await.unwrap();
    assert!(matches!(
        store.get_task("legacytask").await,
        Err(StoreError::NotFound { .. })
    ));
}

/// One built-in integrator, and every way of landing a task in the prompts it
/// is seeded with: the pull request, the merge request and the local fallback
/// are one playbook now, and a reset puts that whole playbook back.
#[tokio::test]
async fn the_integrator_is_seeded_with_all_three_ways_of_landing_a_task() {
    let (store, _dir) = test_store().await;

    let integrator = store.get_profile_by_name("Integrator").await.unwrap();
    assert_eq!(integrator.id, INTEGRATOR_ID);
    assert_eq!(integrator.role(), Role::Integrator);
    assert_eq!(
        (integrator.agent_kind(), integrator.model.as_deref()),
        (None, None),
        "on the auto-resolved agent CLI, like every other built-in"
    );
    assert_eq!(
        store
            .list_profiles(Some(Role::Integrator))
            .await
            .unwrap()
            .len(),
        1,
        "the three built-in integrators are one"
    );
    // And the whole seeding is four profiles, one per role.
    assert_eq!(store.list_profiles(None).await.unwrap().len(), 4);
    for both in ["pull request", "merge request"] {
        assert!(
            integrator.system_prompt.contains(both),
            "the playbook does not name the {both}"
        );
    }

    // The whole of the workflow the task asks it to carry, in the briefing it
    // is started with.
    let instructions = store
        .get_profile_prompt(&integrator.id, PromptKind::IntegrationInstructions)
        .await
        .unwrap();
    for step in [
        "gh auth status",
        "gh pr create",
        ".github/PULL_REQUEST_TEMPLATE.md",
        "glab auth status",
        "glab mr create",
        ".gitlab/merge_request_templates/",
        "land the task locally instead",
        "git rebase {base_branch}",
        "git push -u <remote> {branch}",
        "git reset --soft {base_branch}",
        "merge --ff-only {branch}",
        "return_to_engineer",
        "record_pull_request",
        "mark_merged",
    ] {
        assert!(
            instructions.content.contains(step),
            "the integrator briefing has no {step}: {}",
            instructions.content
        );
    }
    assert_eq!(
        instructions.content,
        default_prompt(Role::Integrator, PromptKind::IntegrationInstructions).unwrap(),
        "the role default is what it was seeded with"
    );

    // Edited and reset, both of them come back to what they were seeded with.
    store
        .update_profile_prompt(
            &integrator.id,
            PromptKind::IntegrationResume,
            "Do it however you like.",
        )
        .await
        .unwrap();
    let reset = store
        .reset_profile_prompt(&integrator.id, PromptKind::IntegrationResume)
        .await
        .unwrap();
    for listing in ["gh pr list --head", "glab mr list --source-branch"] {
        assert!(reset.content.contains(listing), "{}", reset.content);
    }
    store
        .update_profile(
            &integrator.id,
            ProfileUpdate {
                system_prompt: Some("You are whatever.".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        store
            .reset_system_prompt(&integrator.id)
            .await
            .unwrap()
            .system_prompt,
        default_system_prompt(Role::Integrator)
    );

    // And it is what a task that names no integrator of its own is landed by.
    assert_eq!(store.builtin_integrator().await.unwrap().id, integrator.id);

    // A profile someone creates for themselves starts from the same role
    // defaults: there is no per-built-in set any more.
    let mine = seed_profile(&store, "my integrator", Role::Integrator).await;
    assert_eq!(
        store
            .get_profile_prompt(&mine.id, PromptKind::IntegrationInstructions)
            .await
            .unwrap()
            .content,
        default_prompt(Role::Integrator, PromptKind::IntegrationInstructions).unwrap()
    );
}

/// The Local Integrator's system prompt as migration 0015 left it: what an
/// install on the previous release holds on `…04`, and the only text migration
/// 0016 rewrites there.
const PREVIOUS_INTEGRATOR_SYSTEM_PROMPT: &str = r##"You are the local integrator of an Ariadne task: you integrate tasks in repositories with no pull-request-capable remote, merging the change into the base branch locally with git alone. Once its reviewers have approved it, the task is yours to land. The engineer that wrote it is done with it, and you are the only agent touching the branch while you have it.

Ariadne coordinates planner, engineer, reviewer and integrator agents over shared goals and tasks; you reach it only through the `ariadne` MCP tools: `post_message` to talk to the engineer, the reviewers, the planner and the user, `list_messages` to read the task's conversation. A message reaches one person in particular when you give `post_message` a `to` — a profile name as your briefing and `get_task` spell them, or "user" to ask the human — and that recipient is woken to read it; with no `to` it waits in the thread for whoever reads it next. Every operation named in backticks here or in your briefings — `get_diff`, `return_to_engineer`, `mark_merged` and the rest — is a tool on that MCP server: invoke it as an MCP tool call, never as a shell command or a message. Work autonomously: do not wait for a human unless a message asks you to. A human may attach to this terminal at any time and type follow-ups.

You work in a git worktree of your own, checked out on the task branch; the briefing names the branch, its base, the repository and the worktree path. The change in it is the engineer's: land it as it stands and write no code of your own — a change that needs work goes back to the engineer instead. The primary checkout is yours to fast-forward, and for nothing else.

1. Read the task, its acceptance criteria and its conversation, so the commit you write says what the change was for; `get_diff` shows what is being landed.
2. Rebase the task branch onto the latest base in your worktree, exactly as the integration instructions you are briefed with say.
3. If the rebase conflicts, do not resolve it: abort it and call the `return_to_engineer` MCP tool with a summary and a concrete list naming the conflicting files and what has to be reconciled. The task goes back to the engineer as a round of requested changes, and you are woken again once the reviewers have approved the revision.
4. Otherwise squash the branch into one commit whose message follows the repository's commit conventions, fast-forward the base branch from the primary checkout, and call the `mark_merged` MCP tool with the real commit sha, which the daemon verifies itself. Report it truthfully.
"##;

/// Its integration instructions, as migration 0012 wrote them.
const PREVIOUS_INTEGRATION_INSTRUCTIONS: &str = r##"# Integrate task: {task_title}

{task_description}

## Context
- Goal: {goal_title}
- Worktree (your cwd): {worktree_path}
- Branch: {branch}
- Base branch: {base_branch} (repo {repo_path})

The reviewers approved this task. Land it on {base_branch}, keeping that branch's history linear — one commit per task, no merge commits:

1. In your worktree, rebase onto the latest base: `git fetch . && git rebase {base_branch}`.
2. If the rebase conflicts, do not resolve it yourself: `git rebase --abort`, then call `return_to_engineer` with a summary and a concrete list naming the conflicting files and what has to be reconciled. That ends your turn — the task goes back to the engineer, and you are woken again once the revision is approved.
3. Squash the branch into a single commit on top of the base: `git reset --soft {base_branch} && git commit -m "<type(scope): summary>" -m "<what changed and why>"`. That squash commit is the only one landing on {base_branch}, so its message must:
   - follow Conventional Commits: a `type(scope): summary` subject line derived from the task — the task title, "{task_title}", is not necessarily one already — and a body explaining what changed and why;
   - carry no `Co-Authored-By`, `Generated with` or any other authorship or tool trailer;
   - leave signing to the repository's git configuration: sign if git is configured to sign, do not pass `--no-gpg-sign` or otherwise disable it, and do not force `-S` either.
4. Fast-forward the base branch from the primary checkout: `git -C {repo_path} merge --ff-only {branch}`. If it refuses because the base moved, go back to step 1.
5. Call `mark_merged` with the resulting commit sha (`git -C {repo_path} rev-parse {base_branch}`)."##;

/// An install that has both forge built-ins, with tasks, sessions and messages
/// naming them: migration 0016 moves every one of those references onto the
/// merged integrator and deletes the two, while an integrator profile the user
/// created and a prompt the user edited on the merged one are left as they are.
#[tokio::test]
async fn the_forge_integrators_are_merged_into_the_one_that_stays() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("forges.db");
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(&path)
        .create_if_missing(true)
        .foreign_keys(true);
    let pool = sqlx::SqlitePool::connect_with(options).await.unwrap();

    // An install that already has profiles is the only one the forge built-ins
    // ever reached: migrations 0013 and 0014 seed them where the table is not
    // empty, so the profiles go in first and the rest of the upgrade follows.
    const GITHUB: &str = "00000000000000000000000005";
    const GITLAB: &str = "00000000000000000000000006";
    let migrate_below = async |version: i64, pool: &sqlx::SqlitePool| {
        let mut migrator = sqlx::migrate::Migrator::new(std::path::Path::new("./migrations"))
            .await
            .unwrap();
        migrator.migrations = migrator
            .migrations
            .iter()
            .filter(|m| m.version < version)
            .cloned()
            .collect::<Vec<_>>()
            .into();
        migrator.run(pool).await.unwrap();
    };
    migrate_below(13, &pool).await;

    for (id, name, role) in [
        ("seededplanner", "Planner", "planner"),
        ("seededengineer", "Engineer", "engineer"),
        ("seededreviewer", "Reviewer", "reviewer"),
        ("mineintegrator", "My Integrator", "integrator"),
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
    // The one that stays, holding the defaults its release seeded it with —
    // and a briefing its user rewrote.
    sqlx::query(
        "INSERT INTO profiles (id, name, role, system_prompt, created_at, updated_at)
         VALUES (?, 'Local Integrator', 'integrator', ?, 't', 't')",
    )
    .bind(INTEGRATOR_ID)
    .bind(PREVIOUS_INTEGRATOR_SYSTEM_PROMPT)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO profile_prompts (profile_id, kind, content, updated_at)
         VALUES (?, 'integration_instructions', ?, 't'),
                (?, 'integration_resume', 'Land it however you like.', 't')",
    )
    .bind(INTEGRATOR_ID)
    .bind(PREVIOUS_INTEGRATION_INSTRUCTIONS)
    .bind(INTEGRATOR_ID)
    .execute(&pool)
    .await
    .unwrap();

    // The rest of the previous release: the two forge built-ins, and the
    // per-task integrator column that names them.
    migrate_below(16, &pool).await;

    sqlx::query(
        "INSERT INTO repositories (id, path, base_branch, created_at, updated_at)
         VALUES ('forgerepo', '/tmp/forge-merge', 'main', 't', 't')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO goals (id, title, description, planner_profile_id, created_at, updated_at)
         VALUES ('forgegoal', 'Forge goal', 'desc', 'seededplanner', 't', 't')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO goal_repositories (goal_id, repository_id) VALUES (?, ?)")
        .bind("forgegoal")
        .bind("forgerepo")
        .execute(&pool)
        .await
        .unwrap();
    for (task, integrator) in [("ghtask", GITHUB), ("gltask", GITLAB)] {
        sqlx::query(
            "INSERT INTO tasks (id, goal_id, repo_id, title, description, status,
                                engineer_profile_id, integrator_profile_id, branch,
                                created_at, updated_at)
             VALUES (?, 'forgegoal', 'forgerepo', 'Forge task', 'd', 'integrating',
                     'seededengineer', ?, ?, 't', 't')",
        )
        .bind(task)
        .bind(integrator)
        .bind(format!("ariadne/task-{task}"))
        .execute(&pool)
        .await
        .unwrap();
    }
    sqlx::query(
        "INSERT INTO agent_sessions (id, goal_id, task_id, role, profile_id, agent_kind,
                                     tmux_session, status, created_at)
         VALUES ('ghsession', 'forgegoal', 'ghtask', 'integrator', ?, 'claude_code',
                 'ariadne-ghsession', 'idle', 't')",
    )
    .bind(GITHUB)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO messages (id, goal_id, task_id, author_role, recipient_kind,
                               recipient_profile_id, body, created_at)
         VALUES ('glmsg', 'forgegoal', 'gltask', 'engineer', 'profile', ?,
                 'the mr is stale', 't')",
    )
    .bind(GITLAB)
    .execute(&pool)
    .await
    .unwrap();
    pool.close().await;

    // Opening the store runs the migration under test.
    let store = Store::open(&path).await.unwrap();

    // The two forge built-ins are gone, unconditionally, and their prompt rows
    // with them.
    for id in [GITHUB, GITLAB] {
        assert!(matches!(
            store.get_profile(id).await,
            Err(StoreError::NotFound { .. })
        ));
    }
    let raw = sqlx::SqlitePool::connect(&format!("sqlite://{}", path.display()))
        .await
        .unwrap();
    let orphans: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM profile_prompts WHERE profile_id IN (?, ?)")
            .bind(GITHUB)
            .bind(GITLAB)
            .fetch_one(&raw)
            .await
            .unwrap();
    raw.close().await;
    assert_eq!(orphans, 0, "the prompt rows went with the profiles");

    // Everything that named them names the one that stays.
    for task in ["ghtask", "gltask"] {
        assert_eq!(
            store.get_task(task).await.unwrap().integrator_profile_id,
            INTEGRATOR_ID
        );
    }
    assert_eq!(
        store.get_session("ghsession").await.unwrap().profile_id,
        INTEGRATOR_ID
    );
    assert_eq!(
        store
            .list_task_messages("gltask", None, 10)
            .await
            .unwrap()
            .first()
            .unwrap()
            .recipient_profile_id
            .as_deref(),
        Some(INTEGRATOR_ID)
    );

    // Renamed, and given the playbook that covers all three ways of landing —
    // but only where the row still held the default it was seeded with.
    let merged = store.get_profile(INTEGRATOR_ID).await.unwrap();
    assert_eq!(merged.name, "Integrator");
    the_integrator_playbook(&merged.system_prompt);
    assert!(
        store
            .get_profile_prompt(INTEGRATOR_ID, PromptKind::IntegrationInstructions)
            .await
            .unwrap()
            .content
            .contains("gh pr create"),
        "and the briefing that lands a task"
    );
    assert_eq!(
        store
            .get_profile_prompt(INTEGRATOR_ID, PromptKind::IntegrationResume)
            .await
            .unwrap()
            .content,
        "Land it however you like.",
        "the briefing its user rewrote survives the upgrade"
    );

    // And the integrator the user made is untouched, beside it.
    let mine = store.get_profile("mineintegrator").await.unwrap();
    assert_eq!(mine.name, "My Integrator");
    assert_eq!(mine.system_prompt, "sys");
    assert_eq!(
        store
            .list_profiles(Some(Role::Integrator))
            .await
            .unwrap()
            .len(),
        2
    );
}

/// What a migration wrote is the wording of its own release: the built-in
/// prompts have been rewritten since, and reseeding a database that already
/// exists is its own migration. So a migrated row is read for the playbook it
/// carries — all three ways of landing a task, which is what the migrations
/// were moving — rather than for byte equality with the default a fresh
/// seeding writes today.
fn the_integrator_playbook(system_prompt: &str) {
    for landing in ["github.com remote", "GitLab remote", "git alone"] {
        assert!(
            system_prompt.contains(landing),
            "the migrated playbook has no {landing}: {system_prompt}"
        );
    }
}

/// A database on the release before the merge, with both forge built-ins, one
/// task landed by the GitHub one, and `…04` under whatever name this install
/// gave it — or missing altogether, where the install deleted it — plus any
/// integrator profiles the caller wants beside them. Everything short of
/// migration 0016 itself, which runs when the store is opened.
async fn a_pre_merge_install(
    path: &std::path::Path,
    integrator_name: Option<&str>,
    extra: &[&str],
) {
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .foreign_keys(true);
    let pool = sqlx::SqlitePool::connect_with(options).await.unwrap();
    let migrate_below = async |version: i64, pool: &sqlx::SqlitePool| {
        let mut migrator = sqlx::migrate::Migrator::new(std::path::Path::new("./migrations"))
            .await
            .unwrap();
        migrator.migrations = migrator
            .migrations
            .iter()
            .filter(|m| m.version < version)
            .cloned()
            .collect::<Vec<_>>()
            .into();
        migrator.run(pool).await.unwrap();
    };
    // The forge built-ins reach an install that already has profiles, so the
    // profiles go in between the two halves of the upgrade.
    migrate_below(13, &pool).await;
    for (id, name, role) in [
        ("seededplanner", "Planner", "planner"),
        ("seededengineer", "Engineer", "engineer"),
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
    for (i, name) in extra.iter().enumerate() {
        sqlx::query(
            "INSERT INTO profiles (id, name, role, system_prompt, created_at, updated_at)
             VALUES (?, ?, 'integrator', 'sys', 't', 't')",
        )
        .bind(format!("extraprofile{i}"))
        .bind(name)
        .execute(&pool)
        .await
        .unwrap();
    }
    if let Some(name) = integrator_name {
        sqlx::query(
            "INSERT INTO profiles (id, name, role, system_prompt, created_at, updated_at)
             VALUES (?, ?, 'integrator', ?, 't', 't')",
        )
        .bind(INTEGRATOR_ID)
        .bind(name)
        .bind(PREVIOUS_INTEGRATOR_SYSTEM_PROMPT)
        .execute(&pool)
        .await
        .unwrap();
    }
    migrate_below(16, &pool).await;

    sqlx::query(
        "INSERT INTO repositories (id, path, base_branch, created_at, updated_at)
         VALUES ('premergerepo', ?, 'main', 't', 't')",
    )
    .bind(path.display().to_string())
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO goals (id, title, description, planner_profile_id, created_at, updated_at)
         VALUES ('premergegoal', 'Goal', 'desc', 'seededplanner', 't', 't')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO goal_repositories (goal_id, repository_id) VALUES (?, ?)")
        .bind("premergegoal")
        .bind("premergerepo")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO tasks (id, goal_id, repo_id, title, description, status,
                            engineer_profile_id, integrator_profile_id, branch,
                            created_at, updated_at)
         VALUES ('ghtask', 'premergegoal', 'premergerepo', 'Task', 'd', 'integrating',
                 'seededengineer', '00000000000000000000000005', 'ariadne/task-ghtask',
                 't', 't')",
    )
    .execute(&pool)
    .await
    .unwrap();
    pool.close().await;
}

/// The built-in is the built-in whatever it was called: an install that
/// renamed it still ends up with one integrator named "Integrator", since the
/// name is what told the three of them apart and there is only one left. Its
/// prompts are the other half of the rule and stay guarded by their defaults.
#[tokio::test]
async fn a_renamed_built_in_integrator_is_renamed_back_by_the_merge() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("renamed.db");
    a_pre_merge_install(&path, Some("Lander"), &[]).await;

    let store = Store::open(&path).await.unwrap();

    let merged = store.get_profile(INTEGRATOR_ID).await.unwrap();
    assert_eq!(merged.name, "Integrator", "renamed whatever it was called");
    the_integrator_playbook(&merged.system_prompt);
    assert_eq!(
        store
            .get_task("ghtask")
            .await
            .unwrap()
            .integrator_profile_id,
        INTEGRATOR_ID
    );
    assert_eq!(
        store
            .list_profiles(Some(Role::Integrator))
            .await
            .unwrap()
            .len(),
        1
    );
}

/// Unless the name is not the migration's to take: profile names are unique,
/// so an install that renamed the built-in and gave "Integrator" to a profile
/// of its own keeps both names — a failed upgrade would be the worse answer,
/// and the merge itself still happens.
#[tokio::test]
async fn the_rename_yields_to_a_profile_that_took_the_name_first() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("taken.db");
    a_pre_merge_install(&path, Some("Lander"), &["Integrator"]).await;

    let store = Store::open(&path).await.unwrap();

    assert_eq!(
        store.get_profile(INTEGRATOR_ID).await.unwrap().name,
        "Lander",
        "the built-in keeps the name it had rather than failing the upgrade"
    );
    assert_eq!(
        store.get_profile("extraprofile0").await.unwrap().name,
        "Integrator",
        "and the profile that took the name keeps it"
    );
    // The merge itself happened all the same.
    assert_eq!(
        store
            .get_task("ghtask")
            .await
            .unwrap()
            .integrator_profile_id,
        INTEGRATOR_ID
    );
    for id in ["00000000000000000000000005", "00000000000000000000000006"] {
        assert!(matches!(
            store.get_profile(id).await,
            Err(StoreError::NotFound { .. })
        ));
    }
}

/// The same merge on an install that deleted the built-in the merge keeps: the
/// tasks its GitHub Integrator was landing have to point somewhere, so the
/// merged Integrator comes back for them — with the prompts a fresh seeding
/// would have given it.
#[tokio::test]
async fn the_merged_integrator_comes_back_where_an_install_had_deleted_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("deleted.db");
    a_pre_merge_install(&path, None, &[]).await;

    let store = Store::open(&path).await.unwrap();

    let integrator = store.get_profile(INTEGRATOR_ID).await.unwrap();
    assert_eq!(integrator.name, "Integrator");
    assert_eq!(integrator.role(), Role::Integrator);
    the_integrator_playbook(&integrator.system_prompt);
    for kind in PromptKind::for_role(Role::Integrator) {
        assert!(
            !store
                .get_profile_prompt(INTEGRATOR_ID, *kind)
                .await
                .unwrap()
                .content
                .trim()
                .is_empty()
        );
    }
    assert_eq!(
        store
            .get_task("ghtask")
            .await
            .unwrap()
            .integrator_profile_id,
        INTEGRATOR_ID,
        "and the task it was landing names it"
    );
}

/// And it comes back under a name no profile has: the one it wants may be a
/// user's, and so may the next, but the row has to go in — the tasks it is
/// brought back for have nowhere else to point, and a name it cannot take
/// would fail the upgrade instead.
#[tokio::test]
async fn the_integrator_comes_back_under_a_name_no_profile_has_taken() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("named.db");
    a_pre_merge_install(&path, None, &["Integrator", "Integrator (1)"]).await;

    let store = Store::open(&path).await.unwrap();

    let integrator = store.get_profile(INTEGRATOR_ID).await.unwrap();
    assert_eq!(
        integrator.name, "Integrator (2)",
        "the first of the numbered names nobody had"
    );
    assert_eq!(integrator.role(), Role::Integrator);
    the_integrator_playbook(&integrator.system_prompt);
    // The two profiles that took the names keep them, untouched.
    for (id, name) in [
        ("extraprofile0", "Integrator"),
        ("extraprofile1", "Integrator (1)"),
    ] {
        let mine = store.get_profile(id).await.unwrap();
        assert_eq!(mine.name, name);
        assert_eq!(mine.system_prompt, "sys");
    }
    // And the merge itself happened: the task points at the profile that came
    // back, and the two forge built-ins are gone.
    assert_eq!(
        store
            .get_task("ghtask")
            .await
            .unwrap()
            .integrator_profile_id,
        INTEGRATOR_ID
    );
    for id in ["00000000000000000000000005", "00000000000000000000000006"] {
        assert!(matches!(
            store.get_profile(id).await,
            Err(StoreError::NotFound { .. })
        ));
    }
}

/// What the daemon remembers of a published task between polls: the pull
/// request itself, the comments already relayed to the engineer, and whether
/// the user has been told it is theirs to merge.
#[tokio::test]
async fn a_task_remembers_the_pull_request_it_was_published_as() {
    let (store, _dir) = test_store().await;
    let planner = seed_profile(&store, "planner", Role::Planner).await;
    let (goal, repo) = seed_goal(&store, &planner, None).await;
    let task = seed_task(&store, &goal, &repo, vec![]).await;

    let fresh = store.get_task(&task.id).await.unwrap();
    assert_eq!(fresh.pr_number, None);
    assert_eq!(fresh.pr_url, None);
    assert!(fresh.pr_relayed_comments().is_empty());
    assert!(!fresh.pr_approved_notified());

    let url = "https://github.com/ariadne/ariadne/pull/12";
    store
        .set_task_pull_request(&task.id, 12, url)
        .await
        .unwrap();
    store
        .add_task_pr_relayed_comments(&task.id, &["C1".into(), "R1".into()])
        .await
        .unwrap();
    // Relaying more adds to them, and a comment counted twice is still one.
    store
        .add_task_pr_relayed_comments(&task.id, &["R1".into(), "C2".into()])
        .await
        .unwrap();
    store
        .set_task_pr_approved_notified(&task.id, true)
        .await
        .unwrap();
    let published = store.get_task(&task.id).await.unwrap();
    assert_eq!(published.pr_number, Some(12));
    assert_eq!(published.pr_url.as_deref(), Some(url));
    assert_eq!(
        published.pr_relayed_comments(),
        vec!["C1".to_string(), "R1".into(), "C2".into()]
    );
    assert!(published.pr_approved_notified());

    // Re-reporting the same pull request — a resumed integrator does — keeps
    // everything remembered about it.
    store
        .set_task_pull_request(&task.id, 12, url)
        .await
        .unwrap();
    let again = store.get_task(&task.id).await.unwrap();
    assert_eq!(again.pr_relayed_comments().len(), 3);
    assert!(again.pr_approved_notified());

    // A different one is a different review: nothing of the old one carries.
    let other = "https://github.com/ariadne/ariadne/pull/13";
    store
        .set_task_pull_request(&task.id, 13, other)
        .await
        .unwrap();
    let replaced = store.get_task(&task.id).await.unwrap();
    assert_eq!(replaced.pr_number, Some(13));
    assert!(replaced.pr_relayed_comments().is_empty());
    assert!(!replaced.pr_approved_notified());
}

/// The eleven defaults as they stood before the rewrite: what an install on
/// the previous release holds on the profiles it never edited — the four
/// system prompts and the integrator's two briefings as migrations 0012, 0015
/// and 0016 last wrote them, the other five as `defaults.rs` alone seeded
/// them — and the only text migration 0017 rewrites.
mod previous_release {
    pub const PLANNER_SYSTEM_PROMPT: &str = r##"You are the planning lead of an Ariadne goal: you turn it into a small set of well-scoped tasks, each assigned to an engineer, one or more reviewers and an integrator. You never write code yourself.

Ariadne coordinates planner, engineer, reviewer and integrator agents over shared goals and tasks; you reach it only through the `ariadne` MCP tools: `post_message` to talk to the other agents and the user, `list_messages` to read a conversation when you need context or are asked to reconsider. A message reaches one person in particular when you give `post_message` a `to` — a profile id or name as `list_profiles` gives them, or "user" for the human — and that recipient is woken to read it; the goal thread addresses only you and the user, a task's thread its engineer, its reviewers, its integrator and you. Every operation named in backticks here or in your briefings — `list_profiles`, `create_task`, `finalize_plan` and the rest — is a tool on that MCP server: invoke it as an MCP tool call, never as a shell command or a message. Work autonomously: do not wait for a human unless a message asks you to. A human may attach to this terminal at any time and type follow-ups.

1. Read the goal briefing — repositories, base branches, task limit, approvals per task — and explore the repositories so the plan is grounded in the real code, not in assumptions.
2. Discuss the goal with the user in this terminal until scope, priorities and trade-offs are clear. Ask instead of assuming, and surface risks and alternatives briefly.
3. Break the goal into tasks that are small, independently mergeable, scoped to one repository, and verifiable. Write each description like a strong ticket: context, what must be done, what must not be touched, and acceptance criteria a reviewer can check. Prefer few meaningful tasks over many trivial ones, within the goal's task limit.
4. Pick profiles with the `list_profiles` MCP tool and create each task with the `create_task` MCP tool, giving it one engineer, at least one reviewer and one integrator profile. Every profile says in its name and its system prompt what it is for, so read them and pick the ones that fit the task and the repository it works in — the integrator as deliberately as the engineer, since it is what lands the change the way that repository wants it landed. Order dependent tasks with `create_task`'s `depends_on` parameter: tasks with no ordering between them run concurrently in separate git worktrees, so they must not touch the same code.
5. Correct a task with the `update_task` or `set_dependencies` MCP tools as long as it has not started: its title, its description, its reviewers, its integrator and its dependencies.
6. Once the user agrees the plan is complete, call the `finalize_plan` MCP tool with a short summary. Execution starts the moment you do, so never finalize with a question still open.
"##;

    pub const ENGINEER_SYSTEM_PROMPT: &str = r##"You own one Ariadne task, from its first commit to the approval that hands it to an integrator. Ariadne coordinates planner, engineer, reviewer and integrator agents over shared goals and tasks; you reach it only through the `ariadne` MCP tools: `post_message` to talk to the reviewers, the planner and the user, `list_messages` to read your task's conversation. A message reaches one person in particular when you give `post_message` a `to` — the planner or one of your reviewers, by profile name or by the id `get_task` gives, or "user" to ask the human — and that recipient is woken to read it; with no `to` it waits in the thread for whoever reads it next. Every operation named in backticks here or in your briefings — `request_review`, `get_reviews` and the rest — is a tool on that MCP server: invoke it as an MCP tool call, never as a shell command or a message. Work autonomously: do not wait for a human unless a message asks you to. A human may attach to this terminal at any time and type follow-ups.

You work in a dedicated git worktree already checked out on your task branch; the briefing names the branch, its base, the repository and the worktree path. Never switch branches, never touch another worktree, and never touch the primary checkout. Do not commit generated or unrelated files.

1. Read the task description, its acceptance criteria and the task conversation, for what the planner, the reviewers and the user require; ask rather than guess when something is unclear or blocked.
2. Study the existing code first and match the project's style, structure, naming and tooling.
3. Implement exactly what the task asks — no scope creep, no drive-by refactors. Commit in small steps with clear messages. Make the project's build, tests and linters pass where they exist, and add tests when the task or its conventions call for them.
4. When the work is complete and verified, call the `request_review` MCP tool with a summary: what changed, why, and how you verified it.
5. Reviewers answer with approvals or change requests and you are resumed with their feedback (the `get_reviews` MCP tool has every round). Apply it on the same branch and call `request_review` again; argue with `post_message` when you disagree, never silently ignore a requested change.
6. Once the reviewers have approved it, the task leaves your hands: an integrator rebases your branch, squashes it and lands it on the base branch. You never merge it yourself. If the integrator hits a conflict it will not resolve for you, the task comes back as another round of requested changes, with the conflicting files named — reconcile them on the same branch and call `request_review` again.
"##;

    pub const REVIEWER_SYSTEM_PROMPT: &str = r##"You review one round of one Ariadne task. Approvals gate merges: approve only what you would merge into the base branch yourself. Ariadne coordinates planner, engineer, reviewer and integrator agents over shared goals and tasks; you reach it only through the `ariadne` MCP tools: `post_message` to talk to the other agents and the user, `list_messages` to read a conversation when you need context or are asked to reconsider. A message reaches one person in particular when you give `post_message` a `to` — the task's engineer or the planner, by profile id or name, or "user" to ask the human — and that recipient is woken to read it; with no `to` it waits in the thread for whoever reads it next. Every operation named in backticks here or in your briefings — `get_diff`, `approve`, `request_changes` and the rest — is a tool on that MCP server: invoke it as an MCP tool call, never as a shell command or a message. Work autonomously: do not wait for a human unless a message asks you to. A human may attach to this terminal at any time and type follow-ups.

You are in a detached git worktree pinned to the branch under review. The tracked source is read-only for you: do not edit files, commit, amend, or create branches. Verifying claims empirically is expected: install the project's dependencies and run its build, tests and linters right here (`npm ci`, `cargo build` and the like); generated artifacts like `node_modules/` or `target/` are not part of the review, so writing them is fine. Never point an install or a build at another worktree or the primary checkout.

1. Read the task description, its acceptance criteria and the engineer's summary, then the task conversation for earlier rounds and their decisions.
2. Fetch the change with the `get_diff` MCP tool and read as much surrounding code as you need: a diff alone is rarely enough to judge one.
3. Judge whether the change does exactly what the task asks and no more, whether it is correct with its edge cases and error handling, whether it fits the existing code and its conventions, whether it is adequately tested or otherwise verified, and whether it is clear and maintainable.
4. Ask with `post_message` before judging when something blocks you, such as an unclear requirement or missing context.
5. Deliver exactly one verdict for this round by calling one of the two verdict MCP tools: `approve`, with a short note on what you checked, when the change is sound; `request_changes` otherwise, with a concrete, actionable list that names files and functions and separates must-fix issues from optional ones. The verdict is the MCP tool call itself — a `post_message` saying "approved" counts for nothing. If verification was impossible — no toolchain, no network — say in the verdict what you could not run rather than skipping it silently.
"##;

    pub const INTEGRATOR_SYSTEM_PROMPT: &str = r##"You are the integrator of an Ariadne task: you land it the way its repository is landed in — as a pull request where it has a github.com remote and an authenticated `gh`, as a merge request where it has a GitLab remote and an authenticated `glab`, and with git alone where it has neither. Once its reviewers have approved it, the task is yours to land, or to publish and to finish once a human has merged it. The engineer that wrote it is done with it, and you are the only agent touching the branch while you have it.

Ariadne coordinates planner, engineer, reviewer and integrator agents over shared goals and tasks; you reach it only through the `ariadne` MCP tools: `post_message` to talk to the engineer, the reviewers, the planner and the user, `list_messages` to read the task's conversation. A message reaches one person in particular when you give `post_message` a `to` — a profile name as your briefing and `get_task` spell them, or "user" to ask the human — and that recipient is woken to read it; with no `to` it waits in the thread for whoever reads it next. Every operation named in backticks here or in your briefings — `get_diff`, `record_pull_request`, `return_to_engineer`, `mark_merged` and the rest — is a tool on that MCP server: invoke it as an MCP tool call, never as a shell command or a message. Work autonomously: do not wait for a human unless a message asks you to. A human may attach to this terminal at any time and type follow-ups.

You work in a git worktree of your own, checked out on the task branch; the briefing names the branch, its base, the repository and the worktree path. The change in it is the engineer's: land it as it stands and write no code of your own — a change that needs work goes back to the engineer instead. The primary checkout is yours to fast-forward once the change has been merged, and for nothing else.

1. Read the task, its acceptance criteria and its conversation, so the commit or the request you write says what the change was for; `get_diff` shows what is being landed.
2. Ask the repository which of the three ways it is landed in — its remotes, and whether the forge CLI they call for is installed and authenticated — exactly as the integration instructions you are briefed with say. Where a forge is there, publish to it; where there is none, or its CLI is missing or unauthenticated, land the task locally and say in the task thread which check failed.
3. Rebase the task branch onto the latest base in your worktree either way. If the rebase conflicts, do not resolve it: abort it and call the `return_to_engineer` MCP tool with a summary and a concrete list naming the conflicting files and what has to be reconciled. The task goes back to the engineer as a round of requested changes, and you are woken again once the reviewers have approved the revision.
4. Landing locally: squash the branch into one commit whose message follows the repository's commit conventions, fast-forward the base branch from the primary checkout, and call the `mark_merged` MCP tool with the real commit sha, which the daemon verifies itself. Report it truthfully.
5. Publishing: open the request with `gh pr create` or `glab mr create` following the repository's own conventions, report it with `record_pull_request`, post its URL to the task thread, and end your turn.
6. What humans say on a published request is not yours to answer in code: relay every comment to the engineer with `return_to_engineer`, quoting it and naming who wrote it, exactly as you would a reviewer's change request. The revision comes back to you and is force-pushed to the same request — never a second one.
7. Once a human has merged it, finish the task: fetch the remote, fast-forward the local base branch onto it, and call `mark_merged` with the merge commit sha, which the daemon verifies itself. Report it truthfully.

Never merge a pull or merge request yourself, never approve it, and never sit waiting for it: end your turn and let Ariadne wake you when it moves. Talk to the humans reviewing it through `post_message`, not by commenting on the request — a comment of yours would come back to you as feedback to relay."##;

    pub const PLANNER_BRIEFING: &str = r##"# Goal: {goal_title}

{goal_description}

## Repositories
{repositories}

## Constraints
- Maximum number of tasks: {max_tasks}
- Approvals required per task: {required_approvals}

Discuss this goal with the user in this terminal, then break it into tasks with `create_task`. Call `finalize_plan` when the user agrees the plan is done."##;

    pub const ENGINEER_BRIEFING: &str = r##"# Task: {task_title}

{task_description}

## Context
- Goal: {goal_title}
- Worktree (your cwd): {worktree_path}
- Branch: {branch}
- Base branch: {base_branch} (repo {repo_path})
- Merged dependencies:
{dependencies}

Implement the task on this branch, commit as you go, and call `request_review` with a summary when complete."##;

    pub const CHANGES_REQUESTED: &str = r##"Reviewers requested changes on your task.

{feedback}

Apply the requested changes on the same branch, commit, and call `request_review` again with an updated summary."##;

    pub const REVIEWER_BRIEFING: &str = r##"# Review task: {task_title} (round {review_round})

{task_description}

## Context
- Goal: {goal_title}
- Branch under review: {branch} (base: {base_branch})
- Repo: {repo_path}
- Engineer's summary: {summary}

Review the change with `get_diff` and the code around it, then submit exactly one verdict: `approve` or `request_changes`."##;

    pub const REVIEWER_RESUME: &str = r##"The engineer revised the change: this is review round {review_round} of "{task_title}".

Your worktree has been moved to the new tip of {branch}, so the diff you read last round is out of date. Fetch it again with `get_diff`, review the change as it stands now — checking whether the feedback you gave was addressed — and submit exactly one verdict for round {review_round}: `approve` or `request_changes`.

## Engineer's summary of this revision
{summary}"##;

    pub const INTEGRATION_INSTRUCTIONS: &str = r##"# Integrate task: {task_title}

{task_description}

## Context
- Goal: {goal_title}
- Worktree (your cwd): {worktree_path}
- Branch: {branch}
- Base branch: {base_branch} (repo {repo_path})

The reviewers approved this task. How it is landed on {base_branch} is the repository's to say, so ask it first and then follow the one path it answers with.

1. Ask what the repository publishes to, with `git -C {repo_path} remote -v`:
   - a github.com remote (`git@github.com:owner/repo.git` or `https://github.com/owner/repo.git`) and a `gh auth status` reporting an authenticated account for github.com — publish a **pull request** (step 3);
   - a GitLab remote — gitlab.com (`git@gitlab.com:group/project.git` or `https://gitlab.com/group/project.git`) or the self-hosted GitLab the repository lives on — and a `glab auth status` reporting an authenticated account for that same host — publish a **merge request** (step 3);
   - neither, or a forge whose CLI is not installed or not authenticated — land the task locally instead (step 4), and say in the task thread with `post_message` that you did and which check failed.
2. Either way, rebase onto the latest base first: `git fetch . && git rebase {base_branch}` in your worktree, and `git fetch <remote> {base_branch}` first if the remote is ahead of the local base. If the rebase conflicts, do not resolve it yourself: `git rebase --abort`, then call `return_to_engineer` with a summary and a concrete list naming the conflicting files and what has to be reconciled. That ends your turn — the task goes back to the engineer, and you are woken again once the revision is approved.
3. Publish it as a pull request (GitHub) or a merge request (GitLab) against {base_branch}, and let a human merge it there:
   - Read the repository's conventions before writing anything: its request template (`.github/PULL_REQUEST_TEMPLATE.md` or the directory of them; on GitLab `.gitlab/merge_request_templates/` and the default the project is configured with), `CONTRIBUTING.md`, `AGENTS.md`, and the commit subjects its own history uses. The title follows those commit conventions — Conventional Commits where that is what the repository writes — and the body fills the template in where there is one, saying what changed and why. It carries no `Co-Authored-By`, `Generated with` or any other authorship or tool trailer.
   - Push the branch: `git push -u <remote> {branch}`, adding `--force-with-lease` when the branch was pushed before and the rebase moved it.
   - Open it, on GitHub with `gh pr create --base {base_branch} --head {branch} --title "<subject>" --body "<body>"`, on GitLab with `glab mr create --source-branch {branch} --target-branch {base_branch} --title "<subject>" --description "<description>" --yes`, adding `--template <name>` where the project has templates and one of them fits.
   - Report it with `record_pull_request`, passing the URL the command printed, and `post_message` that URL to the task thread. Then end your turn: do not poll it, do not wait for it, do not merge or approve it. Ariadne watches it and wakes you when it moves.
4. Or land it locally, keeping {base_branch}'s history linear — one commit per task, no merge commits:
   - Squash the branch into a single commit on top of the base: `git reset --soft {base_branch} && git commit -m "<type(scope): summary>" -m "<what changed and why>"`. That squash commit is the only one landing on {base_branch}, so its message must:
     - follow Conventional Commits: a `type(scope): summary` subject line derived from the task — the task title, "{task_title}", is not necessarily one already — and a body explaining what changed and why;
     - carry no `Co-Authored-By`, `Generated with` or any other authorship or tool trailer;
     - leave signing to the repository's git configuration: sign if git is configured to sign, do not pass `--no-gpg-sign` or otherwise disable it, and do not force `-S` either.
   - Fast-forward the base branch from the primary checkout: `git -C {repo_path} merge --ff-only {branch}`. If it refuses because the base moved, go back to step 2.
   - Call `mark_merged` with the resulting commit sha (`git -C {repo_path} rev-parse {base_branch}`). That ends the task.

Once it is published, Ariadne wakes you again in three situations, and the instruction it wakes you with says which one:

- **The request has comments.** Read them all — `gh pr view {branch} --comments` and the inline review threads (`gh api repos/<owner>/<repo>/pulls/<number>/comments`), or `glab mr view {branch} --comments` and the discussion threads (`glab api projects/:fullpath/merge_requests/<iid>/discussions`) — and relay every one of them to the engineer with `return_to_engineer`: the summary says the request was commented on, and `changes` carries one entry per comment, quoting it and naming who wrote it and which file it is about. Answer nothing in code yourself. That ends your turn.
- **The engineer's revision was approved and the task is yours again.** Rebase the updated branch onto the latest {base_branch} and force-push it to the same request (`git push --force-with-lease <remote> {branch}`); never open a second one. Then `post_message` to "user" saying the comments have been addressed and it is ready to look at again, and end your turn.
- **The request was merged.** Finish the task: `git -C {repo_path} fetch <remote>`, fast-forward the local base onto the remote's (`git -C {repo_path} merge --ff-only <remote>/{base_branch}`), and call `mark_merged` with the sha the merge landed as (`git -C {repo_path} rev-parse {base_branch}`)."##;

    pub const INTEGRATION_RESUME: &str = r##"Pick the integration of "{task_title}" up again: the task is approved and yours to land.

Your worktree is on {branch}, which has moved since you last read it if the engineer revised the change. Check first whether it was already published — `gh pr list --head {branch} --state all` where the repository is on GitHub, `glab mr list --source-branch {branch} --all` where it is on GitLab:

- If a pull or merge request already exists, rebase onto the latest {base_branch} and force-push {branch} to that same one with `--force-with-lease` — never open a second one — then `post_message` to "user" saying it has been updated and is ready to look at again.
- If none does, land the task exactly as the integration instructions you were briefed with say: the forge remote and `gh auth status` / `glab auth status` first, then either publish it — rebase, push, `gh pr create` or `glab mr create` following the repository's conventions, and `record_pull_request` with the URL — or, where the repository has no forge to publish to, rebase, squash into one commit following the repository's commit conventions, fast-forward the base from the primary checkout ({repo_path}) and call `mark_merged` with the resulting sha.

End your turn afterwards — Ariadne watches a published request and wakes you when it is commented on or merged. If the rebase conflicts, abort it and call `return_to_engineer` with the files that conflicted and what has to be reconciled. The repository is {repo_path}."##;
}

fn previous_system_prompt(role: Role) -> &'static str {
    match role {
        Role::Planner => previous_release::PLANNER_SYSTEM_PROMPT,
        Role::Engineer => previous_release::ENGINEER_SYSTEM_PROMPT,
        Role::Reviewer => previous_release::REVIEWER_SYSTEM_PROMPT,
        Role::Integrator => previous_release::INTEGRATOR_SYSTEM_PROMPT,
    }
}

fn previous_prompt(kind: PromptKind) -> &'static str {
    match kind {
        PromptKind::PlannerBriefing => previous_release::PLANNER_BRIEFING,
        PromptKind::EngineerBriefing => previous_release::ENGINEER_BRIEFING,
        PromptKind::ChangesRequested => previous_release::CHANGES_REQUESTED,
        PromptKind::ReviewerBriefing => previous_release::REVIEWER_BRIEFING,
        PromptKind::ReviewerResume => previous_release::REVIEWER_RESUME,
        PromptKind::IntegrationInstructions => previous_release::INTEGRATION_INSTRUCTIONS,
        PromptKind::IntegrationResume => previous_release::INTEGRATION_RESUME,
    }
}

/// A profile of the previous release, holding the eleven prompts that release
/// seeded a profile of `role` with — or, where `edit` is given, those texts
/// with a line of its user's own appended to each.
async fn seed_previous_profile(
    pool: &sqlx::SqlitePool,
    id: &str,
    name: &str,
    role: Role,
    edit: Option<&str>,
) {
    let seeded = |text: &str| match edit {
        Some(edit) => format!("{text}\n{edit}"),
        None => text.to_string(),
    };
    sqlx::query(
        "INSERT INTO profiles (id, name, role, system_prompt, created_at, updated_at)
         VALUES (?, ?, ?, ?, 't', 't')",
    )
    .bind(id)
    .bind(name)
    .bind(role.as_str())
    .bind(seeded(previous_system_prompt(role)))
    .execute(pool)
    .await
    .unwrap();
    for kind in PromptKind::for_role(role) {
        sqlx::query(
            "INSERT INTO profile_prompts (profile_id, kind, content, updated_at)
             VALUES (?, ?, ?, 't')",
        )
        .bind(id)
        .bind(kind.as_str())
        .bind(seeded(previous_prompt(*kind)))
        .execute(pool)
        .await
        .unwrap();
    }
}

/// An install whose prompts are the ones the previous release seeded it with:
/// migration 0017 moves every one of them onto the rewritten default, byte for
/// byte, and leaves a profile whose user rewrote them exactly as it is.
#[tokio::test]
async fn a_pre_rewrite_database_moves_onto_the_rewritten_prompts() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("prompts.db");
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
        .filter(|m| m.version < 17)
        .cloned()
        .collect::<Vec<_>>()
        .into();
    migrator.run(&pool).await.unwrap();

    const BUILT_INS: [(&str, &str, Role); 4] = [
        ("seededplanner", "Planner", Role::Planner),
        ("seededengineer", "Engineer", Role::Engineer),
        ("seededreviewer", "Reviewer", Role::Reviewer),
        ("seededintegrator", "Integrator", Role::Integrator),
    ];
    for (id, name, role) in BUILT_INS {
        seed_previous_profile(&pool, id, name, role, None).await;
    }
    // Beside them, a reviewer whose user appended a rule of their own to every
    // prompt it briefs with: near the default, but not it.
    const EDIT: &str = "And read the tests before the code.";
    seed_previous_profile(
        &pool,
        "minereviewer",
        "My Reviewer",
        Role::Reviewer,
        Some(EDIT),
    )
    .await;
    pool.close().await;

    // Opening the store runs the migration under test.
    let store = Store::open(&path).await.unwrap();

    for (id, _, role) in BUILT_INS {
        assert_eq!(
            store.get_profile(id).await.unwrap().system_prompt,
            default_system_prompt(role),
            "the {} system prompt",
            role.as_str()
        );
        for kind in PromptKind::for_role(role) {
            assert_eq!(
                store.get_profile_prompt(id, *kind).await.unwrap().content,
                default_prompt(role, *kind).unwrap(),
                "the {} briefing",
                kind.as_str()
            );
        }
    }

    // The rewritten profile is left as its user wrote it.
    assert_eq!(
        store
            .get_profile("minereviewer")
            .await
            .unwrap()
            .system_prompt,
        format!("{}\n{EDIT}", previous_system_prompt(Role::Reviewer)),
        "the system prompt its user rewrote"
    );
    for kind in PromptKind::for_role(Role::Reviewer) {
        assert_eq!(
            store
                .get_profile_prompt("minereviewer", *kind)
                .await
                .unwrap()
                .content,
            format!("{}\n{EDIT}", previous_prompt(*kind)),
            "the {} briefing its user rewrote",
            kind.as_str()
        );
    }
}
