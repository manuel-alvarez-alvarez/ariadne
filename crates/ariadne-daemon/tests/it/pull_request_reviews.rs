//! A request where I am a requested reviewer gets a reviewer session of its
//! own (029): detached at the request's head, told of each push once, posting
//! one review through the daemon, and taken down once the request ends or no
//! longer asks for my review.
use std::path::{Path, PathBuf};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{Value, json};

use crate::common::forge::{StubForgeCli, answer, stub_forge_cli};
use crate::common::{Harness, QUIET, TIMEOUT, as_session, eventually, harness, post_json, sh};
use ariadne_api::SESSION_HEADER;
use ariadne_core::SessionStatus;
use ariadne_daemon::forge::poll::Mode;
use ariadne_store::{AgentSession, SessionFilter};

const PIN: &str = "stub:review-model";

/// How the stub forge shows the request: its state, whether it is a draft,
/// its head, whether the list of requests that ask for my review holds it,
/// and the events about me on its timeline, oldest first.
struct Shown<'a> {
    /// Who opened it: `other`, or `me` for a request of mine.
    author: &'a str,
    state: &'a str,
    draft: bool,
    head: &'a str,
    listed: bool,
    timeline: &'a [&'a str],
}

impl Shown<'_> {
    fn open(head: &str) -> Shown<'_> {
        Shown {
            author: "other",
            state: "OPEN",
            draft: false,
            head,
            listed: true,
            timeline: &["review_requested"],
        }
    }
}

/// What the stub forge answers for one request by `other` that asks for my
/// review: the lists, its own read, its details, and the review it posts.
fn script(shown: &Shown) -> Value {
    let review_requests = match shown.listed {
        true => json!([{"login": "me"}]),
        false => json!([]),
    };
    let read = json!({"number": 1, "url": "https://github.com/acme/widgets/pull/1",
        "title": "Add widgets", "author": {"login": shown.author}, "state": shown.state,
        "isDraft": shown.draft, "headRefName": "fix", "headRefOid": shown.head,
        "headRepository": {"url": "https://github.com/acme/widgets"},
        "baseRefName": "main", "statusCheckRollup": [], "reviewDecision": "",
        "reviewRequests": review_requests,
        "createdAt": "2026-10-01T00:00:00Z"});
    let listed = match shown.listed && shown.state == "OPEN" {
        true => json!([read]),
        false => json!([]),
    };
    let threads =
        json!({"data": {"repository": {"pullRequest": {"reviewThreads": {"nodes": []}}}}});
    let review_comments = json!([
        {"id": 501, "body": "**[P0] Empty list**\n\nAn empty list panics.", "path": "a.txt", "line": 1,
         "created_at": "2026-10-02T00:00:00Z"},
        {"id": 502, "body": "**[P1] Untested**\n\nNo test covers the empty list.", "path": "a.txt",
         "line": 2, "created_at": "2026-10-02T00:00:00Z"}
    ]);
    let timeline: Vec<Value> = shown
        .timeline
        .iter()
        .map(|event| match *event {
            "reviewed" => json!({"event": "reviewed", "user": {"login": "me"}}),
            event => json!({"event": event, "requested_reviewer": {"login": "me"}}),
        })
        .collect();
    json!([
        answer(
            &["api", "repos/acme/widgets/issues/1/timeline"],
            0,
            &json!(timeline).to_string()
        ),
        answer(&["auth", "status"], 0, ""),
        answer(&["api", "user"], 0, "me"),
        answer(&["pr", "list"], 0, &listed.to_string()),
        answer(&["pr", "view"], 0, &read.to_string()),
        answer(
            &[
                "api",
                "repos/acme/widgets/pulls/1/reviews",
                "--hostname",
                "github.com",
                "--method",
                "POST"
            ],
            0,
            r#"{"id": 77, "submitted_at": "2026-10-02T00:00:00Z"}"#
        ),
        answer(
            &["api", "repos/acme/widgets/pulls/1/reviews/77/comments"],
            0,
            &review_comments.to_string()
        ),
        answer(&["api", "repos/acme/widgets/pulls/1/comments"], 0, "[]"),
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
/// of `main`, `a.txt` of two lines. Answers the checkout and that commit.
fn checkout(h: &Harness) -> (PathBuf, String) {
    let path = h.git_repo("widgets");
    sh(
        &path,
        "git remote add origin https://github.com/acme/widgets.git && \
         git checkout -q -b fix && printf 'a\\nb\\n' > a.txt && git add a.txt && \
         git -c user.email=t@t -c user.name=t commit -qm a && git checkout -q main",
    );
    let head = sh(&path, "git rev-parse fix");
    (path, head)
}

/// Push one more commit to `fix`, `b.txt`, without checking it out, and
/// answer it.
fn push(path: &Path) -> String {
    sh(
        path,
        "git worktree add -q ../push fix && cd ../push && echo b > b.txt && \
         git add b.txt && git -c user.email=t@t -c user.name=t commit -qm b && \
         cd - >/dev/null && git worktree remove ../push",
    );
    sh(path, "git rev-parse fix")
}

/// The repository registered with its integration on. `supervise_posts` is
/// explicitly off: a repository defaults to holding its posts back (026
/// rule 24), and this file's reviews and resolves are about the forge's
/// own CLI calls, posted directly, rather than that held-back path
/// (covered instead by
/// `github/reviews.rs::tests::submit_review_pending_adds_each_finding_without_changing_the_body`).
async fn repository(h: &Harness, path: &Path) -> String {
    let repo: Value = h
        .json(
            post_json(
                "/v1/repositories",
                json!({"path": path, "forge": {"enabled": true}, "supervise_posts": false}),
            ),
            StatusCode::CREATED,
        )
        .await;
    repo["id"].as_str().unwrap().to_string()
}

/// Ask Ariadne to review request `number` on `model`: the manual start
/// every review now takes (029), where a test once relied on the
/// repository's own review pin.
///
/// Waits for the row's own live cache evidence that the repository's
/// last-triggered fetch actually finished reading it, not merely that the
/// forge CLI's own `compare` call started: `common/forge.rs`'s stub logs
/// an invocation the moment it is run, before it waits on any gate or
/// writes its answer, so counting invocations proves nothing about
/// whether the read — and the `wake_scheduler` call right after it writes
/// the cache, in `forge/poll.rs::read_row` — ever actually ran. `evidence_ok`
/// is written only once that read's answer is in hand, which is also this
/// call's own write (`http/pull_requests.rs::ask_review`) racing a stale
/// or still-running one.
async fn ask_review(h: &Harness, id: &str, repo: &str, number: i64, model: &str) -> Value {
    eventually(TIMEOUT, "the request's live details", async || {
        h.launcher.live.get(id).is_some_and(|l| l.evidence_ok)
    })
    .await;
    let ask = axum::http::Request::builder()
        .method("PUT")
        .uri(format!(
            "/v1/repositories/{repo}/pull-requests/{number}/ariadne-review"
        ))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"asked": true, "model": model}).to_string(),
        ))
        .unwrap();
    h.json(ask, StatusCode::OK).await
}

