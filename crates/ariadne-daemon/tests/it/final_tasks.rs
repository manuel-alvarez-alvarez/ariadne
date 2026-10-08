//! The end of a `feature_branch` goal: the final task of each repository.
//!
//! The final task is the one task that depends directly on every other task
//! of its repository. It waits for all of them, then works on the goal branch
//! itself and takes it onto the base branch by one request. The agents are the
//! harness's stub, and the test runs the git commands of the briefing.

use crate::common;

use std::path::PathBuf;

use axum::http::StatusCode;

use ariadne_api::tasks::TaskDto;
use ariadne_core::{Actor, ForgeKind, GoalStatus, Landing, MessageKind, Seat, TaskStatus};
use ariadne_store::{Goal, NewTask, NewTaskAgent, Repository, SetForgeIntegration, Task};

use common::forge::{answer, opened_pull, stub_forge_cli};
use common::{Harness, TIMEOUT, as_session, harness, post_json, sh, test_pin};

/// `gh` signed in to github.com, a `pr create` that answers with `url`, and
/// the request it opened from `head` read back.
fn forge_script(url: &str, head: &str) -> serde_json::Value {
    serde_json::json!([
        answer(&["auth", "status"], 0, ""),
        answer(&["pr", "create"], 0, url),
        answer(&["pr", "view"], 0, &opened_pull(url, head).to_string()),
        answer(&["pr", "list"], 0, "[]"),
    ])
}

/// A `feature_branch` goal in planning on `repo`.
async fn feature_goal(h: &Harness, repo: &Repository) -> Goal {
    h.store
        .create_goal(ariadne_store::NewGoal {
            issue_url: None,
            title: "Ship feature".into(),
            description: String::new(),
            repository_ids: vec![repo.id.clone()],
            pin: test_pin(),
            landing: Some(Landing::FeatureBranch),
        })
        .await
        .unwrap()
}

/// A task on `repo` with one author, `reviewers` reviewers and `depends_on`.
async fn task(
    h: &Harness,
    goal: &Goal,
    repo: &Repository,
    title: &str,
    reviewers: usize,
    depends_on: Vec<String>,
) -> Task {
    let mut agents = vec![NewTaskAgent::new(Seat::Author, ["coding"], test_pin())];
    agents.extend(
        (0..reviewers).map(|_| NewTaskAgent::new(Seat::Reviewer, ["code-review"], test_pin())),
    );
    h.store
        .create_task(NewTask {
            goal_id: goal.id.clone(),
            repo_id: repo.id.clone(),
            title: title.into(),
            description: String::new(),
            agents,
            depends_on,
        })
        .await
        .unwrap()
}

async fn finalize(h: &Harness, goal: &Goal, expected: StatusCode) -> serde_json::Value {
    let orchestrator = h.orchestrator_session(goal).await;
    let request = as_session(
        &format!("/v1/goals/{}/finalize", goal.id),
        &orchestrator.id,
        serde_json::json!({}),
    );
    match expected {
        StatusCode::OK => h.json(request, expected).await,
        _ => serde_json::to_value(h.error(request, expected).await.error.message).unwrap(),
    }
}

fn goal_branch(goal: &Goal) -> String {
    format!("ship-feature-{}", &goal.id[goal.id.len() - 6..])
}

#[tokio::test]
async fn a_feature_branch_plan_with_no_final_task_is_refused() {
    let h = harness().await;
    let path = h.git_repo("repo");
    let repo = h.repository(&path).await;
    let goal = feature_goal(&h, &repo).await;
    task(&h, &goal, &repo, "First change", 1, vec![]).await;
    task(&h, &goal, &repo, "Second change", 1, vec![]).await;

    let message = finalize(&h, &goal, StatusCode::CONFLICT).await;

    let message = message.as_str().unwrap();
    assert!(
        message.contains(&format!("repository {} has no final task", repo.path)),
        "{message}"
    );
    assert_eq!(
        h.store.get_goal(&goal.id).await.unwrap().status(),
        GoalStatus::Planning
    );
    assert!(
        !sh(&path, "git branch --list").contains(&goal_branch(&goal)),
        "a refused plan cut a goal branch"
    );
}

