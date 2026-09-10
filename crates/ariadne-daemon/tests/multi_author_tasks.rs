//! A task staffed with several authors.
//!
//! Each author writes the task alone — its own session, its own worktree, its
//! own branch — and each author's review runs to approval beside its
//! siblings'. Once every author stands approved the reviewers pick the one
//! whose change lands, and the branches the pick passed over go with their
//! worktrees.
//!
//! Like `landing_lifecycle`, the `tmux` here is a stub and the `git` is real:
//! the sessions are rows and spawn plans, and the worktrees, branches and
//! merges are the ones the daemon actually made.

mod common;

use std::path::PathBuf;
use std::time::Duration;

use axum::http::StatusCode;

use ariadne_api::tasks::TaskDto;
use ariadne_core::{Actor, AgentKind, MessageKind, Seat, TaskStatus};
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
    h.git_repo("repo");
    let repo = h.repository(&h.at("repo")).await;
    let goal = h.goal_on(&repo, test_pin(AgentKind::ClaudeCode)).await;
    let author = || NewTaskAgent::new(Seat::Author, ["coding"], test_pin(AgentKind::ClaudeCode));
    let task = h
        .store
        .create_task(NewTask {
            goal_id: goal.id.clone(),
            repo_id: repo.id.clone(),
            title: "Contested work".into(),
            description: "do things".into(),
            agents: vec![
                author(),
                author(),
                NewTaskAgent::new(
                    Seat::Reviewer,
                    ["code-review"],
                    test_pin(AgentKind::ClaudeCode),
                ),
            ],
            depends_on: vec![],
            landing: None,
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
        live_authors(&h, &c.task.id).await.len() == 2
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
