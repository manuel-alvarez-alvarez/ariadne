//! Pull requests through HTTP and the forge CLI boundary (026): read live
//! off the forge, and kept only while Ariadne works on them.
use crate::common::forge::{answer, stub_forge_cli};
use crate::common::{delete, get, harness, post_json, sh};
use axum::http::StatusCode;
use serde_json::{Value, json};

const PIN: &str = "stub:review-model";

fn github_pull(number: i64, author: &str) -> Value {
    json!({"number": number, "url": format!("https://github.com/acme/widgets/pull/{number}"),
        "title": "Fix widgets", "body": "Fixes widgets.", "author": {"login": author},
        "state": "OPEN", "isDraft": false, "headRefName": "fix", "headRefOid": "abc",
        "headRepository": {"url": "https://github.com/acme/widgets"},
        "baseRefName": "main", "statusCheckRollup": [], "reviewDecision": "",
        "createdAt": "2026-10-01T00:00:00Z", "updatedAt": "2026-10-02T00:00:00Z"})
}

/// What the stub answers for a detail read of request `number`: no
/// comment, no failed check, a head level with its base.
fn details(number: i64) -> Vec<Value> {
    let threads =
        json!({"data": {"repository": {"pullRequest": {"reviewThreads": {"nodes": []}}}}});
    vec![
        answer(
            &[
                "api",
                &format!("repos/acme/widgets/pulls/{number}/comments"),
            ],
            0,
            "[]",
        ),
        answer(
            &[
                "api",
                &format!("repos/acme/widgets/issues/{number}/comments"),
            ],
            0,
            "[]",
        ),
        answer(
            &["api", &format!("repos/acme/widgets/pulls/{number}/reviews")],
            0,
            "[]",
        ),
        answer(&["api", "graphql"], 0, &threads.to_string()),
        answer(
            &["api", "repos/acme/widgets/commits/abc/check-runs"],
            0,
            r#"{"check_runs": []}"#,
        ),
        answer(
            &["api", "repos/acme/widgets/compare/main...abc"],
            0,
            r#"{"behind_by": 0}"#,
        ),
    ]
}

async fn enabled_repository(h: &crate::common::Harness, review: Option<&str>) -> String {
    let path = h.git_repo("enabled");
    sh(
        &path,
        "git remote add origin https://github.com/acme/widgets.git",
    );
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
    repo["id"].as_str().unwrap().into()
}

/// The stub with `list` as every open request, and as every one that asks
/// for my review, `view` as the read of any one, and the details of #1.
fn script(list: Vec<Value>, view: Value) -> Value {
    let mut answers = vec![
        answer(&["auth", "status"], 0, ""),
        answer(&["api", "user"], 0, "me"),
        answer(&["pr", "list"], 0, &serde_json::to_string(&list).unwrap()),
        answer(&["pr", "view"], 0, &view.to_string()),
        answer(
            &["api", "repos/acme/widgets/issues?state=open&per_page=100"],
            0,
            "[]",
        ),
    ];
    answers.extend(details(1));
    answers.extend(details(2));
    json!(answers)
}

/// The open requests are read live off the forge: every one, mine, or the
/// ones that ask for my review, and one of them whole, its checks and
/// comments read with it. A request of mine Ariadne was not asked to
/// review stays untracked; a request that asks for my review gets a row
/// even on a repository with no review pin (029), so a human still has
/// something to start by hand — but nothing starts its session, and
/// nothing adds or removes a row here by hand.
#[tokio::test]
async fn the_list_is_read_live_and_tracks_only_a_request_that_asks_for_my_review() {
    use crate::common::{TIMEOUT, eventually};
    let stub = stub_forge_cli(script(
        vec![github_pull(1, "me"), github_pull(2, "other")],
        github_pull(2, "other"),
    ));
    let h = harness().forge_cli(&stub).await;
    let id = enabled_repository(&h, None).await;
    eventually(TIMEOUT, "the first fetch", async || {
        stub.invocations()
            .iter()
            .any(|i| i.args.starts_with(&["pr".into(), "list".into()]))
    })
    .await;

    let rows: Vec<Value> = h.get("/v1/pull-requests").await;
    assert_eq!(rows.len(), 2);
    let mine_row = rows.iter().find(|r| r["number"] == 1).unwrap();
    assert!(mine_row["id"].is_null(), "{mine_row:?}");
    let asked_row = rows.iter().find(|r| r["number"] == 2).unwrap();
    assert!(!asked_row["id"].is_null(), "{asked_row:?}");
    let mine: Vec<Value> = h.get("/v1/pull-requests?role=author").await;
    assert_eq!(mine.len(), 1);
    assert_eq!(mine[0]["number"], 1);
    assert_eq!(mine[0]["body"], "Fixes widgets.");
    let asks: Vec<Value> = h
        .get("/v1/pull-requests?role=reviewer&requested=true")
        .await;
    assert_eq!(asks.len(), 1);
    assert_eq!(asks[0]["number"], 2);
    assert_eq!(asks[0]["review_requested"], true);

    let one: Value = h
        .get(&format!("/v1/repositories/{id}/pull-requests/2"))
        .await;
    assert_eq!(one["title"], "Fix widgets");
    assert_eq!(one["unanswered_comments"], 0);
    assert!(!one["id"].is_null());

    // Search excludes a request of mine (`author`) outright; the one that
    // asks for my review is tracked now, so the search says so.
    let matches: Vec<Value> = h
        .get(&format!(
            "/v1/repositories/{id}/pull-requests/search?q=widgets"
        ))
        .await;
    assert!(matches.iter().all(|m| m["tracked"] == true), "{matches:?}");

    let stored = h
        .store
        .list_pull_requests(ariadne_store::PullRequestFilter::default())
        .await
        .unwrap();
    assert_eq!(
        stored.len(),
        1,
        "only the request that asks for my review is tracked: {stored:?}"
    );
    assert_eq!(stored[0].number, 2);
    assert_eq!(stored[0].role, "reviewer");
    for request in [
        post_json(
            "/v1/pull-requests",
            json!({"repository_id": id, "number": 2}),
        ),
        delete("/v1/pull-requests/anything"),
    ] {
        assert_eq!(h.send(request).await.0, StatusCode::METHOD_NOT_ALLOWED);
    }
}