/// The one request of the ledger, once the first fetch recorded it.
async fn the_request(h: &Harness) -> String {
    let mut rows = Vec::new();
    eventually(
        TIMEOUT,
        "Ariadne to start working on the request",
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
    rows[0].id.clone()
}

/// Whether Ariadne works on any request: a row is one.
async fn no_rows(h: &Harness) -> bool {
    h.store
        .list_pull_requests(ariadne_store::PullRequestFilter::default())
        .await
        .unwrap()
        .is_empty()
}

/// The live session on a request, once its briefing turn has ended.
async fn idle_session(h: &Harness, pull_request_id: &str) -> AgentSession {
    let mut found = None;
    eventually(TIMEOUT, "the request's session to be idle", async || {
        found = sessions(h, pull_request_id)
            .await
            .into_iter()
            .find(|s| s.status() == SessionStatus::Idle && s.launched_at.is_some());
        found.is_some()
    })
    .await;
    found.unwrap()
}

async fn sessions(h: &Harness, pull_request_id: &str) -> Vec<AgentSession> {
    h.store
        .list_sessions(SessionFilter {
            pull_request_id: Some(pull_request_id.into()),
            ..Default::default()
        })
        .await
        .unwrap()
}

/// Fetch the repository once more and let the scheduler act on it.
async fn fetch_again(h: &Harness, stub: &StubForgeCli, repo: &str) {
    let before = stub.fetches();
    h.state.forge_poll.wake(repo);
    eventually(TIMEOUT, "the fetch", async || stub.fetches() > before).await;
    h.flush_scheduler().await;
}

fn get_as(uri: &str, session_id: &str) -> Request<Body> {
    Request::builder()
        .uri(uri)
        .header(SESSION_HEADER, session_id)
        .body(Body::empty())
        .unwrap()
}

/// The review posts the stub has seen.
fn review_posts(stub: &StubForgeCli) -> Vec<Vec<String>> {
    review_bodies(stub)
        .into_iter()
        .map(|(args, _)| args)
        .collect()
}

/// The review posts the stub has seen, each with the JSON body it piped in.
fn review_bodies(stub: &StubForgeCli) -> Vec<(Vec<String>, Value)> {
    stub.invocations()
        .into_iter()
        .filter(|i| i.args.iter().any(|a| a == "POST"))
        .filter(|i| i.args.get(1).is_some_and(|path| path.ends_with("/reviews")))
        .map(|i| {
            let body = i
                .input
                .as_deref()
                .map(|input| serde_json::from_str(input).unwrap())
                .unwrap_or(Value::Null);
            (i.args, body)
        })
        .collect()
}

/// An open request out of draft that asks for my review, asked for
/// directly on its own row, gets one live session on that pin: seat
/// `reviewer`, a worktree detached at the head, and the `pr-reviewer`
/// document written and indexed.
#[tokio::test]
async fn an_open_request_i_review_gets_one_session_detached_at_its_head() {
    let stub = stub_forge_cli(json!([]));
    let h = harness().scheduler().forge_cli(&stub).await;
    let (path, head) = checkout(&h);
    stub.reprogram(script(&Shown::open(&head)));
    let repo = repository(&h, &path).await;
    let id = the_request(&h).await;
    ask_review(&h, &id, &repo, 1, PIN).await;
    let session = idle_session(&h, &id).await;

    assert_eq!(session.model, PIN);
    assert_eq!(session.seat.as_deref(), Some("reviewer"));
    assert_eq!(session.goal_id, None);
    assert_eq!(session.task_id, None);
    let worktree = PathBuf::from(session.worktree_path.clone().unwrap());
    assert!(worktree.ends_with(format!("pr-{id}")));
    assert_eq!(sh(&worktree, "git rev-parse HEAD"), head);
    assert_eq!(
        sh(&worktree, "git rev-parse --abbrev-ref HEAD"),
        "HEAD",
        "the worktree is detached"
    );
    let launch = h.launch_file(&session.id).expect("a launch file");
    assert!(
        launch.system_prompt.contains("- pr-reviewer: "),
        "{}",
        launch.system_prompt
    );
    let skill = h
        .launcher
        .cfg
        .run_dir
        .join(&session.id)
        .join("skills/pr-reviewer/SKILL.md");
    assert!(
        std::fs::read_to_string(skill)
            .unwrap()
            .contains("name: pr-reviewer")
    );
    let briefing = h.told(&session.id);
    assert!(
        briefing.contains("# Review pull request: <<<untrusted>>>Add widgets<<<end untrusted>>>"),
        "{briefing}"
    );
    assert!(briefing.contains(&head), "{briefing}");

    h.flush_scheduler().await;
    assert_eq!(sessions(&h, &id).await.len(), 1, "one session");
}

/// A draft starts nothing until it leaves draft.
#[tokio::test]
async fn a_draft_starts_no_review_until_it_leaves_draft() {
    let stub = stub_forge_cli(json!([]));
    let h = harness().scheduler().forge_cli(&stub).await;
    let (path, head) = checkout(&h);
    stub.reprogram(script(&Shown {
        draft: true,
        ..Shown::open(&head)
    }));
    let repo = repository(&h, &path).await;
    h.state.forge_poll.set_mode(&repo, Mode::WakeOnly);
    // Wait for the draft's own fetch round to finish, not merely for a
    // quiet window, so the reprogram below never races a straggler still
    // reading this draft script.
    fetch_again(&h, &stub, &repo).await;
    assert!(no_rows(&h).await, "a draft is no work yet");

    stub.reprogram(script(&Shown::open(&head)));
    h.state.forge_poll.wake(&repo);
    let id = the_request(&h).await;
    ask_review(&h, &id, &repo, 1, PIN).await;
    idle_session(&h, &id).await;
}

/// A request that asks for my review starts no session until the user
/// asks Ariadne for one: every review starts manually now, and there is no
/// repository pin left to start one on its own (the goal this whole task
/// is for).
#[tokio::test]
async fn a_review_requested_pull_request_starts_no_session_until_asked() {
    let stub = stub_forge_cli(json!([]));
    let h = harness().scheduler().forge_cli(&stub).await;
    let (path, head) = checkout(&h);
    stub.reprogram(script(&Shown::open(&head)));
    let repo = repository(&h, &path).await;
    h.state.forge_poll.set_mode(&repo, Mode::WakeOnly);
    h.state.forge_poll.wake(&repo);
    // The request still gets a row — nobody else will ever start a review
    // for it, so a human still has something to start by hand (029) — but
    // asking nobody starts no session of its own.
    let id = the_request(&h).await;
    // Wait for the row's own live cache evidence that the fetch's detail
    // read actually finished, not merely that the forge CLI's `compare`
    // call started (`common/forge.rs`'s stub logs an invocation before it
    // answers). `evidence_ok` is written in the same statement
    // (`forge/poll.rs::read_row`) right before `wake_scheduler` runs.
    eventually(TIMEOUT, "the request's live details", async || {
        h.launcher.live.get(&id).is_some_and(|l| l.evidence_ok)
    })
    .await;
    // Force the scheduler to evaluate the request itself, rather than
    // betting that `wake_scheduler`'s own send already landed in the
    // channel by the time this runs: the two statements share no await
    // point in `read_row`, but nothing stops this test's own task from
    // being scheduled in that gap, on another thread.
    h.state
        .sched_tx
        .as_ref()
        .unwrap()
        .send(ariadne_daemon::scheduler::SchedEvent::PullRequestChanged(
            id.clone(),
        ))
        .unwrap();
    h.flush_scheduler().await;
    assert!(
        sessions(&h, &id).await.is_empty(),
        "nothing reviews it on its own, so it is no automatic work"
    );

    ask_review(&h, &id, &repo, 1, PIN).await;
    idle_session(&h, &id).await;
}

/// A push moves the worktree to the new head and hands the session one
/// prompt that names the last sha it reviewed. The same head again hands
/// nothing.
#[tokio::test]
async fn a_push_moves_the_worktree_and_is_told_once_with_the_last_reviewed_sha() {
    let stub = stub_forge_cli(json!([]));
    let h = harness().scheduler().forge_cli(&stub).await;
    let (path, first) = checkout(&h);
    let before = stub.fetches();
    stub.reprogram(script(&Shown::open(&first)));
    let repo = repository(&h, &path).await;
    let id = the_request(&h).await;
    ask_review(&h, &id, &repo, 1, PIN).await;
    h.state.forge_poll.set_mode(&repo, Mode::WakeOnly);
    let session = idle_session(&h, &id).await;
    // Wait for the repository's own first fetch round to finish: the row,
    // the evidence and the idle session above all land mid-round, well
    // before the round's own trailing calls do.
    eventually(
        TIMEOUT,
        "the repository's first fetch to finish",
        async || stub.fetches() > before,
    )
    .await;
    let worktree = PathBuf::from(session.worktree_path.clone().unwrap());
    let _: Value = h
        .json(
            as_session(
                &format!("/v1/pull-requests/{id}/report"),
                &session.id,
                json!({"reviewed_sha": first}),
            ),
            StatusCode::OK,
        )
        .await;
    let briefed = h.prompts_to(&session).len();

    let second = push(&path);
    stub.reprogram(script(&Shown::open(&second)));
    fetch_again(&h, &stub, &repo).await;
    eventually(TIMEOUT, "the push to reach the agent", async || {
        h.prompts_to(&session).len() == briefed + 1
    })
    .await;
    let news = h.prompts_to(&session).pop().unwrap();
    assert!(
        news.contains(&format!(
            "the head moved to <<<untrusted>>>{second}<<<end untrusted>>>"
        )),
        "{news}"
    );
    assert!(
        news.contains(&format!(
            "The last sha you reviewed is <<<untrusted>>>{first}<<<end untrusted>>>"
        )),
        "{news}"
    );
    assert_eq!(sh(&worktree, "git rev-parse HEAD"), second);

    fetch_again(&h, &stub, &repo).await;
    // Prove that the completed fetch sends no duplicate news for this head.
    tokio::time::sleep(QUIET).await;
    assert_eq!(
        h.prompts_to(&session).len(),
        briefed + 1,
        "the same head is no news"
    );
}

/// A pending review of the viewer's own, and its pending comments and
/// replies — the shape pull request #110 showed waking the reviewer
/// agent too, not only the keeper — give the reviewer session's own
/// `list_comments` nothing of it, and no news to wake it with: the
/// detail read's own filter (026 rule 9) covers a reviewer's request the
/// same way it covers a kept one's
/// (`kept_requests.rs::a_pending_reviews_comments_give_no_live_view_comment_and_no_news`).
#[tokio::test]
async fn a_pending_review_gives_the_reviewer_session_no_live_view_comment_or_news() {
    let stub = stub_forge_cli(json!([]));
    let h = harness().scheduler().forge_cli(&stub).await;
    let (path, head) = checkout(&h);
    stub.reprogram(script(&Shown::open(&head)));
    let repo = repository(&h, &path).await;
    let id = the_request(&h).await;
    ask_review(&h, &id, &repo, 1, PIN).await;
    h.state.forge_poll.set_mode(&repo, Mode::WakeOnly);
    let session = idle_session(&h, &id).await;
    let briefed = h.prompts_to(&session).len();

    let pending_comments = json!([
        {"id": 601, "in_reply_to_id": null, "user": {"login": "me"}, "body": "a pending finding",
         "path": "a.txt", "line": 1, "created_at": "2026-10-02T00:00:00Z",
         "pull_request_review_id": 9},
        {"id": 602, "in_reply_to_id": 601, "user": {"login": "me"}, "body": "a pending reply",
         "path": "a.txt", "line": 1, "created_at": "2026-10-02T00:01:00Z",
         "pull_request_review_id": 9},
    ]);
    let reviews = json!([{"id": 9, "user": {"login": "me"}, "body": "", "submitted_at": null}]);
    let mut pending_script = script(&Shown::open(&head));
    for entry in pending_script.as_array_mut().unwrap() {
        if entry["args"] == json!(["api", "repos/acme/widgets/pulls/1/comments"]) {
            entry["stdout"] = json!(pending_comments.to_string());
        }
        if entry["args"] == json!(["api", "repos/acme/widgets/pulls/1/reviews"]) {
            entry["stdout"] = json!(reviews.to_string());
        }
    }
    // No fetch wait needed here: both edited entries are per-row reads round
    // one's own `details()` already consumed before the idle session above.
    stub.reprogram(pending_script);
    fetch_again(&h, &stub, &repo).await;

    let comments: Vec<Value> = h.get(&format!("/v1/pull-requests/{id}/comments")).await;
    assert!(comments.is_empty(), "{comments:?}");
    // Prove the quiet, not merely that nothing has arrived yet.
    tokio::time::sleep(QUIET).await;
    assert_eq!(
        h.prompts_to(&session).len(),
        briefed,
        "a pending review wakes no reviewer session"
    );
}

/// A push reaches an idle review session on the very next reconcile, with
/// no wait for the news to settle (029): back-to-back pushes each hand
/// the agent their own prompt, rather than being folded into one.
#[tokio::test]
async fn a_push_reaches_an_idle_session_without_a_wait() {
    let stub = stub_forge_cli(json!([]));
    let h = harness().scheduler().forge_cli(&stub).await;
    let (path, first) = checkout(&h);
    let before = stub.fetches();
    stub.reprogram(script(&Shown::open(&first)));
    let repo = repository(&h, &path).await;
    let id = the_request(&h).await;
    ask_review(&h, &id, &repo, 1, PIN).await;
    h.state.forge_poll.set_mode(&repo, Mode::WakeOnly);
    let session = idle_session(&h, &id).await;
    // Wait for the repository's own first fetch round to finish: the row,
    // the evidence and the idle session above all land mid-round, well
    // before the round's own trailing calls do.
    eventually(
        TIMEOUT,
        "the repository's first fetch to finish",
        async || stub.fetches() > before,
    )
    .await;
    let worktree = PathBuf::from(session.worktree_path.clone().unwrap());
    let briefed = h.prompts_to(&session).len();
    let commit = |file: &str| {
        sh(
            &path,
            &format!(
                "git worktree add -q ../burst fix && cd ../burst && echo {file} > {file} && \
                 git add {file} && git -c user.email=t@t -c user.name=t commit -qm {file} && \
                 cd - >/dev/null && git worktree remove ../burst"
            ),
        );
        sh(&path, "git rev-parse fix")
    };

    let second = commit("second.txt");
    stub.reprogram(script(&Shown::open(&second)));
    fetch_again(&h, &stub, &repo).await;
    eventually(
        TIMEOUT,
        "the first push to reach the idle session",
        async || h.prompts_to(&session).len() == briefed + 1,
    )
    .await;
    assert_eq!(
        h.prompts_to(&session).len(),
        briefed + 1,
        "the push reaches the idle session at once, with no settle wait"
    );
    assert_eq!(sh(&worktree, "git rev-parse HEAD"), second);

    let idle = idle_session(&h, &id).await;
    let third = commit("third.txt");
    stub.reprogram(script(&Shown::open(&third)));
    fetch_again(&h, &stub, &repo).await;
    eventually(
        TIMEOUT,
        "the next push to reach the idle session",
        async || h.prompts_to(&idle).len() == briefed + 2,
    )
    .await;
    assert_eq!(
        h.prompts_to(&idle).len(),
        briefed + 2,
        "the next push reaches it too, at once"
    );
    assert_eq!(sh(&worktree, "git rev-parse HEAD"), third);
}

/// `get_diff` reads the change of the worktree against its base, and
/// `since` narrows it to the commits after that sha. Another session is
/// refused.
#[tokio::test]
async fn the_diff_reads_the_worktree_against_its_base_and_since_narrows_it() {
    let stub = stub_forge_cli(json!([]));
    let h = harness().scheduler().forge_cli(&stub).await;
    let (path, first) = checkout(&h);
    let second = push(&path);
    stub.reprogram(script(&Shown::open(&second)));
    let repo = repository(&h, &path).await;
    let id = the_request(&h).await;
    ask_review(&h, &id, &repo, 1, PIN).await;
    let session = idle_session(&h, &id).await;

    let read = async |uri: String| {
        let (status, body) = h.send(get_as(&uri, &session.id)).await;
        assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
        String::from_utf8(body).unwrap()
    };
    let whole = read(format!("/v1/pull-requests/{id}/diff")).await;
    assert!(whole.contains("+++ b/a.txt"), "{whole}");
    assert!(whole.contains("+++ b/b.txt"), "{whole}");
    assert!(
        !whole.contains("file.txt"),
        "the base is not the change: {whole}"
    );
    let since = read(format!("/v1/pull-requests/{id}/diff?since={first}")).await;
    assert!(since.contains("+++ b/b.txt"), "{since}");
    assert!(!since.contains("a.txt"), "{since}");

    let unrelated = sh(
        &path,
        "printf unrelated > unrelated.txt && git add unrelated.txt && \\
         git -c user.email=t@t -c user.name=t commit -qm unrelated && git rev-parse HEAD",
    );
    h.error(
        get_as(
            &format!("/v1/pull-requests/{id}/diff?since={unrelated}"),
            &session.id,
        ),
        StatusCode::BAD_REQUEST,
    )
    .await;

    h.error(
        get_as(
            &format!("/v1/pull-requests/{id}/diff?since=--all"),
            &session.id,
        ),
        StatusCode::BAD_REQUEST,
    )
    .await;
    let stranger = h.lone_session("stranger").await;
    h.error(
        get_as(&format!("/v1/pull-requests/{id}/diff"), &stranger.id),
        StatusCode::FORBIDDEN,
    )
    .await;
}

/// A review asking for changes runs the forge's review call with
/// `REQUEST_CHANGES`, both comments led by their priorities, and is stored
/// as my comments. An approval is refused with 400, and the forge sees no
/// call.
#[tokio::test]
async fn a_review_posts_its_findings_by_priority_and_an_approval_is_refused() {
    let stub = stub_forge_cli(json!([]));
    let h = harness().scheduler().forge_cli(&stub).await;
    let (path, head) = checkout(&h);
    stub.reprogram(script(&Shown::open(&head)));
    let repo = repository(&h, &path).await;
    let id = the_request(&h).await;
    ask_review(&h, &id, &repo, 1, PIN).await;
    let session = idle_session(&h, &id).await;
    let reviews = format!("/v1/pull-requests/{id}/reviews");

    h.error(
        as_session(
            &reviews,
            &session.id,
            json!({"event": "approve", "body": "Looks good."}),
        ),
        StatusCode::BAD_REQUEST,
    )
    .await;
    assert!(
        review_posts(&stub).is_empty(),
        "an approval reaches no forge"
    );

    // A change request with no P0 or P1 inline, and none open, is refused.
    h.error(
        as_session(&reviews, &session.id, json!({"event": "request_changes"})),
        StatusCode::BAD_REQUEST,
    )
    .await;
    h.error(
        as_session(
            &reviews,
            &session.id,
            json!({"event": "comment", "body": "One finding.", "comments": [
                {"path": "a.txt", "line": 1, "body": "No title.", "priority": "P2"}
            ]}),
        ),
        StatusCode::BAD_REQUEST,
    )
    .await;
    // A finding on a line the diff does not show — past the one line
    // `a.txt` has, or in a file the change never touched — is refused
    // before the forge sees it: GitHub takes no such comment, and on a
    // pending review answers it with no thread at all.
    for (path, line) in [("a.txt", 7), ("src/lib.rs", 1)] {
        let refused = h
            .error(
                as_session(
                    &reviews,
                    &session.id,
                    json!({"event": "comment", "body": "One finding.", "comments": [
                        {"path": path, "line": line, "title": "Off the diff",
                         "body": "Nothing here.", "priority": "P2"}
                    ]}),
                ),
                StatusCode::BAD_REQUEST,
            )
            .await;
        assert!(
            refused
                .error
                .message
                .contains(&format!("no line {line} of {path}")),
            "the refusal names the line: {refused:?}"
        );
    }
    assert!(
        review_posts(&stub).is_empty(),
        "a refused review reaches no forge"
    );

    let stored: Vec<Value> = h
        .json(
            as_session(
                &reviews,
                &session.id,
                json!({"event": "request_changes", "comments": [
                    {"path": "a.txt", "line": 1, "title": "Empty list",
                     "body": "An empty list panics.", "priority": "P0"},
                    {"path": "a.txt", "line": 2, "title": "Untested",
                     "body": "No test covers the empty list.", "priority": "P1"}
                ]}),
            ),
            StatusCode::CREATED,
        )
        .await;
    let posts = review_posts(&stub);
    assert_eq!(posts.len(), 1, "{posts:?}");
    let call = &posts[0];
    assert_eq!(call[1], "repos/acme/widgets/pulls/1/reviews");
    // One JSON body on standard input, each comment a whole object: `gh`'s
    // `-f comments[][...]` fields split them across objects (029).
    assert!(call.ends_with(&["--input".into(), "-".into()]), "{call:?}");
    let (_, review) = review_bodies(&stub).pop().unwrap();
    assert_eq!(
        review,
        json!({"event": "REQUEST_CHANGES", "commit_id": head, "comments": [
            // Each finding is signed the review's, invisibly (029).
            {"path": "a.txt", "line": 1, "side": "RIGHT",
             "body": "**[P0] Empty list**\n\nAn empty list panics.\n\n<!-- ariadne:review -->"},
            {"path": "a.txt", "line": 2, "side": "RIGHT",
             "body": "**[P1] Untested**\n\nNo test covers the empty list.\n\n<!-- ariadne:review -->"}
        ]}),
        "the review carries no summary of its own"
    );
    let bodies: Vec<&str> = stored.iter().map(|c| c["body"].as_str().unwrap()).collect();
    assert_eq!(
        bodies,
        [
            "**[P0] Empty list**\n\nAn empty list panics.",
            "**[P1] Untested**\n\nNo test covers the empty list.",
        ]
    );
    assert!(stored.iter().all(|c| c["author_login"] == "me"));
    // What is kept of them is the mark that the review posted them; their
    // text is the forge's.
    let marks = h.store.pull_request_comment_marks(&id).await.unwrap();
    assert_eq!(marks.len(), 2);
    assert!(marks.iter().all(|m| m.from_review));

    // A later round with no new finding posts no forge review or status.
    let none: Vec<Value> = h
        .json(
            as_session(&reviews, &session.id, json!({"event": "request_changes"})),
            StatusCode::CREATED,
        )
        .await;
    assert!(none.is_empty());
    assert_eq!(review_posts(&stub).len(), 1, "no second review");
    assert_eq!(
        h.store.pull_request_comment_marks(&id).await.unwrap().len(),
        2,
        "the no-finding round writes no comment"
    );
}

/// A trailer in a finding never reaches the forge.
#[tokio::test]
async fn a_review_strips_a_trailing_trailer_from_its_findings() {
    let stub = stub_forge_cli(json!([]));
    let h = harness().scheduler().forge_cli(&stub).await;
    let (path, head) = checkout(&h);
    stub.reprogram(script(&Shown::open(&head)));
    let repo = repository(&h, &path).await;
    let id = the_request(&h).await;
    ask_review(&h, &id, &repo, 1, PIN).await;
    let session = idle_session(&h, &id).await;
    let reviews = format!("/v1/pull-requests/{id}/reviews");

    let footer = "\n\n🤖 Generated with [Claude Code](https://claude.com/claude-code)";
    let _: Vec<Value> = h
        .json(
            as_session(
                &reviews,
                &session.id,
                json!({"event": "request_changes", "comments": [
                    {"path": "a.txt", "line": 1, "title": "Empty list",
                     "body": format!("An empty list panics.{footer}"), "priority": "P0"}
                ]}),
            ),
            StatusCode::CREATED,
        )
        .await;
    let (_, review) = review_bodies(&stub).pop().unwrap();
    assert_eq!(
        review["comments"][0]["body"],
        json!("**[P0] Empty list**\n\nAn empty list panics.\n\n<!-- ariadne:review -->")
    );
}

/// Once a push fixed a finding, the review session resolves the thread it
/// opened: through `resolveReviewThread` on the thread GitHub holds for the
/// comment, and the stored thread reads resolved. A thread somebody else
/// opened is refused, and so is a resolve from any other session.
#[tokio::test]
async fn a_review_resolves_its_own_fixed_finding_only_when_posts_are_not_supervised() {
    let stub = stub_forge_cli(json!([]));
    let h = harness().scheduler().forge_cli(&stub).await;
    let (path, head) = checkout(&h);
    let threads = json!({"data": {"repository": {"pullRequest": {"reviewThreads": {"nodes": [
        {"id": "PRRT_501", "isResolved": false, "comments": {"nodes": [{"databaseId": 501}]}},
        {"id": "PRRT_601", "isResolved": false, "comments": {"nodes": [{"databaseId": 601}]}}
    ]}}}}});
    let theirs = json!([{"id": 601, "user": {"login": "other", "type": "User"},
        "body": "Why this name?", "path": "a.txt", "line": 1,
        "created_at": "2026-10-01T00:00:00Z"}]);
    let mut script = script(&Shown::open(&head));
    let entries = script.as_array_mut().unwrap();
    entries.insert(0, answer(&["api", "graphql"], 0, &threads.to_string()));
    entries.insert(
        0,
        answer(
            &["api", "repos/acme/widgets/pulls/1/comments"],
            0,
            &theirs.to_string(),
        ),
    );
    stub.reprogram(script);
    let repo = repository(&h, &path).await;
    let id = the_request(&h).await;
    ask_review(&h, &id, &repo, 1, PIN).await;
    let session = idle_session(&h, &id).await;

    let stored: Vec<Value> = h
        .json(
            as_session(
                &format!("/v1/pull-requests/{id}/reviews"),
                &session.id,
                json!({"event": "request_changes", "body": "One P0.", "comments": [
                    {"path": "a.txt", "line": 1, "title": "Empty list",
                     "body": "An empty list panics.", "priority": "P0"}
                ]}),
            ),
            StatusCode::CREATED,
        )
        .await;
    let mine = stored
        .iter()
        .find(|c| c["id"] == "rc-501")
        .expect("the finding is stored")["id"]
        .as_str()
        .unwrap()
        .to_string();
    let resolve = |comment: &str, as_: &str| {
        as_session(
            &format!("/v1/pull-requests/{id}/comments/{comment}/resolve"),
            as_,
            json!({}),
        )
    };
    let stranger = h.lone_session("stranger").await;
    h.error(resolve(&mine, &stranger.id), StatusCode::FORBIDDEN)
        .await;

    let repository = h
        .store
        .get_repository(&h.store.get_pull_request(&id).await.unwrap().repository_id)
        .await
        .unwrap();
    h.set_supervise_posts(&repository, true).await;
    h.error(resolve(&mine, &session.id), StatusCode::CONFLICT)
        .await;
    assert!(
        stub.invocations().iter().all(|call| !call
            .args
            .iter()
            .any(|arg| arg.contains("resolveReviewThread"))),
        "supervision leaves resolution to the user"
    );
    h.set_supervise_posts(&repository, false).await;

    let resolved: Value = h.json(resolve(&mine, &session.id), StatusCode::OK).await;
    assert_eq!(resolved["resolved"], true);
    let call = stub
        .invocations()
        .into_iter()
        .find(|i| i.args.iter().any(|a| a.contains("resolveReviewThread")))
        .expect("the resolve mutation");
    assert!(call.args.contains(&"id=PRRT_501".to_string()), "{call:?}");

    fetch_again(
        &h,
        &stub,
        &h.store.get_pull_request(&id).await.unwrap().repository_id,
    )
    .await;
    let comments: Vec<Value> = h.get(&format!("/v1/pull-requests/{id}/comments")).await;
    let theirs = comments
        .iter()
        .find(|c| c["id"] == "rc-601")
        .expect("their comment is stored");
    h.error(
        resolve(theirs["id"].as_str().unwrap(), &session.id),
        StatusCode::FORBIDDEN,
    )
    .await;
    let mutations = stub
        .invocations()
        .iter()
        .filter(|i| i.args.iter().any(|a| a.contains("resolveReviewThread")))
        .count();
    assert_eq!(mutations, 1, "their thread stays open");
}

/// A reported `reviewed_sha` is stored, but raises nothing of its own on
/// the session: a pr-reviewer session finishing its review must notify
/// nobody (029), and the `pull_request` attention producer never reads a
/// `waiting_user` flag to decide anything. The same sha again, and a later
/// one, both leave the session's attention alone.
#[tokio::test]
async fn a_reviewed_sha_is_stored_and_raises_no_waiting_user() {
    let stub = stub_forge_cli(json!([]));
    let h = harness().scheduler().forge_cli(&stub).await;
    let (path, first) = checkout(&h);
    stub.reprogram(script(&Shown::open(&first)));
    let repo = repository(&h, &path).await;
    let id = the_request(&h).await;
    ask_review(&h, &id, &repo, 1, PIN).await;
    let session = idle_session(&h, &id).await;
    let report = |sha: &str| {
        as_session(
            &format!("/v1/pull-requests/{id}/report"),
            &session.id,
            json!({"reviewed_sha": sha}),
        )
    };

    let _: Value = h.json(report(&first), StatusCode::OK).await;
    let row = h.store.get_pull_request(&id).await.unwrap();
    assert_eq!(row.reviewed_sha.as_deref(), Some(first.as_str()));
    assert_eq!(h.attention(&session).await, None);

    // The same sha again raises nothing either.
    let _: Value = h.json(report(&first), StatusCode::OK).await;
    assert_eq!(
        h.attention(&session).await,
        None,
        "the same sha raises nothing"
    );

    let second = push(&path);
    let _: Value = h.json(report(&second), StatusCode::OK).await;
    assert_eq!(
        h.attention(&session).await,
        None,
        "a later sha raises nothing either"
    );
    h.error(report("not a sha"), StatusCode::BAD_REQUEST).await;
}

/// The review session of a request that ends, or that no longer asks for
/// my review, is killed and its worktree removed.
async fn the_review_ends_when_the_request_shows(shown: impl Fn(&str) -> Shown<'_>) {
    let stub = stub_forge_cli(json!([]));
    let h = harness().scheduler().forge_cli(&stub).await;
    let (path, head) = checkout(&h);
    let before = stub.fetches();
    stub.reprogram(script(&Shown::open(&head)));
    let repo = repository(&h, &path).await;
    let id = the_request(&h).await;
    ask_review(&h, &id, &repo, 1, PIN).await;
    h.state.forge_poll.set_mode(&repo, Mode::WakeOnly);
    let session = idle_session(&h, &id).await;
    // Wait for the repository's own first fetch round to finish before
    // reprogramming for the new state: the row, the evidence and the idle
    // session above all land mid-round, well before the round's own
    // trailing calls do.
    eventually(
        TIMEOUT,
        "the repository's first fetch to finish",
        async || stub.fetches() > before,
    )
    .await;
    let worktree = PathBuf::from(session.worktree_path.clone().unwrap());

    stub.reprogram(script(&shown(&head)));
    h.state.forge_poll.wake(&repo);
    eventually(
        TIMEOUT,
        "the session to end and its worktree to go",
        async || !h.session_status(&session).await.is_live() && !worktree.exists(),
    )
    .await;
    assert_eq!(
        sh(&path, "git branch --list fix"),
        "fix",
        "no branch is touched"
    );
}

#[tokio::test]
async fn a_merged_request_ends_its_review() {
    the_review_ends_when_the_request_shows(|head| Shown {
        state: "MERGED",
        ..Shown::open(head)
    })
    .await;
}

#[tokio::test]
async fn a_closed_request_ends_its_review() {
    the_review_ends_when_the_request_shows(|head| Shown {
        state: "CLOSED",
        ..Shown::open(head)
    })
    .await;
}

#[tokio::test]
async fn a_withdrawn_review_request_keeps_its_manually_asked_review() {
    let r = reviewed().await;
    r.stub.reprogram(script(&Shown {
        listed: false,
        timeline: &["review_requested", "review_request_removed"],
        ..Shown::open(&r.head)
    }));
    assert_review_survives_withdrawal(&r).await;
}

/// The forge's assignment and the user's explicit ask are independent.
/// Wait for the withdrawal to be read, then run its scheduler pass before
/// asserting that it kept the existing session and worktree.
async fn assert_review_survives_withdrawal(r: &Reviewed) {
    r.h.state.forge_poll.wake(&r.repo);
    eventually(TIMEOUT, "the review request to be withdrawn", async || {
        r.h.launcher
            .live
            .get(&r.id)
            .is_some_and(|live| !live.review_requested)
    })
    .await;
    r.h.flush_scheduler().await;
    assert!(r.h.session_status(&r.session).await.is_live());
    assert!(r.worktree.exists());
    assert!(
        r.h.store
            .get_pull_request(&r.id)
            .await
            .unwrap()
            .review_asked
    );
}

#[tokio::test]
async fn an_unassigned_dependabot_request_is_reviewed_until_stopped() {
    let stub = stub_forge_cli(json!([]));
    let h = harness().scheduler().forge_cli(&stub).await;
    let (path, head) = checkout(&h);
    let shown = || Shown {
        author: "dependabot[bot]",
        listed: false,
        timeline: &[],
        ..Shown::open(&head)
    };
    stub.reprogram(script(&shown()));
    let repo = repository(&h, &path).await;
    h.state.forge_poll.set_mode(&repo, Mode::WakeOnly);
    let route = format!("/v1/repositories/{repo}/pull-requests/1/ariadne-review");
    let asked: Value = h
        .json(
            crate::common::put_json(
                &route,
                json!({"asked": true, "model": PIN, "skills": ["code-review"]}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(
        asked["review_requested"], false,
        "asking does not invent a GitHub assignment"
    );
    let id = asked["id"].as_str().unwrap();
    let session = idle_session(&h, id).await;
    let launch = h.launch_file(&session.id).expect("a launch file");
    for skill in ["- pr-reviewer: ", "- code-review: "] {
        assert!(
            launch.system_prompt.contains(skill),
            "{skill}: {}",
            launch.system_prompt
        );
    }
    let worktree = PathBuf::from(session.worktree_path.clone().unwrap());
    eventually(
        TIMEOUT,
        "unassigned request details to be read",
        async || {
            h.launcher
                .live
                .get(id)
                .is_some_and(|live| live.evidence_ok && !live.review_requested)
        },
    )
    .await;
    h.flush_scheduler().await;
    assert!(h.session_status(&session).await.is_live());
    assert!(worktree.exists());
    let _: Value = h
        .json(
            crate::common::put_json(&route, json!({"asked": false})),
            StatusCode::OK,
        )
        .await;
    eventually(
        TIMEOUT,
        "stopping to end the review and its worktree",
        async || {
            !h.session_status(&session).await.is_live()
                && !worktree.exists()
                && h.store.get_pull_request(id).await.is_err()
        },
    )
    .await;
}

/// A review session up on a request, with one review of it posted.
struct Reviewed {
    stub: StubForgeCli,
    h: Harness,
    head: String,
    repo: String,
    id: String,
    session: AgentSession,
    worktree: PathBuf,
}

async fn reviewed() -> Reviewed {
    let stub = stub_forge_cli(json!([]));
    let h = harness().scheduler().forge_cli(&stub).await;
    let (path, head) = checkout(&h);
    let before = stub.fetches();
    stub.reprogram(script(&Shown::open(&head)));
    let repo = repository(&h, &path).await;
    let id = the_request(&h).await;
    ask_review(&h, &id, &repo, 1, PIN).await;
    h.state.forge_poll.set_mode(&repo, Mode::WakeOnly);
    let session = idle_session(&h, &id).await;
    // Wait for the repository's own first fetch round to finish: every
    // caller of `reviewed()` reprograms the stub right away, and the row,
    // the evidence and the idle session above all land mid-round, well
    // before the round's own trailing calls do.
    eventually(
        TIMEOUT,
        "the repository's first fetch to finish",
        async || stub.fetches() > before,
    )
    .await;
    let worktree = PathBuf::from(session.worktree_path.clone().unwrap());
    let _: Vec<Value> = h
        .json(
            as_session(
                &format!("/v1/pull-requests/{id}/reviews"),
                &session.id,
                json!({"event": "comment", "body": "No findings."}),
            ),
            StatusCode::CREATED,
        )
        .await;
    Reviewed {
        stub,
        h,
        head,
        repo,
        id,
        session,
        worktree,
    }
}

/// A posted review does not restore the forge assignment; the manual
/// review remains active on its own ask.
#[tokio::test]
async fn a_review_posted_after_a_withdrawal_keeps_the_manual_review() {
    let r = reviewed().await;
    r.stub.reprogram(script(&Shown {
        listed: false,
        timeline: &["review_requested", "review_request_removed", "reviewed"],
        ..Shown::open(&r.head)
    }));
    assert_review_survives_withdrawal(&r).await;
}

/// GitHub stops listing a request as asking for my review once I reviewed
/// it, and writes no removal for that. The timeline still ends on the
/// request, so the session and its worktree stay.
#[tokio::test]
async fn a_request_the_forge_stops_listing_after_my_review_keeps_its_review() {
    let r = reviewed().await;
    r.stub.reprogram(script(&Shown {
        listed: false,
        timeline: &["review_requested", "reviewed"],
        ..Shown::open(&r.head)
    }));
    fetch_again(&r.h, &r.stub, &r.repo).await;
    assert!(
        r.h.launcher
            .live
            .get(&r.id)
            .is_some_and(|live| live.review_requested),
        "the timeline still asks"
    );
    assert!(r.h.session_status(&r.session).await.is_live());
    assert!(r.worktree.exists());
}

/// A later assignment and withdrawal leave the explicit manual review
/// active on its existing session and worktree.
#[tokio::test]
async fn a_request_withdrawn_after_my_review_keeps_its_manual_review() {
    let r = reviewed().await;
    r.stub.reprogram(script(&Shown {
        listed: false,
        timeline: &[
            "review_requested",
            "reviewed",
            "review_requested",
            "review_request_removed",
        ],
        ..Shown::open(&r.head)
    }));
    assert_review_survives_withdrawal(&r).await;
}

/// A request of mine gets no review session until the user asks Ariadne for
/// one, on the model they pick (029). Then it gets one on that pin — not the
/// repository's — detached at its head, whose review is posted as a comment
/// whatever it found: no forge takes a change request from a request's own
/// author. Asking no more takes the session down.
#[tokio::test]
async fn a_request_of_mine_is_reviewed_once_asked_and_its_review_is_a_comment() {
    let stub = stub_forge_cli(json!([]));
    let h = harness().scheduler().forge_cli(&stub).await;
    let (path, head) = checkout(&h);
    stub.reprogram(script(&Shown {
        author: "me",
        ..Shown::open(&head)
    }));
    // Every review runs on the pin asked directly: a request of mine is
    // no different.
    let repo = repository(&h, &path).await;
    let mut rows: Vec<Value> = Vec::new();
    eventually(TIMEOUT, "the request to be listed", async || {
        rows = h.get("/v1/pull-requests").await;
        rows.len() == 1
    })
    .await;
    assert_eq!(rows[0]["role"], "author");
    assert_eq!(rows[0]["review_asked"], false);
    assert!(rows[0]["id"].is_null(), "nobody works on it yet");
    h.state.forge_poll.set_mode(&repo, Mode::WakeOnly);
    h.flush_scheduler().await;

    let ask = |asked: bool| {
        axum::http::Request::builder()
            .method("PUT")
            .uri(format!(
                "/v1/repositories/{repo}/pull-requests/1/ariadne-review"
            ))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"asked": asked, "model": asked.then_some(PIN),
                    "skills": ["pr-reviewer", "code-review"]})
                .to_string(),
            ))
            .unwrap()
    };
    let asked: Value = h.json(ask(true), StatusCode::OK).await;
    assert_eq!(asked["review_asked"], true);
    assert_eq!(asked["review_model"], PIN);
    // Asking starts Ariadne's work on it: the request has a row now.
    let id = asked["id"].as_str().expect("a row").to_string();
    let session = idle_session(&h, &id).await;
    assert_eq!(session.seat.as_deref(), Some("reviewer"));
    assert_eq!(
        session.model, PIN,
        "the review runs on the pin the user picked"
    );
    assert_eq!(asked["review_skills"], json!(["code-review"]));
    let launch = h.launch_file(&session.id).expect("a launch file");
    for skill in ["- pr-reviewer: ", "- code-review: "] {
        assert!(
            launch.system_prompt.contains(skill),
            "{skill}: {}",
            launch.system_prompt
        );
    }
    let worktree = PathBuf::from(session.worktree_path.clone().unwrap());
    assert_eq!(sh(&worktree, "git rev-parse HEAD"), head);

    let _: Vec<Value> = h
        .json(
            as_session(
                &format!("/v1/pull-requests/{id}/reviews"),
                &session.id,
                json!({"event": "request_changes", "comments": [
                    {"path": "a.txt", "line": 1, "title": "Empty list",
                     "body": "An empty list panics.", "priority": "P0"}
                ]}),
            ),
            StatusCode::CREATED,
        )
        .await;
    let posts = review_posts(&stub);
    assert_eq!(posts.len(), 1, "{posts:?}");
    assert!(
        review_bodies(&stub)[0].1["event"] == "COMMENT",
        "{:?}",
        posts[0]
    );

    let _: Value = h.json(ask(false), StatusCode::OK).await;
    eventually(TIMEOUT, "the review to end", async || {
        !h.session_status(&session).await.is_live()
    })
    .await;
    // Nobody keeps it and nobody reviews it: Ariadne stops working on it,
    // and the session stays, let go of it.
    eventually(TIMEOUT, "the row to go", async || {
        h.flush_scheduler().await;
        h.store.get_pull_request(&id).await.is_err()
    })
    .await;
    assert_eq!(
        h.store
            .get_session(&session.id)
            .await
            .unwrap()
            .pull_request_id,
        None
    );

    // Asked again on another model, the review is a fresh session on it.
    let again = axum::http::Request::builder()
        .method("PUT")
        .uri(format!(
            "/v1/repositories/{repo}/pull-requests/1/ariadne-review"
        ))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"asked": true, "model": "stub:new-model"}).to_string(),
        ))
        .unwrap();
    let again: Value = h.json(again, StatusCode::OK).await;
    let id = again["id"].as_str().expect("a row").to_string();
    let mut fresh = None;
    eventually(TIMEOUT, "a review on the new model", async || {
        fresh = sessions(&h, &id)
            .await
            .into_iter()
            .find(|s| s.status().is_live() && s.model == "stub:new-model");
        fresh.is_some()
    })
    .await;
    let fresh = fresh.unwrap();
    assert_ne!(fresh.id, session.id, "a fresh session");
}

