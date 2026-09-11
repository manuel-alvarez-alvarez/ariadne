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

mod common;

use std::path::PathBuf;
use std::time::Duration;

use axum::http::StatusCode;

use ariadne_api::messages::MessageDto;
use ariadne_api::tasks::TaskDto;
use ariadne_core::{Actor, AttentionReason, Landing, MessageKind, Seat, TaskStatus};
use ariadne_store::{AgentSession, NewTaskAgent, Repository, Task};

use common::{Cast, Harness, as_session, eventually, get, harness, sh, test_pin};

/// How long a test waits for the scheduler to reach a state.
const TIMEOUT: Duration = Duration::from_secs(20);

/// A goal on a real repository, active, with one task on it ending in
/// `landing`. The agents run on the stub, which discovery has accepted, so
/// the resume paths here are the ones a real session takes.
async fn seeded(landing: Landing) -> (Harness, Cast) {
    let h = harness().scheduler().discover_agents().await;
    h.git_repo("repo");
    let mut cast = h.active_cast().await;
    if landing != Landing::Merge {
        // How a task ends is the task's own, agreed with the user when it is
        // written, so this sets it the way an orchestrator would.
        cast.task = h
            .store
            .update_task(
                &cast.task.id,
                ariadne_store::TaskUpdate {
                    landing: Some(landing),
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

/// The author asks for review and the reviewer approves it.
async fn approve(h: &Harness, task: &Task, reviewer: &str) {
    let task = h
        .store
        .transition_task(&task.id, TaskStatus::UnderReview, Actor::Author, None, None)
        .await
        .unwrap();
    h.verdict(&task, reviewer, MessageKind::Approve, "looks right")
        .await;
    h.notify(&task.id);
}

/// Walk a fresh task to the author landing it: the author commits
/// something, the reviewer approves, the scheduler does the rest. Returns the
/// author's worktree — which it never gave up — and the session that has
/// been briefed to land the change.
async fn walk_to_approved(h: &Harness, task: &Task, reviewer: &str) -> (PathBuf, AgentSession) {
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
    h.notify(&task.id);
    eventually(TIMEOUT, "the author to be spawned", async || {
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

    eventually(TIMEOUT, "the author to be briefed to land it", async || {
        h.status(&task.id).await == TaskStatus::Approved
            && h.running_session(&task.id, Seat::Author)
                .await
                .is_some_and(|s| h.told(&s.id).contains(briefed))
    })
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
            landing: None,
            permission_mode: None,
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
        argv.contains("git reset --soft main"),
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
    eventually(TIMEOUT, "the cleanup and the dependent task", async || {
        !worktree.exists()
            && matches!(
                h.status(&dependent.id).await,
                TaskStatus::Ready | TaskStatus::InProgress
            )
    })
    .await;
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
    for strategy in [Landing::Merge, Landing::PullRequest] {
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
}

/// The other way out of `approved`: the people reading a published request
/// asked for something, the author made it, and the revision goes back to
/// the Ariadne reviewers like any other round — from `approved`, which is
/// where a task being landed sits.
#[tokio::test]
async fn a_revision_of_a_published_request_goes_back_to_the_reviewers() {
    let (h, cast) = seeded(Landing::Merge).await;
    let task = cast.task.clone();
    let (_worktree, author) = walk_to_approved(&h, &task, &cast.reviewer.id).await;

    // The request it published is recorded by the author, and only by it.
    const URL: &str = "https://github.com/owner/repo/pull/12";
    let published: TaskDto = h
        .json(
            as_session(
                &format!("/v1/tasks/{}/pull-request", task.id),
                &author.id,
                serde_json::json!({"url": URL}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(published.pr_url.as_deref(), Some(URL));

    let reviewer_session = h
        .session(&cast.goal, Some(&task), Seat::Reviewer, &cast.reviewer.id)
        .await;
    let (status, refusal) = h
        .send(as_session(
            &format!("/v1/tasks/{}/pull-request", task.id),
            &reviewer_session.id,
            serde_json::json!({"url": URL}),
        ))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "only its author records it");
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
    // to finish landing.
    h.verdict(
        &task,
        &cast.reviewer.id,
        MessageKind::Approve,
        "the answers read right",
    )
    .await;
    h.notify(&task.id);
    eventually(TIMEOUT, "the task to come back to its author", async || {
        h.status(&task.id).await == TaskStatus::Approved
    })
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

/// A request the forge squashed leaves no branch on the base at all, so what
/// the daemon checks there is the other half of the author's last step: the
/// sha it reports is on the base branch of the primary checkout.
#[tokio::test]
async fn a_squashed_request_lands_on_the_sha_the_author_fast_forwarded_to() {
    let (h, cast) = seeded(Landing::PullRequest).await;
    let task = cast.task.clone();
    let (worktree, author) = walk_to_approved(&h, &task, &cast.reviewer.id).await;

    // The briefing is the publishing procedure, and only that: no squash onto
    // the base for the author to run by mistake.
    let argv = h.told(&author.id);
    assert!(
        argv.contains("gh pr create --base main"),
        "the author was not briefed to publish it: {argv}"
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

    // Publishing it is the author's next step, and reporting the URL is
    // what hands the task to a human: nobody but them can merge a request, so
    // the strip has to say so. Nothing said it before the report.
    const URL: &str = "https://github.com/owner/repo/pull/12";
    assert_eq!(
        h.attention(&author).await,
        None,
        "an author that has published nothing yet is nobody's to answer"
    );
    let published: TaskDto = h
        .json(
            as_session(
                &format!("/v1/tasks/{}/pull-request", task.id),
                &author.id,
                serde_json::json!({"url": URL}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(published.pr_url.as_deref(), Some(URL));
    assert_eq!(
        h.attention(&author).await,
        Some(AttentionReason::WaitingUser),
        "a published request is the user's to merge, and the strip says so"
    );

    // And it stays up while the author polls: what it reports is the agent
    // working, which was never what the flag was about.
    h.store
        .clear_agent_attention(&author.id)
        .await
        .expect("an agent event that changes nothing is not an error");
    assert_eq!(
        h.attention(&author).await,
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
                &author.id,
                serde_json::json!({"to": "finished", "merge_commit": sha}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(landed.status, TaskStatus::Finished);
    assert_eq!(landed.merge_commit.as_deref(), Some(sha.as_str()));
}
