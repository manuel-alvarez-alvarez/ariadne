//! Signed deliveries and hook lifecycle through the dedicated listener (027).
use crate::common::forge::{StubForgeCli, answer, stub_forge_cli};
use crate::common::{QUIET, TIMEOUT, eventually, harness, post_json, put_json, sh};
use ariadne_daemon::webhooks::WebhookListen;
use axum::http::StatusCode;
use serde_json::{Value, json};

#[tokio::test]
async fn the_listener_binds_a_separate_random_or_fixed_port_and_serves_no_api() {
    let h = harness().await;
    let listen = WebhookListen::bind(&h.launcher.cfg, h.store.clone(), h.state.forge_poll.clone())
        .await
        .unwrap();
    let api = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    assert_ne!(listen.address().port(), 0);
    assert_ne!(listen.address(), api.local_addr().unwrap());
    let api_address = api.local_addr().unwrap();
    let router = h.router.clone();
    let api_task = tokio::spawn(async move {
        axum::serve(api, router).await.unwrap();
    });
    let client = reqwest::Client::new();
    assert_eq!(
        client
            .get(format!("http://{api_address}/v1/goals"))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    for path in ["/v1/goals", "/docs", "/api-docs/openapi.json"] {
        assert_eq!(
            client
                .get(format!("http://{}{path}", listen.address()))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::NOT_FOUND
        );
    }
    let reservation = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = reservation.local_addr().unwrap();
    drop(reservation);
    let mut cfg = (*h.launcher.cfg).clone();
    cfg.webhook_listen = Some(address);
    cfg.tcp_listen = Some(api_address);
    let fixed = WebhookListen::bind(&cfg, h.store.clone(), h.state.forge_poll.clone())
        .await
        .unwrap();
    assert_eq!(fixed.address(), address);
    let response = client
        .request(
            reqwest::Method::OPTIONS,
            format!("http://{}/webhooks/github/repo", fixed.address()),
        )
        .header("Origin", "https://example.com")
        .header("Access-Control-Request-Method", "POST")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert!(
        response
            .headers()
            .get("access-control-allow-origin")
            .is_none()
    );
    api_task.abort();
}

fn script(gitlab: bool) -> Value {
    json!([
        answer(&["auth", "status"], 0, ""),
        answer(
            &["api", "user"],
            0,
            if gitlab { r#"{"username":"me"}"# } else { "me" }
        ),
        answer(&["pr", "list"], 0, "[]"),
        answer(&["mr", "list"], 0, "[]"),
        answer(&["api"], 0, r#"{"id":17}"#),
    ])
}

async fn enable(h: &crate::common::Harness, gitlab: bool) -> String {
    let path = h.git_repo("webhooks");
    sh(
        &path,
        if gitlab {
            "git remote add origin https://gitlab.com/acme/widgets.git"
        } else {
            "git remote add origin https://github.com/acme/widgets.git"
        },
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
    repo["id"].as_str().unwrap().into()
}

fn fetches(stub: &StubForgeCli) -> usize {
    // Each repository fetch lists authored requests and review requests.
    stub.invocations()
        .iter()
        .filter(|call| call.args.get(1).is_some_and(|a| a == "list"))
        .count()
        / 2
}

async fn quiet(stub: &StubForgeCli, count: usize) {
    assert!(
        tokio::time::timeout(QUIET, async {
            loop {
                assert_eq!(fetches(stub), count);
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .is_err()
    );
}

async fn deliveries(gitlab: bool) {
    let stub = stub_forge_cli(script(gitlab));
    let home = tempfile::tempdir().unwrap();
    std::fs::write(
        home.path().join("config.toml"),
        "webhook_public_url = \"https://hooks.example\"\n",
    )
    .unwrap();
    let h = harness()
        .home(home.path().to_path_buf())
        .forge_cli(&stub)
        .timeouts(ariadne_daemon::timeouts::Timeouts {
            forge_poll: crate::common::RUNS_OUT,
            ..Default::default()
        })
        .await;
    let listener =
        WebhookListen::bind(&h.launcher.cfg, h.store.clone(), h.state.forge_poll.clone())
            .await
            .unwrap();
    let id = enable(&h, gitlab).await;
    eventually(TIMEOUT, "webhook state or fetch", async || {
        fetches(&stub) == 1
    })
    .await;
    let row = h.store.forge_integration(&id).await.unwrap().unwrap();
    assert_eq!(row.webhook_state, "live");
    assert_eq!(row.webhook_id, Some(17));
    let secret = row.webhook_secret.unwrap();
    assert_eq!(secret.len(), 64);
    assert!(secret.bytes().all(|b| b.is_ascii_hexdigit()));
    quiet(&stub, 1).await;
    let kind = if gitlab { "gitlab" } else { "github" };
    let url = format!("http://{}/webhooks/{kind}/{id}", listener.address());
    let body = b"{\"action\":\"opened\"}";
    let signature = if gitlab {
        secret.clone()
    } else {
        // Python provides an independent signature implementation.
        let output = std::process::Command::new("python3").args(["-c", "import hmac,hashlib,sys;print('sha256='+hmac.new(sys.argv[1].encode(),b'{\"action\":\"opened\"}',hashlib.sha256).hexdigest())", &secret]).output().unwrap();
        String::from_utf8(output.stdout).unwrap().trim().into()
    };
    let header = if gitlab {
        "X-Gitlab-Token"
    } else {
        "X-Hub-Signature-256"
    };
    let client = reqwest::Client::new();
    assert_eq!(
        client
            .post(&url)
            .header(header, "wrong")
            .body(body.to_vec())
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    quiet(&stub, 1).await;
    assert_eq!(
        client
            .post(&url)
            .body(body.to_vec())
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    if !gitlab {
        assert_eq!(
            client
                .post(&url)
                .header(header, &signature)
                .body("changed body")
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }
    assert!(
        h.store
            .forge_integration(&id)
            .await
            .unwrap()
            .unwrap()
            .webhook_last_delivery_at
            .is_none()
    );
    assert_eq!(
        client
            .post(&url)
            .header(header, &signature)
            .header("X-Github-Event", "ping")
            .header("X-Gitlab-Event", "ping")
            .body(body.to_vec())
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::ACCEPTED
    );
    quiet(&stub, 1).await;
    assert_eq!(
        client
            .post(&url)
            .header(header, &signature)
            .body(body.to_vec())
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::ACCEPTED
    );
    eventually(TIMEOUT, "webhook state or fetch", async || {
        fetches(&stub) == 2
    })
    .await;
    assert!(
        h.store
            .forge_integration(&id)
            .await
            .unwrap()
            .unwrap()
            .webhook_last_delivery_at
            .is_some()
    );
    assert_eq!(
        client
            .post(&url)
            .header(header, &signature)
            .body(vec![b'x'; 1024 * 1024 + 1])
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
    quiet(&stub, 2).await;
    let dto: Value = h.get(&format!("/v1/repositories/{id}")).await;
    assert_eq!(dto["forge"]["webhook"]["state"], "live");
    assert!(!dto.to_string().contains(&secret));
    let create = stub
        .invocations()
        .into_iter()
        .find(|c| c.args.iter().any(|a| a == "POST"))
        .unwrap();
    assert!(create.args.iter().any(|a| a.contains(&secret)));
    let expected_events: &[&str] = if gitlab {
        &[
            "merge_requests_events=true",
            "note_events=true",
            "pipeline_events=true",
            "issues_events=true",
        ]
    } else {
        &[
            "events[]=pull_request",
            "events[]=pull_request_review",
            "events[]=pull_request_review_comment",
            "events[]=issue_comment",
            "events[]=check_suite",
            "events[]=check_run",
            "events[]=status",
            "events[]=issues",
        ]
    };
    for event in expected_events {
        assert!(
            create.args.iter().any(|a| a == event),
            "missing event {event}"
        );
    }
    assert_eq!(
        create.args[1],
        if gitlab {
            "projects/acme%2Fwidgets/hooks"
        } else {
            "repos/acme/widgets/hooks"
        }
    );

    assert!(
        create
            .args
            .iter()
            .any(|a| a.contains(&format!("https://hooks.example/webhooks/{kind}/{id}")))
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
    assert_eq!(
        client
            .post(&url)
            .header(header, &signature)
            .body(body.to_vec())
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    quiet(&stub, 2).await;
    assert!(
        stub.invocations()
            .iter()
            .any(|c| c.args.iter().any(|a| a == "DELETE"))
    );
}

#[tokio::test]
async fn github_signed_deliveries_wake_once_and_bad_signatures_ping_and_large_bodies_do_not() {
    deliveries(false).await;
}
#[tokio::test]
async fn gitlab_tokens_wake_once_and_bad_tokens_ping_and_large_bodies_do_not() {
    deliveries(true).await;
}

#[tokio::test]
async fn url_changes_update_hooks_withdrawal_restores_timer_and_disable_deletes() {
    url_changes(false).await;
}

#[tokio::test]
async fn gitlab_url_updates_preserve_the_token_and_withdrawal_restores_timer() {
    url_changes(true).await;
}

async fn url_changes(gitlab: bool) {
    let stub = stub_forge_cli(script(gitlab));
    let h = harness()
        .forge_cli(&stub)
        .timeouts(ariadne_daemon::timeouts::Timeouts {
            forge_poll: crate::common::RUNS_OUT,
            ..Default::default()
        })
        .await;
    let url = h.state.forge_poll.webhook_url();
    url.set(Some("https://first.example".into()));
    let id = enable(&h, gitlab).await;
    eventually(TIMEOUT, "webhook state or fetch", async || {
        fetches(&stub) == 1
    })
    .await;
    let kind = if gitlab { "gitlab" } else { "github" };
    url.set(Some("https://second.example".into()));
    eventually(TIMEOUT, "webhook state or fetch", async || {
        h.store
            .forge_integration(&id)
            .await
            .unwrap()
            .unwrap()
            .webhook_url
            .as_deref()
            == Some(&format!("https://second.example/webhooks/{kind}/{id}"))
    })
    .await;
    assert!(stub.invocations().iter().any(|c| {
        c.args
            .iter()
            .any(|a| a == if gitlab { "PUT" } else { "PATCH" })
    }));
    if gitlab {
        let secret = h
            .store
            .forge_integration(&id)
            .await
            .unwrap()
            .unwrap()
            .webhook_secret
            .unwrap();
        let update = stub
            .invocations()
            .into_iter()
            .find(|c| c.args.iter().any(|a| a == "PUT"))
            .unwrap();
        assert!(update.args.contains(&format!("token={secret}")));
    }
    eventually(TIMEOUT, "webhook state or fetch", async || {
        fetches(&stub) == 2
    })
    .await;
    url.set(None);
    eventually(TIMEOUT, "webhook state or fetch", async || {
        h.store
            .forge_integration(&id)
            .await
            .unwrap()
            .unwrap()
            .webhook_state
            == "polling"
            && fetches(&stub) == 3
    })
    .await;
    assert!(
        h.store
            .forge_integration(&id)
            .await
            .unwrap()
            .unwrap()
            .webhook_error
            .is_none()
    );
    eventually(TIMEOUT, "timer after withdrawal", async || {
        fetches(&stub) >= 4
    })
    .await;
    url.set(Some("https://second.example".into()));
    eventually(TIMEOUT, "webhook state or fetch", async || {
        h.store
            .forge_integration(&id)
            .await
            .unwrap()
            .unwrap()
            .webhook_state
            == "live"
    })
    .await;
    let listener =
        WebhookListen::bind(&h.launcher.cfg, h.store.clone(), h.state.forge_poll.clone())
            .await
            .unwrap();
    let _: Value = h
        .json(
            put_json(
                &format!("/v1/repositories/{id}"),
                json!({"forge":{"enabled":false}}),
            ),
            StatusCode::OK,
        )
        .await;
    assert!(
        stub.invocations()
            .iter()
            .any(|c| c.args.iter().any(|a| a == "DELETE"))
    );
    assert!(
        h.store
            .forge_integration(&id)
            .await
            .unwrap()
            .unwrap()
            .webhook_id
            .is_none()
    );
    assert_eq!(
        reqwest::Client::new()
            .post(format!(
                "http://{}/webhooks/github/{id}",
                listener.address()
            ))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn a_refused_hook_keeps_the_forge_message_and_timer_fetches() {
    let mut entries = script(false).as_array().unwrap().clone();
    entries.pop();
    entries.push(json!({"args":["api"],"exit":1,"stderr":"HTTP 403: needs admin rights"}));
    let stub = stub_forge_cli(Value::Array(entries));
    let h = harness()
        .forge_cli(&stub)
        .timeouts(ariadne_daemon::timeouts::Timeouts {
            forge_poll: crate::common::RUNS_OUT,
            ..Default::default()
        })
        .await;
    h.state
        .forge_poll
        .webhook_url()
        .set(Some("https://hooks.example".into()));
    let id = enable(&h, false).await;
    let row = h.store.forge_integration(&id).await.unwrap().unwrap();
    assert_eq!(row.webhook_state, "polling");
    assert!(row.webhook_error.unwrap().contains("needs admin rights"));
    eventually(TIMEOUT, "webhook state or fetch", async || {
        fetches(&stub) >= 2
    })
    .await;
    assert_eq!(
        stub.invocations()
            .iter()
            .filter(|c| c.args.iter().any(|a| a == "POST"))
            .count(),
        1
    );
}

#[tokio::test]
async fn a_missing_stored_hook_restores_timer_and_wakes_a_fetch() {
    let stub = stub_forge_cli(script(false));
    let h = harness()
        .forge_cli(&stub)
        .timeouts(ariadne_daemon::timeouts::Timeouts {
            forge_poll: crate::common::RUNS_OUT,
            ..Default::default()
        })
        .await;
    let public = h.state.forge_poll.webhook_url();
    public.set(Some("https://first.example".into()));
    let id = enable(&h, false).await;
    eventually(TIMEOUT, "initial fetch", async || fetches(&stub) == 1).await;
    let mut entries = script(false).as_array().unwrap().clone();
    entries.pop();
    entries.push(json!({"args":["api"],"exit":1,"stderr":"HTTP 404: hook not found"}));
    stub.reprogram(Value::Array(entries));
    public.set(Some("https://second.example".into()));
    eventually(TIMEOUT, "missing hook switches to polling", async || {
        let row = h.store.forge_integration(&id).await.unwrap().unwrap();
        row.webhook_state == "polling" && row.webhook_id.is_none() && fetches(&stub) >= 2
    })
    .await;
    eventually(TIMEOUT, "timer after missing hook", async || {
        fetches(&stub) >= 3
    })
    .await;
}

#[tokio::test]
async fn a_failed_hook_update_keeps_its_id_and_reports_the_error_with_timer_fallback() {
    let stub = stub_forge_cli(script(true));
    let h = harness()
        .forge_cli(&stub)
        .timeouts(ariadne_daemon::timeouts::Timeouts {
            forge_poll: crate::common::RUNS_OUT,
            ..Default::default()
        })
        .await;
    let public = h.state.forge_poll.webhook_url();
    public.set(Some("https://first.example".into()));
    let id = enable(&h, true).await;
    eventually(TIMEOUT, "initial fetch", async || fetches(&stub) == 1).await;
    let secret = h
        .store
        .forge_integration(&id)
        .await
        .unwrap()
        .unwrap()
        .webhook_secret
        .unwrap();
    let mut entries = script(true).as_array().unwrap().clone();
    entries.pop();
    entries.push(json!({"args":["api"],"exit":1,"stderr":format!("HTTP 500: failed request token={secret}")}));
    stub.reprogram(Value::Array(entries));
    public.set(Some("https://second.example".into()));
    eventually(TIMEOUT, "failed update", async || {
        h.store
            .forge_integration(&id)
            .await
            .unwrap()
            .unwrap()
            .webhook_state
            == "failed"
    })
    .await;
    let row = h.store.forge_integration(&id).await.unwrap().unwrap();
    assert_eq!(row.webhook_id, Some(17));
    assert!(row.webhook_error.as_deref().unwrap().contains("HTTP 500"));
    assert!(!row.webhook_error.as_deref().unwrap().contains(&secret));
    eventually(TIMEOUT, "timer after failure", async || fetches(&stub) >= 3).await;
}

#[tokio::test]
async fn startup_checks_a_stored_hook_without_creating_another_and_fetches_once() {
    use ariadne_core::ForgeKind;
    use ariadne_store::{NewRepository, SetForgeIntegration, Store};
    let stub = stub_forge_cli(script(false));
    let h = harness().forge_cli(&stub).await;
    let store = Store::open(&h.dir.path().join("restart.db")).await.unwrap();
    let repo = store
        .create_repository_with_forge(
            NewRepository {
                path: "/test/widgets".into(),
                base_branch: "main".into(),
                description: None,
                permission_mode: None,
                default_landing: None,
            },
            Some(SetForgeIntegration {
                repository_id: String::new(),
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
            }),
        )
        .await
        .unwrap();
    let mut row = repo.forge.unwrap();
    row.webhook_id = Some(17);
    row.webhook_secret = Some("ab".repeat(32));
    row.webhook_url = Some(format!("https://hooks.example/webhooks/github/{}", repo.id));
    row.webhook_state = "live".into();
    store.set_webhook(&row).await.unwrap();
    let bus = ariadne_daemon::bus::start(store.clone());
    let mut cfg = (*h.launcher.cfg).clone();
    cfg.webhook_public_url = Some("https://hooks.example".into());
    let poll = ariadne_daemon::forge::poll::start(
        store.clone(),
        std::sync::Arc::new(cfg),
        &bus,
        crate::common::RUNS_OUT,
        ariadne_daemon::timeouts::Timeouts::default().forge_details,
    );
    eventually(TIMEOUT, "startup fetch", async || fetches(&stub) == 1).await;
    quiet(&stub, 1).await;
    let calls = stub.invocations();
    assert_eq!(
        calls
            .iter()
            .filter(|c| c.args.first().is_some_and(|a| a == "api"))
            .count(),
        1
    );
    assert!(
        calls.iter().any(
            |c| c.args.iter().any(|a| a == "GET") && c.args[1] == "repos/acme/widgets/hooks/17"
        )
    );
    assert_eq!(
        store
            .forge_integration(&repo.id)
            .await
            .unwrap()
            .unwrap()
            .webhook_state,
        "live"
    );
    drop(poll);
}

#[tokio::test]
async fn a_changed_remote_deletes_the_old_hook_without_reusing_its_secret_or_id() {
    let stub = stub_forge_cli(script(false));
    let h = harness().forge_cli(&stub).await;
    h.state
        .forge_poll
        .webhook_url()
        .set(Some("https://hooks.example".into()));
    let id = enable(&h, false).await;
    let original = h.store.forge_integration(&id).await.unwrap().unwrap();
    let repo = h.store.get_repository(&id).await.unwrap();
    sh(
        std::path::Path::new(&repo.path),
        "git remote set-url origin https://github.com/acme/other.git",
    );
    let _: Value = h
        .json(
            put_json(
                &format!("/v1/repositories/{id}"),
                json!({"description":"new remote"}),
            ),
            StatusCode::OK,
        )
        .await;
    let changed = h.store.forge_integration(&id).await.unwrap().unwrap();
    assert!(!changed.enabled);
    assert!(changed.webhook_id.is_none());
    assert!(changed.webhook_secret.is_none());
    let deletions: Vec<_> = stub
        .invocations()
        .into_iter()
        .filter(|c| c.args.iter().any(|a| a == "DELETE"))
        .collect();
    assert_eq!(deletions.len(), 1);
    assert_eq!(deletions[0].args[1], "repos/acme/widgets/hooks/17");
    let _: Value = h
        .json(
            put_json(
                &format!("/v1/repositories/{id}"),
                json!({"forge":{"enabled":true}}),
            ),
            StatusCode::OK,
        )
        .await;
    let new = h.store.forge_integration(&id).await.unwrap().unwrap();
    assert_eq!(new.webhook_state, "live");
    assert_ne!(original.webhook_secret, new.webhook_secret);
}

#[tokio::test]
async fn replacing_and_enabling_a_remote_in_one_edit_creates_its_own_hook() {
    let stub = stub_forge_cli(script(false));
    let h = harness().forge_cli(&stub).await;
    h.state
        .forge_poll
        .webhook_url()
        .set(Some("https://hooks.example".into()));
    let id = enable(&h, false).await;
    let original = h.store.forge_integration(&id).await.unwrap().unwrap();
    let repo = h.store.get_repository(&id).await.unwrap();
    sh(
        std::path::Path::new(&repo.path),
        "git remote set-url origin https://github.com/acme/other.git",
    );
    let _: Value = h
        .json(
            put_json(
                &format!("/v1/repositories/{id}"),
                json!({"forge":{"enabled":true}}),
            ),
            StatusCode::OK,
        )
        .await;
    let row = h.store.forge_integration(&id).await.unwrap().unwrap();
    assert_eq!(row.webhook_state, "live");
    assert_ne!(row.webhook_secret, original.webhook_secret);
    assert!(stub.invocations().iter().any(|c| {
        c.args.get(1).is_some_and(|p| p == "repos/acme/other/hooks")
            && c.args.iter().any(|a| a == "POST")
    }));
}
