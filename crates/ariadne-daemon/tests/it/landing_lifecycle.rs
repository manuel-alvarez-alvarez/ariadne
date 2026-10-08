//! What an approved task does, driven by the scheduler over a real git
//! repository.
//!
//! The approvals leave the task with the author that wrote it: the same
//! session, the same worktree, briefed with the landing instructions its
//! repository's merge strategy names. From there it has two ways out, and
//! both are the author's own — `finish_task` once the change is on the base
//! branch, and `request_review` for a revision the people on a published
//! request asked for, which the Ariadne reviewers judge like any other round.
//!
//! The agents are the harness's stub: they answer a briefing and sit at
//! their prompt, and what each was told is read back from its launch file and
//! from the prompts the stub was sent. `git` is real, and so is the merge the
//! daemon verifies before accepting it — the agent doing the rebase, the
//! squash and the fast-forward is the test itself, running the commands its
//! briefing tells the agent to run.

use crate::common;

use std::path::PathBuf;
use std::time::Duration;

use axum::http::StatusCode;

use ariadne_api::goals::GoalDto;
use ariadne_api::messages::MessageDto;
use ariadne_api::tasks::TaskDto;
use ariadne_core::{Actor, ForgeKind, Landing, MessageKind, Seat, TaskStatus};
use ariadne_store::{AgentSession, NewTaskAgent, Repository, SetForgeIntegration, Task};

use common::forge::{answer, opened_pull, stub_forge_cli};
use common::{Cast, Harness, as_session, get, harness, patch_json, post_json, sh, test_pin};

/// How long a test waits for the scheduler to reach a state.
const TIMEOUT: Duration = Duration::from_secs(20);

/// A goal on a real repository, active, ending in `landing`, with one task
/// on it. The agents run on the stub, which discovery has accepted, so the
/// resume paths here are the ones a real session takes.
async fn seeded(landing: Landing) -> (Harness, Cast) {
    let h = harness().scheduler().discover_agents().await;
    h.git_repo("repo");
    // How a task ends is its goal's, chosen when the goal is created.
    let cast = h.active_cast_ending_in(landing).await;
    (h, cast)
}

fn repo_path(repo: &Repository) -> PathBuf {
    PathBuf::from(&repo.path)
}

/// `gh` signed in to github.com, and a `pr create` that answers with `url`.
fn forge_script(url: &str) -> serde_json::Value {
    serde_json::json!([
        answer(&["auth", "status"], 0, ""),
        answer(&["pr", "create"], 0, url),
        answer(&["pr", "view"], 0, &opened_pull(url, "fix").to_string()),
        answer(&["pr", "list"], 0, "[]"),
    ])
}

/// A bare remote for `repo` named `origin`, and a forge row the daemon can
/// call `gh` through — independent of the real git remote, since the CLI is
/// stubbed and only the push and `ls-remote` the daemon runs need to be real.
/// The integration is on: an author keeps the request it opens, and the
/// integration is what reads it for the author (005).
pub(crate) async fn with_forge(h: &Harness, repo: &Repository) {
    with_forge_enabled(h, repo, true).await
}

async fn with_forge_enabled(h: &Harness, repo: &Repository, enabled: bool) {
    let remote = h.at("remote.git");
    let path = repo_path(repo);
    sh(
        &path,
        &format!(
            "git init -q --bare '{}' && git remote add origin '{}'",
            remote.display(),
            remote.display()
        ),
    );
    h.store
        .set_forge_integration(SetForgeIntegration {
            repository_id: repo.id.clone(),
            kind: ForgeKind::Github,
            host: "github.com".into(),
            owner: "acme".into(),
            name: "widgets".into(),
            remote: "origin".into(),
            enabled,
            login: enabled.then(|| "me".into()),
            review_model: None,
            review_effort: None,
        })
        .await
        .unwrap();
}

/// The author asks for review and the reviewer approves it.
async fn approve(h: &Harness, task: &Task, reviewer: &str) {
    let task = h
        .store
        .transition_task(&task.id, TaskStatus::UnderReview, Actor::Author, None, None)
        .await
        .unwrap();
    h.verdict(&task, reviewer, MessageKind::Approve, "looks right")
        .await;
}

/// Walk a fresh task to the author landing it: the author commits
/// something, the reviewer approves, the scheduler does the rest. Returns the
/// author's worktree — which it never gave up — and the session that has
/// been briefed to land the change.
pub(crate) async fn walk_to_approved(
    h: &Harness,
    task: &Task,
    reviewer: &str,
) -> (PathBuf, AgentSession) {
    let heading = format!("# Land task: {}", task.title);
    walk_to_landing(h, task, reviewer, &heading).await
}