/// A detail read asks the forge for its parts together: with the reviews
/// held open, the comments, the review threads, and the check runs and
/// compare that wait on the request's own read are all asked already.
#[tokio::test]
async fn a_detail_read_asks_the_forge_for_its_parts_together() {
    use crate::common::{TIMEOUT, eventually};
    let stub = stub_forge_cli(script(vec![], github_pull(2, "other")));
    let h = harness().forge_cli(&stub).await;
    let id = enabled_repository(&h, None).await;
    let gate = h.dir.path().join("release-reviews");
    let mut blocked = script(vec![], github_pull(2, "other"));
    for entry in blocked.as_array_mut().unwrap() {
        if entry["args"][1] == "repos/acme/widgets/pulls/2/reviews" {
            entry["wait_for"] = json!(gate);
        }
    }
    stub.reprogram(blocked);
    let others = [
        "graphql",
        "repos/acme/widgets/pulls/2/comments",
        "repos/acme/widgets/issues/2/comments",
        "repos/acme/widgets/commits/abc/check-runs",
        "repos/acme/widgets/compare/main...abc",
    ];

    let uri = format!("/v1/repositories/{id}/pull-requests/2");
    let read = h.get::<Value>(&uri);
    let release = async {
        eventually(TIMEOUT, "the other parts asked meanwhile", async || {
            let calls = stub.invocations();
            others.iter().all(|path| {
                calls
                    .iter()
                    .any(|i| i.args.get(1).is_some_and(|a| a == path))
            })
        })
        .await;
        std::fs::write(&gate, "").unwrap();
    };
    let (one, ()) = tokio::join!(read, release);
    assert_eq!(one["number"], 2);
    assert_eq!(one["behind_base"], false);
}

/// A request that asks for my review on a repository with a review pin is
/// one Ariadne reviews (029): the fetch that finds it gives it a row, which
/// lists with its id. Once it merged its review is taken down and the row
/// goes, so it takes no room; nothing brings it back.
#[tokio::test]
async fn a_review_request_has_a_row_while_it_is_reviewed_and_none_once_it_merged() {
    use crate::common::{TIMEOUT, eventually};
    let stub = stub_forge_cli(script(
        vec![github_pull(1, "me"), github_pull(2, "other")],
        github_pull(2, "other"),
    ));
    let h = harness().scheduler().forge_cli(&stub).await;
    let id = enabled_repository(&h, Some(PIN)).await;
    let rows = || async {
        h.store
            .list_pull_requests(ariadne_store::PullRequestFilter::default())
            .await
            .unwrap()
    };
    eventually(TIMEOUT, "the review request's row", async || {
        rows().await.len() == 1
    })
    .await;
    let row = rows().await.pop().unwrap();
    assert_eq!(row.number, 2, "a request of mine nobody works on has none");
    assert_eq!(row.role, "reviewer");
    let listed: Vec<Value> = h.get(&format!("/v1/pull-requests?repo={id}")).await;
    let reviewed = listed.iter().find(|r| r["number"] == 2).unwrap();
    assert_eq!(reviewed["id"], row.id);

    let mut merged = github_pull(2, "other");
    merged["state"] = json!("MERGED");
    stub.reprogram(script(vec![github_pull(1, "me")], merged));
    h.state.forge_poll.wake(&id);
    eventually(TIMEOUT, "the merged request's row to go", async || {
        rows().await.is_empty()
    })
    .await;
}

