//! What an approved task does, driven by the scheduler over a real git
//! repository.
//!
//! The approvals leave the task with the engineer that wrote it: the same
//! session, the same worktree, briefed with the landing instructions its
//! repository's merge strategy names. From there it has two ways out, and
//! both are the engineer's own — `mark_merged` once the change is on the base
//! branch, and `request_review` for a revision the people on a published
//! request asked for, which the Ariadne reviewers judge like any other round.
//!
//! No tmux and no agent CLI: `tmux` is a stub script that answers "no such
//! session" and records what it was asked for, so the sessions here are rows
//! and spawn plans rather than panes. `git` is real, and so is the merge the
//! daemon verifies before accepting it — the agent doing the rebase, the
//! squash and the fast-forward is the test itself, running the commands its
//! briefing tells the agent to run.

mod common;

use std::path::PathBuf;
use std::time::Duration;

use axum::http::StatusCode;

use ariadne_api::reviews::ReviewDto;
use ariadne_api::tasks::TaskDto;
use ariadne_core::{Actor, AttentionReason, MergeStrategy, ReviewVerdict, Role, TaskStatus};
use ariadne_store::{AgentSession, NewReview, RepositoryUpdate, Repository, ReviewerSlot, Task};

use common::{Cast, Harness, as_session, eventually, get, harness, sh};

/// How long a test waits for the scheduler to reach a state.
const TIMEOUT: Duration = Duration::from_secs(20);


