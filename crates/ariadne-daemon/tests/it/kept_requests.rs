//! A request of mine is kept by the agent of the `pr` column of the task
//! that opened it, until a human merges or closes it (030, 026): the detail
//! fetch, the news handed to that agent's own session, the tools it answers
//! through, and the cleanup once the request ended and its task is over. A
//! request of mine gets no session of its own.
use std::path::PathBuf;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{Value, json};

use crate::common::forge::{StubForgeCli, answer, check_states, stub_forge_cli};
use crate::common::{
    Harness, QUIET, TIMEOUT, as_session, eventually, harness, next_event, post_json, sh, with_forge,
};
use ariadne_api::SESSION_HEADER;
use ariadne_api::tasks::TaskDto;
use ariadne_core::{SessionStatus, TaskStatus};
use ariadne_daemon::forge::poll::Mode;
use ariadne_daemon::scheduler::SchedEvent;
use ariadne_store::defaults::PULL_REQUEST_WORKFLOW;
use ariadne_store::{AgentSession, SessionFilter, Task};

/// Where `pr create` says the task's request is.
const URL: &str = "https://github.com/acme/widgets/pull/1";

/// The host `script_of`'s own check-states answer is keyed on: every
/// fixture here names `acme/widgets` on `github.com`.
const HOST: &str = "github.com";

fn pull(state: &str, sha: &str) -> Value {
    json!({"number": 1, "url": URL,
        "title": "Fix widgets", "author": {"login": "me"}, "state": state, "isDraft": false,
        "headRefName": "fix", "headRefOid": sha,
        "headRepository": {"url": "https://github.com/acme/widgets"},
        "baseRefName": "main", "reviewDecision": "",
        "createdAt": "2026-10-01T00:00:00Z"})
}

fn review_comment(id: i64, login: &str, reply_to: Option<i64>, at: &str) -> Value {
    json!({"id": id, "in_reply_to_id": reply_to, "user": {"login": login, "type": "User"},
        "body": format!("comment {id} by {login}"), "path": "widget", "line": 1,
        "created_at": at})
}

/// What the stub forge answers: the request in `state` on its own read, its
/// review comments in one thread, and its check runs. No list holds it: a
/// fetch's open list is empty, and reads the request on its own.
/// `script_of`'s own check-states answer repeats `check_runs`' own
/// conclusions (`combined_state`): the request read and its detailed check
/// read agree on a check's outcome, so a fetch comparing the two
/// (`Live::unmoved`) sees the move a real forge read would too.
fn script(state: &str, comments: &[Value], check_runs: Value) -> Value {
    let threads = json!({"data": {"repository": {"pullRequest": {"reviewThreads": {"nodes": [
        {"id": "T1", "isResolved": false, "comments": {"nodes": [{"databaseId": 101}]}}
    ]}}}}});
    script_of(
        pull(state, "abc"),
        &[],
        comments,
        &json!({"check_runs": check_runs}).to_string(),
        "[]",
        &threads.to_string(),
    )
}

/// `check_runs`' own REST conclusions, lowercase, read as GitHub's own
/// combined enum, uppercase (026): what `script_of`'s own check-states
/// answer must repeat for `checks` to move the way a detail read of the
/// same check runs also does. `None` of an empty list: no rollup at all.
fn combined_state(check_runs: &Value) -> Option<&'static str> {
    let runs = check_runs.as_array().cloned().unwrap_or_default();
    if runs.is_empty() {
        return None;
    }
    Some(
        if runs.iter().any(|r| {
            matches!(
                r["conclusion"].as_str(),
                Some("failure" | "timed_out" | "cancelled" | "action_required" | "startup_failure")
            )
        }) {
            "FAILURE"
        } else if runs.iter().any(|r| r["conclusion"].as_str().is_none()) {
            "PENDING"
        } else {
            "SUCCESS"
        },
    )
}

/// `script`, with the pull's own `updatedAt` moved to `at`: what a real
/// forge push, comment or edit would also move, so a fetch whose only
/// other change is to `comments` is read as having moved
/// (`Live::unmoved`), not as repeating the held read.
fn moved_at(mut script: Value, at: &str) -> Value {
    for entry in script.as_array_mut().unwrap() {
        if entry["args"] == json!(["pr", "view"]) {
            let mut pull: Value = serde_json::from_str(entry["stdout"].as_str().unwrap()).unwrap();
            pull["updatedAt"] = json!(at);
            entry["stdout"] = json!(pull.to_string());
        }
    }
    script
}

/// The same, for a request read as `read` and lists that hold `listed`, with
/// the check runs and the review threads as `gh api --paginate` writes them:
/// one JSON value per page, back to back. The combined check state
/// (`check_states`) is `combined_state(check_runs)`, read apart from both.
fn script_of(
    read: Value,
    listed: &[Value],
    comments: &[Value],
    check_runs: &str,
    reviews: &str,
    threads: &str,
) -> Value {
    let number = read["number"].as_i64().unwrap();
    let runs: Value = serde_json::from_str(check_runs)
        .ok()
        .and_then(|v: Value| v.get("check_runs").cloned())
        .unwrap_or_else(|| json!([]));
    let pairs: Vec<(i64, &str)> = match combined_state(&runs) {
        Some(state) => vec![(number, state)],
        None => vec![],
    };
    let read = read.to_string();
    let answers = vec![
        answer(&["auth", "status"], 0, ""),
        answer(&["api", "user"], 0, "me"),
        answer(&["pr", "create"], 0, URL),
        answer(&["pr", "list"], 0, &json!(listed).to_string()),
        answer(&["pr", "view"], 0, &read),
        check_states(HOST, &pairs),
        answer(
            &["pr", "comment"],
            0,
            "https://github.com/acme/widgets/pull/1#issuecomment-301",
        ),
        answer(
            &["api", "repos/acme/widgets/pulls/1/comments"],
            0,
            &json!(comments).to_string(),
        ),
        // The review's one summary comment: posted once, then edited.
        answer(
            &[
                "api",
                "repos/acme/widgets/issues/1/comments",
                "--hostname",
                "github.com",
                "--method",
                "POST",
            ],
            0,
            r#"{"id": 301, "created_at": "2026-10-02T00:00:00Z"}"#,
        ),
        answer(
            &["api", "repos/acme/widgets/issues/comments/301"],
            0,
            r#"{"id": 301, "created_at": "2026-10-02T00:00:00Z"}"#,
        ),
        answer(&["api", "repos/acme/widgets/issues/1/comments"], 0, "[]"),
        answer(&["api", "repos/acme/widgets/pulls/1/reviews"], 0, reviews),
        answer(
            &["api", "repos/acme/widgets/pulls/1/comments/101/replies"],
            0,
            r#"{"id": 201}"#,
        ),
        answer(&["api", "graphql"], 0, threads),
        answer(
            &["api", "repos/acme/widgets/commits/abc/check-runs"],
            0,
            check_runs,
        ),
        answer(
            &["api", "repos/acme/widgets/compare/main...abc"],
            0,
            r#"{"behind_by": 0}"#,
        ),
    ];
    json!(answers)
}

fn quiet_script() -> Value {
    script("OPEN", &[], json!([]))
}

/// A task of a `develop-review-pr` goal, walked to its `pr` column, whose
/// agent opened its request: the request the ledger keeps for it, that
/// agent's own session, and the checkout with its bare remote.
struct Kept {
    h: Harness,
    stub: StubForgeCli,
    task: Task,
    agent: AgentSession,
    /// The ledger row of the task's request.
    id: String,
    repo: String,
    path: PathBuf,
    bare: PathBuf,
}

/// A task of a `develop-review-pr` goal, walked to its `pr` column, its
/// branch pushed: the request is not open yet, so its agent may open one
/// with any body a test wants.
struct ReadyToOpen {
    h: Harness,
    stub: StubForgeCli,
    task: Task,
    agent: AgentSession,
    repo: String,
    path: PathBuf,
    bare: PathBuf,
}

async fn ready_to_open(script: Value) -> ReadyToOpen {
    let stub = stub_forge_cli(script);
    let h = harness()
        .scheduler()
        .discover_agents()
        .forge_cli(&stub)
        .await;
    let path = h.git_repo("repo");
    let cast = h.active_cast_running(Some(PULL_REQUEST_WORKFLOW)).await;
    with_forge(&h, &cast.repo).await;
    let bare = h.at("remote.git");
    // The base is on the remote, as a forge's is: a merge is fetched from it.
    sh(&path, "git push -q origin main");
    let task = cast.task.clone();
    // The develop column commits the change, the review column passes it on,
    // and the `pr` column's agent pushes the branch and opens the request.
    let develop = h.step_session(&task, "develop").await;
    let worktree = PathBuf::from(develop.worktree_path.as_deref().unwrap());
    sh(
        &worktree,
        "echo change > widget && git add widget && git -c user.name=Test -c user.email=test@test commit -qm 'feat: add widgets'",
    );
    h.complete_step(&task, &develop, "The change is committed.")
        .await;
    let review = h.step_session(&task, "review").await;
    h.complete_step(&task, &review, "The change passes review.")
        .await;
    let agent = h.step_session(&task, "pr").await;
    sh(&worktree, "git push -q origin HEAD");
    let repo = cast.repo.id.clone();
    ReadyToOpen {
        h,
        stub,
        task,
        agent,
        repo,
        path,
        bare,
    }
}

