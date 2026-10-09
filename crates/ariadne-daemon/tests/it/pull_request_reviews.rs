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
use ariadne_core::{AttentionReason, SessionStatus};
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
    let read = json!({"number": 1, "url": "https://github.com/acme/widgets/pull/1",
        "title": "Add widgets", "author": {"login": shown.author}, "state": shown.state,
        "isDraft": shown.draft, "headRefName": "fix", "headRefOid": shown.head,
        "headRepository": {"url": "https://github.com/acme/widgets"},
        "baseRefName": "main", "statusCheckRollup": [], "reviewDecision": "",
        "createdAt": "2026-10-01T00:00:00Z"});
    let listed = match shown.listed && shown.state == "OPEN" {
        true => json!([read]),
        false => json!([]),
    };
    let threads =
        json!({"data": {"repository": {"pullRequest": {"reviewThreads": {"nodes": []}}}}});
    let review_comments = json!([
        {"id": 501, "body": "**[P0] Empty list**\n\nAn empty list panics.", "path": "src/lib.rs", "line": 3,
         "created_at": "2026-10-02T00:00:00Z"},
        {"id": 502, "body": "**[P1] Untested**\n\nNo test covers the empty list.", "path": "tests/it.rs",
         "line": 9, "created_at": "2026-10-02T00:00:00Z"}
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
/// of `main`, `a.txt`. Answers the checkout and that commit.
fn checkout(h: &Harness) -> (PathBuf, String) {
    let path = h.git_repo("widgets");
    sh(
        &path,
        "git remote add origin https://github.com/acme/widgets.git && \
         git checkout -q -b fix && echo a > a.txt && git add a.txt && \
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

/// The repository registered with its integration on, and the pin a review
/// session runs on where `review` names one.
async fn repository(h: &Harness, path: &Path, review: Option<&str>) -> String {
    let mut forge = json!({"enabled": true});
    if let Some(model) = review {
        forge["review_model"] = json!(model);
    }
    let repo: Value = h
        .json(
            post_json("/v1/repositories", json!({"path": path, "forge": forge})),
            StatusCode::CREATED,
        )
        .await;
    repo["id"].as_str().unwrap().to_string()
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

fn get_as(uri: &str, session_id: &str) -> Request<Body> {
    Request::builder()
        .uri(uri)
        .header(SESSION_HEADER, session_id)
        .body(Body::empty())
        .unwrap()
}

/// The review posts the stub has seen.
fn review_posts(stub: &StubForgeCli) -> Vec<Vec<String>> {
    stub.invocations()
        .into_iter()
        .filter(|i| i.args.iter().any(|a| a == "POST"))
        .filter(|i| i.args.get(1).is_some_and(|path| path.ends_with("/reviews")))
        .map(|i| i.args)
        .collect()
}

/// The writes of the review's summary comment the stub has seen: the
/// method, `POST` or `PATCH`, and the body sent.
fn summary_writes(stub: &StubForgeCli) -> Vec<(String, String)> {
    stub.invocations()
        .into_iter()
        .filter(|i| {
            i.args.get(1).is_some_and(|path| {
                path == "repos/acme/widgets/issues/1/comments"
                    || path.starts_with("repos/acme/widgets/issues/comments/")
            })
        })
        .filter_map(|i| {
            let at = i.args.iter().position(|a| a == "--method")?;
            let method = i.args.get(at + 1)?.clone();
            let body = i
                .args
                .iter()
                .find_map(|a| a.strip_prefix("body="))?
                .to_string();
            Some((method, body))
        })
        .collect()
}

/// An open request out of draft that asks for my review, in a repository
/// with a `review_model`, gets one live session on that pin: seat
/// `reviewer`, a worktree detached at the head, and the `pr-reviewer`
/// document written and indexed.
#[tokio::test]
async fn an_open_request_i_review_gets_one_session_detached_at_its_head() {
    let stub = stub_forge_cli(json!([]));
    let h = harness().scheduler().forge_cli(&stub).await;
    let (path, head) = checkout(&h);
    stub.reprogram(script(&Shown::open(&head)));
    repository(&h, &path, Some(PIN)).await;
    let id = the_request(&h).await;
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
        briefing.contains("# Review pull request: Add widgets"),
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
    let repo = repository(&h, &path, Some(PIN)).await;
    h.state.forge_poll.set_mode(&repo, Mode::WakeOnly);
    h.state.forge_poll.wake(&repo);
    tokio::time::sleep(QUIET).await;
    h.flush_scheduler().await;
    assert!(no_rows(&h).await, "a draft is no work yet");

    stub.reprogram(script(&Shown::open(&head)));
    h.state.forge_poll.wake(&repo);
    let id = the_request(&h).await;
    idle_session(&h, &id).await;
}

/// A repository with no `review_model` starts nothing for a request I
/// review.
#[tokio::test]
async fn a_repository_with_no_review_model_starts_no_review() {
    let stub = stub_forge_cli(json!([]));
    let h = harness().scheduler().forge_cli(&stub).await;
    let (path, head) = checkout(&h);
    stub.reprogram(script(&Shown::open(&head)));
    let repo = repository(&h, &path, None).await;
    h.state.forge_poll.set_mode(&repo, Mode::WakeOnly);
    h.state.forge_poll.wake(&repo);
    tokio::time::sleep(QUIET).await;
    h.flush_scheduler().await;
    assert!(no_rows(&h).await, "nothing reviews it, so it is no work");
}

/// A push moves the worktree to the new head and hands the session one
/// prompt that names the last sha it reviewed. The same head again hands
/// nothing.
#[tokio::test]
async fn a_push_moves_the_worktree_and_is_told_once_with_the_last_reviewed_sha() {
    let stub = stub_forge_cli(json!([]));
    let h = harness().scheduler().forge_cli(&stub).await;
    let (path, first) = checkout(&h);
    stub.reprogram(script(&Shown::open(&first)));
    let repo = repository(&h, &path, Some(PIN)).await;
    let id = the_request(&h).await;
    h.state.forge_poll.set_mode(&repo, Mode::WakeOnly);
    let session = idle_session(&h, &id).await;
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
        news.contains(&format!("the head moved to {second}")),
        "{news}"
    );
    assert!(
        news.contains(&format!("The last sha you reviewed is {first}")),
        "{news}"
    );
    assert_eq!(sh(&worktree, "git rev-parse HEAD"), second);

    fetch_again(&h, &stub, &repo).await;
    assert_eq!(
        h.prompts_to(&session).len(),
        briefed + 1,
        "the same head is no news"
    );
}

/// News for a review settles before it is told (029): two pushes in quick
/// succession reach the agent as one prompt on the last head, once no new
/// activity came for `review_news_settle`, and each push restarts that wait.
#[tokio::test]
async fn a_burst_of_pushes_is_told_once_after_it_settles() {
    const SETTLE: std::time::Duration = std::time::Duration::from_secs(3);
    let stub = stub_forge_cli(json!([]));
    let h = harness()
        .scheduler()
        .forge_cli(&stub)
        .timeouts(ariadne_daemon::timeouts::Timeouts {
            review_news_settle: SETTLE,
            ..ariadne_daemon::timeouts::Timeouts::default()
        })
        .await;
    let (path, first) = checkout(&h);
    stub.reprogram(script(&Shown::open(&first)));
    let repo = repository(&h, &path, Some(PIN)).await;
    let id = the_request(&h).await;
    h.state.forge_poll.set_mode(&repo, Mode::WakeOnly);
    let session = idle_session(&h, &id).await;
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
    assert_eq!(
        h.prompts_to(&session).len(),
        briefed,
        "a push waits for the activity to settle"
    );
    let third = commit("third.txt");
    stub.reprogram(script(&Shown::open(&third)));
    fetch_again(&h, &stub, &repo).await;
    assert_eq!(
        h.prompts_to(&session).len(),
        briefed,
        "a new push waits again"
    );

    eventually(TIMEOUT, "the settled news to reach the agent", async || {
        h.prompts_to(&session).len() > briefed
    })
    .await;
    let news = h.prompts_to(&session).pop().unwrap();
    assert!(
        news.contains(&format!("the head moved to {third}")),
        "{news}"
    );
    assert_eq!(sh(&worktree, "git rev-parse HEAD"), third);
    tokio::time::sleep(SETTLE + QUIET).await;
    h.flush_scheduler().await;
    assert_eq!(
        h.prompts_to(&session).len(),
        briefed + 1,
        "the burst is one prompt"
    );
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
    repository(&h, &path, Some(PIN)).await;
    let id = the_request(&h).await;
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
    repository(&h, &path, Some(PIN)).await;
    let id = the_request(&h).await;
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

    // A change request with no P0 inline, and none open, is refused: the
    // findings are the comments, not a list in the summary.
    h.error(
        as_session(
            &reviews,
            &session.id,
            json!({"event": "request_changes", "body": "P0 on src/lib.rs:3: it panics."}),
        ),
        StatusCode::BAD_REQUEST,
    )
    .await;
    h.error(
        as_session(
            &reviews,
            &session.id,
            json!({"event": "comment", "body": "One finding.", "comments": [
                {"path": "src/lib.rs", "line": 3, "body": "No title.", "priority": "P2"}
            ]}),
        ),
        StatusCode::BAD_REQUEST,
    )
    .await;
    h.error(
        as_session(
            &reviews,
            &session.id,
            json!({"event": "comment", "body": " "}),
        ),
        StatusCode::BAD_REQUEST,
    )
    .await;
    assert!(
        review_posts(&stub).is_empty() && summary_writes(&stub).is_empty(),
        "a refused review reaches no forge"
    );

    let summary = "Reviewed main..abc. Changes requested: P0 open.";
    let stored: Vec<Value> = h
        .json(
            as_session(
                &reviews,
                &session.id,
                json!({"event": "request_changes", "body": summary, "comments": [
                    {"path": "src/lib.rs", "line": 3, "title": "Empty list",
                     "body": "An empty list panics.", "priority": "P0"},
                    {"path": "tests/it.rs", "line": 9, "title": "Untested",
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
    for field in [
        "event=REQUEST_CHANGES",
        &format!("commit_id={head}"),
        "comments[][path]=src/lib.rs",
        "comments[][line]=3",
        // Each finding is signed the review's, invisibly (029).
        "comments[][body]=**[P0] Empty list**\n\nAn empty list panics.\n\n<!-- ariadne:review -->",
        "comments[][path]=tests/it.rs",
        "comments[][line]=9",
        "comments[][body]=**[P1] Untested**\n\nNo test covers the empty list.\n\n<!-- ariadne:review -->",
    ] {
        assert!(call.iter().any(|a| a == field), "{field}: {call:?}");
    }
    assert!(
        !call.iter().any(|a| a.starts_with("body=")),
        "the review carries no summary of its own: {call:?}"
    );
    let signed =
        |text: &str| format!("{text}\n\n<!-- ariadne:review-summary -->\n<!-- ariadne:review -->");
    assert_eq!(
        summary_writes(&stub),
        [("POST".to_string(), signed(summary))],
        "the summary is one comment of its own"
    );
    let bodies: Vec<&str> = stored.iter().map(|c| c["body"].as_str().unwrap()).collect();
    assert_eq!(
        bodies,
        [
            "**[P0] Empty list**\n\nAn empty list panics.",
            "**[P1] Untested**\n\nNo test covers the empty list.",
            summary,
        ]
    );
    assert!(stored.iter().all(|c| c["author_login"] == "me"));
    // What is kept of them is the mark that the review posted them; their
    // text is the forge's.
    let marks = h.store.pull_request_comment_marks(&id).await.unwrap();
    assert_eq!(marks.len(), 3);
    assert!(marks.iter().all(|m| m.from_review));

    // A later round with no new finding posts no review: it edits the one
    // summary, and the P0 still open keeps the change request standing.
    let later = "Reviewed abc..def. Changes requested: P0 still open.";
    let _: Vec<Value> = h
        .json(
            as_session(
                &reviews,
                &session.id,
                json!({"event": "request_changes", "body": later}),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert_eq!(review_posts(&stub).len(), 1, "no second review");
    assert_eq!(
        summary_writes(&stub).last(),
        Some(&("PATCH".to_string(), signed(later)))
    );
    assert_eq!(
        h.store.pull_request_comment_marks(&id).await.unwrap().len(),
        3,
        "the summary is edited, not added"
    );
    let row = h.store.get_pull_request(&id).await.unwrap();
    assert_eq!(row.summary_comment_id.as_deref(), Some("ic-301"));
}

/// Once a push fixed a finding, the review session resolves the thread it
/// opened: through `resolveReviewThread` on the thread GitHub holds for the
/// comment, and the stored thread reads resolved. A thread somebody else
/// opened is refused, and so is a resolve from any other session.
#[tokio::test]
async fn a_review_resolves_the_thread_of_its_own_fixed_finding_and_no_other() {
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
    repository(&h, &path, Some(PIN)).await;
    let id = the_request(&h).await;
    let session = idle_session(&h, &id).await;

    let stored: Vec<Value> = h
        .json(
            as_session(
                &format!("/v1/pull-requests/{id}/reviews"),
                &session.id,
                json!({"event": "request_changes", "body": "One P0.", "comments": [
                    {"path": "src/lib.rs", "line": 3, "title": "Empty list",
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

/// A reported `reviewed_sha` is stored and raises `waiting_user` for my
/// approval. The same sha again raises nothing; a later sha raises it
/// again.
#[tokio::test]
async fn a_reviewed_sha_raises_waiting_user_and_a_later_one_raises_it_again() {
    let stub = stub_forge_cli(json!([]));
    let h = harness().scheduler().forge_cli(&stub).await;
    let (path, first) = checkout(&h);
    stub.reprogram(script(&Shown::open(&first)));
    repository(&h, &path, Some(PIN)).await;
    let id = the_request(&h).await;
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
    assert_eq!(
        h.attention(&session).await,
        Some(AttentionReason::WaitingUser)
    );

    // The user read it; the same sha again raises nothing.
    h.store.clear_session_attention(&session.id).await.unwrap();
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
        Some(AttentionReason::WaitingUser),
        "a later sha raises it again"
    );
    h.error(report("not a sha"), StatusCode::BAD_REQUEST).await;
}

/// The review session of a request that ends, or that no longer asks for
/// my review, is killed and its worktree removed.
async fn the_review_ends_when_the_request_shows(shown: impl Fn(&str) -> Shown<'_>) {
    let stub = stub_forge_cli(json!([]));
    let h = harness().scheduler().forge_cli(&stub).await;
    let (path, head) = checkout(&h);
    stub.reprogram(script(&Shown::open(&head)));
    let repo = repository(&h, &path, Some(PIN)).await;
    let id = the_request(&h).await;
    h.state.forge_poll.set_mode(&repo, Mode::WakeOnly);
    let session = idle_session(&h, &id).await;
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
async fn a_withdrawn_review_request_ends_its_review() {
    the_review_ends_when_the_request_shows(|head| Shown {
        listed: false,
        timeline: &["review_requested", "review_request_removed"],
        ..Shown::open(head)
    })
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
    stub.reprogram(script(&Shown::open(&head)));
    let repo = repository(&h, &path, Some(PIN)).await;
    let id = the_request(&h).await;
    h.state.forge_poll.set_mode(&repo, Mode::WakeOnly);
    let session = idle_session(&h, &id).await;
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

/// A review posted after a withdrawal does not ask for my review again: the
/// session ends and its worktree goes.
#[tokio::test]
async fn a_review_posted_after_a_withdrawal_does_not_keep_the_review() {
    let r = reviewed().await;
    r.stub.reprogram(script(&Shown {
        listed: false,
        timeline: &["review_requested", "review_request_removed", "reviewed"],
        ..Shown::open(&r.head)
    }));
    r.h.state.forge_poll.wake(&r.repo);
    eventually(
        TIMEOUT,
        "the session to end and its worktree to go",
        async || !r.h.session_status(&r.session).await.is_live() && !r.worktree.exists(),
    )
    .await;
    eventually(TIMEOUT, "Ariadne to stop working on it", async || {
        r.h.store.get_pull_request(&r.id).await.is_err()
    })
    .await;
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

/// A review request asked again after my review, then withdrawn, ends the
/// session and removes its worktree: a review of mine keeps nothing alive.
#[tokio::test]
async fn a_request_withdrawn_after_my_review_ends_its_review() {
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
    r.h.state.forge_poll.wake(&r.repo);
    eventually(
        TIMEOUT,
        "the session to end and its worktree to go",
        async || !r.h.session_status(&r.session).await.is_live() && !r.worktree.exists(),
    )
    .await;
    eventually(TIMEOUT, "Ariadne to stop working on it", async || {
        r.h.store.get_pull_request(&r.id).await.is_err()
    })
    .await;
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
    // No repository review pin: a request of mine runs on the one asked.
    let repo = repository(&h, &path, None).await;
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
                json!({"event": "request_changes", "body": "One finding.", "comments": [
                    {"path": "src/lib.rs", "line": 3, "title": "Empty list",
                     "body": "An empty list panics.", "priority": "P0"}
                ]}),
            ),
            StatusCode::CREATED,
        )
        .await;
    let posts = review_posts(&stub);
    assert_eq!(posts.len(), 1, "{posts:?}");
    assert!(
        posts[0].iter().any(|a| a == "event=COMMENT"),
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

    // The row of the first review went, and the summary it posted is found
    // again on the forge by its mark: the next round edits it rather than
    // post a second.
    let mut holds = script(&Shown {
        author: "me",
        ..Shown::open(&head)
    });
    entry(&mut holds, &["api", "repos/acme/widgets/issues/1/comments"])["stdout"] = json!(
        json!([{"id": 301, "user": {"login": "me", "type": "User"},
            "body": "One finding.\n\n<!-- ariadne:review-summary -->\n<!-- ariadne:review -->",
            "created_at": "2026-10-02T00:00:00Z"}])
        .to_string()
    );
    stub.reprogram(holds);
    let _: Value = h.get(&format!("/v1/pull-requests/{id}")).await;
    let _: Vec<Value> = h
        .json(
            as_session(
                &format!("/v1/pull-requests/{id}/reviews"),
                &fresh.id,
                json!({"event": "comment", "body": "No findings."}),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert_eq!(
        summary_writes(&stub).last(),
        Some(&(
            "PATCH".to_string(),
            "No findings.\n\n<!-- ariadne:review-summary -->\n<!-- ariadne:review -->".to_string()
        ))
    );
}

/// Asking needs a model the catalog holds, and a request that asks for my
/// review takes no asking: it has a review session of its own already.
#[tokio::test]
async fn asking_needs_a_model_and_a_request_of_mine() {
    let stub = stub_forge_cli(json!([]));
    let h = harness().scheduler().forge_cli(&stub).await;
    let (path, head) = checkout(&h);
    stub.reprogram(script(&Shown {
        author: "me",
        ..Shown::open(&head)
    }));
    let repo = repository(&h, &path, None).await;
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

    // A request that asks for my review is reviewed on the repository's
    // pin, and takes no asking.
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
    h.error(
        ask(json!({"asked": true, "model": PIN})),
        StatusCode::CONFLICT,
    )
    .await;
}

/// On GitLab a finding on a renamed file is placed with the file's path on
/// each side of the diff: its old path, read off the merge request's diffs
/// before anything is posted, and the new one the finding names.
#[tokio::test]
async fn a_gitlab_finding_on_a_renamed_file_names_its_old_path() {
    use ariadne_core::ForgeKind;
    use ariadne_daemon::forge::ForgeClient;
    use ariadne_daemon::forge::pulls::{DraftComment, ReviewDraft};

    let base = "projects/team%2Fwidgets/merge_requests/7";
    let diffs = format!("{base}/diffs?per_page=100");
    let discussions = format!("{base}/discussions");
    let notes = format!("{base}/notes");
    let stub = stub_forge_cli(json!([
        {"program": "glab", "args": ["api", diffs], "stdout": json!([
            {"old_path": "src/old.rs", "new_path": "src/new.rs"},
            {"old_path": "src/same.rs", "new_path": "src/same.rs"}
        ]).to_string()},
        {"program": "glab", "args": ["api", discussions], "stdout": json!(
            {"id": "d1", "notes": [{"id": 9, "created_at": "2026-10-02T00:00:00Z"}]}
        ).to_string()},
        {"program": "glab", "args": ["api", notes], "stdout": json!(
            {"id": 10, "created_at": "2026-10-02T00:00:00Z"}
        ).to_string()},
        {"program": "glab", "args": ["api", base], "stdout": json!({"diff_refs":
            {"base_sha": "b", "start_sha": "s", "head_sha": "h"}}).to_string()},
    ]));
    let h = harness().forge_cli(&stub).await;
    let finding = |path: &str| DraftComment {
        path: path.into(),
        line: 3,
        body: "**[P1] Renamed**\n\nIt breaks.".into(),
    };
    ForgeClient::new(&h.launcher.cfg, ForgeKind::Gitlab)
        .submit_review(
            "gitlab.com/team/widgets",
            7,
            &ReviewDraft {
                request_changes: false,
                head_sha: "h".into(),
                comments: vec![finding("src/new.rs"), finding("src/same.rs")],
            },
            "maria",
        )
        .await
        .expect("the review is posted");
    let posted: Vec<Vec<String>> = stub
        .invocations()
        .into_iter()
        .filter(|call| call.args.get(1) == Some(&discussions))
        .map(|call| call.args)
        .collect();
    assert_eq!(posted.len(), 2, "{posted:?}");
    for (call, old, new) in [
        (&posted[0], "src/old.rs", "src/new.rs"),
        (&posted[1], "src/same.rs", "src/same.rs"),
    ] {
        assert!(
            call.contains(&format!("position[old_path]={old}")),
            "{call:?}"
        );
        assert!(
            call.contains(&format!("position[new_path]={new}")),
            "{call:?}"
        );
    }

    // The summary is one note: posted once, then edited in place.
    let client = ForgeClient::new(&h.launcher.cfg, ForgeKind::Gitlab);
    let (first, _) = client
        .write_summary("gitlab.com/team/widgets", 7, None, "Reviewed a..b.")
        .await
        .expect("the summary is posted");
    assert_eq!(first, "note-10");
    stub.reprogram(json!([
        {"program": "glab", "args": ["api", format!("{notes}/10")], "stdout": json!(
            {"id": 10, "created_at": "2026-10-02T00:00:00Z"}
        ).to_string()},
    ]));
    let (again, _) = client
        .write_summary("gitlab.com/team/widgets", 7, Some(&first), "Reviewed b..c.")
        .await
        .expect("the summary is edited");
    assert_eq!(again, first);
    let edit = stub
        .invocations()
        .into_iter()
        .find(|i| i.args.get(1) == Some(&format!("{notes}/10")))
        .expect("the edit");
    assert!(edit.args.contains(&"PUT".to_string()), "{edit:?}");
    assert!(
        edit.args.contains(&"body=Reviewed b..c.".to_string()),
        "{edit:?}"
    );
}

/// The answer of `script` for `args`, as the stub matches it.
fn entry<'a>(script: &'a mut Value, args: &[&str]) -> &'a mut Value {
    script
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|e| e["args"] == json!(args))
        .expect("the entry")
}

/// A round whose summary fails keeps the findings it posted (029): they are
/// marked the review's before the summary is written, the error says so,
/// and a round with no comments writes the summary without posting them
/// again.
#[tokio::test]
async fn a_failed_summary_keeps_the_posted_findings_for_a_round_with_no_comments() {
    let stub = stub_forge_cli(json!([]));
    let h = harness().scheduler().forge_cli(&stub).await;
    let (path, head) = checkout(&h);
    let summary_post = [
        "api",
        "repos/acme/widgets/issues/1/comments",
        "--hostname",
        "github.com",
        "--method",
        "POST",
    ];
    let mut failing = script(&Shown::open(&head));
    entry(&mut failing, &summary_post)["exit"] = json!(1);
    entry(&mut failing, &summary_post)["stderr"] = json!("gh: Server Error (HTTP 502)");
    stub.reprogram(failing);
    repository(&h, &path, Some(PIN)).await;
    let id = the_request(&h).await;
    let session = idle_session(&h, &id).await;
    let reviews = format!("/v1/pull-requests/{id}/reviews");
    let refused = h
        .error(
            as_session(
                &reviews,
                &session.id,
                json!({"event": "comment", "body": "One P1.", "comments": [
                    {"path": "src/lib.rs", "line": 3, "title": "Empty list",
                     "body": "An empty list panics.", "priority": "P1"}
                ]}),
            ),
            StatusCode::BAD_GATEWAY,
        )
        .await;
    assert!(
        refused.error.message.contains("findings are posted"),
        "{}",
        refused.error.message
    );
    let marks = h.store.pull_request_comment_marks(&id).await.unwrap();
    assert!(
        marks
            .iter()
            .any(|m| m.forge_id == "rc-501" && m.from_review),
        "{marks:?}"
    );
    assert_eq!(review_posts(&stub).len(), 1);

    stub.reprogram(script(&Shown::open(&head)));
    let _: Vec<Value> = h
        .json(
            as_session(
                &reviews,
                &session.id,
                json!({"event": "comment", "body": "One P1."}),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert_eq!(
        review_posts(&stub).len(),
        1,
        "the findings are not posted again"
    );
    assert_eq!(
        summary_writes(&stub)
            .last()
            .map(|(method, _)| method.as_str()),
        Some("POST")
    );
}

/// A summary edit that fails is the error, and posts no second summary: only
/// a summary GitHub answers 404 for is posted again (029).
#[tokio::test]
async fn a_summary_is_posted_again_only_where_github_says_it_is_gone() {
    use ariadne_core::ForgeKind;
    use ariadne_daemon::forge::ForgeClient;
    let edit = ["api", "repos/acme/widgets/issues/comments/301"];
    let post = json!({"id": 302, "created_at": "2026-10-02T00:00:00Z"}).to_string();
    let stub = stub_forge_cli(json!([
        {"args": edit, "exit": 1, "stderr": "gh: Server Error (HTTP 502)"},
        {"args": ["api", "repos/acme/widgets/issues/1/comments"], "stdout": post},
    ]));
    let h = harness().forge_cli(&stub).await;
    let client = ForgeClient::new(&h.launcher.cfg, ForgeKind::Github);
    let posts = || {
        stub.invocations()
            .iter()
            .filter(|i| {
                i.args
                    .get(1)
                    .is_some_and(|a| a == "repos/acme/widgets/issues/1/comments")
            })
            .count()
    };
    assert!(
        client
            .write_summary(
                "github.com/acme/widgets",
                1,
                Some("ic-301"),
                "- P1: Handle HTTP 404 responses (404 Not Found)"
            )
            .await
            .is_err()
    );
    assert_eq!(posts(), 0, "a failed edit posts no second summary");

    stub.reprogram(json!([
        {"args": edit, "exit": 1, "stderr": "gh: Not Found (HTTP 404)"},
        {"args": ["api", "repos/acme/widgets/issues/1/comments"], "stdout": post},
    ]));
    let (id, _) = client
        .write_summary(
            "github.com/acme/widgets",
            1,
            Some("ic-301"),
            "Reviewed a..b.",
        )
        .await
        .expect("the summary is posted again");
    assert_eq!(id, "ic-302");
    assert_eq!(posts(), 1);
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
    let repo = repository(&h, &path, Some(PIN)).await;
    let id = the_request(&h).await;
    h.state.forge_poll.set_mode(&repo, Mode::WakeOnly);
    h.launcher.live.remove(&id);
    let _: Value = h.get(&format!("/v1/pull-requests/{id}")).await;
    assert!(
        h.launcher.live.get(&id).is_some_and(|l| l.review_requested),
        "a detail read is no withdrawal"
    );
}
