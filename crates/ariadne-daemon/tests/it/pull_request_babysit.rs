//! A request of the user's own that no task opened gets a keeper session
//! once asked for by hand (032): on the worktree and local branch named
//! above, told of the author's news once, accepting its `ready` report,
//! and taken down with its worktree and its branch once the request ends
//! or asking stops.
use std::path::{Path, PathBuf};
use std::time::Duration;

use axum::body::Body;
use axum::http::StatusCode;
use serde_json::{Value, json};

use crate::common::forge::{StubForgeCli, answer, stub_forge_cli};
use crate::common::{
    Harness, QUIET, TIMEOUT, as_session, eventually, harness, put_json, sh, with_forge,
};
use ariadne_core::{Seat, SessionStatus};
use ariadne_daemon::forge::poll::Mode;
use ariadne_daemon::scheduler::SPAWN_RETRY_BUDGET;
use ariadne_daemon::timeouts::Timeouts;
use ariadne_store::{AgentSession, SessionFilter};

const PIN: &str = "stub:babysit-model";

/// How often the scheduler's full reconcile tick runs, sped up so a dead
/// launch's own budget runs out within the test's own timeout rather than
/// the daemon's ordinary five seconds.
const SWEEPS_SOON: Duration = Duration::from_millis(100);

/// How the stub forge shows one request of mine: its state, the head
/// repository it lives in, and the comments on its conversation, oldest
/// first.
struct Shown<'a> {
    author: &'a str,
    state: &'a str,
    head: &'a str,
    head_repo: &'a str,
    comments: Value,
}

impl Shown<'_> {
    fn open(head: &str) -> Shown<'_> {
        Shown {
            author: "me",
            state: "OPEN",
            head,
            head_repo: "https://github.com/acme/widgets",
            comments: json!([]),
        }
    }
}

/// What the stub forge answers for one request of mine, read for the
/// scheduler's own detail fetch.
fn script(shown: &Shown) -> Value {
    let read = json!({"number": 1, "url": "https://github.com/acme/widgets/pull/1",
        "title": "Add widgets", "author": {"login": shown.author}, "state": shown.state,
        "isDraft": false, "headRefName": "fix", "headRefOid": shown.head,
        "headRepository": {"url": shown.head_repo},
        "baseRefName": "main", "statusCheckRollup": [], "reviewDecision": "",
        "createdAt": "2026-10-01T00:00:00Z"});
    let listed = match shown.state == "OPEN" {
        true => json!([read]),
        false => json!([]),
    };
    let threads =
        json!({"data": {"repository": {"pullRequest": {"reviewThreads": {"nodes": []}}}}});
    json!([
        answer(&["api", "repos/acme/widgets/issues/1/timeline"], 0, "[]"),
        answer(&["auth", "status"], 0, ""),
        answer(&["api", "user"], 0, "me"),
        answer(&["pr", "list"], 0, &listed.to_string()),
        answer(&["pr", "view"], 0, &read.to_string()),
        answer(
            &["api", "repos/acme/widgets/issues/1/comments"],
            0,
            &shown.comments.to_string()
        ),
        answer(&["api", "repos/acme/widgets/pulls/1/comments"], 0, "[]"),
        answer(&["api", "repos/acme/widgets/pulls/1/reviews"], 0, "[]"),
        answer(&["api", "graphql"], 0, &threads.to_string()),
        answer(
            &[
                "api",
                &format!("repos/acme/widgets/commits/{}/check-runs", shown.head)
            ],
            0,
            r#"{"check_runs": []}"#
        ),
        answer(
            &[
                "api",
                &format!("repos/acme/widgets/compare/main...{}", shown.head)
            ],
            0,
            r#"{"behind_by": 0}"#
        ),
    ])
}

