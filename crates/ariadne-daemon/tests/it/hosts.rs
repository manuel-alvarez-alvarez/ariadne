//! Integration tests for the host list and its configuration (037).

use crate::common;

use ariadne_api::hosts::{HostDto, HostKind, HostStatus, HostSyncDto, SyncState};
use ariadne_daemon::config::Config;
use common::harness;

/// A home whose `config.toml` says `toml`.
fn home_with(toml: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(home.join("config.toml"), toml).unwrap();
    (dir, home)
}

/// The sync of a host before anything is pushed to it.
fn pending() -> HostSyncDto {
    HostSyncDto {
        state: SyncState::Pending,
        error: None,
        missing_repositories: Vec::new(),
        skipped_permissions: 0,
    }
}

#[tokio::test]
async fn a_daemon_without_hosts_lists_only_local() {
    let h = harness().await;
    let hosts: Vec<HostDto> = h.get("/v1/hosts").await;
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0].name, "local");
    assert_eq!(hosts[0].kind, HostKind::Local);
    assert_eq!(hosts[0].status, HostStatus::Ready);
    assert_eq!(hosts[0].error, None);
    assert_eq!(hosts[0].sync, HostSyncDto::in_sync());
    assert_eq!(
        hosts[0].endpoint.as_deref(),
        Some(h.launcher.cfg.socket_path.to_str().unwrap())
    );
}

#[tokio::test]
async fn configured_hosts_follow_local_in_file_order() {
    let (_dir, home) = home_with(
        "tcp_listen = \"127.0.0.1:7676\"\n\
         [hosts.zeta]\nendpoint = \"http://10.0.0.2:7676\"\n\
         [hosts.alpha]\nendpoint = \"ssh://me@box:2222\"\nremote = \"/run/ariadne.sock\"\n\
         [hosts.sock]\nendpoint = \"/tmp/other.sock\"\n",
    );
    let h = harness().home(home).await;
    let hosts: Vec<HostDto> = h.get("/v1/hosts").await;
    let names: Vec<_> = hosts.iter().map(|h| h.name.as_str()).collect();
    assert_eq!(names, ["local", "zeta", "alpha", "sock"]);
    assert_eq!(hosts[0].endpoint.as_deref(), Some("http://127.0.0.1:7676"));
    assert_eq!(
        hosts[1],
        HostDto {
            name: "zeta".into(),
            kind: HostKind::Direct,
            endpoint: Some("http://10.0.0.2:7676".into()),
            status: HostStatus::Ready,
            error: None,
            remote: None,
            sync: pending(),
        }
    );
    assert_eq!(
        hosts[2],
        HostDto {
            name: "alpha".into(),
            kind: HostKind::Ssh,
            endpoint: None,
            status: HostStatus::Down,
            error: Some("ssh forward not started".into()),
            remote: Some("/run/ariadne.sock".into()),
            sync: pending(),
        }
    );
    assert_eq!(hosts[3].kind, HostKind::Direct);
    assert_eq!(hosts[3].endpoint.as_deref(), Some("/tmp/other.sock"));
}

/// Each refusal names the host and the key, at startup and in `--check-config`.
#[test]
fn a_bad_host_is_refused_by_name_at_startup_and_by_check_config() {
    let cases: [(&str, &str); 13] = [
        ("[hosts.local]\nendpoint = \"/x.sock\"\n", "hosts.local"),
        (
            "[hosts.all]\nendpoint = \"/x.sock\"\n",
            "hosts.all: the name all selects every host",
        ),
        ("[hosts.\"a b\"]\nendpoint = \"/x.sock\"\n", "hosts.a b"),
        (
            "[hosts.box]\nendpoint = \"relative.sock\"\n",
            "hosts.box.endpoint",
        ),
        (
            "[hosts.box]\nendpoint = \"http://box\"\n",
            "hosts.box.endpoint",
        ),
        (
            "[hosts.box]\nendpoint = \"https://box:1\"\n",
            "hosts.box.endpoint",
        ),
        ("[hosts.box]\nendpoint = \"ssh://\"\n", "hosts.box.endpoint"),
        (
            "[hosts.box]\nendpoint = \"SSH://me@box\"\n",
            "hosts.box.endpoint",
        ),
        (
            "[hosts.box]\nendpoint = \"HTTP://box:1\"\n",
            "hosts.box.endpoint",
        ),
        (
            "[hosts.box]\nendpoint = \"http://box:1\"\nremote = \"/x\"\n",
            "hosts.box.remote",
        ),
        (
            "[hosts.box]\nendpoint = \"/x.sock\"\nremote = \"/x\"\n",
            "hosts.box.remote",
        ),
        (
            "[hosts.box]\nendpoint = \"ssh://box\"\nremote = \"relative\"\n",
            "hosts.box.remote",
        ),
        (
            "[hosts.box]\nendpoint = \"ssh://box\"\nremote = \"10.0.0.1:7676\"\n",
            "hosts.box.remote",
        ),
    ];
    for (toml, named) in cases {
        let (_dir, home) = home_with(toml);
        let started = Config::load(Some(home.clone()))
            .expect_err(toml)
            .to_string();
        let checked = Config::check(Some(home)).expect_err(toml).to_string();
        assert!(started.contains(named), "{toml}: {started}");
        assert_eq!(started, checked, "{toml}");
    }
}

#[test]
fn good_hosts_pass_check_config() {
    let (_dir, home) = home_with(
        "[hosts.a]\nendpoint = \"ssh://box\"\nremote = \"127.0.0.1:7676\"\n\
         [hosts.b-1]\nendpoint = \"ssh://me@box:22\"\nremote = \"/run/a.sock\"\n\
         [hosts.c_2]\nendpoint = \"http://box:7676\"\n",
    );
    assert!(Config::check(Some(home)).unwrap().contains(": ok"));
}

/// `http://host:80` names its port even though 80 is the scheme's default.
#[tokio::test]
async fn an_http_host_on_the_default_port_is_accepted_and_listed() {
    let (_dir, home) = home_with("[hosts.web]\nendpoint = \"http://example.com:80\"\n");
    assert!(Config::check(Some(home.clone())).unwrap().contains(": ok"));
    let h = harness().home(home).await;
    let hosts: Vec<HostDto> = h.get("/v1/hosts").await;
    assert_eq!(hosts[1].endpoint.as_deref(), Some("http://example.com:80"));
}

#[tokio::test]
async fn openapi_carries_the_route_the_dto_and_the_event() {
    let h = harness().await;
    let doc: serde_json::Value = h.get("/api-docs/openapi.json").await;
    assert!(doc["paths"]["/v1/hosts"]["get"].is_object());
    assert!(doc["components"]["schemas"]["HostDto"].is_object());
    let host = doc["components"]["schemas"]["HostDto"].to_string();
    assert!(host.contains("\"sync\""), "{host}");
    let sync = doc["components"]["schemas"]["HostSyncDto"].to_string();
    for field in ["missing_repositories", "skipped_permissions"] {
        assert!(sync.contains(field), "{sync}");
    }
    assert!(doc["components"]["schemas"]["MissingRepositoryDto"].is_object());
    let state = doc["components"]["schemas"]["SyncState"].to_string();
    for value in ["in_sync", "pending", "failed"] {
        assert!(state.contains(value), "{state}");
    }
    assert!(doc["paths"]["/v1/permissions/learned"]["post"].is_object());
    let event = doc["components"]["schemas"]["DomainEvent"].to_string();
    assert!(event.contains("host_updated"), "{event}");
}
