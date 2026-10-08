//! A pull request of mine gets a session of its own, fed by the daemon (026):
//! the detail fetch, the news, the tools the session answers through, and the
//! cleanup once a human merged or closed the request.
use std::path::{Path, PathBuf};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{Value, json};

use crate::common::forge::{StubForgeCli, answer, stub_forge_cli};
use crate::common::{
    Harness, QUIET, TIMEOUT, as_session, eventually, harness, post_json, put_json, sh,
};
use ariadne_api::SESSION_HEADER;
use ariadne_core::{AttentionReason, SessionStatus};
use ariadne_daemon::forge::poll::Mode;
use ariadne_daemon::scheduler::SchedEvent;
use ariadne_store::{AgentSession, NewGoal, SessionFilter};

const PIN: &str = "stub:test-model";

fn pull(state: &str, sha: &str) -> Value {
    json!({"number": 1, "url": "https://github.com/acme/widgets/pull/1",
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

/// What the stub forge answers: the request in `state` on the lists and its
/// own read, its review comments in one thread, and its check runs.
fn script(state: &str, comments: &[Value], check_runs: Value) -> Value {
    let threads = json!({"data": {"repository": {"pullRequest": {"reviewThreads": {"nodes": [
        {"id": "T1", "isResolved": false, "comments": {"nodes": [{"databaseId": 101}]}}
    ]}}}}});
    script_of(
        pull(state, "abc"),
        comments,
        &json!({"check_runs": check_runs}).to_string(),
        &threads.to_string(),
    )
}

/// The same, for a request read as `read`, with the check runs and the
/// review threads as `gh api --paginate` writes them: one JSON value per
/// page, back to back.
fn script_of(read: Value, comments: &[Value], check_runs: &str, threads: &str) -> Value {
    let listed = match read["state"].as_str() {
        Some("OPEN") => json!([read]),
        _ => json!([]),
    };
    let read = read.to_string();
    json!([
        answer(&["auth", "status"], 0, ""),
        answer(&["api", "user"], 0, "me"),
        answer(&["pr", "list"], 0, &listed.to_string()),
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

/// A checkout of `acme/widgets` holding the request's head branch `fix`, with
/// pushes going to a local bare repository so a remote delete can be read.
fn checkout(h: &Harness) -> (PathBuf, PathBuf) {
    let path = h.git_repo("widgets");
    let bare = h.at("widgets-remote.git");
    sh(
        &path,
        &format!(
            "git branch fix next && git init -q --bare '{bare}' && \
             git remote add origin https://github.com/acme/widgets.git && \
             git remote set-url --push origin '{bare}' && git push -q origin fix",
            bare = bare.display()
        ),
    );
    (path, bare)
}

/// The repository registered with its integration on, and the pin a request
/// session runs on where `babysit` names one.
async fn repository(h: &Harness, path: &Path, babysit: Option<&str>) -> String {
    let mut forge = json!({"enabled": true});
    if let Some(model) = babysit {
        forge["babysit_model"] = json!(model);
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
async fn the_request(h: &Harness) -> Value {
    let mut rows: Vec<Value> = Vec::new();
    eventually(TIMEOUT, "the request to be recorded", async || {
        rows = h.get("/v1/pull-requests?state=all").await;
        rows.len() == 1
    })
    .await;
    rows.remove(0)
}

/// The live session on a request, once its agent is up and has its briefing.
async fn live_session(h: &Harness, pull_request_id: &str) -> AgentSession {
    let mut found = None;
    eventually(TIMEOUT, "the request's session to be up", async || {
        found = h
            .store
            .list_sessions(SessionFilter {
                pull_request_id: Some(pull_request_id.into()),
                live_only: true,
                ..Default::default()
            })
            .await
            .unwrap()
            .into_iter()
            .find(|s| {
                matches!(s.status(), SessionStatus::Idle | SessionStatus::Running)
                    && s.launched_at.is_some()
            });
        found.is_some()
    })
    .await;
    found.unwrap()
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

/// An open request of mine in an enabled repository with a `babysit_model`
/// gets one live session on that pin: no goal and no task, the request's
/// title, a worktree on its head branch, and the `pr-babysit` document
/// written and indexed. The session listing filters by the request.
#[tokio::test]
async fn an_open_request_of_mine_gets_one_session_on_the_babysit_pin() {
    let stub = stub_forge_cli(quiet_script());
    let h = harness().scheduler().forge_cli(&stub).await;
    let (path, _) = checkout(&h);
    repository(&h, &path, Some(PIN)).await;
    let row = the_request(&h).await;
    let id = row["id"].as_str().unwrap();
    let session = live_session(&h, id).await;

    assert_eq!(session.model, PIN);
    assert_eq!(session.pull_request_id.as_deref(), Some(id));
    assert_eq!(session.goal_id, None);
    assert_eq!(session.task_id, None);
    assert_eq!(session.task_agent_id, None);
    assert_eq!(session.seat.as_deref(), Some("author"));
    assert_eq!(session.title.as_deref(), Some("Fix widgets"));
    let worktree = PathBuf::from(session.worktree_path.clone().unwrap());
    assert!(
        worktree.ends_with(format!("pr-{id}")),
        "{}",
        worktree.display()
    );
    assert_eq!(sh(&worktree, "git rev-parse --abbrev-ref HEAD"), "fix");

    let launch = h.launch_file(&session.id).expect("a launch file");
    assert!(
        launch.system_prompt.contains("- pr-babysit: "),
        "{}",
        launch.system_prompt
    );
    let skill = h
        .launcher
        .cfg
        .run_dir
        .join(&session.id)
        .join("skills/pr-babysit/SKILL.md");
    assert!(
        std::fs::read_to_string(skill)
            .unwrap()
            .contains("name: pr-babysit")
    );
    let env = &launch.mcp_servers[0].env;
    assert!(
        env.iter()
            .any(|v| v.name == "ARIADNE_PULL_REQUEST_ID" && v.value == id)
    );
    let briefing = h.told(&session.id);
    assert!(
        briefing.contains("# Pull request: Fix widgets"),
        "{briefing}"
    );
    assert!(briefing.contains(&worktree.display().to_string()));

    let listed: Value = h.get(&format!("/v1/sessions?pull_request={id}")).await;
    assert_eq!(listed["sessions"].as_array().unwrap().len(), 1);
    assert_eq!(listed["sessions"][0]["pull_request_id"], id);
    let dto: Value = h.get(&format!("/v1/pull-requests/{id}")).await;
    assert_eq!(dto["session_id"], session.id);

    // Still one after another fetch and another pass.
    h.flush_scheduler().await;
    let all = h
        .store
        .list_sessions(SessionFilter {
            pull_request_id: Some(id.into()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(all.len(), 1);
}

/// A repository with no `babysit_model` starts nothing for its requests.
#[tokio::test]
async fn a_request_with_no_babysit_model_gets_no_session() {
    let stub = stub_forge_cli(quiet_script());
    let h = harness().scheduler().forge_cli(&stub).await;
    let (path, _) = checkout(&h);
    let repo = repository(&h, &path, None).await;
    the_request(&h).await;
    h.state.forge_poll.set_mode(&repo, Mode::WakeOnly);
    h.state.forge_poll.wake(&repo);
    tokio::time::sleep(QUIET).await;
    h.flush_scheduler().await;
    assert!(
        h.store
            .list_sessions(SessionFilter::default())
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        detail_fetches(&stub),
        0,
        "a request nobody watches is not read"
    );
}

/// A new comment by another login is stored, counted, and handed to the
/// session in one prompt that names it; the same details again hand nothing;
/// a check that turned to failure is handed by name.
#[tokio::test]
async fn the_news_of_a_fetch_is_handed_to_the_session_once() {
    let stub = stub_forge_cli(quiet_script());
    let h = harness().scheduler().forge_cli(&stub).await;
    let (path, _) = checkout(&h);
    let repo = repository(&h, &path, Some(PIN)).await;
    let row = the_request(&h).await;
    let id = row["id"].as_str().unwrap().to_string();
    h.state.forge_poll.set_mode(&repo, Mode::WakeOnly);
    let session = live_session(&h, &id).await;
    eventually(TIMEOUT, "the briefing turn to end", async || {
        h.session_status(&session).await == SessionStatus::Idle
    })
    .await;
    let briefed = h.prompts_to(&session).len();

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
    eventually(TIMEOUT, "the news to reach the agent", async || {
        h.prompts_to(&session).len() == briefed + 1
    })
    .await;
    let news = h.prompts_to(&session).pop().unwrap();
    assert!(news.contains(&comment_id), "{news}");
    assert!(news.contains("alice"), "{news}");

    fetch_again(&h, &stub, &repo).await;
    assert_eq!(
        h.prompts_to(&session).len(),
        briefed + 1,
        "the same details are no news"
    );

    let failed = json!([{"name": "lint", "html_url": "https://ci.example/1",
        "conclusion": "failure", "status": "completed"}]);
    stub.reprogram(script("OPEN", &comments, failed));
    fetch_again(&h, &stub, &repo).await;
    eventually(TIMEOUT, "the failed check to reach the agent", async || {
        h.prompts_to(&session).len() == briefed + 2
    })
    .await;
    let news = h.prompts_to(&session).pop().unwrap();
    assert!(news.contains("Check lint turned to failure"), "{news}");
    assert!(
        !news.contains(&comment_id),
        "a comment is told once: {news}"
    );
    let dto: Value = h.get(&format!("/v1/pull-requests/{id}")).await;
    assert_eq!(dto["failed_checks"][0]["name"], "lint");
}

/// A reply runs the forge's reply command with the body, is stored as my
/// comment in the thread, and answers it; the next fetch tells nothing of
/// that thread, and nothing anywhere resolves a thread.
#[tokio::test]
async fn a_reply_answers_the_thread_and_resolves_nothing() {
    let comments = [review_comment(101, "alice", None, "2026-10-02T00:00:00Z")];
    let stub = stub_forge_cli(script("OPEN", &comments, json!([])));
    let h = harness().scheduler().forge_cli(&stub).await;
    let (path, _) = checkout(&h);
    let repo = repository(&h, &path, Some(PIN)).await;
    let row = the_request(&h).await;
    let id = row["id"].as_str().unwrap().to_string();
    h.state.forge_poll.set_mode(&repo, Mode::WakeOnly);
    let session = live_session(&h, &id).await;
    let mut stored: Vec<Value> = Vec::new();
    eventually(TIMEOUT, "the comment to be told", async || {
        stored = h.get(&format!("/v1/pull-requests/{id}/comments")).await;
        stored.len() == 1 && stored[0]["told_at"].is_string()
    })
    .await;
    let comment_id = stored[0]["id"].as_str().unwrap();
    // The claim stamps `told_at` right before the prompt is written, so the
    // prompt the agent read is waited for on its own.
    eventually(TIMEOUT, "the agent to read the comment", async || {
        h.prompts_to(&session)
            .iter()
            .any(|p| p.contains(comment_id))
    })
    .await;
    let told = h.prompts_to(&session).len();

    let reply: Value = h
        .json(
            as_session(
                &format!("/v1/pull-requests/{id}/comments/{comment_id}/reply"),
                &session.id,
                json!({"body": "Renamed it."}),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert_eq!(reply["author_login"], "me");
    assert_eq!(reply["thread_id"], "T1");
    assert_eq!(reply["forge_id"], "rc-201");
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
    let threads: Vec<Value> = h
        .get(&format!(
            "/v1/pull-requests/{id}/comments?unanswered_only=true"
        ))
        .await;
    assert!(threads.is_empty());

    let mut answered = comments.to_vec();
    answered.push(review_comment(201, "me", Some(101), "2026-10-03T00:00:00Z"));
    stub.reprogram(script("OPEN", &answered, json!([])));
    fetch_again(&h, &stub, &repo).await;
    assert_eq!(
        h.prompts_to(&session).len(),
        told,
        "an answered thread is no news"
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
}

/// `ready: true` raises `waiting_user` once and a repeat raises nothing;
/// `ready: false` takes it down. Another session is refused.
#[tokio::test]
async fn a_ready_report_raises_waiting_user_once_and_only_from_its_own_session() {
    let stub = stub_forge_cli(quiet_script());
    let h = harness().scheduler().forge_cli(&stub).await;
    let (path, _) = checkout(&h);
    repository(&h, &path, Some(PIN)).await;
    let row = the_request(&h).await;
    let id = row["id"].as_str().unwrap().to_string();
    let session = live_session(&h, &id).await;
    let report = |ready: bool| {
        as_session(
            &format!("/v1/pull-requests/{id}/report"),
            &session.id,
            json!({"ready": ready}),
        )
    };

    let dto: Value = h.json(report(true), StatusCode::OK).await;
    assert_eq!(dto["ready"], true);
    assert_eq!(
        h.attention(&session).await,
        Some(AttentionReason::WaitingUser)
    );
    let since = h
        .store
        .get_session(&session.id)
        .await
        .unwrap()
        .attention_since;
    let _: Value = h.json(report(true), StatusCode::OK).await;
    assert_eq!(
        h.store
            .get_session(&session.id)
            .await
            .unwrap()
            .attention_since,
        since,
        "a repeat raises nothing"
    );
    let _: Value = h.json(report(false), StatusCode::OK).await;
    assert_eq!(h.attention(&session).await, None);

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

/// A request that turns merged hands its session the news, kills it, removes
/// its worktree, and deletes the goal branch it is on, local and remote.
#[tokio::test]
async fn a_merged_request_ends_its_session_and_deletes_its_goal_branch() {
    let stub = stub_forge_cli(quiet_script());
    let h = harness().scheduler().forge_cli(&stub).await;
    let (path, bare) = checkout(&h);
    let repo = repository(&h, &path, Some(PIN)).await;
    goal_on_branch(&h, &repo, "fix").await;
    let row = the_request(&h).await;
    let id = row["id"].as_str().unwrap().to_string();
    h.state.forge_poll.set_mode(&repo, Mode::WakeOnly);
    let session = live_session(&h, &id).await;
    let worktree = PathBuf::from(session.worktree_path.clone().unwrap());

    stub.reprogram(script("MERGED", &[], json!([])));
    h.state.forge_poll.wake(&repo);
    eventually(TIMEOUT, "the session to be told and ended", async || {
        !h.session_status(&session).await.is_live()
    })
    .await;
    assert!(
        h.prompted(&session).contains("The request is now merged."),
        "{}",
        h.prompted(&session)
    );
    eventually(TIMEOUT, "the worktree and the branch to go", async || {
        !worktree.exists() && sh(&path, "git branch --list fix").is_empty()
    })
    .await;
    // The remote delete comes after the local one, in the same cleanup.
    eventually(
        TIMEOUT,
        "the goal branch to go on the remote too",
        async || sh(&bare, "git branch --list fix").is_empty(),
    )
    .await;
}

/// A request that turns closed ends its session too, and deletes no goal
/// branch: a goal that was not merged is not done.
#[tokio::test]
async fn a_closed_request_deletes_no_goal_branch() {
    let stub = stub_forge_cli(quiet_script());
    let h = harness().scheduler().forge_cli(&stub).await;
    let (path, bare) = checkout(&h);
    let repo = repository(&h, &path, Some(PIN)).await;
    goal_on_branch(&h, &repo, "fix").await;
    let row = the_request(&h).await;
    let id = row["id"].as_str().unwrap().to_string();
    h.state.forge_poll.set_mode(&repo, Mode::WakeOnly);
    let session = live_session(&h, &id).await;
    let worktree = PathBuf::from(session.worktree_path.clone().unwrap());

    stub.reprogram(script("CLOSED", &[], json!([])));
    h.state.forge_poll.wake(&repo);
    eventually(TIMEOUT, "the session to end", async || {
        !h.session_status(&session).await.is_live() && !worktree.exists()
    })
    .await;
    assert_eq!(sh(&path, "git branch --list fix"), "fix");
    assert_eq!(sh(&bare, "git branch --list fix"), "fix");
}

/// A session whose agent went away is resumed on its own row, and disabling
/// the integration ends it.
#[tokio::test]
async fn a_dead_agent_is_resumed_on_its_row_and_disabling_ends_the_session() {
    let stub = stub_forge_cli(quiet_script());
    let h = harness().scheduler().forge_cli(&stub).await;
    let (path, _) = checkout(&h);
    let repo = repository(&h, &path, Some(PIN)).await;
    let row = the_request(&h).await;
    let id = row["id"].as_str().unwrap().to_string();
    h.state.forge_poll.set_mode(&repo, Mode::WakeOnly);
    let session = live_session(&h, &id).await;
    eventually(
        TIMEOUT,
        "the agent to report its conversation",
        async || {
            h.store
                .get_session(&session.id)
                .await
                .unwrap()
                .internal_session_id
                .is_some()
        },
    )
    .await;
    let launched = h.launched_at(&session).await;

    h.launcher.kill_session(&session.id).await.unwrap();
    h.state.forge_poll.wake(&repo);
    eventually(TIMEOUT, "the same row to come back", async || {
        h.relaunched(&session, &launched).await
    })
    .await;
    let rows = h
        .store
        .list_sessions(SessionFilter {
            pull_request_id: Some(id.clone()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(rows.len(), 1, "the same session row, not a sibling");
    let resumed = h.launch_file(&session.id).expect("a launch file");
    assert_eq!(resumed.resume_session_id.as_deref(), Some("stub-session"));
    assert!(
        resumed
            .initial_prompt
            .unwrap_or_default()
            .contains("# Pull request: Fix widgets"),
        "the resume carries the request's briefing"
    );

    let _: Value = h
        .json(
            put_json(
                &format!("/v1/repositories/{repo}"),
                json!({"forge": {"enabled": false}}),
            ),
            StatusCode::OK,
        )
        .await;
    eventually(TIMEOUT, "the session to end", async || {
        !h.session_status(&session).await.is_live()
    })
    .await;
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

/// Wake the scheduler about one request and wait for that pass to end.
async fn pass_over(h: &Harness, pull_request_id: &str) {
    h.sched
        .as_ref()
        .expect("this harness has a scheduler")
        .send(SchedEvent::PullRequestChanged(pull_request_id.into()))
        .unwrap();
    h.flush_scheduler().await;
}

/// News handed to a session mid-turn waits in its queue, and is told only
/// once its prompt goes out. A kill before that leaves the comment untold,
/// and the next launch is told it.
#[tokio::test]
async fn news_queued_behind_a_turn_survives_a_kill_and_reaches_the_next_launch() {
    let comments = [review_comment(101, "alice", None, "2026-10-02T00:00:00Z")];
    let stub = stub_forge_cli(script("OPEN", &comments, json!([])));
    let h = harness().scheduler().forge_cli(&stub).await;
    let release = h.at("release-briefing");
    let mut held = crate::common::acp::script();
    held["prompts"] = json!([{"wait_for": release.display().to_string(), "updates": [],
        "stop_reason": "end_turn"}]);
    h.agent.reprogram(held);
    let (path, _) = checkout(&h);
    let repo = repository(&h, &path, Some(PIN)).await;
    let row = the_request(&h).await;
    let id = row["id"].as_str().unwrap().to_string();
    h.state.forge_poll.set_mode(&repo, Mode::WakeOnly);
    let session = live_session(&h, &id).await;
    let reached = h.at("release-briefing.reached");
    eventually(TIMEOUT, "the briefing turn to be held", async || {
        reached.exists()
    })
    .await;
    let mut stored: Vec<Value> = Vec::new();
    eventually(TIMEOUT, "the comment to be stored", async || {
        stored = h.get(&format!("/v1/pull-requests/{id}/comments")).await;
        stored.len() == 1
    })
    .await;
    let comment_id = stored[0]["id"].as_str().unwrap().to_string();
    // A pass over the request while the briefing runs queues the news.
    pass_over(&h, &id).await;
    let told = async || {
        let stored: Vec<Value> = h.get(&format!("/v1/pull-requests/{id}/comments")).await;
        stored[0]["told_at"].is_string()
    };
    assert!(!told().await, "news queued behind a turn is not told yet");

    h.launcher.kill_session(&session.id).await.unwrap();
    std::fs::write(&release, "").unwrap();
    assert!(!told().await, "a kill leaves the queued news untold");
    h.state.forge_poll.wake(&repo);
    // The killed launch was never heard from after its start, so the next
    // one may be a fresh row (009 rule 27): whichever it is gets the news.
    eventually(TIMEOUT, "the next launch to be told the news", async || {
        h.store
            .list_sessions(SessionFilter {
                pull_request_id: Some(id.clone()),
                live_only: true,
                ..Default::default()
            })
            .await
            .unwrap()
            .iter()
            .any(|s| {
                h.launch_file(&s.id)
                    .is_some_and(|l| l.initial_prompt.is_some())
                    && h.prompts_to(s).iter().any(|p| p.contains(&comment_id))
            })
    })
    .await;
    eventually(TIMEOUT, "the news to be marked told", told).await;
}

/// A check that fails is told, its recovery is no news but is recorded, and
/// its next failure is told again. So is a head that falls behind its base
/// a second time.
#[tokio::test]
async fn a_check_that_recovers_and_fails_again_is_told_again() {
    let stub = stub_forge_cli(quiet_script());
    let h = harness().scheduler().forge_cli(&stub).await;
    let (path, _) = checkout(&h);
    let repo = repository(&h, &path, Some(PIN)).await;
    let row = the_request(&h).await;
    let id = row["id"].as_str().unwrap().to_string();
    h.state.forge_poll.set_mode(&repo, Mode::WakeOnly);
    let session = live_session(&h, &id).await;
    eventually(TIMEOUT, "the briefing turn to end", async || {
        h.session_status(&session).await == SessionStatus::Idle
    })
    .await;
    let failed = json!([{"name": "lint", "html_url": "https://ci.example/1",
        "conclusion": "failure"}]);
    let told_lint = || {
        h.prompts_to(&session)
            .iter()
            .filter(|p| p.contains("Check lint turned to failure"))
            .count()
    };

    stub.reprogram(script("OPEN", &[], failed.clone()));
    fetch_again(&h, &stub, &repo).await;
    eventually(TIMEOUT, "the failure to be told", async || told_lint() == 1).await;

    stub.reprogram(quiet_script());
    fetch_again(&h, &stub, &repo).await;
    let after_recovery = h.prompts_to(&session).len();

    stub.reprogram(script("OPEN", &[], failed));
    fetch_again(&h, &stub, &repo).await;
    eventually(TIMEOUT, "the second failure to be told", async || {
        told_lint() == 2
    })
    .await;
    assert_eq!(
        h.prompts_to(&session).len(),
        after_recovery + 1,
        "the recovery itself was no news"
    );
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
        &[],
        r#"{"check_runs": []}"#,
        &threads.to_string(),
    ));
    let h = harness().scheduler().forge_cli(&stub).await;
    let (path, _) = checkout(&h);
    let repo = repository(&h, &path, Some(PIN)).await;
    let row = the_request(&h).await;
    assert_eq!(row["role"], "reviewer");
    let id = row["id"].as_str().unwrap().to_string();
    h.state.forge_poll.set_mode(&repo, Mode::WakeOnly);

    read["state"] = json!("CLOSED");
    stub.reprogram(script_of(
        read,
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

/// A head branch checked out elsewhere — the author of the task that opened
/// the request, still finishing it — is waited for, however many passes it
/// takes, and spends no launch attempt. Once the branch is free the request
/// gets its session.
#[tokio::test]
async fn a_head_branch_in_use_is_waited_for_and_then_handed_over() {
    let stub = stub_forge_cli(quiet_script());
    let h = harness().scheduler().forge_cli(&stub).await;
    let (path, _) = checkout(&h);
    let occupied = h.at("task-worktree");
    sh(
        &path,
        &format!("git worktree add -q '{}' fix", occupied.display()),
    );
    let repo = repository(&h, &path, Some(PIN)).await;
    let row = the_request(&h).await;
    let id = row["id"].as_str().unwrap().to_string();
    h.state.forge_poll.set_mode(&repo, Mode::WakeOnly);
    for _ in 0..4 {
        pass_over(&h, &id).await;
    }
    assert!(
        h.store
            .list_sessions(SessionFilter {
                pull_request_id: Some(id.clone()),
                ..Default::default()
            })
            .await
            .unwrap()
            .is_empty(),
        "no session row while the branch is in use"
    );

    sh(
        &path,
        &format!("git worktree remove '{}'", occupied.display()),
    );
    pass_over(&h, &id).await;
    let session = live_session(&h, &id).await;
    let worktree = PathBuf::from(session.worktree_path.unwrap());
    assert_eq!(sh(&worktree, "git rev-parse --abbrev-ref HEAD"), "fix");
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
    let stub = stub_forge_cli(script_of(
        pull("OPEN", "abc"),
        &comments,
        &check_runs,
        &threads,
    ));
    let h = harness().scheduler().forge_cli(&stub).await;
    let (path, _) = checkout(&h);
    repository(&h, &path, Some(PIN)).await;
    let row = the_request(&h).await;
    let id = row["id"].as_str().unwrap().to_string();
    let mut dto = Value::Null;
    eventually(TIMEOUT, "the details to be stored", async || {
        dto = h.get(&format!("/v1/pull-requests/{id}")).await;
        dto["failed_checks"]
            .as_array()
            .is_some_and(|c| !c.is_empty())
    })
    .await;
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

/// A goal branch whose remote delete fails is not forgotten: the cleanup
/// stays pending on the row, and the first pass of a restarted daemon after
/// the remote recovered deletes it.
#[tokio::test]
async fn a_failed_remote_delete_of_a_goal_branch_is_tried_again() {
    let stub = stub_forge_cli(quiet_script());
    let h = harness().scheduler().forge_cli(&stub).await;
    let (path, bare) = checkout(&h);
    let gone = h.at("no-such-remote.git");
    sh(
        &path,
        &format!("git remote set-url --push origin '{}'", gone.display()),
    );
    let repo = repository(&h, &path, Some(PIN)).await;
    goal_on_branch(&h, &repo, "fix").await;
    let row = the_request(&h).await;
    let id = row["id"].as_str().unwrap().to_string();
    h.state.forge_poll.set_mode(&repo, Mode::WakeOnly);
    let session = live_session(&h, &id).await;

    stub.reprogram(script("MERGED", &[], json!([])));
    h.state.forge_poll.wake(&repo);
    eventually(TIMEOUT, "the local branch to go", async || {
        !h.session_status(&session).await.is_live() && sh(&path, "git branch --list fix").is_empty()
    })
    .await;
    pass_over(&h, &id).await;
    assert_eq!(
        sh(&bare, "git branch --list fix"),
        "fix",
        "the remote could not be reached"
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

/// The end of a request is told even where older news holds the request's
/// place in the queue behind a running turn: the session is kept until the
/// end was claimed and read, and each piece of news reaches it once.
#[tokio::test]
async fn a_merge_while_older_news_waits_behind_a_turn_is_still_told() {
    let comments = [review_comment(101, "alice", None, "2026-10-02T00:00:00Z")];
    let stub = stub_forge_cli(script("OPEN", &comments, json!([])));
    let h = harness().scheduler().forge_cli(&stub).await;
    let release = h.at("release-briefing");
    let mut held = crate::common::acp::script();
    held["prompts"] = json!([{"wait_for": release.display().to_string(), "updates": [],
        "stop_reason": "end_turn"}]);
    h.agent.reprogram(held);
    let (path, _) = checkout(&h);
    let repo = repository(&h, &path, Some(PIN)).await;
    let row = the_request(&h).await;
    let id = row["id"].as_str().unwrap().to_string();
    h.state.forge_poll.set_mode(&repo, Mode::WakeOnly);
    let session = live_session(&h, &id).await;
    let reached = h.at("release-briefing.reached");
    eventually(TIMEOUT, "the briefing turn to be held", async || {
        reached.exists()
    })
    .await;
    let mut stored: Vec<Value> = Vec::new();
    eventually(TIMEOUT, "the comment to be stored", async || {
        stored = h.get(&format!("/v1/pull-requests/{id}/comments")).await;
        stored.len() == 1
    })
    .await;
    let comment_id = stored[0]["id"].as_str().unwrap().to_string();
    pass_over(&h, &id).await;

    stub.reprogram(script("MERGED", &comments, json!([])));
    h.state.forge_poll.wake(&repo);
    eventually(TIMEOUT, "the request to merge", async || {
        h.get::<Value>(&format!("/v1/pull-requests/{id}")).await["state"] == "merged"
    })
    .await;
    for _ in 0..3 {
        pass_over(&h, &id).await;
    }
    assert!(
        h.session_status(&session).await.is_live(),
        "the session stays until it read the end"
    );

    std::fs::write(&release, "").unwrap();
    eventually(
        TIMEOUT,
        "the end to be told and the session ended",
        async || {
            h.prompted(&session).contains("The request is now merged.")
                && !h.session_status(&session).await.is_live()
        },
    )
    .await;
    let telling = |text: &str| {
        h.prompts_to(&session)
            .iter()
            .filter(|p| p.contains(text))
            .count()
    };
    assert_eq!(telling(&comment_id), 1, "the comment is told once");
    assert_eq!(
        telling("The request is now merged."),
        1,
        "the end is told once"
    );
}
