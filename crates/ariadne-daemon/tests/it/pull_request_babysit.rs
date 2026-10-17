//! An open request that no task opened gets a keeper session
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
    Harness, QUIET, RUNS_OUT, TIMEOUT, as_session, eventually, harness, next_event, put_json, sh,
    with_forge,
};
use ariadne_core::{Seat, SessionStatus};
use ariadne_daemon::forge::poll::Mode;
use ariadne_daemon::scheduler::SPAWN_RETRY_BUDGET;
use ariadne_daemon::timeouts::Timeouts;
use ariadne_store::{AgentSession, SessionFilter};

const PIN: &str = "stub:babysit-model";

/// How the stub forge shows one request: its state, the head
/// repository it lives in, and the comments on its conversation, oldest
/// first.
struct Shown<'a> {
    author: &'a str,
    state: &'a str,
    head: &'a str,
    updated_at: &'a str,
    head_repo: &'a str,
    comments: Value,
    checks: &'a str,
    review_decision: &'a str,
    mergeable: &'a str,
}

impl Shown<'_> {
    fn open(head: &str) -> Shown<'_> {
        Shown {
            author: "me",
            state: "OPEN",
            head,
            updated_at: "2026-10-01T00:00:00Z",
            head_repo: "https://github.com/acme/widgets",
            comments: json!([]),
            checks: "NONE",
            review_decision: "",
            mergeable: "UNKNOWN",
        }
    }
}

/// What the stub forge answers for one request, read for the
/// scheduler's own detail fetch.
fn script(shown: &Shown) -> Value {
    let read = json!({"number": 1, "url": "https://github.com/acme/widgets/pull/1",
        "title": "Add widgets", "author": {"login": shown.author}, "state": shown.state,
        "isDraft": false, "headRefName": "fix", "headRefOid": shown.head,
        "headRepository": {"url": shown.head_repo},
        "baseRefName": "main", "reviewDecision": shown.review_decision,
        "mergeStateStatus": shown.mergeable,
        "createdAt": "2026-10-01T00:00:00Z", "updatedAt": shown.updated_at});
    let listed = match shown.state == "OPEN" {
        true => json!([read]),
        false => json!([]),
    };
    let threads = json!({"data": {"repository": {"pullRequest": {"reviewThreads": {"nodes": []}},
            "pr1": {"statusCheckRollup": {"state": shown.checks}}}}});
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
            &["pr", "comment"],
            0,
            "https://github.com/acme/widgets/pull/1#issuecomment-301"
        ),
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

/// Fetch the repository once more, and wait for the resulting pass to end,
/// so a test that proves "no news the second time" first proves the
/// second fetch actually ran.
async fn fetch_again(h: &Harness, stub: &StubForgeCli, repo: &str) {
    let before = stub.fetches();
    h.state.forge_poll.wake(repo);
    eventually(TIMEOUT, "the fetch", async || stub.fetches() > before).await;
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
    keep_until_merge("me").await;
}

#[tokio::test]
async fn asking_keeps_a_dependabot_request_until_the_merge() {
    keep_until_merge("dependabot[bot]").await;
}

#[tokio::test]
async fn asking_keeps_another_authors_request_until_the_merge() {
    keep_until_merge("alice").await;
}

