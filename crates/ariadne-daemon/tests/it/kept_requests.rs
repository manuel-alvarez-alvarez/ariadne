//! A request of mine is kept by the author of the task that opened it, until
//! a human merges or closes it (005, 026): the detail fetch, the news handed
//! to that author's own session, the tools it answers through, and the
//! cleanup once the request ended and its task is over. A request of mine
//! gets no session of its own.
use std::path::PathBuf;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{Value, json};

use crate::common::forge::{StubForgeCli, answer, stub_forge_cli};
use crate::common::{Harness, QUIET, TIMEOUT, as_session, eventually, harness, post_json, sh};
use crate::landing_lifecycle::{walk_to_approved, with_forge};
use ariadne_api::SESSION_HEADER;
use ariadne_api::tasks::TaskDto;
use ariadne_core::{AttentionReason, Landing, SessionStatus, TaskStatus};
use ariadne_daemon::forge::poll::Mode;
use ariadne_daemon::scheduler::SchedEvent;
use ariadne_store::{AgentSession, NewGoal, SessionFilter, Task};

/// Where `pr create` says the task's request is.
const URL: &str = "https://github.com/acme/widgets/pull/1";

fn pull(state: &str, sha: &str) -> Value {
    json!({"number": 1, "url": URL,
        "title": "Fix widgets", "author": {"login": "me"}, "state": state, "isDraft": false,
        "headRefName": "fix", "headRefOid": sha,
        "headRepository": {"url": "https://github.com/acme/widgets"},
        "baseRefName": "main", "statusCheckRollup": [], "reviewDecision": "",
        "createdAt": "2026-10-01T00:00:00Z"})
}

fn review_comment(id: i64, login: &str, reply_to: Option<i64>, at: &str) -> Value {
    json!({"id": id, "in_reply_to_id": reply_to, "user": {"login": login, "type": "User"},
        "body": format!("comment {id} by {login}"), "path": "src/lib.rs", "line": 3,
        "created_at": at})
}

/// What the stub forge answers: the request in `state` on its own read, its
/// review comments in one thread, and its check runs. No list holds it: a
/// fetch lists the review requests alone, and reads the request on its own.
fn script(state: &str, comments: &[Value], check_runs: Value) -> Value {
    let threads = json!({"data": {"repository": {"pullRequest": {"reviewThreads": {"nodes": [
        {"id": "T1", "isResolved": false, "comments": {"nodes": [{"databaseId": 101}]}}
    ]}}}}});
    script_of(
        pull(state, "abc"),
        &[],
        comments,
        &json!({"check_runs": check_runs}).to_string(),
        &threads.to_string(),
    )
}

/// The same, for a request read as `read` and lists that hold `listed`, with
/// the check runs and the review threads as `gh api --paginate` writes them:
/// one JSON value per page, back to back.
fn script_of(
    read: Value,
    listed: &[Value],
    comments: &[Value],
    check_runs: &str,
    threads: &str,
) -> Value {
    let read = read.to_string();
    json!([
        answer(&["auth", "status"], 0, ""),
        answer(&["api", "user"], 0, "me"),
        answer(&["pr", "create"], 0, URL),
        answer(&["pr", "list"], 0, &json!(listed).to_string()),
        answer(&["pr", "view"], 0, &read),
        answer(
            &["pr", "comment"],
            0,
            "https://github.com/acme/widgets/pull/1#issuecomment-301"
        ),
        answer(
            &["api", "repos/acme/widgets/pulls/1/comments"],
            0,
            &json!(comments).to_string()
        ),
        answer(&["api", "repos/acme/widgets/issues/1/comments"], 0, "[]"),
        answer(&["api", "repos/acme/widgets/pulls/1/reviews"], 0, "[]"),
        answer(
            &["api", "repos/acme/widgets/pulls/1/comments/101/replies"],
            0,
            r#"{"id": 201}"#
        ),
        answer(&["api", "graphql"], 0, threads),
        answer(
            &["api", "repos/acme/widgets/commits/abc/check-runs"],
            0,
            check_runs
        ),
        answer(
            &["api", "repos/acme/widgets/compare/main...abc"],
            0,
            r#"{"behind_by": 0}"#
        ),
    ])
}