/// The same walk, waiting for a landing briefing that opens on `briefed`:
/// what the author is handed is the repository's text, so a repository with
/// one of its own is picked up with words the defaults never contain.
async fn walk_to_landing(
    h: &Harness,
    task: &Task,
    reviewer: &str,
    briefed: &str,
) -> (PathBuf, AgentSession) {
    h.reconcile_task_until(&task.id, TIMEOUT, "the author to be spawned", async || {
        h.status(&task.id).await == TaskStatus::InProgress
            && h.running_session(&task.id, Seat::Author).await.is_some()
    })
    .await;
    let writing = h
        .running_session(&task.id, Seat::Author)
        .await
        .expect("a live author session");

    let worktree = PathBuf::from(
        h.store
            .get_task(&task.id)
            .await
            .unwrap()
            .worktree_path
            .unwrap(),
    );
    sh(
        &worktree,
        "echo change > feature.txt && git add . && \
         git -c user.email=t@t -c user.name=t commit -qm 'wip: the change'",
    );
    approve(h, task, reviewer).await;

    h.reconcile_task_until(
        &task.id,
        TIMEOUT,
        "the author to be briefed to land it",
        async || {
            h.status(&task.id).await == TaskStatus::Approved
                && h.running_session(&task.id, Seat::Author)
                    .await
                    .is_some_and(|s| h.told(&s.id).contains(briefed))
        },
    )
    .await;
    let landing = h
        .running_session(&task.id, Seat::Author)
        .await
        .expect("a live author session");
    assert_eq!(
        landing.id, writing.id,
        "the session that wrote the change is the one landing it"
    );
    (worktree, landing)
}

