//! A task staffed with several authors.
//!
//! Each author writes the task alone — its own session, its own worktree, its
//! own branch — and each author's review runs to approval beside its
//! siblings'. Once every author stands approved the reviewers pick the one
//! whose change lands, and the branches the pick passed over go with their
//! worktrees.
//!
//! Like `landing_lifecycle`, the agents are the harness's stub and the `git`
//! is real: the worktrees, branches and merges are the ones the daemon
//! actually made.

use crate::common;

use std::path::PathBuf;
use std::time::Duration;

use axum::http::StatusCode;

use ariadne_api::tasks::TaskDto;
use ariadne_core::{Actor, MessageKind, Seat, SessionStatus, TaskStatus};
use ariadne_store::{
    AgentSession, Goal, NewMessage, NewTask, NewTaskAgent, Repository, SessionFilter, Task,
    TaskAgent, author_branch,
};

use common::{Harness, as_session, eventually, harness, sh, test_pin};

/// How long a test waits for the scheduler to reach a state.
const TIMEOUT: Duration = Duration::from_secs(20);

/// A goal on a real repository, holding one task staffed with two authors and
/// one reviewer, and the staffing read back in listing order.
struct Contest {
    goal: Goal,
    repo: Repository,
    task: Task,
    authors: Vec<TaskAgent>,
    reviewer: TaskAgent,
}

async fn contest(h: &Harness) -> Contest {
    contest_reviewed_by(h, 1).await
}

/// The same, with `reviewers` reviewers on it: what a review announced one
/// reviewer at a time needs, since one reviewer is one row and no sequence.
async fn contest_reviewed_by(h: &Harness, reviewers: usize) -> Contest {
    h.git_repo("repo");
    let repo = h.repository(&h.at("repo")).await;
    let goal = h.goal_on(&repo, test_pin()).await;
    let author = || NewTaskAgent::new(Seat::Author, ["coding"], test_pin());
    let task = h
        .store
        .create_task(NewTask {
            goal_id: goal.id.clone(),
            repo_id: repo.id.clone(),
            title: "Contested work".into(),
            description: "do things".into(),
            agents: [author(), author()]
                .into_iter()
                .chain(
                    (0..reviewers)
                        .map(|_| NewTaskAgent::new(Seat::Reviewer, ["code-review"], test_pin())),
                )
                .collect(),
            depends_on: vec![],
            landing: None,
            permission_mode: None,
        })
        .await
        .unwrap();
    let authors = h.store.list_task_authors(&task.id).await.unwrap();
    let reviewer = h
        .store
        .list_task_reviewers(&task.id)
        .await
        .unwrap()
        .remove(0);
    let goal = h.activate(&goal).await;
    Contest {
        goal,
        repo,
        task,
        authors,
        reviewer,
    }
}

/// The live author sessions of a task, in no particular order.
async fn live_authors(h: &Harness, task_id: &str) -> Vec<AgentSession> {
    h.store
        .list_sessions(SessionFilter {
            task_id: Some(task_id.to_string()),
            live_only: true,
            ..Default::default()
        })
        .await
        .unwrap()
        .into_iter()
        .filter(|s| s.seat() == Seat::Author)
        .collect()
}

/// One author's open review, written the way `request_review` writes it: a
/// request from that author to every reviewer, carrying its summary.
async fn ask_for_review(h: &Harness, task: &Task, author: &TaskAgent, summary: &str) {
    for reviewer in h.store.list_task_reviewers(&task.id).await.unwrap() {
        h.store
            .send_message(NewMessage {
                goal_id: task.goal_id.clone(),
                task_id: Some(task.id.clone()),
                kind: MessageKind::ReviewRequest,
                from_actor: Actor::Author,
                from_agent_id: Some(author.id.clone()),
                from_session: None,
                to_actor: Actor::Reviewer,
                to_agent_id: Some(reviewer.id.clone()),
                body: summary.to_string(),
            })
            .await
            .unwrap();
    }
}

/// One reviewer's verdict on one author's open review.
async fn verdict_on(
    h: &Harness,
    task: &Task,
    reviewer: &TaskAgent,
    author: &TaskAgent,
    kind: MessageKind,
) {
    h.store
        .send_message(NewMessage {
            goal_id: task.goal_id.clone(),
            task_id: Some(task.id.clone()),
            kind,
            from_actor: Actor::Reviewer,
            from_agent_id: Some(reviewer.id.clone()),
            from_session: None,
            to_actor: Actor::Author,
            to_agent_id: Some(author.id.clone()),
            body: "judged".to_string(),
        })
        .await
        .unwrap();
}

/// Two authors on one task are two agents at work at once: a session each, a
/// worktree each, and a branch each — the task's own name for the first, and
/// the `-a2` sibling for the second.
#[tokio::test]
async fn a_task_staffed_with_two_authors_runs_two_sessions_in_two_worktrees() {
    let h = harness().scheduler().await;
    let c = contest(&h).await;
    h.notify(&c.task.id);

    eventually(TIMEOUT, "both authors to be spawned", async || {
        h.status(&c.task.id).await == TaskStatus::InProgress
            && live_authors(&h, &c.task.id).await.len() == 2
    })
    .await;

    let sessions = live_authors(&h, &c.task.id).await;
    let mut worktrees: Vec<PathBuf> = sessions
        .iter()
        .map(|s| PathBuf::from(s.worktree_path.as_deref().expect("a worktree per author")))
        .collect();
    worktrees.sort();
    worktrees.dedup();
    assert_eq!(worktrees.len(), 2, "two authors share a worktree");
    for worktree in &worktrees {
        assert!(worktree.is_dir(), "{} is not on disk", worktree.display());
    }

    // Each session runs the author it was staffed for, and each worktree is
    // checked out on that author's own branch.
    let mut branches: Vec<String> = worktrees
        .iter()
        .map(|wt| sh(wt, "git rev-parse --abbrev-ref HEAD"))
        .collect();
    branches.sort();
    let mut staffed: Vec<String> = c
        .authors
        .iter()
        .map(|a| author_branch(&c.task.branch, a.ordinal))
        .collect();
    staffed.sort();
    assert_eq!(branches, staffed);
}

