//! `POST /v1/hosts/{name}/reconnect`: one host reconnects at once (037).

use std::time::Duration;

use crate::common::acp::pid_is_alive;
use crate::common::ssh::stub_ssh;
use crate::common::{RUNS_OUT, TIMEOUT, eventually, harness, post, roomy_hardware};
use crate::host_health::{Health, Updates, listed, local_with, serve_on_socket, serve_on_tcp};
use ariadne_api::hosts::{HostDto, HostStatus};
use ariadne_api::stream::{DeletedDto, DomainEvent};
use ariadne_daemon::bus::BusEvent;
use ariadne_daemon::timeouts::Timeouts;
use axum::http::StatusCode;
use serde_json::json;

const RECONNECT: &str = "/v1/hosts/far/reconnect";

/// Checks that never run by themselves while a test lasts: each change of
/// the host comes from the reconnect.
fn still() -> Timeouts {
    Timeouts {
        host_unanswered: TIMEOUT,
        host_check: TIMEOUT,
        ssh_retry: TIMEOUT,
        ..Timeouts::default()
    }
}

async fn reconnect(h: &crate::common::Harness) -> HostDto {
    h.json(post(RECONNECT), StatusCode::OK).await
}

#[tokio::test]
async fn local_is_400_and_an_unknown_name_is_404_in_the_error_envelope() {
    let h = harness().await;
    let local = h
        .error(post("/v1/hosts/local/reconnect"), StatusCode::BAD_REQUEST)
        .await;
    assert_eq!(local.error.code, "invalid_request");
    let unknown = h
        .error(post("/v1/hosts/nowhere/reconnect"), StatusCode::NOT_FOUND)
        .await;
    assert_eq!(unknown.error.code, "not_found");
    assert_eq!(unknown.error.message, "unknown host nowhere");
}

#[tokio::test]
async fn a_down_forward_starts_again_at_once_and_waits_the_first_backoff_after() {
    // The wait between runs is an hour: no test outlasts it, so a run that
    // comes while the test waits is the reconnect's, with no clock to read.
    // That the wait is the first backoff again after it is the backoff's own
    // test (`hosts.rs::a_reset_backoff_waits_the_first_value_again_and_the_cap_holds`).
    let stub = stub_ssh(json!({"stderr": "Connection refused", "exit": 255}));
    let host = "endpoint = \"ssh://box\"\nremote = \"/run/far.sock\"\n";
    let timeouts = Timeouts {
        ssh_retry: Duration::from_secs(3600),
        ..Timeouts::default()
    };
    let (local, _dir) = local_with(host, Some(&stub), timeouts).await;
    local.state.hosts.start();
    eventually(TIMEOUT, "the first ssh run", || async {
        stub.runs().len() == 1
    })
    .await;
    eventually(TIMEOUT, "the first run down", || async {
        listed(&local).await.status == HostStatus::Down
    })
    .await;
    let mut updates = Updates(local.bus.subscribe());

    reconnect(&local).await;
    assert_eq!(updates.next().await.status, HostStatus::Connecting);
    eventually(TIMEOUT, "a run at once", || async {
        stub.runs().len() == 2
    })
    .await;
    assert_eq!(updates.next().await.status, HostStatus::Down);

    // Down again, with an hour to wait: a second reconnect still runs at once.
    reconnect(&local).await;
    eventually(TIMEOUT, "a second run at once", || async {
        stub.runs().len() == 3
    })
    .await;
}

#[tokio::test]
async fn a_connecting_forward_loses_its_try_and_starts_a_new_one_at_once() {
    let nowhere = tempfile::tempdir().unwrap();
    let remote = nowhere.path().join("nobody.sock").display().to_string();
    let stub = stub_ssh(json!({"forward": true}));
    let host = format!("endpoint = \"ssh://box\"\nremote = \"{remote}\"\n");
    let (local, _dir) = local_with(&host, Some(&stub), still()).await;
    local.state.hosts.start();
    eventually(TIMEOUT, "a first ssh run", || async {
        stub.runs().len() == 1
    })
    .await;
    assert_eq!(listed(&local).await.status, HostStatus::Connecting);

    let host = reconnect(&local).await;
    assert_eq!(host.status, HostStatus::Connecting);
    eventually(RUNS_OUT * 4, "a second ssh run", || async {
        stub.runs().len() == 2
    })
    .await;
    let first = stub.runs()[0].pid;
    eventually(RUNS_OUT * 4, "the first run ends", || async {
        !pid_is_alive(first)
    })
    .await;
}

#[tokio::test]
async fn each_reconnect_reaps_the_previous_forward_before_answering() {
    let nowhere = tempfile::tempdir().unwrap();
    let remote = nowhere.path().join("nobody.sock").display().to_string();
    let stub = stub_ssh(json!({"forward": true}));
    let host = format!("endpoint = \"ssh://box\"\nremote = \"{remote}\"\n");
    let (local, _dir) = local_with(&host, Some(&stub), still()).await;
    local.state.hosts.start();

    for run in 1..=16 {
        eventually(TIMEOUT, "the current SSH forward starts", || async {
            stub.runs().len() == run
        })
        .await;
        let previous = stub.runs()[run - 1].pid;
        let host = reconnect(&local).await;
        assert_eq!(host.status, HostStatus::Connecting);
        assert!(
            !pid_is_alive(previous),
            "the previous forward must be reaped before reconnect answers"
        );
    }
    local.state.hosts.shutdown().await;
}

