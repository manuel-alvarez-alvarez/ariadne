//! The health checks that keep the status of each host true (037).
//!
//! The far daemon is a second harness served behind a [`Health`] switch: a
//! test turns its `GET /v1/health` off and on, and every other route
//! answers as it does.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::common::acp::pid_is_alive;
use crate::common::ssh::{StubSsh, stub_ssh};
use crate::common::{
    Harness, QUIET, RUNS_OUT, TIMEOUT, eventually, harness, next_event, roomy_hardware,
};
use ariadne_api::hosts::{HostDto, HostStatus, SyncState};
use ariadne_api::stream::DomainEvent;
use ariadne_daemon::bus::BusEvent;
use ariadne_daemon::timeouts::Timeouts;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde_json::json;
use tokio::sync::broadcast::Receiver;

/// The checks of these tests: a host gives up after `RUNS_OUT`, and a ready
/// host is checked ten times in that while.
fn timeouts() -> Timeouts {
    Timeouts {
        host_unanswered: RUNS_OUT,
        host_check: RUNS_OUT / 10,
        ssh_retry: RUNS_OUT,
        ..Timeouts::default()
    }
}

/// The `GET /v1/health` of a far daemon: on, or a `503`, and how many
/// times it was asked.
#[derive(Clone, Default)]
pub(crate) struct Health {
    off: Arc<AtomicBool>,
    asked: Arc<AtomicUsize>,
}

impl Health {
    pub(crate) fn set_off(&self, off: bool) {
        self.off.store(off, Ordering::SeqCst);
    }

    pub(crate) fn asked(&self) -> usize {
        self.asked.load(Ordering::SeqCst)
    }

    /// `far`'s router, with its health check behind this switch.
    fn router(&self, far: &Harness) -> axum::Router {
        let health = self.clone();
        far.router.clone().layer(axum::middleware::from_fn(
            move |request: axum::extract::Request, next: axum::middleware::Next| {
                let health = health.clone();
                async move {
                    if request.uri().path() == "/v1/health" {
                        health.asked.fetch_add(1, Ordering::SeqCst);
                        if health.off.load(Ordering::SeqCst) {
                            return StatusCode::SERVICE_UNAVAILABLE.into_response();
                        }
                    }
                    next.run(request).await
                }
            },
        ))
    }
}

/// The far daemon on a loopback TCP port, behind `health`.
pub(crate) async fn serve_on_tcp(far: &Harness, health: &Health) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let router = health.router(far);
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    address
}

/// The far daemon on a unix socket of its own, behind `health`.
pub(crate) async fn serve_on_socket(far: &Harness, health: &Health) -> (String, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("far.sock");
    let listener = tokio::net::UnixListener::bind(&path).unwrap();
    let router = health.router(far);
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (path.display().to_string(), dir)
}

/// A local daemon whose one host `far` is `host`, a TOML table body.
async fn local(host: &str, stub: Option<&StubSsh>) -> (Harness, tempfile::TempDir) {
    local_with(host, stub, timeouts()).await
}

/// The same, with `timeouts`.
pub(crate) async fn local_with(
    host: &str,
    stub: Option<&StubSsh>,
    timeouts: Timeouts,
) -> (Harness, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let home: PathBuf = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(
        home.join("host-fixture.toml"),
        format!("[hosts.far]\n{host}"),
    )
    .unwrap();
    let mut builder = harness()
        .ai_permissions_hardware(roomy_hardware())
        .home(home)
        .timeouts(timeouts);
    if let Some(stub) = stub {
        builder = builder.ssh(stub);
    }
    (builder.await, dir)
}

/// The `host_updated` events of the host, read off the bus.
pub(crate) struct Updates(pub(crate) Receiver<BusEvent>);

impl Updates {
    /// The next `host_updated`, whatever it moved.
    pub(crate) async fn next(&mut self) -> HostDto {
        let event = next_event(&mut self.0, |event| {
            matches!(event.event, DomainEvent::HostUpdated(_))
        })
        .await;
        let DomainEvent::HostUpdated(host) = event.event else {
            unreachable!()
        };
        host
    }

    /// The next `host_updated` that moves the status. Each one it skips
    /// moves only the sync and keeps the status `before`.
    pub(crate) async fn next_status(&mut self, before: HostStatus) -> HostDto {
        loop {
            let host = self.next().await;
            if host.status != before {
                return host;
            }
        }
    }
}

/// The host `far` as `GET /v1/hosts` lists it.
pub(crate) async fn listed(h: &Harness) -> HostDto {
    let hosts: Vec<HostDto> = h.get("/v1/hosts").await;
    hosts[1].clone()
}