/// A checkout of `acme/widgets` whose branch `fix` holds one commit on top
/// of `main`.
fn checkout(h: &Harness) -> (PathBuf, String) {
    let path = h.git_repo("widgets");
    sh(
        &path,
        "git checkout -q -b fix && echo a > a.txt && git add a.txt && \
         git -c user.email=t@t -c user.name=t commit -qm a && git checkout -q main",
    );
    let head = sh(&path, "git rev-parse fix");
    (path, head)
}

/// The repository registered with its integration on, `fix` pushed to its
/// own bare remote: the forge CLI is stubbed, so only the push and the
/// `ls-remote` a keeper's teardown runs (032) need a real remote behind
/// `origin` — and behind the head repository URL the stub always names,
/// which a keeper's own push and safety check read straight off the
/// request rather than off `origin` (032).
async fn repository(h: &Harness, path: &Path) -> String {
    let repo = h.repository(path).await;
    with_forge(h, &repo).await;
    let remote = h.at("remote.git");
    sh(
        path,
        &format!(
            "git config 'url.{remote}.insteadOf' https://github.com/acme/widgets.git && \
             git push -q origin fix",
            remote = remote.display(),
        ),
    );
    repo.id
}

/// Ask Ariadne to babysit request `number` on `model` (032).
fn babysit(repo: &str, number: i64, body: Value) -> axum::http::Request<Body> {
    put_json(
        &format!("/v1/repositories/{repo}/pull-requests/{number}/ariadne-babysit"),
        body,
    )
}

/// How many detail fetches of the repository the stub has answered.
fn detail_fetches(stub: &StubForgeCli) -> usize {
    stub.invocations()
        .iter()
        .filter(|i| i.args.get(1).is_some_and(|a| a.contains("/compare/")))
        .count()
}

/// Fetch the repository once more, and wait for the resulting pass to end,
/// so a test that proves "no news the second time" first proves the
/// second fetch actually ran.
async fn fetch_again(h: &Harness, stub: &StubForgeCli, repo: &str) {
    let before = detail_fetches(stub);
    h.state.forge_poll.wake(repo);
    eventually(TIMEOUT, "the detail fetch", async || {
        detail_fetches(stub) > before
    })
    .await;
    h.flush_scheduler().await;
}

async fn sessions(h: &Harness, pull_request_id: &str) -> Vec<AgentSession> {
    h.store
        .list_sessions(SessionFilter {
            pull_request_id: Some(pull_request_id.to_string()),
            ..Default::default()
        })
        .await
        .unwrap()
}

/// The live keeper session of a request, once its briefing turn has ended.
async fn idle_keeper(h: &Harness, pull_request_id: &str) -> AgentSession {
    let mut found = None;
    eventually(TIMEOUT, "the keeper session to be idle", async || {
        found = sessions(h, pull_request_id).await.into_iter().find(|s| {
            s.seat() == Some(Seat::Keeper)
                && s.status() == SessionStatus::Idle
                && s.launched_at.is_some()
        });
        found.is_some()
    })
    .await;
    found.unwrap()
}

