//! Store integration tests against a temp-file SQLite database.

use ariadne_core::{
    Actor, AgentKind, AttentionReason, GoalStatus, Landing, MessageKind, Seat, SessionStatus,
    TaskStatus, TokenUsage,
};
use ariadne_store::defaults::default_landing_prompt;
use ariadne_store::*;

async fn test_store() -> (Store, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("test.db")).await.unwrap();
    (store, dir)
}

/// The pin every seeded agent runs on: a model is required everywhere, so the
/// fixtures name one and the tests that care name their own.
fn pin(agent_kind: AgentKind, model: &str) -> AgentPin {
    AgentPin {
        agent_kind,
        model: model.into(),
        effort: None,
    }
}

/// The claude_code pin the fixtures default to.
fn default_pin() -> AgentPin {
    pin(AgentKind::ClaudeCode, "claude-sonnet-5")
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

async fn seed_goal(store: &Store) -> (Goal, Repository) {
    let repo = seed_repository(store).await;
    let goal = store
        .create_goal(NewGoal {
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
        tmux: &str,
        seat: Seat,
        agent_id: Option<&str>,
        task_id: Option<&str>,
    ) -> AgentSession {
        self.store
            .create_session(NewSession {
                goal_id: self.goal.id.clone(),
                task_id: task_id.map(str::to_string),
                seat,
                task_agent_id: agent_id.map(str::to_string),
                agent_kind: AgentKind::ClaudeCode,
                model: "claude-sonnet-5".into(),
                effort: None,
                tmux_session: tmux.into(),
                worktree_path: Some("/tmp/wt".into()),
            })
            .await
            .unwrap()
    }

    /// The author session of this world's task.
    async fn author_session(&self) -> AgentSession {
        let author = self.store.task_author(&self.task.id).await.unwrap();
        self.session(
            "ariadne-test-eng",
            Seat::Author,
            Some(&author.id),
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
/// `finished`, and who does each of them.
const HAPPY_PATH: [(TaskStatus, Actor); 5] = [
    (TaskStatus::Ready, Actor::Daemon),
    (TaskStatus::InProgress, Actor::Daemon),
    (TaskStatus::UnderReview, Actor::Author),
    (TaskStatus::Approved, Actor::Daemon),
    (TaskStatus::Finished, Actor::Author),
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
        let merge_commit = (*status == TaskStatus::Finished).then_some("abc123");
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

/// A task's author, which is where its pin lives.
async fn author_of(store: &Store, task: &Task) -> ariadne_store::TaskAgent {
    store.task_author(&task.id).await.unwrap()
}

async fn seed_task(store: &Store, goal: &Goal, repo: &Repository, deps: Vec<String>) -> Task {
    store
        .create_task(NewTask {
            goal_id: goal.id.clone(),
            repo_id: repo.id.clone(),
            title: "task".into(),
            description: "do things".into(),
            agents: vec![
                NewTaskAgent::new(Seat::Author, ["coding"], default_pin()),
                NewTaskAgent::new(Seat::Reviewer, ["code-review"], default_pin()),
            ],
            depends_on: deps,
            landing: None,
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
    assert!(
        store
            .get_agent_config(AgentKind::Acp)
            .await
            .unwrap()
            .extra_flags()
            .is_empty()
    );
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
/// How a task ends is the task's own, and the built-in procedure of that
/// ending is the whole of what its author is briefed with. A repository has
/// no say in it: it is a checkout and a base branch, and a second answer
/// stored on it could only disagree with the task's.
#[tokio::test]
async fn a_task_lands_by_the_ending_it_carries_and_the_repository_has_no_say() {
    let (store, _dir) = test_store().await;
    let repo = seed_repository(&store).await;
    let goal = store
        .create_goal(NewGoal {
            title: "Ship it".into(),
            description: String::new(),
            repository_ids: vec![repo.id.clone()],
            pin: default_pin(),
        })
        .await
        .unwrap();

    let staffed = || {
        vec![
            NewTaskAgent::new(Seat::Author, ["coding"], default_pin()),
            NewTaskAgent::new(Seat::Reviewer, ["code-review"], default_pin()),
        ]
    };
    // Nothing said: a task lands on the base branch, which is what most work
    // does.
    let default = store
        .create_task(NewTask {
            goal_id: goal.id.clone(),
            repo_id: repo.id.clone(),
            title: "Nothing said".into(),
            description: String::new(),
            agents: staffed(),
            depends_on: vec![],
            landing: None,
        })
        .await
        .unwrap();
    assert_eq!(default.landing(), Landing::Merge);

    // And each ending is briefed with its own procedure, whatever repository
    // the task is in.
    for landing in Landing::ALL {
        let task = store
            .create_task(NewTask {
                goal_id: goal.id.clone(),
                repo_id: repo.id.clone(),
                title: format!("Ends in {}", landing.as_str()),
                description: String::new(),
                agents: staffed(),
                depends_on: vec![],
                landing: Some(landing),
            })
            .await
            .unwrap();
        assert_eq!(task.landing(), landing);
        assert_eq!(
            task.landing_prompt_text(),
            default_landing_prompt(landing),
            "{}",
            landing.as_str()
        );
    }
}

#[tokio::test]
async fn a_goal_reads_its_repositories_live() {
    let (store, _dir) = test_store().await;
    let api = seed_repository(&store).await;
    let ui = seed_repository(&store).await;

    let goal = store
        .create_goal(NewGoal {
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
                agents: vec![
                    NewTaskAgent::new(Seat::Author, ["coding"], default_pin()),
                    NewTaskAgent::new(Seat::Reviewer, ["code-review"], default_pin()),
                ],
                depends_on: vec![],
                landing: None,
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
            title: "Fix the landing briefing: real fetch/rebase".into(),
            description: "d".into(),
            agents: vec![
                NewTaskAgent::new(Seat::Author, ["coding"], default_pin()),
                NewTaskAgent::new(Seat::Reviewer, ["code-review"], default_pin()),
            ],
            depends_on: vec![],
            landing: None,
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

    walk_to(&w.store, &w.task.id, TaskStatus::UnderReview).await;

    let t = walk_to(&w.store, &w.task.id, TaskStatus::Finished).await;
    assert_eq!(t.status(), TaskStatus::Finished);
    assert_eq!(t.merge_commit.as_deref(), Some("abc123"));

    let audit = w.store.list_task_transitions(&t.id).await.unwrap();
    assert_eq!(audit.len(), 5);
    assert_eq!(audit[0].from_status, "pending");
    assert_eq!(audit[4].to_status, "finished");
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
                TaskStatus::Finished,
                Actor::Author,
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
    // Finished requires a commit.
    let t = walk_to(&w.store, &task.id, TaskStatus::Approved).await;
    assert!(matches!(
        w.store
            .transition_task(&t.id, TaskStatus::Finished, Actor::Author, None, None)
            .await,
        Err(StoreError::Invalid(_))
    ));

    let audit = w.store.list_task_transitions(&task.id).await.unwrap();
    assert_eq!(audit.len(), 4, "failed transitions leave no audit rows");
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

/// What a verdict belongs to is the review that was asked for, and asking
/// again is what supersedes the verdicts before it.
///
/// There is no round number any more, so the boundary is the `review_request`
/// row itself: the verdicts that count are the ones after the last one. A
/// question asked in between is not a verdict, and a verdict from the review
/// before is not counted in this one.
#[tokio::test]
async fn a_verdict_belongs_to_the_review_that_was_asked_for() {
    let w = World::new().await;
    let (store, task) = (&w.store, &w.task);
    let reviewer = store
        .list_task_reviewers(&task.id)
        .await
        .unwrap()
        .remove(0)
        .id;
    let author = store.task_author(&task.id).await.unwrap().id;
    let message = |kind: MessageKind, from: Actor, body: &str| NewMessage {
        goal_id: task.goal_id.clone(),
        task_id: Some(task.id.clone()),
        kind,
        from_actor: from,
        from_agent_id: Some(match from {
            Actor::Author => author.clone(),
            _ => reviewer.clone(),
        }),
        from_session: None,
        to_actor: match from {
            Actor::Author => Actor::Reviewer,
            _ => Actor::Author,
        },
        to_agent_id: Some(match from {
            Actor::Author => reviewer.clone(),
            _ => author.clone(),
        }),
        body: body.into(),
    };

    // Nothing asked for yet, and nothing to read.
    assert!(store.open_review_request(&task.id).await.unwrap().is_none());
    assert!(store.open_verdicts(&task.id).await.unwrap().is_empty());

    store
        .send_message(message(
            MessageKind::ReviewRequest,
            Actor::Author,
            "have a look",
        ))
        .await
        .unwrap();
    let first = store
        .open_review_request(&task.id)
        .await
        .unwrap()
        .expect("the review that was asked for");

    store
        .send_message(message(
            MessageKind::RequestChanges,
            Actor::Reviewer,
            "please fix",
        ))
        .await
        .unwrap();
    store
        .send_message(message(
            MessageKind::Message,
            Actor::Reviewer,
            "the flag is read here too",
        ))
        .await
        .unwrap();
    assert_eq!(
        store.open_verdicts(&task.id).await.unwrap().len(),
        1,
        "a message is not counted as a verdict"
    );

    // Asked for again: the verdict before it belongs to the review before it.
    store
        .send_message(message(MessageKind::ReviewRequest, Actor::Author, "fixed"))
        .await
        .unwrap();
    let second = store.open_review_request(&task.id).await.unwrap().unwrap();
    assert_ne!(second, first, "a second request is a second review");
    assert!(
        store.open_verdicts(&task.id).await.unwrap().is_empty(),
        "asking again supersedes what was said about the change before it"
    );

    store
        .send_message(message(
            MessageKind::Approve,
            Actor::Reviewer,
            "looks right now",
        ))
        .await
        .unwrap();
    let open = store.open_verdicts(&task.id).await.unwrap();
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].kind(), Some(MessageKind::Approve));
}

/// A message goes to exactly one recipient and is delivered once: the stamp is
/// what says which of them have reached a pane.
#[tokio::test]
async fn a_message_is_delivered_once_and_the_stamp_says_so() {
    let w = World::new().await;
    let (store, task) = (&w.store, &w.task);
    let author = store.task_author(&task.id).await.unwrap().id;

    let asked = store
        .send_message(NewMessage {
            goal_id: task.goal_id.clone(),
            task_id: Some(task.id.clone()),
            kind: MessageKind::Message,
            from_actor: Actor::Reviewer,
            from_agent_id: Some(
                store.list_task_reviewers(&task.id).await.unwrap()[0]
                    .id
                    .clone(),
            ),
            from_session: None,
            to_actor: Actor::Author,
            to_agent_id: Some(author.clone()),
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

    store.mark_message_delivered(&asked.id).await.unwrap();
    let delivered = store.get_message(&asked.id).await.unwrap();
    let at = delivered.delivered_at.clone().expect("stamped");

    // Stamping twice keeps the first time: a message typed twice is a bug in
    // the caller, and overwriting the stamp would hide it.
    store.mark_message_delivered(&asked.id).await.unwrap();
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

    let session = w.author_session().await;
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

/// What a launch is dated for: the one clock a watchdog reads when a session
/// has reported nothing at all. A relaunch moves the date, which is what makes
/// the silence it measures this run's rather than the row's.
#[tokio::test]
async fn every_launch_of_a_session_is_dated() {
    let w = World::new().await;
    let store = &w.store;

    let session = w.author_session().await;
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

    let session = w.author_session().await;
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

    let session = w.author_session().await;
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
    let session = w.author_session().await;

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
    let session = w.author_session().await;
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

/// Whether a tool that asks the user something is still waiting on them is
/// read off the session's own log, and the last event about such a call is the
/// whole of the answer: everything the turn reports in between belongs to its
/// other tool calls. Answered, dismissed by a prompt or ended with the turn,
/// the question is over — and the clear it takes with it is the narrow one.
#[tokio::test]
async fn a_pending_question_is_the_last_word_of_a_sessions_log() {
    let w = World::new().await;
    let store = &w.store;
    let session = w.author_session().await;
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
    // dialog and what the user owes the task are answered somewhere else.
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
    let session = w.author_session().await;
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
                Actor::Author,
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

/// Why an ended task ended, read off the transition that ended it: the
/// author's own `fail_task` reason, or whatever cancelled it.
///
/// Only an ending has one. A task still being worked on carries nothing,
/// however much has been said in its transitions — a review request's summary
/// is the round's, not the task's — and a retry that puts it back to work
/// takes the answer away with it.
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
        .transition_task(
            &task.id,
            TaskStatus::UnderReview,
            Actor::Author,
            Some("the first pass, with a test per lane"),
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        reason().await,
        None,
        "a round's summary is not why the task ended"
    );

    store
        .transition_task(
            &task.id,
            TaskStatus::Failed,
            Actor::Author,
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
    let author = w.author_session().await;
    let staffed = w.store.list_task_reviewers(&task.id).await.unwrap();
    let reviewer = w
        .session(
            "ariadne-test-rev",
            Seat::Reviewer,
            Some(&staffed[0].id),
            Some(&task.id),
        )
        .await;
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
    let alone = w
        .session("ariadne-test-plan", Seat::Orchestrator, None, None)
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

    let session = w.author_session().await;
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
    let failed = w.author_session().await;
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
    let session = w.author_session().await;
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
        let racing = w.author_session().await;
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
/// is the whole of the answer, and every agent names its CLI and its model.
#[tokio::test]
async fn an_agent_is_written_on_the_pin_it_was_given_whole() {
    let (store, _dir) = test_store().await;
    let (goal, repo) = seed_goal(&store).await;

    let pinned = AgentPin {
        agent_kind: AgentKind::Codex,
        model: "gpt-5.6-luna".into(),
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
                    ..NewTaskAgent::new(Seat::Author, ["coding"], default_pin())
                },
                NewTaskAgent::new(Seat::Reviewer, ["code-review"], default_pin()),
            ],
            depends_on: vec![],
            landing: None,
        })
        .await
        .unwrap();

    let author = store.task_author(&task.id).await.unwrap();
    assert_eq!(author.agent_kind(), AgentKind::Codex);
    assert_eq!(author.model, "gpt-5.6-luna");
    assert_eq!(author.effort.as_deref(), Some("max"));

    let reviewers = store.list_task_reviewers(&task.id).await.unwrap();
    assert_eq!(reviewers[0].agent_kind(), AgentKind::ClaudeCode);
    assert_eq!(reviewers[0].model, "claude-sonnet-5");
    assert_eq!(reviewers[0].effort, None);

    // And the user's later choice replaces it whole, with no half left behind.
    let moved = store
        .set_agent_pin(
            &author.id,
            &AgentPin {
                agent_kind: AgentKind::ClaudeCode,
                model: "claude-opus-5".into(),
                effort: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(moved.agent_kind(), AgentKind::ClaudeCode);
    assert_eq!(moved.model, "claude-opus-5");
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
    let session = w.author_session().await;
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

/// What a task spent, by the profile that spent it: its author once, and
/// each reviewer with every round it sat summed into one entry — a reviewer
/// runs a session per round, and nobody reads them round by round.
#[tokio::test]
async fn a_tasks_usage_groups_every_round_of_a_reviewer_together() {
    let w = World::new().await;
    let author = w.author_session().await;
    let reviewer_id = w.store.list_task_reviewers(&w.task.id).await.unwrap()[0]
        .id
        .clone();
    let first_round = w
        .session(
            "rev-round-1",
            Seat::Reviewer,
            Some(&reviewer_id),
            Some(&w.task.id),
        )
        .await;
    let second_round = w
        .session(
            "rev-round-2",
            Seat::Reviewer,
            Some(&reviewer_id),
            Some(&w.task.id),
        )
        .await;

    for (session, spent) in [
        (&author, usage(100, 80, 10)),
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
            AgentUsage {
                seat: Seat::Author,
                agent_id: author_of(&w.store, &w.task).await.id,
                usage: usage(100, 80, 10),
            },
            AgentUsage {
                seat: Seat::Reviewer,
                agent_id: reviewer_id,
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
    let _author = w.author_session().await;
    let grouped = w.store.task_usage(&w.task.id).await.unwrap();
    assert_eq!(
        grouped,
        vec![AgentUsage {
            seat: Seat::Author,
            agent_id: author_of(&w.store, &w.task).await.id,
            usage: TokenUsage::default(),
        }]
    );
}

/// A goal's usage is grouped by seat rather than by agent, and its
/// orchestrator counts: an orchestrator session belongs to no task, so
/// nothing under a task would ever have found it.
#[tokio::test]
async fn a_goals_usage_is_grouped_by_seat_and_counts_its_orchestrator() {
    let w = World::new().await;
    let orchestrator = w.session("plan", Seat::Orchestrator, None, None).await;
    let author = w.author_session().await;
    let reviewer_id = w.store.list_task_reviewers(&w.task.id).await.unwrap()[0]
        .id
        .clone();
    let reviewer = w
        .session("rev", Seat::Reviewer, Some(&reviewer_id), Some(&w.task.id))
        .await;

    for (session, spent) in [
        (&orchestrator, usage(40, 30, 8)),
        (&author, usage(100, 80, 10)),
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
                seat: Seat::Author,
                usage: usage(100, 80, 10),
            },
            SeatUsage {
                seat: Seat::Orchestrator,
                usage: usage(40, 30, 8),
            },
            SeatUsage {
                seat: Seat::Reviewer,
                usage: usage(20, 10, 4),
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
    let session = w.author_session().await;
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
            agents: vec![
                NewTaskAgent::new(Seat::Author, ["api-design"], default_pin()),
                NewTaskAgent::new(Seat::Reviewer, ["code-review"], default_pin()),
            ],
            depends_on: vec![],
            landing: None,
        })
        .await
        .unwrap();

    let refused = store.delete_skill("api-design").await;
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
            agents: vec![
                NewTaskAgent::new(Seat::Author, ["telepathy"], default_pin()),
                NewTaskAgent::new(Seat::Reviewer, ["code-review"], default_pin()),
            ],
            depends_on: vec![],
            landing: None,
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
            agents: vec![
                NewTaskAgent::new(Seat::Author, ["orchestration"], default_pin()),
                NewTaskAgent::new(Seat::Reviewer, ["code-review"], default_pin()),
            ],
            depends_on: vec![],
            landing: None,
        })
        .await;
    let message = format!("{:?}", refused.expect_err("the orchestrator's skill"));
    assert!(message.contains("orchestration"), "{message}");
    assert!(message.contains("orchestrator"), "{message}");

    // The same door is closed on a re-staff: a reviewer cannot be moved onto
    // it either.
    let task = seed_task(&store, &goal, &repo, vec![]).await;
    let reviewer = store.list_task_reviewers(&task.id).await.unwrap().remove(0);
    let refused = store
        .set_agent_skills(&reviewer.id, &["orchestration".to_string()])
        .await;
    assert!(
        matches!(refused, Err(StoreError::Conflict(_))),
        "{refused:?}"
    );
}

/// The staffing a several-author task is held to: one author or more, each on
/// a branch of its own, and — with several — at least one reviewer to pick
/// the winner.
#[tokio::test]
async fn a_task_takes_several_authors_each_on_a_branch_of_its_own() {
    let w = World::new().await;
    let staffed = |authors: usize, reviewers: usize| {
        let mut agents: Vec<NewTaskAgent> = (0..authors)
            .map(|_| NewTaskAgent::new(Seat::Author, ["coding"], default_pin()))
            .collect();
        agents.extend(
            (0..reviewers)
                .map(|_| NewTaskAgent::new(Seat::Reviewer, ["code-review"], default_pin())),
        );
        NewTask {
            goal_id: w.goal.id.clone(),
            repo_id: w.repo.id.clone(),
            title: "Contested work".into(),
            description: "do things".into(),
            agents,
            depends_on: vec![],
            landing: None,
        }
    };

    let task = w.store.create_task(staffed(2, 1)).await.unwrap();
    let authors = w.store.list_task_authors(&task.id).await.unwrap();
    assert_eq!(authors.len(), 2);
    assert_eq!(
        authors.iter().map(|a| a.ordinal).collect::<Vec<_>>(),
        [0, 1]
    );
    // The first author holds the task branch itself, so a one-author task
    // reads exactly as it always did; the second works beside it.
    assert_eq!(author_branch(&task.branch, 0), task.branch);
    assert_eq!(
        author_branch(&task.branch, 1),
        format!("{}-a2", task.branch)
    );

    // No author at all, and several with nobody to pick between them, are
    // both staffings no task can run on.
    let none = format!(
        "{:?}",
        w.store.create_task(staffed(0, 1)).await.unwrap_err()
    );
    assert!(none.contains("at least one author"), "{none}");
    let unpicked = format!(
        "{:?}",
        w.store.create_task(staffed(2, 0)).await.unwrap_err()
    );
    assert!(unpicked.contains("needs a reviewer"), "{unpicked}");
}

/// An edit replaces the author list whole, the way it always replaced the
/// reviewers — and the one-author pin fields refuse a task that has several,
/// since each of those names its own model.
#[tokio::test]
async fn an_edit_replaces_the_whole_author_list() {
    let w = World::new().await;
    let two_authors = || {
        vec![
            NewTaskAgent::new(Seat::Author, ["coding"], default_pin()),
            NewTaskAgent::new(
                Seat::Author,
                ["coding", "testing"],
                pin(AgentKind::Codex, "gpt-5.6-terra"),
            ),
        ]
    };

    let task = w
        .store
        .update_task(
            &w.task.id,
            TaskUpdate {
                authors: Some(two_authors()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let authors = w.store.list_task_authors(&task.id).await.unwrap();
    assert_eq!(authors.len(), 2);
    assert_eq!(authors[1].agent_kind(), AgentKind::Codex);

    // The task's own model field means "the author's", and it has several
    // now: the edit is refused rather than guessed about.
    let refused = format!(
        "{:?}",
        w.store
            .update_task(
                &w.task.id,
                TaskUpdate {
                    pin: Some(default_pin()),
                    ..Default::default()
                },
            )
            .await
            .unwrap_err()
    );
    assert!(refused.contains("several authors"), "{refused}");

    // And an edit cannot leave several authors with nobody to pick a winner.
    let unpicked = format!(
        "{:?}",
        w.store
            .update_task(
                &w.task.id,
                TaskUpdate {
                    reviewers: Some(vec![]),
                    ..Default::default()
                },
            )
            .await
            .unwrap_err()
    );
    assert!(unpicked.contains("needs a reviewer"), "{unpicked}");

    // Back to one author, and the pin fields mean what they always did.
    w.store
        .update_task(
            &w.task.id,
            TaskUpdate {
                authors: Some(vec![NewTaskAgent::new(
                    Seat::Author,
                    ["coding"],
                    default_pin(),
                )]),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    w.store
        .update_task(
            &w.task.id,
            TaskUpdate {
                pin: Some(pin(AgentKind::Codex, "gpt-5.6-terra")),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let author = w.store.task_author(&w.task.id).await.unwrap();
    assert_eq!(author.agent_kind(), AgentKind::Codex);
}

/// The pick as the store holds it: one row per reviewer, a second one refused
/// by the reviewer's name, the winner read off the counts with a tie to the
/// first listed, and a retry clearing the lot.
#[tokio::test]
async fn a_reviewer_picks_once_and_the_picks_settle_a_winner() {
    let w = World::new().await;
    let task = w
        .store
        .update_task(
            &w.task.id,
            TaskUpdate {
                authors: Some(vec![
                    NewTaskAgent::new(Seat::Author, ["coding"], default_pin()),
                    NewTaskAgent::new(Seat::Author, ["coding"], default_pin()),
                ]),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let authors = w.store.list_task_authors(&task.id).await.unwrap();
    let reviewer = w
        .store
        .list_task_reviewers(&task.id)
        .await
        .unwrap()
        .remove(0);

    w.store
        .record_pick(&task.id, &reviewer.id, &authors[1].id)
        .await
        .unwrap();
    let again = format!(
        "{:?}",
        w.store
            .record_pick(&task.id, &reviewer.id, &authors[0].id)
            .await
            .unwrap_err()
    );
    assert!(again.contains(&reviewer.id), "{again}");
    assert!(again.contains("already picked"), "{again}");

    let picks = w.store.list_task_picks(&task.id).await.unwrap();
    assert_eq!(picks.len(), 1);
    assert_eq!(
        picked_winner(&authors, &picks).map(|a| a.id.as_str()),
        Some(authors[1].id.as_str())
    );

    w.store
        .set_task_picked(&task.id, &authors[1].id)
        .await
        .unwrap();
    assert_eq!(
        w.task().await.picked_agent_id.as_deref(),
        Some(authors[1].id.as_str())
    );

    // A retry reviews everything afresh, so the picks go with it.
    w.store.clear_task_picks(&task.id).await.unwrap();
    assert!(w.store.list_task_picks(&task.id).await.unwrap().is_empty());
    assert_eq!(w.task().await.picked_agent_id, None);
}