/// The whole of it, the way `direct` says: the approvals leave the task with
/// its author — same session, same worktree, briefed to land it —
/// rebase-squash-fast-forward, `finish_task` accepted, cleanup, dependents
/// woken.
#[tokio::test]
async fn an_approved_task_is_landed_by_its_own_author() {
    let (h, cast) = seeded(Landing::Merge).await;
    let task = cast.task.clone();
    let dependent = h
        .store
        .create_task(ariadne_store::NewTask {
            goal_id: cast.goal.id.clone(),
            repo_id: cast.repo.id.clone(),
            title: "Use what the first one built".into(),
            description: "do things".into(),
            agents: vec![
                NewTaskAgent::new(Seat::Author, ["coding"], test_pin()),
                NewTaskAgent::new(Seat::Reviewer, ["code-review"], test_pin()),
            ],
            depends_on: vec![task.id.clone()],
        })
        .await
        .unwrap();
    let (worktree, author) = walk_to_approved(&h, &task, &cast.reviewer.id).await;

    // Nobody took the branch: the worktree the change was written in is still
    // the task's, still on the branch, and still on disk.
    assert!(worktree.exists(), "the author lost its worktree");
    assert_eq!(
        h.store
            .get_task(&task.id)
            .await
            .unwrap()
            .worktree_path
            .as_deref(),
        Some(worktree.display().to_string().as_str())
    );
    assert_eq!(
        sh(&worktree, "git rev-parse --abbrev-ref HEAD"),
        task.branch
    );

    // And the briefing it was picked up with is this repository's procedure,
    // whole: the squash it is about to run, and not a word of the forge half
    // it would have had to skip.
    let argv = h.told(&author.id);
    assert!(
        argv.contains("git reset --soft \"$(git merge-base main HEAD)\""),
        "the landing briefing does not carry the squash: {argv}"
    );
    // The forge commands themselves, not a bare "gh": the seat's own prompt
    // rides beside the briefing, and "Enough approvals" carries those two
    // letters.
    for published in ["gh pr", "glab mr", "pull request", "merge request"] {
        assert!(
            !argv.contains(published),
            "the direct landing briefing names {published}: {argv}"
        );
    }

    // What the briefing tells it to do, done: rebase, squash, fast-forward.
    sh(&worktree, "git rebase -q main");
    sh(
        &worktree,
        "git reset --soft \"$(git merge-base main HEAD)\" && \
         git -c user.email=t@t -c user.name=t commit -qm 'feat(board): render it'",
    );
    let repo = repo_path(&cast.repo);
    sh(&repo, &format!("git merge -q --ff-only {}", task.branch));
    let sha = sh(&repo, "git rev-parse main");

    let landed: TaskDto = h
        .json(
            as_session(
                &format!("/v1/tasks/{}/transitions", task.id),
                &author.id,
                serde_json::json!({"to": "finished", "merge_commit": sha}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(landed.status, TaskStatus::Finished);
    assert_eq!(landed.merge_commit.as_deref(), Some(sha.as_str()));

    // Cleanup takes the worktree with it, and the task that was waiting on
    // this one starts.
    h.reconcile_task_until(
        &task.id,
        TIMEOUT,
        "the cleanup and the dependent task",
        async || {
            !worktree.exists()
                && matches!(
                    h.status(&dependent.id).await,
                    TaskStatus::Ready | TaskStatus::InProgress
                )
        },
    )
    .await;
}

/// A merge nobody made is refused, under either procedure: the daemon checks
/// the sha really is on the base branch of the primary checkout before it
/// believes it, and the tip of the task branch is not.
///
/// That check is what keeps a reported sha worth anything, and it is the same
/// check on both paths — a squash on the forge leaves no branch on the base
/// either, so a daemon that trusted the caller would accept both.
async fn refuse_merge_that_never_happened(strategy: Landing) {
    let (h, cast) = seeded(strategy).await;
    let (worktree, author) = walk_to_approved(&h, &cast.task, &cast.reviewer.id).await;

    // The tip of the branch: real, and nowhere near the base branch.
    let sha = sh(&worktree, "git rev-parse HEAD");
    let (status, body) = h
        .send(as_session(
            &format!("/v1/tasks/{}/transitions", cast.task.id),
            &author.id,
            serde_json::json!({"to": "finished", "merge_commit": sha}),
        ))
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{strategy:?}");
    let message = String::from_utf8_lossy(&body);
    assert!(message.contains("merge not verified"), "{message}");
    assert_eq!(h.status(&cast.task.id).await, TaskStatus::Approved);
}

#[tokio::test]
async fn a_merge_that_never_happened_is_refused() {
    refuse_merge_that_never_happened(Landing::Merge).await;
}

#[tokio::test]
async fn a_squash_merge_that_never_happened_is_refused() {
    refuse_merge_that_never_happened(Landing::PullRequest).await;
}

/// The user chooses how work lands once, on the goal, and every task of the
/// goal follows it: a goal created with `pull_request` briefs each of its
/// tasks to land by pull request, and each task reads that landing back.
#[tokio::test]
async fn a_pull_request_goal_briefs_every_task_to_land_by_pull_request() {
    let h = harness().scheduler().discover_agents().await;
    let repo = h.repository(&h.git_repo("repo")).await;
    let goal: GoalDto = h
        .json(
            post_json(
                "/v1/goals",
                serde_json::json!({
                    "title": "Ship it",
                    "repository_ids": [repo.id],
                    "model": test_pin().model,
                    "landing": "pull_request",
                }),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert_eq!(goal.landing, Landing::PullRequest);

    let staffed = serde_json::json!([
        { "seat": "author", "skills": ["coding"], "model": test_pin().model },
        { "seat": "reviewer", "skills": ["code-review"], "model": test_pin().model },
    ]);
    let mut tasks = Vec::new();
    for title in ["The first change", "The second change"] {
        let task: TaskDto = h
            .json(
                post_json(
                    &format!("/v1/goals/{}/tasks", goal.id),
                    serde_json::json!({ "title": title, "agents": staffed }),
                ),
                StatusCode::CREATED,
            )
            .await;
        assert_eq!(task.landing, Landing::PullRequest, "{title}");
        tasks.push(task);
    }
    h.activate(&h.store.get_goal(&goal.id).await.unwrap()).await;

    for task in tasks {
        let reviewer = h.store.list_task_reviewers(&task.id).await.unwrap()[0].clone();
        let task = h.store.get_task(&task.id).await.unwrap();
        let (_worktree, author) = walk_to_approved(&h, &task, &reviewer.id).await;
        let told = h.told(&author.id);
        assert!(
            told.contains("`open_pull_request`"),
            "{} is not briefed to open a request: {told}",
            task.title
        );
        assert!(
            !told.contains("reset --soft"),
            "{} is briefed to squash onto the base: {told}",
            task.title
        );
    }
}

/// A goal created with no landing lands by merge, and a task takes no
/// landing of its own: a create or an edit that names one is refused, the
/// way any field the daemon does not declare is.
#[tokio::test]
async fn a_goal_with_no_landing_merges_and_a_task_takes_none_of_its_own() {
    let h = harness().await;
    let repo = h.repository(&h.git_repo("repo")).await;
    let goal: GoalDto = h
        .json(
            post_json(
                "/v1/goals",
                serde_json::json!({
                    "title": "Ship it",
                    "repository_ids": [repo.id],
                    "model": test_pin().model,
                }),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert_eq!(goal.landing, Landing::Merge);

    let tasks = format!("/v1/goals/{}/tasks", goal.id);
    let staffed = serde_json::json!([
        { "seat": "author", "skills": ["coding"], "model": test_pin().model },
    ]);
    let refused = h
        .error(
            post_json(
                &tasks,
                serde_json::json!({
                    "title": "Do it", "agents": staffed, "landing": "pull_request",
                }),
            ),
            StatusCode::UNPROCESSABLE_ENTITY,
        )
        .await;
    assert!(
        refused.error.message.contains("unknown field `landing`"),
        "{}",
        refused.error.message
    );

    let task: TaskDto = h
        .json(
            post_json(
                &tasks,
                serde_json::json!({ "title": "Do it", "agents": staffed }),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert_eq!(task.landing, Landing::Merge);
    let refused = h
        .error(
            patch_json(
                &format!("/v1/tasks/{}", task.id),
                serde_json::json!({ "landing": "none" }),
            ),
            StatusCode::UNPROCESSABLE_ENTITY,
        )
        .await;
    assert!(
        refused.error.message.contains("unknown field `landing`"),
        "{}",
        refused.error.message
    );
}

/// A `feature_branch` goal lands its tasks exactly as `merge` does, until the
/// goal has a branch of its own: the author is briefed to squash onto the
/// base, and the daemon accepts the sha only once the branch is on it.
#[tokio::test]
async fn a_feature_branch_goal_lands_its_tasks_like_merge() {
    let (h, cast) = seeded(Landing::FeatureBranch).await;
    let task = cast.task.clone();
    let dto: TaskDto = h.get(&format!("/v1/tasks/{}", task.id)).await;
    assert_eq!(dto.landing, Landing::FeatureBranch);
    let (worktree, author) = walk_to_approved(&h, &task, &cast.reviewer.id).await;
    let told = h.told(&author.id);
    assert!(
        told.contains("git reset --soft \"$(git merge-base main HEAD)\""),
        "the briefing does not carry the squash: {told}"
    );
    assert!(!told.contains("gh pr"), "{told}");

    let finish = |sha: String| {
        as_session(
            &format!("/v1/tasks/{}/transitions", task.id),
            &author.id,
            serde_json::json!({"to": "finished", "merge_commit": sha}),
        )
    };
    let tip = sh(&worktree, "git rev-parse HEAD");
    let (status, _) = h.send(finish(tip)).await;
    assert_eq!(status, StatusCode::CONFLICT, "a merge that never happened");

    sh(&worktree, "git rebase -q main");
    sh(
        &worktree,
        "git reset --soft \"$(git merge-base main HEAD)\" && \
         git -c user.email=t@t -c user.name=t commit -qm 'feat(board): render it'",
    );
    let repo = repo_path(&cast.repo);
    sh(&repo, &format!("git merge -q --ff-only {}", task.branch));
    let sha = sh(&repo, "git rev-parse main");
    let landed: TaskDto = h.json(finish(sha), StatusCode::OK).await;
    assert_eq!(landed.status, TaskStatus::Finished);
}

/// The other way out of `approved`: the people reading a published request
/// asked for something, the author made it, and the revision goes back to
/// the Ariadne reviewers like any other round — from `approved`, which is
/// where a task being landed sits.
#[tokio::test]
async fn a_revision_of_a_published_request_goes_back_to_the_reviewers() {
    const URL: &str = "https://github.com/acme/widgets/pull/12";
    let cli = stub_forge_cli(forge_script(URL));
    let h = harness()
        .scheduler()
        .discover_agents()
        .forge_cli(&cli)
        .await;
    h.git_repo("repo");
    let cast = h.active_cast_ending_in(Landing::Merge).await;
    with_forge(&h, &cast.repo).await;
    let task = cast.task.clone();
    let (worktree, author) = walk_to_approved(&h, &task, &cast.reviewer.id).await;
    sh(&worktree, "git push origin HEAD");

    // The request it opened is recorded by the author, and only by it.
    let published: TaskDto = h
        .json(
            as_session(
                &format!("/v1/tasks/{}/pull-request", task.id),
                &author.id,
                serde_json::json!({"title": "feat: ship it", "body": "Ships it."}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(published.pr_url.as_deref(), Some(URL));
    assert_eq!(
        h.store
            .pull_request_of_task(&task.id)
            .await
            .unwrap()
            .and_then(|pull| pull.origin_task_id),
        Some(task.id.clone()),
        "the ledger holds the request with its task as origin"
    );

    let reviewer_session = h
        .session(&cast.goal, Some(&task), Seat::Reviewer, &cast.reviewer.id)
        .await;
    let (status, refusal) = h
        .send(as_session(
            &format!("/v1/tasks/{}/pull-request", task.id),
            &reviewer_session.id,
            serde_json::json!({"title": "feat: ship it", "body": "Ships it."}),
        ))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "only its author opens it");
    let refusal = String::from_utf8_lossy(&refusal);
    assert!(refusal.contains("only the author"), "{refusal}");

    // And the revision it made for them is reviewed like any other change.
    let revised: TaskDto = h
        .json(
            as_session(
                &format!("/v1/tasks/{}/transitions", task.id),
                &author.id,
                serde_json::json!({
                    "to": "under_review",
                    "reason": "answered every comment on the request",
                }),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(revised.status, TaskStatus::UnderReview);
    assert_eq!(
        revised.pr_url.as_deref(),
        Some(URL),
        "the request it is a revision of is still the task's"
    );

    // The reviewers judge it, and the approval hands it back to the author
    // to push the revision and keep its request: it opens no second one.
    h.verdict(
        &task,
        &cast.reviewer.id,
        MessageKind::Approve,
        "the answers read right",
    )
    .await;
    h.reconcile_task_until(
        &task.id,
        TIMEOUT,
        "the task to come back to its author",
        async || {
            h.status(&task.id).await == TaskStatus::Approved
                && h.running_session(&task.id, Seat::Author)
                    .await
                    .is_some_and(|s| h.told(&s.id).contains("# Keep request:"))
        },
    )
    .await;

    // One channel carries both rounds, and every verdict in it is this
    // reviewer's.
    let messages: Vec<MessageDto> = h
        .json(
            get(&format!("/v1/tasks/{}/messages", task.id)),
            StatusCode::OK,
        )
        .await;
    let verdicts: Vec<&MessageDto> = messages
        .iter()
        .filter(|m| m.kind == MessageKind::Approve || m.kind == MessageKind::RequestChanges)
        .collect();
    assert_eq!(verdicts.len(), 2, "{messages:?}");
    assert!(
        verdicts
            .iter()
            .all(|m| m.from_agent_id.as_deref() == Some(cast.reviewer.id.as_str()))
    );
}

/// Opening a request runs `gh pr create` once, with the pushed branch, the
/// base, the title and the body; a second call answers the same URL and
/// opens nothing. An unpushed branch is refused. The task stays approved,
/// its author keeps the request with the `pr-babysit` skill, and the task
/// finishes only once a human merged the request.
#[tokio::test]
async fn opening_a_pull_request_runs_the_forge_cli_once_and_keeps_the_task_until_the_merge() {
    const URL: &str = "https://github.com/acme/widgets/pull/12";
    let cli = stub_forge_cli(forge_script(URL));
    let h = harness()
        .scheduler()
        .discover_agents()
        .forge_cli(&cli)
        .await;
    h.git_repo("repo");
    let cast = h.active_cast_ending_in(Landing::PullRequest).await;
    with_forge(&h, &cast.repo).await;
    let task = cast.task.clone();
    let (worktree, author) = walk_to_approved(&h, &task, &cast.reviewer.id).await;

    // The briefing is the publishing procedure, and only that: no squash onto
    // the base for the author to run by mistake, and no forge CLI named.
    let told = h.told(&author.id);
    assert!(told.contains("`open_pull_request`"), "{told}");
    for named in [
        "reset --soft".to_string(),
        format!("merge --ff-only {}", task.branch),
        "`gh`".to_string(),
        "`glab`".to_string(),
    ] {
        assert!(!told.contains(&named), "{named}: {told}");
    }

    let open = |title: &str, body: &str| {
        as_session(
            &format!("/v1/tasks/{}/pull-request", task.id),
            &author.id,
            serde_json::json!({"title": title, "body": body}),
        )
    };

    // An unpushed branch is refused with 409 that says to push first.
    let refusal = h
        .error(open("feat: ship it", "Ships it."), StatusCode::CONFLICT)
        .await;
    assert!(
        refusal.error.message.contains("push"),
        "{}",
        refusal.error.message
    );

    // Pushed, the daemon opens the request itself and stores the URL.
    sh(&worktree, "git push origin HEAD");
    let published: TaskDto = h
        .json(open("feat: ship it", "Ships it."), StatusCode::OK)
        .await;
    assert_eq!(published.pr_url.as_deref(), Some(URL));
    let creates: Vec<_> = cli
        .invocations()
        .into_iter()
        .filter(|call| {
            call.args
                .starts_with(&["pr".to_string(), "create".to_string()])
        })
        .collect();
    assert_eq!(creates.len(), 1, "{creates:?}");
    for value in [
        "github.com/acme/widgets",
        task.branch.as_str(),
        "main",
        "feat: ship it",
        "Ships it.",
    ] {
        assert!(
            creates[0].args.iter().any(|a| a == value),
            "{value}: {:?}",
            creates[0].args
        );
    }

    // One request per task: a second call answers the same URL and the stub
    // sees no second create.
    let again: TaskDto = h
        .json(open("feat: ship it", "Ships it."), StatusCode::OK)
        .await;
    assert_eq!(again.pr_url.as_deref(), Some(URL));
    assert_eq!(
        cli.invocations()
            .iter()
            .filter(|call| call
                .args
                .starts_with(&["pr".to_string(), "create".to_string()]))
            .count(),
        1,
        "a second call opened another request"
    );

    // The author keeps the request: the skill is loaded for it, and the
    // task stays approved until a human merges the request.
    let launch = h.launch_file(&author.id).expect("a launch file");
    assert!(
        launch.system_prompt.contains("- pr-babysit: "),
        "{}",
        launch.system_prompt
    );
    assert_eq!(h.status(&task.id).await, TaskStatus::Approved);
    let tip = sh(&worktree, "git rev-parse HEAD");
    let finish = || {
        as_session(
            &format!("/v1/tasks/{}/transitions", task.id),
            &author.id,
            serde_json::json!({"to": "finished", "merge_commit": tip}),
        )
    };
    let refused = h.error(finish(), StatusCode::CONFLICT).await;
    assert!(
        refused.error.message.contains("is not merged"),
        "{}",
        refused.error.message
    );
    let kept = h
        .store
        .pull_request_of_task(&task.id)
        .await
        .unwrap()
        .expect("the ledger holds the task's request");
    h.store
        .set_pull_request_state(&kept.id, "merged")
        .await
        .unwrap();
    let finished: TaskDto = h.json(finish(), StatusCode::OK).await;
    assert_eq!(finished.status, TaskStatus::Finished);
    assert_eq!(finished.merge_commit.as_deref(), Some(tip.as_str()));
}

/// A task of a repository whose forge integration is off is refused with
/// 409, and opens nothing: nothing would read the request for its author.
#[tokio::test]
async fn opening_a_pull_request_refuses_with_the_integration_off() {
    let cli = stub_forge_cli(forge_script("https://github.com/acme/widgets/pull/12"));
    let h = harness()
        .scheduler()
        .discover_agents()
        .forge_cli(&cli)
        .await;
    h.git_repo("repo");
    let cast = h.active_cast_ending_in(Landing::PullRequest).await;
    with_forge_enabled(&h, &cast.repo, false).await;
    let task = cast.task.clone();
    let (worktree, author) = walk_to_approved(&h, &task, &cast.reviewer.id).await;
    sh(&worktree, "git push origin HEAD");

    let refusal = h
        .error(
            as_session(
                &format!("/v1/tasks/{}/pull-request", task.id),
                &author.id,
                serde_json::json!({"title": "feat: ship it", "body": "Ships it."}),
            ),
            StatusCode::CONFLICT,
        )
        .await;
    assert_eq!(refusal.error.code, "forge_disabled");
    assert!(
        !cli.invocations().iter().any(|call| call
            .args
            .starts_with(&["pr".to_string(), "create".to_string()])),
        "nothing was opened"
    );
}

/// A task of a repository with no detected forge is refused with 409.
#[tokio::test]
async fn opening_a_pull_request_refuses_with_no_forge_detected() {
    let (h, cast) = seeded(Landing::PullRequest).await;
    let task = cast.task.clone();
    let (_worktree, author) = walk_to_approved(&h, &task, &cast.reviewer.id).await;

    let refusal = h
        .error(
            as_session(
                &format!("/v1/tasks/{}/pull-request", task.id),
                &author.id,
                serde_json::json!({"title": "feat: ship it", "body": "Ships it."}),
            ),
            StatusCode::CONFLICT,
        )
        .await;
    assert!(
        refusal.error.message.contains("no forge"),
        "{}",
        refusal.error.message
    );
}

/// A CLI whose `auth status` exits 1 is refused with 409 and the probe's own
/// message.
#[tokio::test]
async fn opening_a_pull_request_refuses_an_unauthenticated_cli() {
    let cli = stub_forge_cli(serde_json::json!([answer(
        &["auth", "status"],
        1,
        "not logged in"
    )]));
    let h = harness()
        .scheduler()
        .discover_agents()
        .forge_cli(&cli)
        .await;
    h.git_repo("repo");
    let cast = h.active_cast_ending_in(Landing::PullRequest).await;
    with_forge(&h, &cast.repo).await;
    let task = cast.task.clone();
    let (worktree, author) = walk_to_approved(&h, &task, &cast.reviewer.id).await;
    sh(&worktree, "git push origin HEAD");

    let refusal = h
        .error(
            as_session(
                &format!("/v1/tasks/{}/pull-request", task.id),
                &author.id,
                serde_json::json!({"title": "feat: ship it", "body": "Ships it."}),
            ),
            StatusCode::CONFLICT,
        )
        .await;
    assert!(
        refusal.error.message.contains("not logged in"),
        "{}",
        refusal.error.message
    );
}

/// An approval that lands while the author's agent is still coming up is not
/// lost. The agent is launched — the row says so — before it has said its
/// own session start, and a resume in that window finds no conversation to
/// go back to; the spawn it falls back to is then refused, since the seat is
/// still the starting session's. The briefing must still go out once the
/// agent is heard from, rather than being counted as sent by the attempt
/// that could not send it — and it has gone out only once the agent has it:
/// the relaunch that carries it can still fail.
#[tokio::test]
async fn an_approval_during_the_authors_start_still_briefs_it_to_land() {
    let (h, cast) = seeded(Landing::Merge).await;
    let task = cast.task.clone();
    // An agent a few seconds slow to come up, from the next launch on:
    // discovery has already accepted the stub, and every session the daemon
    // starts from here reads this script.
    // Still the agent that keeps its conversations, as the harness's own
    // script is: the briefing goes out on a resume of the one it starts.
    let mut slow = common::acp::script();
    slow["start_delay"] = serde_json::json!(3.0);
    slow["stored_sessions"] = serde_json::json!(["stub-session"]);
    h.agent.reprogram(slow);

    // Launched, on the launcher's own word alone: nothing heard from it yet.
    h.reconcile_task_until(&task.id, TIMEOUT, "the author to be launched", async || {
        h.status(&task.id).await == TaskStatus::InProgress
            && h.sessions_of(&task.id)
                .await
                .iter()
                .any(|s| s.seat() == Some(Seat::Author) && s.launched_at.is_some())
    })
    .await;
    approve(&h, &task, &cast.reviewer.id).await;

    h.reconcile_task_until(
        &task.id,
        TIMEOUT,
        "the author to be briefed to land it",
        async || {
            h.status(&task.id).await == TaskStatus::Approved
                && h.running_session(&task.id, Seat::Author)
                    .await
                    .is_some_and(|s| h.prompted(&s).contains("# Land task:"))
        },
    )
    .await;
}

/// An approved task whose author cannot go back to its conversation — the
/// agent never saved it, or has it no longer — is not failed for it. The
/// resume that would brief it to land is refused and dies on arrival, and
/// resuming the same conversation again would only be refused again until
/// the retry budget ran out. A fresh author takes over instead, briefed on
/// the task and then to land it.
#[tokio::test]
async fn an_author_that_cannot_reopen_its_conversation_is_started_afresh_to_land() {
    let (h, cast) = seeded(Landing::Merge).await;
    let task = cast.task.clone();
    h.reconcile_task_until(&task.id, TIMEOUT, "the author to be spawned", async || {
        h.status(&task.id).await == TaskStatus::InProgress
            && h.running_session(&task.id, Seat::Author)
                .await
                .is_some_and(|s| s.internal_session_id.is_some())
    })
    .await;
    let first = h
        .running_session(&task.id, Seat::Author)
        .await
        .expect("a live author session");
    // From the next launch on, the agent keeps no conversation to reopen.
    h.agent.reprogram(common::acp::script());

    approve(&h, &task, &cast.reviewer.id).await;

    h.reconcile_task_until(
        &task.id,
        TIMEOUT,
        "a fresh author to be briefed to land it",
        async || {
            h.status(&task.id).await == TaskStatus::Approved
                && h.running_session(&task.id, Seat::Author)
                    .await
                    .is_some_and(|s| s.id != first.id && h.prompted(&s).contains("# Land task:"))
        },
    )
    .await;
    let fresh = h
        .running_session(&task.id, Seat::Author)
        .await
        .expect("a live author session");
    let prompted = h.prompted(&fresh);
    assert!(
        prompted.contains(&task.title),
        "the fresh author is briefed on its task too: {prompted}"
    );
    assert_eq!(
        h.agent.calls_of("session/resume").len(),
        1,
        "the refused conversation is not resumed again"
    );
}

#[tokio::test]
async fn feature_tasks_land_on_the_goal_branch_and_keep_the_base_unchanged() {
    let h = harness().scheduler().discover_agents().await;
    let path = h.git_repo("repo");
    let repo = h.repository(&path).await;
    let spare_path = h.git_repo("spare");
    let spare = h.repository(&spare_path).await;
    h.store
        .update_repository(
            &spare.id,
            ariadne_store::RepositoryUpdate {
                base_branch: Some("next".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let remote = h.at("remote.git");
    sh(
        &path,
        &format!(
            "git init -q --bare '{}' && git remote add origin '{}'",
            remote.display(),
            remote.display()
        ),
    );
    let goal = h
        .store
        .create_goal(ariadne_store::NewGoal {
            issue_url: None,
            title: "Ship feature".into(),
            description: String::new(),
            repository_ids: vec![repo.id.clone(), spare.id.clone()],
            pin: test_pin(),
            landing: Some(Landing::FeatureBranch),
        })
        .await
        .unwrap();
    let first = h.task_on(&goal, &repo, "First change", 1, test_pin()).await;
    let second = h
        .store
        .create_task(ariadne_store::NewTask {
            goal_id: goal.id.clone(),
            repo_id: repo.id.clone(),
            title: "Second change".into(),
            description: String::new(),
            agents: vec![
                NewTaskAgent::new(Seat::Author, ["coding"], test_pin()),
                NewTaskAgent::new(Seat::Reviewer, ["code-review"], test_pin()),
            ],
            depends_on: vec![first.id.clone()],
        })
        .await
        .unwrap();
    // Each repository needs a final task before the plan can start.
    for (final_repo, depends_on) in [
        (&repo, vec![first.id.clone(), second.id.clone()]),
        (&spare, vec![]),
    ] {
        h.store
            .create_task(ariadne_store::NewTask {
                goal_id: goal.id.clone(),
                repo_id: final_repo.id.clone(),
                title: "Open the request".into(),
                description: String::new(),
                agents: vec![NewTaskAgent::new(Seat::Author, ["coding"], test_pin())],
                depends_on,
            })
            .await
            .unwrap();
    }
    let orchestrator = h.orchestrator_session(&goal).await;
    let before: serde_json::Value = h.get(&format!("/v1/goals/{}", goal.id)).await;
    assert!(
        before["repos"][0]
            .get("goal_branch")
            .is_some_and(serde_json::Value::is_null)
    );
    let finalized: serde_json::Value = h
        .json(
            as_session(
                &format!("/v1/goals/{}/finalize", goal.id),
                &orchestrator.id,
                serde_json::json!({}),
            ),
            StatusCode::OK,
        )
        .await;
    let branch = format!("ship-feature-{}", &goal.id[goal.id.len() - 6..]);
    for entry in finalized["repos"].as_array().unwrap() {
        assert_eq!(entry["goal_branch"], branch);
        let checkout = PathBuf::from(entry["path"].as_str().unwrap());
        assert_eq!(
            sh(&checkout, &format!("git rev-parse {branch}")),
            sh(
                &checkout,
                &format!("git rev-parse {}", entry["base_branch"].as_str().unwrap())
            )
        );
    }
    assert_eq!(
        sh(&path, &format!("git rev-parse {branch}")),
        sh(&remote, &format!("git rev-parse {branch}"))
    );
    let base = sh(&path, "git rev-parse main");
    std::fs::write(path.join("file.txt"), "user edits\n").unwrap();
    let mut previous = base.clone();
    for (index, task) in [first, second].iter().enumerate() {
        h.reconcile_task_until(&task.id, TIMEOUT, "the author to start", async || {
            h.status(&task.id).await == TaskStatus::InProgress
                && h.running_session(&task.id, Seat::Author).await.is_some()
        })
        .await;
        let author = h.running_session(&task.id, Seat::Author).await.unwrap();
        let worktree = PathBuf::from(author.worktree_path.as_ref().unwrap());
        assert_eq!(sh(&worktree, "git rev-parse HEAD"), previous);
        assert!(h.told(&author.id).contains(&format!("onto {branch}")));
        sh(
            &worktree,
            &format!(
                "echo change > change{index}.txt && git add . && git -c user.email=t@t -c user.name=t commit -qm 'feat: add change'"
            ),
        );
        let reviewer = h
            .store
            .list_task_reviewers(&task.id)
            .await
            .unwrap()
            .remove(0);
        h.store
            .transition_task(&task.id, TaskStatus::UnderReview, Actor::Author, None, None)
            .await
            .unwrap();
        h.reconcile_task_until(&task.id, TIMEOUT, "the reviewer to start", async || {
            h.running_session(&task.id, Seat::Reviewer).await.is_some()
        })
        .await;
        let reviewing = h.running_session(&task.id, Seat::Reviewer).await.unwrap();
        assert!(h.told(&reviewing.id).contains(&format!("onto {branch}")));
        h.verdict(task, &reviewer.id, MessageKind::Approve, "looks right")
            .await;
        h.reconcile_task_until(&task.id, TIMEOUT, "the landing briefing", async || {
            h.status(&task.id).await == TaskStatus::Approved
                && h.told(&author.id).contains("# Land task:")
        })
        .await;
        assert!(
            h.told(&author.id)
                .contains(&format!("git merge-base {branch} HEAD"))
        );
        let (status, body) = h.send(get(&format!("/v1/tasks/{}/diff", task.id))).await;
        assert_eq!(status, StatusCode::OK);
        let diff = String::from_utf8(body).unwrap();
        assert!(diff.contains(&format!("change{index}.txt")));
        if index == 1 {
            assert!(!diff.contains("change0.txt"));
        }
        let sha = sh(&worktree, "git rev-parse HEAD");
        if index == 0 {
            // A commit present only on the repository base does not land this task.
            sh(&path, &format!("git update-ref refs/heads/main {sha}"));
            h.error(
                as_session(
                    &format!("/v1/tasks/{}/transitions", task.id),
                    &author.id,
                    serde_json::json!({"to": "finished", "merge_commit": sha}),
                ),
                StatusCode::CONFLICT,
            )
            .await;
            sh(&path, &format!("git update-ref refs/heads/main {base}"));
        }
        let briefing = h.told(&author.id);
        let merge = briefing
            .split('`')
            .skip(1)
            .step_by(2)
            .find(|command| command.contains(&format!("fetch . {}:{branch}", task.branch)))
            .expect("the landing briefing carries the fetch command");
        sh(&worktree, merge);
        assert_eq!(sh(&path, "git branch --show-current"), "main");
        assert_eq!(
            std::fs::read_to_string(path.join("file.txt")).unwrap(),
            "user edits\n"
        );
        if index == 0 {
            let base_only = sh(&path, "git rev-parse next");
            sh(
                &path,
                &format!("git update-ref refs/heads/main {base_only}"),
            );
            h.error(
                as_session(
                    &format!("/v1/tasks/{}/transitions", task.id),
                    &author.id,
                    serde_json::json!({"to": "finished", "merge_commit": base_only}),
                ),
                StatusCode::CONFLICT,
            )
            .await;
            sh(&path, &format!("git update-ref refs/heads/main {base}"));
        }
        let landed: TaskDto = h
            .json(
                as_session(
                    &format!("/v1/tasks/{}/transitions", task.id),
                    &author.id,
                    serde_json::json!({"to": "finished", "merge_commit": sha}),
                ),
                StatusCode::OK,
            )
            .await;
        assert_eq!(landed.status, TaskStatus::Finished);
        h.launcher.cleanup_task(&task.id, true, true).await.unwrap();
        assert_eq!(sh(&path, &format!("git rev-parse {branch}")), sha);
        assert_eq!(sh(&path, "git rev-parse main"), base);
        previous = sha;
    }
}