async fn kept_request(script: Value) -> Kept {
    let ReadyToOpen {
        h,
        stub,
        task,
        agent,
        repo,
        path,
        bare,
    } = ready_to_open(script).await;
    let opened: TaskDto = h
        .json(
            as_session(
                &format!("/v1/tasks/{}/pull-request", task.id),
                &agent.id,
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
    h.state.forge_poll.set_mode(&repo, Mode::WakeOnly);
    eventually(
        TIMEOUT,
        "the request agent's first turn to end",
        async || h.session_status(&agent).await == SessionStatus::Idle,
    )
    .await;
    eventually(TIMEOUT, "the first fetch", async || stub.fetches() >= 1).await;
    Kept {
        h,
        stub,
        task,
        agent,
        id,
        repo,
        path,
        bare,
    }
}

/// Fetch the repository once more and let the scheduler act on it.
async fn fetch_again(h: &Harness, stub: &StubForgeCli, repo: &str) {
    let before = stub.fetches();
    h.state.forge_poll.wake(repo);
    eventually(TIMEOUT, "the fetch", async || stub.fetches() > before).await;
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

/// A new comment by another login is stored, counted, and handed to the
/// task's `pr` agent in one prompt that names it; the same details again
/// hand nothing; a check that turned to failure is handed by name. The
/// request has no session of its own: its row names the agent's.
#[tokio::test]
async fn the_news_of_its_request_reaches_the_pr_agent_once() {
    let Kept {
        h,
        stub,
        agent,
        id,
        repo,
        ..
    } = kept_request(quiet_script()).await;
    let briefed = h.prompts_to(&agent).len();

    let comments = [review_comment(101, "alice", None, "2026-10-02T00:00:00Z")];
    stub.reprogram(moved_at(
        script("OPEN", &comments, json!([])),
        "2026-10-02T00:00:00Z",
    ));
    fetch_again(&h, &stub, &repo).await;
    let mut stored: Vec<Value> = Vec::new();
    eventually(TIMEOUT, "the comment to be stored", async || {
        stored = h.get(&format!("/v1/pull-requests/{id}/comments")).await;
        stored.len() == 1
    })
    .await;
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0]["thread_id"], "T1");
    assert_eq!(stored[0]["author_login"], "alice");
    let comment_id = stored[0]["id"].as_str().unwrap().to_string();
    let dto: Value = h.get(&format!("/v1/pull-requests/{id}")).await;
    assert_eq!(dto["unanswered_comments"], 1);
    assert_eq!(
        dto["session_id"], agent.id,
        "the request's session is its agent's"
    );
    eventually(TIMEOUT, "the news to reach the agent", async || {
        h.prompts_to(&agent).len() == briefed + 1
    })
    .await;
    let news = h.prompts_to(&agent).pop().unwrap();
    assert!(news.contains(&comment_id), "{news}");
    assert!(news.contains("alice"), "{news}");

    // Nothing moved since the held read, so this fetch skips the detail
    // read outright (`Live::unmoved`); wait for the fetch itself to finish,
    // not for a detail read that will not happen.
    let before = stub.fetches();
    h.state.forge_poll.wake(&repo);
    eventually(TIMEOUT, "the unmoved fetch to finish", async || {
        stub.fetches() > before
    })
    .await;
    h.flush_scheduler().await;
    // Prove that the completed fetch hands no later duplicate news.
    tokio::time::sleep(QUIET).await;
    assert_eq!(
        h.prompts_to(&agent).len(),
        briefed + 1,
        "the same details are no news"
    );

    let failed = json!([{"name": "lint", "html_url": "https://ci.example/1",
        "conclusion": "failure", "status": "completed"}]);
    stub.reprogram(script("OPEN", &comments, failed));
    fetch_again(&h, &stub, &repo).await;
    eventually(TIMEOUT, "the failed check to reach the agent", async || {
        h.prompts_to(&agent).len() == briefed + 2
    })
    .await;
    let news = h.prompts_to(&agent).pop().unwrap();
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

/// A pending review of the viewer's own — the shape pull request #110
/// showed waking the reviewer and keeper agents — gives no live-view
/// comment and no news: GitHub's `pulls/{number}/comments` answers the
/// pending review's own inline comment and its reply the same as a
/// submitted review's, undocumented but real, so the detail read filters
/// them by their `pull_request_review_id` rather than trusting the forge
/// to withhold them. The `pr` agent that owns this request hears nothing
/// of it.
#[tokio::test]
async fn a_pending_reviews_comments_give_no_live_view_comment_and_no_news() {
    let Kept {
        h,
        stub,
        agent,
        id,
        repo,
        ..
    } = kept_request(quiet_script()).await;
    let briefed = h.prompts_to(&agent).len();

    let pending_comments = [
        json!({"id": 201, "in_reply_to_id": null, "user": {"login": "me", "type": "User"},
            "body": "a pending finding", "path": "widget", "line": 1,
            "created_at": "2026-10-02T00:00:00Z", "pull_request_review_id": 9}),
        json!({"id": 202, "in_reply_to_id": 201, "user": {"login": "me", "type": "User"},
            "body": "a pending reply", "path": "widget", "line": 1,
            "created_at": "2026-10-02T00:01:00Z", "pull_request_review_id": 9}),
    ];
    let mut script = moved_at(
        script("OPEN", &pending_comments, json!([])),
        "2026-10-02T00:00:00Z",
    );
    let reviews = json!([{"id": 9, "user": {"login": "me"}, "body": "", "submitted_at": null}]);
    for entry in script.as_array_mut().unwrap() {
        if entry["args"] == json!(["api", "repos/acme/widgets/pulls/1/reviews"]) {
            entry["stdout"] = json!(reviews.to_string());
        }
    }
    stub.reprogram(script);
    fetch_again(&h, &stub, &repo).await;

    eventually(
        TIMEOUT,
        "the detail fetch to read the pending review",
        async || {
            h.launcher.live.get(&id).is_some_and(|live| {
                live.evidence_ok && live.pull.updated_at == "2026-10-02T00:00:00Z"
            })
        },
    )
    .await;
    let comments: Vec<Value> = h.get(&format!("/v1/pull-requests/{id}/comments")).await;
    assert_eq!(
        comments,
        Vec::<Value>::new(),
        "the pending review's own comment and its reply give no live-view comment"
    );
    // Prove the quiet, not merely that nothing has arrived yet.
    tokio::time::sleep(QUIET).await;
    assert_eq!(
        h.prompts_to(&agent).len(),
        briefed,
        "a pending review wakes no agent"
    );
}

/// The `pr` agent replies through the forge's reply command, stored as my
/// comment in the thread, which answers it; the agent resolves no thread.
/// Its `ready: true` is stored but raises nothing of its own on the
/// session (029): the `pull_request` attention producer reads the forge's
/// own evidence against the claim instead. Any other session is refused,
/// and so is a call that comes from no session.
///
/// The repository does not supervise its posts here, so the reply posts
/// directly, as it always has (026 rule 24 covers the supervised case
/// separately, below).
#[tokio::test]
async fn the_pr_agent_replies_and_reports_and_no_other_session_may() {
    let comments = [review_comment(101, "alice", None, "2026-10-02T00:00:00Z")];
    let Kept {
        h,
        stub,
        agent,
        id,
        repo,
        ..
    } = kept_request(script("OPEN", &comments, json!([]))).await;
    let repository = h.store.get_repository(&repo).await.unwrap();
    h.set_supervise_posts(&repository, false).await;
    fetch_again(&h, &stub, &repo).await;
    let mut stored: Vec<Value> = Vec::new();
    eventually(TIMEOUT, "the comment to be told", async || {
        stored = h.get(&format!("/v1/pull-requests/{id}/comments")).await;
        stored.len() == 1 && stored[0]["told_at"].is_string()
    })
    .await;
    let comment_id = stored[0]["id"].as_str().unwrap().to_string();
    eventually(TIMEOUT, "the agent to read the comment", async || {
        h.prompts_to(&agent).iter().any(|p| p.contains(&comment_id))
    })
    .await;

    let reply: Value = h
        .json(
            as_session(
                &format!("/v1/pull-requests/{id}/comments/{comment_id}/reply"),
                &agent.id,
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
    // The forge holds the reply now, and the next read finds the thread
    // answered.
    let answered = [
        review_comment(101, "alice", None, "2026-10-02T00:00:00Z"),
        review_comment(201, "me", Some(101), "2026-10-03T00:00:00Z"),
    ];
    stub.reprogram(script("OPEN", &answered, json!([])));
    fetch_again(&h, &stub, &repo).await;
    let dto: Value = h.get(&format!("/v1/pull-requests/{id}")).await;
    assert_eq!(dto["unanswered_comments"], 0);
    h.error(
        as_session(
            &format!("/v1/pull-requests/{id}/comments/{comment_id}/resolve"),
            &agent.id,
            json!({}),
        ),
        StatusCode::FORBIDDEN,
    )
    .await;
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
            &agent.id,
            json!({"ready": ready, "head_sha": "a".repeat(40)}),
        )
    };
    let dto: Value = h.json(report(true), StatusCode::OK).await;
    assert_eq!(dto["ready"], true);
    assert_eq!(h.attention(&agent).await, None);
    let _: Value = h.json(report(false), StatusCode::OK).await;
    assert_eq!(h.attention(&agent).await, None);

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

    // An Ariadne self-review of the same request (029) also answers for
    // it — `own_session` lets it call `/report` at all — but its own
    // `ready` claim must never pass as the babysitting `pr` agent's
    // (031): only the task's own column agent reports this row ready.
    h.store
        .set_pull_request_review_asked(
            &id,
            Some((
                &ariadne_store::AgentPin {
                    model: "stub:model".into(),
                    effort: None,
                },
                &[],
            )),
        )
        .await
        .unwrap();
    let reviewer = h
        .store
        .create_session(ariadne_store::NewSession {
            goal_id: None,
            task_id: None,
            seat: Some(ariadne_core::Seat::Reviewer),
            task_agent_id: None,
            model: "stub:model".into(),
            effort: None,
            worktree_path: None,
            pull_request_id: Some(id.clone()),
        })
        .await
        .unwrap();
    h.error(
        as_session(
            &format!("/v1/pull-requests/{id}/report"),
            &reviewer.id,
            json!({"ready": true}),
        ),
        StatusCode::FORBIDDEN,
    )
    .await;
    assert!(
        !h.store.get_pull_request(&id).await.unwrap().ready,
        "the reviewer's claim never lands"
    );
    // Its own `reviewed_sha` still goes through: that is legitimately
    // its own report to make.
    let dto: Value = h
        .json(
            as_session(
                &format!("/v1/pull-requests/{id}/report"),
                &reviewer.id,
                json!({"reviewed_sha": "0".repeat(40)}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(dto["id"], id);
}

/// `open_pull_request`'s body reaches the forge without a trailer an
/// agent's skill appends: the daemon strips it before the request ever
/// opens.
#[tokio::test]
async fn open_pull_request_strips_a_trailing_trailer_from_the_body() {
    let ReadyToOpen {
        h,
        stub,
        task,
        agent,
        ..
    } = ready_to_open(quiet_script()).await;
    let body = "Fixes widgets.\n\n\
                🤖 Generated with [Claude Code](https://claude.com/claude-code)\n\n\
                Co-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>";
    let opened: TaskDto = h
        .json(
            as_session(
                &format!("/v1/tasks/{}/pull-request", task.id),
                &agent.id,
                json!({"title": "Fix widgets", "body": body}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(opened.pr_url.as_deref(), Some(URL));
    let call = stub
        .invocations()
        .into_iter()
        .find(|i| {
            i.args
                .starts_with(&["pr".to_string(), "create".to_string()])
        })
        .expect("the open call");
    let sent = call
        .args
        .iter()
        .position(|a| a == "--body")
        .map(|at| call.args[at + 1].clone())
        .expect("a --body argument");
    assert_eq!(sent, "Fixes widgets.");
}

/// A repository that supervises its posts (002) opens every request as a
/// draft, whatever the agent's own `draft` asked (026 rule 23).
#[tokio::test]
async fn open_pull_request_forces_a_draft_when_the_repository_supervises_posts() {
    let ReadyToOpen {
        h,
        stub,
        task,
        agent,
        ..
    } = ready_to_open(quiet_script()).await;
    let opened: TaskDto = h
        .json(
            as_session(
                &format!("/v1/tasks/{}/pull-request", task.id),
                &agent.id,
                json!({"title": "Fix widgets", "body": "Fixes widgets.", "draft": false}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(opened.pr_url.as_deref(), Some(URL));
    let call = stub
        .invocations()
        .into_iter()
        .find(|i| {
            i.args
                .starts_with(&["pr".to_string(), "create".to_string()])
        })
        .expect("the open call");
    assert!(
        call.args.iter().any(|a| a == "--draft"),
        "the repository supervises its posts, so the request opens as a draft \
         whatever the agent asked: {:?}",
        call.args
    );
}

/// A repository that does not supervise its posts leaves the agent's own
/// `draft` value through unchanged (026 rule 23).
#[tokio::test]
async fn open_pull_request_leaves_the_agents_draft_value_unchanged_when_supervision_is_off() {
    let ReadyToOpen {
        h,
        stub,
        task,
        agent,
        repo,
        ..
    } = ready_to_open(quiet_script()).await;
    let repository = h.store.get_repository(&repo).await.unwrap();
    h.set_supervise_posts(&repository, false).await;
    let opened: TaskDto = h
        .json(
            as_session(
                &format!("/v1/tasks/{}/pull-request", task.id),
                &agent.id,
                json!({"title": "Fix widgets", "body": "Fixes widgets.", "draft": false}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(opened.pr_url.as_deref(), Some(URL));
    let call = stub
        .invocations()
        .into_iter()
        .find(|i| {
            i.args
                .starts_with(&["pr".to_string(), "create".to_string()])
        })
        .expect("the open call");
    assert!(
        !call.args.iter().any(|a| a == "--draft"),
        "supervision is off, so the agent's own draft: false goes through: {:?}",
        call.args
    );
}

/// A review body posted on the conversation, with no path and no line, is
/// the other side's: the `pr` agent's reply to it through `reply_comment`
/// goes through the forge's `gh pr comment`, once. A second reply to the
/// same body, and a reply to a body of the agent's own side, are refused,
/// and no gh call follows either refusal.
///
/// The repository does not supervise its posts here, so the reply posts
/// directly (026 rule 24 covers the supervised case separately, below).
#[tokio::test]
async fn the_pr_agent_replies_once_to_a_conversation_comment_of_the_other_side() {
    let threads =
        json!({"data": {"repository": {"pullRequest": {"reviewThreads": {"nodes": []}}}}});
    let reviews = json!([
        {"id": 502, "user": {"login": "me"}, "body": "Thanks for the look.",
         "submitted_at": "2026-10-02T00:00:00Z"},
        {"id": 501, "user": {"login": "alice"}, "body": "Looks fine overall.",
         "submitted_at": "2026-10-02T00:01:00Z"},
    ]);
    let mut script = script_of(
        pull("OPEN", "abc"),
        &[],
        &[],
        &json!({"check_runs": []}).to_string(),
        &reviews.to_string(),
        &threads.to_string(),
    );
    let Kept {
        h,
        stub,
        agent,
        id,
        repo,
        ..
    } = kept_request(script.clone()).await;
    let repository = h.store.get_repository(&repo).await.unwrap();
    h.set_supervise_posts(&repository, false).await;
    let mut stored: Vec<Value> = Vec::new();
    eventually(TIMEOUT, "both review bodies to be stored", async || {
        stored = h.get(&format!("/v1/pull-requests/{id}/comments")).await;
        stored.iter().filter(|c| c["kind"] == "review").count() == 2
    })
    .await;
    let other_side = stored
        .iter()
        .find(|c| c["author_login"] == "alice")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let own_side = stored.iter().find(|c| c["author_login"] == "me").unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    let replies = |stub: &StubForgeCli| {
        stub.invocations()
            .into_iter()
            .filter(|i| {
                i.args.first().map(String::as_str) == Some("pr")
                    && i.args.get(1).map(String::as_str) == Some("comment")
            })
            .count()
    };
    let before = replies(&stub);
    let reply: Value = h
        .json(
            as_session(
                &format!("/v1/pull-requests/{id}/comments/{other_side}/reply"),
                &agent.id,
                json!({"body": "Thanks!"}),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert_eq!(reply["author_login"], "me");
    assert_eq!(reply["thread_id"], "conversation");
    assert_eq!(replies(&stub), before + 1, "one gh call replies");

    // The forge holds the reply now, too: a background poll that reads
    // the stub again, between this reply and the next call, must find the
    // same answer `add_comments` already gave the live cache, not fall
    // back to a read that never heard of it.
    let posted = json!([{"id": 301, "user": {"login": "me", "type": "User"},
        "body": "Thanks!", "created_at": "2026-10-03T00:00:00Z"}]);
    for entry in script.as_array_mut().unwrap() {
        if entry["args"] == json!(["api", "repos/acme/widgets/issues/1/comments"]) {
            entry["stdout"] = json!(posted.to_string());
        }
    }
    stub.reprogram(script.clone());

    let before = replies(&stub);
    h.error(
        as_session(
            &format!("/v1/pull-requests/{id}/comments/{other_side}/reply"),
            &agent.id,
            json!({"body": "Again."}),
        ),
        StatusCode::BAD_REQUEST,
    )
    .await;
    assert_eq!(
        replies(&stub),
        before,
        "a second reply to the same body posts no further reply"
    );

    h.error(
        as_session(
            &format!("/v1/pull-requests/{id}/comments/{own_side}/reply"),
            &agent.id,
            json!({"body": "Thanks!"}),
        ),
        StatusCode::BAD_REQUEST,
    )
    .await;
    assert_eq!(
        replies(&stub),
        before,
        "a reply to the agent's own side's body posts no further reply"
    );
}

/// Where the repository supervises its posts (002 rule 13), the keeping
/// agent's reply to a top-level conversation comment holds nothing on the
/// forge directly: no `gh pr comment` call is made. Instead it quotes the
/// comment it answers, with a link to it, and appends that to the
/// viewer's own pending review, answering `pending: true` and the
/// review's own id (026 rule 24). The comment counts answered from then
/// on, so a second reply to it is refused the same way an already
/// answered comment always is.
#[tokio::test]
async fn the_pr_agent_holds_its_reply_pending_when_the_repository_supervises_posts() {
    let threads =
        json!({"data": {"repository": {"pullRequest": {"reviewThreads": {"nodes": []}}}}});
    let reviews = json!([
        {"id": 501, "user": {"login": "alice"}, "body": "Why not here?",
         "submitted_at": "2026-10-02T00:01:00Z"},
    ]);
    let mut script = script_of(
        pull("OPEN", "abc"),
        &[],
        &[],
        &json!({"check_runs": []}).to_string(),
        &reviews.to_string(),
        &threads.to_string(),
    );
    // Ahead of the generic reads `script_of` already answers: finding no
    // pending review of "me" among `reviews` above, the reply creates one
    // with no body of its own, then appends to it through GraphQL. Each
    // gets its own entry here, matched before the generic one it would
    // otherwise fall through to.
    script.as_array_mut().unwrap().splice(
        0..0,
        [
            answer(
                &[
                    "api",
                    "repos/acme/widgets/pulls/1/reviews",
                    "--hostname",
                    "github.com",
                    "--method",
                    "POST",
                ],
                0,
                r#"{"id": 9, "node_id": "PR_kwDOx", "state": "PENDING", "body": "",
                    "user": {"login": "me"}}"#,
            ),
            answer(
                &[
                    "api",
                    "graphql",
                    "--hostname",
                    "github.com",
                    "-f",
                    "query=mutation($id: ID!, $body: String!) { \
                     updatePullRequestReview(input: {pullRequestReviewId: $id, body: $body}) { \
                     pullRequestReview { id } } }",
                ],
                0,
                r#"{"data": {"updatePullRequestReview": {"pullRequestReview": {"id": "PR_kwDOx"}}}}"#,
            ),
        ],
    );
    let Kept {
        h,
        stub,
        agent,
        id,
        repo,
        ..
    } = kept_request(script).await;
    let mut stored: Vec<Value> = Vec::new();
    eventually(TIMEOUT, "alice's comment to be stored", async || {
        stored = h.get(&format!("/v1/pull-requests/{id}/comments")).await;
        stored.iter().any(|c| c["author_login"] == "alice")
    })
    .await;
    let other_side = stored
        .iter()
        .find(|c| c["author_login"] == "alice")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    let before = stub.invocations().len();
    // The mark lives in the store alone, on no field a forge fetch ever
    // reads back, so the route announces the change itself: a watcher
    // waiting on `pull_requests_changed` learns of it without a poll.
    let mut events = h.bus.subscribe();
    let moved = |event: &ariadne_daemon::bus::BusEvent| matches!(&event.event, ariadne_api::stream::DomainEvent::PullRequestsChanged(p) if p.repository_id == repo);
    let reply: Value = h
        .json(
            as_session(
                &format!("/v1/pull-requests/{id}/comments/{other_side}/reply"),
                &agent.id,
                json!({"body": "Fixed it."}),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert_eq!(reply["pending"], true);
    assert_eq!(reply["id"], "rv-9");
    next_event(&mut events, moved).await;
    assert!(
        !stub.invocations()[before..]
            .iter()
            .any(|i| i.args.first().map(String::as_str) == Some("pr")
                && i.args.get(1).map(String::as_str) == Some("comment")),
        "no public comment is posted while the repository supervises its posts"
    );

    // The comment counts answered with no reply visible on the forge.
    let dto: Value = h.get(&format!("/v1/pull-requests/{id}")).await;
    assert_eq!(dto["unanswered_comments"], 0);

    // A second reply is refused, the same way a comment the login's own
    // side already answered always is: the pending mark alone settles it.
    h.error(
        as_session(
            &format!("/v1/pull-requests/{id}/comments/{other_side}/reply"),
            &agent.id,
            json!({"body": "Again."}),
        ),
        StatusCode::BAD_REQUEST,
    )
    .await;
}

/// A reply that finds the pending review already there — no `/reviews`
/// post of this task's own ever ran to hold a comment of it — still has
/// `list_comments` answer its body right after: the echo this fix added
/// writes it through `add_comments`, which holds a comment whether or
/// not one was already there, never through `edit_comment`, which only
/// ever changes one already held and answers nothing for one it does not
/// find (026 rule 25).
#[tokio::test]
async fn a_reply_holds_the_pending_reviews_body_even_with_no_review_post_of_its_own() {
    let threads =
        json!({"data": {"repository": {"pullRequest": {"reviewThreads": {"nodes": []}}}}});
    let reviews = json!([
        {"id": 501, "user": {"login": "alice"}, "body": "Why not here?",
         "submitted_at": "2026-10-02T00:01:00Z"},
        {"id": 9, "node_id": "PR_kwDOx", "state": "PENDING", "user": {"login": "me"},
         "body": "<!-- ariadne:pending-review-verdict -->", "submitted_at": null},
    ]);
    let mut script = script_of(
        pull("OPEN", "abc"),
        &[],
        &[],
        &json!({"check_runs": []}).to_string(),
        &reviews.to_string(),
        &threads.to_string(),
    );
    script.as_array_mut().unwrap().splice(
        0..0,
        [answer(
            &[
                "api",
                "graphql",
                "--hostname",
                "github.com",
                "-f",
                "query=mutation($id: ID!, $body: String!) { \
                 updatePullRequestReview(input: {pullRequestReviewId: $id, body: $body}) { \
                 pullRequestReview { id } } }",
            ],
            0,
            r#"{"data": {"updatePullRequestReview": {"pullRequestReview": {"id": "PR_kwDOx"}}}}"#,
        )],
    );
    // The stub's own tempdir, which its `gh` binary lives under, must
    // outlive this test: dropping it via `..` deletes the binary mid-test.
    let Kept {
        h,
        stub: _stub,
        agent,
        id,
        ..
    } = kept_request(script).await;
    let mut stored: Vec<Value> = Vec::new();
    eventually(TIMEOUT, "alice's comment to be stored", async || {
        stored = h.get(&format!("/v1/pull-requests/{id}/comments")).await;
        stored.iter().any(|c| c["author_login"] == "alice")
    })
    .await;
    let other_side = stored
        .iter()
        .find(|c| c["author_login"] == "alice")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let reply: Value = h
        .json(
            as_session(
                &format!("/v1/pull-requests/{id}/comments/{other_side}/reply"),
                &agent.id,
                json!({"body": "Fixed it."}),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert_eq!(reply["pending"], true);
    assert_eq!(reply["id"], "rv-9");
    let comments: Vec<Value> = h.get(&format!("/v1/pull-requests/{id}/comments")).await;
    let review = comments
        .iter()
        .find(|c| c["id"] == "rv-9")
        .expect("the pending review's own body is held, not merely edited in place");
    assert!(
        review["body"]
            .as_str()
            .unwrap()
            .contains("ariadne:pending-review-verdict"),
        "{review:?}"
    );
}

/// The three pending-edit routes (026 rule 25) answer 409 with no pending review or
/// draft note of the viewer's own to edit, making only the one listing call
/// `quiet_script` already answers empty: there is nothing pending yet.
#[tokio::test]
async fn pending_edit_routes_answer_409_with_no_pending_review() {
    // The stub's own tempdir, which its `gh` binary lives under, must
    // outlive this test: dropping it via `..` deletes the binary mid-test.
    let Kept {
        h,
        stub: _stub,
        agent,
        id,
        ..
    } = kept_request(quiet_script()).await;
    let error = h
        .error(
            as_session(
                &format!("/v1/pull-requests/{id}/comments/edit"),
                &agent.id,
                json!({"forge_id": "rc-501", "body": "Fixed."}),
            ),
            StatusCode::CONFLICT,
        )
        .await;
    assert_eq!(error.error.code, "conflict");
    assert!(error.error.message.contains("rc-501"), "{error:?}");

    h.error(
        as_session(
            &format!("/v1/pull-requests/{id}/comments/delete"),
            &agent.id,
            json!({"forge_id": "rc-501"}),
        ),
        StatusCode::CONFLICT,
    )
    .await;

    h.error(
        as_session(
            &format!("/v1/pull-requests/{id}/pending-review/body"),
            &agent.id,
            json!({"body": "New body."}),
        ),
        StatusCode::CONFLICT,
    )
    .await;
}

/// A `forge_id` of the wrong shape — naming no comment GitHub or GitLab
/// ever holds, whole or pending — is the caller's own mistake, not the
/// forge's: 400, refused before any forge call and with no pending
/// review needed to tell it apart from one that is merely gone (026 rule
/// 25).
#[tokio::test]
async fn pending_edit_routes_answer_400_with_a_malformed_forge_id() {
    let Kept {
        h,
        stub: _stub,
        agent,
        id,
        ..
    } = kept_request(quiet_script()).await;
    let error = h
        .error(
            as_session(
                &format!("/v1/pull-requests/{id}/comments/edit"),
                &agent.id,
                json!({"forge_id": "ic-501", "body": "Fixed."}),
            ),
            StatusCode::BAD_REQUEST,
        )
        .await;
    assert_eq!(error.error.code, "invalid_request");
    assert!(error.error.message.contains("ic-501"), "{error:?}");

    let error = h
        .error(
            as_session(
                &format!("/v1/pull-requests/{id}/comments/delete"),
                &agent.id,
                json!({"forge_id": "ic-501"}),
            ),
            StatusCode::BAD_REQUEST,
        )
        .await;
    assert_eq!(error.error.code, "invalid_request");
}

/// `edit_pending_review_body` (026 rule 25) replaces the viewer's pending review
/// body outright, through the same `updatePullRequestReview` mutation
/// `reply_pending` reuses, answering which forge item changed.
#[tokio::test]
async fn edit_pending_review_body_replaces_the_pending_reviews_body() {
    let threads =
        json!({"data": {"repository": {"pullRequest": {"reviewThreads": {"nodes": []}}}}});
    let mut script = script_of(
        pull("OPEN", "abc"),
        &[],
        &[],
        &json!({"check_runs": []}).to_string(),
        "[]",
        &threads.to_string(),
    );
    script.as_array_mut().unwrap().splice(
        0..0,
        [
            answer(
                &[
                    "api",
                    "repos/acme/widgets/pulls/1/reviews",
                    "--hostname",
                    "github.com",
                ],
                0,
                r#"[{"id": 9, "node_id": "PR_kwDOx", "state": "PENDING", "body": "Old note.",
                    "user": {"login": "me"}}]"#,
            ),
            answer(
                &[
                    "api",
                    "graphql",
                    "--hostname",
                    "github.com",
                    "-f",
                    "query=mutation($id: ID!, $body: String!) { \
                     updatePullRequestReview(input: {pullRequestReviewId: $id, body: $body}) { \
                     pullRequestReview { id } } }",
                ],
                0,
                r#"{"data": {"updatePullRequestReview": {"pullRequestReview": {"id": "PR_kwDOx"}}}}"#,
            ),
        ],
    );
    let Kept {
        h,
        stub: _stub,
        agent,
        id,
        ..
    } = kept_request(script).await;
    let answer: Value = h
        .json(
            as_session(
                &format!("/v1/pull-requests/{id}/pending-review/body"),
                &agent.id,
                json!({"body": "New body."}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(answer["id"], "rv-9");
    assert_eq!(answer["changed"], "the GitHub pending review body");
}

/// Two replies racing for the same conversation body: both pass the once
/// check before either reaches the forge, but the pull request's reply
/// lock serializes them, so the second rereads the first's answer instead
/// of racing it. One `gh pr comment` call posts; the other is refused.
/// The repository does not supervise its posts: this is about the race,
/// not about the supervised path (026 rule 24 covers that separately).
#[tokio::test]
async fn two_replies_racing_for_one_comment_post_it_once() {
    let threads =
        json!({"data": {"repository": {"pullRequest": {"reviewThreads": {"nodes": []}}}}});
    let reviews = json!([
        {"id": 502, "user": {"login": "me"}, "body": "Thanks for the look.",
         "submitted_at": "2026-10-02T00:00:00Z"},
        {"id": 501, "user": {"login": "alice"}, "body": "Looks fine overall.",
         "submitted_at": "2026-10-02T00:01:00Z"},
    ]);
    let gate_dir = tempfile::tempdir().unwrap();
    let gate = gate_dir.path().join("go");
    let mut script = script_of(
        pull("OPEN", "abc"),
        &[],
        &[],
        &json!({"check_runs": []}).to_string(),
        &reviews.to_string(),
        &threads.to_string(),
    );
    for entry in script.as_array_mut().unwrap() {
        if entry["args"] == json!(["pr", "comment"]) {
            entry["wait_for"] = json!(gate.display().to_string());
        }
    }
    let Kept {
        h,
        stub,
        agent,
        id,
        repo,
        ..
    } = kept_request(script.clone()).await;
    let repository = h.store.get_repository(&repo).await.unwrap();
    h.set_supervise_posts(&repository, false).await;
    let mut stored: Vec<Value> = Vec::new();
    eventually(TIMEOUT, "both review bodies to be stored", async || {
        stored = h.get(&format!("/v1/pull-requests/{id}/comments")).await;
        stored.iter().filter(|c| c["kind"] == "review").count() == 2
    })
    .await;
    let other_side = stored
        .iter()
        .find(|c| c["author_login"] == "alice")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    let replies = || {
        stub.invocations()
            .into_iter()
            .filter(|i| {
                i.args.first().map(String::as_str) == Some("pr")
                    && i.args.get(1).map(String::as_str) == Some("comment")
            })
            .count()
    };
    let reply = |body: &str| {
        h.send(as_session(
            &format!("/v1/pull-requests/{id}/comments/{other_side}/reply"),
            &agent.id,
            json!({"body": body}),
        ))
    };
    let release_when_called = async {
        eventually(TIMEOUT, "the first reply to reach the forge", async || {
            replies() >= 1
        })
        .await;
        std::fs::write(&gate, "go").unwrap();
    };
    let ((a, _), (b, _), ()) = tokio::join!(reply("First."), reply("Second."), release_when_called);
    let mut statuses = [a, b];
    statuses.sort();
    assert_eq!(
        statuses,
        [StatusCode::CREATED, StatusCode::BAD_REQUEST],
        "the lock serializes the two, so only one is answered"
    );
    assert_eq!(replies(), 1, "only the winner calls the forge CLI");
}

/// Counts of a detail fetch's own `issues/1/comments` read: the
/// conversation side of a request, the one the reply below answers on.
fn conversation_reads(stub: &StubForgeCli) -> usize {
    stub.invocations()
        .iter()
        .filter(|i| {
            i.args.get(1).map(String::as_str) == Some("repos/acme/widgets/issues/1/comments")
                && !i.args.contains(&"POST".to_string())
        })
        .count()
}

/// A poll's detail read that began before a reply posted, and so reads
/// the conversation with no word of it, does not erase it once that read
/// finishes after the reply: `LivePulls::set_details` carries forward
/// only a local addition newer than the generation the read itself
/// captured, and the reply is exactly that. The once check still refuses
/// a second reply to the same comment once the poll's stale read lands.
/// The repository does not supervise its posts: this is about the race,
/// not about the supervised path (026 rule 24 covers that separately).
#[tokio::test]
async fn a_poll_that_began_before_a_reply_does_not_erase_it() {
    let threads =
        json!({"data": {"repository": {"pullRequest": {"reviewThreads": {"nodes": []}}}}});
    let reviews = json!([
        {"id": 502, "user": {"login": "me"}, "body": "Thanks for the look.",
         "submitted_at": "2026-10-02T00:00:00Z"},
        {"id": 501, "user": {"login": "alice"}, "body": "Looks fine overall.",
         "submitted_at": "2026-10-02T00:01:00Z"},
    ]);
    let gate_dir = tempfile::tempdir().unwrap();
    let gate = gate_dir.path().join("go");
    let mut script = script_of(
        pull("OPEN", "abc"),
        &[],
        &[],
        &json!({"check_runs": []}).to_string(),
        &reviews.to_string(),
        &threads.to_string(),
    );
    let Kept {
        h,
        stub,
        agent,
        id,
        repo,
        ..
    } = kept_request(script.clone()).await;
    let repository = h.store.get_repository(&repo).await.unwrap();
    h.set_supervise_posts(&repository, false).await;
    let mut stored: Vec<Value> = Vec::new();
    eventually(TIMEOUT, "both review bodies to be stored", async || {
        stored = h.get(&format!("/v1/pull-requests/{id}/comments")).await;
        stored.iter().filter(|c| c["kind"] == "review").count() == 2
    })
    .await;
    let other_side = stored
        .iter()
        .find(|c| c["author_login"] == "alice")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Only now does a detail read's own conversation call wait on the
    // gate: the setup fetch above, before this, ran free of it. The pull
    // itself also moves its own `updatedAt`, the way a real push or edit
    // would: nothing else in this read repeats the held one, so this wake's
    // fetch reads the detail read as due (`Live::unmoved`) rather than
    // skipping it.
    for entry in script.as_array_mut().unwrap() {
        if entry["args"] == json!(["api", "repos/acme/widgets/issues/1/comments"]) {
            entry["wait_for"] = json!(gate.display().to_string());
        }
    }
    script = moved_at(script, "2026-10-02T00:00:00Z");
    stub.reprogram(script.clone());

    // The poll's own detail read reaches, and is held on, its
    // conversation read before the reply below posts: its own snapshot
    // of "since" is taken before the reply exists.
    let before = conversation_reads(&stub);
    h.state.forge_poll.wake(&repo);
    eventually(
        TIMEOUT,
        "the poll's conversation read to reach the forge",
        async || conversation_reads(&stub) > before,
    )
    .await;

    let reply: Value = h
        .json(
            as_session(
                &format!("/v1/pull-requests/{id}/comments/{other_side}/reply"),
                &agent.id,
                json!({"body": "Thanks!"}),
            ),
            StatusCode::CREATED,
        )
        .await;

    // Let the poll's held, stale read land now that the reply exists.
    // The cache is read directly, not through the comments route: that
    // route always reads the forge again, which would race this same
    // generation check a second time, on a since this call captures only
    // after the reply, rather than prove what the poll's own read did
    // with it.
    std::fs::write(&gate, "go").unwrap();
    let reply_forge_id = reply["id"].as_str().unwrap().to_string();
    eventually(
        TIMEOUT,
        "the poll's stale read to leave the reply standing",
        async || {
            h.launcher
                .live
                .get(&id)
                .and_then(|live| live.details)
                .is_some_and(|details| {
                    details
                        .comments
                        .iter()
                        .any(|c| c.forge_id == reply_forge_id)
                })
        },
    )
    .await;

    h.error(
        as_session(
            &format!("/v1/pull-requests/{id}/comments/{other_side}/reply"),
            &agent.id,
            json!({"body": "Again."}),
        ),
        StatusCode::BAD_REQUEST,
    )
    .await;
}

/// A comment a read once found, gone from a later read that started no
/// earlier than that first one, is dropped rather than carried forward:
/// the forge no longer holds it, and `LivePulls::set_details` only
/// protects a local addition newer than a read's own starting
/// generation, never a comment the forge itself once reported and now
/// does not.
#[tokio::test]
async fn a_comment_gone_from_a_later_read_is_dropped() {
    let threads =
        json!({"data": {"repository": {"pullRequest": {"reviewThreads": {"nodes": []}}}}});
    let reviews = json!([
        {"id": 502, "user": {"login": "me"}, "body": "Thanks for the look.",
         "submitted_at": "2026-10-02T00:00:00Z"},
        {"id": 501, "user": {"login": "alice"}, "body": "Looks fine overall.",
         "submitted_at": "2026-10-02T00:01:00Z"},
    ]);
    let mut script = script_of(
        pull("OPEN", "abc"),
        &[],
        &[],
        &json!({"check_runs": []}).to_string(),
        &reviews.to_string(),
        &threads.to_string(),
    );
    let Kept {
        h, stub, id, repo, ..
    } = kept_request(script.clone()).await;
    let mut stored: Vec<Value> = Vec::new();
    eventually(TIMEOUT, "both review bodies to be stored", async || {
        stored = h.get(&format!("/v1/pull-requests/{id}/comments")).await;
        stored.iter().filter(|c| c["kind"] == "review").count() == 2
    })
    .await;
    assert!(stored.iter().any(|c| c["author_login"] == "alice"));

    // Alice deleted her review body: the forge no longer answers with it.
    let reviews_without_alice = json!([
        {"id": 502, "user": {"login": "me"}, "body": "Thanks for the look.",
         "submitted_at": "2026-10-02T00:00:00Z"},
    ]);
    for entry in script.as_array_mut().unwrap() {
        if entry["args"] == json!(["api", "repos/acme/widgets/pulls/1/reviews"]) {
            entry["stdout"] = json!(reviews_without_alice.to_string());
        }
    }
    // The deletion itself moves the pull's own `updatedAt`, the way a real
    // forge edit would: otherwise nothing else in this read repeats the
    // held one, and the next fetch would skip the detail read outright
    // (`Live::unmoved`).
    script = moved_at(script, "2026-10-03T00:00:00Z");
    stub.reprogram(script.clone());
    fetch_again(&h, &stub, &repo).await;

    eventually(
        TIMEOUT,
        "alice's deleted review to leave the comments",
        async || {
            let after: Vec<Value> = h.get(&format!("/v1/pull-requests/{id}/comments")).await;
            !after.iter().any(|c| c["author_login"] == "alice")
        },
    )
    .await;
}

/// A whole script, every detail entry keyed on `head`: what the initial
/// open and a later push each need, parameterized on the one thing that
/// actually changes between them.
fn script_for_head(head: &str) -> Value {
    let threads =
        json!({"data": {"repository": {"pullRequest": {"reviewThreads": {"nodes": []}}}}});
    json!([
        answer(&["auth", "status"], 0, ""),
        answer(&["api", "user"], 0, "me"),
        answer(&["pr", "create"], 0, URL),
        answer(&["pr", "list"], 0, "[]"),
        answer(&["pr", "view"], 0, &pull("OPEN", head).to_string()),
        answer(&["api", "repos/acme/widgets/pulls/1/comments"], 0, "[]"),
        answer(&["api", "repos/acme/widgets/issues/1/comments"], 0, "[]"),
        answer(&["api", "repos/acme/widgets/pulls/1/reviews"], 0, "[]"),
        answer(&["api", "graphql"], 0, &threads.to_string()),
        answer(
            &[
                "api",
                &format!("repos/acme/widgets/commits/{head}/check-runs")
            ],
            0,
            r#"{"check_runs": []}"#
        ),
        answer(
            &["api", &format!("repos/acme/widgets/compare/main...{head}")],
            0,
            r#"{"behind_by": 0}"#
        ),
    ])
}

/// `script_for_head`, with the one entry whose args start with `failing`
/// made to fail outright: what an outer list call or an initial, listless
/// read failing looks like to the stub, everything else of the fetch
/// answering exactly as it would have.
fn script_for_head_with(head: &str, failing: &[&str]) -> Value {
    let Value::Array(mut entries) = script_for_head(head) else {
        unreachable!()
    };
    for entry in &mut entries {
        if entry["args"] == json!(failing) {
            entry["exit"] = json!(1);
            entry["stdout"] = json!("boom");
        }
    }
    Value::Array(entries)
}

/// A detail fetch that fails at the very same head the last one succeeded
/// on, through the real forge fetch path — `read_row`'s own `Err` branch —
/// marks the comment evidence `evidence_ok: false`; the next fetch that
/// actually succeeds again, still on that same head, marks it `true` once
/// more. Neither move is visible to a head comparison alone, since the
/// head itself never moves (031).
#[tokio::test]
async fn evidence_ok_tracks_a_real_fetchs_own_success_and_failure_at_the_same_head() {
    let head = "a".repeat(40);
    let Kept {
        h, stub, id, repo, ..
    } = kept_request(script_for_head(&head)).await;
    assert!(h.launcher.live.get(&id).unwrap().evidence_ok);

    // The next fetch fails to read the comment evidence at all — the
    // detail entries themselves are gone — though the request's own read
    // still succeeds, on the exact same head.
    stub.reprogram(json!([
        answer(&["auth", "status"], 0, ""),
        answer(&["api", "user"], 0, "me"),
        answer(&["pr", "create"], 0, URL),
        answer(&["pr", "list"], 0, "[]"),
        answer(&["pr", "view"], 0, &pull("OPEN", &head).to_string()),
    ]));
    h.state.forge_poll.wake(&repo);
    eventually(TIMEOUT, "evidence_ok to drop", async || {
        h.launcher.live.get(&id).is_some_and(|l| !l.evidence_ok)
    })
    .await;
    assert_eq!(
        h.launcher.live.get(&id).map(|l| l.pull.head_sha),
        Some(head.clone()),
        "the head itself never moved"
    );

    // A later fetch succeeds again, on the same head: evidence_ok
    // recovers.
    stub.reprogram(script_for_head(&head));
    fetch_again(&h, &stub, &repo).await;
    eventually(TIMEOUT, "evidence_ok to recover", async || {
        h.launcher.live.get(&id).is_some_and(|l| l.evidence_ok)
    })
    .await;
}

/// A failure of the outer list itself — `fetch`'s own `Err` branch,
/// before any row is ever read — withdraws every row it already holds
/// cached evidence for, through the real forge fetch path, the same way
/// a detail fetch failing does: a row's evidence is no less unconfirmed
/// for the whole round having failed before its own read ever ran. A
/// later fetch that succeeds again restores it.
#[tokio::test]
async fn an_outer_list_failure_withdraws_the_rows_evidence() {
    let head = "a".repeat(40);
    let Kept {
        h, stub, id, repo, ..
    } = kept_request(script_for_head(&head)).await;
    assert!(h.launcher.live.get(&id).unwrap().evidence_ok);

    stub.reprogram(script_for_head_with(&head, &["pr", "list"]));
    h.state.forge_poll.wake(&repo);
    eventually(
        TIMEOUT,
        "evidence_ok to drop on an outer list failure",
        async || h.launcher.live.get(&id).is_some_and(|l| !l.evidence_ok),
    )
    .await;

    stub.reprogram(script_for_head(&head));
    fetch_again(&h, &stub, &repo).await;
    eventually(TIMEOUT, "evidence_ok to recover", async || {
        h.launcher.live.get(&id).is_some_and(|l| l.evidence_ok)
    })
    .await;
}

/// A failure of the initial, listless read itself — `read_row`'s own
/// lookup for a request the list missed, before `set_pull_evidence_failed`
/// ever runs — withdraws its evidence through the real forge fetch path
/// the same way a failed detail fetch does. A later fetch that succeeds
/// again restores it.
#[tokio::test]
async fn a_direct_read_failure_withdraws_the_rows_evidence() {
    let head = "a".repeat(40);
    let Kept {
        h, stub, id, repo, ..
    } = kept_request(script_for_head(&head)).await;
    assert!(h.launcher.live.get(&id).unwrap().evidence_ok);

    stub.reprogram(script_for_head_with(&head, &["pr", "view"]));
    h.state.forge_poll.wake(&repo);
    eventually(
        TIMEOUT,
        "evidence_ok to drop on a direct read failure",
        async || h.launcher.live.get(&id).is_some_and(|l| !l.evidence_ok),
    )
    .await;

    stub.reprogram(script_for_head(&head));
    fetch_again(&h, &stub, &repo).await;
    eventually(TIMEOUT, "evidence_ok to recover", async || {
        h.launcher.live.get(&id).is_some_and(|l| l.evidence_ok)
    })
    .await;
}

/// The request's own read, fully approved, every check green, and the
/// forge reporting the head clear to merge: what a babysat request's
/// readiness item needs beside its own `ready` report.
fn ready_pull(head: &str) -> Value {
    let mut ready = pull("OPEN", head);
    ready["reviewDecision"] = json!("APPROVED");
    ready["mergeStateStatus"] = json!("CLEAN");
    ready
}

/// `script_for_head`, with the request's own read given as `read` rather
/// than built in: every other entry — the comments, the reviews, the
/// threads, the checks and the comparison — stays the one a ready
/// request needs, so only what `read` itself says about the request can
/// differ between two scripts built this way. `checks` reads `SUCCESS`,
/// through its own check-states answer (`check_states`), the same every
/// one of those readiness scripts needs.
fn script_with_read(head: &str, read: &Value) -> Value {
    let threads =
        json!({"data": {"repository": {"pullRequest": {"reviewThreads": {"nodes": []}}}}});
    let number = read["number"].as_i64().unwrap();
    json!([
        answer(&["auth", "status"], 0, ""),
        answer(&["api", "user"], 0, "me"),
        answer(&["pr", "create"], 0, URL),
        answer(&["pr", "list"], 0, "[]"),
        answer(&["pr", "view"], 0, &read.to_string()),
        check_states(HOST, &[(number, "SUCCESS")]),
        answer(&["api", "repos/acme/widgets/pulls/1/comments"], 0, "[]"),
        answer(&["api", "repos/acme/widgets/issues/1/comments"], 0, "[]"),
        answer(&["api", "repos/acme/widgets/pulls/1/reviews"], 0, "[]"),
        answer(&["api", "graphql"], 0, &threads.to_string()),
        answer(
            &[
                "api",
                &format!("repos/acme/widgets/commits/{head}/check-runs")
            ],
            0,
            r#"{"check_runs": []}"#
        ),
        answer(
            &["api", &format!("repos/acme/widgets/compare/main...{head}")],
            0,
            r#"{"behind_by": 0}"#
        ),
    ])
}

/// `script_for_head`, with the request's own read fully approved, every
/// check green, and the forge reporting the head clear to merge: what a
/// babysat request's readiness item needs beside its own `ready` report.
fn script_ready_for_head(head: &str) -> Value {
    script_with_read(head, &ready_pull(head))
}

/// `script_ready_for_head`, with the one entry whose args start with
/// `failing` made to fail outright.
fn script_ready_for_head_with(head: &str, failing: &[&str]) -> Value {
    let Value::Array(mut entries) = script_ready_for_head(head) else {
        unreachable!()
    };
    for entry in &mut entries {
        if entry["args"] == json!(failing) {
            entry["exit"] = json!(1);
            entry["stdout"] = json!("boom");
        }
    }
    Value::Array(entries)
}

/// A direct, route-level read through `GET /v1/pull-requests/{id}/comments`
/// moves the readiness item itself, not only `evidence_ok`: a failed read there
/// withdraws it from a fresh `attention::collect`, and publishes
/// `pull_requests_changed`, the one thing that reaches an existing
/// watcher — a `WakeOnly` poll worker, left untouched throughout, could
/// never repair that disagreement on its own. The recovery brings the
/// item back and publishes the same way.
#[tokio::test]
async fn a_route_level_read_failure_withdraws_the_readiness_item_and_its_recovery_restores_it() {
    use ariadne_api::stream::DomainEvent;
    let head = "a".repeat(40);
    let Kept {
        h,
        stub,
        agent,
        id,
        repo,
        ..
    } = kept_request(script_ready_for_head(&head)).await;
    h.state.forge_poll.set_mode(&repo, Mode::WakeOnly);
    fetch_again(&h, &stub, &repo).await;

    let _: Value = h
        .json(
            as_session(
                &format!("/v1/pull-requests/{id}/report"),
                &agent.id,
                json!({"ready": true, "head_sha": head}),
            ),
            StatusCode::OK,
        )
        .await;
    let items = || async {
        ariadne_daemon::attention::collect(&h.store, &h.launcher)
            .await
            .items
    };
    eventually(TIMEOUT, "the readiness item", async || {
        !items().await.is_empty()
    })
    .await;

    let mut events = h.bus.subscribe();
    let moved = |event: &ariadne_daemon::bus::BusEvent| matches!(&event.event, DomainEvent::PullRequestsChanged(p) if p.repository_id == repo);

    stub.reprogram(script_ready_for_head_with(&head, &["pr", "view"]));
    let error = h
        .error(
            crate::common::get(&format!("/v1/pull-requests/{id}/comments")),
            StatusCode::BAD_GATEWAY,
        )
        .await;
    assert!(!error.error.message.is_empty());
    next_event(&mut events, moved).await;
    assert!(
        items().await.is_empty(),
        "a fresh attention read withholds the item once the route's own read failed"
    );

    stub.reprogram(script_ready_for_head(&head));
    let _: Value = h.get(&format!("/v1/pull-requests/{id}/comments")).await;
    next_event(&mut events, moved).await;
    assert!(!items().await.is_empty(), "the recovery restores it");
}

/// A direct, route-level read that succeeds outright, same as the one
/// before it, but reads the approval dismissed: `evidence_ok` stays
/// `true` on both reads, so comparing only that flag would miss this
/// transition entirely. Comparing the fingerprint of everything the
/// readiness item depends on catches it all the same, withdraws the
/// item, and publishes `pull_requests_changed` — held in `WakeOnly`
/// throughout, so nothing but this direct read could have told anyone.
#[tokio::test]
async fn a_successful_route_level_read_withdraws_the_readiness_item_when_approval_is_dismissed() {
    use ariadne_api::stream::DomainEvent;
    let head = "a".repeat(40);
    let Kept {
        h,
        stub,
        agent,
        id,
        repo,
        ..
    } = kept_request(script_ready_for_head(&head)).await;
    h.state.forge_poll.set_mode(&repo, Mode::WakeOnly);
    fetch_again(&h, &stub, &repo).await;

    let _: Value = h
        .json(
            as_session(
                &format!("/v1/pull-requests/{id}/report"),
                &agent.id,
                json!({"ready": true, "head_sha": head}),
            ),
            StatusCode::OK,
        )
        .await;
    let items = || async {
        ariadne_daemon::attention::collect(&h.store, &h.launcher)
            .await
            .items
    };
    eventually(TIMEOUT, "the readiness item", async || {
        !items().await.is_empty()
    })
    .await;

    let mut events = h.bus.subscribe();
    let moved = |event: &ariadne_daemon::bus::BusEvent| matches!(&event.event, DomainEvent::PullRequestsChanged(p) if p.repository_id == repo);

    let mut dismissed = ready_pull(&head);
    dismissed["reviewDecision"] = json!("");
    stub.reprogram(script_with_read(&head, &dismissed));
    let _: Value = h.get(&format!("/v1/pull-requests/{id}/comments")).await;
    assert!(
        h.launcher.live.get(&id).unwrap().evidence_ok,
        "the read succeeded outright; evidence_ok never moved"
    );
    next_event(&mut events, moved).await;
    assert!(
        items().await.is_empty(),
        "the dismissed approval withdraws the item although evidence_ok stayed true throughout"
    );
}

/// A direct, route-level read that is this request's very first —
/// `LivePulls` holds nothing of it yet, the way a restart or an outer
/// list failure before the poll's own first read would leave it, while
/// the store's own `ready` claim already stands — still publishes once
/// it creates the readiness item out of nothing: `attention::items`
/// skips a row the cache holds no evidence for at all (`live::of_row`),
/// so the very first successful read can turn an absent item into a
/// present one, which an existing watcher has no way to learn of on its
/// own under `WakeOnly`. A repeated, identical read after that publishes
/// nothing more.
#[tokio::test]
async fn a_first_successful_route_level_read_creates_the_readiness_item_and_publishes_it() {
    use ariadne_api::stream::DomainEvent;
    let head = "a".repeat(40);
    let Kept {
        h,
        stub,
        agent,
        id,
        repo,
        ..
    } = kept_request(script_ready_for_head(&head)).await;
    h.state.forge_poll.set_mode(&repo, Mode::WakeOnly);
    fetch_again(&h, &stub, &repo).await;

    let _: Value = h
        .json(
            as_session(
                &format!("/v1/pull-requests/{id}/report"),
                &agent.id,
                json!({"ready": true, "head_sha": head}),
            ),
            StatusCode::OK,
        )
        .await;
    let items = || async {
        ariadne_daemon::attention::collect(&h.store, &h.launcher)
            .await
            .items
    };
    eventually(TIMEOUT, "the readiness item", async || {
        !items().await.is_empty()
    })
    .await;

    // A restart, or an outer list failure before the poll's own first
    // read, leaves the cache holding nothing of this row, though the
    // store's own `ready` claim stands untouched.
    h.launcher.live.remove(&id);
    assert!(
        items().await.is_empty(),
        "a fresh attention read sees no item with nothing cached"
    );

    let mut events = h.bus.subscribe();
    let moved = |event: &ariadne_daemon::bus::BusEvent| matches!(&event.event, DomainEvent::PullRequestsChanged(p) if p.repository_id == repo);

    let _: Value = h.get(&format!("/v1/pull-requests/{id}")).await;
    next_event(&mut events, moved).await;
    assert!(
        !items().await.is_empty(),
        "the first successful read creates the item"
    );

    // The same read again changes nothing: no second publish.
    let _: Value = h.get(&format!("/v1/pull-requests/{id}")).await;
    assert!(
        tokio::time::timeout(QUIET, next_event(&mut events, moved))
            .await
            .is_err(),
        "a repeated, unchanged read publishes nothing more"
    );
}

/// A push lands, through the real forge fetch path, between the agent's
/// own read of the request and its `ready` report: the report must still
/// bind to the head the agent actually read, not to whatever the daemon's
/// own cache has moved on to by the time the call lands (031). Confirming
/// readiness for a revision the agent never actually inspected would let
/// a stale confirmation pass for a push it never saw.
#[tokio::test]
async fn a_ready_report_binds_the_head_the_agent_actually_read_not_a_later_push() {
    let old_head = "a".repeat(40);
    let new_head = "b".repeat(40);
    let Kept {
        h,
        stub,
        agent,
        id,
        repo,
        ..
    } = kept_request(script_for_head(&old_head)).await;
    // The agent's own read: the request is on the old head.
    assert_eq!(
        h.launcher.live.get(&id).map(|l| l.pull.head_sha),
        Some(old_head.clone())
    );

    // A push lands before the agent gets to report: the next fetch reads
    // the request whole, on its new head.
    stub.reprogram(script_for_head(&new_head));
    fetch_again(&h, &stub, &repo).await;
    eventually(TIMEOUT, "the cache to read the pushed head", async || {
        h.launcher.live.get(&id).map(|l| l.pull.head_sha) == Some(new_head.clone())
    })
    .await;

    // The agent reports ready on the head it actually read — the old
    // one, never the pushed one it never saw.
    let dto: Value = h
        .json(
            as_session(
                &format!("/v1/pull-requests/{id}/report"),
                &agent.id,
                json!({"ready": true, "head_sha": old_head}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(dto["ready"], true);
    let row = h.store.get_pull_request(&id).await.unwrap();
    assert_eq!(
        row.ready_head_sha.as_deref(),
        Some(old_head.as_str()),
        "the report binds to the head the agent read, not the cache's current one"
    );
}

/// A merge done on the forge while the `pr` agent is down ends the task on
/// the request's own merge commit — not on a later commit another request put
/// on the base before the pass — and only once the local base branch holds
/// it, so the tasks that wait on this one branch from a base with its change.
#[tokio::test]
async fn a_merge_ends_the_task_on_its_own_merge_commit_once_the_local_base_holds_it() {
    let Kept {
        h,
        stub,
        task,
        agent,
        repo,
        path,
        bare,
        ..
    } = kept_request(quiet_script()).await;
    // A human merges the request on the forge, and another request merges
    // on top of it before Ariadne's next pass.
    let clone = h.at("forge-clone");
    sh(
        &h.at(""),
        &format!("git clone -q {} {}", bare.display(), clone.display()),
    );
    sh(
        &clone,
        &format!(
            "git checkout -q main && git -c user.name=t -c user.email=t@t merge -q --no-ff origin/{} -m merged && git push -q origin HEAD:main",
            task.branch
        ),
    );
    let merge = sh(&clone, "git rev-parse HEAD");
    sh(
        &clone,
        "echo later > later.txt && git add later.txt && git -c user.name=t -c user.email=t@t commit -qm later && git push -q origin HEAD:main",
    );
    let later = sh(&clone, "git rev-parse HEAD");
    assert_ne!(sh(&path, "git rev-parse main"), later);

    let _: Value = h
        .json(
            crate::common::post(&format!("/v1/sessions/{}/kill", agent.id)),
            StatusCode::OK,
        )
        .await;
    let mut merged = pull("MERGED", "abc");
    merged["mergeCommit"] = json!({"oid": merge});
    stub.reprogram(script_of(
        merged,
        &[],
        &[],
        r#"{"check_runs": []}"#,
        "[]",
        r#"{"data": {"repository": {"pullRequest": {"reviewThreads": {"nodes": []}}}}}"#,
    ));
    h.state.forge_poll.wake(&repo);
    eventually(TIMEOUT, "the daemon to finish the task", async || {
        h.flush_scheduler().await;
        h.status(&task.id).await == TaskStatus::Finished
    })
    .await;
    let finished = h.store.get_task(&task.id).await.unwrap();
    assert_eq!(
        finished.merge_commit.as_deref(),
        Some(merge.as_str()),
        "the request's own merge, not the later tip"
    );
    assert_eq!(
        sh(&path, "git rev-parse main"),
        later,
        "the local base is fast-forwarded to the remote's"
    );
}

/// An Ariadne review of a request a task's `pr` agent keeps posts under the
/// user's login, yet its findings are the agent's to answer (029): each is
/// told to the agent once, named as the review's, and waits on it; the
/// agent's reply answers it.
#[tokio::test]
async fn an_ariadne_review_of_a_kept_request_reaches_its_pr_agent() {
    let Kept {
        h,
        stub,
        task,
        agent,
        id,
        repo,
        path,
        ..
    } = kept_request(quiet_script()).await;
    // Not about the supervised path (026 rule 24 covers that separately):
    // the reply below posts directly, as it did before that rule existed.
    let repository = h.store.get_repository(&repo).await.unwrap();
    h.set_supervise_posts(&repository, false).await;
    // The request's head is the task branch, so the review has a commit to
    // be detached at.
    let tip = sh(&path, &format!("git rev-parse {}", task.branch));
    let headed = |script: Value| -> Value {
        serde_json::from_str(
            &script
                .to_string()
                .replace("abc", &tip)
                .replace("\"fix\"", &format!("\"{}\"", task.branch)),
        )
        .unwrap()
    };
    let finding = json!([{"id": 501, "body": "**[P1] Untested**\n\nNo test covers it.",
        "path": "widget", "line": 1, "created_at": "2026-10-02T00:00:00Z"}]);
    let mut script = headed(quiet_script());
    let entries = script.as_array_mut().unwrap();
    for entry in [
        answer(
            &[
                "api",
                "repos/acme/widgets/pulls/1/reviews",
                "--hostname",
                "github.com",
                "--method",
                "POST",
            ],
            0,
            r#"{"id": 77, "submitted_at": "2026-10-02T00:00:00Z"}"#,
        ),
        answer(
            &["api", "repos/acme/widgets/pulls/1/reviews/77/comments"],
            0,
            &finding.to_string(),
        ),
        answer(
            &["api", "repos/acme/widgets/pulls/1/comments/501/replies"],
            0,
            r#"{"id": 601}"#,
        ),
    ] {
        entries.insert(0, entry);
    }
    stub.reprogram(script.clone());
    // The setup fetch can finish after the fixture changes but before this
    // wake records its detail-call baseline. Wait for the state that either
    // fetch must leave instead of missing that completed refresh.
    h.state.forge_poll.wake(&repo);
    eventually(
        TIMEOUT,
        "the detail fetch to read the new head",
        async || {
            h.launcher.live.get(&id).is_some_and(|live| {
                live.pull.head_sha == tip
                    && live
                        .details
                        .as_ref()
                        .is_some_and(|details| details.head_sha == tip)
            })
        },
    )
    .await;
    h.flush_scheduler().await;

    let ask = Request::builder()
        .method("PUT")
        .uri(format!(
            "/v1/repositories/{repo}/pull-requests/1/ariadne-review"
        ))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"asked": true, "model": "stub:review-model"}).to_string(),
        ))
        .unwrap();
    let _: Value = h.json(ask, StatusCode::OK).await;
    let mut review = None;
    eventually(TIMEOUT, "the review session to be idle", async || {
        review = h
            .store
            .list_sessions(SessionFilter {
                pull_request_id: Some(id.clone()),
                ..Default::default()
            })
            .await
            .unwrap()
            .into_iter()
            .find(|s| s.status() == SessionStatus::Idle && s.launched_at.is_some());
        review.is_some()
    })
    .await;
    let review = review.unwrap();
    let told = h.prompts_to(&agent).len();
    let stored: Vec<Value> = h
        .json(
            as_session(
                &format!("/v1/pull-requests/{id}/reviews"),
                &review.id,
                json!({"event": "comment", "body": "One P1.", "comments": [
                    {"path": "widget", "line": 1, "title": "Untested",
                     "body": "No test covers it.", "priority": "P1"}
                ]}),
            ),
            StatusCode::CREATED,
        )
        .await;
    let finding_id = stored
        .iter()
        .find(|c| c["id"] == "rc-501")
        .expect("the finding is stored")["id"]
        .as_str()
        .unwrap()
        .to_string();
    // The forge holds the review finding on its line; no review summary is posted.
    let holds = |script: &mut Value, comments: Value, summary: Value| {
        for entry in script.as_array_mut().unwrap() {
            if entry["args"] == json!(["api", "repos/acme/widgets/pulls/1/comments"]) {
                entry["stdout"] = json!(comments.to_string());
            }
            if entry["args"] == json!(["api", "repos/acme/widgets/issues/1/comments"]) {
                entry["stdout"] = json!(summary.to_string());
            }
        }
    };
    let me = json!({"login": "me", "type": "User"});
    let summary = json!([{"id": 301, "user": me, "body": "One P1.",
        "created_at": "2026-10-02T00:00:00Z"}]);
    let mut posted = finding.clone();
    posted[0]["user"] = me.clone();
    holds(&mut script, posted.clone(), summary.clone());
    // The review's own post moves the pull's own `updatedAt`, the way a
    // real forge comment would: otherwise nothing else in this read
    // repeats the held one, and the next fetch would skip the detail read
    // outright (`Live::unmoved`).
    script = moved_at(script, "2026-10-03T00:00:00Z");
    stub.reprogram(script.clone());
    fetch_again(&h, &stub, &repo).await;
    let dto: Value = h.get(&format!("/v1/pull-requests/{id}")).await;
    assert_eq!(dto["unanswered_comments"], 1, "the finding waits");
    eventually(TIMEOUT, "the finding to reach the agent", async || {
        h.prompts_to(&agent)
            .iter()
            .skip(told)
            .any(|p| p.contains(&finding_id))
    })
    .await;
    let news = h
        .prompts_to(&agent)
        .into_iter()
        .skip(told)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(news.contains("by the Ariadne review"), "{news}");
    assert!(
        !h.prompted(&review).contains(&finding_id),
        "the review is not told its own finding"
    );

    let answer: Value = h
        .json(
            as_session(
                &format!("/v1/pull-requests/{id}/comments/{finding_id}/reply"),
                &agent.id,
                json!({"body": "Added the test."}),
            ),
            StatusCode::CREATED,
        )
        .await;
    let mut answered = posted.as_array().unwrap().clone();
    answered.push(
        json!({"id": 601, "in_reply_to_id": 501, "user": me, "body": "Added the test.",
        "path": "widget", "line": 1, "created_at": "2026-10-03T00:00:00Z"}),
    );
    holds(&mut script, json!(answered), summary);
    stub.reprogram(script);
    // The answer is posted under the user's login, yet it is news to the
    // review that opened the thread.
    let answer_id = answer["id"].as_str().unwrap().to_string();
    eventually(TIMEOUT, "the answer to reach the review", async || {
        h.flush_scheduler().await;
        h.prompted(&review).contains(&answer_id)
    })
    .await;
    let dto: Value = h.get(&format!("/v1/pull-requests/{id}")).await;
    assert_eq!(
        dto["unanswered_comments"], 0,
        "the agent's reply answers the finding"
    );
}

/// Editing or deleting a held pending finding (026 rule 25) updates
/// Ariadne's own view of it at once: GitHub never lists a pending review's
/// own comments among a request's, so a fresh detail read never catches
/// either change up on its own (`live::LivePulls::edit_comment`,
/// `::remove_comment`). A stale cache would otherwise read a deleted
/// finding as still open, which `submit_review`'s own `open_p0` check
/// reads to require `request_changes` all over again.
#[tokio::test]
async fn editing_and_deleting_a_held_pending_finding_updates_list_comments_at_once() {
    let Kept {
        h,
        stub,
        task,
        id,
        repo,
        path,
        ..
    } = kept_request(quiet_script()).await;
    let tip = sh(&path, &format!("git rev-parse {}", task.branch));
    let headed = |script: Value| -> Value {
        serde_json::from_str(
            &script
                .to_string()
                .replace("abc", &tip)
                .replace("\"fix\"", &format!("\"{}\"", task.branch)),
        )
        .unwrap()
    };
    let mut script = headed(quiet_script());
    let entries = script.as_array_mut().unwrap();
    for entry in [
        // The viewer's pending review already exists: found, never created.
        answer(
            &[
                "api",
                "repos/acme/widgets/pulls/1/reviews",
                "--hostname",
                "github.com",
            ],
            0,
            r#"[{"id": 9, "node_id": "PR_kwDOx", "state": "PENDING",
                "body": "<!-- ariadne:pending-review-verdict -->",
                "user": {"login": "me"}}]"#,
        ),
        answer(
            &[
                "api",
                "graphql",
                "--hostname",
                "github.com",
                "-f",
                "query=mutation($reviewId: ID!, $path: String!, $line: Int!, $body: String!) { \
                 addPullRequestReviewThread(input: {pullRequestReviewId: $reviewId, path: \
                 $path, line: $line, side: RIGHT, body: $body}) { thread { comments(first: 1) \
                 { nodes { databaseId createdAt } } } } }",
            ],
            0,
            r#"{"data":{"addPullRequestReviewThread":{"thread":{"comments":{"nodes":[
                {"databaseId": 501, "createdAt": "2026-10-02T00:00:00Z"}]}}}}}"#,
        ),
        answer(
            &[
                "api",
                "graphql",
                "--hostname",
                "github.com",
                "-f",
                "query=mutation($id: ID!, $body: String!) { \
                 updatePullRequestReview(input: {pullRequestReviewId: $id, body: $body}) { \
                 pullRequestReview { id } } }",
            ],
            0,
            r#"{"data":{"updatePullRequestReview":{"pullRequestReview":{"id":"PR_kwDOx"}}}}"#,
        ),
        answer(
            &[
                "api",
                "graphql",
                "--hostname",
                "github.com",
                "--paginate",
                "-f",
                "query=query($id: ID!, $endCursor: String) { node(id: $id) { ... on \
                 PullRequestReview { comments(first: 100, after: $endCursor) { pageInfo { \
                 hasNextPage endCursor } nodes { id databaseId } } } } }",
            ],
            0,
            r#"{"data":{"node":{"comments":{"nodes":[{"id":"PRRC_1","databaseId":501}]}}}}"#,
        ),
        answer(
            &[
                "api",
                "graphql",
                "--hostname",
                "github.com",
                "-f",
                "query=mutation($id: ID!, $body: String!) { updatePullRequestReviewComment(input: \
                 {pullRequestReviewCommentId: $id, body: $body}) { pullRequestReviewComment { \
                 databaseId } } }",
            ],
            0,
            r#"{"data":{"updatePullRequestReviewComment":{"pullRequestReviewComment":{"databaseId":501}}}}"#,
        ),
        answer(
            &[
                "api",
                "graphql",
                "--hostname",
                "github.com",
                "-f",
                "query=mutation($id: ID!) { deletePullRequestReviewComment(input: \
                 {pullRequestReviewCommentId: $id}) { pullRequestReview { id } } }",
            ],
            0,
            r#"{"data":{"deletePullRequestReviewComment":{"pullRequestReview":{"id":"PR_kwDOx"}}}}"#,
        ),
    ] {
        entries.insert(0, entry);
    }
    stub.reprogram(script);
    h.state.forge_poll.wake(&repo);
    eventually(
        TIMEOUT,
        "the detail fetch to read the new head",
        async || {
            h.launcher.live.get(&id).is_some_and(|live| {
                live.pull.head_sha == tip
                    && live
                        .details
                        .as_ref()
                        .is_some_and(|details| details.head_sha == tip)
            })
        },
    )
    .await;
    h.flush_scheduler().await;

    let ask = Request::builder()
        .method("PUT")
        .uri(format!(
            "/v1/repositories/{repo}/pull-requests/1/ariadne-review"
        ))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"asked": true, "model": "stub:review-model"}).to_string(),
        ))
        .unwrap();
    let _: Value = h.json(ask, StatusCode::OK).await;
    let mut review = None;
    eventually(TIMEOUT, "the review session to be idle", async || {
        review = h
            .store
            .list_sessions(SessionFilter {
                pull_request_id: Some(id.clone()),
                ..Default::default()
            })
            .await
            .unwrap()
            .into_iter()
            .find(|s| s.status() == SessionStatus::Idle && s.launched_at.is_some());
        review.is_some()
    })
    .await;
    let review = review.unwrap();
    let posted: Value = h
        .json(
            as_session(
                &format!("/v1/pull-requests/{id}/reviews"),
                &review.id,
                json!({"event": "comment", "body": "One P0.", "comments": [
                    {"path": "widget", "line": 1, "title": "An empty list panics",
                     "body": "An empty list panics.", "priority": "P0"}
                ]}),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert_eq!(posted, json!({"pending": true}));

    let edited: Value = h
        .json(
            as_session(
                &format!("/v1/pull-requests/{id}/comments/edit"),
                &review.id,
                json!({"forge_id": "rc-501", "body": "**[P0] Fixed on review**\n\nActually fine."}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(edited["id"], "rc-501");
    let comments: Vec<Value> = h.get(&format!("/v1/pull-requests/{id}/comments")).await;
    let finding = comments
        .iter()
        .find(|c| c["id"] == "rc-501")
        .expect("the edited finding is still held");
    assert_eq!(
        finding["body"], "**[P0] Fixed on review**\n\nActually fine.",
        "list_comments reads the edited body, never the one it was posted with: {comments:?}"
    );

    let deleted: Value = h
        .json(
            as_session(
                &format!("/v1/pull-requests/{id}/comments/delete"),
                &review.id,
                json!({"forge_id": "rc-501"}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(deleted["id"], "rc-501");
    let comments: Vec<Value> = h.get(&format!("/v1/pull-requests/{id}/comments")).await;
    assert!(
        comments.iter().all(|c| c["id"] != "rc-501"),
        "the deleted finding is gone from list_comments, not merely from the forge: {comments:?}"
    );
}

/// A task over while the last read of its request still says open asks the
/// forge again before Ariadne lets go of it (026 rule 23). A read that
/// fails proves nothing: the row stays. Once the forge says it merged, its
/// work is taken down and the row goes.
#[tokio::test]
async fn a_failed_read_of_an_ended_tasks_request_keeps_its_row() {
    use ariadne_core::Actor;
    let Kept {
        h, stub, task, id, ..
    } = kept_request(quiet_script()).await;
    let mut failing = quiet_script();
    for entry in failing.as_array_mut().unwrap() {
        if entry["args"] == json!(["pr", "view"]) {
            entry["exit"] = json!(1);
            entry["stderr"] = json!("gh: Server Error (HTTP 502)");
        }
    }
    stub.reprogram(failing);
    let tip = sh(&h.at("repo"), &format!("git rev-parse {}", task.branch));
    h.store
        .transition_task(
            &task.id,
            TaskStatus::Finished,
            Actor::Daemon,
            None,
            Some(&tip),
        )
        .await
        .unwrap();
    pass_over(&h, &id).await;
    assert!(
        h.store.get_pull_request(&id).await.is_ok(),
        "a failed read keeps the row"
    );

    stub.reprogram(script("MERGED", &[], json!([])));
    eventually(TIMEOUT, "the merged request's row to go", async || {
        pass_over(&h, &id).await;
        h.store.get_pull_request(&id).await.is_err()
    })
    .await;
}

/// A check that fails is told, its recovery is no news but is recorded, and
/// its next failure is told again.
#[tokio::test]
async fn a_check_that_recovers_and_fails_again_is_told_again() {
    let Kept {
        h,
        stub,
        agent,
        id,
        repo,
        ..
    } = kept_request(quiet_script()).await;
    let failed = json!([{"name": "lint", "html_url": "https://ci.example/1",
        "conclusion": "failure"}]);
    let told_lint = || {
        h.prompts_to(&agent)
            .iter()
            .filter(|p| p.contains("Check lint turned to failure"))
            .count()
    };

    stub.reprogram(script("OPEN", &[], failed.clone()));
    fetch_again(&h, &stub, &repo).await;
    eventually(TIMEOUT, "the failure to be told", async || told_lint() == 1).await;

    stub.reprogram(quiet_script());
    fetch_again(&h, &stub, &repo).await;
    eventually(TIMEOUT, "the recovered check to be stored", async || {
        let dto: Value = h.get(&format!("/v1/pull-requests/{id}")).await;
        dto["failed_checks"].as_array().is_some_and(Vec::is_empty)
    })
    .await;
    let after_recovery = h.prompts_to(&agent).len();

    stub.reprogram(script("OPEN", &[], failed));
    fetch_again(&h, &stub, &repo).await;
    eventually(TIMEOUT, "the second failure to be told", async || {
        told_lint() == 2
    })
    .await;
    assert_eq!(
        h.prompts_to(&agent).len(),
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
        "[]",
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
        // The combined check state's own `graphql` call (026) never
        // paginates: one aliased read answers every number at once, unlike
        // the review threads read it shares the program and the subcommand
        // with.
        let is_check_states = call.args.get(4).is_some_and(|a| a == "-f");
        let paged = !is_check_states
            && call
                .args
                .get(1)
                .is_some_and(|a| a == "graphql" || a.ends_with("/check-runs"));
        if paged {
            assert!(call.args.contains(&"--paginate".to_string()), "{call:?}");
        }
    }
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
        "[]",
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
    let ask = Request::builder()
        .method("PUT")
        .uri(format!(
            "/v1/repositories/{repo}/pull-requests/1/ariadne-review"
        ))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"asked": true, "model": "stub:review-model"}).to_string(),
        ))
        .unwrap();
    let dto: Value = h.json(ask, StatusCode::OK).await;
    assert_eq!(dto["role"], "reviewer");
    let id = dto["id"].as_str().unwrap().to_string();
    h.state.forge_poll.set_mode(&repo, Mode::WakeOnly);

    read["state"] = json!("CLOSED");
    stub.reprogram(script_of(
        read,
        &[],
        &[],
        r#"{"check_runs": []}"#,
        "[]",
        &threads.to_string(),
    ));
    h.state.forge_poll.wake(&repo);
    eventually(
        TIMEOUT,
        "Ariadne to stop working on the closed request",
        async || {
            h.flush_scheduler().await;
            h.store.get_pull_request(&id).await.is_err()
        },
    )
    .await;
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