#[tokio::test]
async fn timer_wake_only_and_disable_control_repository_fetches() {
    use crate::common::{QUIET, RUNS_OUT, TIMEOUT, eventually, put_json};
    use ariadne_daemon::{forge::poll::Mode, timeouts::Timeouts};
    let stub = stub_forge_cli(script(
        vec![github_pull(1, "other")],
        github_pull(1, "other"),
    ));
    let h = harness()
        .forge_cli(&stub)
        .timeouts(Timeouts {
            forge_poll: RUNS_OUT,
            ..Default::default()
        })
        .await;
    let id = enabled_repository(&h, None).await;
    let lists = || {
        stub.invocations()
            .iter()
            .filter(|i| i.args.starts_with(&["pr".into(), "list".into()]))
            .count()
    };
    eventually(TIMEOUT, "enable and timer fetch", async || lists() >= 4).await;
    h.state.forge_poll.set_mode(&id, Mode::WakeOnly);
    let idle = || stub.completed() == stub.invocations().len();
    eventually(TIMEOUT, "the fetches to settle", async || idle()).await;
    let count = lists();
    // Prove that WakeOnly starts no timer fetch.
    tokio::time::sleep(QUIET + RUNS_OUT).await;
    assert_eq!(lists(), count, "WakeOnly must not use the timer");
    h.state.forge_poll.wake(&id);
    eventually(TIMEOUT, "the wake's fetch", async || {
        lists() == count + 2 && idle()
    })
    .await;
    assert_eq!(
        lists(),
        count + 2,
        "one wake lists authored and review requests once"
    );
    let _: Value = h
        .json(
            put_json(
                &format!("/v1/repositories/{id}"),
                json!({"forge":{"enabled":false}}),
            ),
            StatusCode::OK,
        )
        .await;
    let count = lists();
    h.state.forge_poll.wake(&id);
    // Prove that the disabled repository makes no timer or wake fetch.
    tokio::time::sleep(QUIET + RUNS_OUT).await;
    assert_eq!(lists(), count);
    // Nothing reads the forge for it now: its requests are not listed.
    h.error(
        get(&format!("/v1/pull-requests?repo={id}")),
        StatusCode::CONFLICT,
    )
    .await;
}

#[tokio::test]
async fn wakes_during_a_fetch_coalesce_into_one_following_fetch() {
    use crate::common::{QUIET, TIMEOUT, eventually};
    use ariadne_daemon::forge::poll::Mode;
    let stub = stub_forge_cli(script(vec![], github_pull(1, "other")));
    let h = harness().forge_cli(&stub).await;
    let gate = h.dir.path().join("release-forge");
    let mut blocked = script(vec![github_pull(1, "other")], github_pull(1, "other"));
    blocked[2]["wait_for"] = json!(gate);
    stub.reprogram(blocked);
    let id = enabled_repository(&h, None).await;
    let lists = || {
        stub.invocations()
            .iter()
            .filter(|i| i.args.starts_with(&["pr".into(), "list".into()]))
            .count()
    };
    // A fetch reads its open list and its review requests together.
    eventually(TIMEOUT, "blocked first fetch", async || lists() == 2).await;
    h.state.forge_poll.set_mode(&id, Mode::WakeOnly);
    for _ in 0..5 {
        h.state.forge_poll.wake(&id);
    }
    // Prove that no second fetch starts before the gate opens.
    tokio::time::sleep(QUIET).await;
    assert_eq!(lists(), 2, "a second fetch must wait for the first");
    std::fs::write(gate, "").unwrap();
    eventually(TIMEOUT, "the one following fetch", async || lists() == 4).await;
    // Prove that the coalesced wakes start no later fetch.
    tokio::time::sleep(QUIET).await;
    assert_eq!(lists(), 4, "the five wakes must coalesce");
}

