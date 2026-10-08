//! The ledger through HTTP and the forge CLI boundary (026).
use crate::common::forge::{answer, stub_forge_cli};
use crate::common::{delete, get, harness, post_json, sh};
use axum::http::StatusCode;
use serde_json::{Value, json};

fn github_pull(number: i64, author: &str) -> Value {
    json!({"number": number, "url": format!("https://github.com/acme/widgets/pull/{number}"),
        "title": "Fix widgets", "author": {"login": author}, "state": "OPEN", "isDraft": false,
        "headRefName": "fix", "headRefOid": "abc", "headRepository": {"url": "https://github.com/acme/widgets"},
        "baseRefName": "main", "statusCheckRollup": [], "reviewDecision": "", "comments": [],
        "createdAt": "2026-10-01T00:00:00Z"})
}

#[tokio::test]
async fn adding_a_url_variant_keeps_one_row_and_user_tracking() {
    let pull = github_pull(42, "me").to_string();
    let stub = stub_forge_cli(json!([
        answer(&["auth", "status"], 0, ""),
        answer(&["api", "user"], 0, "me"),
        answer(&["pr", "list"], 0, "[]"),
        answer(&["pr", "view", "42"], 0, &pull),
    ]));
    let h = harness().forge_cli(&stub).await;
    let path = h.git_repo("pull-requests");
    sh(
        &path,
        "git remote add origin https://github.com/acme/widgets.git",
    );
    let repo: Value = h
        .json(
            post_json(
                "/v1/repositories",
                json!({"path": path, "forge": {"enabled": true}}),
            ),
            StatusCode::CREATED,
        )
        .await;
    let first: Value = h
        .json(
            post_json(
                "/v1/pull-requests",
                json!({"url": "http://GITHUB.com/ACME/Widgets/pull/42/?x=y#comment"}),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert_eq!(first["role"], "author");
    assert_eq!(first["tracked_by"], "user");
    let second: Value = h
        .json(
            post_json(
                "/v1/pull-requests",
                json!({"repository_id": repo["id"], "number": 42}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(first["id"], second["id"]);
    let rows: Vec<Value> = h.json(get("/v1/pull-requests"), StatusCode::OK).await;
    assert_eq!(rows.len(), 1);
    let id = first["id"].as_str().unwrap();
    assert_eq!(
        h.send(delete(&format!("/v1/pull-requests/{id}"))).await.0,
        StatusCode::NO_CONTENT
    );
}

async fn enabled_repository(h: &crate::common::Harness) -> String {
    let path = h.git_repo("enabled");
    sh(
        &path,
        "git remote add origin https://github.com/acme/widgets.git",
    );
    let repo: Value = h
        .json(
            post_json(
                "/v1/repositories",
                json!({"path": path, "forge": {"enabled":true}}),
            ),
            StatusCode::CREATED,
        )
        .await;
    repo["id"].as_str().unwrap().into()
}
fn script(list: Vec<Value>, view: Value) -> Value {
    json!([
        answer(&["auth", "status"], 0, ""),
        answer(&["api", "user"], 0, "me"),
        answer(&["pr", "list"], 0, &serde_json::to_string(&list).unwrap()),
        answer(&["pr", "view"], 0, &view.to_string()),
    ])
}

#[tokio::test]
async fn fetch_tracks_roles_once_reads_missing_rows_and_closes_only_from_the_forge() {
    use crate::common::{TIMEOUT, eventually, next_event};
    use ariadne_api::stream::DomainEvent;
    use ariadne_daemon::forge::poll::Mode;
    let stub = stub_forge_cli(script(
        vec![github_pull(1, "me"), github_pull(2, "other")],
        github_pull(1, "me"),
    ));
    let h = harness().forge_cli(&stub).await;
    let mut events = h.bus.subscribe();
    let id = enabled_repository(&h).await;
    for _ in 0..2 {
        let event = next_event(&mut events, |e| e.event.kind().starts_with("pull_request_")).await;
        assert!(matches!(event.event, DomainEvent::PullRequestCreated(_)));
    }
    let rows: Vec<Value> = h.get("/v1/pull-requests").await;
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows.iter().find(|r| r["number"] == 1).unwrap()["role"],
        "author"
    );
    assert_eq!(
        rows.iter().find(|r| r["number"] == 2).unwrap()["role"],
        "reviewer"
    );
    let forge_id = rows.iter().find(|r| r["number"] == 2).unwrap()["id"]
        .as_str()
        .unwrap();
    h.error(
        delete(&format!("/v1/pull-requests/{forge_id}")),
        StatusCode::CONFLICT,
    )
    .await;
    h.state.forge_poll.set_mode(&id, Mode::WakeOnly);
    h.state.forge_poll.wake(&id);
    for _ in 0..2 {
        let event = next_event(&mut events, |e| e.event.kind().starts_with("pull_request_")).await;
        assert!(matches!(event.event, DomainEvent::PullRequestUpdated(_)));
    }
    let matches: Vec<Value> = h
        .get(&format!(
            "/v1/repositories/{id}/pull-requests/search?q=widgets"
        ))
        .await;
    assert_eq!(matches.len(), 2);
    assert!(matches.iter().all(|r| r["tracked"] == true));
    let added: Value = h
        .json(
            post_json("/v1/pull-requests", json!({"repository_id":id,"number":1})),
            StatusCode::OK,
        )
        .await;
    assert_eq!(added["tracked_by"], "user");
    next_event(&mut events, |e| e.event.kind() == "pull_request_updated").await;
    stub.reprogram(script(vec![github_pull(2, "other")], github_pull(1, "me")));
    h.state.forge_poll.wake(&id);
    next_event(
        &mut events,
        |e| matches!(&e.event,DomainEvent::PullRequestUpdated(p) if p.number == 1),
    )
    .await;
    let row: Value = h
        .get(&format!(
            "/v1/pull-requests/{}",
            added["id"].as_str().unwrap()
        ))
        .await;
    assert_eq!(row["state"], "open");
    assert_eq!(row["tracked_by"], "user");
    let mut merged = github_pull(1, "me");
    merged["state"] = json!("MERGED");
    stub.reprogram(script(vec![github_pull(2, "other")], merged));
    assert_eq!(
        h.send(post_json(
            &format!("/v1/pull-requests/refresh?repo={id}"),
            json!({})
        ))
        .await
        .0,
        StatusCode::ACCEPTED
    );
    eventually(TIMEOUT, "merged request", async || {
        let rows: Vec<Value> = h
            .get(&format!(
                "/v1/pull-requests?repo={id}&role=author&state=merged"
            ))
            .await;
        rows.len() == 1 && rows[0]["number"] == 1
    })
    .await;
    let open: Vec<Value> = h.get("/v1/pull-requests").await;
    assert_eq!(open.len(), 1);
    let all: Vec<Value> = h.get("/v1/pull-requests?state=all").await;
    assert_eq!(all.len(), 2);
}

#[tokio::test]
async fn timer_wake_only_and_disable_control_repository_fetches() {
    use crate::common::{QUIET, RUNS_OUT, TIMEOUT, eventually, put_json};
    use ariadne_daemon::{forge::poll::Mode, timeouts::Timeouts};
    let stub = stub_forge_cli(script(vec![github_pull(1, "me")], github_pull(1, "me")));
    let h = harness()
        .forge_cli(&stub)
        .timeouts(Timeouts {
            forge_poll: RUNS_OUT,
            ..Default::default()
        })
        .await;
    let id = enabled_repository(&h).await;
    let lists = || {
        stub.invocations()
            .iter()
            .filter(|i| i.args.starts_with(&["pr".into(), "list".into()]))
            .count()
    };
    eventually(TIMEOUT, "enable and timer fetch", async || lists() >= 4).await;
    h.state.forge_poll.set_mode(&id, Mode::WakeOnly);
    // The fetch after the mode change is told apart by what it reads: a
    // title no earlier fetch wrote. It has ended once its row carries that
    // title and no forge call is still running.
    let mut marked = github_pull(1, "me");
    marked["title"] = json!("Fix widgets again");
    stub.reprogram(script(vec![marked.clone()], marked));
    h.state.forge_poll.wake(&id);
    let idle = || stub.completed() == stub.invocations().len();
    eventually(TIMEOUT, "the fetch after the mode change", async || {
        let rows: Vec<Value> = h.get("/v1/pull-requests").await;
        rows.iter().any(|r| r["title"] == "Fix widgets again") && idle()
    })
    .await;
    let count = lists();
    tokio::time::sleep(QUIET + RUNS_OUT).await;
    assert_eq!(lists(), count, "WakeOnly must not use the timer");
    h.state.forge_poll.wake(&id);
    eventually(TIMEOUT, "the wake's fetch", async || {
        lists() >= count + 2 && idle()
    })
    .await;
    tokio::time::sleep(QUIET).await;
    assert_eq!(
        lists(),
        count + 2,
        "one wake lists author and reviewer requests once"
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
    let closed: Vec<Value> = h.get("/v1/pull-requests?state=closed").await;
    assert_eq!(closed.len(), 1);
    let count = lists();
    h.state.forge_poll.wake(&id);
    tokio::time::sleep(QUIET + RUNS_OUT).await;
    assert_eq!(lists(), count);
    h.error(
        post_json("/v1/pull-requests", json!({"repository_id":id,"number":2})),
        StatusCode::CONFLICT,
    )
    .await;
    h.error(
        post_json(
            "/v1/pull-requests",
            json!({"url":"https://github.com/acme/widgets/pull/2"}),
        ),
        StatusCode::NOT_FOUND,
    )
    .await;
}

#[tokio::test]
async fn wakes_during_a_fetch_coalesce_into_one_following_fetch() {
    use crate::common::{QUIET, TIMEOUT, eventually};
    use ariadne_daemon::forge::poll::Mode;
    let stub = stub_forge_cli(script(vec![], github_pull(1, "me")));
    let h = harness().forge_cli(&stub).await;
    let gate = h.dir.path().join("release-forge");
    let mut blocked = script(vec![github_pull(1, "me")], github_pull(1, "me"));
    blocked[2]["wait_for"] = json!(gate);
    stub.reprogram(blocked);
    let id = enabled_repository(&h).await;
    let lists = || {
        stub.invocations()
            .iter()
            .filter(|i| i.args.starts_with(&["pr".into(), "list".into()]))
            .count()
    };
    eventually(TIMEOUT, "blocked first fetch", async || lists() == 1).await;
    h.state.forge_poll.set_mode(&id, Mode::WakeOnly);
    for _ in 0..5 {
        h.state.forge_poll.wake(&id);
    }
    tokio::time::sleep(QUIET).await;
    assert_eq!(lists(), 1, "a second fetch must wait for the first");
    let mut events = h.bus.subscribe();
    std::fs::write(gate, "").unwrap();
    crate::common::next_event(&mut events, |e| e.event.kind() == "pull_request_created").await;
    crate::common::next_event(&mut events, |e| e.event.kind() == "pull_request_updated").await;
    assert_eq!(lists(), 4);
    tokio::time::sleep(QUIET).await;
    assert_eq!(lists(), 4, "the five wakes must coalesce");
}

#[tokio::test]
async fn all_three_births_keep_one_identity_in_every_order() {
    use crate::common::{as_session, next_event};
    use ariadne_api::stream::DomainEvent;
    use ariadne_core::{Actor, ForgeKind, Landing, Seat, TaskStatus};
    use ariadne_daemon::forge::poll::Mode;
    use ariadne_store::SetForgeIntegration;
    for order in [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ] {
        let mut initial = script(vec![], github_pull(42, "me"));
        initial.as_array_mut().unwrap().push(answer(
            &["pr", "create"],
            0,
            "https://github.com/acme/widgets/pull/42",
        ));
        let stub = stub_forge_cli(initial.clone());
        let h = harness().forge_cli(&stub).await;
        let checkout = h.git_repo("repo");
        let cast = h.active_cast_ending_in(Landing::PullRequest).await;
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
        for (status, actor) in [
            (TaskStatus::Ready, Actor::Daemon),
            (TaskStatus::InProgress, Actor::Daemon),
            (TaskStatus::UnderReview, Actor::Author),
            (TaskStatus::Approved, Actor::Daemon),
        ] {
            h.store
                .transition_task(&cast.task.id, status, actor, None, None)
                .await
                .unwrap();
        }
        let author = h
            .session(&cast.goal, Some(&cast.task), Seat::Author, &cast.author.id)
            .await;
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
                babysit_model: None,
                babysit_effort: None,
                review_model: None,
                review_effort: None,
            })
            .await
            .unwrap();
        h.state.forge_poll.set_mode(&cast.repo.id, Mode::WakeOnly);
        crate::common::eventually(crate::common::TIMEOUT, "initial empty fetch", async || {
            stub.completed() >= 2
        })
        .await;
        let mut events = h.bus.subscribe();
        for (index, birth) in order.into_iter().enumerate() {
            match birth {
                0 => {
                    let _: Value = h
                        .json(
                            as_session(
                                &format!("/v1/tasks/{}/pull-request", cast.task.id),
                                &author.id,
                                json!({"title":"Fix widgets","body":"Fix widgets."}),
                            ),
                            StatusCode::OK,
                        )
                        .await;
                }
                1 => {
                    initial[2]["stdout"] =
                        json!(serde_json::to_string(&vec![github_pull(42, "me")]).unwrap());
                    stub.reprogram(initial.clone());
                    h.state.forge_poll.wake(&cast.repo.id);
                }
                _ => {
                    let _:Value=h.json(post_json("/v1/pull-requests",json!({"url":"http://GITHUB.com/ACME/Widgets/pull/42/?q=x#comment"})),if index==0 {StatusCode::CREATED}else{StatusCode::OK}).await;
                }
            }
            let event =
                next_event(&mut events, |e| e.event.kind().starts_with("pull_request_")).await;
            if index == 0 {
                assert!(
                    matches!(event.event, DomainEvent::PullRequestCreated(_)),
                    "{order:?}"
                );
            } else {
                assert!(
                    matches!(event.event, DomainEvent::PullRequestUpdated(_)),
                    "{order:?}"
                );
            }
        }
        h.state.forge_poll.wake(&cast.repo.id);
        next_event(&mut events, |e| e.event.kind() == "pull_request_updated").await;
        let rows: Vec<Value> = h.get("/v1/pull-requests").await;
        assert_eq!(rows.len(), 1, "{order:?}");
        assert_eq!(rows[0]["origin_task_id"], cast.task.id);
        assert_eq!(rows[0]["tracked_by"], "user");
        assert_eq!(rows[0]["role"], "author");
    }
}

#[tokio::test]
async fn startup_fetches_enabled_repositories_and_reads_a_missing_forge_row_as_merged() {
    use crate::common::{TIMEOUT, eventually, next_event};
    use ariadne_daemon::forge::poll;
    let stub = stub_forge_cli(script(vec![github_pull(1, "me")], github_pull(1, "me")));
    let mut h = harness().forge_cli(&stub).await;
    let id = enabled_repository(&h).await;
    eventually(TIMEOUT, "first row", async || {
        h.get::<Vec<Value>>("/v1/pull-requests").await.len() == 1
    })
    .await;
    let new = poll::start(
        h.store.clone(),
        h.launcher.cfg.clone(),
        &h.bus,
        h.timeouts.forge_poll,
        h.timeouts.forge_details,
    );
    // Replace the only public handle, which shuts down the old manager.
    // The router also holds a clone, so drop its state before replacement.
    h.router = axum::Router::new();
    h.state.forge_poll = new.clone();
    let mut events = h.bus.subscribe();
    next_event(&mut events, |e| e.event.kind() == "pull_request_updated").await;
    let mut merged = github_pull(1, "me");
    merged["state"] = json!("MERGED");
    stub.reprogram(script(vec![], merged));
    new.wake(&id);
    let event=next_event(&mut events,|e| matches!(&e.event,ariadne_api::stream::DomainEvent::PullRequestUpdated(p) if p.state=="merged")).await;
    assert_eq!(event.event.payload()["tracked_by"], "forge");
}

#[tokio::test]
async fn gitlab_requests_keep_fork_checks_review_and_search_by_author_or_number() {
    let pull = json!({"iid":7,"project_id":1,"source_project_id":2,"web_url":"https://gitlab.com/group/sub/widgets/-/merge_requests/7","title":"Fix widgets","author":{"username":"other"},"state":"opened","draft":true,"source_branch":"fix","target_branch":"main","sha":"abc123","head_pipeline":{"status":"failed"},"detailed_merge_status":"mergeable","created_at":"2026-10-01T00:00:00Z"});
    let stub = stub_forge_cli(json!([
        answer(&["auth", "status"], 0, ""),
        answer(&["api", "user"], 0, "{\"username\":\"me\"}"),
        answer(&["mr", "list"], 0, "[]"),
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
    let added: Value = h
        .json(
            post_json(
                "/v1/pull-requests",
                json!({"url":"http://GITLAB.com/GROUP/Sub/Widgets/merge_requests/7/#note"}),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert_eq!(added["role"], "reviewer");
    assert_eq!(added["tracked_by"], "user");
    assert_eq!(added["head_repo"], "https://gitlab.com/other/widgets.git");
    assert_eq!(added["checks"], "failure");
    assert_eq!(added["review_decision"], "approved");
    assert_eq!(added["draft"], true);
    let script = json!([
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
            "{\"approved\":true,\"approvals_left\":0,\"approved_by\":[{}]}"
        ),
    ]);
    stub.reprogram(script);
    for query in ["7", "other", "widgets"] {
        let found: Vec<Value> = h
            .get(&format!(
                "/v1/repositories/{}/pull-requests/search?q={query}",
                repo["id"].as_str().unwrap()
            ))
            .await;
        assert_eq!(found.len(), 1);
        assert_eq!(found[0]["tracked"], true);
    }
}

#[tokio::test]
async fn removing_a_user_row_during_its_read_does_not_recreate_it() {
    use crate::common::{QUIET, TIMEOUT, eventually, next_event};
    use ariadne_daemon::forge::poll::Mode;
    let stub = stub_forge_cli(script(vec![], github_pull(42, "other")));
    let h = harness().forge_cli(&stub).await;
    let repo = enabled_repository(&h).await;
    h.state.forge_poll.set_mode(&repo, Mode::WakeOnly);
    eventually(TIMEOUT, "initial empty fetch", async || {
        stub.completed() >= 4
    })
    .await;
    let row: Value = h
        .json(
            post_json(
                "/v1/pull-requests",
                json!({"repository_id":repo,"number":42}),
            ),
            StatusCode::CREATED,
        )
        .await;
    let gate = h.dir.path().join("release-read");
    let mut blocked = script(vec![], github_pull(42, "other"));
    blocked[3]["wait_for"] = json!(gate);
    stub.reprogram(blocked);
    h.state.forge_poll.wake(&repo);
    eventually(TIMEOUT, "individual read", async || {
        stub.invocations()
            .iter()
            .filter(|i| i.args.starts_with(&["pr".into(), "view".into()]))
            .count()
            == 2
    })
    .await;
    let mut events = h.bus.subscribe();
    assert_eq!(
        h.send(delete(&format!(
            "/v1/pull-requests/{}",
            row["id"].as_str().unwrap()
        )))
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    next_event(&mut events, |e| e.event.kind() == "pull_request_deleted").await;
    std::fs::write(gate, "").unwrap();
    assert!(
        tokio::time::timeout(
            QUIET,
            next_event(&mut events, |e| e.event.kind().starts_with("pull_request_"))
        )
        .await
        .is_err()
    );
    assert!(
        h.get::<Vec<Value>>("/v1/pull-requests?state=all")
            .await
            .is_empty()
    );
}

#[tokio::test]
async fn disabling_during_a_fetch_cancels_it_without_reopening_rows() {
    use crate::common::{QUIET, TIMEOUT, eventually, next_event, put_json};
    use ariadne_daemon::forge::poll::Mode;
    let stub = stub_forge_cli(script(vec![github_pull(1, "me")], github_pull(1, "me")));
    let h = harness().forge_cli(&stub).await;
    let repo = enabled_repository(&h).await;
    h.state.forge_poll.set_mode(&repo, Mode::WakeOnly);
    eventually(TIMEOUT, "first row", async || {
        h.get::<Vec<Value>>("/v1/pull-requests").await.len() == 1
    })
    .await;
    let gate = h.dir.path().join("release-disabled");
    let mut blocked = script(vec![github_pull(1, "me")], github_pull(1, "me"));
    blocked[2]["wait_for"] = json!(gate);
    stub.reprogram(blocked);
    h.state.forge_poll.wake(&repo);
    let lists = || {
        stub.invocations()
            .iter()
            .filter(|i| i.args.starts_with(&["pr".into(), "list".into()]))
            .count()
    };
    eventually(TIMEOUT, "blocked fetch", async || lists() == 3).await;
    let mut events = h.bus.subscribe();
    let _: Value = h
        .json(
            put_json(
                &format!("/v1/repositories/{repo}"),
                json!({"forge":{"enabled":false}}),
            ),
            StatusCode::OK,
        )
        .await;
    next_event(&mut events, |e| e.event.kind() == "pull_request_updated").await;
    std::fs::write(gate, "").unwrap();
    h.state.forge_poll.wake(&repo);
    assert!(
        tokio::time::timeout(
            QUIET,
            next_event(&mut events, |e| e.event.kind().starts_with("pull_request_"))
        )
        .await
        .is_err()
    );
    let rows: Vec<Value> = h.get("/v1/pull-requests?state=closed").await;
    assert_eq!(rows.len(), 1);
    assert_eq!(lists(), 3);
}
