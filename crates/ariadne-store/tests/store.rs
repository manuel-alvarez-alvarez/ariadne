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
            extra_flags: vec![],
            prompts: vec![],
        })
        .await
        .unwrap()
}

async fn seed_goal(store: &Store, planner: &Profile, max_tasks: Option<i64>) -> (Goal, GoalRepo) {
    let goal = store
        .create_goal(NewGoal {
            title: "Test goal".into(),
            description: "desc".into(),
            planner_profile_id: planner.id.clone(),
            max_tasks,
            required_approvals: 1,
            repos: vec![("/tmp/repo".into(), "main".into())],
        })
        .await
        .unwrap();
    let repo = store.list_goal_repos(&goal.id).await.unwrap().remove(0);
    (goal, repo)
}

async fn seed_task(store: &Store, goal: &Goal, repo: &GoalRepo, deps: Vec<String>) -> Task {
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
            extra_flags: vec![],
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

    // Nothing references a repository yet, so delete has no guard.
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