#[tokio::test]
async fn a_ready_forward_takes_one_check_and_a_good_answer_cuts_no_stream() {
    let far = harness().ai_permissions_hardware(roomy_hardware()).await;
    let health = Health::default();
    let (socket, _socket_dir) = serve_on_socket(&far, &health).await;
    let stub = stub_ssh(json!({"forward": true}));
    let host = format!("endpoint = \"ssh://box\"\nremote = \"{socket}\"\n");
    let (local, _dir) = local_with(&host, Some(&stub), still()).await;
    let mut updates = Updates(local.bus.subscribe());
    local.state.hosts.start();
    assert_eq!(updates.next().await.status, HostStatus::Connecting);
    let ready = updates.next().await;
    assert_eq!(ready.status, HostStatus::Ready);
    let endpoint = ready.endpoint.clone().unwrap();
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let mut stream = client
        .get(format!("{endpoint}/v1/events/stream"))
        .send()
        .await
        .unwrap();
    assert_eq!(stream.status(), reqwest::StatusCode::OK);
    read_until(&mut stream, "event: heartbeat").await;
    let asked = health.asked();

    let host = reconnect(&local).await;
    assert_eq!(health.asked(), asked + 1, "one check at once");
    assert_eq!(host.status, HostStatus::Ready);
    assert_eq!(host.endpoint.as_deref(), Some(endpoint.as_str()));
    assert_eq!(stub.runs().len(), 1, "no new forward");
    assert!(pid_is_alive(stub.runs()[0].pid), "the forward stays up");

    // The stream opened before the reconnect still carries events.
    far.bus.publish(BusEvent {
        event: DomainEvent::SkillDeleted(DeletedDto {
            id: "published-after-reconnect".into(),
        }),
        goal_id: None,
        task_id: None,
        recorded: None,
    });
    read_until(&mut stream, "published-after-reconnect").await;
}

#[tokio::test]
async fn a_ready_forward_whose_check_fails_is_down_and_starts_again_at_once() {
    let far = harness().ai_permissions_hardware(roomy_hardware()).await;
    let health = Health::default();
    let (socket, _socket_dir) = serve_on_socket(&far, &health).await;
    let stub = stub_ssh(json!({"forward": true}));
    let host = format!("endpoint = \"ssh://box\"\nremote = \"{socket}\"\n");
    let (local, _dir) = local_with(&host, Some(&stub), still()).await;
    let mut updates = Updates(local.bus.subscribe());
    local.state.hosts.start();
    assert_eq!(updates.next().await.status, HostStatus::Connecting);
    let ready = updates.next().await;
    assert_eq!(ready.status, HostStatus::Ready);
    let endpoint = ready.endpoint.unwrap();

    health.set_off(true);
    let host = reconnect(&local).await;
    assert_eq!(host.status, HostStatus::Connecting);
    let down = updates.next_status(HostStatus::Ready).await;
    assert_eq!(down.status, HostStatus::Down);
    assert_eq!(down.endpoint, None);
    let error = down.error.unwrap();
    assert!(
        error.starts_with(&format!("{endpoint} does not answer")),
        "{error}"
    );
    // `ssh_retry` is the whole test: the new forward waits on no backoff.
    let connecting = updates.next_status(HostStatus::Down).await;
    assert_eq!(connecting.status, HostStatus::Connecting);
    eventually(RUNS_OUT * 4, "a second ssh run", || async {
        stub.runs().len() == 2
    })
    .await;
    assert!(!pid_is_alive(stub.runs()[0].pid), "the old forward ends");
}

#[tokio::test]
async fn a_direct_host_takes_one_check_and_its_status_follows_the_answer() {
    let far = harness().ai_permissions_hardware(roomy_hardware()).await;
    let health = Health::default();
    let address = serve_on_tcp(&far, &health).await;
    let endpoint = format!("http://{address}");
    let host = format!("endpoint = \"{endpoint}\"\n");
    let (local, _dir) = local_with(&host, None, still()).await;
    let mut updates = Updates(local.bus.subscribe());
    local.state.hosts.start();
    assert_eq!(updates.next().await.status, HostStatus::Ready);

    health.set_off(true);
    let asked = health.asked();
    let down = reconnect(&local).await;
    assert_eq!(health.asked(), asked + 1, "one check at once");
    assert_eq!(down.status, HostStatus::Down);
    assert_eq!(down.endpoint.as_deref(), Some(endpoint.as_str()));
    let error = down.error.clone().unwrap();
    assert!(
        error.starts_with(&format!("{endpoint} does not answer")),
        "{error}"
    );
    let published = updates.next_status(HostStatus::Ready).await;
    assert_eq!(
        (published.status, published.error),
        (HostStatus::Down, Some(error))
    );

    health.set_off(false);
    let ready = reconnect(&local).await;
    assert_eq!(ready.status, HostStatus::Ready);
    assert_eq!(ready.error, None);
    let published = updates.next_status(HostStatus::Down).await;
    assert_eq!(published.status, HostStatus::Ready);
}

#[tokio::test]
async fn openapi_carries_the_reconnect_route() {
    let h = harness().await;
    let doc: serde_json::Value = h.get("/api-docs/openapi.json").await;
    let route = &doc["paths"]["/v1/hosts/{name}/reconnect"]["post"];
    assert!(route.is_object(), "{}", doc["paths"]);
    let ok = route["responses"]["200"].to_string();
    assert!(ok.contains("HostDto"), "{ok}");
    for status in ["400", "404"] {
        assert!(route["responses"][status].is_object(), "{route}");
    }
}

/// Read `stream` until it has carried `needle`.
async fn read_until(stream: &mut reqwest::Response, needle: &str) {
    let mut seen = String::new();
    while !seen.contains(needle) {
        let chunk = tokio::time::timeout(TIMEOUT, stream.chunk())
            .await
            .expect("a chunk in time")
            .unwrap()
            .expect("the stream stays open");
        seen.push_str(&String::from_utf8_lossy(&chunk));
    }
}