/// A request of mine is Ariadne's to work on once a task opened it (005,
/// 030): the agent of the task's `pr` column opens it, the open gives it a
/// row, the task its origin, and the list joins the row's id and the task to
/// the forge's read. The fetch that lists it alone gives it none.
#[tokio::test]
async fn a_request_of_mine_has_a_row_once_a_task_opened_it_and_not_before() {
    use crate::common::{TIMEOUT, as_session, eventually};
    use ariadne_core::{ForgeKind, Seat};
    use ariadne_daemon::forge::poll::Mode;
    use ariadne_store::SetForgeIntegration;
    let mut initial = script(vec![github_pull(42, "me")], github_pull(42, "me"));
    initial.as_array_mut().unwrap().push(answer(
        &["pr", "create"],
        0,
        "https://github.com/acme/widgets/pull/42",
    ));
    let stub = stub_forge_cli(initial);
    let h = harness().forge_cli(&stub).await;
    let checkout = h.git_repo("repo");
    let cast = h.active_cast_running(Some("develop-review-pr")).await;
    let remote = h.dir.path().join("remote.git");
    sh(
        &checkout,
        &format!(
            "git init -q --bare '{}' && git remote add origin '{}' && git checkout -b '{}' && git push -u origin HEAD",
            remote.display(),
            remote.display(),
            cast.task.branch
        ),
    );
    h.advance_to(&cast.task, "pr").await;
    let agent = h.agent_session(&cast, "pr").await;
    h.store
        .set_forge_integration(SetForgeIntegration {
            repository_id: cast.repo.id.clone(),
            kind: ForgeKind::Github,
            host: "github.com".into(),
            owner: "acme".into(),
            name: "widgets".into(),
            remote: "origin".into(),
            enabled: true,
            login: Some("me".into()),
            review_model: None,
            review_effort: None,
        })
        .await
        .unwrap();
    h.state.forge_poll.set_mode(&cast.repo.id, Mode::WakeOnly);
    eventually(TIMEOUT, "a fetch that lists the request", async || {
        stub.invocations()
            .iter()
            .any(|i| i.args.starts_with(&["pr".into(), "list".into()]))
            && stub.completed() == stub.invocations().len()
    })
    .await;
    assert!(
        h.store
            .list_pull_requests(ariadne_store::PullRequestFilter::default())
            .await
            .unwrap()
            .is_empty(),
        "listed alone, it is nobody's work"
    );
    // Only the agent of the current column opens the task's request.
    let develop = h.agent_session(&cast, "develop").await;
    h.error(
        as_session(
            &format!("/v1/tasks/{}/pull-request", cast.task.id),
            &develop.id,
            json!({"title":"Fix widgets","body":"Fix widgets."}),
        ),
        StatusCode::FORBIDDEN,
    )
    .await;
    let _: Value = h
        .json(
            as_session(
                &format!("/v1/tasks/{}/pull-request", cast.task.id),
                &agent.id,
                json!({"title":"Fix widgets","body":"Fix widgets."}),
            ),
            StatusCode::OK,
        )
        .await;
    let row = h
        .store
        .pull_request_of_task(&cast.task.id)
        .await
        .unwrap()
        .expect("the open gives it a row");
    assert_eq!(row.role, "author");
    assert_eq!(agent.seat(), Some(Seat::Agent));
    let kept: Vec<Value> = h
        .get(&format!("/v1/pull-requests?task={}", cast.task.id))
        .await;
    assert_eq!(kept.len(), 1, "the task's request");
    assert_eq!(kept[0]["id"], row.id);
    let listed: Vec<Value> = h
        .get(&format!("/v1/pull-requests?repo={}", cast.repo.id))
        .await;
    assert_eq!(listed[0]["id"], row.id);
    assert_eq!(listed[0]["origin_task_id"], cast.task.id);
}

/// A daemon that starts again reads every request Ariadne works on at once:
/// a row the open list no longer holds is read on its own, and a merge is
/// what that read finds.
#[tokio::test]
async fn startup_fetches_enabled_repositories_and_reads_a_missing_row_as_merged() {
    use crate::common::{TIMEOUT, eventually};
    use ariadne_daemon::forge::poll;
    let stub = stub_forge_cli(script(
        vec![github_pull(1, "other")],
        github_pull(1, "other"),
    ));
    let mut h = harness().forge_cli(&stub).await;
    let id = enabled_repository(&h, Some(PIN)).await;
    let row = || async {
        h.store
            .list_pull_requests(ariadne_store::PullRequestFilter::default())
            .await
            .unwrap()
            .pop()
    };
    eventually(TIMEOUT, "the row", async || row().await.is_some()).await;
    let row = row().await.unwrap();
    let mut merged = github_pull(1, "other");
    merged["state"] = json!("MERGED");
    stub.reprogram(script(vec![], merged));
    let live = ariadne_daemon::forge::live::LivePulls::default();
    let new = poll::start(
        h.store.clone(),
        h.launcher.cfg.clone(),
        &h.bus,
        live.clone(),
        h.timeouts.forge_poll,
        h.timeouts.forge_details,
    );
    // Replace the only public handle, which shuts down the old manager.
    // The router also holds a clone, so drop its state before replacement.
    h.router = axum::Router::new();
    h.state.forge_poll = new.clone();
    new.wake(&id);
    eventually(TIMEOUT, "the merge to be read", async || {
        live.get(&row.id).is_some_and(|l| l.pull.state == "merged")
    })
    .await;
}