/// The gate the pick waits behind: a reviewer that has approved one author
/// and not the other cannot pick yet, and the refusal says why.
#[tokio::test]
async fn the_pick_starts_only_after_every_author_is_approved() {
    let h = harness().await;
    let c = contest(&h).await;
    h.advance(&c.task, TaskStatus::UnderReview).await;
    let reviewer_session = h
        .session(&c.goal, Some(&c.task), Seat::Reviewer, &c.reviewer.id)
        .await;

    // Both authors asked; only the first is approved.
    ask_for_review(&h, &c.task, &c.authors[0], "the first attempt").await;
    ask_for_review(&h, &c.task, &c.authors[1], "the second attempt").await;
    verdict_on(
        &h,
        &c.task,
        &c.reviewer,
        &c.authors[0],
        MessageKind::Approve,
    )
    .await;

    let (status, body) = h
        .send(as_session(
            &format!("/v1/tasks/{}/pick", c.task.id),
            &reviewer_session.id,
            serde_json::json!({"author_agent_id": c.authors[0].id}),
        ))
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let refusal = String::from_utf8_lossy(&body);
    assert!(
        refusal.contains("every author") && refusal.contains("approved"),
        "{refusal}"
    );

    // The second approval opens the pick.
    verdict_on(
        &h,
        &c.task,
        &c.reviewer,
        &c.authors[1],
        MessageKind::Approve,
    )
    .await;
    let picked: TaskDto = h
        .json(
            as_session(
                &format!("/v1/tasks/{}/pick", c.task.id),
                &reviewer_session.id,
                serde_json::json!({"author_agent_id": c.authors[0].id}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(picked.picks.len(), 1);
    assert_eq!(picked.picks[0].reviewer_agent_id, c.reviewer.id);
    assert_eq!(picked.picks[0].author_agent_id, c.authors[0].id);
}

/// One pick per reviewer per task, and the second is refused by the
/// reviewer's name.
#[tokio::test]
async fn a_second_pick_from_the_same_reviewer_is_refused_by_name() {
    let h = harness().await;
    let c = contest(&h).await;
    h.advance(&c.task, TaskStatus::UnderReview).await;
    let reviewer_session = h
        .session(&c.goal, Some(&c.task), Seat::Reviewer, &c.reviewer.id)
        .await;
    for author in &c.authors {
        ask_for_review(&h, &c.task, author, "an attempt").await;
        verdict_on(&h, &c.task, &c.reviewer, author, MessageKind::Approve).await;
    }

    h.json::<TaskDto>(
        as_session(
            &format!("/v1/tasks/{}/pick", c.task.id),
            &reviewer_session.id,
            serde_json::json!({"author_agent_id": c.authors[0].id}),
        ),
        StatusCode::OK,
    )
    .await;

    let (status, body) = h
        .send(as_session(
            &format!("/v1/tasks/{}/pick", c.task.id),
            &reviewer_session.id,
            serde_json::json!({"author_agent_id": c.authors[1].id}),
        ))
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let refusal = String::from_utf8_lossy(&body);
    assert!(refusal.contains(&c.reviewer.id), "{refusal}");
    assert!(refusal.contains("already picked"), "{refusal}");
}

/// The whole of it: two authors write two changes, both are approved, the
/// reviewer picks one, and exactly that branch lands — the loser's worktree
/// and branch are gone by the time the landing is done.
#[tokio::test]
async fn exactly_one_branch_lands_and_the_losers_are_gone() {
    let h = harness().scheduler().await;
    let c = contest(&h).await;
    h.notify(&c.task.id);
    eventually(TIMEOUT, "both authors to be spawned", async || {
        h.status(&c.task.id).await == TaskStatus::InProgress
            && live_authors(&h, &c.task.id).await.len() == 2
    })
    .await;

    // Each author commits its own attempt in its own worktree.
    let sessions = live_authors(&h, &c.task.id).await;
    for (n, author) in c.authors.iter().enumerate() {
        let session = sessions
            .iter()
            .find(|s| s.task_agent_id.as_deref() == Some(author.id.as_str()))
            .expect("a session per author");
        let worktree = PathBuf::from(session.worktree_path.as_deref().unwrap());
        sh(
            &worktree,
            &format!(
                "echo attempt-{n} > feature.txt && git add . && \
                 git -c user.email=t@t -c user.name=t commit -qm 'wip: attempt {n}'"
            ),
        );
    }

    // Both authors submit, the reviewer approves both, and picks the second.
    h.store
        .transition_task(
            &c.task.id,
            TaskStatus::UnderReview,
            Actor::Author,
            None,
            None,
        )
        .await
        .unwrap();
    let winner = &c.authors[1];
    let loser = &c.authors[0];
    for author in &c.authors {
        ask_for_review(&h, &c.task, author, "an attempt").await;
        verdict_on(&h, &c.task, &c.reviewer, author, MessageKind::Approve).await;
    }
    h.store
        .record_pick(&c.task.id, &c.reviewer.id, &winner.id)
        .await
        .unwrap();
    h.notify(&c.task.id);

    // The pick settles: the winner is on the task, the task is approved, and
    // the loser's worktree and branch are already gone.
    let loser_branch = author_branch(&c.task.branch, loser.ordinal);
    let winner_branch = author_branch(&c.task.branch, winner.ordinal);
    let repo = PathBuf::from(&c.repo.path);
    eventually(TIMEOUT, "the pick to settle", async || {
        let task = h.store.get_task(&c.task.id).await.unwrap();
        task.picked_agent_id.as_deref() == Some(winner.id.as_str())
            && task.status() == TaskStatus::Approved
    })
    .await;
    let loser_worktree = PathBuf::from(
        sessions
            .iter()
            .find(|s| s.task_agent_id.as_deref() == Some(loser.id.as_str()))
            .unwrap()
            .worktree_path
            .as_deref()
            .unwrap(),
    );
    eventually(TIMEOUT, "the loser to be cleaned up", async || {
        !loser_worktree.exists()
            && sh(
                &repo,
                &format!("git branch --list {loser_branch} | wc -l | tr -d ' '"),
            ) == "0"
    })
    .await;

    // The winner's worktree became the task's own, and the landing is run
    // there: rebase, squash, fast-forward, exactly as a lone author lands.
    let task = h.store.get_task(&c.task.id).await.unwrap();
    let winner_worktree = PathBuf::from(task.worktree_path.as_deref().expect("the winner's tree"));
    assert!(winner_worktree.exists());
    assert_eq!(
        sh(&winner_worktree, "git rev-parse --abbrev-ref HEAD"),
        winner_branch
    );
    // The landing resumes the winner's own session; the loser has none left.
    eventually(TIMEOUT, "the winner to be briefed to land it", async || {
        h.running_session(&c.task.id, Seat::Author)
            .await
            .is_some_and(|s| s.task_agent_id.as_deref() == Some(winner.id.as_str()))
    })
    .await;
    let author_session = h
        .running_session(&c.task.id, Seat::Author)
        .await
        .expect("the winner's session is the one landing it");

    sh(&winner_worktree, "git rebase -q main");
    sh(
        &winner_worktree,
        "git reset --soft main && \
         git -c user.email=t@t -c user.name=t commit -qm 'feat: the contested change'",
    );
    sh(&repo, &format!("git merge -q --ff-only {winner_branch}"));
    let sha = sh(&repo, "git rev-parse main");

    let landed: TaskDto = h
        .json(
            as_session(
                &format!("/v1/tasks/{}/transitions", c.task.id),
                &author_session.id,
                serde_json::json!({"to": "finished", "merge_commit": sha}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(landed.status, TaskStatus::Finished);

    // Exactly one branch landed: the base carries the winner's attempt, and
    // after the cleanup neither author branch nor worktree is left.
    assert_eq!(sh(&repo, "cat feature.txt"), "attempt-1");
    eventually(TIMEOUT, "the landing cleanup", async || {
        !winner_worktree.exists()
            && sh(
                &repo,
                &format!(
                    "git branch --list {winner_branch} {loser_branch} {} | wc -l | tr -d ' '",
                    c.task.branch
                ),
            ) == "0"
    })
    .await;
}

/// Asking a reviewer to pick is not delivered and forgotten if the hand-off
/// to the live reviewer failed. Its runtime entry can close its prompt
/// channel in the moment between the liveness check and the hand-off — the
/// same state its own connection ending leaves behind for a few awaits
/// before it is deregistered — and `hand_prompt` says so by failing. Marked
/// asked at that failed attempt regardless, the reviewer would never be
/// asked again.
#[tokio::test]
async fn a_pick_ask_survives_a_failed_hand_off_to_a_live_reviewer() {
    let h = harness().scheduler().await;
    let c = contest(&h).await;
    h.notify(&c.task.id);
    eventually(TIMEOUT, "both authors to be spawned", async || {
        h.status(&c.task.id).await == TaskStatus::InProgress
            && live_authors(&h, &c.task.id).await.len() == 2
    })
    .await;

    h.store
        .transition_task(
            &c.task.id,
            TaskStatus::UnderReview,
            Actor::Author,
            None,
            None,
        )
        .await
        .unwrap();
    ask_for_review(&h, &c.task, &c.authors[0], "the first attempt").await;
    h.notify(&c.task.id);
    eventually(TIMEOUT, "the reviewer to be spawned", async || {
        h.running_session(&c.task.id, Seat::Reviewer)
            .await
            .is_some()
    })
    .await;
    let reviewer_session = h
        .running_session(&c.task.id, Seat::Reviewer)
        .await
        .expect("a live reviewer session");
    ask_for_review(&h, &c.task, &c.authors[1], "the second attempt").await;
    verdict_on(
        &h,
        &c.task,
        &c.reviewer,
        &c.authors[0],
        MessageKind::Approve,
    )
    .await;

    // Live per the registry, but its prompt channel is already closed —
    // closed before the second approval, so the pick-ask that follows it is
    // the hand-off that fails.
    h.launcher
        .acp
        .close_prompt_channel_for_test(&reviewer_session.id);
    verdict_on(
        &h,
        &c.task,
        &c.reviewer,
        &c.authors[1],
        MessageKind::Approve,
    )
    .await;
    h.notify(&c.task.id);
    // Waited out rather than slept past: the flush answers only once the
    // notify above's own reconciliation is done, which is the failed
    // hand-off actually having been attempted.
    h.flush_scheduler().await;
    assert!(
        !h.prompted(&reviewer_session)
            .contains("Pick the one change that lands"),
        "the closed channel could not have delivered a pick ask"
    );

    // The agent comes back — a fresh, working registration under the same
    // session, still live throughout rather than killed and relaunched, so
    // this is the same live hand-off retrying rather than the fallback path
    // taking over — and the pick is still owed. `agent_runs` starts it from a
    // session no longer `Running`, so the reviewer's briefing turn ends first.
    eventually(TIMEOUT, "the reviewer's briefing turn to end", async || {
        h.store
            .get_session(&reviewer_session.id)
            .await
            .unwrap()
            .status()
            != SessionStatus::Running
    })
    .await;
    h.agent_runs(&reviewer_session).await;
    h.notify(&c.task.id);
    eventually(
        TIMEOUT,
        "the reviewer to be asked to pick now that it can hear it",
        async || {
            h.prompted(&reviewer_session)
                .contains("Pick the one change that lands")
        },
    )
    .await;
    assert_eq!(
        h.prompted(&reviewer_session)
            .matches("Pick the one change that lands")
            .count(),
        1,
        "exactly one ask, once the channel could carry it"
    );
}

/// A contested review request is not skipped and forgotten if the resume
/// that would spawn its first reviewer fails. `resume_reviewer_for`'s
/// worktree setup refuses a branch that does not exist — the same refusal
/// an author that has not pushed anything would hit. As in the uncontested
/// case, a plain retry of the resume would not show whether the marker
/// moved: with no live session the resume runs fresh every pass regardless.
/// What proves it is the reviewer's own live path, later: a session already
/// seeded starting, resumed by its own agent rather than by another call
/// the scheduler drives, comes up live and is briefed only if the failed
/// attempt left the marker clear.
#[tokio::test]
async fn a_contested_review_survives_a_failed_resume_of_its_first_reviewer() {
    let h = harness().scheduler().await;
    let c = contest(&h).await;
    // No branch for either author yet: neither has ever spawned to create
    // one. `rouse_reviewer_for`'s worktree setup has nothing to check out.
    h.advance(&c.task, TaskStatus::UnderReview).await;
    const SUMMARY: &str = "the seeded contested summary";
    ask_for_review(&h, &c.task, &c.authors[0], SUMMARY).await;

    // A reviewer session already starting, as if an earlier round had once
    // reported from it: the resume below finds this row resumable — the
    // same one that later comes up on its own — rather than falling back to
    // a fresh spawn.
    let seeded = h
        .session(&c.goal, Some(&c.task), Seat::Reviewer, &c.reviewer.id)
        .await;
    h.store
        .set_session_internal_id(&seeded.id, "seeded-reviewer-session")
        .await
        .unwrap();

    h.notify(&c.task.id);
    // Waited out rather than slept past: the flush answers only once the
    // notify above's own reconciliation is done, which is the failed resume
    // actually having been attempted.
    h.flush_scheduler().await;
    assert_eq!(
        h.session_status(&seeded).await,
        SessionStatus::Starting,
        "the worktree refusal lands before restart_session ever touches the row"
    );
    let request = h
        .store
        .list_messages(ariadne_store::MessageFilter {
            task_id: Some(c.task.id.clone()),
            to_agent_id: Some(c.reviewer.id.clone()),
            ..Default::default()
        })
        .await
        .unwrap()
        .into_iter()
        .find(|message| message.kind() == Some(MessageKind::ReviewRequest))
        .expect("the review request");
    assert!(
        !h.store
            .get_message(&request.id)
            .await
            .unwrap()
            .is_delivered(),
        "a failed resume does not stamp the request delivered"
    );

    // The first author's branch exists now — what a pushed change looks
    // like. The same seeded session comes up on its own, the way an agent
    // already mid-launch would — not through another resume the scheduler
    // drives — so whether it is briefed depends only on what the failed
    // attempt above left on `review_briefed`.
    sh(
        &PathBuf::from(&c.repo.path),
        &format!(
            "git branch {}",
            author_branch(&c.task.branch, c.authors[0].ordinal)
        ),
    );
    h.agent_runs(&seeded).await;
    h.notify(&c.task.id);
    eventually(
        TIMEOUT,
        "the live reviewer to receive the briefing now that it can hear it",
        async || h.prompted(&seeded).contains(SUMMARY),
    )
    .await;
    assert_eq!(
        h.prompted(&seeded).matches(SUMMARY).count(),
        1,
        "exactly one delivery, once the failed attempt left the marker clear"
    );
    assert!(
        h.store
            .get_message(&request.id)
            .await
            .unwrap()
            .is_delivered(),
        "the live briefing stamps its request delivered"
    );
}

/// A pick ask is not skipped and forgotten if `run_the_pick`'s fallback
/// resume, for a reviewer with nothing live yet, fails. Its worktree setup
/// refuses a branch that does not exist, the same as either review-resume
/// path above, and the same shape of proof applies: a session already
/// seeded starting, brought up by its own agent rather than by another
/// resume the scheduler drives, is asked to pick only if the failed attempt
/// left `pick_briefed` clear.
#[tokio::test]
async fn a_pick_ask_survives_a_failed_resume_of_its_reviewer() {
    let h = harness().scheduler().await;
    let c = contest(&h).await;
    // No branch for either author: neither ever spawns to create one, and
    // the pick's fallback resume reads the task's own branch — the first
    // author's own name for it — which is what its worktree setup refuses.
    h.advance(&c.task, TaskStatus::UnderReview).await;
    ask_for_review(&h, &c.task, &c.authors[0], "the first attempt").await;
    ask_for_review(&h, &c.task, &c.authors[1], "the second attempt").await;
    verdict_on(
        &h,
        &c.task,
        &c.reviewer,
        &c.authors[0],
        MessageKind::Approve,
    )
    .await;
    verdict_on(
        &h,
        &c.task,
        &c.reviewer,
        &c.authors[1],
        MessageKind::Approve,
    )
    .await;

    // A reviewer session already starting, as if an earlier round had once
    // reported from it: the fallback resume below finds this row resumable
    // — the same one that later comes up on its own — rather than falling
    // back to a fresh spawn.
    let seeded = h
        .session(&c.goal, Some(&c.task), Seat::Reviewer, &c.reviewer.id)
        .await;
    h.store
        .set_session_internal_id(&seeded.id, "seeded-reviewer-session")
        .await
        .unwrap();

    h.notify(&c.task.id);
    // Waited out rather than slept past: the flush answers only once the
    // notify above's own reconciliation is done, which is the failed resume
    // actually having been attempted.
    h.flush_scheduler().await;
    assert_eq!(
        h.session_status(&seeded).await,
        SessionStatus::Starting,
        "the worktree refusal lands before restart_session ever touches the row"
    );
    assert!(
        h.prompted(&seeded).is_empty(),
        "a failed resume could not have asked the reviewer to pick"
    );

    // The first author's branch exists now — what a pushed change looks
    // like. The same seeded session comes up on its own, the way an agent
    // already mid-launch would — not through another resume the scheduler
    // drives — so whether it is asked to pick depends only on what the
    // failed attempt above left on `pick_briefed`.
    sh(
        &PathBuf::from(&c.repo.path),
        &format!(
            "git branch {}",
            author_branch(&c.task.branch, c.authors[0].ordinal)
        ),
    );
    h.agent_runs(&seeded).await;
    h.notify(&c.task.id);
    eventually(
        TIMEOUT,
        "the live reviewer to be asked to pick now that it can hear it",
        async || {
            h.prompted(&seeded)
                .contains("Pick the one change that lands")
        },
    )
    .await;
    assert_eq!(
        h.prompted(&seeded)
            .matches("Pick the one change that lands")
            .count(),
        1,
        "exactly one ask, once the failed attempt left the marker clear"
    );
}

/// A reviewer whose agent survived the first review is handed the next
/// author's review the moment it owes it: the full briefing, naming that
/// author and its branch, sent straight to the live agent — not held for the
/// quiet clock — and its detached worktree moved to that branch first.
#[tokio::test]
async fn a_live_reviewer_is_briefed_for_the_next_author_without_the_quiet_clock() {
    // The agents stay up, so the reviewer that judged the first author is a
    // live session when the second author's review opens.
    let h = harness().scheduler().await;
    let c = contest(&h).await;
    h.notify(&c.task.id);
    eventually(TIMEOUT, "both authors to be spawned", async || {
        h.status(&c.task.id).await == TaskStatus::InProgress
            && live_authors(&h, &c.task.id).await.len() == 2
    })
    .await;

    // Each author commits its own attempt, so the two branches have tips of
    // their own for the reviewer's worktree to be pinned at.
    let sessions = live_authors(&h, &c.task.id).await;
    for (n, author) in c.authors.iter().enumerate() {
        let session = sessions
            .iter()
            .find(|s| s.task_agent_id.as_deref() == Some(author.id.as_str()))
            .expect("a session per author");
        let worktree = PathBuf::from(session.worktree_path.as_deref().unwrap());
        sh(
            &worktree,
            &format!(
                "echo attempt-{n} > feature.txt && git add . && \
                 git -c user.email=t@t -c user.name=t commit -qm 'wip: attempt {n}'"
            ),
        );
    }

    // The first author asks, and the reviewer is spawned for that review.
    h.store
        .transition_task(
            &c.task.id,
            TaskStatus::UnderReview,
            Actor::Author,
            None,
            None,
        )
        .await
        .unwrap();
    ask_for_review(&h, &c.task, &c.authors[0], "the first attempt").await;
    h.notify(&c.task.id);
    eventually(TIMEOUT, "the reviewer to be spawned", async || {
        h.running_session(&c.task.id, Seat::Reviewer)
            .await
            .is_some()
    })
    .await;
    let reviewer_session = h
        .running_session(&c.task.id, Seat::Reviewer)
        .await
        .expect("a live reviewer session");
    assert!(
        h.told(&reviewer_session.id)
            .contains(&format!("Give your verdict to author {}", c.authors[0].id)),
        "the reviewer was not spawned for the first author's review"
    );

    // The second author asks while the reviewer's agent stays up, and the
    // reviewer settles the first review from that live session.
    ask_for_review(&h, &c.task, &c.authors[1], "the second attempt").await;
    h.notify(&c.task.id);
    h.json::<ariadne_api::messages::MessageDto>(
        as_session(
            &format!("/v1/tasks/{}/messages", c.task.id),
            &reviewer_session.id,
            serde_json::json!({
                "kind": "approve",
                "to_actor": "author",
                "to_agent_id": c.authors[0].id,
                "body": "the first attempt reads right",
            }),
        ),
        StatusCode::CREATED,
    )
    .await;

    // The verdict is the event that hands it the next review: the live agent
    // is briefed at once, naming the second author and its branch. The wait
    // here is seconds, an order of magnitude under the quiet clock — a
    // briefing that waited for the nudge would fail this test.
    let second_branch = author_branch(&c.task.branch, c.authors[1].ordinal);
    eventually(TIMEOUT, "the live reviewer to be briefed", async || {
        let pasted = h.prompted(&reviewer_session);
        pasted.contains(&format!("Give your verdict to author {}", c.authors[1].id))
            && pasted.contains(&second_branch)
    })
    .await;

    // And the tree it verifies in moved first: the detached worktree stands
    // on the second author's branch tip.
    let reviewer_worktree = PathBuf::from(reviewer_session.worktree_path.as_deref().unwrap());
    let repo = PathBuf::from(&c.repo.path);
    assert_eq!(
        sh(&reviewer_worktree, "git rev-parse HEAD"),
        sh(&repo, &format!("git rev-parse {second_branch}")),
    );
}

/// A settlement the daemon died in is finished by the daemon that comes
/// back: the winner was written, the approval was not, and the losers still
/// stand. The startup pass reads the picks and the approvals — all still on
/// the store — and runs the settlement again.
#[tokio::test]
async fn a_restart_finishes_a_settlement_the_daemon_died_in() {
    use ariadne_daemon::scheduler::{self, SchedEvent};

    let h = harness().await;
    let c = contest(&h).await;
    // The authors as the first daemon left them: spawned, each with a commit
    // of its own, both approved, the pick complete, and the winner written —
    // then nothing, which is the crash.
    let mut worktrees = Vec::new();
    for (n, author) in c.authors.iter().enumerate() {
        let session = h
            .launcher
            .spawn_author_agent(&c.task.id, &author.id)
            .await
            .unwrap();
        let worktree = PathBuf::from(session.worktree_path.as_deref().unwrap());
        sh(
            &worktree,
            &format!(
                "echo attempt-{n} > feature.txt && git add . && \
                 git -c user.email=t@t -c user.name=t commit -qm 'wip: attempt {n}'"
            ),
        );
        worktrees.push(worktree);
    }
    h.advance(&c.task, TaskStatus::UnderReview).await;
    for author in &c.authors {
        ask_for_review(&h, &c.task, author, "an attempt").await;
        verdict_on(&h, &c.task, &c.reviewer, author, MessageKind::Approve).await;
    }
    let winner = &c.authors[1];
    let loser = &c.authors[0];
    h.store
        .record_pick(&c.task.id, &c.reviewer.id, &winner.id)
        .await
        .unwrap();
    h.store
        .set_task_picked(&c.task.id, &winner.id)
        .await
        .unwrap();

    // The daemon that comes back: its first pass over the task finds the
    // half-finished settlement and completes it.
    let sched = scheduler::start(h.store.clone(), h.launcher.clone(), false, h.timeouts);
    sched
        .send(SchedEvent::TaskChanged(c.task.id.clone()))
        .unwrap();

    let loser_branch = author_branch(&c.task.branch, loser.ordinal);
    let repo = PathBuf::from(&c.repo.path);
    let loser_worktree = worktrees[loser.ordinal as usize].clone();
    eventually(TIMEOUT, "the settlement to be finished", async || {
        let task = h.store.get_task(&c.task.id).await.unwrap();
        task.status() == TaskStatus::Approved
            && !loser_worktree.exists()
            && sh(
                &repo,
                &format!("git branch --list {loser_branch} | wc -l | tr -d ' '"),
            ) == "0"
    })
    .await;
    // And the winner stands untouched, its worktree now the task's own.
    let task = h.store.get_task(&c.task.id).await.unwrap();
    assert_eq!(task.picked_agent_id.as_deref(), Some(winner.id.as_str()));
    assert_eq!(
        task.worktree_path.as_deref(),
        Some(worktrees[winner.ordinal as usize].display().to_string()).as_deref()
    );
    assert!(worktrees[winner.ordinal as usize].exists());
}

/// On a contested task too, each reviewer is briefed once for the request it
/// was sent.
///
/// One author's announcement writes one row per reviewer, so the newest of
/// that author's rows walks forward while it runs. A pass in between must key
/// each reviewer on its own row, and the reviewer whose row is not written
/// yet must inherit its briefing when it lands — not ask for a second one.
#[tokio::test]
async fn each_contested_reviewer_is_briefed_once_when_its_request_row_lands_late() {
    let h = harness().scheduler().await;
    let c = contest_reviewed_by(&h, 2).await;
    let reviewers = h.store.list_task_reviewers(&c.task.id).await.unwrap();
    h.notify(&c.task.id);
    eventually(TIMEOUT, "both authors to be spawned", async || {
        h.status(&c.task.id).await == TaskStatus::InProgress
            && live_authors(&h, &c.task.id).await.len() == 2
    })
    .await;
    for (n, session) in live_authors(&h, &c.task.id).await.iter().enumerate() {
        let worktree = PathBuf::from(session.worktree_path.as_deref().unwrap());
        sh(
            &worktree,
            &format!(
                "echo attempt-{n} > feature.txt && git add . && \
                 git -c user.email=t@t -c user.name=t commit -qm 'wip: attempt {n}'"
            ),
        );
    }

    // The first author's review, announced one reviewer at a time with a
    // scheduler pass in between.
    h.store
        .transition_task(
            &c.task.id,
            TaskStatus::UnderReview,
            Actor::Author,
            None,
            None,
        )
        .await
        .unwrap();
    for reviewer in &reviewers {
        let request = h
            .store
            .send_message(NewMessage {
                goal_id: c.task.goal_id.clone(),
                task_id: Some(c.task.id.clone()),
                kind: MessageKind::ReviewRequest,
                from_actor: Actor::Author,
                from_agent_id: Some(c.authors[0].id.clone()),
                from_session: None,
                to_actor: Actor::Reviewer,
                to_agent_id: Some(reviewer.id.clone()),
                body: "the first attempt".to_string(),
            })
            .await
            .unwrap();
        h.notify(&c.task.id);
        eventually(TIMEOUT, "the request to be stamped delivered", async || {
            h.store
                .get_message(&request.id)
                .await
                .unwrap()
                .is_delivered()
        })
        .await;
    }
    // Several passes over both requests, so a briefing owed to either has
    // every chance to go out.
    for _ in 0..3 {
        h.notify(&c.task.id);
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    let sessions = h.sessions_of(&c.task.id).await;
    for reviewer in &reviewers {
        let session = sessions
            .iter()
            .find(|s| {
                s.seat() == Seat::Reviewer
                    && s.task_agent_id.as_deref() == Some(reviewer.id.as_str())
            })
            .expect("a session per reviewer");
        let prompts = h.prompts_to(session);
        assert_eq!(
            prompts.len(),
            1,
            "reviewer {} was briefed twice for one request: {prompts:#?}",
            reviewer.id
        );
    }
}

/// A reviewer briefed before its request row exists holds that briefing
/// against the author it was for, and one author's marker never answers for
/// another's.
///
/// A contested reviewer owes verdicts on several authors at once, so it can
/// carry a marker for one author's review while a second author's review opens
/// under it. One marker for every author would let the second review find the
/// first's and brief nobody, and that review would reach the reviewer never.
#[tokio::test]
async fn a_contested_reviewer_keeps_a_briefing_marker_for_each_author() {
    let h = harness().scheduler().await;
    let (c, _reviewers, marked) = two_pending_markers(&h).await;

    let prompts = h.prompts_to(&marked);
    assert_eq!(
        prompts.len(),
        2,
        "one briefing per author's review is owed: {prompts:#?}"
    );
    assert!(
        prompts[1].contains(&format!("Give your verdict to author {}", c.authors[1].id)),
        "the second briefing is the second author's review: {}",
        prompts[1]
    );
}

/// And the marker of the author whose row lands is the one that row takes
/// over: it stamps that author's request delivered, asks for no further
/// briefing, and leaves the other author's marker where it is.
///
/// A reviewer carrying a marker for two authors at once is where a hand-over
/// can reach for the wrong one. The row that lands belongs to one review, so
/// only that review's marker answers for it.
#[tokio::test]
async fn a_contested_reviewer_adopts_the_row_of_the_author_its_marker_names() {
    let h = harness().scheduler().await;
    let (c, reviewers, marked) = two_pending_markers(&h).await;

    // The rest of the second author's announcement: the row for the reviewer
    // that was briefed ahead of it.
    let request = h
        .store
        .send_message(NewMessage {
            goal_id: c.task.goal_id.clone(),
            task_id: Some(c.task.id.clone()),
            kind: MessageKind::ReviewRequest,
            from_actor: Actor::Author,
            from_agent_id: Some(c.authors[1].id.clone()),
            from_session: None,
            to_actor: Actor::Reviewer,
            to_agent_id: Some(reviewers[0].id.clone()),
            body: "the second attempt".to_string(),
        })
        .await
        .unwrap();
    h.notify(&c.task.id);
    eventually(TIMEOUT, "that row to be stamped delivered", async || {
        h.store
            .get_message(&request.id)
            .await
            .unwrap()
            .is_delivered()
    })
    .await;
    // Several passes over it, so a briefing owed to it has every chance to go
    // out.
    for _ in 0..3 {
        h.notify(&c.task.id);
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    let prompts = h.prompts_to(&marked);
    assert_eq!(
        prompts.len(),
        2,
        "the briefing already sent is that row's: {prompts:#?}"
    );
}

/// One contested reviewer carrying a marker for each of two authors.
///
/// Each author's review opens with a row for the second reviewer and none for
/// the first, which is the window an announcement passes through. The first
/// reviewer is roused for each review in turn and marked under its author, its
/// own row never having been written. Answers the contest, its reviewers in
/// listing order, and the session of the reviewer that holds both markers.
async fn two_pending_markers(h: &Harness) -> (Contest, Vec<TaskAgent>, AgentSession) {
    let c = contest_reviewed_by(h, 2).await;
    let reviewers = h.store.list_task_reviewers(&c.task.id).await.unwrap();
    h.notify(&c.task.id);
    eventually(TIMEOUT, "both authors to be spawned", async || {
        h.status(&c.task.id).await == TaskStatus::InProgress
            && live_authors(h, &c.task.id).await.len() == 2
    })
    .await;
    for (n, session) in live_authors(h, &c.task.id).await.iter().enumerate() {
        let worktree = PathBuf::from(session.worktree_path.as_deref().unwrap());
        sh(
            &worktree,
            &format!(
                "echo attempt-{n} > feature.txt && git add . && \
                 git -c user.email=t@t -c user.name=t commit -qm 'wip: attempt {n}'"
            ),
        );
    }
    h.store
        .transition_task(
            &c.task.id,
            TaskStatus::UnderReview,
            Actor::Author,
            None,
            None,
        )
        .await
        .unwrap();

    ask_one_reviewer(
        h,
        &c.task,
        &c.authors[0],
        &reviewers[1],
        "the first attempt",
    )
    .await;
    h.notify(&c.task.id);
    let marked = eventually_reviewer_session(h, &c, &reviewers[0]).await;
    eventually(TIMEOUT, "the first reviewer to be briefed", async || {
        h.prompts_to(&marked).len() == 1
    })
    .await;

    // Both reviewers approve that author, so the next review the first
    // reviewer owes is the second author's. The marker for the first author
    // stays behind: its row was never written, so nothing took it over.
    for reviewer in &reviewers {
        verdict_on(h, &c.task, reviewer, &c.authors[0], MessageKind::Approve).await;
    }

    ask_one_reviewer(
        h,
        &c.task,
        &c.authors[1],
        &reviewers[1],
        "the second attempt",
    )
    .await;
    // Several passes over the second review, so the briefing it owes has
    // every chance to go out. What went out is left for the caller to say.
    for _ in 0..5 {
        h.notify(&c.task.id);
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    (c, reviewers, marked)
}

/// One author's review request to one reviewer, where the rest of that
/// author's announcement has not been written.
async fn ask_one_reviewer(
    h: &Harness,
    task: &Task,
    author: &TaskAgent,
    reviewer: &TaskAgent,
    summary: &str,
) {
    h.store
        .send_message(NewMessage {
            goal_id: task.goal_id.clone(),
            task_id: Some(task.id.clone()),
            kind: MessageKind::ReviewRequest,
            from_actor: Actor::Author,
            from_agent_id: Some(author.id.clone()),
            from_session: None,
            to_actor: Actor::Reviewer,
            to_agent_id: Some(reviewer.id.clone()),
            body: summary.to_string(),
        })
        .await
        .unwrap();
}

/// The session one reviewer was started in, once it has one.
async fn eventually_reviewer_session(
    h: &Harness,
    c: &Contest,
    reviewer: &TaskAgent,
) -> AgentSession {
    let of = async |h: &Harness| {
        h.sessions_of(&c.task.id).await.into_iter().find(|s| {
            s.seat() == Seat::Reviewer && s.task_agent_id.as_deref() == Some(reviewer.id.as_str())
        })
    };
    eventually(TIMEOUT, "the reviewer to be started", async || {
        of(h).await.is_some()
    })
    .await;
    of(h).await.expect("a session for the reviewer")
}

/// A contested review request never reaches a live reviewer as a bare
/// message: the summary alone names neither the author nor the branch, and
/// it would land while the worktree still stands on the review before it.
/// The full briefing is the only delivery, and it stamps the request
/// delivered on the channel.
#[tokio::test]
async fn a_contested_review_request_reaches_a_live_reviewer_only_as_its_briefing() {
    let h = harness().scheduler().await;
    let c = contest(&h).await;
    h.notify(&c.task.id);
    eventually(TIMEOUT, "both authors to be spawned", async || {
        h.status(&c.task.id).await == TaskStatus::InProgress
            && live_authors(&h, &c.task.id).await.len() == 2
    })
    .await;

    let sessions = live_authors(&h, &c.task.id).await;
    for (n, author) in c.authors.iter().enumerate() {
        let session = sessions
            .iter()
            .find(|s| s.task_agent_id.as_deref() == Some(author.id.as_str()))
            .expect("a session per author");
        let worktree = PathBuf::from(session.worktree_path.as_deref().unwrap());
        sh(
            &worktree,
            &format!(
                "echo attempt-{n} > feature.txt && git add . && \
                 git -c user.email=t@t -c user.name=t commit -qm 'wip: attempt {n}'"
            ),
        );
    }

    // The first author asks and the reviewer comes up for that review; the
    // second asks while the reviewer's agent stays live, and the reviewer's
    // approval of the first hands it the second.
    h.store
        .transition_task(
            &c.task.id,
            TaskStatus::UnderReview,
            Actor::Author,
            None,
            None,
        )
        .await
        .unwrap();
    ask_for_review(&h, &c.task, &c.authors[0], "the first attempt").await;
    h.notify(&c.task.id);
    eventually(TIMEOUT, "the reviewer to be spawned", async || {
        h.running_session(&c.task.id, Seat::Reviewer)
            .await
            .is_some()
    })
    .await;
    let reviewer_session = h
        .running_session(&c.task.id, Seat::Reviewer)
        .await
        .expect("a live reviewer session");
    ask_for_review(&h, &c.task, &c.authors[1], "the second attempt").await;
    h.notify(&c.task.id);
    h.json::<ariadne_api::messages::MessageDto>(
        as_session(
            &format!("/v1/tasks/{}/messages", c.task.id),
            &reviewer_session.id,
            serde_json::json!({
                "kind": "approve",
                "to_actor": "author",
                "to_agent_id": c.authors[0].id,
                "body": "the first attempt reads right",
            }),
        ),
        StatusCode::CREATED,
    )
    .await;

    // The second review reaches the agent as its full briefing, and as
    // nothing before it: no bare review-request turn is ever sent.
    eventually(TIMEOUT, "the live reviewer to be briefed", async || {
        h.prompted(&reviewer_session)
            .contains(&format!("Give your verdict to author {}", c.authors[1].id))
    })
    .await;
    let pasted = h.prompted(&reviewer_session);
    assert!(
        !pasted.contains("Message from your author"),
        "a contested review request was sent as a bare message: {pasted}"
    );

    // And the channel's record still says both requests reached it: the
    // briefing that carried each one stamped it delivered.
    eventually(
        TIMEOUT,
        "the requests to be stamped delivered",
        async || {
            h.store
                .list_messages(ariadne_store::MessageFilter {
                    task_id: Some(c.task.id.clone()),
                    to_agent_id: Some(c.reviewer.id.clone()),
                    ..Default::default()
                })
                .await
                .unwrap()
                .into_iter()
                .filter(|m| m.kind() == Some(MessageKind::ReviewRequest))
                .all(|m| m.is_delivered())
        },
    )
    .await;
}