#[tokio::test]
async fn a_direct_host_is_ready_while_it_answers_and_down_while_it_does_not() {
    let far = harness().ai_permissions_hardware(roomy_hardware()).await;
    let health = Health::default();
    let address = serve_on_tcp(&far, &health).await;
    let endpoint = format!("http://{address}");
    let (local, _dir) = local(&format!("endpoint = \"{endpoint}\"\n"), None).await;
    assert_eq!(listed(&local).await.status, HostStatus::Connecting);
    let mut updates = Updates(local.bus.subscribe());
    local.state.hosts.start();

    // One event for each change: the status, then the sync it starts.
    let ready = updates.next().await;
    assert_eq!(ready.status, HostStatus::Ready);
    assert_eq!(ready.endpoint.as_deref(), Some(endpoint.as_str()));
    assert_eq!(ready.error, None);
    assert_eq!(ready.sync.state, SyncState::Pending);
    let in_sync = updates.next().await;
    assert_eq!(
        (in_sync.status, in_sync.sync.state),
        (HostStatus::Ready, SyncState::InSync)
    );

    health.set_off(true);
    let down = updates.next().await;
    assert_eq!(down.status, HostStatus::Down);
    assert_eq!(down.endpoint.as_deref(), Some(endpoint.as_str()));
    let error = down.error.clone().unwrap();
    assert!(error.contains(&endpoint), "{error}");
    assert!(error.contains("has not answered"), "{error}");
    let listed_down = listed(&local).await;
    assert_eq!(
        (listed_down.status, listed_down.error),
        (HostStatus::Down, Some(error))
    );
    // The sync stopped with the host.
    let stopped = updates.next().await;
    assert_eq!(
        (stopped.status, stopped.sync.state),
        (HostStatus::Down, SyncState::Pending)
    );

    // The daemon goes on to check the host that is down.
    health.set_off(false);
    let again = updates.next().await;
    assert_eq!(again.status, HostStatus::Ready);
    assert_eq!(again.error, None);
    // Its next `ready` starts the sync again.
    let in_sync = updates.next().await;
    assert_eq!(
        (in_sync.status, in_sync.sync.state),
        (HostStatus::Ready, SyncState::InSync)
    );
}

#[tokio::test]
async fn a_direct_host_that_does_not_answer_is_down_and_ready_once_it_answers() {
    // A port nothing listens on yet.
    let address = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap()
    };
    let endpoint = format!("http://{address}");
    let (local, _dir) = local(&format!("endpoint = \"{endpoint}\"\n"), None).await;
    let mut updates = Updates(local.bus.subscribe());
    local.state.hosts.start();

    let down = updates.next().await;
    assert_eq!(down.status, HostStatus::Down);
    let error = down.error.unwrap();
    assert!(
        error.starts_with(&format!("{endpoint} does not answer")),
        "{error}"
    );
    assert_eq!(down.sync.state, SyncState::Pending);

    let far = harness().ai_permissions_hardware(roomy_hardware()).await;
    let listener = tokio::net::TcpListener::bind(address).await.unwrap();
    let router = far.router.clone();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let ready = updates.next().await;
    assert_eq!(ready.status, HostStatus::Ready);
    assert_eq!(ready.error, None);
    assert_eq!(updates.next().await.sync.state, SyncState::InSync);
}

#[tokio::test]
async fn a_direct_host_on_a_socket_path_is_checked_through_the_socket() {
    let far = harness().ai_permissions_hardware(roomy_hardware()).await;
    let health = Health::default();
    let (socket, _socket_dir) = serve_on_socket(&far, &health).await;
    let (local, _dir) = local(&format!("endpoint = \"{socket}\"\n"), None).await;
    let mut updates = Updates(local.bus.subscribe());
    local.state.hosts.start();

    let ready = updates.next().await;
    assert_eq!(ready.status, HostStatus::Ready);
    assert_eq!(ready.endpoint.as_deref(), Some(socket.as_str()));
    assert!(health.asked() > 0, "the check went through the socket");

    health.set_off(true);
    let down = updates.next_status(HostStatus::Ready).await;
    assert_eq!(down.status, HostStatus::Down);
    let error = down.error.unwrap();
    assert!(error.contains(&socket), "{error}");
}

