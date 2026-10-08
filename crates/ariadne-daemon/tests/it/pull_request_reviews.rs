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
    let mut rows: Vec<Value> = Vec::new();
    eventually(TIMEOUT, "the request to be recorded", async || {
        rows = h.get("/v1/pull-requests?state=all").await;
        rows.len() == 1
    })
    .await;
    assert_eq!(rows[0]["role"], "reviewer");
    rows[0]["id"].as_str().unwrap().to_string()
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
        .map(|i| i.args)
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
    let id = the_request(&h).await;
    h.state.forge_poll.set_mode(&repo, Mode::WakeOnly);
    tokio::time::sleep(QUIET).await;
    h.flush_scheduler().await;
    assert!(
        sessions(&h, &id).await.is_empty(),
        "a draft gets no session"
    );

    stub.reprogram(script(&Shown::open(&head)));
    h.state.forge_poll.wake(&repo);
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
    let id = the_request(&h).await;
    h.state.forge_poll.set_mode(&repo, Mode::WakeOnly);
    h.state.forge_poll.wake(&repo);
    tokio::time::sleep(QUIET).await;
    h.flush_scheduler().await;
    assert!(sessions(&h, &id).await.is_empty());
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

    let stored: Vec<Value> = h
        .json(
            as_session(
                &reviews,
                &session.id,
                json!({"event": "request_changes", "body": "Two findings.", "comments": [
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
    // A change request with no P0 inline is refused: the findings are the
    // comments, not a list in the summary.
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
    assert_eq!(
        review_posts(&stub).len(),
        1,
        "a refused review reaches no forge"
    );
    assert_eq!(call[1], "repos/acme/widgets/pulls/1/reviews");
    for field in [
        "event=REQUEST_CHANGES",
        "body=Two findings.",
        &format!("commit_id={head}"),
        "comments[][path]=src/lib.rs",
        "comments[][line]=3",
        "comments[][body]=**[P0] Empty list**\n\nAn empty list panics.",
        "comments[][path]=tests/it.rs",
        "comments[][line]=9",
        "comments[][body]=**[P1] Untested**\n\nNo test covers the empty list.",
    ] {
        assert!(call.iter().any(|a| a == field), "{field}: {call:?}");
    }
    let bodies: Vec<&str> = stored.iter().map(|c| c["body"].as_str().unwrap()).collect();
    assert_eq!(
        bodies,
        [
            "Two findings.",
            "**[P0] Empty list**\n\nAn empty list panics.",
            "**[P1] Untested**\n\nNo test covers the empty list."
        ]
    );
    assert!(stored.iter().all(|c| c["author_login"] == "me"));
    let comments: Vec<Value> = h.get(&format!("/v1/pull-requests/{id}/comments")).await;
    assert_eq!(comments.len(), 3, "the review is stored");
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
        .find(|c| c["forge_id"] == "rc-501")
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
        .find(|c| c["forge_id"] == "rc-601")
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
    assert!(
        !r.h.store
            .get_pull_request(&r.id)
            .await
            .unwrap()
            .review_requested
    );
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
        r.h.store
            .get_pull_request(&r.id)
            .await
            .unwrap()
            .review_requested
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
    assert!(
        !r.h.store
            .get_pull_request(&r.id)
            .await
            .unwrap()
            .review_requested
    );
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
    eventually(TIMEOUT, "the request to be recorded", async || {
        rows = h.get("/v1/pull-requests?state=all").await;
        rows.len() == 1
    })
    .await;
    assert_eq!(rows[0]["role"], "author");
    assert_eq!(rows[0]["review_asked"], false);
    let id = rows[0]["id"].as_str().unwrap().to_string();
    h.state.forge_poll.set_mode(&repo, Mode::WakeOnly);
    h.flush_scheduler().await;
    assert!(
        sessions(&h, &id).await.is_empty(),
        "nobody asked for a review"
    );

    let ask = |asked: bool| {
        axum::http::Request::builder()
            .method("PUT")
            .uri(format!("/v1/pull-requests/{id}/ariadne-review"))
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
        sessions(&h, &id)
            .await
            .iter()
            .all(|s| !s.status().is_live())
    })
    .await;

    // Asked again on another model, the review is a fresh session on it:
    // the last conversation keeps the model it started on.
    let again = axum::http::Request::builder()
        .method("PUT")
        .uri(format!("/v1/pull-requests/{id}/ariadne-review"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"asked": true, "model": "stub:new-model"}).to_string(),
        ))
        .unwrap();
    let _: Value = h.json(again, StatusCode::OK).await;
    let mut fresh = None;
    eventually(TIMEOUT, "a review on the new model", async || {
        fresh = sessions(&h, &id)
            .await
            .into_iter()
            .find(|s| s.status().is_live() && s.model == "stub:new-model");
        fresh.is_some()
    })
    .await;
    assert_ne!(fresh.unwrap().id, session.id, "a fresh session");
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
    repository(&h, &path, None).await;
    let mut rows: Vec<Value> = Vec::new();
    eventually(TIMEOUT, "the request to be recorded", async || {
        rows = h.get("/v1/pull-requests?state=all").await;
        rows.len() == 1
    })
    .await;
    let id = rows[0]["id"].as_str().unwrap().to_string();
    let ask = |body: Value| {
        axum::http::Request::builder()
            .method("PUT")
            .uri(format!("/v1/pull-requests/{id}/ariadne-review"))
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
    let row: Value = h.get(&format!("/v1/pull-requests/{id}")).await;
    assert_eq!(row["review_asked"], false, "a refusal asks nothing");

    let reviewing = h
        .store
        .upsert_pull_request(ariadne_store::NewPullRequest {
            existing_id: None,
            repository_id: rows[0]["repository_id"].as_str().unwrap().into(),
            number: 2,
            url: "https://github.com/acme/widgets/pull/2".into(),
            title: "Theirs".into(),
            body: String::new(),
            author_login: "other".into(),
            tracked_by: "forge".into(),
            state: "open".into(),
            draft: false,
            head_branch: "theirs".into(),
            head_sha: head.clone(),
            head_repo: None,
            base_branch: "main".into(),
            checks: "none".into(),
            review_decision: "none".into(),
            origin_task_id: None,
            opened_at: "2026-10-01T00:00:00Z".into(),
        })
        .await
        .unwrap()
        .0;
    let ask = axum::http::Request::builder()
        .method("PUT")
        .uri(format!("/v1/pull-requests/{}/ariadne-review", reviewing.id))
        .header("content-type", "application/json")
        .body(Body::from(json!({"asked": true, "model": PIN}).to_string()))
        .unwrap();
    h.error(ask, StatusCode::CONFLICT).await;
}