/// A goal on a real repository, active, with one task on it. The profiles are
/// pinned to an agent kind: the internal session id a resume needs is one the
/// Claude adapter chooses at spawn, so the resume paths here are the ones a
/// real session takes.
async fn seeded(strategy: MergeStrategy) -> (Harness, Cast) {
    let h = harness().scheduler().await;
    h.git_repo("repo");
    let cast = h.active_cast().await;
    if strategy != MergeStrategy::Direct {
        h.store
            .update_repository(
                &cast.repo.id,
                RepositoryUpdate {
                    merge_strategy: Some(strategy),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
    }
    (h, cast)
}

fn repo_path(repo: &Repository) -> PathBuf {
    PathBuf::from(&repo.path)
}

/// The engineer asks for review and the reviewer approves it.
async fn approve(h: &Harness, task: &Task, reviewer: &str) {
    let task = h
        .store
        .transition_task(
            &task.id,
            TaskStatus::UnderReview,
            Actor::Engineer,
            None,
            None,
        )
        .await
        .unwrap();
    h.store
        .create_review(NewReview {
            task_id: task.id.clone(),
            round: task.review_round,
            reviewer_profile_id: reviewer.to_string(),
            session_id: None,
            verdict: ReviewVerdict::Approve,
            body: Some("looks right".into()),
        })
        .await
        .unwrap();
    h.notify(&task.id);
}

/// Walk a fresh task to the engineer landing it: the engineer commits
/// something, the reviewer approves, the scheduler does the rest. Returns the
/// engineer's worktree — which it never gave up — and the session that has
/// been briefed to land the change.
async fn walk_to_approved(h: &Harness, task: &Task, reviewer: &str) -> (PathBuf, AgentSession) {
    let heading = format!("# Land task: {}", task.title);
    walk_to_landing(h, task, reviewer, &heading).await
}

/// The same walk, waiting for a landing briefing that opens on `briefed`:
/// what the engineer is handed is the repository's text, so a repository with
/// one of its own is picked up with words the defaults never contain.
async fn walk_to_landing(
    h: &Harness,
    task: &Task,
    reviewer: &str,
    briefed: &str,
) -> (PathBuf, AgentSession) {
    h.notify(&task.id);
    eventually(TIMEOUT, "the engineer to be spawned", async || {
        h.status(&task.id).await == TaskStatus::InProgress
            && h.running_session(&task.id, Role::Engineer).await.is_some()
    })
    .await;
    let writing = h
        .running_session(&task.id, Role::Engineer)
        .await
        .expect("a live engineer session");

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

    eventually(TIMEOUT, "the engineer to be briefed to land it", async || {
        h.status(&task.id).await == TaskStatus::Approved
            && h.running_session(&task.id, Role::Engineer)
                .await
                .is_some_and(|s| h.spawn_argv(&s.id).contains(briefed))
    })
    .await;
    let landing = h
        .running_session(&task.id, Role::Engineer)
        .await
        .expect("a live engineer session");
    assert_eq!(
        landing.id, writing.id,
        "the session that wrote the change is the one landing it"
    );
    (worktree, landing)
}

/// The whole of it, the way `direct` says: the approvals leave the task with
/// its engineer — same session, same worktree, briefed to land it —
/// rebase-squash-fast-forward, `mark_merged` accepted, cleanup, dependents
/// woken.
#[tokio::test]
async fn an_approved_task_is_landed_by_its_own_engineer() {
    let (h, cast) = seeded(MergeStrategy::Direct).await;
    let task = cast.task.clone();
    let dependent = h
        .store
        .create_task(ariadne_store::NewTask {
            goal_id: cast.goal.id.clone(),
            repo_id: cast.repo.id.clone(),
            title: "Use what the first one built".into(),
            description: "do things".into(),
            engineer_profile_id: cast.engineer.id.clone(),
            pin: None,
            reviewers: vec![ReviewerSlot::of(&cast.reviewer.id)],
            depends_on: vec![task.id.clone()],
        })
        .await
        .unwrap();
    let (worktree, engineer) = walk_to_approved(&h, &task, &cast.reviewer.id).await;

    // Nobody took the branch: the worktree the change was written in is still
    // the task's, still on the branch, and still on disk.
    assert!(worktree.exists(), "the engineer lost its worktree");
    assert_eq!(
        h.store
            .get_task(&task.id)
            .await
            .unwrap()
            .worktree_path
            .as_deref(),
        Some(worktree.display().to_string().as_str())
    );
    assert_eq!(sh(&worktree, "git rev-parse --abbrev-ref HEAD"), task.branch);

    // And the briefing it was picked up with is this repository's procedure,
    // whole: the squash it is about to run, and not a word of the forge half
    // it would have had to skip.
    let argv = h.spawn_argv(&engineer.id);
    assert!(
        argv.contains("git reset --soft main"),
        "the landing briefing does not carry the squash: {argv}"
    );
    for published in ["gh ", "glab ", "pull request", "merge request"] {
        assert!(
            !argv.contains(published),
            "the direct landing briefing names {published}: {argv}"
        );
    }

    // What the briefing tells it to do, done: rebase, squash, fast-forward.
    sh(&worktree, "git rebase -q main");
    sh(
        &worktree,
        "git reset --soft main && \
         git -c user.email=t@t -c user.name=t commit -qm 'feat(board): render it'",
    );
    let repo = repo_path(&cast.repo);
    sh(&repo, &format!("git merge -q --ff-only {}", task.branch));
    let sha = sh(&repo, "git rev-parse main");

    let landed: TaskDto = h
        .json(
            as_session(
                &format!("/v1/tasks/{}/transitions", task.id),
                &engineer.id,
                serde_json::json!({"to": "merged", "merge_commit": sha}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(landed.status, TaskStatus::Merged);
    assert_eq!(landed.merge_commit.as_deref(), Some(sha.as_str()));

    // Cleanup takes the worktree with it, and the task that was waiting on
    // this one starts.
    eventually(TIMEOUT, "the cleanup and the dependent task", async || {
        !worktree.exists()
            && matches!(
                h.status(&dependent.id).await,
                TaskStatus::Ready | TaskStatus::InProgress
            )
    })
    .await;
}

/// The landing briefing belongs to the repository: a text written on it is
/// what its engineer is picked up with, rendered with this task's values, and
/// the merge strategy's default is only what stands while there is none.
#[tokio::test]
async fn an_approved_engineer_is_briefed_with_the_repositorys_own_landing_text() {
    let (h, cast) = seeded(MergeStrategy::Direct).await;
    let task = cast.task.clone();
    h.store
        .update_repository(
            &cast.repo.id,
            RepositoryUpdate {
                landing_prompt: Some(Some(
                    "Ship {task_title}: {branch} onto {base_branch} in {repo_path}.".into(),
                )),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    let heading = format!("Ship {}:", task.title);
    let (_worktree, engineer) = walk_to_landing(&h, &task, &cast.reviewer.id, &heading).await;

    let argv = h.spawn_argv(&engineer.id);
    assert!(
        argv.contains(&format!(
            "Ship {}: {} onto main in {}.",
            task.title, task.branch, cast.repo.path
        )),
        "the repository's landing text did not reach the engineer, filled in: {argv}"
    );
    // And nothing of the default it replaced.
    assert!(!argv.contains("git reset --soft main"), "{argv}");
}

/// A merge nobody made is refused, under either procedure: the daemon checks
/// the sha really is on the base branch of the primary checkout before it
/// believes it, and the tip of the task branch is not.
///
/// That check is what keeps a reported sha worth anything, and it is the same
/// check on both paths — a squash on the forge leaves no branch on the base
/// either, so a daemon that trusted the caller would accept both.
#[tokio::test]
async fn a_merge_that_never_happened_is_refused() {
    for strategy in [MergeStrategy::Direct, MergeStrategy::PullRequest] {
        let (h, cast) = seeded(strategy).await;
        let (worktree, engineer) = walk_to_approved(&h, &cast.task, &cast.reviewer.id).await;

        // The tip of the branch: real, and nowhere near the base branch.
        let sha = sh(&worktree, "git rev-parse HEAD");
        let (status, body) = h
            .send(as_session(
                &format!("/v1/tasks/{}/transitions", cast.task.id),
                &engineer.id,
                serde_json::json!({"to": "merged", "merge_commit": sha}),
            ))
            .await;
        assert_eq!(status, StatusCode::CONFLICT, "{strategy:?}");
        let message = String::from_utf8_lossy(&body);
        assert!(message.contains("merge not verified"), "{message}");
        assert_eq!(h.status(&cast.task.id).await, TaskStatus::Approved);
    }
}

/// The other way out of `approved`: the people reading a published request
/// asked for something, the engineer made it, and the revision goes back to
/// the Ariadne reviewers like any other round — from `approved`, which is
/// where a task being landed sits.
#[tokio::test]
async fn a_revision_of_a_published_request_goes_back_to_the_reviewers() {
    let (h, cast) = seeded(MergeStrategy::Direct).await;
    let task = cast.task.clone();
    let (_worktree, engineer) = walk_to_approved(&h, &task, &cast.reviewer.id).await;

    // The request it published is recorded by the engineer, and only by it.
    const URL: &str = "https://github.com/owner/repo/pull/12";
    let published: TaskDto = h
        .json(
            as_session(
                &format!("/v1/tasks/{}/pull-request", task.id),
                &engineer.id,
                serde_json::json!({"url": URL}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(published.pr_url.as_deref(), Some(URL));

    let reviewer_session = h
        .session(&cast.goal, Some(&task), Role::Reviewer, &cast.reviewer.id)
        .await;
    let (status, refusal) = h
        .send(as_session(
            &format!("/v1/tasks/{}/pull-request", task.id),
            &reviewer_session.id,
            serde_json::json!({"url": URL}),
        ))
        .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "only its engineer records it"
    );
    let refusal = String::from_utf8_lossy(&refusal);
    assert!(refusal.contains("only the engineer"), "{refusal}");

    // And the revision it made for them is reviewed like any other round.
    let round = h.store.get_task(&task.id).await.unwrap().review_round;
    let revised: TaskDto = h
        .json(
            as_session(
                &format!("/v1/tasks/{}/transitions", task.id),
                &engineer.id,
                serde_json::json!({
                    "to": "under_review",
                    "reason": "answered every comment on the request",
                }),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(revised.status, TaskStatus::UnderReview);
    assert_eq!(revised.review_round, round + 1);
    assert_eq!(
        revised.pr_url.as_deref(),
        Some(URL),
        "the request it is a revision of is still the task's"
    );

    // The reviewers judge it, and the approval hands it back to the engineer
    // to finish landing.
    h.store
        .create_review(NewReview {
            task_id: task.id.clone(),
            round: revised.review_round,
            reviewer_profile_id: cast.reviewer.id.clone(),
            session_id: None,
            verdict: ReviewVerdict::Approve,
            body: Some("the answers read right".into()),
        })
        .await
        .unwrap();
    h.notify(&task.id);
    eventually(TIMEOUT, "the task to come back to its engineer", async || {
        h.status(&task.id).await == TaskStatus::Approved
    })
    .await;

    // One round of verdicts per reviewer, both rounds readable.
    let reviews: Vec<ReviewDto> = h
        .json(get(&format!("/v1/tasks/{}/reviews", task.id)), StatusCode::OK)
        .await;
    assert_eq!(reviews.len(), 2, "{reviews:?}");
    assert!(
        reviews
            .iter()
            .all(|r| r.reviewer_profile_id == cast.reviewer.id)
    );
}

/// A request the forge squashed leaves no branch on the base at all, so what
/// the daemon checks there is the other half of the engineer's last step: the
/// sha it reports is on the base branch of the primary checkout.
#[tokio::test]
async fn a_squashed_request_lands_on_the_sha_the_engineer_fast_forwarded_to() {
    let (h, cast) = seeded(MergeStrategy::PullRequest).await;
    let task = cast.task.clone();
    let (worktree, engineer) = walk_to_approved(&h, &task, &cast.reviewer.id).await;

    // The briefing is the publishing procedure, and only that: no squash onto
    // the base for the engineer to run by mistake.
    let argv = h.spawn_argv(&engineer.id);
    assert!(
        argv.contains("gh pr create --base main"),
        "the engineer was not briefed to publish it: {argv}"
    );
    for squashed in [
        "reset --soft".to_string(),
        format!("merge --ff-only {}", task.branch),
    ] {
        assert!(
            !argv.contains(&squashed),
            "the published landing briefing names {squashed}: {argv}"
        );
    }

    // Publishing it is the engineer's next step, and reporting the URL is
    // what hands the task to a human: nobody but them can merge a request, so
    // the strip has to say so. Nothing said it before the report.
    const URL: &str = "https://github.com/owner/repo/pull/12";
    assert_eq!(
        h.attention(&engineer).await,
        None,
        "an engineer that has published nothing yet is nobody's to answer"
    );
    let published: TaskDto = h
        .json(
            as_session(
                &format!("/v1/tasks/{}/pull-request", task.id),
                &engineer.id,
                serde_json::json!({"url": URL}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(published.pr_url.as_deref(), Some(URL));
    assert_eq!(
        h.attention(&engineer).await,
        Some(AttentionReason::WaitingUser),
        "a published request is the user's to merge, and the strip says so"
    );

    // And it stays up while the engineer polls: what it reports is the agent
    // working, which was never what the flag was about.
    h.store
        .clear_agent_attention(&engineer.id)
        .await
        .expect("an agent event that changes nothing is not an error");
    assert_eq!(
        h.attention(&engineer).await,
        Some(AttentionReason::WaitingUser),
        "the agent polling its own request does not answer for the user"
    );

    // What a squash merge on the forge leaves behind, reproduced with git: a
    // commit on the base that no branch points at, and a task branch that is
    // not its ancestor.
    let repo = repo_path(&cast.repo);
    sh(
        &repo,
        &format!(
            "git merge -q --squash {} && \
             git -c user.email=t@t -c user.name=t commit -qm 'feat(board): render it (#12)'",
            task.branch
        ),
    );
    let sha = sh(&repo, "git rev-parse main");
    assert_ne!(sha, sh(&worktree, "git rev-parse HEAD"));

    let landed: TaskDto = h
        .json(
            as_session(
                &format!("/v1/tasks/{}/transitions", task.id),
                &engineer.id,
                serde_json::json!({"to": "merged", "merge_commit": sha}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(landed.status, TaskStatus::Merged);
    assert_eq!(landed.merge_commit.as_deref(), Some(sha.as_str()));
}