fn quiet_script() -> Value {
    script("OPEN", &[], json!([]))
}

/// A task that lands by request, walked to its approval, whose author opened
/// its request: the request the ledger keeps for it, the author's own
/// session, and the checkout with its bare remote.
struct Kept {
    h: Harness,
    stub: StubForgeCli,
    task: Task,
    author: AgentSession,
    /// The ledger row of the task's request.
    id: String,
    repo: String,
    path: PathBuf,
    bare: PathBuf,
}

async fn kept_request(script: Value) -> Kept {
    let stub = stub_forge_cli(script);
    let h = harness()
        .scheduler()
        .discover_agents()
        .forge_cli(&stub)
        .await;
    let path = h.git_repo("repo");
    let cast = h.active_cast_ending_in(Landing::PullRequest).await;
    with_forge(&h, &cast.repo).await;
    let bare = h.at("remote.git");
    let task = cast.task.clone();
    let (worktree, author) = walk_to_approved(&h, &task, &cast.reviewer.id).await;
    sh(&worktree, "git push -q origin HEAD");
    let opened: TaskDto = h
        .json(
            as_session(
                &format!("/v1/tasks/{}/pull-request", task.id),
                &author.id,
                json!({"title": "Fix widgets", "body": "Fixes widgets."}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(opened.pr_url.as_deref(), Some(URL));
    let id = h
        .store
        .pull_request_of_task(&task.id)
        .await
        .unwrap()
        .expect("the ledger holds the task's request")
        .id;
    let repo = cast.repo.id.clone();
    h.state.forge_poll.set_mode(&repo, Mode::WakeOnly);
    eventually(TIMEOUT, "the author's landing turn to end", async || {
        h.session_status(&author).await == SessionStatus::Idle
    })
    .await;
    Kept {
        h,
        stub,
        task,
        author,
        id,
        repo,
        path,
        bare,
    }
}

/// How many detail fetches the stub has answered: one compare per fetch.
fn detail_fetches(stub: &StubForgeCli) -> usize {
    stub.invocations()
        .iter()
        .filter(|i| i.args.get(1).is_some_and(|a| a.contains("/compare/")))
        .count()
}

/// Fetch the repository once more and let the scheduler act on it.
async fn fetch_again(h: &Harness, stub: &StubForgeCli, repo: &str) {
    let before = detail_fetches(stub);
    h.state.forge_poll.wake(repo);
    eventually(TIMEOUT, "the detail fetch", async || {
        detail_fetches(stub) > before
    })
    .await;
    tokio::time::sleep(QUIET).await;
    h.flush_scheduler().await;
}

/// Wake the scheduler about one request and wait for that pass to end.
async fn pass_over(h: &Harness, pull_request_id: &str) {
    h.sched
        .as_ref()
        .expect("this harness has a scheduler")
        .send(SchedEvent::PullRequestChanged(pull_request_id.into()))
        .unwrap();
    h.flush_scheduler().await;
}

fn get_as(uri: &str, session_id: &str) -> Request<Body> {
    Request::builder()
        .uri(uri)
        .header(SESSION_HEADER, session_id)
        .body(Body::empty())
        .unwrap()
}

/// A finish of the task, by its author, with `sha`.
fn finish(task: &Task, author: &AgentSession, sha: &str) -> Request<Body> {
    as_session(
        &format!("/v1/tasks/{}/transitions", task.id),
        &author.id,
        json!({"to": "finished", "merge_commit": sha}),
    )
}

/// A new comment by another login is stored, counted, and handed to the
/// task's author in one prompt that names it; the same details again hand
/// nothing; a check that turned to failure is handed by name. The request has
/// no session of its own: its row names the author's.
#[tokio::test]
async fn the_news_of_its_request_reaches_the_author_once() {
    let Kept {
        h,
        stub,
        author,
        id,
        repo,
        ..
    } = kept_request(quiet_script()).await;
    let briefed = h.prompts_to(&author).len();

    let comments = [review_comment(101, "alice", None, "2026-10-02T00:00:00Z")];
    stub.reprogram(script("OPEN", &comments, json!([])));
    fetch_again(&h, &stub, &repo).await;
    let stored: Vec<Value> = h.get(&format!("/v1/pull-requests/{id}/comments")).await;
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0]["thread_id"], "T1");
    assert_eq!(stored[0]["author_login"], "alice");
    let comment_id = stored[0]["id"].as_str().unwrap().to_string();
    let dto: Value = h.get(&format!("/v1/pull-requests/{id}")).await;
    assert_eq!(dto["unanswered_comments"], 1);
    assert_eq!(
        dto["session_id"], author.id,
        "the request's session is its author's"
    );
    eventually(TIMEOUT, "the news to reach the author", async || {
        h.prompts_to(&author).len() == briefed + 1
    })
    .await;
    let news = h.prompts_to(&author).pop().unwrap();
    assert!(news.contains(&comment_id), "{news}");
    assert!(news.contains("alice"), "{news}");

    fetch_again(&h, &stub, &repo).await;
    assert_eq!(
        h.prompts_to(&author).len(),
        briefed + 1,
        "the same details are no news"
    );

    let failed = json!([{"name": "lint", "html_url": "https://ci.example/1",
        "conclusion": "failure", "status": "completed"}]);
    stub.reprogram(script("OPEN", &comments, failed));
    fetch_again(&h, &stub, &repo).await;
    eventually(
        TIMEOUT,
        "the failed check to reach the author",
        async || h.prompts_to(&author).len() == briefed + 2,
    )
    .await;
    let news = h.prompts_to(&author).pop().unwrap();
    assert!(news.contains("Check lint turned to failure"), "{news}");
    assert!(
        !news.contains(&comment_id),
        "a comment is told once: {news}"
    );
    let dto: Value = h.get(&format!("/v1/pull-requests/{id}")).await;
    assert_eq!(dto["failed_checks"][0]["name"], "lint");

    assert!(
        h.store
            .list_sessions(SessionFilter {
                pull_request_id: Some(id.clone()),
                ..Default::default()
            })
            .await
            .unwrap()
            .is_empty(),
        "a request of mine gets no session of its own"
    );
}

/// The author replies through the forge's reply command, stored as my
/// comment in the thread, which answers it; nothing resolves a thread. Its
/// `ready: true` raises `waiting_user` on its own session once, and
/// `ready: false` takes it down. Any other session is refused, and so is a
/// call that comes from no session.
#[tokio::test]
async fn the_author_replies_and_reports_and_no_other_session_may() {
    let comments = [review_comment(101, "alice", None, "2026-10-02T00:00:00Z")];
    let Kept {
        h,
        stub,
        author,
        id,
        repo,
        ..
    } = kept_request(script("OPEN", &comments, json!([]))).await;
    fetch_again(&h, &stub, &repo).await;
    let mut stored: Vec<Value> = Vec::new();
    eventually(TIMEOUT, "the comment to be told", async || {
        stored = h.get(&format!("/v1/pull-requests/{id}/comments")).await;
        stored.len() == 1 && stored[0]["told_at"].is_string()
    })
    .await;
    let comment_id = stored[0]["id"].as_str().unwrap().to_string();
    eventually(TIMEOUT, "the author to read the comment", async || {
        h.prompts_to(&author)
            .iter()
            .any(|p| p.contains(&comment_id))
    })
    .await;

    let reply: Value = h
        .json(
            as_session(
                &format!("/v1/pull-requests/{id}/comments/{comment_id}/reply"),
                &author.id,
                json!({"body": "Renamed it."}),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert_eq!(reply["author_login"], "me");
    assert_eq!(reply["thread_id"], "T1");
    let call = stub
        .invocations()
        .into_iter()
        .find(|i| i.args.iter().any(|a| a.ends_with("/comments/101/replies")))
        .expect("the reply command");
    assert!(
        call.args.contains(&"body=Renamed it.".to_string()),
        "{call:?}"
    );
    let dto: Value = h.get(&format!("/v1/pull-requests/{id}")).await;
    assert_eq!(dto["unanswered_comments"], 0);
    for call in stub.invocations() {
        let line = call.args.join(" ");
        assert!(
            !line.contains("resolveReviewThread") && !line.contains("resolved=true"),
            "{line}"
        );
    }

    let report = |ready: bool| {
        as_session(
            &format!("/v1/pull-requests/{id}/report"),
            &author.id,
            json!({"ready": ready}),
        )
    };
    let dto: Value = h.json(report(true), StatusCode::OK).await;
    assert_eq!(dto["ready"], true);
    assert_eq!(
        h.attention(&author).await,
        Some(AttentionReason::WaitingUser)
    );
    let _: Value = h.json(report(false), StatusCode::OK).await;
    assert_eq!(h.attention(&author).await, None);

    let stranger = h.lone_session("stranger").await;
    h.error(
        as_session(
            &format!("/v1/pull-requests/{id}/report"),
            &stranger.id,
            json!({"ready": true}),
        ),
        StatusCode::FORBIDDEN,
    )
    .await;
    h.error(
        get_as(&format!("/v1/pull-requests/{id}/comments"), &stranger.id),
        StatusCode::FORBIDDEN,
    )
    .await;
    h.error(
        post_json(
            &format!("/v1/pull-requests/{id}/report"),
            json!({"ready": true}),
        ),
        StatusCode::FORBIDDEN,
    )
    .await;
}

/// The task stays approved while its request is open, and its finish is
/// refused. A merge is told to the author, whose finish is then accepted; the
/// request ended with its task, so its cleanup is recorded.
#[tokio::test]
async fn a_merge_is_told_to_the_author_whose_finish_it_then_accepts() {
    let Kept {
        h,
        stub,
        task,
        author,
        id,
        repo,
        ..
    } = kept_request(quiet_script()).await;
    let tip = sh(&h.at("repo"), &format!("git rev-parse {}", task.branch));
    assert_eq!(h.status(&task.id).await, TaskStatus::Approved);
    let refused = h
        .error(finish(&task, &author, &tip), StatusCode::CONFLICT)
        .await;
    assert!(
        refused.error.message.contains("is not merged"),
        "{}",
        refused.error.message
    );

    stub.reprogram(script("MERGED", &[], json!([])));
    h.state.forge_poll.wake(&repo);
    eventually(TIMEOUT, "the merge to reach the author", async || {
        h.prompted(&author).contains("The request is now merged.")
    })
    .await;
    let finished: TaskDto = h.json(finish(&task, &author, &tip), StatusCode::OK).await;
    assert_eq!(finished.status, TaskStatus::Finished);
    eventually(
        TIMEOUT,
        "the request's cleanup to be recorded",
        async || {
            pass_over(&h, &id).await;
            h.store
                .get_pull_request(&id)
                .await
                .unwrap()
                .cleaned_at
                .is_some()
        },
    )
    .await;
}

/// A close is told to the author too, and a closed request finishes nothing:
/// the author fails the task instead.
#[tokio::test]
async fn a_close_is_told_to_the_author_and_finishes_nothing() {
    let Kept {
        h,
        stub,
        task,
        author,
        repo,
        ..
    } = kept_request(quiet_script()).await;
    stub.reprogram(script("CLOSED", &[], json!([])));
    h.state.forge_poll.wake(&repo);
    eventually(TIMEOUT, "the close to reach the author", async || {
        h.prompted(&author).contains("The request is now closed.")
    })
    .await;
    let tip = sh(&h.at("repo"), &format!("git rev-parse {}", task.branch));
    h.error(finish(&task, &author, &tip), StatusCode::CONFLICT)
        .await;
    assert_eq!(h.status(&task.id).await, TaskStatus::Approved);
}

/// A check that fails is told, its recovery is no news but is recorded, and
/// its next failure is told again.
#[tokio::test]
async fn a_check_that_recovers_and_fails_again_is_told_again() {
    let Kept {
        h,
        stub,
        author,
        repo,
        ..
    } = kept_request(quiet_script()).await;
    let failed = json!([{"name": "lint", "html_url": "https://ci.example/1",
        "conclusion": "failure"}]);
    let told_lint = || {
        h.prompts_to(&author)
            .iter()
            .filter(|p| p.contains("Check lint turned to failure"))
            .count()
    };

    stub.reprogram(script("OPEN", &[], failed.clone()));
    fetch_again(&h, &stub, &repo).await;
    eventually(TIMEOUT, "the failure to be told", async || told_lint() == 1).await;

    stub.reprogram(quiet_script());
    fetch_again(&h, &stub, &repo).await;
    let after_recovery = h.prompts_to(&author).len();

    stub.reprogram(script("OPEN", &[], failed));
    fetch_again(&h, &stub, &repo).await;
    eventually(TIMEOUT, "the second failure to be told", async || {
        told_lint() == 2
    })
    .await;
    assert_eq!(
        h.prompts_to(&author).len(),
        after_recovery + 1,
        "the recovery itself was no news"
    );
}

/// Every page is read: a resolved thread on the second page of review
/// threads counts nothing, and a failed check on the second page of check
/// runs is a failure.
#[tokio::test]
async fn the_detail_fetch_reads_every_page_of_threads_and_check_runs() {
    let comments = [
        review_comment(101, "alice", None, "2026-10-02T00:00:00Z"),
        review_comment(102, "bob", None, "2026-10-02T00:00:00Z"),
    ];
    let page = |id: &str, resolved: bool, comment: i64, next: bool| {
        json!({"data": {"repository": {"pullRequest": {"reviewThreads": {
            "pageInfo": {"hasNextPage": next, "endCursor": "c1"},
            "nodes": [{"id": id, "isResolved": resolved,
                "comments": {"nodes": [{"databaseId": comment}]}}]}}}}})
    };
    let threads = format!(
        "{}{}",
        page("T1", false, 101, true),
        page("T2", true, 102, false)
    );
    let check_runs = format!(
        "{}{}",
        json!({"total_count": 2, "check_runs": [{"name": "build", "html_url": "",
            "conclusion": "success"}]}),
        json!({"total_count": 2, "check_runs": [{"name": "lint", "html_url": "",
            "conclusion": "failure"}]})
    );
    let Kept {
        h, stub, id, repo, ..
    } = kept_request(script_of(
        pull("OPEN", "abc"),
        &[],
        &comments,
        &check_runs,
        &threads,
    ))
    .await;
    fetch_again(&h, &stub, &repo).await;
    let dto: Value = h.get(&format!("/v1/pull-requests/{id}")).await;
    assert_eq!(dto["failed_checks"][0]["name"], "lint");
    assert_eq!(
        dto["unanswered_comments"], 1,
        "a resolved thread counts zero"
    );
    let stored: Vec<Value> = h.get(&format!("/v1/pull-requests/{id}/comments")).await;
    let bob = stored.iter().find(|c| c["author_login"] == "bob").unwrap();
    assert_eq!(bob["thread_id"], "T2");
    assert_eq!(bob["resolved"], true);
    for call in stub.invocations() {
        let paged = call
            .args
            .get(1)
            .is_some_and(|a| a == "graphql" || a.ends_with("/check-runs"));
        if paged {
            assert!(call.args.contains(&"--paginate".to_string()), "{call:?}");
        }
    }
}

/// A goal on the repository whose goal branch is `branch`.
async fn goal_on_branch(h: &Harness, repository_id: &str, branch: &str) {
    let goal = h
        .store
        .create_goal(NewGoal {
            issue_url: None,
            landing: None,
            title: "Ship widgets".into(),
            description: "desc".into(),
            repository_ids: vec![repository_id.into()],
            pin: crate::common::test_pin(),
        })
        .await
        .unwrap();
    h.store
        .set_goal_branch(&goal.id, repository_id, branch)
        .await
        .unwrap();
}

/// A merged request whose head is a goal branch takes that branch down,
/// local and remote, once its task is over. A remote delete that fails is
/// not forgotten: the cleanup stays pending on the row, and the first pass
/// of a restarted daemon after the remote recovered deletes it.
#[tokio::test]
async fn a_merged_goal_branch_goes_once_its_task_is_over_and_a_failed_remote_delete_is_tried_again()
{
    let Kept {
        h,
        stub,
        task,
        author,
        id,
        repo,
        path,
        bare,
    } = kept_request(quiet_script()).await;
    // The request's head is `fix`, a goal branch, on the checkout and on
    // the remote; the remote cannot be pushed to yet.
    sh(&path, "git branch fix && git push -q origin fix");
    goal_on_branch(&h, &repo, "fix").await;
    let gone = h.at("no-such-remote.git");
    sh(
        &path,
        &format!("git remote set-url --push origin '{}'", gone.display()),
    );

    stub.reprogram(script("MERGED", &[], json!([])));
    h.state.forge_poll.wake(&repo);
    eventually(TIMEOUT, "the merge to reach the author", async || {
        h.prompted(&author).contains("The request is now merged.")
    })
    .await;
    pass_over(&h, &id).await;
    assert_eq!(
        sh(&path, "git branch --list fix"),
        "fix",
        "the goal branch stays while its task is not over"
    );
    let tip = sh(&path, &format!("git rev-parse {}", task.branch));
    let _: TaskDto = h.json(finish(&task, &author, &tip), StatusCode::OK).await;
    eventually(TIMEOUT, "the local goal branch to go", async || {
        pass_over(&h, &id).await;
        sh(&path, "git branch --list fix").is_empty()
    })
    .await;
    assert_eq!(
        sh(&bare, "git branch --list fix"),
        "fix",
        "the remote could not be reached"
    );
    assert!(
        h.store
            .get_pull_request(&id)
            .await
            .unwrap()
            .cleaned_at
            .is_none(),
        "the cleanup stays owed"
    );

    sh(
        &path,
        &format!("git remote set-url --push origin '{}'", bare.display()),
    );
    // A daemon that restarts owes the cleanup too: a scheduler of its own,
    // with nothing in memory, takes the remote branch down on its first
    // full pass, with no change of the request to wake it.
    let _restarted =
        ariadne_daemon::scheduler::start(h.store.clone(), h.launcher.clone(), false, h.timeouts);
    eventually(TIMEOUT, "the remote goal branch to go", async || {
        sh(&bare, "git branch --list fix").is_empty()
    })
    .await;
}

/// A request the user reviews is no lifecycle of this one: closing it takes
/// down nothing, not even a local branch of the same name as its head.
#[tokio::test]
async fn a_closed_request_i_review_deletes_no_branch() {
    let mut read = pull("OPEN", "abc");
    read["author"] = json!({"login": "other"});
    let threads =
        json!({"data": {"repository": {"pullRequest": {"reviewThreads": {"nodes": []}}}}});
    let stub = stub_forge_cli(script_of(
        read.clone(),
        std::slice::from_ref(&read),
        &[],
        r#"{"check_runs": []}"#,
        &threads.to_string(),
    ));
    let h = harness().scheduler().forge_cli(&stub).await;
    let path = h.git_repo("widgets");
    sh(
        &path,
        "git branch fix && git remote add origin https://github.com/acme/widgets.git",
    );
    let created: Value = h
        .json(
            post_json(
                "/v1/repositories",
                json!({"path": path, "forge": {"enabled": true}}),
            ),
            StatusCode::CREATED,
        )
        .await;
    let repo = created["id"].as_str().unwrap().to_string();
    let mut rows: Vec<Value> = Vec::new();
    eventually(TIMEOUT, "the request to be recorded", async || {
        rows = h.get("/v1/pull-requests?state=all").await;
        rows.len() == 1
    })
    .await;
    assert_eq!(rows[0]["role"], "reviewer");
    let id = rows[0]["id"].as_str().unwrap().to_string();
    h.state.forge_poll.set_mode(&repo, Mode::WakeOnly);

    read["state"] = json!("CLOSED");
    stub.reprogram(script_of(
        read,
        &[],
        &[],
        r#"{"check_runs": []}"#,
        &threads.to_string(),
    ));
    h.state.forge_poll.wake(&repo);
    eventually(TIMEOUT, "the request to close", async || {
        h.get::<Value>(&format!("/v1/pull-requests/{id}")).await["state"] == "closed"
    })
    .await;
    pass_over(&h, &id).await;
    assert_eq!(sh(&path, "git branch --list fix"), "fix");
    assert!(
        h.store
            .list_sessions(SessionFilter {
                pull_request_id: Some(id),
                ..Default::default()
            })
            .await
            .unwrap()
            .is_empty()
    );
}