/// Asking Ariadne to babysit a request of mine starts a keeper session on
/// the pin picked, loading `pr-babysit`, in a worktree on a local branch
/// named after the request — distinct from a reviewer's own, detached
/// worktree. The author's news, a comment by another login, reaches it
/// once; its `ready` report is accepted; and the merge ends it, taking its
/// worktree and its branch down with it.
#[tokio::test]
async fn asking_keeps_a_request_of_mine_by_hand_until_the_merge() {
    let stub = stub_forge_cli(json!([]));
    let h = harness().scheduler().forge_cli(&stub).await;
    let (path, head) = checkout(&h);
    stub.reprogram(script(&Shown::open(&head)));
    let repo = repository(&h, &path).await;
    // Nobody works on a request of mine until there is a reason to: no
    // task opened it, nobody asked for a review of it, and nobody has
    // asked to babysit it yet.
    let mut rows: Vec<Value> = Vec::new();
    eventually(TIMEOUT, "the request to be listed", async || {
        rows = h.get("/v1/pull-requests").await;
        rows.len() == 1
    })
    .await;
    assert!(rows[0]["id"].is_null(), "nobody works on it yet");
    h.state.forge_poll.set_mode(&repo, Mode::WakeOnly);
    h.flush_scheduler().await;

    let asked: Value = h
        .json(
            babysit(
                &repo,
                1,
                json!({"asked": true, "model": PIN, "skills": ["coding"]}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(asked["babysit_asked"], true);
    assert_eq!(asked["babysit_model"], PIN);
    assert_eq!(asked["babysit_skills"], json!(["coding"]));
    // Asking starts Ariadne's work on the request.
    let id = asked["id"].as_str().expect("a row").to_string();
    assert_eq!(h.store.get_pull_request(&id).await.unwrap().role, "author");

    let session = idle_keeper(&h, &id).await;
    assert_eq!(
        sessions(&h, &id)
            .await
            .iter()
            .filter(|s| s.seat() == Some(Seat::Keeper) && s.status().is_live())
            .count(),
        1,
        "exactly one live keeper session, never two on the same request"
    );
    assert_eq!(session.model, PIN);
    let launch = h.launch_file(&session.id).expect("a launch file");
    assert!(
        launch.system_prompt.contains("- pr-babysit: "),
        "{}",
        launch.system_prompt
    );
    let worktree = PathBuf::from(session.worktree_path.clone().unwrap());
    assert!(
        worktree
            .display()
            .to_string()
            .ends_with(&format!("pr-{id}-keep")),
        "{}",
        worktree.display()
    );
    assert_eq!(
        sh(&worktree, "git rev-parse --abbrev-ref HEAD"),
        format!("ariadne/keep-{id}"),
        "the keeper works on its own branch, not a detached head"
    );
    assert_eq!(sh(&worktree, "git rev-parse HEAD"), head);

    // A comment by another login is the author's news, told once.
    stub.reprogram(script(&Shown {
        comments: json!([
            {"id": 301, "user": {"login": "alice"}, "body": "Looks good but one nit.",
             "created_at": "2026-10-02T00:00:00Z"}
        ]),
        ..Shown::open(&head)
    }));
    h.state.forge_poll.wake(&repo);
    eventually(TIMEOUT, "the news to reach the keeper", async || {
        h.prompts_to(&session)
            .last()
            .is_some_and(|news| news.contains("ic-301"))
    })
    .await;
    let told = h.prompts_to(&session).len();
    let news = h.prompts_to(&session).pop().unwrap();
    assert!(news.contains("alice"), "{news}");

    // The same comment is told once: a later fetch that reads the same
    // details hands the keeper no second prompt naming it.
    fetch_again(&h, &stub, &repo).await;
    tokio::time::sleep(QUIET).await;
    assert_eq!(
        h.prompts_to(&session).len(),
        told,
        "the same comment is no news the second time"
    );

    // A reply from the keeper follows the keeping agent's own rule (026
    // rule 16): it answers an inline review comment, the thread of a
    // defect, and a reply to the conversation itself is refused.
    h.error(
        as_session(
            &format!("/v1/pull-requests/{id}/comments/ic-301/reply"),
            &session.id,
            json!({"body": "Thanks!"}),
        ),
        StatusCode::BAD_REQUEST,
    )
    .await;

    // The keeper reports the request ready, on the head it read.
    let reported: Value = h
        .json(
            as_session(
                &format!("/v1/pull-requests/{id}/report"),
                &session.id,
                json!({"ready": true, "head_sha": head}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(reported["ready"], true);

    // The merge takes the keeper session, its worktree and its branch
    // down: nothing keeps the row any more, since no task opened it and
    // no review was ever asked of it.
    stub.reprogram(script(&Shown {
        state: "MERGED",
        ..Shown::open(&head)
    }));
    h.state.forge_poll.wake(&repo);
    eventually(
        TIMEOUT,
        "the keeper session to end, its worktree to go and its branch with it",
        async || {
            !h.session_status(&session).await.is_live()
                && !worktree.exists()
                && sh(&path, "git branch --list ariadne/keep-*").is_empty()
        },
    )
    .await;
    eventually(TIMEOUT, "Ariadne to stop working on it", async || {
        h.store.get_pull_request(&id).await.is_err()
    })
    .await;
}

/// Stopping before the keeper ever pushes leaves its worktree and its
/// branch on disk: teardown must not throw away the only copy of a commit
/// nobody has (032).
#[tokio::test]
async fn stopping_before_a_push_preserves_the_keepers_unpublished_commit() {
    let stub = stub_forge_cli(json!([]));
    let h = harness().scheduler().forge_cli(&stub).await;
    let (path, head) = checkout(&h);
    stub.reprogram(script(&Shown::open(&head)));
    let repo = repository(&h, &path).await;
    h.state.forge_poll.set_mode(&repo, Mode::WakeOnly);
    h.flush_scheduler().await;

    let asked: Value = h
        .json(
            babysit(&repo, 1, json!({"asked": true, "model": PIN})),
            StatusCode::OK,
        )
        .await;
    let id = asked["id"].as_str().expect("a row").to_string();
    let session = idle_keeper(&h, &id).await;
    let worktree = PathBuf::from(session.worktree_path.clone().unwrap());

    // The keeper commits a fix but never pushes it before the user stops
    // babysitting.
    sh(
        &worktree,
        "echo b > b.txt && git add b.txt && \
         git -c user.email=t@t -c user.name=t commit -qm b",
    );
    let unpublished = sh(&worktree, "git rev-parse HEAD");
    assert_ne!(unpublished, head, "the keeper has a commit of its own");

    let stopped: Value = h
        .json(babysit(&repo, 1, json!({"asked": false})), StatusCode::OK)
        .await;
    assert_eq!(stopped["babysit_asked"], false);

    eventually(TIMEOUT, "the keeper session to end", async || {
        !h.session_status(&session).await.is_live()
    })
    .await;
    eventually(TIMEOUT, "Ariadne to stop working on it", async || {
        h.store.get_pull_request(&id).await.is_err()
    })
    .await;
    assert!(
        worktree.exists(),
        "the worktree holding the unpublished commit is left on disk"
    );
    assert_eq!(
        sh(&worktree, "git rev-parse HEAD"),
        unpublished,
        "the commit is still there"
    );
    assert!(
        !sh(&path, "git branch --list ariadne/keep-*").is_empty(),
        "the branch naming that commit is left too"
    );
}

/// The branch itself holds the only copy of an unpublished commit once its
/// worktree is already gone: teardown must keep checking it against the
/// request's head repository rather than skip straight to deleting it
/// because there is no worktree left to check against (032).
#[tokio::test]
async fn a_branch_with_no_worktree_left_keeps_its_unpublished_commit_too() {
    let stub = stub_forge_cli(json!([]));
    let h = harness().scheduler().forge_cli(&stub).await;
    let (path, head) = checkout(&h);
    stub.reprogram(script(&Shown::open(&head)));
    let repo = repository(&h, &path).await;
    h.state.forge_poll.set_mode(&repo, Mode::WakeOnly);
    h.flush_scheduler().await;

    let asked: Value = h
        .json(
            babysit(&repo, 1, json!({"asked": true, "model": PIN})),
            StatusCode::OK,
        )
        .await;
    let id = asked["id"].as_str().expect("a row").to_string();
    let session = idle_keeper(&h, &id).await;
    let worktree = PathBuf::from(session.worktree_path.clone().unwrap());

    sh(
        &worktree,
        "echo b > b.txt && git add b.txt && \
         git -c user.email=t@t -c user.name=t commit -qm b",
    );
    let unpublished = sh(&worktree, "git rev-parse HEAD");
    let branch = format!("ariadne/keep-{id}");

    // The worktree is gone before teardown ever runs: the branch is its
    // only trace now, and nothing has checked it against the remote yet.
    sh(
        &path,
        &format!("git worktree remove --force '{}'", worktree.display()),
    );
    assert!(!worktree.exists());

    let _: Value = h
        .json(babysit(&repo, 1, json!({"asked": false})), StatusCode::OK)
        .await;
    eventually(TIMEOUT, "the keeper session to end", async || {
        !h.session_status(&session).await.is_live()
    })
    .await;
    eventually(TIMEOUT, "Ariadne to stop working on it", async || {
        h.store.get_pull_request(&id).await.is_err()
    })
    .await;
    assert_eq!(
        sh(&path, &format!("git rev-parse {branch}")),
        unpublished,
        "the branch naming the unpublished commit is left too"
    );
}

/// A second ask that moves the keeper's model while its session still runs
/// kills that session and starts a fresh one on the new pin, reusing the
/// same worktree and local branch rather than cutting them again (032).
#[tokio::test]
async fn a_second_ask_that_moves_the_pin_restarts_the_live_keeper() {
    const PIN2: &str = "stub:babysit-model-2";
    let stub = stub_forge_cli(json!([]));
    let h = harness().scheduler().forge_cli(&stub).await;
    let (path, head) = checkout(&h);
    stub.reprogram(script(&Shown::open(&head)));
    let repo = repository(&h, &path).await;
    h.state.forge_poll.set_mode(&repo, Mode::WakeOnly);
    h.flush_scheduler().await;

    let asked: Value = h
        .json(
            babysit(&repo, 1, json!({"asked": true, "model": PIN})),
            StatusCode::OK,
        )
        .await;
    let id = asked["id"].as_str().expect("a row").to_string();
    let session = idle_keeper(&h, &id).await;
    let worktree = session.worktree_path.clone().unwrap();

    let moved: Value = h
        .json(
            babysit(&repo, 1, json!({"asked": true, "model": PIN2})),
            StatusCode::OK,
        )
        .await;
    assert_eq!(moved["babysit_model"], PIN2);

    let mut restarted = None;
    eventually(
        TIMEOUT,
        "the keeper to restart on the new pin",
        async || {
            restarted = sessions(&h, &id).await.into_iter().find(|s| {
                s.seat() == Some(Seat::Keeper)
                    && s.model == PIN2
                    && s.status() == SessionStatus::Idle
                    && s.launched_at.is_some()
            });
            restarted.is_some()
        },
    )
    .await;
    let session2 = restarted.unwrap();
    assert_ne!(session2.id, session.id, "a new session, not the old one");
    assert!(
        !h.session_status(&session).await.is_live(),
        "the old pin's session is gone"
    );
    assert_eq!(
        session2.worktree_path.as_deref(),
        Some(worktree.as_str()),
        "the restart reuses the worktree rather than cutting another"
    );
    assert_eq!(
        sh(PathBuf::from(&worktree).as_path(), "git rev-parse HEAD"),
        head,
        "no work was lost across the restart"
    );
}

/// A second ask that moves only the keeper's skills, its model and effort
/// left as they were, relaunches the live session on its own row — the
/// same agent and model need no new one — rather than leave it running on
/// the skills it was first briefed on: the skills a running session loads
/// are not on its own row, read instead off the pin the scheduler last
/// applied, so a skills-only change must move that mark as surely as a
/// model change does (032).
#[tokio::test]
async fn a_second_ask_that_moves_only_the_skills_restarts_the_live_keeper() {
    let stub = stub_forge_cli(json!([]));
    let h = harness().scheduler().forge_cli(&stub).await;
    let (path, head) = checkout(&h);
    stub.reprogram(script(&Shown::open(&head)));
    let repo = repository(&h, &path).await;
    h.state.forge_poll.set_mode(&repo, Mode::WakeOnly);
    h.flush_scheduler().await;

    let asked: Value = h
        .json(
            babysit(
                &repo,
                1,
                json!({"asked": true, "model": PIN, "skills": ["coding"]}),
            ),
            StatusCode::OK,
        )
        .await;
    let id = asked["id"].as_str().expect("a row").to_string();
    let session = idle_keeper(&h, &id).await;
    let first_launch = session.launch_id.clone();

    let moved: Value = h
        .json(
            babysit(
                &repo,
                1,
                json!({"asked": true, "model": PIN, "skills": ["migration"]}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(moved["babysit_skills"], json!(["migration"]));

    eventually(
        TIMEOUT,
        "the keeper to relaunch, its model and effort letting it keep its row",
        async || {
            sessions(&h, &id).await.into_iter().any(|s| {
                s.seat() == Some(Seat::Keeper)
                    && s.id == session.id
                    && s.status() == SessionStatus::Idle
                    && s.launch_id.is_some()
                    && s.launch_id != first_launch
            })
        },
    )
    .await;
    let launch = h.launch_file(&session.id).expect("a launch file");
    assert!(
        launch.system_prompt.contains("- migration: "),
        "{}",
        launch.system_prompt
    );
    assert!(
        !launch.system_prompt.contains("- coding: "),
        "{}",
        launch.system_prompt
    );
}

/// A keeper pushes a fix onto the request's own head repository, not the
/// base repository's remote: a fork's head lives away from it, and the
/// command the briefing names must reach the fork, not `origin` (032).
#[tokio::test]
async fn a_keeper_on_a_fork_is_told_to_push_to_the_forks_own_repository() {
    let stub = stub_forge_cli(json!([]));
    let h = harness().scheduler().forge_cli(&stub).await;
    let (path, head) = checkout(&h);
    stub.reprogram(script(&Shown {
        head_repo: "https://github.com/alice/widgets",
        ..Shown::open(&head)
    }));
    let repo = repository(&h, &path).await;
    h.state.forge_poll.set_mode(&repo, Mode::WakeOnly);
    h.flush_scheduler().await;

    let asked: Value = h
        .json(
            babysit(&repo, 1, json!({"asked": true, "model": PIN})),
            StatusCode::OK,
        )
        .await;
    let id = asked["id"].as_str().expect("a row").to_string();
    let session = idle_keeper(&h, &id).await;

    let told = h.told(&session.id);
    assert!(
        told.contains("git push https://github.com/alice/widgets.git HEAD:fix"),
        "{told}"
    );
    assert!(
        !told.contains("git push origin"),
        "a fork's head is not origin's to push to: {told}"
    );
}

/// A keeper on the same repository as its base pushes through the
/// repository's own configured remote, not the HTTPS URL the forge always
/// reports for the head: an SSH-only remote could never push to that one
/// (032).
#[tokio::test]
async fn a_keeper_on_the_same_repository_pushes_through_the_configured_remote() {
    let stub = stub_forge_cli(json!([]));
    let h = harness().scheduler().forge_cli(&stub).await;
    let (path, head) = checkout(&h);
    stub.reprogram(script(&Shown::open(&head)));
    let repo = repository(&h, &path).await;
    h.state.forge_poll.set_mode(&repo, Mode::WakeOnly);
    h.flush_scheduler().await;

    let asked: Value = h
        .json(
            babysit(&repo, 1, json!({"asked": true, "model": PIN})),
            StatusCode::OK,
        )
        .await;
    let id = asked["id"].as_str().expect("a row").to_string();
    let session = idle_keeper(&h, &id).await;

    let told = h.told(&session.id);
    assert!(told.contains("git push origin HEAD:fix"), "{told}");
}

/// A merge that deletes its head branch at once still lets teardown clean
/// up a commit the keeper already pushed: `ls-remote` finds no ref any
/// more, but the local tip is exactly the head the forge itself last read
/// for the request, which is confirmation enough that it is not only on
/// disk here (032).
#[tokio::test]
async fn a_merge_that_auto_deletes_the_head_branch_still_cleans_up_published_work() {
    let stub = stub_forge_cli(json!([]));
    let h = harness().scheduler().forge_cli(&stub).await;
    let (path, head) = checkout(&h);
    stub.reprogram(script(&Shown::open(&head)));
    let repo = repository(&h, &path).await;
    h.state.forge_poll.set_mode(&repo, Mode::WakeOnly);
    h.flush_scheduler().await;

    let asked: Value = h
        .json(
            babysit(&repo, 1, json!({"asked": true, "model": PIN})),
            StatusCode::OK,
        )
        .await;
    let id = asked["id"].as_str().expect("a row").to_string();
    let session = idle_keeper(&h, &id).await;
    let worktree = PathBuf::from(session.worktree_path.clone().unwrap());
    let branch = format!("ariadne/keep-{id}");

    // The keeper pushes its fix to the request's head branch.
    sh(
        &worktree,
        "echo b > b.txt && git add b.txt && \
         git -c user.email=t@t -c user.name=t commit -qm b",
    );
    let pushed = sh(&worktree, "git rev-parse HEAD");
    sh(&worktree, &format!("git push -q origin {branch}:fix"));

    // GitHub records the pushed head, merges it, and deletes its head
    // branch — all before Ariadne's own teardown ever runs.
    sh(&path, "git push -q origin --delete fix");
    stub.reprogram(script(&Shown {
        state: "MERGED",
        ..Shown::open(&pushed)
    }));
    h.state.forge_poll.wake(&repo);

    eventually(
        TIMEOUT,
        "the keeper session to end, its worktree to go and its branch with it",
        async || {
            !h.session_status(&session).await.is_live()
                && !worktree.exists()
                && sh(&path, "git branch --list ariadne/keep-*").is_empty()
        },
    )
    .await;
    eventually(TIMEOUT, "Ariadne to stop working on it", async || {
        h.store.get_pull_request(&id).await.is_err()
    })
    .await;
}

/// A keeper's agent that comes up and exits at once — a broken binary, a
/// pin that will not run — spends the same spawn-retry budget a dead
/// reviewer launch does (`start_pull_request_session`), on a budget key of
/// its own: three dead launches, and Ariadne stops trying, rather than a
/// fresh session every pass for as long as the row lives (032).
#[tokio::test]
async fn a_keeper_whose_agent_dies_the_moment_it_starts_gives_up_after_the_budget() {
    let stub = stub_forge_cli(json!([]));
    let h = harness()
        .scheduler()
        .dying_agent()
        .timeouts(Timeouts {
            full_reconcile: SWEEPS_SOON,
            ..Timeouts::default()
        })
        .forge_cli(&stub)
        .await;
    let (path, head) = checkout(&h);
    stub.reprogram(script(&Shown::open(&head)));
    let repo = repository(&h, &path).await;
    let mut rows: Vec<Value> = Vec::new();
    eventually(TIMEOUT, "the request to be listed", async || {
        rows = h.get("/v1/pull-requests").await;
        rows.len() == 1
    })
    .await;

    let asked: Value = h
        .json(
            babysit(&repo, 1, json!({"asked": true, "model": PIN})),
            StatusCode::OK,
        )
        .await;
    let id = asked["id"].as_str().expect("a row").to_string();

    eventually(
        TIMEOUT,
        "the dead launches to run the budget out",
        async || {
            h.state.forge_poll.wake(&repo);
            let rows = sessions(&h, &id).await;
            rows.len() >= SPAWN_RETRY_BUDGET as usize && rows.iter().all(|s| !s.status().is_live())
        },
    )
    .await;

    // And nothing has been started again: given up on, rather than between
    // two launches. Three more wakes is what "a tick later" needs proving,
    // and a flush sent after them is what proves they ran, rather than a
    // wait that only bets they were fast enough.
    let exhausted = sessions(&h, &id).await.len();
    for _ in 0..3 {
        h.state.forge_poll.wake(&repo);
    }
    h.flush_scheduler().await;
    assert_eq!(
        sessions(&h, &id).await.len(),
        exhausted,
        "a keeper given up on after the budget is not spawned again"
    );
}

/// Asking needs a model the catalog holds, and a skill a task agent is
/// staffed on. A refusal starts no work. Only a request of the user's own
/// that no task opened can be asked for: a request by another login, or
/// one a task already keeps, is refused with 409.
#[tokio::test]
async fn asking_is_refused_with_no_model_an_unknown_one_an_untaskable_skill_or_the_wrong_request() {
    let stub = stub_forge_cli(json!([]));
    let h = harness().forge_cli(&stub).await;
    let (path, head) = checkout(&h);
    stub.reprogram(script(&Shown::open(&head)));
    let repo = repository(&h, &path).await;

    h.error(
        babysit(&repo, 1, json!({"asked": true})),
        StatusCode::BAD_REQUEST,
    )
    .await;
    h.error(
        babysit(&repo, 1, json!({"asked": true, "model": "nosuch:model"})),
        StatusCode::BAD_REQUEST,
    )
    .await;
    h.error(
        babysit(
            &repo,
            1,
            json!({"asked": true, "model": PIN, "skills": ["orchestration"]}),
        ),
        StatusCode::BAD_REQUEST,
    )
    .await;
    assert!(
        h.store
            .list_pull_requests(ariadne_store::PullRequestFilter::default())
            .await
            .unwrap()
            .is_empty(),
        "a refusal starts no work"
    );

    // A request not open is refused once asking actually starts work.
    stub.reprogram(script(&Shown {
        state: "CLOSED",
        ..Shown::open(&head)
    }));
    h.error(
        babysit(&repo, 1, json!({"asked": true, "model": PIN})),
        StatusCode::CONFLICT,
    )
    .await;

    // A request by another login is never Ariadne's to babysit by hand.
    stub.reprogram(script(&Shown {
        author: "other",
        ..Shown::open(&head)
    }));
    h.error(
        babysit(&repo, 1, json!({"asked": true, "model": PIN})),
        StatusCode::CONFLICT,
    )
    .await;

    // A request a task already keeps is refused too: `origin_task_id` on
    // its row is enough to prove it, with no need to drive a whole task
    // through its own workflow just to set it.
    stub.reprogram(script(&Shown::open(&head)));
    let goal = h
        .store
        .create_goal(ariadne_store::NewGoal {
            workflow: None,
            title: "Widgets".into(),
            description: "d".into(),
            issue_url: None,
            repository_ids: vec![repo.clone()],
            pin: ariadne_store::AgentPin {
                model: "stub:m".into(),
                effort: None,
            },
        })
        .await
        .unwrap();
    let task = h
        .store
        .create_task(ariadne_store::NewTask {
            goal_id: goal.id.clone(),
            repo_id: repo.clone(),
            title: "Fix widgets".into(),
            description: "d".into(),
            agents: Vec::new(),
            depends_on: Vec::new(),
        })
        .await
        .unwrap();
    h.store
        .upsert_pull_request(ariadne_store::NewPullRequest {
            repository_id: repo.clone(),
            number: 1,
            url: "https://github.com/acme/widgets/pull/1".into(),
            role: "author".into(),
            origin_task_id: Some(task.id),
        })
        .await
        .unwrap();
    h.error(
        babysit(&repo, 1, json!({"asked": true, "model": PIN})),
        StatusCode::CONFLICT,
    )
    .await;
}