#[tokio::test]
async fn gitlab_requests_keep_fork_checks_review_and_search_by_author_or_number() {
    let pull = json!({"iid":7,"project_id":1,"source_project_id":2,"web_url":"https://gitlab.com/group/sub/widgets/-/merge_requests/7","title":"Fix widgets","author":{"username":"other"},"state":"opened","draft":true,"source_branch":"fix","target_branch":"main","sha":"abc123","head_pipeline":{"status":"failed"},"detailed_merge_status":"mergeable","created_at":"2026-10-01T00:00:00Z"});
    let stub = stub_forge_cli(json!([
        answer(&["auth", "status"], 0, ""),
        answer(&["api", "user"], 0, "{\"username\":\"me\"}"),
        answer(&["mr", "list"], 0, &json!([pull]).to_string()),
        answer(&["mr", "view", "7"], 0, &pull.to_string()),
        answer(
            &["api", "projects/2"],
            0,
            "{\"http_url_to_repo\":\"https://gitlab.com/other/widgets.git\"}"
        ),
        answer(
            &["api", "projects/1/merge_requests/7/approvals"],
            0,
            "{\"approved\":true,\"approvals_left\":0,\"approved_by\":[{\"user\":{\"username\":\"me\"}}]}"
        ),
    ]));
    let h = harness().forge_cli(&stub).await;
    let path = h.git_repo("gitlab");
    sh(
        &path,
        "git remote add origin https://gitlab.com/group/sub/widgets.git",
    );
    let repo: Value = h
        .json(
            post_json(
                "/v1/repositories",
                json!({"path":path,"forge":{"enabled":true}}),
            ),
            StatusCode::CREATED,
        )
        .await;
    let id = repo["id"].as_str().unwrap();
    let listed: Vec<Value> = h.get(&format!("/v1/pull-requests?repo={id}")).await;
    let read = listed
        .iter()
        .find(|r| r["number"] == 7)
        .expect("the request");
    assert_eq!(read["role"], "reviewer");
    assert_eq!(read["head_repo"], "https://gitlab.com/other/widgets.git");
    assert_eq!(read["checks"], "failure");
    assert_eq!(read["review_decision"], "approved");
    assert_eq!(read["draft"], true);
    for query in ["7", "other", "widgets"] {
        let found: Vec<Value> = h
            .get(&format!(
                "/v1/repositories/{id}/pull-requests/search?q={query}"
            ))
            .await;
        assert_eq!(found.len(), 1);
        assert_eq!(found[0]["tracked"], false);
    }
    // `-R` names the project by its URL: a bare `host/group/name` would be
    // read as a group path on the CLI's default host.
    for call in stub.invocations() {
        if let Some(at) = call.args.iter().position(|a| a == "-R") {
            assert!(
                call.args[at + 1].starts_with("https://gitlab.com/"),
                "{call:?}"
            );
        }
    }
}