async fn keep_until_merge(author: &str) {
    let stub = stub_forge_cli(json!([]));
    let h = harness().scheduler().forge_cli(&stub).await;
    let (path, head) = checkout(&h);
    let shown = || Shown {
        author,
        ..Shown::open(&head)
    };
    stub.reprogram(script(&shown()));
    let repo = repository(&h, &path).await;
    // The repository's own worker starts a fetch of its own the moment it
    // is created: waited out before the script changes below, since one
    // still under way would answer its own remaining calls from the new
    // script instead of this one.
    eventually(TIMEOUT, "the first fetch to end", async || {
        stub.fetches() >= 1
    })
    .await;
    // Not about the supervised path (026 rule 24 covers that separately):
    // the reply below posts directly, as it did before that rule existed.
    let repository_row = h.store.get_repository(&repo).await.unwrap();
    h.set_supervise_posts(&repository_row, false).await;
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

    let before = stub.fetches();
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
    assert_eq!(
        h.store.get_pull_request(&id).await.unwrap().role,
        if author == "me" { "author" } else { "reviewer" }
    );

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
    // The ask itself wakes a round of its own too
    // (`http::pull_requests`'s babysit handler): wait for it to finish
    // before the script changes below.
    eventually(TIMEOUT, "the ask's own fetch to finish", async || {
        stub.fetches() > before
    })
    .await;

    // A comment by another login is the author's news, told once.
    stub.reprogram(script(&Shown {
        updated_at: "2026-10-02T00:00:00Z",
        checks: "FAILURE",
        comments: json!([
            {"id": 301, "user": {"login": "alice"}, "body": "Looks good but one nit.",
             "created_at": "2026-10-02T00:00:00Z"}
        ]),
        ..shown()
    }));
    h.state.forge_poll.wake(&repo);
    eventually(TIMEOUT, "the news to reach the keeper", async || {
        h.prompts_to(&session)
            .last()
            .is_some_and(|news| news.contains("ic-301"))
    })
    .await;
    let news = h.prompts_to(&session).pop().unwrap();
    assert!(news.contains("alice"), "{news}");
    eventually(TIMEOUT, "failed checks to reach the keeper", async || {
        h.prompts_to(&session)
            .iter()
            .any(|news| news.contains("checks now read") && news.contains("failure"))
    })
    .await;

    let told = h.prompts_to(&session).len();

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
    // defect, and a comment of the other side in the conversation too.
    let before = stub.fetches();
    let reply: Value = h
        .json(
            as_session(
                &format!("/v1/pull-requests/{id}/comments/ic-301/reply"),
                &session.id,
                json!({"body": "Thanks!"}),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert_eq!(reply["body"], "Thanks!");
    // The reply itself wakes a round of its own too
    // (`http::pull_requests`'s comment-reply handler): wait for it to
    // finish before the script changes below.
    eventually(TIMEOUT, "the reply's own fetch to finish", async || {
        stub.fetches() > before
    })
    .await;

    // The keeper reports the request ready, on the head it read. `report`
    // wakes no fetch of its own, so no further wait is needed before the
    // merge below.
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

    // Forge evidence backs the report, regardless of who authored the PR.
    stub.reprogram(script(&Shown {
        updated_at: "2026-10-03T00:00:00Z",
        checks: "SUCCESS",
        review_decision: "APPROVED",
        mergeable: "CLEAN",
        ..shown()
    }));
    fetch_again(&h, &stub, &repo).await;
    eventually(TIMEOUT, "readiness to reach attention", async || {
        let list: ariadne_api::attention::AttentionListDto = h.get("/v1/attention").await;
        list.items
            .iter()
            .any(|item| item.id == format!("pull_request:ready:{id}"))
    })
    .await;

    // The merge takes the keeper session, its worktree and its branch
    // down: nothing keeps the row any more, since no task opened it and
    // no review was ever asked of it.
    stub.reprogram(script(&Shown {
        state: "MERGED",
        ..shown()
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

/// Each seat follows its own ask on a PR by another author. Ending one
/// must preserve the other's session, worktree and request row.
#[tokio::test]
async fn a_dependabot_request_can_be_reviewed_and_babysat_independently() {
    let stub = stub_forge_cli(json!([]));
    let h = harness().scheduler().forge_cli(&stub).await;
    let (path, head) = checkout(&h);
    stub.reprogram(script(&Shown {
        author: "dependabot[bot]",
        ..Shown::open(&head)
    }));
    let repo = repository(&h, &path).await;
    h.state.forge_poll.set_mode(&repo, Mode::WakeOnly);
    let asked: Value = h
        .json(
            babysit(&repo, 1, json!({"asked": true, "model": PIN})),
            StatusCode::OK,
        )
        .await;
    let id = asked["id"].as_str().unwrap();
    let keeper = idle_keeper(&h, id).await;
    let review_route = format!("/v1/repositories/{repo}/pull-requests/1/ariadne-review");
    let mut reviewer = None;
    for stop_review in [true, false] {
        let _: Value = h
            .json(
                put_json(&review_route, json!({"asked": true, "model": PIN})),
                StatusCode::OK,
            )
            .await;
        eventually(TIMEOUT, "the reviewer beside the keeper", async || {
            reviewer = sessions(&h, id).await.into_iter().find(|s| {
                s.seat() == Some(Seat::Reviewer)
                    && s.status() == SessionStatus::Idle
                    && s.launched_at.is_some()
            });
            reviewer.is_some()
        })
        .await;
        if stop_review {
            let _: Value = h
                .json(
                    put_json(&review_route, json!({"asked": false})),
                    StatusCode::OK,
                )
                .await;
            eventually(TIMEOUT, "only the review to stop", async || {
                !h.session_status(reviewer.as_ref().unwrap()).await.is_live()
            })
            .await;
            h.flush_scheduler().await;
            assert!(h.session_status(&keeper).await.is_live());
            assert!(h.store.get_pull_request(id).await.unwrap().babysit_asked);
        }
    }
    let reviewer = reviewer.unwrap();
    let _: Value = h
        .json(babysit(&repo, 1, json!({"asked": false})), StatusCode::OK)
        .await;
    eventually(TIMEOUT, "only the keeper to stop", async || {
        !h.session_status(&keeper).await.is_live()
    })
    .await;
    h.flush_scheduler().await;
    assert!(h.session_status(&reviewer).await.is_live());
    assert!(h.store.get_pull_request(id).await.unwrap().review_asked);
    let _: Value = h
        .json(
            put_json(&review_route, json!({"asked": false})),
            StatusCode::OK,
        )
        .await;
    eventually(
        TIMEOUT,
        "the last session and its request to end",
        async || {
            !h.session_status(&reviewer).await.is_live()
                && h.store.get_pull_request(id).await.is_err()
        },
    )
    .await;
}

/// Where the repository supervises its posts, the default (002 rule 13),
/// the keeper's reply to a top-level conversation comment holds nothing on
/// the forge directly, the same route and the same check the task's `pr`
/// agent answers through (026 rule 24, 032 rule 13): it answers
/// `pending: true` with the pending review's own id, posts no
/// `gh pr comment`, and the comment counts answered with no further reply
/// taken — so a later poll hands the keeper no further news of it.
#[tokio::test]
async fn a_keeper_holds_its_reply_pending_when_the_repository_supervises_posts() {
    let stub = stub_forge_cli(json!([]));
    let h = harness().scheduler().forge_cli(&stub).await;
    let (path, head) = checkout(&h);
    stub.reprogram(script(&Shown::open(&head)));
    let repo = repository(&h, &path).await;
    // The repository's own worker starts a fetch of its own the moment it
    // is created: waited out before the script changes below, since one
    // still under way would answer its own remaining calls from the new
    // script instead of this one.
    eventually(TIMEOUT, "the first fetch to end", async || {
        stub.fetches() >= 1
    })
    .await;
    h.state.forge_poll.set_mode(&repo, Mode::WakeOnly);
    h.flush_scheduler().await;

    let before_ask = stub.fetches();
    let asked: Value = h
        .json(
            babysit(&repo, 1, json!({"asked": true, "model": PIN})),
            StatusCode::OK,
        )
        .await;
    let id = asked["id"].as_str().expect("a row").to_string();
    let session = idle_keeper(&h, &id).await;
    // The ask itself wakes a round of its own too
    // (`http::pull_requests`'s babysit handler): wait for it to finish
    // before the script changes below.
    eventually(TIMEOUT, "the ask's own fetch to finish", async || {
        stub.fetches() > before_ask
    })
    .await;

    // A comment by another login is the author's news, told once.
    let shown_with_alice = Shown {
        updated_at: "2026-10-02T00:00:00Z",
        comments: json!([
            {"id": 301, "user": {"login": "alice"}, "body": "Looks good but one nit.",
             "created_at": "2026-10-02T00:00:00Z"}
        ]),
        ..Shown::open(&head)
    };
    stub.reprogram(script(&shown_with_alice));
    let before_alice = stub.fetches();
    h.state.forge_poll.wake(&repo);
    eventually(TIMEOUT, "the news to reach the keeper", async || {
        h.prompts_to(&session)
            .last()
            .is_some_and(|news| news.contains("ic-301"))
    })
    .await;
    let told = h.prompts_to(&session).len();
    // The scheduler is woken with the row's own news mid-fetch, before the
    // round's own trailing calls end (`forge::poll::fetch`): waited out
    // here too, since the script changes below for the same reason as the
    // first fetch's own wait above.
    eventually(TIMEOUT, "the alice fetch to end", async || {
        stub.fetches() > before_alice
    })
    .await;

    // The repository holds posts back: the reply finds no pending review
    // of its own yet (the plain `pulls/1/reviews` entry below still
    // answers `[]`), creates one with no body, then appends to it through
    // GraphQL — each matched ahead of the generic entry it would
    // otherwise fall through to.
    let mut pending_script = script(&shown_with_alice);
    pending_script.as_array_mut().unwrap().splice(
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
    stub.reprogram(pending_script);

    let before = stub.invocations().len();
    // The mark lives in the store alone, on no field a forge fetch ever
    // reads back, so the route announces the change itself: a watcher
    // waiting on `pull_requests_changed` learns of it without a poll.
    let mut events = h.bus.subscribe();
    let moved = |event: &ariadne_daemon::bus::BusEvent| matches!(&event.event, ariadne_api::stream::DomainEvent::PullRequestsChanged(p) if p.repository_id == repo);
    let reply: Value = h
        .json(
            as_session(
                &format!("/v1/pull-requests/{id}/comments/ic-301/reply"),
                &session.id,
                json!({"body": "Thanks!"}),
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
            &format!("/v1/pull-requests/{id}/comments/ic-301/reply"),
            &session.id,
            json!({"body": "Again."}),
        ),
        StatusCode::BAD_REQUEST,
    )
    .await;

    // No new prompt reaches the keeper: the pending mark already answers
    // the comment, so a later poll hands it no news of it again.
    fetch_again(&h, &stub, &repo).await;
    tokio::time::sleep(QUIET).await;
    assert_eq!(
        h.prompts_to(&session).len(),
        told,
        "the pending mark alone settles the comment: no further poll wakes the keeper"
    );
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
        told.contains("git push 'https://github.com/alice/widgets.git' HEAD:'fix'"),
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
    assert!(told.contains("git push 'origin' HEAD:'fix'"), "{told}");
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
    // The repository's own worker starts a fetch of its own the moment it
    // is created: waited out before anything that wakes it again, since
    // one still under way would answer its own remaining calls from a
    // later script instead of this one.
    eventually(TIMEOUT, "the first fetch to end", async || {
        stub.fetches() >= 1
    })
    .await;
    h.state.forge_poll.set_mode(&repo, Mode::WakeOnly);
    h.flush_scheduler().await;

    let before = stub.fetches();
    let asked: Value = h
        .json(
            babysit(&repo, 1, json!({"asked": true, "model": PIN})),
            StatusCode::OK,
        )
        .await;
    let id = asked["id"].as_str().expect("a row").to_string();
    let session = idle_keeper(&h, &id).await;
    // The ask itself wakes a round of its own too
    // (`http::pull_requests`'s babysit handler): wait for it to finish
    // before the script changes below.
    eventually(TIMEOUT, "the ask's own fetch to finish", async || {
        stub.fetches() > before
    })
    .await;
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

/// A source branch that disappears without a merge is not proof of
/// anything: the forge's last recorded head is the one the keeper itself
/// pushed, but nobody merged it, so teardown must still preserve the
/// branch naming it rather than trust that head as it would once the
/// request actually landed (032).
#[tokio::test]
async fn a_branch_whose_source_vanished_before_any_merge_is_preserved() {
    let stub = stub_forge_cli(json!([]));
    let h = harness().scheduler().forge_cli(&stub).await;
    let (path, head) = checkout(&h);
    stub.reprogram(script(&Shown::open(&head)));
    let repo = repository(&h, &path).await;
    // The repository's own worker starts a fetch of its own the moment it
    // is created: waited out before anything that wakes it again, since
    // one still under way would answer its own remaining calls from a
    // later script instead of this one.
    eventually(TIMEOUT, "the first fetch to end", async || {
        stub.fetches() >= 1
    })
    .await;
    h.state.forge_poll.set_mode(&repo, Mode::WakeOnly);
    h.flush_scheduler().await;

    let before = stub.fetches();
    let asked: Value = h
        .json(
            babysit(&repo, 1, json!({"asked": true, "model": PIN})),
            StatusCode::OK,
        )
        .await;
    let id = asked["id"].as_str().expect("a row").to_string();
    let session = idle_keeper(&h, &id).await;
    // The ask itself wakes a round of its own too
    // (`http::pull_requests`'s babysit handler): wait for it to finish
    // before the script changes below.
    eventually(TIMEOUT, "the ask's own fetch to finish", async || {
        stub.fetches() > before
    })
    .await;
    let worktree = PathBuf::from(session.worktree_path.clone().unwrap());
    let branch = format!("ariadne/keep-{id}");

    // The keeper pushes a fix, the forge reads it as the request's head,
    // and then — before anybody merges it — its source branch vanishes:
    // force-deleted, or a fork the user removed.
    sh(
        &worktree,
        "echo b > b.txt && git add b.txt && \
         git -c user.email=t@t -c user.name=t commit -qm b",
    );
    let pushed = sh(&worktree, "git rev-parse HEAD");
    sh(&worktree, &format!("git push -q origin {branch}:fix"));
    sh(&path, "git push -q origin --delete fix");
    stub.reprogram(script(&Shown::open(&pushed)));
    fetch_again(&h, &stub, &repo).await;

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
        pushed,
        "the branch naming the unmerged commit is left too"
    );
}

/// A keeper on a fork is reached the way the configured remote reaches its
/// own repository, not the plain HTTPS URL the forge always reports for a
/// head: a private fork needs the same credentials the base already has,
/// which an SSH remote does not carry over HTTPS (032).
#[tokio::test]
async fn a_keeper_on_a_fork_pushes_through_the_configured_remotes_own_scheme() {
    let stub = stub_forge_cli(json!([]));
    let h = harness().scheduler().forge_cli(&stub).await;
    let (path, head) = checkout(&h);
    stub.reprogram(script(&Shown {
        head_repo: "https://github.com/alice/widgets",
        ..Shown::open(&head)
    }));
    let repo = repository(&h, &path).await;
    sh(
        &path,
        "git remote set-url origin git@github.com:acme/widgets.git",
    );
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
        told.contains("git push 'git@github.com:alice/widgets.git' HEAD:'fix'"),
        "{told}"
    );
}

/// A keeper on a fork pushes through the configured remote's own push
/// URL, not the one it fetches from, where a remote reads over one
/// protocol and writes over another: the fetch and the push are not
/// always the same credentials (032).
#[tokio::test]
async fn a_keeper_on_a_fork_pushes_through_the_configured_remotes_own_push_url() {
    let stub = stub_forge_cli(json!([]));
    let h = harness().scheduler().forge_cli(&stub).await;
    let (path, head) = checkout(&h);
    stub.reprogram(script(&Shown {
        head_repo: "https://github.com/alice/widgets",
        ..Shown::open(&head)
    }));
    let repo = repository(&h, &path).await;
    sh(
        &path,
        "git remote set-url origin https://github.com/acme/widgets.git && \
         git remote set-url --push origin git@github.com:acme/widgets.git",
    );
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
        told.contains("git push 'git@github.com:alice/widgets.git' HEAD:'fix'"),
        "the push command follows the push URL's own scheme: {told}"
    );
}

/// A keeper fetches a fork's head through the configured remote's own
/// push URL too, where the head is not already on disk: a private fork
/// needs authenticated transport to be read as surely as to be written,
/// and the push URL is the one the user's credentials are actually set
/// up for — the fetch URL is often plain HTTPS even where the push one is
/// SSH, since fetching never used to need to authenticate (032).
#[tokio::test]
async fn a_keeper_fetches_a_forks_head_through_the_configured_remotes_own_push_url() {
    let stub = stub_forge_cli(json!([]));
    let h = harness().scheduler().forge_cli(&stub).await;
    let (path, _head) = checkout(&h);
    let repo = repository(&h, &path).await;
    sh(
        &path,
        "git remote set-url origin https://github.com/acme/widgets.git && \
         git remote set-url --push origin git@github.com:acme/widgets.git",
    );

    // The fork holds a commit of its own, never fetched into the base
    // checkout: a scratch clone pushes it to the fork's own bare remote,
    // which the push URL's rewrite — `git@github.com:alice/widgets.git`
    // — is mapped to, standing in for the fork the forge would otherwise
    // name.
    let fork_remote = h.at("fork.git");
    let scratch = h.at("fork-scratch");
    sh(
        &path,
        &format!(
            "git init -q --bare '{}' && \
             git clone -q '{}' '{}'",
            fork_remote.display(),
            path.display(),
            scratch.display(),
        ),
    );
    sh(
        &scratch,
        &format!(
            "git checkout -q fix && echo c > c.txt && git add c.txt && \
             git -c user.email=t@t -c user.name=t commit -qm c && \
             git remote add fork '{}' && git push -q fork fix",
            fork_remote.display(),
        ),
    );
    let fork_head = sh(&scratch, "git rev-parse fix");
    sh(
        &path,
        &format!(
            "git config 'url.{}.insteadOf' git@github.com:alice/widgets.git",
            fork_remote.display(),
        ),
    );

    stub.reprogram(script(&Shown {
        head_repo: "https://github.com/alice/widgets",
        ..Shown::open(&fork_head)
    }));
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

    assert_eq!(
        sh(&worktree, "git rev-parse HEAD"),
        fork_head,
        "the fork's head, fetched through the push URL's own transport"
    );
}

/// A keeper's agent that comes up and exits at once — a broken binary, a
/// pin that will not run — spends the same spawn-retry budget a dead
/// reviewer launch does (`start_pull_request_session`), on a budget key of
/// its own: three dead launches, and Ariadne stops trying, rather than a
/// fresh session every pass for as long as the row lives (032).
#[tokio::test]
async fn a_keeper_whose_agent_dies_the_moment_it_starts_gives_up_after_the_budget() {
    let stub = stub_forge_cli(json!([]));
    let timeouts = Timeouts {
        full_reconcile: RUNS_OUT,
        dead_launch_patience: Duration::from_secs(300),
        ..Timeouts::default()
    };
    let h = harness()
        .scheduler()
        .dying_agent()
        .timeouts(timeouts)
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

    // Each dead launch is driven and observed on its own, with the full
    // timeout to itself, rather than all three raced against one shared
    // wait: a host under load may take a while to notice any one of them
    // dead, but this still holds once it does, whatever that takes.
    for attempt in 1..=SPAWN_RETRY_BUDGET as usize {
        eventually(
            timeouts.dead_launch_patience,
            "the next dead launch to be counted",
            async || {
                h.state.forge_poll.wake(&repo);
                let rows = sessions(&h, &id).await;
                rows.len() >= attempt && rows.iter().all(|s| !s.status().is_live())
            },
        )
        .await;
    }

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
/// staffed on. A refusal starts no work. A request that a task already
/// keeps is refused with 409, regardless of its author.
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