#[tokio::test]
async fn a_forward_whose_far_daemon_does_not_answer_is_down_and_starts_again() {
    let nowhere = tempfile::tempdir().unwrap();
    let remote = nowhere.path().join("nobody.sock").display().to_string();
    let stub = stub_ssh(json!({"forward": true}));
    let host = format!("endpoint = \"ssh://box\"\nremote = \"{remote}\"\n");
    let (local, _dir) = local(&host, Some(&stub)).await;
    let mut updates = Updates(local.bus.subscribe());
    local.state.hosts.start();

    assert_eq!(updates.next().await.status, HostStatus::Connecting);
    let down = updates.next().await;
    assert_eq!(down.status, HostStatus::Down);
    assert_eq!(down.endpoint, None);
    let error = down.error.unwrap();
    assert!(
        error.starts_with("the far daemon does not answer through the forward"),
        "{error}"
    );
    let first = stub.runs()[0].pid;
    assert!(!pid_is_alive(first), "the forward that did not answer ends");

    assert_eq!(updates.next().await.status, HostStatus::Connecting);
    eventually(TIMEOUT, "a second ssh run", || async {
        stub.runs().len() == 2
    })
    .await;
}

#[tokio::test]
async fn a_ready_forward_that_stops_answering_loses_its_forward_and_starts_again() {
    let far = harness().ai_permissions_hardware(roomy_hardware()).await;
    let health = Health::default();
    let (socket, _socket_dir) = serve_on_socket(&far, &health).await;
    let stub = stub_ssh(json!({"forward": true}));
    let host = format!("endpoint = \"ssh://box\"\nremote = \"{socket}\"\n");
    let (local, _dir) = local(&host, Some(&stub)).await;
    let mut updates = Updates(local.bus.subscribe());
    local.state.hosts.start();

    assert_eq!(updates.next().await.status, HostStatus::Connecting);
    let ready = updates.next().await;
    assert_eq!(ready.status, HostStatus::Ready);
    let endpoint = ready.endpoint.unwrap();

    health.set_off(true);
    let down = updates.next_status(HostStatus::Ready).await;
    assert_eq!(down.status, HostStatus::Down);
    assert_eq!(down.endpoint, None);
    let error = down.error.unwrap();
    assert!(error.contains(&endpoint), "{error}");
    assert!(error.contains("has not answered"), "{error}");
    let first = stub.runs()[0].pid;
    assert!(!pid_is_alive(first), "the forward of a down host ends");

    health.set_off(false);
    let connecting = updates.next_status(HostStatus::Down).await;
    assert_eq!(connecting.status, HostStatus::Connecting);
    let again = updates.next_status(HostStatus::Connecting).await;
    assert_eq!(again.status, HostStatus::Ready);
    assert_eq!(stub.runs().len(), 2);
}

#[tokio::test]
async fn shutdown_ends_the_checks_of_a_direct_host() {
    let far = harness().ai_permissions_hardware(roomy_hardware()).await;
    let health = Health::default();
    let address = serve_on_tcp(&far, &health).await;
    let (local, _dir) = local(&format!("endpoint = \"http://{address}\"\n"), None).await;
    local.state.hosts.start();
    // The checks go on while the host is ready.
    eventually(TIMEOUT, "checks of the ready host", || async {
        listed(&local).await.status == HostStatus::Ready && health.asked() >= 3
    })
    .await;

    local.state.hosts.shutdown().await;
    let asked = health.asked();
    tokio::time::sleep(QUIET).await;
    assert_eq!(health.asked(), asked, "no check after shutdown");
}

#[tokio::test]
async fn editing_and_deleting_a_host_stop_its_old_health_checks() {
    let far = harness().ai_permissions_hardware(roomy_hardware()).await;
    let old = Health::default();
    let new = Health::default();
    let old_address = serve_on_tcp(&far, &old).await;
    let new_address = serve_on_tcp(&far, &new).await;
    let local = harness().timeouts(timeouts()).await;
    let _: HostDto = local
        .json(
            crate::common::post_json(
                "/v1/hosts",
                json!({"name":"far","endpoint":format!("http://{old_address}")}),
            ),
            StatusCode::CREATED,
        )
        .await;
    eventually(TIMEOUT, "initial host checks", || async {
        old.asked() >= 3
    })
    .await;
    let coloured: HostDto = local
        .json(
            crate::common::patch_json("/v1/hosts/far", json!({"color":"blue"})),
            StatusCode::OK,
        )
        .await;
    assert_eq!(coloured.status, HostStatus::Ready);
    let _: HostDto = local
        .json(
            crate::common::patch_json(
                "/v1/hosts/far",
                json!({"endpoint":format!("http://{new_address}")}),
            ),
            StatusCode::OK,
        )
        .await;
    eventually(TIMEOUT, "replacement host checks", || async {
        new.asked() >= 3
    })
    .await;
    let old_count = old.asked();
    let (status, _) = local.send(crate::common::delete("/v1/hosts/far")).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let new_count = new.asked();
    tokio::time::sleep(QUIET).await;
    assert_eq!(old.asked(), old_count);
    assert_eq!(new.asked(), new_count);
    let response = reqwest::get(format!("http://{new_address}/v1/health"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    local.state.hosts.shutdown().await;
}