/// A fetch the integration is turned off during starts no work: the
/// review request it was reading gets no row.
#[tokio::test]
async fn disabling_during_a_fetch_cancels_it_without_starting_work() {
    use crate::common::{QUIET, TIMEOUT, eventually, put_json};
    use ariadne_daemon::forge::poll::Mode;
    let stub = stub_forge_cli(script(vec![], github_pull(1, "other")));
    let h = harness().forge_cli(&stub).await;
    let repo = enabled_repository(&h, Some(PIN)).await;
    h.state.forge_poll.set_mode(&repo, Mode::WakeOnly);
    let lists = || {
        stub.invocations()
            .iter()
            .filter(|i| i.args.starts_with(&["pr".into(), "list".into()]))
            .count()
    };
    eventually(TIMEOUT, "the first fetch", async || {
        lists() >= 2 && stub.completed() == stub.invocations().len()
    })
    .await;
    let gate = h.dir.path().join("release-disabled");
    let mut blocked = script(vec![github_pull(1, "other")], github_pull(1, "other"));
    blocked[2]["wait_for"] = json!(gate);
    stub.reprogram(blocked);
    h.state.forge_poll.wake(&repo);
    // A fetch reads its open list and its review requests together.
    eventually(TIMEOUT, "blocked fetch", async || lists() == 4).await;
    let _: Value = h
        .json(
            put_json(
                &format!("/v1/repositories/{repo}"),
                json!({"forge":{"enabled":false}}),
            ),
            StatusCode::OK,
        )
        .await;
    std::fs::write(gate, "").unwrap();
    h.state.forge_poll.wake(&repo);
    // Prove that the canceled fetch creates no pull request row or new fetch.
    tokio::time::sleep(QUIET).await;
    assert!(
        h.store
            .list_pull_requests(ariadne_store::PullRequestFilter::default())
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(lists(), 4);
}

/// Each fetch reads the repository's open issues too, and publishes
/// `issues_changed` where they moved since its last read (028): the first
/// read, then a new issue. A read that finds the same issues publishes
/// nothing.
#[tokio::test]
async fn a_fetch_publishes_issues_changed_only_when_the_open_issues_moved() {
    use crate::common::{QUIET, next_event};
    use ariadne_api::stream::DomainEvent;
    use ariadne_daemon::forge::poll::Mode;
    let issue = |number: i64, title: &str| {
        json!({"number": number, "title": title, "body": "", "html_url":
            format!("https://github.com/acme/widgets/issues/{number}"),
            "labels": [], "assignees": [], "updated_at": "2026-10-01T00:00:00Z"})
    };
    let with_issues = |issues: Vec<Value>| {
        let mut scripted = script(vec![], github_pull(1, "other"));
        scripted.as_array_mut().unwrap().insert(
            0,
            answer(
                &["api", "repos/acme/widgets/issues?state=open&per_page=100"],
                0,
                &serde_json::to_string(&issues).unwrap(),
            ),
        );
        scripted
    };
    let stub = stub_forge_cli(with_issues(vec![issue(7, "Fix widgets")]));
    let h = harness().forge_cli(&stub).await;
    let mut events = h.bus.subscribe();
    let id = enabled_repository(&h, None).await;
    let moved = |event: &ariadne_daemon::bus::BusEvent| matches!(&event.event, DomainEvent::IssuesChanged(i) if i.repository_id == id);
    next_event(&mut events, moved).await;
    h.state.forge_poll.set_mode(&id, Mode::WakeOnly);

    h.state.forge_poll.wake(&id);
    assert!(
        tokio::time::timeout(QUIET, next_event(&mut events, moved))
            .await
            .is_err(),
        "the same issues are no news"
    );

    stub.reprogram(with_issues(vec![
        issue(7, "Fix widgets"),
        issue(8, "Add widgets"),
    ]));
    h.state.forge_poll.wake(&id);
    next_event(&mut events, moved).await;
}

/// Each fetch publishes `pull_requests_changed` where the repository's
/// requests moved since its last read (026): the desktop reads them live,
/// so this is what tells it to read again. The same requests are no news.
#[tokio::test]
async fn a_fetch_publishes_pull_requests_changed_only_when_the_requests_moved() {
    use crate::common::{QUIET, next_event};
    use ariadne_api::stream::DomainEvent;
    use ariadne_daemon::forge::poll::Mode;
    let stub = stub_forge_cli(script(
        vec![github_pull(1, "other")],
        github_pull(1, "other"),
    ));
    let h = harness().forge_cli(&stub).await;
    let mut events = h.bus.subscribe();
    let id = enabled_repository(&h, None).await;
    let moved = |event: &ariadne_daemon::bus::BusEvent| matches!(&event.event, DomainEvent::PullRequestsChanged(p) if p.repository_id == id);
    next_event(&mut events, moved).await;
    // The first fetch both inserts the request's row — a review request
    // is tracked whether or not the repository has a review pin (029) —
    // and latches the listed/requested state `pulls_moved` watches, each
    // publishing its own event; drain the second before the quiet check
    // below, which is about a later, unchanged fetch, not this first one.
    let _ = tokio::time::timeout(QUIET, next_event(&mut events, moved)).await;
    h.state.forge_poll.set_mode(&id, Mode::WakeOnly);
    h.state.forge_poll.wake(&id);
    assert!(
        tokio::time::timeout(QUIET, next_event(&mut events, moved))
            .await
            .is_err(),
        "the same requests are no news"
    );
    let mut renamed = github_pull(1, "other");
    renamed["title"] = json!("Fix widgets again");
    stub.reprogram(script(vec![renamed.clone()], renamed));
    h.state.forge_poll.wake(&id);
    next_event(&mut events, moved).await;
}

/// A mergeability-only transition — every other hashed field unchanged —
/// still publishes `pull_requests_changed`: a client watching only the
/// hashed fields for news would otherwise miss exactly the change the
/// `pull_request` attention producer's readiness item depends on (031).
#[tokio::test]
async fn a_mergeability_only_transition_publishes_pull_requests_changed() {
    use crate::common::{QUIET, next_event};
    use ariadne_api::stream::DomainEvent;
    use ariadne_daemon::forge::poll::Mode;
    let mut unknown = github_pull(1, "other");
    unknown["mergeStateStatus"] = json!("UNKNOWN");
    let stub = stub_forge_cli(script(vec![unknown.clone()], unknown.clone()));
    let h = harness().forge_cli(&stub).await;
    let mut events = h.bus.subscribe();
    let id = enabled_repository(&h, None).await;
    let moved = |event: &ariadne_daemon::bus::BusEvent| matches!(&event.event, DomainEvent::PullRequestsChanged(p) if p.repository_id == id);
    next_event(&mut events, moved).await;
    let _ = tokio::time::timeout(QUIET, next_event(&mut events, moved)).await;
    h.state.forge_poll.set_mode(&id, Mode::WakeOnly);
    h.state.forge_poll.wake(&id);
    assert!(
        tokio::time::timeout(QUIET, next_event(&mut events, moved))
            .await
            .is_err(),
        "the same requests are no news"
    );

    // Mergeability alone moves, UNKNOWN to CLEAN: every other hashed field
    // — number, state, draft, title, head, checks, review decision,
    // updated time — reads exactly as it did.
    let mut clean = unknown.clone();
    clean["mergeStateStatus"] = json!("CLEAN");
    stub.reprogram(script(vec![clean.clone()], clean.clone()));
    h.state.forge_poll.wake(&id);
    next_event(&mut events, moved).await;
    let _ = tokio::time::timeout(QUIET, next_event(&mut events, moved)).await;

    // And the other direction, CLEAN to BLOCKED.
    let mut blocked = clean.clone();
    blocked["mergeStateStatus"] = json!("BLOCKED");
    stub.reprogram(script(vec![blocked.clone()], blocked.clone()));
    h.state.forge_poll.wake(&id);
    next_event(&mut events, moved).await;

    // An unchanged fetch after that is no news either.
    h.state.forge_poll.wake(&id);
    assert!(
        tokio::time::timeout(QUIET, next_event(&mut events, moved))
            .await
            .is_err(),
        "the same requests are no news"
    );
}

/// A detail fetch that fails at the very same head the last one succeeded
/// on still publishes `pull_requests_changed`: every hashed field of the
/// request itself reads exactly as it did, but `Live::evidence_ok` flips,
/// and a client watching only the other fields would otherwise miss
/// exactly the transition a `pull_request` readiness item depends on
/// (031). The next fetch that succeeds again, still on that head,
/// publishes the recovery the same way; an unchanged fetch after that is
/// no news either.
#[tokio::test]
async fn an_evidence_refresh_failure_and_recovery_at_the_same_head_publish_pull_requests_changed() {
    use crate::common::{QUIET, next_event};
    use ariadne_api::stream::DomainEvent;
    use ariadne_daemon::forge::poll::Mode;
    let full = script(vec![github_pull(1, "other")], github_pull(1, "other"));
    let stub = stub_forge_cli(full.clone());
    let h = harness().forge_cli(&stub).await;
    let mut events = h.bus.subscribe();
    let id = enabled_repository(&h, None).await;
    let moved = |event: &ariadne_daemon::bus::BusEvent| matches!(&event.event, DomainEvent::PullRequestsChanged(p) if p.repository_id == id);
    next_event(&mut events, moved).await;
    let _ = tokio::time::timeout(QUIET, next_event(&mut events, moved)).await;
    h.state.forge_poll.set_mode(&id, Mode::WakeOnly);

    // The next fetch finds the request's own read unchanged, but its
    // detail entries gone: the comment evidence fails to refresh.
    let failed = json!([
        answer(&["auth", "status"], 0, ""),
        answer(&["api", "user"], 0, "me"),
        answer(
            &["pr", "list"],
            0,
            &serde_json::to_string(&vec![github_pull(1, "other")]).unwrap()
        ),
        answer(&["pr", "view"], 0, &github_pull(1, "other").to_string()),
        answer(
            &["api", "repos/acme/widgets/issues?state=open&per_page=100"],
            0,
            "[]"
        ),
    ]);
    stub.reprogram(failed);
    h.state.forge_poll.wake(&id);
    next_event(&mut events, moved).await;

    // The fetch after that succeeds again, on the same head: the
    // recovery is published too.
    stub.reprogram(full);
    h.state.forge_poll.wake(&id);
    next_event(&mut events, moved).await;
    let _ = tokio::time::timeout(QUIET, next_event(&mut events, moved)).await;

    // An unchanged fetch after that is no news either.
    h.state.forge_poll.wake(&id);
    assert!(
        tokio::time::timeout(QUIET, next_event(&mut events, moved))
            .await
            .is_err(),
        "the same requests are no news"
    );
}

/// A failure of the outer list call itself — `fetch`'s own `Err` branch,
/// before any row is ever read — still publishes `pull_requests_changed`
/// for every row it invalidates: the whole round failing before a row's
/// own read ever ran is no less a reason for a client to read the
/// request's evidence again than a detail fetch failing is. The next
/// fetch that succeeds again publishes the recovery the same way.
#[tokio::test]
async fn an_outer_list_failure_publishes_pull_requests_changed() {
    use crate::common::{QUIET, next_event};
    use ariadne_api::stream::DomainEvent;
    use ariadne_daemon::forge::poll::Mode;
    let full = script(vec![github_pull(1, "other")], github_pull(1, "other"));
    let stub = stub_forge_cli(full.clone());
    let h = harness().forge_cli(&stub).await;
    let mut events = h.bus.subscribe();
    let id = enabled_repository(&h, None).await;
    let moved = |event: &ariadne_daemon::bus::BusEvent| matches!(&event.event, DomainEvent::PullRequestsChanged(p) if p.repository_id == id);
    next_event(&mut events, moved).await;
    let _ = tokio::time::timeout(QUIET, next_event(&mut events, moved)).await;
    h.state.forge_poll.set_mode(&id, Mode::WakeOnly);

    // The next fetch fails before any row is ever read: the outer list
    // call itself is gone.
    let mut failed = full.clone();
    for entry in failed.as_array_mut().unwrap() {
        if entry["args"] == json!(["pr", "list"]) {
            entry["exit"] = json!(1);
            entry["stdout"] = json!("boom");
        }
    }
    stub.reprogram(failed);
    h.state.forge_poll.wake(&id);
    next_event(&mut events, moved).await;

    // The fetch after that succeeds again: the recovery is published too.
    stub.reprogram(full);
    h.state.forge_poll.wake(&id);
    next_event(&mut events, moved).await;
}

/// A read, off the forge now, through the HTTP route itself
/// (`GET /v1/pull-requests/{id}`) rather than the poll's own cycle: a
/// failed detail read there leaves the cache's evidence no less
/// unconfirmed, through `read_now`'s own path, which the poll's worker
/// never runs, and publishes `pull_requests_changed` on the transition —
/// the same event a watcher relies on, since nothing here ever wakes the
/// poll worker. A later read that succeeds again restores it and
/// publishes the recovery the same way. Held in `WakeOnly` throughout:
/// a timer tick racing one of these direct reads would confuse which of
/// the two actually produced a given publish. Repeating the same
/// outcome — two failures, or two successes — in a row publishes once,
/// not on every call: an event-triggered panel re-read of this very
/// route must never republish forever over a state that never moved.
#[tokio::test]
async fn a_route_level_read_failure_withdraws_the_rows_evidence() {
    use crate::common::{QUIET, TIMEOUT, eventually, next_event};
    use ariadne_api::stream::DomainEvent;
    use ariadne_daemon::forge::poll::Mode;
    let stub = stub_forge_cli(script(
        vec![github_pull(2, "other")],
        github_pull(2, "other"),
    ));
    let h = harness().scheduler().forge_cli(&stub).await;
    let repo_id = enabled_repository(&h, Some(PIN)).await;
    let rows = || async {
        h.store
            .list_pull_requests(ariadne_store::PullRequestFilter::default())
            .await
            .unwrap()
    };
    eventually(TIMEOUT, "the review request's row", async || {
        rows().await.len() == 1
    })
    .await;
    let id = rows().await.pop().unwrap().id;
    eventually(TIMEOUT, "the row's first live read", async || {
        h.launcher.live.get(&id).is_some()
    })
    .await;
    assert!(h.launcher.live.get(&id).unwrap().evidence_ok);
    h.state.forge_poll.set_mode(&repo_id, Mode::WakeOnly);
    let mut events = h.bus.subscribe();
    let moved = |event: &ariadne_daemon::bus::BusEvent| matches!(&event.event, DomainEvent::PullRequestsChanged(p) if p.repository_id == repo_id);

    let mut failed = script(vec![github_pull(2, "other")], github_pull(2, "other"));
    for entry in failed.as_array_mut().unwrap() {
        if entry["args"] == json!(["pr", "view"]) {
            entry["exit"] = json!(1);
            entry["stdout"] = json!("boom");
        }
    }
    stub.reprogram(failed);
    let error = h
        .error(
            get(&format!("/v1/pull-requests/{id}")),
            StatusCode::BAD_GATEWAY,
        )
        .await;
    assert!(!error.error.message.is_empty());
    assert!(
        !h.launcher.live.get(&id).unwrap().evidence_ok,
        "a direct read failure through the route withdraws the cached evidence"
    );
    next_event(&mut events, moved).await;

    // The same failure again changes nothing: no second publish.
    let _ = h
        .error(
            get(&format!("/v1/pull-requests/{id}")),
            StatusCode::BAD_GATEWAY,
        )
        .await;
    assert!(
        tokio::time::timeout(QUIET, next_event(&mut events, moved))
            .await
            .is_err(),
        "a repeated, unchanged failure publishes nothing more"
    );

    stub.reprogram(script(
        vec![github_pull(2, "other")],
        github_pull(2, "other"),
    ));
    let _: Value = h.get(&format!("/v1/pull-requests/{id}")).await;
    assert!(
        h.launcher.live.get(&id).unwrap().evidence_ok,
        "a read that succeeds again restores it"
    );
    next_event(&mut events, moved).await;

    // The same success again changes nothing: no second publish.
    let _: Value = h.get(&format!("/v1/pull-requests/{id}")).await;
    assert!(
        tokio::time::timeout(QUIET, next_event(&mut events, moved))
            .await
            .is_err(),
        "a repeated, unchanged success publishes nothing more"
    );
}