/// Asking needs a model the catalog holds. A request that asks for my
/// review takes the same asking a request of mine does (029): nothing
/// else will ever start a session for it.
#[tokio::test]
async fn asking_needs_a_model_and_takes_a_review_request_of_anothers() {
    let stub = stub_forge_cli(json!([]));
    let h = harness().scheduler().forge_cli(&stub).await;
    let (path, head) = checkout(&h);
    let before = stub.fetches();
    stub.reprogram(script(&Shown {
        author: "me",
        ..Shown::open(&head)
    }));
    let repo = repository(&h, &path).await;
    let ask = |body: Value| {
        axum::http::Request::builder()
            .method("PUT")
            .uri(format!(
                "/v1/repositories/{repo}/pull-requests/1/ariadne-review"
            ))
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    };
    h.error(ask(json!({"asked": true})), StatusCode::BAD_REQUEST)
        .await;
    h.error(
        ask(json!({"asked": true, "model": "nosuch:model"})),
        StatusCode::BAD_REQUEST,
    )
    .await;
    h.error(
        ask(json!({"asked": true, "model": PIN, "skills": ["orchestration"]})),
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
    // Wait for the repository's own first fetch round to finish: this test
    // sets no `WakeOnly` lock, so only proving a round actually over keeps
    // the reprogram below from racing a straggler still reading the
    // "me"-authored script above.
    eventually(
        TIMEOUT,
        "the repository's first fetch to finish",
        async || stub.fetches() > before,
    )
    .await;

    // A request that asks for my review takes a manual ask the same way a
    // request of mine does: nobody else is going to start it (029).
    let mut theirs = script(&Shown::open(&head));
    theirs.as_array_mut().unwrap().insert(
        0,
        answer(
            &["pr", "view"],
            0,
            &json!({"number": 1, "url": "https://github.com/acme/widgets/pull/1",
            "title": "Theirs", "author": {"login": "other"}, "state": "OPEN", "isDraft": false,
            "headRefName": "fix", "headRefOid": head, "baseRefName": "main",
            "statusCheckRollup": [], "reviewDecision": "", "createdAt": "2026-10-01T00:00:00Z"})
            .to_string(),
        ),
    );
    stub.reprogram(theirs);
    let dto: Value = h
        .json(ask(json!({"asked": true, "model": PIN})), StatusCode::OK)
        .await;
    let id = dto["id"].as_str().unwrap().to_string();
    assert_eq!(dto["role"], "reviewer");
    assert_eq!(dto["review_asked"], true);
    assert_eq!(dto["review_model"], PIN);

    // The scheduler runs well before the next forge refresh could ever
    // confirm the request still asks for my review: the row must survive
    // that gap, and a reviewer session must actually start on the pin just
    // chosen, not be read as a request that stopped asking and deleted
    // (`scheduler::pull_requests::end_review`) before the fetch ever ran.
    let session = idle_session(&h, &id).await;
    assert_eq!(session.model, PIN);
    assert_eq!(
        h.store.get_pull_request(&id).await.unwrap().id,
        id,
        "the row survives the scheduler pass"
    );
}

/// A detail read before any fetch says nothing of whether the request still
/// asks for my review: a request I review still asks until a fetch says
/// otherwise, so the review it has is not taken for withdrawn (029).
#[tokio::test]
async fn a_detail_read_before_any_fetch_keeps_a_review_request_asking() {
    let stub = stub_forge_cli(json!([]));
    let h = harness().scheduler().forge_cli(&stub).await;
    let (path, head) = checkout(&h);
    stub.reprogram(script(&Shown::open(&head)));
    let repo = repository(&h, &path).await;
    let id = the_request(&h).await;
    h.state.forge_poll.set_mode(&repo, Mode::WakeOnly);
    eventually(TIMEOUT, "the first fetch to finish", async || {
        stub.fetches() >= 1
    })
    .await;
    stub.reprogram(script(&Shown {
        listed: false,
        timeline: &["review_requested", "reviewed"],
        ..Shown::open(&head)
    }));
    h.launcher.live.remove(&id);
    let _: Value = h.get(&format!("/v1/pull-requests/{id}")).await;
    assert!(
        h.launcher.live.get(&id).is_some_and(|l| l.review_requested),
        "a detail read is no withdrawal"
    );
    h.state
        .sched_tx
        .as_ref()
        .unwrap()
        .send(ariadne_daemon::scheduler::SchedEvent::PullRequestChanged(
            id.clone(),
        ))
        .unwrap();
    h.flush_scheduler().await;
    let row = h.store.get_pull_request(&id).await.unwrap();
    assert!(!row.review_asked && !row.babysit_asked);
    assert!(sessions(&h, &id).await.is_empty());

    fetch_again(&h, &stub, &repo).await;
    assert!(stub.invocations().iter().any(|i| {
        i.args
            .starts_with(&["api".into(), "repos/acme/widgets/issues/1/timeline".into()])
    }));
    assert!(h.store.get_pull_request(&id).await.is_ok());
    assert!(
        h.launcher.live.get(&id).is_some_and(|l| l.review_requested),
        "the timeline still asks after my review"
    );

    stub.reprogram(script(&Shown {
        listed: false,
        timeline: &["review_requested", "reviewed", "review_request_removed"],
        ..Shown::open(&head)
    }));
    fetch_again(&h, &stub, &repo).await;
    assert!(no_rows(&h).await, "a confirmed withdrawal removes the row");
}