#[tokio::test]
async fn a_task_created_after_finalize_joins_the_final_task() {
    let h = harness().await;
    let path = h.git_repo("repo");
    let repo = h.repository(&path).await;
    let goal = feature_goal(&h, &repo).await;
    let first = task(&h, &goal, &repo, "First change", 1, vec![]).await;
    let last = task(
        &h,
        &goal,
        &repo,
        "Open the request",
        0,
        vec![first.id.clone()],
    )
    .await;
    finalize(&h, &goal, StatusCode::OK).await;

    let late: TaskDto = h
        .json(
            post_json(
                &format!("/v1/goals/{}/tasks", goal.id),
                serde_json::json!({"title": "Late change", "agents": [
                    {"seat": "author", "skills": ["coding"], "model": test_pin().model}]}),
            ),
            StatusCode::CREATED,
        )
        .await;

    assert_eq!(h.store.list_task_dependencies(&last.id).await.unwrap(), {
        let mut ids = vec![first.id.clone(), late.id.clone()];
        ids.sort();
        ids
    });
    assert_eq!(
        h.store
            .final_task(&goal.id, &repo.id)
            .await
            .unwrap()
            .map(|t| t.id),
        Some(last.id)
    );
}

#[tokio::test]
async fn the_final_task_waits_then_lands_the_goal_branch_on_the_base() {
    const URL: &str = "https://github.com/acme/widgets/pull/9";
    // Every request this test reads is the goal branch's, whose name is the
    // goal's: the stub is written once the goal exists.
    let cli = stub_forge_cli(forge_script(URL, "fix"));
    let h = harness()
        .scheduler()
        .discover_agents()
        .forge_cli(&cli)
        .await;
    let path = h.git_repo("repo");
    let repo = h.repository(&path).await;
    let remote = h.at("remote.git");
    sh(
        &path,
        &format!(
            "git init -q --bare '{}' && git remote add origin '{}'",
            remote.display(),
            remote.display()
        ),
    );
    // A forge row the daemon can call `gh` through, independent of the real
    // git remote above: the CLI is stubbed, so only the actual push and
    // `ls-remote` need to be real.
    h.store
        .set_forge_integration(SetForgeIntegration {
            repository_id: repo.id.clone(),
            kind: ForgeKind::Github,
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
    let goal = feature_goal(&h, &repo).await;
    cli.reprogram(forge_script(URL, &goal_branch(&goal)));
    let first = task(&h, &goal, &repo, "First change", 1, vec![]).await;
    let last = task(
        &h,
        &goal,
        &repo,
        "Open the request",
        0,
        vec![first.id.clone()],
    )
    .await;
    finalize(&h, &goal, StatusCode::OK).await;
    let branch = goal_branch(&goal);

    // The first task runs, and the final task waits for it.
    h.reconcile_task_until(&first.id, TIMEOUT, "the first author", async || {
        h.running_session(&first.id, Seat::Author).await.is_some()
    })
    .await;
    assert_eq!(h.status(&last.id).await, TaskStatus::Pending);
    let author = h.running_session(&first.id, Seat::Author).await.unwrap();
    let worktree = PathBuf::from(author.worktree_path.as_ref().unwrap());
    sh(
        &worktree,
        "echo change > change.txt && git add . && git -c user.email=t@t -c user.name=t commit -qm 'feat: add change'",
    );
    let reviewer = h
        .store
        .list_task_reviewers(&first.id)
        .await
        .unwrap()
        .remove(0);
    h.store
        .transition_task(
            &first.id,
            TaskStatus::UnderReview,
            Actor::Author,
            None,
            None,
        )
        .await
        .unwrap();
    h.verdict(&first, &reviewer.id, MessageKind::Approve, "looks right")
        .await;
    h.reconcile_task_until(&first.id, TIMEOUT, "the first approval", async || {
        h.status(&first.id).await == TaskStatus::Approved
    })
    .await;
    assert_eq!(h.status(&last.id).await, TaskStatus::Pending);
    sh(&path, &format!("git fetch -q . {}:{branch}", first.branch));
    let landed = sh(&path, &format!("git rev-parse {branch}"));
    let _: TaskDto = h
        .json(
            as_session(
                &format!("/v1/tasks/{}/transitions", first.id),
                &author.id,
                serde_json::json!({"to": "finished", "merge_commit": landed}),
            ),
            StatusCode::OK,
        )
        .await;

    // Then the final task starts on the goal branch, and cuts no branch.
    h.reconcile_task_until(&last.id, TIMEOUT, "the final author", async || {
        h.status(&last.id).await == TaskStatus::InProgress
            && h.running_session(&last.id, Seat::Author).await.is_some()
    })
    .await;
    let author = h.running_session(&last.id, Seat::Author).await.unwrap();
    let worktree = PathBuf::from(author.worktree_path.as_ref().unwrap());
    assert_eq!(sh(&worktree, "git branch --show-current"), branch);
    assert_eq!(sh(&worktree, "git rev-parse HEAD"), landed);
    assert!(!sh(&path, "git branch --list").contains(&last.branch));
    let dto: TaskDto = h.get(&format!("/v1/tasks/{}", last.id)).await;
    assert_eq!(dto.branch, branch);

    // With no reviewer it is approved at once, and briefed to publish the
    // goal branch against the base branch.
    h.store
        .transition_task(&last.id, TaskStatus::UnderReview, Actor::Author, None, None)
        .await
        .unwrap();
    h.reconcile_task_until(&last.id, TIMEOUT, "the final landing", async || {
        h.status(&last.id).await == TaskStatus::Approved
            && h.told(&author.id).contains("# Land goal branch:")
    })
    .await;
    let told = h.told(&author.id);
    assert!(
        told.contains(&format!(
            "Open a pull or merge request for the goal branch {branch} onto main"
        )),
        "{told}"
    );
    assert!(told.contains("`open_pull_request`"), "{told}");

    // Finishing before the branch is pushed is refused.
    let tip = sh(&worktree, "git rev-parse HEAD");
    let finish = |sha: &str| {
        as_session(
            &format!("/v1/tasks/{}/transitions", last.id),
            &author.id,
            serde_json::json!({"to": "finished", "merge_commit": sha}),
        )
    };
    h.error(finish(&tip), StatusCode::CONFLICT).await;

    // Push the goal branch and open the request: the daemon runs `gh`.
    sh(&worktree, "git push origin HEAD");
    let opened: TaskDto = h
        .json(
            as_session(
                &format!("/v1/tasks/{}/pull-request", last.id),
                &author.id,
                serde_json::json!({"title": "feat: ship feature", "body": "Ships it."}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(opened.pr_url.as_deref(), Some(URL));
    assert_eq!(
        cli.invocations()
            .iter()
            .filter(|call| call
                .args
                .starts_with(&["pr".to_string(), "create".to_string()]))
            .count(),
        1,
        "{:?}",
        cli.invocations()
    );

    // Its author keeps the request with the skill the daemon loaded for it,
    // and the task stays approved until a human merges it: nothing merges
    // the goal branch onto the base.
    let launch = h.launch_file(&author.id).expect("a launch file");
    assert!(
        launch.system_prompt.contains("- pr-babysit: "),
        "{}",
        launch.system_prompt
    );
    assert_eq!(h.status(&last.id).await, TaskStatus::Approved);
    let refused = h.error(finish(&tip), StatusCode::CONFLICT).await;
    assert!(
        refused.error.message.contains("is not merged"),
        "{}",
        refused.error.message
    );
    let kept = h
        .store
        .pull_request_of_task(&last.id)
        .await
        .unwrap()
        .expect("the ledger holds the task's request");
    h.store
        .set_pull_request_state(&kept.id, "merged")
        .await
        .unwrap();
    let finished: TaskDto = h.json(finish(&tip), StatusCode::OK).await;
    assert_eq!(finished.status, TaskStatus::Finished);
    assert_eq!(finished.merge_commit.as_deref(), Some(tip.as_str()));
    let main_before_cleanup = sh(&path, "git rev-parse main");

    // The task over and its request merged, the daemon deletes the goal
    // branch, local and remote (026); nothing of it merged the goal branch
    // onto the base itself.
    common::eventually(TIMEOUT, "the goal branch to go", async || {
        h.flush_scheduler().await;
        !sh(&path, "git branch --list").contains(&branch)
    })
    .await;
    assert_eq!(
        sh(&path, "git rev-parse main"),
        main_before_cleanup,
        "nothing merged the goal branch onto the base"
    );
    assert!(
        !sh(&remote, "git branch --list").contains(&branch),
        "the goal branch is gone from the remote too"
    );
}

/// A feature goal finalized with a first task and its final task, the first
/// already finished: the final task is free to start.
async fn final_task_free_to_start(h: &Harness) -> (Goal, Repository, Task) {
    let repo = h.repository(&h.git_repo("repo")).await;
    let goal = feature_goal(h, &repo).await;
    let first = task(h, &goal, &repo, "First change", 0, vec![]).await;
    let last = task(
        h,
        &goal,
        &repo,
        "Open the request",
        0,
        vec![first.id.clone()],
    )
    .await;
    finalize(h, &goal, StatusCode::OK).await;
    h.advance(&first, TaskStatus::UnderReview).await;
    for (to, actor) in [
        (TaskStatus::Approved, Actor::Daemon),
        (TaskStatus::Finished, Actor::Author),
    ] {
        h.store
            .transition_task(&first.id, to, actor, None, Some("abc"))
            .await
            .unwrap();
    }
    (goal, repo, last)
}

async fn create_late(h: &Harness, goal: &Goal) -> axum::http::Response<axum::body::Body> {
    h.response(post_json(
        &format!("/v1/goals/{}/tasks", goal.id),
        serde_json::json!({"title": "Late change", "agents": [
            {"seat": "author", "skills": ["coding"], "model": test_pin().model}]}),
    ))
    .await
}

/// A task created while the final task is about to start joins it first: the
/// stale `ready` goes back to `pending`, and the author does not start.
#[tokio::test]
async fn a_task_created_before_the_final_task_starts_delays_the_start() {
    let h = harness().await;
    let (goal, _repo, last) = final_task_free_to_start(&h).await;
    // The scheduler read the finished dependency, and then the create came.
    let late = create_late(&h, &goal).await;
    assert_eq!(late.status(), StatusCode::CREATED);
    h.store
        .transition_task(&last.id, TaskStatus::Ready, Actor::Daemon, None, None)
        .await
        .unwrap();

    let started = h.launcher.spawn_author(&last.id).await;

    assert!(
        started.is_err(),
        "the final author started before the late task finished"
    );
    assert_eq!(h.status(&last.id).await, TaskStatus::Pending);
    assert!(h.running_session(&last.id, Seat::Author).await.is_none());
    assert_eq!(
        h.store.get_task(&last.id).await.unwrap().branch,
        last.branch
    );
}

/// A task created once the final task has claimed the goal branch is refused
/// before it exists, even while the final task still reads `ready`.
#[tokio::test]
async fn a_task_created_after_the_final_task_starts_is_refused() {
    let h = harness().await;
    let (goal, _repo, last) = final_task_free_to_start(&h).await;
    h.store
        .transition_task(&last.id, TaskStatus::Ready, Actor::Daemon, None, None)
        .await
        .unwrap();
    h.store
        .start_on_goal_branch(&last.id)
        .await
        .unwrap()
        .expect("the final task is free to start");
    let before = h.store.list_tasks(Default::default()).await.unwrap().len();

    let late = create_late(&h, &goal).await;

    assert_eq!(late.status(), StatusCode::CONFLICT);
    assert_eq!(
        h.store.list_tasks(Default::default()).await.unwrap().len(),
        before
    );
    assert_eq!(h.status(&last.id).await, TaskStatus::Ready);
}

/// A task that joins the final task as it is about to start sends it back to
/// wait. The wait is no failed spawn: four in a row do not fail the final task,
/// and it starts once every task it waits for is finished.
#[tokio::test]
async fn a_final_task_sent_back_to_wait_again_and_again_still_starts() {
    let h = harness().scheduler().discover_agents().await;
    let repo = h.repository(&h.git_repo("repo")).await;
    let goal = feature_goal(&h, &repo).await;
    let first = task(&h, &goal, &repo, "First change", 0, vec![]).await;
    let last = task(
        &h,
        &goal,
        &repo,
        "Open the request",
        0,
        vec![first.id.clone()],
    )
    .await;
    finalize(&h, &goal, StatusCode::OK).await;

    let mut joined = vec![first];
    for round in 0..4 {
        joined.push(task(&h, &goal, &repo, &format!("Late change {round}"), 0, vec![]).await);
        // The scheduler read the dependencies before the task joined.
        h.store
            .transition_task(&last.id, TaskStatus::Ready, Actor::Daemon, None, None)
            .await
            .unwrap();
        h.notify(&last.id);
        h.flush_scheduler().await;
        assert_eq!(
            h.status(&last.id).await,
            TaskStatus::Pending,
            "round {round}"
        );
        assert!(h.running_session(&last.id, Seat::Author).await.is_none());
    }

    for joined in &joined {
        h.reconcile_task_until(&joined.id, TIMEOUT, "the author to start", async || {
            h.status(&joined.id).await == TaskStatus::InProgress
        })
        .await;
        h.store
            .transition_task(
                &joined.id,
                TaskStatus::UnderReview,
                Actor::Author,
                None,
                None,
            )
            .await
            .unwrap();
        h.reconcile_task_until(&joined.id, TIMEOUT, "the approval", async || {
            h.status(&joined.id).await == TaskStatus::Approved
        })
        .await;
        h.store
            .transition_task(
                &joined.id,
                TaskStatus::Finished,
                Actor::Author,
                None,
                Some("abc"),
            )
            .await
            .unwrap();
    }

    h.reconcile_task_until(&last.id, TIMEOUT, "the final author", async || {
        h.status(&last.id).await == TaskStatus::InProgress
            && h.running_session(&last.id, Seat::Author).await.is_some()
    })
    .await;
    assert_eq!(
        h.store.get_task(&last.id).await.unwrap().branch,
        goal_branch(&goal)
    );
}

/// A cancelled task used to count against the "every other task" side of
/// every candidate match (005 rule 10), forever: cancelled is terminal and
/// never retried, so no live final task could ever equal a count that still
/// included it. A cancelled task the final task never depended on must not
/// stop the plan from finalizing, and the final task must still start once
/// the live task it depends on is finished — the cancelled one is no reason
/// for it to wait or fail.
#[tokio::test]
async fn a_cancelled_task_does_not_block_finalize_or_the_final_task_starting() {
    let h = harness().scheduler().discover_agents().await;
    let repo = h.repository(&h.git_repo("repo")).await;
    let goal = feature_goal(&h, &repo).await;
    let first = task(&h, &goal, &repo, "First change", 0, vec![]).await;
    let redundant = task(&h, &goal, &repo, "Redundant change", 0, vec![]).await;
    let last = task(
        &h,
        &goal,
        &repo,
        "Open the request",
        0,
        vec![first.id.clone()],
    )
    .await;
    h.store
        .transition_task(
            &redundant.id,
            TaskStatus::Cancelled,
            Actor::User,
            None,
            None,
        )
        .await
        .unwrap();

    finalize(&h, &goal, StatusCode::OK).await;

    // The scheduler runs the live task through to `finished`, on its own.
    h.reconcile_task_until(&first.id, TIMEOUT, "the first author", async || {
        h.running_session(&first.id, Seat::Author).await.is_some()
    })
    .await;
    h.store
        .transition_task(
            &first.id,
            TaskStatus::UnderReview,
            Actor::Author,
            None,
            None,
        )
        .await
        .unwrap();
    h.reconcile_task_until(&first.id, TIMEOUT, "the first approval", async || {
        h.status(&first.id).await == TaskStatus::Approved
    })
    .await;
    h.store
        .transition_task(
            &first.id,
            TaskStatus::Finished,
            Actor::Author,
            None,
            Some("abc"),
        )
        .await
        .unwrap();

    // Then the scheduler, not the test, starts the final task on the goal
    // branch: if a cancelled dependency were still failing it, this is where
    // it would show.
    h.reconcile_task_until(&last.id, TIMEOUT, "the final author", async || {
        h.status(&last.id).await == TaskStatus::InProgress
            && h.running_session(&last.id, Seat::Author).await.is_some()
    })
    .await;
    assert_eq!(
        h.store.get_task(&last.id).await.unwrap().branch,
        goal_branch(&goal)
    );
    assert_eq!(h.status(&redundant.id).await, TaskStatus::Cancelled);
}
