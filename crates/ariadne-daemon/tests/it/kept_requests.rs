//! A request of mine is kept by the agent of the `pr` column of the task
//! that opened it, until a human merges or closes it (030, 026): the detail
//! fetch, the news handed to that agent's own session, the tools it answers
//! through, and the cleanup once the request ended and its task is over. A
//! request of mine gets no session of its own.
use std::path::PathBuf;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{Value, json};

use crate::common::forge::{StubForgeCli, answer, stub_forge_cli};
use crate::common::{
    Harness, QUIET, TIMEOUT, as_session, eventually, harness, post_json, sh, with_forge,
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
        // The review's one summary comment: posted once, then edited.
        answer(
            &[
                "api",
                "repos/acme/widgets/issues/1/comments",
                "--hostname",
                "github.com",
                "--method",
                "POST"
            ],
            0,
            r#"{"id": 301, "created_at": "2026-10-02T00:00:00Z"}"#
        ),
        answer(
            &["api", "repos/acme/widgets/issues/comments/301"],
            0,
            r#"{"id": 301, "created_at": "2026-10-02T00:00:00Z"}"#
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

async fn kept_request(script: Value) -> Kept {
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
    let repo = cast.repo.id.clone();
    h.state.forge_poll.set_mode(&repo, Mode::WakeOnly);
    eventually(
        TIMEOUT,
        "the request agent's first turn to end",
        async || h.session_status(&agent).await == SessionStatus::Idle,
    )
    .await;
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
    stub.reprogram(script("OPEN", &comments, json!([])));
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

    fetch_again(&h, &stub, &repo).await;
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

/// The `pr` agent replies through the forge's reply command, stored as my
/// comment in the thread, which answers it; the agent resolves no thread.
/// Its `ready: true` is stored but raises nothing of its own on the
/// session (029): the `pull_request` attention producer reads the forge's
/// own evidence against the claim instead. Any other session is refused,
/// and so is a call that comes from no session.
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
            json!({"ready": ready}),
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
        "path": "src/lib.rs", "line": 3, "created_at": "2026-10-02T00:00:00Z"}]);
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
    fetch_again(&h, &stub, &repo).await;

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
                    {"path": "src/lib.rs", "line": 3, "title": "Untested",
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
    // The forge holds what the review posted from now on: the finding on
    // its line, and the summary on the conversation.
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
    stub.reprogram(script.clone());
    let dto: Value = h.get(&format!("/v1/pull-requests/{id}")).await;
    assert_eq!(
        dto["unanswered_comments"], 2,
        "the finding and the summary wait"
    );

    fetch_again(&h, &stub, &repo).await;
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
        "path": "src/lib.rs", "line": 3, "created_at": "2026-10-03T00:00:00Z"}),
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
        dto["unanswered_comments"], 1,
        "the agent's reply answers the finding"
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
                json!({"path": path, "forge": {"enabled": true, "review_model": "stub:review-model"}}),
            ),
            StatusCode::CREATED,
        )
        .await;
    let repo = created["id"].as_str().unwrap().to_string();
    let mut rows = Vec::new();
    eventually(
        TIMEOUT,
        "Ariadne to start reviewing the request",
        async || {
            rows = h
                .store
                .list_pull_requests(ariadne_store::PullRequestFilter::default())
                .await
                .unwrap();
            rows.len() == 1
        },
    )
    .await;
    assert_eq!(rows[0].role, "reviewer");
    let id = rows[0].id.clone();
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
