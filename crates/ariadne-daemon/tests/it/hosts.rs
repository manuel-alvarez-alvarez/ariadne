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
async fn stored_hosts_follow_local_in_insertion_order() {
    let (_dir, home) = home_with(
        "tcp_listen = \"127.0.0.1:7676\"\n\
         [hosts.zeta]\nendpoint = \"http://10.0.0.2:7676\"\n\
         [hosts.alpha]\nendpoint = \"ssh://me@box:2222\"\nremote = \"/run/ariadne.sock\"\n\
         [hosts.sock]\nendpoint = \"/tmp/other.sock\"\n",
    );
    std::fs::rename(home.join("config.toml"), home.join("host-fixture.toml")).unwrap();
    let h = harness().home(home).await;
    let hosts: Vec<HostDto> = h.get("/v1/hosts").await;
    let names: Vec<_> = hosts.iter().map(|h| h.name.as_str()).collect();
    assert_eq!(names, ["local", "zeta", "alpha", "sock"]);

    assert_eq!(
        hosts[1],
        HostDto {
            color: None,
            name: "zeta".into(),
            kind: HostKind::Direct,
            endpoint: Some("http://10.0.0.2:7676".into()),
            status: HostStatus::Connecting,
            error: None,
            remote: None,
            sync: pending(),
        }
    );
    assert_eq!(
        hosts[2],
        HostDto {
            color: None,
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
    for (toml, _) in cases {
        let named = "add each host again on the Hosts screen of the app or with ariadne host add";
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
fn even_an_empty_hosts_table_is_refused() {
    let (_dir, home) = home_with("[hosts]\n");
    let started = Config::load(Some(home.clone())).unwrap_err().to_string();
    assert_eq!(started, Config::check(Some(home)).unwrap_err().to_string());
    assert!(started.contains("add each host again"));
}

/// `http://host:80` names its port even though 80 is the scheme's default.
#[tokio::test]
async fn an_http_host_on_the_default_port_is_accepted_and_listed() {
    let (_dir, home) = home_with("[hosts.web]\nendpoint = \"http://example.com:80\"\n");
    std::fs::rename(home.join("config.toml"), home.join("host-fixture.toml")).unwrap();
    let h = harness().home(home).await;
    let hosts: Vec<HostDto> = h.get("/v1/hosts").await;
    assert_eq!(hosts[1].endpoint.as_deref(), Some("http://example.com:80"));
}

#[tokio::test]
async fn openapi_carries_the_route_the_dto_and_the_event() {
    let h = harness().await;
    let doc: serde_json::Value = h.get("/api-docs/openapi.json").await;
    if let Ok(path) = std::env::var("ARIADNE_TEST_OPENAPI") {
        std::fs::write(path, serde_json::to_vec_pretty(&doc).unwrap()).unwrap();
    }
    assert!(doc["paths"]["/v1/hosts"]["post"].is_object());
    assert!(doc["paths"]["/v1/hosts/{name}"]["patch"].is_object());
    assert!(doc["paths"]["/v1/hosts/{name}"]["delete"].is_object());
    assert_eq!(
        doc["components"]["schemas"]["HostColor"]["enum"],
        serde_json::json!([
            "red", "orange", "amber", "green", "teal", "blue", "violet", "pink"
        ])
    );
    assert!(doc["components"]["schemas"]["HostDto"]["properties"]["color"].is_object());
    assert!(doc["paths"]["/v1/hosts/{name}/setup"]["post"].is_object());
    assert!(doc["paths"]["/v1/hosts/{name}/setup"]["get"].is_object());
    assert!(doc["paths"]["/v1/hosts/{name}/update"]["get"].is_object());
    assert!(doc["paths"]["/v1/hosts/{name}/update"]["post"].is_object());
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
    assert!(event.contains("host_deleted"), "{event}");
}

#[tokio::test]
async fn repository_setup_preserves_an_unrelated_existing_destination() {
    assert_existing_destination_preserved(true).await;
}

#[tokio::test]
async fn repository_setup_preserves_an_empty_existing_destination() {
    assert_existing_destination_preserved(false).await;
}

/// A clone refuses an existing destination and leaves it as it was. One
/// harness per test: a second harness in one test process starts slowly.
async fn assert_existing_destination_preserved(has_file: bool) {
    let (remote, state, id) = repository_setup_result(Some("local"), Some(has_file)).await;
    assert_eq!(state["state"], "incomplete", "{state}");
    let step = state["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == format!("clone:{id}"))
        .unwrap();
    assert_eq!(step["complete"], false, "{state}");
    assert!(
        step["error"].as_str().unwrap().contains("destination"),
        "{state}"
    );
    let destination = remote.path().join("workspace/project");
    if has_file {
        assert_eq!(
            std::fs::read_to_string(destination.join("keep.txt")).unwrap(),
            "unrelated content"
        );
    } else {
        assert_eq!(std::fs::read_dir(destination).unwrap().count(), 0);
    }
    assert!(!remote.path().join("worker-repositories.json").exists());
}

#[tokio::test]
async fn repository_setup_reports_a_missing_source_remote() {
    let (remote, state, id) = repository_setup_result(None, None).await;
    assert_eq!(state["state"], "incomplete", "{state}");
    let step = state["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == format!("clone:{id}"))
        .unwrap();
    assert_eq!(step["complete"], false, "{state}");
    assert_eq!(step["error"], "source repository has no Git remote");
    assert!(!remote.path().join("workspace/project").exists());
    assert!(!remote.path().join("worker-repositories.json").exists());
}

#[tokio::test]
async fn repository_setup_reports_worker_git_authentication_failure() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let requests = Arc::new(AtomicUsize::new(0));
    let received = requests.clone();
    let app = axum::Router::new().fallback(move || {
        let received = received.clone();
        async move {
            received.fetch_add(1, Ordering::SeqCst);
            (
                axum::http::StatusCode::UNAUTHORIZED,
                [(
                    axum::http::header::WWW_AUTHENTICATE,
                    "Basic realm=\"worker-repository\"",
                )],
            )
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/private.git", listener.local_addr().unwrap());
    let mut servers = tokio::task::JoinSet::new();
    servers.spawn(async move { axum::serve(listener, app).await.unwrap() });
    let (remote, state, id) = repository_setup_result(Some(&url), None).await;
    assert_eq!(state["state"], "incomplete", "{state}");
    let step = state["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == format!("clone:{id}"))
        .unwrap();
    assert_eq!(step["complete"], false, "{state}");
    assert!(
        step["error"]
            .as_str()
            .unwrap()
            .contains("worker Git authentication"),
        "{state}"
    );
    assert!(
        requests.load(Ordering::SeqCst) > 0,
        "Git must reach the authentication challenge"
    );
    assert!(!remote.path().join("worker-repositories.json").exists());
}

/// Run a repository selection through the public setup routes and return its terminal result.
async fn repository_setup_result(
    remote_url: Option<&str>,
    existing_destination: Option<bool>,
) -> (tempfile::TempDir, serde_json::Value, String) {
    use common::{TIMEOUT, eventually, post_json};
    use serde_json::{Value, json};
    let remote = tempfile::tempdir().unwrap();
    let binaries = tempfile::tempdir().unwrap();
    let mut header = vec![0u8; 64];
    header[..6].copy_from_slice(b"\x7fELF\x02\x01");
    header[16] = 2;
    header[18] = 62;
    let cli = binaries.path().join("ariadne");
    let daemon = binaries.path().join("ariadned");
    std::fs::write(&cli, &header).unwrap();
    std::fs::write(&daemon, &header).unwrap();
    let ssh = common::ssh::stub_ssh(
        json!({"setup":{"home":remote.path(),"os":"Linux","repository_fixture":{}},"exit":0}),
    );
    let h = harness().ssh(&ssh).await;
    let source = h.git_repo("source");
    if let Some(url) = remote_url {
        let url = if url == "local" {
            source.to_str().unwrap()
        } else {
            url
        };
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(&source)
                .args(["remote", "add", "origin", url])
                .status()
                .unwrap()
                .success()
        );
    }
    let repository: Value = h
        .json(
            post_json(
                "/v1/repositories",
                json!({"path":source,"base_branch":"main"}),
            ),
            axum::http::StatusCode::CREATED,
        )
        .await;
    let id = repository["id"].as_str().unwrap().to_owned();
    let destination = remote.path().join("workspace/project");
    if let Some(has_file) = existing_destination {
        std::fs::create_dir_all(&destination).unwrap();
        if has_file {
            std::fs::write(destination.join("keep.txt"), "unrelated content").unwrap();
        }
    }
    let request = json!({"ssh":"ssh://box","source":{"kind":"local","cli":cli,"daemon":daemon},"repositories":{&id:destination}});
    let _: Value = h
        .json(
            post_json("/v1/hosts/builder/setup", request),
            axum::http::StatusCode::ACCEPTED,
        )
        .await;
    eventually(TIMEOUT, "repository setup result", || async {
        matches!(
            h.get::<Value>("/v1/hosts/builder/setup").await["state"].as_str(),
            Some("incomplete" | "failed" | "complete")
        )
    })
    .await;
    let state = h.get("/v1/hosts/builder/setup").await;
    h.state.hosts.shutdown().await;
    (remote, state, id)
}

#[tokio::test]
async fn selected_repositories_clone_only_committed_files_and_retry_registration() {
    use common::{TIMEOUT, eventually, post_json};
    use serde_json::{Value, json};
    let remote = tempfile::tempdir().unwrap();
    let binaries = tempfile::tempdir().unwrap();
    let mut header = vec![0u8; 64];
    header[..6].copy_from_slice(b"\x7fELF\x02\x01");
    header[16] = 2;
    header[18] = 62;
    let cli = binaries.path().join("ariadne");
    let daemon = binaries.path().join("ariadned");
    std::fs::write(&cli, &header).unwrap();
    std::fs::write(&daemon, &header).unwrap();
    let source = harness().await;
    let source_path = source.git_repo("source-project");
    assert!(
        std::process::Command::new("git")
            .arg("-C")
            .arg(&source_path)
            .args(["remote", "add", "origin"])
            .arg(&source_path)
            .status()
            .unwrap()
            .success()
    );
    let repository: Value = source
        .json(
            post_json(
                "/v1/repositories",
                json!({"path":source_path,"base_branch":"main"}),
            ),
            axum::http::StatusCode::CREATED,
        )
        .await;
    std::fs::write(source_path.join("untracked-secret"), "private").unwrap();
    std::fs::write(source_path.join("file.txt"), "uncommitted edit").unwrap();
    let ssh = common::ssh::stub_ssh(
        json!({"setup": {"home": remote.path(), "os":"Linux", "repository_fixture": repository}, "exit": 0}),
    );
    let h = harness().ssh(&ssh).await;
    // Select a repository registered on the source settings daemon.
    let selected: Value = h
        .json(
            post_json(
                "/v1/repositories",
                json!({"path":source_path,"base_branch":"main"}),
            ),
            axum::http::StatusCode::CREATED,
        )
        .await;
    let id = selected["id"].as_str().unwrap();
    let destination = remote.path().join("workspace/project");
    let fail = remote.path().join("fail-registration");
    std::fs::write(&fail, "").unwrap();
    let request = json!({"ssh":"ssh://box", "source":{"kind":"local","cli":cli,"daemon":daemon},"repositories":{id:destination}});
    let _: Value = h
        .json(
            post_json("/v1/hosts/builder/setup", request.clone()),
            axum::http::StatusCode::ACCEPTED,
        )
        .await;
    eventually(TIMEOUT, "registration failure", || async {
        h.get::<Value>("/v1/hosts/builder/setup").await["state"] == "incomplete"
    })
    .await;
    let progress: Value = h.get("/v1/hosts/builder/setup").await;
    assert!(
        progress["steps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["name"] == format!("clone:{id}") && s["complete"] == true),
        "{progress}"
    );
    assert!(!destination.join("untracked-secret").exists());
    assert_eq!(
        std::fs::read_to_string(destination.join("file.txt")).unwrap(),
        "v1\n"
    );
    std::fs::write(destination.join("worker-note"), "keep").unwrap();
    std::fs::remove_file(&fail).unwrap();
    let _: Value = h
        .json(
            post_json("/v1/hosts/builder/setup", request.clone()),
            axum::http::StatusCode::ACCEPTED,
        )
        .await;
    eventually(TIMEOUT, "registration retry", || async {
        h.get::<Value>("/v1/hosts/builder/setup").await["state"] == "complete"
    })
    .await;
    assert_eq!(
        std::fs::read_to_string(destination.join("worker-note")).unwrap(),
        "keep"
    );
    let reused_path = h.git_repo("reused-source");
    assert!(
        std::process::Command::new("git")
            .arg("-C")
            .arg(&reused_path)
            .args(["remote", "add", "origin", "git@github.com:org/project.git"])
            .status()
            .unwrap()
            .success()
    );
    let reused: Value = h
        .json(
            post_json(
                "/v1/repositories",
                json!({"path":reused_path,"base_branch":"main"}),
            ),
            axum::http::StatusCode::CREATED,
        )
        .await;
    let reused_id = reused["id"].as_str().unwrap();
    let mut worker = reused.clone();
    worker["id"] = json!("existing-worker-repository");
    worker["path"] = json!("/existing/worker/checkout");
    worker["forge"]["remote"] = json!("https-origin");
    let registered_path = remote.path().join("worker-repositories.json");
    std::fs::write(&registered_path, json!([worker]).to_string()).unwrap();
    let proposed = remote.path().join("workspace/ignored-destination");
    let mut update = request;
    update["repositories"] = json!({reused_id:proposed});
    let _: Value = h
        .json(
            post_json("/v1/hosts/builder/setup", update),
            axum::http::StatusCode::ACCEPTED,
        )
        .await;
    eventually(TIMEOUT, "registered checkout reuse", || async {
        h.get::<Value>("/v1/hosts/builder/setup").await["state"] == "complete"
    })
    .await;
    assert!(!proposed.exists());
    let retained: Value = serde_json::from_slice(&std::fs::read(registered_path).unwrap()).unwrap();
    assert_eq!(retained[0]["path"], "/existing/worker/checkout");
}

#[tokio::test]
async fn setup_rejects_unsafe_repository_destinations_before_starting() {
    use serde_json::json;
    let h = harness().await;
    for destination in [
        "relative/project",
        "~other/project",
        "/tmp/project\n",
        "/tmp/project\0",
    ] {
        h.error(common::post_json("/v1/hosts/builder/setup", json!({"ssh":"ssh://box", "source":{"kind":"local","cli":"/cli","daemon":"/daemon"}, "repositories":{"source":destination}})), axum::http::StatusCode::BAD_REQUEST).await;
    }
    h.error(
        common::get("/v1/hosts/builder/setup"),
        axum::http::StatusCode::NOT_FOUND,
    )
    .await;
}

#[tokio::test]
async fn setup_accepts_selected_repository_destinations() {
    let h = harness().await;
    let _: serde_json::Value = h
        .json(
            common::post_json(
                "/v1/hosts/builder/setup",
                serde_json::json!({
                    "ssh": "ssh://box",
                    "source": {"kind": "local", "cli": "/tmp/ariadne", "daemon": "/tmp/ariadned"},
                    "repositories": {"source-id": "/home/worker/workspace/project"}
                }),
            ),
            axum::http::StatusCode::ACCEPTED,
        )
        .await;
}

#[tokio::test]
async fn setup_rejects_reserved_names_and_keeps_unknown_operations_absent() {
    let h = harness().await;
    h.error(common::post_json("/v1/hosts/local/setup", serde_json::json!({"ssh":"ssh://box", "source":{"kind":"local", "cli":"/tmp/ariadne", "daemon":"/tmp/ariadned"}})), axum::http::StatusCode::BAD_REQUEST).await;
    h.error(
        common::get("/v1/hosts/absent/setup"),
        axum::http::StatusCode::NOT_FOUND,
    )
    .await;
}

#[tokio::test]
async fn setup_preserves_installation_after_service_failure_and_retries_safely() {
    use common::{TIMEOUT, eventually, post_json};
    use serde_json::{Value, json};
    futures_util::future::join_all(["Linux", "Darwin"].map(|os| async move {
        let remote = tempfile::tempdir().unwrap();
        let binaries = tempfile::tempdir().unwrap();
        let cli = binaries.path().join("ariadne");
        let daemon = binaries.path().join("ariadned");
        // An ELF64, little-endian x86_64 executable header, independent of this machine.
        let mut header = vec![0u8; 64];
        header[..6].copy_from_slice(b"\x7fELF\x02\x01");
        header[16] = 2;
        header[18] = 62;
        if os == "Darwin" {
            header.fill(0);
            header[..8].copy_from_slice(&[0xcf, 0xfa, 0xed, 0xfe, 7, 0, 0, 1]);
            header[12] = 2;
        }
        std::fs::write(&cli, &header).unwrap();
        std::fs::write(&daemon, &header).unwrap();
        std::fs::create_dir_all(remote.path().join(".ariadne")).unwrap();
        let config = remote.path().join(".ariadne/config.toml");
        std::fs::write(&config, "prevent_sleep = false\n").unwrap();
        let unit = remote.path().join(".config/systemd/user/ariadned.service");
        std::fs::create_dir_all(unit.parent().unwrap()).unwrap();
        std::fs::write(&unit, "# machine-specific service settings\n").unwrap();
        let plist = remote
            .path()
            .join("Library/LaunchAgents/dev.ariadne.daemon.plist");
        std::fs::create_dir_all(plist.parent().unwrap()).unwrap();
        let old_plist = "<?xml version=\"1.0\"?><plist version=\"1.0\"><dict><key>EnvironmentVariables</key><dict><key>PATH</key><string>/custom/worker/bin</string></dict></dict></plist>\n";
        std::fs::write(&plist, old_plist).unwrap();
        let fail = remote.path().join("fail-service");
        std::fs::write(&fail, "").unwrap();
        let ssh = common::ssh::stub_ssh(
            json!({"setup": {"home":remote.path(), "os":os, "path":"/remote/.cargo/bin:/opt/homebrew/bin:/usr/bin:/bin", "fail_service":fail}, "exit":0}),
        );
        let mut h = harness().ssh(&ssh).await;
        let request = json!({"ssh":"ssh://worker@box:2222", "source":{"kind":"local", "cli":cli, "daemon":daemon}});
        let _: Value = h
            .json(
                post_json("/v1/hosts/builder/setup", request.clone()),
                axum::http::StatusCode::ACCEPTED,
            )
            .await;
        eventually(TIMEOUT, "worker setup state", || async {
            let state: Value = h.get("/v1/hosts/builder/setup").await;
            state["state"] == "incomplete"
        })
        .await;
        let state: Value = h.get("/v1/hosts/builder/setup").await;
        assert_eq!(state["steps"][3]["complete"], true);
        assert!(
            state["steps"].as_array().unwrap().iter().find(|step| step["name"] == "service").unwrap()["error"]
                .as_str()
                .unwrap()
                .contains(if os == "Linux" {
                    "administrator"
                } else {
                    "launchd"
                })
        );
        assert_eq!(
            std::fs::read(remote.path().join(".local/bin/ariadne")).unwrap(),
            header
        );
        assert_eq!(
            std::fs::read_to_string(&config).unwrap(),
            "prevent_sleep = false\n"
        );
        h.state.hosts.shutdown().await;
        h.state.hosts = ariadne_daemon::hosts::Hosts::new(
            h.launcher.cfg.clone(),
            h.store.clone(),
            h.bus.clone(),
            h.timeouts,
        )
        .await;
        h.router = ariadne_daemon::http::router(h.state.clone());
        let restored: Value = h.get("/v1/hosts/builder/setup").await;
        assert_eq!(restored["state"], "incomplete");
        assert_eq!(restored["steps"][3]["complete"], true);
        assert_eq!(
            std::fs::read_to_string(&unit).unwrap(),
            "# machine-specific service settings\n"
        );
        std::fs::remove_file(fail).unwrap();
        let _: Value = h
            .json(
                post_json("/v1/hosts/builder/setup/resume", json!({})),
                axum::http::StatusCode::ACCEPTED,
            )
            .await;
        eventually(TIMEOUT, "worker setup state", || async {
            let state: Value = h.get("/v1/hosts/builder/setup").await;
            state["state"] == "complete"
        })
        .await;
        let hosts: Vec<HostDto> = h.get("/v1/hosts").await;
        assert!(hosts.iter().any(|h| h.name == "builder"));
        assert_eq!(
            ssh.runs()
                .iter()
                .filter(|r| r.args.last().is_some_and(|s| s.contains("; cat >")))
                .count(),
            2
        );
        h.state.hosts.shutdown().await;
        let restored = ariadne_daemon::hosts::Hosts::new(
            h.launcher.cfg.clone(),
            h.store.clone(),
            h.bus.clone(),
            h.timeouts,
        )
        .await;
        assert!(restored.list().iter().any(|h| h.name == "builder"));
        h.store.delete_host("builder").await.unwrap();
        let without_connection = ariadne_daemon::hosts::Hosts::new(
            h.launcher.cfg.clone(),
            h.store.clone(),
            h.bus.clone(),
            h.timeouts,
        )
        .await;
        assert!(
            !without_connection
                .list()
                .iter()
                .any(|h| h.name == "builder")
        );
        let setup_records =
            std::fs::read_to_string(h.launcher.cfg.root.join("worker-setup.json")).unwrap();
        let records: Value = serde_json::from_str(&setup_records).unwrap();
        assert!(records["builder"].get("remote").is_none());
        assert_eq!(
            std::fs::read_to_string(&unit).unwrap(),
            "# machine-specific service settings\n"
        );
        assert_eq!(std::fs::read_to_string(&plist).unwrap(), old_plist);
    })).await;
}

#[tokio::test]
async fn setup_checks_linux_x86_64_before_it_transfers_a_binary() {
    assert_worker_platform_setup(
        "Linux",
        "x86_64",
        &[
            0x7f, b'E', b'L', b'F', 2, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 62, 0,
        ],
    )
    .await;
}

#[tokio::test]
async fn setup_checks_linux_arm64_before_it_transfers_a_binary() {
    assert_worker_platform_setup(
        "Linux",
        "aarch64",
        &[
            0x7f, b'E', b'L', b'F', 2, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 183, 0,
        ],
    )
    .await;
}

#[tokio::test]
async fn setup_checks_macos_x86_64_before_it_transfers_a_binary() {
    assert_worker_platform_setup(
        "Darwin",
        "x86_64",
        &[0xcf, 0xfa, 0xed, 0xfe, 7, 0, 0, 1, 3, 0, 0, 0, 2, 0, 0, 0],
    )
    .await;
}

#[tokio::test]
async fn setup_checks_macos_arm64_before_it_transfers_a_binary() {
    assert_worker_platform_setup(
        "Darwin",
        "arm64",
        &[0xcf, 0xfa, 0xed, 0xfe, 12, 0, 0, 1, 0, 0, 0, 0, 2, 0, 0, 0],
    )
    .await;
}

async fn assert_worker_platform_setup(os: &str, arch: &str, header: &[u8]) {
    use common::{TIMEOUT, eventually, post_json};
    use serde_json::{Value, json};
    let remote = tempfile::tempdir().unwrap();
    let local = tempfile::tempdir().unwrap();
    let cli = local.path().join("ariadne");
    let daemon = local.path().join("ariadned");
    let mut binary = header.to_vec();
    binary.resize(64, 0);
    if os == "Darwin" && arch == "arm64" {
        // A universal Mach-O with an ARM64 slice at offset 4096.
        let mut universal = vec![0; 4096];
        universal[..8].copy_from_slice(&[0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 1]);
        universal[8..12].copy_from_slice(&[1, 0, 0, 12]);
        universal[16..20].copy_from_slice(&4096u32.to_be_bytes());
        universal[20..24].copy_from_slice(&64u32.to_be_bytes());
        universal.extend_from_slice(&binary);
        binary = universal;
    }
    std::fs::write(&cli, &binary).unwrap();
    std::fs::write(&daemon, b"incompatible daemon").unwrap();
    let captured_path = r#"/remote/.cargo/bin:/remote/npm space/bin:/remote/%h&<"'$HOME\tools/bin:/opt/homebrew/bin:/usr/bin:/bin"#;
    let ssh = common::ssh::stub_ssh(
        json!({"setup":{"home":remote.path(), "os":os, "arch":arch, "path":captured_path}, "exit":0}),
    );
    let h = harness().ssh(&ssh).await;
    let request = json!({"ssh":"ssh://box", "source":{"kind":"local", "cli":cli, "daemon":daemon}});
    let _: Value = h
        .json(
            post_json("/v1/hosts/cross/setup", request.clone()),
            axum::http::StatusCode::ACCEPTED,
        )
        .await;
    eventually(TIMEOUT, "binary validation failure", || async {
        let state: Value = h.get("/v1/hosts/cross/setup").await;
        state["state"] == "incomplete"
    })
    .await;
    assert!(!remote.path().join(".local/bin/ariadne").exists());
    assert!(
        ssh.runs()
            .iter()
            .all(|run| !run.args.last().unwrap().contains("cat >")),
        "{os} {arch}: no binary transfer before validation"
    );
    std::fs::write(&daemon, &binary).unwrap();
    let _: Value = h
        .json(
            post_json("/v1/hosts/cross/setup", request),
            axum::http::StatusCode::ACCEPTED,
        )
        .await;
    eventually(TIMEOUT, "supported worker installation", || async {
        let state: Value = h.get("/v1/hosts/cross/setup").await;
        state["state"] == "complete"
    })
    .await;
    assert_eq!(
        std::fs::read(remote.path().join(".local/bin/ariadned")).unwrap(),
        binary
    );
    let service_file = if os == "Linux" {
        remote.path().join(".config/systemd/user/ariadned.service")
    } else {
        remote
            .path()
            .join("Library/LaunchAgents/dev.ariadne.daemon.plist")
    };
    assert!(
        std::fs::read_to_string(&service_file)
            .unwrap()
            .contains(".local/bin/ariadned")
    );
    let service = std::fs::read_to_string(&service_file).unwrap();
    if os == "Linux" {
        assert!(service.contains(&format!(r#"Environment="PATH={}/.local/bin:/remote/.cargo/bin:/remote/npm space/bin:/remote/%%h&<\"'$HOME\\tools/bin:/opt/homebrew/bin:/usr/bin:/bin""#, remote.path().display())), "{service}");
    } else {
        assert!(
            service.contains("<key>EnvironmentVariables</key>"),
            "{service}"
        );
        assert!(
            service.contains("%h&amp;&lt;&quot;&apos;$HOME"),
            "{service}"
        );
        let parsed = std::process::Command::new("python3").args(["-c", "import plistlib,sys; print(plistlib.load(open(sys.argv[1], 'rb'))['EnvironmentVariables']['PATH'], end='')"]).arg(&service_file).output().unwrap();
        assert!(
            parsed.status.success(),
            "{}",
            String::from_utf8_lossy(&parsed.stderr)
        );
        assert_eq!(
            String::from_utf8(parsed.stdout).unwrap(),
            format!("{}/.local/bin:{captured_path}", remote.path().display())
        );
    }
    h.state.hosts.shutdown().await;
}

#[tokio::test]
async fn release_setup_requires_workflow_and_tag_verification_before_transfer() {
    verify_release_setup(false).await;
}

#[tokio::test]
async fn daemon_version_installs_the_recorded_release_of_a_release_daemon() {
    verify_release_setup(true).await;
}

async fn verify_release_setup(daemon: bool) {
    use common::{TIMEOUT, eventually, post_json};
    use serde_json::{Value, json};
    let remote = tempfile::tempdir().unwrap();
    let gh = common::forge::stub_forge_cli(json!([
        {"args":["--version"], "stdout":"gh version 2.102.0\n"},
        {"args":["release", "download"], "exit":0},
        {"args":["attestation", "verify"], "exit":1, "stderr":"private token must not escape"}
    ]));
    let ssh = common::ssh::stub_ssh(
        json!({"setup":{"home":remote.path(), "maintenance_fixture":true}, "exit":0}),
    );
    let builder = harness().ssh(&ssh).forge_cli(&gh);
    let h = if daemon {
        builder.daemon_build(ariadne_daemon::build::Build {
            repository: Some("acme/ariadne".into()),
            checkout: "/no-release-checkout".into(),
            commit: "0".repeat(40),
            uncommitted: false,
        })
    } else {
        builder
    }
    .await;
    let downloads = h.launcher.cfg.root.join("worker-downloads/released");
    std::fs::create_dir_all(&downloads).unwrap();
    let source = tempfile::tempdir().unwrap();
    let mut binary = vec![0; 64];
    binary[..6].copy_from_slice(b"\x7fELF\x02\x01");
    binary[16] = 2;
    binary[18] = 62;
    for name in ["ariadne", "ariadned"] {
        std::fs::write(source.path().join(name), &binary).unwrap();
    }
    assert!(
        std::process::Command::new("tar")
            .arg("-czf")
            .arg(downloads.join("ariadne-x86_64-unknown-linux-gnu.tar.gz"))
            .arg("-C")
            .arg(source.path())
            .args(["ariadne", "ariadned"])
            .status()
            .unwrap()
            .success()
    );
    let repository = "acme/ariadne";
    let tag = if daemon {
        format!("v{}", env!("CARGO_PKG_VERSION"))
    } else {
        "v1.0.0".into()
    };
    let request = if daemon {
        json!({"ssh":"ssh://box"})
    } else {
        json!({"ssh":"ssh://box", "source":{"kind":"release", "repository":repository, "tag":tag}})
    };
    if daemon {
        let review: Value = h
            .json(
                post_json("/v1/hosts/released/setup/review", request.clone()),
                axum::http::StatusCode::OK,
            )
            .await;
        assert_eq!(review["inspection"]["binaries"]["method"], "release");
        assert_eq!(review["inspection"]["binaries"]["tag"], tag);
        assert_eq!(
            review["inspection"]["binaries"]["version"],
            env!("CARGO_PKG_VERSION")
        );
        assert!(
            group_of(&review["plan"], "install")["detail"]
                .as_str()
                .unwrap()
                .contains(&tag)
        );
    }
    let _: Value = h
        .json(
            post_json("/v1/hosts/released/setup", request.clone()),
            axum::http::StatusCode::ACCEPTED,
        )
        .await;
    eventually(TIMEOUT, "rejected attestation", || async {
        let state: Value = h.get("/v1/hosts/released/setup").await;
        state["state"] == "incomplete"
    })
    .await;
    assert!(
        ssh.runs()
            .iter()
            .all(|run| !run.args.last().unwrap().contains("cat >"))
    );
    let verification = gh
        .invocations()
        .into_iter()
        .find(|call| {
            call.args
                .starts_with(&["attestation".into(), "verify".into()])
        })
        .unwrap();
    assert!(verification.args.windows(2).any(|pair| pair
        == [
            "--signer-workflow",
            &format!("{repository}/.github/workflows/release.yml")
        ]));
    assert!(
        verification
            .args
            .windows(2)
            .any(|pair| pair == ["--source-ref", &format!("refs/tags/{tag}")])
    );
    let state: Value = h.get("/v1/hosts/released/setup").await;
    assert!(!state.to_string().contains("private token"));
    gh.reprogram(json!([
        {"args":["--version"], "stdout":"gh version 2.102.0\n"},
        {"args":["release", "download"], "exit":0},
        {"args":["attestation", "verify"], "exit":0}
    ]));
    let _: Value = h
        .json(
            post_json("/v1/hosts/released/setup", request),
            axum::http::StatusCode::ACCEPTED,
        )
        .await;
    eventually(TIMEOUT, "verified release installation", || async {
        let state: Value = h.get("/v1/hosts/released/setup").await;
        state["state"] == "complete"
    })
    .await;
    assert_eq!(
        std::fs::read(remote.path().join(".local/bin/ariadned")).unwrap(),
        binary
    );
    if daemon {
        std::fs::write(remote.path().join("idle"), "").unwrap();
        let update_downloads = h.launcher.cfg.root.join("worker-downloads/update:released");
        std::fs::create_dir_all(&update_downloads).unwrap();
        let asset = "ariadne-x86_64-unknown-linux-gnu.tar.gz";
        std::fs::copy(downloads.join(asset), update_downloads.join(asset)).unwrap();
        let _: Value = h
            .json(
                post_json(
                    "/v1/hosts/released/update",
                    json!({"source":{"kind":"daemon"}}),
                ),
                axum::http::StatusCode::ACCEPTED,
            )
            .await;
        eventually(TIMEOUT, "daemon version update", || async {
            let status: Value = h.get("/v1/hosts/released/update").await;
            assert!(
                !matches!(status["state"].as_str(), Some("failed" | "incomplete")),
                "{status}"
            );
            status["state"] == "complete"
        })
        .await;
        assert_eq!(
            std::fs::read(remote.path().join(".local/bin/ariadned")).unwrap(),
            binary
        );
    }
    h.state.hosts.shutdown().await;
}

#[tokio::test]
async fn setup_rejects_control_characters_in_remote_path_before_transfer() {
    use common::{TIMEOUT, eventually, post_json};
    use serde_json::{Value, json};
    futures_util::future::join_all([
        "/tools\n",
        "/tools\n/other",
        "/tools\0/bin",
        "/tools\t/bin",
        "/tools\x7f/bin",
        "/tools\u{85}/bin",
        "/tools\r/bin",
    ].map(|path| async move {
        let remote = tempfile::tempdir().unwrap();
        let mut setup = json!({"home":remote.path(), "path":path});
        if path.contains('\0') {
            setup["raw_path"] = json!(path);
        }
        let ssh = common::ssh::stub_ssh(json!({"setup":setup, "exit":0}));
        let h = harness().ssh(&ssh).await;
        let _: Value = h.json(post_json("/v1/hosts/invalid-path/setup", json!({"ssh":"ssh://box", "source":{"kind":"local", "cli":"/missing/ariadne", "daemon":"/missing/ariadned"}})), axum::http::StatusCode::ACCEPTED).await;
        eventually(TIMEOUT, "rejected remote PATH", || async {
            let state: Value = h.get("/v1/hosts/invalid-path/setup").await;
            state["state"] == "failed" || state["state"] == "incomplete"
        })
        .await;
        let state: Value = h.get("/v1/hosts/invalid-path/setup").await;
        assert_eq!(state["state"], "failed", "{path:?}: {state}");
        assert!(
            state["steps"][0]["error"]
                .as_str()
                .unwrap()
                .contains("PATH"),
            "{state}"
        );
        assert_eq!(state["steps"][0]["complete"], false);
        assert_eq!(ssh.runs().len(), 1);
        assert!(!remote.path().join(".local/bin/ariadne").exists());
        h.state.hosts.shutdown().await;
    })).await;
}

#[tokio::test]
async fn setup_uses_the_ssh_path_when_the_login_shell_fails_or_returns_no_path() {
    use common::{TIMEOUT, eventually, post_json};
    use serde_json::{Value, json};
    futures_util::future::join_all([
        json!({"shell_fail":true}),
        json!({"missing_markers":true, "path":"/unmarked/tools:/usr/bin:/bin", "banner":"Welcome!\n"}),
        json!({"path":"", "banner":"Welcome!"}),
    ].map(|mut setup| async move {
        let remote = tempfile::tempdir().unwrap();
        let local = tempfile::tempdir().unwrap();
        let mut binary = vec![0; 64];
        binary[..6].copy_from_slice(b"\x7fELF\x02\x01");
        binary[16] = 2;
        binary[18] = 62;
        let cli = local.path().join("ariadne");
        let daemon = local.path().join("ariadned");
        std::fs::write(&cli, &binary).unwrap();
        std::fs::write(&daemon, &binary).unwrap();
        setup["home"] = json!(remote.path());
        setup["inherited_path"] = json!("/fallback/tools:/usr/bin:/bin");
        let ssh = common::ssh::stub_ssh(json!({"setup":setup, "exit":0}));
        let h = harness().ssh(&ssh).await;
        let _: Value = h
        .json(
            post_json(
                "/v1/hosts/fallback/setup",
                json!({"ssh":"ssh://box", "source":{"kind":"local", "cli":cli, "daemon":daemon}}),
            ),
            axum::http::StatusCode::ACCEPTED,
        )
        .await;
        eventually(TIMEOUT, "fallback PATH installation", || async {
            let state: Value = h.get("/v1/hosts/fallback/setup").await;
            state["state"] == "complete"
                || state["state"] == "failed"
                || state["state"] == "incomplete"
        })
        .await;
        let state: Value = h.get("/v1/hosts/fallback/setup").await;
        assert_eq!(state["state"], "complete", "{state}");
        let unit =
            std::fs::read_to_string(remote.path().join(".config/systemd/user/ariadned.service"))
                .unwrap();
        assert!(unit.contains("KillMode=process"), "{unit}");
        assert!(
            unit.contains(&format!(
                "Environment=\"PATH={}/.local/bin:",
                remote.path().display()
            )),
            "{unit}"
        );
        assert!(unit.contains(":/fallback/tools:/usr/bin:/bin\""), "{unit}");
        h.state.hosts.shutdown().await;
    })).await;
}

#[tokio::test]
async fn setup_ignores_login_banners_with_a_newline_in_service_paths() {
    assert_login_banner_does_not_change_service_path("Welcome to the worker!\n").await;
}

#[tokio::test]
async fn setup_ignores_login_banners_without_a_newline_in_service_paths() {
    assert_login_banner_does_not_change_service_path("Welcome to the worker!").await;
}

async fn assert_login_banner_does_not_change_service_path(banner: &str) {
    use common::{TIMEOUT, eventually, post_json};
    use serde_json::{Value, json};
    for os in ["Linux", "Darwin"] {
        let remote = tempfile::tempdir().unwrap();
        let local = tempfile::tempdir().unwrap();
        let mut binary = vec![0; 64];
        if os == "Linux" {
            binary[..6].copy_from_slice(b"\x7fELF\x02\x01");
            binary[16] = 2;
            binary[18] = 62;
        } else {
            binary[..8].copy_from_slice(&[0xcf, 0xfa, 0xed, 0xfe, 7, 0, 0, 1]);
            binary[12] = 2;
        }
        let cli = local.path().join("ariadne");
        let daemon = local.path().join("ariadned");
        std::fs::write(&cli, &binary).unwrap();
        std::fs::write(&daemon, &binary).unwrap();
        let ssh = common::ssh::stub_ssh(
            json!({"setup": {"home":remote.path(), "os":os, "banner":banner, "suffix":"Goodbye!\n", "path":"/remote/.cargo/bin:/opt/homebrew/bin:/usr/bin:/bin"}, "exit":0}),
        );
        let h = harness().ssh(&ssh).await;
        let _: Value = h.json(post_json("/v1/hosts/banner/setup", json!({"ssh":"ssh://box", "source":{"kind":"local", "cli":cli, "daemon":daemon}})), axum::http::StatusCode::ACCEPTED).await;
        eventually(TIMEOUT, "login banner setup result", || async {
            let state: Value = h.get("/v1/hosts/banner/setup").await;
            state["state"] == "complete"
                || state["state"] == "failed"
                || state["state"] == "incomplete"
        })
        .await;
        let state: Value = h.get("/v1/hosts/banner/setup").await;
        assert_eq!(state["state"], "complete", "{os}: {state}");
        let expected = format!(
            "{}/.local/bin:/remote/.cargo/bin:/opt/homebrew/bin:/usr/bin:/bin",
            remote.path().display()
        );
        if os == "Linux" {
            let unit = std::fs::read_to_string(
                remote.path().join(".config/systemd/user/ariadned.service"),
            )
            .unwrap();
            assert!(
                unit.lines()
                    .any(|line| line == format!("Environment=\"PATH={expected}\"")),
                "{unit}"
            );
        } else {
            let plist = remote
                .path()
                .join("Library/LaunchAgents/dev.ariadne.daemon.plist");
            let parsed = std::process::Command::new("python3").args(["-c", "import plistlib,sys; print(plistlib.load(open(sys.argv[1], 'rb'))['EnvironmentVariables']['PATH'], end='')"]).arg(plist).output().unwrap();
            assert!(
                parsed.status.success(),
                "{}",
                String::from_utf8_lossy(&parsed.stderr)
            );
            assert_eq!(String::from_utf8(parsed.stdout).unwrap(), expected);
        }
        h.state.hosts.shutdown().await;
    }
}

#[tokio::test]
async fn setup_reports_custom_tools_without_executing_install_commands() {
    use serde_json::{Value, json};
    let remote = tempfile::tempdir().unwrap();
    let binaries = tempfile::tempdir().unwrap();
    let cli = binaries.path().join("ariadne");
    let daemon = binaries.path().join("ariadned");
    let mut header = vec![0u8; 64];
    header[..6].copy_from_slice(b"\x7fELF\x02\x01");
    header[16] = 2;
    header[18] = 62;
    std::fs::write(&cli, &header).unwrap();
    std::fs::write(&daemon, &header).unwrap();
    let ssh = common::ssh::stub_ssh(json!({"setup":{"home":remote.path()},"exit":0}));
    let h = harness().ssh(&ssh).await;
    let source = h.launcher.cfg.native_home.join(".codex/auth.json");
    std::fs::create_dir_all(source.parent().unwrap()).unwrap();
    std::fs::write(&source, r#"{"token":"private-test-value"}"#).unwrap();
    let _: Value = h
        .json(
            common::post_json(
                "/v1/hosts/tools/setup",
                json!({
                    "ssh":"ssh://worker@box", "source":{"kind":"local","cli":cli,"daemon":daemon},
                    "tools":{"agents":["custom-agent"],"github":false}, "credentials":["codex"]
                }),
            ),
            axum::http::StatusCode::ACCEPTED,
        )
        .await;
    common::eventually(common::TIMEOUT, "custom installation guidance", || async {
        let state: Value = h.get("/v1/hosts/tools/setup").await;
        state["state"] == "incomplete"
    })
    .await;
    assert_eq!(
        std::fs::read(remote.path().join(".codex/auth.json")).unwrap(),
        std::fs::read(&source).unwrap()
    );
    let state: Value = h.get("/v1/hosts/tools/setup").await;
    let step = state["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "tool:custom-agent")
        .unwrap();
    assert!(!step["complete"].as_bool().unwrap());
    assert!(
        step["error"]
            .as_str()
            .unwrap()
            .contains("manual installation")
    );
    assert!(
        remote
            .path()
            .join(".config/systemd/user/ariadned.service")
            .exists()
    );
    let _: Value = h
        .json(
            common::post_json(
                "/v1/hosts/unsupported/setup",
                json!({
                    "ssh":"ssh://worker@box", "source":{"kind":"local","cli":cli,"daemon":daemon},
                    "credentials":["ssh"]
                }),
            ),
            axum::http::StatusCode::ACCEPTED,
        )
        .await;
    common::eventually(
        common::TIMEOUT,
        "unsupported credential guidance",
        || async {
            let state: Value = h.get("/v1/hosts/unsupported/setup").await;
            state["state"] == "incomplete"
        },
    )
    .await;
    let state: Value = h.get("/v1/hosts/unsupported/setup").await;
    assert!(state["steps"].as_array().unwrap().iter().any(|s| {
        s["name"] == "credential:ssh"
            && s["error"]
                .as_str()
                .is_some_and(|e| e.contains("unsupported credential transfer"))
    }));
    assert!(!remote.path().join(".ssh").exists());
}

#[tokio::test]
async fn selected_credentials_cross_ssh_without_entering_progress_and_retry_skips_the_copy() {
    use serde_json::{Value, json};
    use std::os::unix::fs::PermissionsExt;
    let remote = tempfile::tempdir().unwrap();
    let bin = remote.path().join(".local/bin");
    std::fs::create_dir_all(&bin).unwrap();
    let stub = common::shared_script(
        r#"#!/bin/sh
case ${0##*/} in
node) exit 0;;
npm)
    printf '%s\n' "$*" >> "$HOME/npm-runs"
    case $1 in
      list) [ -e "$HOME/npm-installed" ];;
      install) touch "$HOME/npm-installed";;
      exec) exit 1;;
      *) exit 0;;
    esac;;
codex) [ ! -e "$HOME/fail-auth" ] && [ -s "$HOME/.codex/auth.json" ];;
esac
"#,
    );
    for tool in ["node", "npm", "codex"] {
        std::os::unix::fs::symlink(&stub, bin.join(tool)).unwrap();
    }
    let cli = remote.path().join("source-cli");
    let daemon = remote.path().join("source-daemon");
    let mut header = vec![0u8; 64];
    header[..6].copy_from_slice(b"\x7fELF\x02\x01");
    header[16] = 2;
    header[18] = 62;
    std::fs::write(&cli, &header).unwrap();
    std::fs::write(&daemon, &header).unwrap();
    std::fs::write(remote.path().join("fail-auth"), "").unwrap();
    let ssh = common::ssh::stub_ssh(json!({"setup":{"home":remote.path()},"exit":0}));
    let h = harness().ssh(&ssh).await;
    let source = h.launcher.cfg.native_home.join(".codex/auth.json");
    std::fs::create_dir_all(source.parent().unwrap()).unwrap();
    let secret = r#"{"tokens":{"access_token":"selected-test-secret"}}"#;
    std::fs::write(&source, secret).unwrap();
    let request = json!({"ssh":"ssh://worker@box", "source":{"kind":"local","cli":cli,"daemon":daemon}, "tools":{"agents":["codex-acp"]}, "credentials":["codex"]});
    let mut unselected = request.clone();
    unselected.as_object_mut().unwrap().remove("credentials");
    let _: Value = h
        .json(
            common::post_json("/v1/hosts/unselected/setup", unselected),
            axum::http::StatusCode::ACCEPTED,
        )
        .await;
    common::eventually(common::TIMEOUT, "separate worker login", || async {
        let state: Value = h.get("/v1/hosts/unselected/setup").await;
        state["state"] == "incomplete"
    })
    .await;
    assert!(!remote.path().join(".codex/auth.json").exists());
    let npm_runs = std::fs::read_to_string(remote.path().join("npm-runs")).unwrap();
    assert!(npm_runs.contains("install --global --prefix"));
    assert!(npm_runs.contains("@agentclientprotocol/codex-acp@1.13.1"));
    assert!(!npm_runs.lines().any(|line| line.starts_with("exec ")));
    let state: Value = h.get("/v1/hosts/unselected/setup").await;
    assert!(
        state["steps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| { s["name"] == "tool:codex-acp" && s["complete"] == true })
    );

    let _: Value = h
        .json(
            common::post_json("/v1/hosts/selected/setup", request.clone()),
            axum::http::StatusCode::ACCEPTED,
        )
        .await;
    common::eventually(common::TIMEOUT, "authentication failure", || async {
        let state: Value = h.get("/v1/hosts/selected/setup").await;
        state["state"] == "incomplete"
    })
    .await;
    let state: Value = h.get("/v1/hosts/selected/setup").await;
    assert!(
        state["steps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["name"] == "authentication:codex-acp"
                && s["error"].as_str().is_some_and(|e| e.contains("sign in")))
    );
    let destination = remote.path().join(".codex/auth.json");
    assert_eq!(std::fs::read_to_string(&destination).unwrap(), secret);
    assert_eq!(
        std::fs::metadata(&destination)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert!(!remote.path().join(".config/gh").exists());
    assert!(!state.to_string().contains("selected-test-secret"));
    let persisted = std::fs::read_to_string(h.launcher.cfg.root.join("worker-setup.json")).unwrap();
    assert!(!persisted.contains("selected-test-secret"));
    assert!(!format!("{:?}", ssh.runs()).contains("selected-test-secret"));
    std::fs::remove_file(source).unwrap();
    std::fs::remove_file(remote.path().join("fail-auth")).unwrap();
    let _: Value = h
        .json(
            common::post_json("/v1/hosts/selected/setup", request),
            axum::http::StatusCode::ACCEPTED,
        )
        .await;
    common::eventually(
        common::TIMEOUT,
        "authenticated worker completion",
        || async {
            let state: Value = h.get("/v1/hosts/selected/setup").await;
            state["state"] == "complete"
        },
    )
    .await;
    assert_eq!(
        ssh.runs()
            .iter()
            .filter(|r| r.args.last().is_some_and(|s| s.contains("cat > \"$temp\"")))
            .count(),
        1
    );
    let npm_runs = std::fs::read_to_string(remote.path().join("npm-runs")).unwrap();
    assert!(!npm_runs.lines().any(|line| line.starts_with("exec ")));
    assert_eq!(
        npm_runs
            .lines()
            .filter(|line| line.starts_with("install "))
            .count(),
        1
    );
    assert!(
        npm_runs
            .lines()
            .any(|line| line.starts_with("list --global --prefix ")
                && line.ends_with("@agentclientprotocol/codex-acp@1.13.1"))
    );
}

#[tokio::test]
async fn credential_values_are_rejected_before_an_operation_is_persisted() {
    use serde_json::{Value, json};
    let h = harness().await;
    h.error(
        common::post_json(
            "/v1/hosts/unsafe/setup",
            json!({
                "ssh":"ssh://box", "source":{"kind":"local","cli":"/cli","daemon":"/daemon"},
                "credentials":["secret-token-value"]
            }),
        ),
        axum::http::StatusCode::BAD_REQUEST,
    )
    .await;
    h.error(
        common::get("/v1/hosts/unsafe/setup"),
        axum::http::StatusCode::NOT_FOUND,
    )
    .await;
    assert!(!h.launcher.cfg.root.join("worker-setup.json").exists());
    let doc: Value = h.get("/api-docs/openapi.json").await;
    assert!(doc["components"]["schemas"]["SetupRequest"]["properties"]["tools"].is_object());
}

#[tokio::test]
async fn selected_github_credentials_enable_git_without_entering_command_arguments() {
    use serde_json::{Value, json};
    let remote = tempfile::tempdir().unwrap();
    let bin = remote.path().join(".local/bin");
    std::fs::create_dir_all(&bin).unwrap();
    let gh = common::shared_script(
        r#"#!/bin/sh
case "$*" in
  '--version') echo 'gh version 2.102.0';;
  'auth token --hostname github.com') echo 'github-selected-test-secret';;
  'auth login --hostname github.com --git-protocol https --with-token')
    mkdir -p "$HOME/.config/gh"
    cat > "$HOME/.config/gh/token";;
  'auth status --hostname github.com') [ -s "$HOME/.config/gh/token" ];;
  'auth setup-git --hostname github.com') touch "$HOME/git-auth-configured";;
  *) exit 1;;
esac
"#,
    );
    let gh_path = bin.join("gh");
    std::os::unix::fs::symlink(gh, &gh_path).unwrap();
    let cli = remote.path().join("source-cli");
    let daemon = remote.path().join("source-daemon");
    let mut header = vec![0u8; 64];
    header[..6].copy_from_slice(b"\x7fELF\x02\x01");
    header[16] = 2;
    header[18] = 62;
    std::fs::write(&cli, &header).unwrap();
    std::fs::write(&daemon, &header).unwrap();
    let ssh = common::ssh::stub_ssh(json!({"setup":{"home":remote.path()},"exit":0}));
    let mut h = harness().ssh(&ssh).await;
    let mut cfg = (*h.launcher.cfg).clone();
    cfg.gh_bin = Some(gh_path.display().to_string());
    h.state.hosts = ariadne_daemon::hosts::Hosts::new(
        std::sync::Arc::new(cfg),
        h.store.clone(),
        h.bus.clone(),
        h.timeouts,
    )
    .await;
    h.router = ariadne_daemon::http::router(h.state.clone());
    let _: Value = h
        .json(
            common::post_json(
                "/v1/hosts/forge/setup",
                json!({
                    "ssh":"ssh://box", "source":{"kind":"local","cli":cli,"daemon":daemon},
                    "tools":{"agents":[], "github":true}, "credentials":["github"]
                }),
            ),
            axum::http::StatusCode::ACCEPTED,
        )
        .await;
    common::eventually(
        common::TIMEOUT,
        "GitHub authentication and Git configuration",
        || async {
            let state: Value = h.get("/v1/hosts/forge/setup").await;
            state["state"] == "complete"
        },
    )
    .await;
    assert_eq!(
        std::fs::read_to_string(remote.path().join(".config/gh/token")).unwrap(),
        "github-selected-test-secret\n"
    );
    assert!(remote.path().join("git-auth-configured").exists());
    let state: Value = h.get("/v1/hosts/forge/setup").await;
    assert!(!state.to_string().contains("github-selected-test-secret"));
    assert!(
        !std::fs::read_to_string(h.launcher.cfg.root.join("worker-setup.json"))
            .unwrap()
            .contains("github-selected-test-secret")
    );
    assert!(!format!("{:?}", ssh.runs()).contains("github-selected-test-secret"));
}

#[tokio::test]
async fn local_rejects_an_endpoint_even_when_null() {
    let h = harness().await;
    h.error(
        common::patch_json("/v1/hosts/local", serde_json::json!({"endpoint":null})),
        axum::http::StatusCode::BAD_REQUEST,
    )
    .await;
}

#[tokio::test]
async fn host_routes_persist_nullable_overrides_and_publish_full_rows() {
    use ariadne_api::stream::DomainEvent;
    use axum::http::StatusCode;
    use serde_json::{Value, json};
    let ssh = common::ssh::stub_ssh(json!({"exit":1}));
    let mut h = harness().ssh(&ssh).await;
    let mut events = h.bus.subscribe();
    let added: Value = h.json(common::post_json("/v1/hosts", json!({"name":"builder","endpoint":"ssh://box","remote":"/tmp/worker.sock","color":"teal"})), StatusCode::CREATED).await;
    assert_eq!(added["name"], "builder");
    assert_eq!(added["color"], "teal");
    let updated = common::next_event(&mut events, |e| matches!(&e.event, DomainEvent::HostUpdated(h) if h.name == "builder" && h.color == Some(ariadne_api::hosts::HostColor::Teal))).await;
    let DomainEvent::HostUpdated(row) = updated.event else {
        unreachable!()
    };
    assert_eq!(serde_json::to_value(row).unwrap(), added);
    let changed: Value = h
        .json(
            common::patch_json(
                "/v1/hosts/builder",
                json!({"endpoint":"http://127.0.0.1:1","remote":null,"color":null}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(changed["endpoint"], "http://127.0.0.1:1");
    assert!(changed["remote"].is_null());
    assert!(changed["color"].is_null());
    let local: Value = h
        .json(
            common::patch_json("/v1/hosts/local", json!({"color":"pink"})),
            StatusCode::OK,
        )
        .await;
    assert_eq!(local["color"], "pink");
    h.state.hosts.shutdown().await;
    h.state.hosts = ariadne_daemon::hosts::Hosts::new(
        h.launcher.cfg.clone(),
        h.store.clone(),
        h.bus.clone(),
        h.timeouts,
    )
    .await;
    h.router = ariadne_daemon::http::router(h.state.clone());
    let rows: Value = h.get("/v1/hosts").await;
    assert_eq!(rows[0]["color"], "pink");
    assert_eq!(rows[1]["endpoint"], "http://127.0.0.1:1");
    assert!(rows[1]["color"].is_null());
    let (status, body) = h.send(common::delete("/v1/hosts/builder")).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(body.is_empty());
    let deleted = common::next_event(
        &mut events,
        |e| matches!(&e.event, DomainEvent::HostDeleted(h) if h.name == "builder"),
    )
    .await;
    let DomainEvent::HostDeleted(row) = deleted.event else {
        unreachable!()
    };
    assert_eq!(serde_json::to_value(row).unwrap(), rows[1]);
    assert_eq!(h.store.list_hosts().await.unwrap().len(), 1);
    let cleared: Value = h
        .json(
            common::patch_json("/v1/hosts/local", json!({"color":null})),
            StatusCode::OK,
        )
        .await;
    assert!(cleared["color"].is_null());
}

#[tokio::test]
async fn host_routes_refuse_invalid_fields_duplicates_and_unknown_names() {
    use axum::http::StatusCode;
    use serde_json::{Value, json};
    let h = harness().await;
    for (body, field) in [
        (json!({"name":"local","endpoint":"/tmp/a.sock"}), "local"),
        (json!({"name":"all","endpoint":"/tmp/a.sock"}), "all"),
        (json!({"name":"a b","endpoint":"/tmp/a.sock"}), "a b"),
        (json!({"name":"","endpoint":"/tmp/a.sock"}), "host"),
        (json!({"name":"box","endpoint":"relative"}), "endpoint"),
        (json!({"name":"box","endpoint":"http://box"}), "endpoint"),
        (json!({"name":"box","endpoint":"https://box:1"}), "endpoint"),
        (json!({"name":"box","endpoint":"SSH://box"}), "endpoint"),
        (
            json!({"name":"box","endpoint":"/tmp/a.sock","remote":"/tmp/b.sock"}),
            "remote",
        ),
        (
            json!({"name":"box","endpoint":"ssh://box","remote":"10.0.0.1:1"}),
            "remote",
        ),
    ] {
        let error = h
            .error(
                common::post_json("/v1/hosts", body),
                StatusCode::BAD_REQUEST,
            )
            .await;
        assert!(format!("{error:?}").contains(field));
    }
    let _: Value = h
        .json(
            common::post_json("/v1/hosts", json!({"name":"box","endpoint":"/tmp/a.sock"})),
            StatusCode::CREATED,
        )
        .await;
    h.error(
        common::post_json("/v1/hosts", json!({"name":"box","endpoint":"/tmp/b.sock"})),
        StatusCode::CONFLICT,
    )
    .await;
    h.error(
        common::patch_json("/v1/hosts/box", json!({"endpoint":"relative"})),
        StatusCode::BAD_REQUEST,
    )
    .await;
    h.error(
        common::patch_json("/v1/hosts/local", json!({"remote":null})),
        StatusCode::BAD_REQUEST,
    )
    .await;
    h.error(common::delete("/v1/hosts/local"), StatusCode::BAD_REQUEST)
        .await;
    for request in [
        common::patch_json("/v1/hosts/missing", json!({"color":"red"})),
        common::delete("/v1/hosts/missing"),
    ] {
        let error = h.error(request, StatusCode::NOT_FOUND).await;
        assert!(format!("{error:?}").contains("unknown_host"));
    }
    h.state.hosts.shutdown().await;
}

#[tokio::test]
async fn reconnecting_the_first_host_preserves_database_insertion_order() {
    use axum::http::StatusCode;
    use serde_json::json;
    let mut h = harness().await;
    let replacement = h
        .launcher
        .cfg
        .root
        .join("replacement.sock")
        .display()
        .to_string();
    for name in ["a", "b"] {
        let _: HostDto = h
            .json(
                common::post_json(
                    "/v1/hosts",
                    json!({"name":name,"endpoint":h.launcher.cfg.root.join(format!("{name}.sock")).display().to_string()}),
                ),
                StatusCode::CREATED,
            )
            .await;
    }
    let _: HostDto = h
        .json(
            common::patch_json("/v1/hosts/a", json!({"endpoint":replacement})),
            StatusCode::OK,
        )
        .await;
    let rows: Vec<HostDto> = h.get("/v1/hosts").await;
    assert_eq!(
        rows.iter().map(|h| h.name.as_str()).collect::<Vec<_>>(),
        ["local", "a", "b"]
    );
    h.state.hosts.shutdown().await;
    h.state.hosts = ariadne_daemon::hosts::Hosts::new(
        h.launcher.cfg.clone(),
        h.store.clone(),
        h.bus.clone(),
        h.timeouts,
    )
    .await;
    h.router = ariadne_daemon::http::router(h.state.clone());
    let restored: Vec<HostDto> = h.get("/v1/hosts").await;
    assert_eq!(
        restored.iter().map(|h| h.name.as_str()).collect::<Vec<_>>(),
        ["local", "a", "b"]
    );
    assert_eq!(restored[1].endpoint.as_deref(), Some(replacement.as_str()));
}

#[tokio::test]
async fn updates_wait_for_idle_and_retry_without_replacing_installed_binaries() {
    use axum::http::StatusCode;
    use common::{TIMEOUT, eventually, post_json};
    use serde_json::{Value, json};
    let remote = tempfile::tempdir().unwrap();
    let bin = remote.path().join(".local/bin");
    std::fs::create_dir_all(&bin).unwrap();
    let programs = common::shared_script(
        r#"#!/bin/sh
case ${0##*/} in
brew)
    printf '%s\n' "$*" >> "$HOME/brew-runs"
    case $1 in --prefix) printf '%s\n' "$HOME";; esac;;
npm) printf '%s\n' "$*" >> "$HOME/npm-runs";;
node|codex) exit 0;;
esac
"#,
    );
    for tool in ["brew", "npm", "node", "codex"] {
        std::os::unix::fs::symlink(&programs, bin.join(tool)).unwrap();
    }
    let binaries = tempfile::tempdir().unwrap();
    let cli = binaries.path().join("ariadne");
    let daemon = binaries.path().join("ariadned");
    let mut header = vec![0u8; 64];
    header[..6].copy_from_slice(b"\x7fELF\x02\x01");
    header[16] = 2;
    header[18] = 62;
    std::fs::write(&cli, &header).unwrap();
    std::fs::write(&daemon, &header).unwrap();
    let fail = remote.path().join("fail-service");
    std::fs::write(&fail, "").unwrap();
    let ssh = common::ssh::stub_ssh(
        json!({"setup":{"home":remote.path(), "maintenance_fixture":true, "fail_service":fail}, "exit":0}),
    );
    let h = harness().ssh(&ssh).await;
    let _: Value = h
        .json(
            post_json(
                "/v1/hosts",
                json!({"name":"builder","endpoint":"ssh://box"}),
            ),
            StatusCode::CREATED,
        )
        .await;
    let request = json!({"source":{"kind":"local","cli":cli,"daemon":daemon},"tools":{"agents":["codex-acp"]}});
    let _: Value = h
        .json(
            post_json("/v1/hosts/builder/update", request.clone()),
            StatusCode::ACCEPTED,
        )
        .await;
    eventually(TIMEOUT, "maintenance observation", || async {
        remote.path().join("maintenance-held").exists()
    })
    .await;
    assert!(!remote.path().join(".local/bin/ariadne").exists());
    assert!(!remote.path().join("npm-runs").exists());
    std::fs::write(remote.path().join("idle"), "").unwrap();
    eventually(TIMEOUT, "update service failure", || async {
        h.get::<Value>("/v1/hosts/builder/update").await["state"] == "incomplete"
    })
    .await;
    assert_eq!(
        std::fs::read(remote.path().join(".local/bin/ariadne")).unwrap(),
        header
    );
    assert!(remote.path().join("maintenance-held").exists());
    assert!(remote.path().join("maintenance-resumed").exists());
    std::fs::remove_file(fail).unwrap();
    std::fs::write(
        remote.path().join(".local/bin/ariadne"),
        "installed checkpoint",
    )
    .unwrap();
    let _: Value = h
        .json(
            post_json("/v1/hosts/builder/update", request.clone()),
            StatusCode::ACCEPTED,
        )
        .await;
    eventually(TIMEOUT, "update completion", || async {
        h.get::<Value>("/v1/hosts/builder/update").await["state"] == "complete"
    })
    .await;
    assert_eq!(
        std::fs::read_to_string(remote.path().join(".local/bin/ariadne")).unwrap(),
        "installed checkpoint"
    );
    assert!(remote.path().join("maintenance-resumed").exists());
    let npm = std::fs::read_to_string(remote.path().join("npm-runs")).unwrap();
    assert_eq!(
        npm.lines()
            .filter(|line| line.starts_with("install "))
            .count(),
        2,
        "{npm}"
    );
    assert!(npm.contains("@openai/codex"));
    let brew = std::fs::read_to_string(remote.path().join("brew-runs")).unwrap();
    assert!(brew.contains("upgrade git"), "{brew}");
    assert!(brew.contains("upgrade node"), "{brew}");
    let hosts: Value = h.get("/v1/hosts").await;
    assert!(
        hosts
            .as_array()
            .unwrap()
            .iter()
            .any(|host| host["name"] == "builder" && host["remote"].is_null())
    );
    let _: Value = h
        .json(
            post_json("/v1/hosts/builder/update", request),
            StatusCode::ACCEPTED,
        )
        .await;
    eventually(TIMEOUT, "subsequent explicit update", || async {
        h.get::<Value>("/v1/hosts/builder/update").await["state"] == "complete"
    })
    .await;
    assert_eq!(
        std::fs::read(remote.path().join(".local/bin/ariadne")).unwrap(),
        header
    );
    h.error(
        common::get("/v1/hosts/builder/setup"),
        StatusCode::NOT_FOUND,
    )
    .await;
}

struct UpdateFixture {
    h: common::Harness,
    remote: tempfile::TempDir,
    _ssh: common::ssh::StubSsh,
}

impl UpdateFixture {
    async fn new(options: serde_json::Value) -> Self {
        use axum::http::StatusCode;
        use serde_json::{Value, json};
        let remote = tempfile::tempdir().unwrap();
        let bin = remote.path().join(".local/bin");
        std::fs::create_dir_all(&bin).unwrap();
        let program = common::shared_script(
            r#"#!/bin/sh
case ${0##*/} in
brew) case $1 in --prefix) printf '%s\n' "$HOME";; esac;;
npm|node|codex) exit 0;;
esac
"#,
        );
        for tool in ["brew", "npm", "node", "codex"] {
            std::os::unix::fs::symlink(&program, bin.join(tool)).unwrap();
        }
        let mut header = vec![0u8; 64];
        header[..6].copy_from_slice(b"\x7fELF\x02\x01");
        header[16] = 2;
        header[18] = 62;
        for file in ["source-cli", "source-daemon"] {
            std::fs::write(remote.path().join(file), &header).unwrap();
        }
        std::fs::write(remote.path().join("idle"), "").unwrap();
        let mut setup = options;
        setup["home"] = json!(remote.path());
        setup["maintenance_fixture"] = json!(true);
        let ssh = common::ssh::stub_ssh(json!({"setup":setup,"exit":0}));
        let h = harness().ssh(&ssh).await;
        let _: Value = h
            .json(
                common::post_json(
                    "/v1/hosts",
                    json!({"name":"builder","endpoint":"ssh://saved@box:2222"}),
                ),
                StatusCode::CREATED,
            )
            .await;
        Self {
            h,
            remote,
            _ssh: ssh,
        }
    }

    fn request(&self) -> serde_json::Value {
        serde_json::json!({"source":{"kind":"local","cli":self.remote.path().join("source-cli"),"daemon":self.remote.path().join("source-daemon")}})
    }

    async fn start(&self, request: serde_json::Value) {
        let _: serde_json::Value = self
            .h
            .json(
                common::post_json("/v1/hosts/builder/update", request),
                axum::http::StatusCode::ACCEPTED,
            )
            .await;
    }

    async fn terminal(&self) -> serde_json::Value {
        common::eventually(common::TIMEOUT, "terminal worker update", || async {
            matches!(
                self.h
                    .get::<serde_json::Value>("/v1/hosts/builder/update")
                    .await["state"]
                    .as_str(),
                Some("complete" | "incomplete" | "failed")
            )
        })
        .await;
        self.h.get("/v1/hosts/builder/update").await
    }
}

#[tokio::test]
async fn updates_use_saved_ssh_and_allow_binary_only_selection() {
    let f = UpdateFixture::new(serde_json::json!({})).await;
    f.start(f.request()).await;
    assert_eq!(f.terminal().await["state"], "complete");
    assert!(f.remote.path().join(".local/bin/ariadne").exists());
    assert!(f.remote.path().join("maintenance-resumed").exists());
}

#[tokio::test]
async fn tools_only_updates_preserve_worker_binaries() {
    let f = UpdateFixture::new(serde_json::json!({})).await;
    std::fs::write(f.remote.path().join(".local/bin/ariadne"), "existing CLI").unwrap();
    f.start(serde_json::json!({"tools":{"agents":[]}})).await;
    assert_eq!(f.terminal().await["state"], "complete");
    assert_eq!(
        std::fs::read_to_string(f.remote.path().join(".local/bin/ariadne")).unwrap(),
        "existing CLI"
    );
    assert!(!f.remote.path().join(".local/bin/ariadned").exists());
}

#[tokio::test]
async fn unsupported_tools_can_be_removed_on_retry_without_reinstalling_binaries() {
    let f = UpdateFixture::new(serde_json::json!({})).await;
    let mut request = f.request();
    request["tools"] = serde_json::json!({"agents":["unknown-agent"]});
    f.start(request).await;
    let result = f.terminal().await;
    assert_eq!(result["state"], "incomplete");
    assert!(result["steps"].as_array().unwrap().iter().any(|s| {
        s["name"] == "tool:unknown-agent"
            && s["error"]
                .as_str()
                .is_some_and(|e| e.contains("unsupported"))
    }));
    assert!(f.remote.path().join("maintenance-resumed").exists());
    std::fs::write(
        f.remote.path().join(".local/bin/ariadne"),
        "retained installation",
    )
    .unwrap();
    f.start(f.request()).await;
    assert_eq!(f.terminal().await["state"], "complete");
    assert_eq!(
        std::fs::read_to_string(f.remote.path().join(".local/bin/ariadne")).unwrap(),
        "retained installation"
    );
}

#[tokio::test]
async fn invalid_binary_sources_can_change_on_retry() {
    let f = UpdateFixture::new(serde_json::json!({})).await;
    let invalid = f.remote.path().join("invalid-cli");
    std::fs::write(&invalid, "incompatible").unwrap();
    let mut request = f.request();
    request["source"]["cli"] = serde_json::json!(invalid);
    f.start(request).await;
    assert_eq!(f.terminal().await["state"], "incomplete");
    assert!(!f.remote.path().join("maintenance-held").exists());
    f.start(f.request()).await;
    assert_eq!(f.terminal().await["state"], "complete");
}

#[tokio::test]
async fn unsupported_replacements_fail_before_installation_or_maintenance() {
    let f = UpdateFixture::new(serde_json::json!({"replacement_maintenance":false})).await;
    f.start(f.request()).await;
    let result = f.terminal().await;
    assert_eq!(result["state"], "incomplete");
    assert!(result["steps"].as_array().unwrap().iter().any(|s| {
        s["error"]
            .as_str()
            .is_some_and(|e| e.contains("maintenance"))
    }));
    assert!(!f.remote.path().join(".local/bin/ariadne").exists());
    assert!(!f.remote.path().join("maintenance-held").exists());
}

#[tokio::test]
async fn failed_updates_keep_maintenance_when_worker_health_fails() {
    let f = UpdateFixture::new(serde_json::json!({"health_failure":true})).await;
    let mut request = f.request();
    request["tools"] = serde_json::json!({"agents":["unknown-agent"]});
    f.start(request).await;
    let result = f.terminal().await;
    assert_eq!(result["state"], "incomplete");
    assert!(f.remote.path().join("maintenance-held").exists());
    assert!(!f.remote.path().join("maintenance-resumed").exists());
    assert!(
        result["steps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["name"] == "resume" && s["error"].is_string())
    );
}

#[tokio::test]
async fn unsupported_current_workers_fail_before_binary_transfer() {
    let f = UpdateFixture::new(serde_json::json!({"worker_maintenance":false})).await;
    f.start(f.request()).await;
    let result = f.terminal().await;
    assert_eq!(result["state"], "incomplete");
    assert!(
        result["steps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["name"] == "worker-capability" && s["error"].is_string())
    );
    assert!(
        !f.remote
            .path()
            .join(".ariadne/worker-setup-update:builder")
            .exists()
    );
    assert!(!f.remote.path().join("maintenance-held").exists());
}

#[tokio::test]
async fn revised_binary_sources_invalidate_the_install_checkpoint() {
    let f = UpdateFixture::new(serde_json::json!({})).await;
    let mut request = f.request();
    request["tools"] = serde_json::json!({"agents":["unknown-agent"]});
    f.start(request).await;
    assert_eq!(f.terminal().await["state"], "incomplete");
    let old = std::fs::read(f.remote.path().join(".local/bin/ariadne")).unwrap();
    let mut replacement = old.clone();
    replacement.extend_from_slice(b"selected replacement");
    let source = f.remote.path().join("replacement-cli");
    std::fs::write(&source, &replacement).unwrap();
    let mut request = f.request();
    request["source"]["cli"] = serde_json::json!(source);
    f.start(request).await;
    assert_eq!(f.terminal().await["state"], "complete");
    assert_eq!(
        std::fs::read(f.remote.path().join(".local/bin/ariadne")).unwrap(),
        replacement
    );
}

#[tokio::test]
async fn explicit_updates_upgrade_already_installed_uv_tools() {
    let f = UpdateFixture::new(serde_json::json!({})).await;
    let uv = common::shared_script(
        r#"#!/bin/sh
case "$*" in
"--version") exit 0;;
"python install") exit 0;;
"tool install --upgrade -- "*) printf 'current' > "$HOME/uv-version";;
"tool install -- "*) exit 0;;
*) exit 1;;
esac
"#,
    );
    std::os::unix::fs::symlink(uv, f.remote.path().join(".local/bin/uv")).unwrap();
    std::fs::write(f.remote.path().join("uv-version"), "older").unwrap();
    f.start(serde_json::json!({"tools":{"agents":["fast-agent"]}}))
        .await;
    let result = f.terminal().await;
    assert!(
        result["steps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| { s["name"] == "tool:fast-agent" && s["complete"] == true }),
        "{result}"
    );
    assert_eq!(
        std::fs::read_to_string(f.remote.path().join("uv-version")).unwrap(),
        "current"
    );
}

/// An update on a worker whose runtimes the user installed by hand. With `homebrew`, a
/// Homebrew that owns none of them logs its calls to `$HOME/brew.log`. Each stub in
/// `tools` logs its arguments to `$HOME/<name>.log` and succeeds.
async fn update_of_hand_installed_runtimes(
    agent: &str,
    tools: &[&str],
    homebrew: bool,
) -> (UpdateFixture, serde_json::Value) {
    let f = UpdateFixture::new(serde_json::json!({})).await;
    let bin = f.remote.path().join(".local/bin");
    std::fs::remove_file(bin.join("brew")).unwrap();
    if homebrew {
        let brew = common::shared_script(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$HOME/brew.log\"\ncase $1 in --prefix) printf '%s\\n' \"$HOME\";; list) exit 1;; esac\nexit 0\n",
        );
        std::os::unix::fs::symlink(brew, bin.join("brew")).unwrap();
    }
    std::fs::remove_file(bin.join("npm")).unwrap();
    for tool in tools {
        let stub = common::shared_script(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$HOME/${0##*/}.log\"\ncase \"$*\" in \"tool list\") echo 'fast-agent-acp v0.0.1';; esac\nexit 0\n",
        );
        let link = bin.join(tool);
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink(stub, link).unwrap();
    }
    f.start(serde_json::json!({"tools":{"agents":[agent]}}))
        .await;
    let result = f.terminal().await;
    (f, result)
}

#[tokio::test]
async fn an_update_reuses_hand_installed_node_and_npm_without_homebrew() {
    let (f, result) = update_of_hand_installed_runtimes("codex-acp", &["npm"], false).await;
    let step = result["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "tool:codex-acp")
        .unwrap();
    assert_eq!(step["complete"], true, "{result}");
    let log = std::fs::read_to_string(f.remote.path().join("npm.log")).unwrap();
    assert!(log.contains("install --global"), "{log}");
}

#[tokio::test]
async fn an_update_reuses_a_hand_installed_uv_without_homebrew() {
    let (f, result) = update_of_hand_installed_runtimes("fast-agent", &["uv"], false).await;
    let step = result["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "tool:fast-agent")
        .unwrap();
    assert_eq!(step["complete"], true, "{result}");
    let log = std::fs::read_to_string(f.remote.path().join("uv.log")).unwrap();
    assert!(log.contains("tool install --upgrade"), "{log}");
}

/// Homebrew is there, and it owns neither runtime nor Git: an update installs none of them.
#[tokio::test]
async fn an_update_installs_no_second_runtime_beside_a_hand_installed_one() {
    for (agent, tool, log) in [
        ("codex-acp", "npm", "install --global"),
        ("fast-agent", "uv", "tool install --upgrade"),
    ] {
        let (f, result) = update_of_hand_installed_runtimes(agent, &[tool], true).await;
        let name = format!("tool:{agent}");
        for step in [name.as_str(), "tool:git"] {
            let item = result["steps"]
                .as_array()
                .unwrap()
                .iter()
                .find(|s| s["name"] == step)
                .unwrap();
            assert_eq!(item["complete"], true, "{result}");
        }
        let tool_log =
            std::fs::read_to_string(f.remote.path().join(format!("{tool}.log"))).unwrap();
        assert!(tool_log.contains(log), "{tool_log}");
        let brew = std::fs::read_to_string(f.remote.path().join("brew.log")).unwrap_or_default();
        assert!(
            !brew.contains("install") && !brew.contains("upgrade"),
            "{agent}: {brew}"
        );
    }
}

/// Homebrew reports success but leaves no runtime on the PATH: the update names the runtime.
#[tokio::test]
async fn an_update_names_the_runtime_that_homebrew_did_not_provide() {
    for (agent, removed, expected) in [
        ("codex-acp", ["node", "npm"], "Node.js and npm are missing"),
        ("fast-agent", ["uv", "uv"], "uv is missing"),
    ] {
        let f = UpdateFixture::new(serde_json::json!({})).await;
        let bin = f.remote.path().join(".local/bin");
        for tool in removed {
            let _ = std::fs::remove_file(bin.join(tool));
        }
        f.start(serde_json::json!({"tools":{"agents":[agent]}}))
            .await;
        let result = f.terminal().await;
        assert_eq!(result["state"], "incomplete", "{agent}: {result}");
        assert!(result.to_string().contains(expected), "{agent}: {result}");
    }
}

#[tokio::test]
async fn inspection_needs_only_ssh_and_does_not_install_or_register() {
    use serde_json::{Value, json};
    let remote = tempfile::tempdir().unwrap();
    let ssh = common::ssh::stub_ssh(json!({"setup":{"home":remote.path()},"exit":0}));
    let h = harness().ssh(&ssh).await;
    let result: Value = h
        .json(
            common::post_json(
                "/v1/hosts/builder/setup/inspect",
                json!({"ssh":"ssh://worker@box"}),
            ),
            axum::http::StatusCode::OK,
        )
        .await;
    assert_eq!(result["target"], "x86_64-unknown-linux-gnu");
    assert!(
        result["checks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["id"] == "service")
    );
    assert!(std::fs::read_dir(remote.path()).unwrap().next().is_none());
    assert!(!remote.path().join(".ariadne").exists());
    let hosts: Value = h.get("/v1/hosts").await;
    assert_eq!(hosts.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn review_lists_transfers_and_manual_actions_without_changing_the_worker() {
    use serde_json::{Value, json};
    let remote = tempfile::tempdir().unwrap();
    let ssh = common::ssh::stub_ssh(json!({"setup":{"home":remote.path()},"exit":0}));
    let h = harness().ssh(&ssh).await;
    let source = h.launcher.cfg.native_home.join(".codex/auth.json");
    std::fs::create_dir_all(source.parent().unwrap()).unwrap();
    std::fs::write(source, r#"{"token":"review-private-value"}"#).unwrap();
    let result: Value = h.json(common::post_json("/v1/hosts/builder/setup/review", json!({
        "ssh":"ssh://worker@box", "source":{"kind":"local","cli":"/missing","daemon":"/missing"},
        "tools":{"agents":["custom-agent"],"github":false}, "credentials":["codex"]
    })), axum::http::StatusCode::OK).await;
    assert!(
        result["actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a.as_str().unwrap().contains("Codex"))
    );
    assert!(
        result["prerequisites"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["id"] == "binaries" && p["satisfied"] == false)
    );
    assert!(
        result["preserved"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p.as_str().unwrap().contains("machine"))
    );
    assert!(!result.to_string().contains("review-private-value"));
    assert!(!remote.path().join(".local").exists());
}

/// A worker PATH with Git, Node.js and npm, and with no GitHub CLI, uv, Homebrew or user service.
fn worker_with_git_and_node(remote: &std::path::Path) -> std::path::PathBuf {
    let bin = remote.join("worker-bin");
    std::fs::create_dir(&bin).unwrap();
    std::os::unix::fs::symlink("/usr/bin/git", bin.join("git")).unwrap();
    let present = common::shared_script("#!/bin/sh\nexit 0\n");
    for tool in ["node", "npm"] {
        std::os::unix::fs::symlink(&present, bin.join(tool)).unwrap();
    }
    bin
}

fn group_of<'v>(items: &'v serde_json::Value, id: &str) -> &'v serde_json::Value {
    items
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == id)
        .unwrap_or_else(|| panic!("no {id} in {items}"))
}

#[tokio::test]
async fn inspection_names_who_installs_each_agent_github_and_each_sign_in() {
    use serde_json::{Value, json};
    let remote = tempfile::tempdir().unwrap();
    let bin = worker_with_git_and_node(remote.path());
    let ssh = common::ssh::stub_ssh(json!({"setup":{"home":remote.path(),"path":bin},"exit":0}));
    let h = harness().ssh(&ssh).await;
    let inspect = || async {
        h.json::<Value>(
            common::post_json(
                "/v1/hosts/builder/setup/inspect",
                json!({"ssh":"ssh://worker@box"}),
            ),
            axum::http::StatusCode::OK,
        )
        .await
    };
    common::eventually(common::TIMEOUT, "the stub agent is ready here", || async {
        group_of(&inspect().await["agents"], common::STUB)["on_this_machine"] == true
    })
    .await;
    let result = inspect().await;
    let agents = &result["agents"];
    let codex = group_of(agents, "codex-acp");
    assert_eq!(codex["name"], "Codex");
    assert_eq!(codex["group"], "ariadne_installs");
    assert_eq!(codex["manual"], Value::Null);
    assert_eq!(codex["sign_in"], "codex");
    let uv = group_of(agents, "fast-agent");
    assert_eq!(uv["group"], "manual");
    assert_eq!(
        uv["manual"]["command"],
        "curl -LsSf https://astral.sh/uv/install.sh | sh"
    );
    let binary = group_of(agents, "amp-acp");
    assert_eq!(binary["group"], "manual");
    assert!(binary["manual"]["reason"].as_str().unwrap().contains("Amp"));
    assert_eq!(binary["manual"]["command"], Value::Null);
    let custom = group_of(agents, common::STUB);
    assert_eq!(custom["group"], "manual");
    assert!(
        custom["manual"]["reason"]
            .as_str()
            .unwrap()
            .contains("configuration")
    );
    assert_eq!(result["github"]["group"], "manual");
    assert!(
        result["github"]["manual"]["reason"]
            .as_str()
            .unwrap()
            .contains("GitHub CLI")
    );
    assert_eq!(
        result["sign_ins"],
        json!([
            {"id":"codex","name":"Codex","copy_supported":true,"command":"codex login"},
            {"id":"github","name":"GitHub","copy_supported":true,"command":"gh auth login --hostname github.com"},
            {"id":"claude","name":"Claude","copy_supported":false,"command":"claude auth login"},
            {"id":"opencode","name":"OpenCode","copy_supported":false,"command":"opencode auth login"},
            {"id":"ssh","name":"SSH keys","copy_supported":false,"command":null}
        ])
    );
}

#[tokio::test]
async fn review_plans_each_requested_item_in_one_of_three_groups() {
    use serde_json::{Value, json};
    let remote = tempfile::tempdir().unwrap();
    let bin = worker_with_git_and_node(remote.path());
    let ssh = common::ssh::stub_ssh(json!({"setup":{"home":remote.path(),"path":bin},"exit":0}));
    let h = harness().ssh(&ssh).await;
    let source = h.launcher.cfg.native_home.join(".codex/auth.json");
    std::fs::create_dir_all(source.parent().unwrap()).unwrap();
    std::fs::write(source, r#"{"token":"plan-private-value"}"#).unwrap();
    let result: Value = h.json(common::post_json("/v1/hosts/builder/setup/review", json!({
        "ssh":"ssh://worker@box", "source":{"kind":"local","cli":"/missing","daemon":"/missing"},
        "tools":{"agents":["codex-acp","amp-acp"],"github":true}, "credentials":["codex","claude"]
    })), axum::http::StatusCode::OK).await;
    let plan = &result["plan"];
    let ids: Vec<_> = plan
        .as_array()
        .unwrap()
        .iter()
        .map(|i| (i["id"].as_str().unwrap(), i["group"].as_str().unwrap()))
        .collect();
    assert_eq!(
        ids,
        [
            ("install", "manual"),
            ("tool:git", "already_present"),
            ("tool:github", "manual"),
            ("tool:codex-acp", "ariadne_installs"),
            ("tool:amp-acp", "manual"),
            ("credential:codex", "ariadne_installs"),
            ("credential:claude", "manual"),
            ("authentication:github", "manual"),
            ("authentication:codex-acp", "ariadne_installs"),
            ("authentication:amp-acp", "manual"),
            ("service", "manual"),
            ("register", "ariadne_installs"),
        ]
    );
    for item in plan.as_array().unwrap() {
        assert!(!item["title"].as_str().unwrap().is_empty(), "{item}");
        assert!(!item["detail"].as_str().unwrap().is_empty(), "{item}");
        assert_eq!(
            item["group"] == "manual",
            item["manual"]["reason"].is_string(),
            "{item}"
        );
    }
    assert_eq!(
        group_of(plan, "credential:claude")["manual"]["command"],
        "claude auth login"
    );
    assert_eq!(
        group_of(plan, "authentication:github")["manual"]["command"],
        "gh auth login --hostname github.com"
    );
    assert_eq!(
        group_of(plan, "service")["manual"]["command"],
        "sudo loginctl enable-linger \"$USER\""
    );
    assert_eq!(group_of(plan, "install")["manual"]["command"], Value::Null);
    assert!(!result.to_string().contains("plan-private-value"));
    assert!(!remote.path().join(".local").exists());
}

#[tokio::test]
async fn a_step_that_waits_for_manual_work_says_what_its_plan_item_says() {
    use serde_json::{Value, json};
    let remote = tempfile::tempdir().unwrap();
    let bin = worker_with_git_and_node(remote.path());
    let ssh = common::ssh::stub_ssh(json!({"setup":{"home":remote.path(),"path":bin},"exit":0}));
    let h = harness().ssh(&ssh).await;
    let request = json!({
        "ssh":"ssh://worker@box", "source":{"kind":"local","cli":"/missing","daemon":"/missing"},
        "tools":{"agents":["amp-acp"],"github":false}
    });
    let review: Value = h
        .json(
            common::post_json("/v1/hosts/manual/setup/review", request.clone()),
            axum::http::StatusCode::OK,
        )
        .await;
    let _: Value = h
        .json(
            common::post_json("/v1/hosts/manual/setup", request),
            axum::http::StatusCode::ACCEPTED,
        )
        .await;
    common::eventually(common::TIMEOUT, "setup stops for manual work", || async {
        let state: Value = h.get("/v1/hosts/manual/setup").await;
        state["state"] == "incomplete"
    })
    .await;
    let state: Value = h.get("/v1/hosts/manual/setup").await;
    let steps = state["steps"].as_array().unwrap();
    for step in steps {
        assert!(!step["title"].as_str().unwrap().is_empty(), "{step}");
    }
    assert!(
        steps
            .iter()
            .any(|s| s["name"] == "tool:amp-acp" && s["title"] == "Install Amp")
    );
    let waiting: Vec<_> = steps.iter().filter(|s| !s["manual"].is_null()).collect();
    assert_eq!(
        waiting
            .iter()
            .map(|s| s["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["validate", "tool:amp-acp", "authentication:amp-acp"],
        "{state}"
    );
    for step in waiting {
        let item = match step["name"].as_str().unwrap() {
            "validate" => "install",
            name => name,
        };
        assert_eq!(
            step["manual"],
            group_of(&review["plan"], item)["manual"],
            "{step}"
        );
    }
}

/// A worker like [`worker_with_git_and_node`] whose `.local` npm prefix already
/// holds the shipped Codex ACP package and the Codex CLI. Every npm install fails.
fn worker_with_codex_installed(remote: &std::path::Path) -> std::path::PathBuf {
    let bin = worker_with_git_and_node(remote);
    let index: serde_json::Value =
        serde_json::from_str(ariadne_daemon::acp_discovery::SHIPPED_INDEX).unwrap();
    let spec = index["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == "codex-acp")
        .unwrap()["distribution"]["npx"]["package"]
        .as_str()
        .unwrap()
        .to_owned();
    let npm = common::shared_script(&format!(
        "#!/bin/sh\ncase \"$1\" in\nlist) case \"$*\" in *--parseable*) printf '%s\\n' \"$HOME/.local/lib/node_modules/codex-acp:{spec}\" \"$HOME/.local/lib/node_modules/@openai/codex:@openai/codex@1.0.0\";; esac; exit 0;;\n--version) exit 0;;\n*) exit 1;;\nesac\n"
    ));
    std::fs::remove_file(bin.join("npm")).unwrap();
    std::os::unix::fs::symlink(npm, bin.join("npm")).unwrap();
    bin
}

#[tokio::test]
async fn an_agent_package_already_on_the_worker_is_already_present() {
    use serde_json::{Value, json};
    let remote = tempfile::tempdir().unwrap();
    let bin = worker_with_codex_installed(remote.path());
    let ssh = common::ssh::stub_ssh(json!({"setup":{"home":remote.path(),"path":bin},"exit":0}));
    let h = harness().ssh(&ssh).await;
    let inspection: Value = h
        .json(
            common::post_json(
                "/v1/hosts/builder/setup/inspect",
                json!({"ssh":"ssh://worker@box"}),
            ),
            axum::http::StatusCode::OK,
        )
        .await;
    assert_eq!(
        group_of(&inspection["agents"], "codex-acp")["group"],
        "already_present"
    );
    assert_eq!(
        group_of(&inspection["agents"], "codex-acp")["manual"],
        Value::Null
    );
    // The Claude ACP package is not in the prefix, so setup installs it.
    assert_eq!(
        group_of(&inspection["agents"], "claude-acp")["group"],
        "ariadne_installs"
    );
    let review: Value = h.json(common::post_json("/v1/hosts/builder/setup/review", json!({
        "ssh":"ssh://worker@box", "source":{"kind":"local","cli":"/missing","daemon":"/missing"},
        "tools":{"agents":["codex-acp"],"github":false}
    })), axum::http::StatusCode::OK).await;
    assert_eq!(
        group_of(&review["plan"], "tool:codex-acp")["group"],
        "already_present"
    );
}

#[tokio::test]
async fn progress_puts_a_step_that_reused_a_present_item_in_already_present() {
    use serde_json::{Value, json};
    let remote = tempfile::tempdir().unwrap();
    let bin = worker_with_codex_installed(remote.path());
    let ssh = common::ssh::stub_ssh(json!({"setup":{"home":remote.path(),"path":bin},"exit":0}));
    let h = harness().ssh(&ssh).await;
    let request = json!({
        "ssh":"ssh://worker@box", "source":{"kind":"local","cli":"/missing","daemon":"/missing"},
        "tools":{"agents":["codex-acp"],"github":false}
    });
    let _: Value = h
        .json(
            common::post_json("/v1/hosts/present/setup", request),
            axum::http::StatusCode::ACCEPTED,
        )
        .await;
    common::eventually(common::TIMEOUT, "setup stops for manual work", || async {
        let state: Value = h.get("/v1/hosts/present/setup").await;
        state["state"] == "incomplete"
    })
    .await;
    let state: Value = h.get("/v1/hosts/present/setup").await;
    let groups: Vec<_> = state["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| (s["name"].as_str().unwrap(), s["group"].as_str().unwrap()))
        .collect();
    assert_eq!(
        groups,
        [
            ("probe", "ariadne_installs"),
            ("validate", "manual"),
            ("transfer", "ariadne_installs"),
            ("install", "ariadne_installs"),
            ("tool:git", "already_present"),
            ("tool:codex-acp", "already_present"),
            ("authentication:codex-acp", "manual"),
            ("service", "ariadne_installs"),
            ("health", "ariadne_installs"),
            ("register", "ariadne_installs"),
        ],
        "{state}"
    );
}

#[tokio::test]
async fn missing_binaries_do_not_block_an_independent_selected_credential_transfer() {
    use serde_json::{Value, json};
    let remote = tempfile::tempdir().unwrap();
    let ssh = common::ssh::stub_ssh(json!({"setup":{"home":remote.path()},"exit":0}));
    let h = harness().ssh(&ssh).await;
    let source = h.launcher.cfg.native_home.join(".codex/auth.json");
    std::fs::create_dir_all(source.parent().unwrap()).unwrap();
    std::fs::write(&source, r#"{"token":"test-private-value"}"#).unwrap();
    let _: Value = h.json(common::post_json("/v1/hosts/partial/setup", json!({"ssh":"ssh://worker@box", "source":{"kind":"local","cli":"/missing","daemon":"/missing"}, "credentials":["codex"]})), axum::http::StatusCode::ACCEPTED).await;
    common::eventually(common::TIMEOUT, "partial setup", || async {
        let state: Value = h.get("/v1/hosts/partial/setup").await;
        state["state"] == "incomplete"
    })
    .await;
    let state: Value = h.get("/v1/hosts/partial/setup").await;
    assert!(
        state["steps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["name"] == "credential:codex" && s["complete"] == true)
    );
    assert_eq!(
        std::fs::read(remote.path().join(".codex/auth.json")).unwrap(),
        std::fs::read(source).unwrap()
    );
    assert!(!state.to_string().contains("test-private-value"));
    let hosts: Vec<HostDto> = h.get("/v1/hosts").await;
    assert!(!hosts.iter().any(|h| h.name == "partial"));
}

#[tokio::test]
async fn binary_only_setup_requires_git_and_resumes_after_manual_installation() {
    use serde_json::{Value, json};
    let remote = tempfile::tempdir().unwrap();
    let bin = remote.path().join("prerequisites");
    std::fs::create_dir(&bin).unwrap();
    for tool in ["sh", "mkdir", "cat", "chmod", "mv", "rmdir", "id"] {
        let source = ["/bin", "/usr/bin"]
            .into_iter()
            .map(|dir| std::path::Path::new(dir).join(tool))
            .find(|path| path.exists())
            .unwrap();
        std::os::unix::fs::symlink(source, bin.join(tool)).unwrap();
    }
    let binaries = tempfile::tempdir().unwrap();
    let cli = binaries.path().join("ariadne");
    let daemon = binaries.path().join("ariadned");
    let mut header = vec![0u8; 64];
    header[..6].copy_from_slice(b"\x7fELF\x02\x01");
    header[16] = 2;
    header[18] = 62;
    std::fs::write(&cli, &header).unwrap();
    std::fs::write(&daemon, &header).unwrap();
    let ssh = common::ssh::stub_ssh(json!({"setup":{
        "home":remote.path(), "path":bin, "inherited_path":bin
    },"exit":0}));
    let h = harness().ssh(&ssh).await;
    let request = json!({"ssh":"ssh://worker@box", "source":{
        "kind":"local", "cli":cli, "daemon":daemon
    }});
    let _: Value = h
        .json(
            common::post_json("/v1/hosts/builder/setup", request),
            axum::http::StatusCode::ACCEPTED,
        )
        .await;
    common::eventually(common::TIMEOUT, "setup ends", || async {
        let state: Value = h.get("/v1/hosts/builder/setup").await;
        !matches!(state["state"].as_str(), Some("pending" | "running"))
    })
    .await;
    let state: Value = h.get("/v1/hosts/builder/setup").await;
    assert_eq!(state["state"], "incomplete", "{state}");
    assert!(
        state["steps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["name"] == "prerequisite:git"
                && s["error"].as_str().is_some_and(|e| e.contains("Git"))),
        "{state}"
    );
    assert!(remote.path().join(".local/bin/ariadne").exists());
    let hosts: Vec<HostDto> = h.get("/v1/hosts").await;
    assert!(!hosts.iter().any(|h| h.name == "builder"));
    let transfers = || {
        ssh.runs()
            .iter()
            .filter(|run| {
                run.args
                    .last()
                    .is_some_and(|s| s.contains("cat >") && s.contains("worker-setup-builder/"))
            })
            .count()
    };
    let before = transfers();
    assert_eq!(before, 2);
    std::os::unix::fs::symlink("/usr/bin/git", bin.join("git")).unwrap();
    let _: Value = h
        .json(
            common::post_json("/v1/hosts/builder/setup/resume", json!({})),
            axum::http::StatusCode::ACCEPTED,
        )
        .await;
    common::eventually(common::TIMEOUT, "setup completes with Git", || async {
        let state: Value = h.get("/v1/hosts/builder/setup").await;
        state["state"] == "complete"
    })
    .await;
    assert_eq!(transfers(), before);
    let hosts: Vec<HostDto> = h.get("/v1/hosts").await;
    assert_eq!(hosts.iter().filter(|h| h.name == "builder").count(), 1);
}

/// One saved operation, as `worker-setup.json` stores it.
fn saved_operation(name: &str, state: &str, tools: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "request": {
            "ssh": format!("ssh://me@{name}.example:2222"),
            "source": {"kind": "release", "repository": "acme/ariadne", "tag": "v1.0.0"},
            "tools": tools,
            "credentials": ["codex"],
        },
        "progress": {"name": name.trim_start_matches("update:"), "state": state, "steps": [
            {"name": "probe", "complete": true, "error": null},
            {"name": "service", "complete": false, "error": "Enable user services"},
        ]},
    })
}

#[tokio::test]
async fn saved_setups_are_listed_without_credentials_and_only_a_stopped_one_is_discarded() {
    use axum::http::StatusCode;
    use serde_json::{Value, json};
    let remote = tempfile::tempdir().unwrap();
    let ssh = common::ssh::stub_ssh(
        json!({"setup": {"home":remote.path(), "os":"Linux", "path":"/usr/bin"}, "exit":0}),
    );
    let mut h = harness().ssh(&ssh).await;
    let tools = json!({"agents": ["codex-acp"], "github": true});
    let records = json!({
        "alpha": saved_operation("alpha", "incomplete", tools.clone()),
        "beta": saved_operation("beta", "failed", Value::Null),
        "gamma": saved_operation("gamma", "complete", Value::Null),
        "update:gamma": saved_operation("update:gamma", "incomplete", Value::Null),
    });
    std::fs::write(
        h.launcher.cfg.root.join("worker-setup.json"),
        records.to_string(),
    )
    .unwrap();
    h.state.hosts.shutdown().await;
    h.state.hosts = ariadne_daemon::hosts::Hosts::new(
        h.launcher.cfg.clone(),
        h.store.clone(),
        h.bus.clone(),
        h.timeouts,
    )
    .await;
    h.router = ariadne_daemon::http::router(h.state.clone());

    let (status, body) = h.send(common::get("/v1/host-setups")).await;
    assert_eq!(status, StatusCode::OK);
    let text = String::from_utf8(body).unwrap();
    assert!(
        !text.contains("credentials") && !text.contains("codex\""),
        "{text}"
    );
    let listed: Value = serde_json::from_str(&text).unwrap();
    let row = |name: &str, operation: &str| {
        listed
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["name"] == name && r["operation"] == operation)
            .unwrap_or_else(|| panic!("no {operation} row for {name}: {text}"))
            .clone()
    };
    assert_eq!(listed.as_array().unwrap().len(), 4, "{text}");
    let alpha = row("alpha", "setup");
    assert_eq!(alpha["ssh"], "ssh://me@alpha.example:2222");
    assert_eq!(alpha["state"], "incomplete");
    assert_eq!(alpha["tools"], tools);
    assert_eq!(alpha["steps"][1]["error"], "Enable user services");
    assert!(row("beta", "setup")["tools"].is_null());
    assert_eq!(row("gamma", "setup")["state"], "complete");
    assert_eq!(row("gamma", "update")["state"], "incomplete");

    for name in ["alpha", "beta"] {
        let (status, body) = h
            .send(common::delete(&format!("/v1/hosts/{name}/setup")))
            .await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{name}");
        assert!(body.is_empty());
        h.error(
            common::get(&format!("/v1/hosts/{name}/setup")),
            StatusCode::NOT_FOUND,
        )
        .await;
    }
    assert!(ssh.runs().is_empty(), "a discard must not reach the worker");
    let listed: Value = h.get("/v1/host-setups").await;
    assert_eq!(listed.as_array().unwrap().len(), 2, "{listed}");
    let saved: Value = serde_json::from_slice(
        &std::fs::read(h.launcher.cfg.root.join("worker-setup.json")).unwrap(),
    )
    .unwrap();
    assert!(saved.get("alpha").is_none() && saved.get("beta").is_none());

    let refused = h
        .error(
            common::delete("/v1/hosts/gamma/setup"),
            StatusCode::CONFLICT,
        )
        .await;
    assert!(refused.error.message.contains("complete"), "{refused:?}");
    h.error(
        common::delete("/v1/hosts/update:gamma/setup"),
        StatusCode::NOT_FOUND,
    )
    .await;
    h.error(
        common::delete("/v1/hosts/nobody/setup"),
        StatusCode::NOT_FOUND,
    )
    .await;
    assert_eq!(
        h.get::<Value>("/v1/hosts/gamma/setup").await["state"],
        "complete"
    );

    let again: Value = h
        .json(
            common::post_json(
                "/v1/hosts/alpha/setup",
                json!({"ssh": "ssh://me@alpha.example:2222", "source": {"kind": "release", "repository": "acme/ariadne", "tag": "v1.0.0"}}),
            ),
            StatusCode::ACCEPTED,
        )
        .await;
    assert_eq!(again["steps"][0]["name"], "probe");
    assert!(
        again["steps"]
            .as_array()
            .unwrap()
            .iter()
            .all(|s| s["complete"] == false)
    );
}

/// A daemon that is a source build: a checkout of a commit, with an
/// uncommitted edit and an untracked file, and a worker whose PATH has the
/// programs setup runs. Stand-ins answer for the C compiler, the linker,
/// curl, rustup and cargo, so nothing is downloaded or built.
struct SourceBuild {
    remote: tempfile::TempDir,
    bin: std::path::PathBuf,
    /// A stand-in curl that answers with a stand-in rustup installer.
    installer: std::path::PathBuf,
    build: ariadne_daemon::build::Build,
    _checkout: tempfile::TempDir,
}

impl SourceBuild {
    async fn new(compiler: bool, curl: bool) -> Self {
        let checkout_dir = tempfile::tempdir().unwrap();
        let checkout = common::git_repo_copy(checkout_dir.path(), "checkout").await;
        let head = std::process::Command::new("git")
            .arg("-C")
            .arg(&checkout)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();
        let commit = String::from_utf8(head.stdout).unwrap().trim().to_owned();
        std::fs::write(checkout.join("file.txt"), "uncommitted edit\n").unwrap();
        std::fs::write(checkout.join("untracked.txt"), "untracked\n").unwrap();
        let remote = tempfile::tempdir().unwrap();
        let bin = remote.path().join("worker-bin");
        std::fs::create_dir(&bin).unwrap();
        for tool in [
            "sh", "mkdir", "cat", "chmod", "mv", "rmdir", "id", "rm", "cp", "ln", "mktemp", "tar",
            "gzip", "git",
        ] {
            let source = ["/bin", "/usr/bin"]
                .into_iter()
                .map(|dir| std::path::Path::new(dir).join(tool))
                .find(|path| path.exists())
                .unwrap();
            std::os::unix::fs::symlink(source, bin.join(tool)).unwrap();
        }
        // The stand-in builds each program from the file.txt of the source it runs in.
        let cargo = common::shared_script(
            r#"#!/bin/sh
case $1 in --version) echo 'cargo 1.0.0'; exit 0;; esac
printf '%s %s\n' "$ARIADNE_SOURCE_COMMIT" "$*" >> "$HOME/cargo-runs"
mkdir -p "$CARGO_TARGET_DIR/release"
for program in ariadne ariadned; do
    cp file.txt "$CARGO_TARGET_DIR/release/$program"
    chmod 755 "$CARGO_TARGET_DIR/release/$program"
done
"#,
        );
        let installer = common::shared_script(&format!(
            r#"#!/bin/sh
printf '%s\n' "$*" > "$HOME/curl-args"
cat <<'INSTALLER'
printf '%s\n' "$*" > "$HOME/rustup-args"
mkdir -p "$HOME/.cargo/bin"
ln -s '{}' "$HOME/.cargo/bin/cargo"
INSTALLER
"#,
            cargo.display()
        ));
        let administrator =
            common::shared_script("#!/bin/sh\n: > \"$HOME/administrator-command\"\nexit 1\n");
        for tool in ["sudo", "apt-get"] {
            std::os::unix::fs::symlink(&administrator, bin.join(tool)).unwrap();
        }
        let source = Self {
            remote,
            bin,
            installer,
            build: ariadne_daemon::build::Build {
                repository: None,
                checkout: checkout.display().to_string(),
                commit,
                uncommitted: true,
            },
            _checkout: checkout_dir,
        };
        if compiler {
            source.add_compiler();
        }
        if curl {
            source.add_curl();
        }
        source
    }

    fn add_curl(&self) {
        std::os::unix::fs::symlink(&self.installer, self.bin.join("curl")).unwrap();
    }

    fn add_compiler(&self) {
        let present = common::shared_script("#!/bin/sh\nexit 0\n");
        for tool in ["cc", "ld"] {
            std::os::unix::fs::symlink(&present, self.bin.join(tool)).unwrap();
        }
    }

    fn ssh(&self, mut setup: serde_json::Value) -> common::ssh::StubSsh {
        setup["home"] = serde_json::json!(self.remote.path());
        setup["path"] = serde_json::json!(self.bin);
        setup["inherited_path"] = serde_json::json!(self.bin);
        common::ssh::stub_ssh(serde_json::json!({"setup":setup,"exit":0}))
    }

    fn cargo_runs(&self) -> Vec<String> {
        std::fs::read_to_string(self.remote.path().join("cargo-runs"))
            .unwrap_or_default()
            .lines()
            .map(String::from)
            .collect()
    }

    /// Each worker command, and that none of them runs an administrator command.
    fn commands(&self, ssh: &common::ssh::StubSsh) -> Vec<String> {
        assert!(!self.remote.path().join("administrator-command").exists());
        let commands: Vec<String> = ssh
            .runs()
            .iter()
            .map(|run| run.args.last().unwrap().clone())
            .collect();
        assert!(commands.iter().all(|c| !c.contains("sudo")));
        commands
    }
}

async fn setup_ends(h: &common::Harness, name: &str) -> serde_json::Value {
    let path = format!("/v1/hosts/{name}/setup");
    common::eventually(common::TIMEOUT, "setup ends", || async {
        let state: serde_json::Value = h.get(&path).await;
        !matches!(state["state"].as_str(), Some("pending" | "running"))
    })
    .await;
    h.get(&path).await
}

fn step_of<'v>(state: &'v serde_json::Value, name: &str) -> &'v serde_json::Value {
    state["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == name)
        .unwrap_or_else(|| panic!("no step {name} in {state}"))
}

#[tokio::test]
async fn a_source_build_daemon_builds_its_commit_on_the_new_machine() {
    use serde_json::{Value, json};
    let source = SourceBuild::new(true, true).await;
    let ssh = source.ssh(json!({}));
    let h = harness().ssh(&ssh).daemon_build(source.build.clone()).await;
    let request = json!({"ssh":"ssh://worker@box"});
    let review: Value = h
        .json(
            common::post_json("/v1/hosts/builder/setup/review", request.clone()),
            axum::http::StatusCode::OK,
        )
        .await;
    let binaries = &review["inspection"]["binaries"];
    assert_eq!(binaries["method"], "source_build");
    assert_eq!(binaries["commit"], source.build.commit.as_str());
    assert_eq!(binaries["uncommitted"], true);
    let install = group_of(&review["plan"], "install");
    assert_eq!(install["group"], "ariadne_installs", "{install}");
    let detail = install["detail"].as_str().unwrap();
    assert!(detail.contains(&source.build.commit), "{detail}");
    assert!(detail.contains("commit only"), "{detail}");
    for (id, group) in [
        ("prerequisite:compiler", "already_present"),
        ("prerequisite:curl", "already_present"),
        ("source", "ariadne_installs"),
        ("rust", "ariadne_installs"),
        ("build", "ariadne_installs"),
    ] {
        assert_eq!(group_of(&review["plan"], id)["group"], group, "{id}");
    }
    let _: Value = h
        .json(
            common::post_json("/v1/hosts/builder/setup", request),
            axum::http::StatusCode::ACCEPTED,
        )
        .await;
    let state = setup_ends(&h, "builder").await;
    assert_eq!(state["state"], "complete", "{state}");
    for (name, title) in [
        ("source", "Copy the Ariadne source"),
        ("rust", "Install Rust"),
        ("build", "Build the Ariadne programs"),
    ] {
        let step = step_of(&state, name);
        assert_eq!(step["title"], title);
        assert_eq!(step["complete"], true);
        assert_eq!(step["group"], "ariadne_installs");
    }
    let remote = source.remote.path();
    // The programs come from the commit, without the uncommitted edit or the untracked file.
    for program in ["ariadne", "ariadned"] {
        assert_eq!(
            std::fs::read_to_string(remote.join(".local/bin").join(program)).unwrap(),
            "v1\n"
        );
    }
    assert!(remote.join(".ariadne/worker-source/file.txt").exists());
    assert!(!remote.join(".ariadne/worker-source/untracked.txt").exists());
    assert_eq!(
        source.cargo_runs(),
        [format!(
            "{} build --release --locked --bin ariadne --bin ariadned",
            source.build.commit
        )]
    );
    // Rust came from rustup, in the user's home, without a change to the shell profile.
    assert!(remote.join(".cargo/bin/cargo").exists());
    let rustup = std::fs::read_to_string(remote.join("rustup-args")).unwrap();
    assert!(rustup.contains("-y") && rustup.contains("--no-modify-path"));
    assert!(
        std::fs::read_to_string(remote.join("curl-args"))
            .unwrap()
            .contains("https://sh.rustup.rs")
    );
    source.commands(&ssh);
    let hosts: Vec<HostDto> = h.get("/v1/hosts").await;
    assert!(hosts.iter().any(|h| h.name == "builder"));
    let review: Value = h
        .json(
            common::post_json(
                "/v1/hosts/other/setup/review",
                json!({"ssh":"ssh://worker@box"}),
            ),
            axum::http::StatusCode::OK,
        )
        .await;
    assert_eq!(
        group_of(&review["plan"], "rust")["group"],
        "already_present"
    );
}

#[tokio::test]
async fn a_source_build_is_refused_before_transfer_without_its_checkout_or_commit() {
    use serde_json::{Value, json};
    let source = SourceBuild::new(true, true).await;
    let missing_checkout = ariadne_daemon::build::Build {
        checkout: "/no/such/ariadne-checkout".into(),
        ..source.build.clone()
    };
    let missing_commit = ariadne_daemon::build::Build {
        commit: "f".repeat(40),
        ..source.build.clone()
    };
    for (build, missing) in [
        (missing_checkout, "/no/such/ariadne-checkout".to_owned()),
        (missing_commit, "f".repeat(40)),
    ] {
        let ssh = source.ssh(json!({}));
        let h = harness().ssh(&ssh).daemon_build(build).await;
        let request = json!({"ssh":"ssh://worker@box", "credentials":["codex"]});
        let credential = h.launcher.cfg.native_home.join(".codex/auth.json");
        std::fs::create_dir_all(credential.parent().unwrap()).unwrap();
        std::fs::write(credential, r#"{"token":"source-build-secret"}"#).unwrap();
        let review: Value = h
            .json(
                common::post_json("/v1/hosts/builder/setup/review", request.clone()),
                axum::http::StatusCode::OK,
            )
            .await;
        let install = group_of(&review["plan"], "install");
        assert_eq!(install["group"], "manual", "{install}");
        assert!(
            install["manual"]["reason"]
                .as_str()
                .unwrap()
                .contains(&missing)
        );
        let _: Value = h
            .json(
                common::post_json("/v1/hosts/builder/setup", request),
                axum::http::StatusCode::ACCEPTED,
            )
            .await;
        let state = setup_ends(&h, "builder").await;
        let validate = step_of(&state, "validate");
        let error = validate["error"].as_str().unwrap();
        assert!(
            error.contains(&missing) && error.contains("missing"),
            "{error}"
        );
        assert_eq!(validate["manual"]["reason"], error);
        assert_eq!(validate["group"], "manual");
        assert!(
            source
                .commands(&ssh)
                .iter()
                .all(|c| !c.contains("tar -xzf"))
        );
        assert!(!source.remote.path().join(".ariadne/worker-source").exists());
        assert!(!source.remote.path().join(".codex/auth.json").exists());
        assert!(source.cargo_runs().is_empty());
    }
}

#[tokio::test]
async fn a_missing_compiler_is_manual_and_a_retry_does_not_build_the_same_commit_again() {
    use serde_json::{Value, json};
    let source = SourceBuild::new(false, true).await;
    let ssh = source.ssh(json!({}));
    let h = harness().ssh(&ssh).daemon_build(source.build.clone()).await;
    let request = json!({"ssh":"ssh://worker@box"});
    let review: Value = h
        .json(
            common::post_json("/v1/hosts/builder/setup/review", request.clone()),
            axum::http::StatusCode::OK,
        )
        .await;
    let compiler = group_of(&review["plan"], "prerequisite:compiler");
    assert_eq!(compiler["group"], "manual", "{compiler}");
    assert_eq!(
        compiler["manual"]["command"],
        "sudo apt-get install -y build-essential"
    );
    let _: Value = h
        .json(
            common::post_json("/v1/hosts/builder/setup", request),
            axum::http::StatusCode::ACCEPTED,
        )
        .await;
    let state = setup_ends(&h, "builder").await;
    assert_eq!(state["state"], "incomplete", "{state}");
    let step = step_of(&state, "prerequisite:compiler");
    assert_eq!(step["group"], "manual");
    assert_eq!(step["manual"], compiler["manual"]);
    assert_eq!(step_of(&state, "source")["complete"], false);
    assert!(source.cargo_runs().is_empty());
    // The compiler arrives, and the first retry fails after the build, at the installation.
    source.add_compiler();
    let local = source.remote.path().join(".local");
    std::fs::write(&local, "").unwrap();
    let _: Value = h
        .json(
            common::post_json("/v1/hosts/builder/setup/resume", json!({})),
            axum::http::StatusCode::ACCEPTED,
        )
        .await;
    let state = setup_ends(&h, "builder").await;
    assert_eq!(state["state"], "incomplete", "{state}");
    assert_eq!(step_of(&state, "prerequisite:compiler")["complete"], true);
    assert_eq!(step_of(&state, "build")["complete"], true);
    assert!(step_of(&state, "install")["error"].is_string(), "{state}");
    assert_eq!(source.cargo_runs().len(), 1);
    std::fs::remove_file(&local).unwrap();
    let _: Value = h
        .json(
            common::post_json("/v1/hosts/builder/setup/resume", json!({})),
            axum::http::StatusCode::ACCEPTED,
        )
        .await;
    let state = setup_ends(&h, "builder").await;
    assert_eq!(state["state"], "complete", "{state}");
    assert_eq!(source.cargo_runs().len(), 1);
    assert_eq!(step_of(&state, "build")["group"], "already_present");
    assert_eq!(
        source
            .commands(&ssh)
            .iter()
            .filter(|c| c.contains("tar -xzf"))
            .count(),
        1
    );
    assert_eq!(
        std::fs::read_to_string(source.remote.path().join(".local/bin/ariadned")).unwrap(),
        "v1\n"
    );
}

#[tokio::test]
async fn an_update_from_a_source_build_daemon_builds_before_it_holds_maintenance() {
    use serde_json::{Value, json};
    let source = SourceBuild::new(true, true).await;
    std::fs::write(source.remote.path().join("idle"), "").unwrap();
    let ssh = source.ssh(json!({"maintenance_fixture":true}));
    let h = harness().ssh(&ssh).daemon_build(source.build.clone()).await;
    let _: Value = h
        .json(
            common::post_json(
                "/v1/hosts",
                json!({"name":"builder","endpoint":"ssh://saved@box:2222"}),
            ),
            axum::http::StatusCode::CREATED,
        )
        .await;
    let _: Value = h
        .json(
            common::post_json(
                "/v1/hosts/builder/update",
                json!({"source":{"kind":"daemon"}}),
            ),
            axum::http::StatusCode::ACCEPTED,
        )
        .await;
    common::eventually(common::TIMEOUT, "update ends", || async {
        let state: Value = h.get("/v1/hosts/builder/update").await;
        !matches!(state["state"].as_str(), Some("pending" | "running"))
    })
    .await;
    let state: Value = h.get("/v1/hosts/builder/update").await;
    assert_eq!(state["state"], "complete", "{state}");
    let names: Vec<&str> = state["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    let at = |name: &str| names.iter().position(|n| *n == name).unwrap();
    assert!(at("validate") < at("source") && at("source") < at("rust"));
    assert!(at("rust") < at("build") && at("build") < at("replacement-capability"));
    assert!(at("replacement-capability") < at("maintenance") && at("maintenance") < at("install"));
    assert!(!names.contains(&"transfer"));
    assert!(source.remote.path().join("maintenance-held").exists());
    assert!(source.remote.path().join("maintenance-resumed").exists());
    assert_eq!(source.cargo_runs().len(), 1);
    assert_eq!(
        std::fs::read_to_string(source.remote.path().join(".local/bin/ariadned")).unwrap(),
        "v1\n"
    );
    source.commands(&ssh);
}

#[tokio::test]
async fn a_compiler_that_cannot_build_with_the_c_headers_is_a_manual_item() {
    use serde_json::{Value, json};
    let source = SourceBuild::new(false, true).await;
    // cc and ld answer for their versions, but no C program compiles or links.
    let headless = common::shared_script("#!/bin/sh\n[ \"$1\" = --version ] || [ \"$1\" = -v ]\n");
    for tool in ["cc", "ld"] {
        std::os::unix::fs::symlink(&headless, source.bin.join(tool)).unwrap();
    }
    let ssh = source.ssh(json!({}));
    let h = harness().ssh(&ssh).daemon_build(source.build.clone()).await;
    let request = json!({"ssh":"ssh://worker@box"});
    let review: Value = h
        .json(
            common::post_json("/v1/hosts/builder/setup/review", request.clone()),
            axum::http::StatusCode::OK,
        )
        .await;
    let compiler = group_of(&review["plan"], "prerequisite:compiler");
    assert_eq!(compiler["group"], "manual", "{compiler}");
    assert_eq!(
        compiler["manual"]["command"],
        "sudo apt-get install -y build-essential"
    );
    assert!(
        compiler["manual"]["reason"]
            .as_str()
            .unwrap()
            .contains("C library headers")
    );
    let _: Value = h
        .json(
            common::post_json("/v1/hosts/builder/setup", request),
            axum::http::StatusCode::ACCEPTED,
        )
        .await;
    let state = setup_ends(&h, "builder").await;
    assert_eq!(state["state"], "incomplete", "{state}");
    assert_eq!(
        step_of(&state, "prerequisite:compiler")["manual"],
        compiler["manual"]
    );
    assert!(source.cargo_runs().is_empty());
    source.commands(&ssh);
}

#[tokio::test]
async fn an_unusable_cargo_without_curl_needs_curl_and_a_resume_installs_rust() {
    use serde_json::{Value, json};
    let source = SourceBuild::new(true, false).await;
    // A rustup shim without a toolchain: cargo is on the PATH, but does not run.
    std::os::unix::fs::symlink(
        common::shared_script("#!/bin/sh\nexit 1\n"),
        source.bin.join("cargo"),
    )
    .unwrap();
    let ssh = source.ssh(json!({}));
    let h = harness().ssh(&ssh).daemon_build(source.build.clone()).await;
    let request = json!({"ssh":"ssh://worker@box"});
    let review: Value = h
        .json(
            common::post_json("/v1/hosts/builder/setup/review", request.clone()),
            axum::http::StatusCode::OK,
        )
        .await;
    assert_eq!(
        group_of(&review["plan"], "rust")["group"],
        "ariadne_installs"
    );
    let curl = group_of(&review["plan"], "prerequisite:curl");
    assert_eq!(curl["group"], "manual", "{curl}");
    assert_eq!(curl["manual"]["command"], "sudo apt-get install -y curl");
    let _: Value = h
        .json(
            common::post_json("/v1/hosts/builder/setup", request),
            axum::http::StatusCode::ACCEPTED,
        )
        .await;
    let state = setup_ends(&h, "builder").await;
    assert_eq!(state["state"], "incomplete", "{state}");
    assert_eq!(
        step_of(&state, "prerequisite:curl")["manual"],
        curl["manual"]
    );
    assert_eq!(step_of(&state, "rust")["complete"], false);
    source.add_curl();
    let _: Value = h
        .json(
            common::post_json("/v1/hosts/builder/setup/resume", json!({})),
            axum::http::StatusCode::ACCEPTED,
        )
        .await;
    let state = setup_ends(&h, "builder").await;
    assert_eq!(state["state"], "complete", "{state}");
    assert!(source.remote.path().join(".cargo/bin/cargo").exists());
    assert_eq!(source.cargo_runs().len(), 1);
    source.commands(&ssh);
}

/// A worker whose `~/.local/bin` holds Node.js and npm, a Codex CLI that is not
/// signed in, and a GitHub CLI and Homebrew that do not work. The GitHub CLI
/// waits on the `hold` FIFO of the home while it exists. Each CLI logs its runs in the home.
fn worker_with_broken_github_and_no_codex_sign_in(remote: &std::path::Path) {
    let bin = remote.join(".local/bin");
    std::fs::create_dir_all(&bin).unwrap();
    let stub = common::shared_script(
        r#"#!/bin/sh
case ${0##*/} in
node) exit 0;;
npm)
    case $1 in
      list) [ -e "$HOME/npm-installed" ];;
      install) touch "$HOME/npm-installed";;
      *) exit 0;;
    esac;;
codex) printf '%s\n' "$*" >> "$HOME/codex-runs"; exit 1;;
gh)
    printf '%s\n' "$*" >> "$HOME/gh-runs"
    if [ -p "$HOME/hold" ]; then read -r _ < "$HOME/hold" || :; fi
    exit 1;;
brew) exit 1;;
esac
"#,
    );
    for tool in ["node", "npm", "codex", "gh", "brew"] {
        std::os::unix::fs::symlink(&stub, bin.join(tool)).unwrap();
    }
}

/// A FIFO that holds each reader until [`Hold::release`]. A drop, also on a
/// failed test, wakes any reader that waits and removes the FIFO.
struct Hold(std::path::PathBuf);

impl Hold {
    fn new(path: std::path::PathBuf) -> Self {
        let made = std::process::Command::new("mkfifo")
            .arg(&path)
            .status()
            .unwrap();
        assert!(made.success(), "mkfifo {}", path.display());
        Self(path)
    }

    /// Wait for a reader, remove the FIFO, then let the reader go.
    async fn release(self) {
        let path = self.0.clone();
        let writer = tokio::task::spawn_blocking(move || {
            std::fs::OpenOptions::new().write(true).open(path).unwrap()
        })
        .await
        .unwrap();
        drop(self);
        drop(writer);
    }
}

impl Drop for Hold {
    fn drop(&mut self) {
        use rustix::fs::{Mode, OFlags};
        let _ = rustix::fs::open(&self.0, OFlags::WRONLY | OFlags::NONBLOCK, Mode::empty());
        let _ = std::fs::remove_file(&self.0);
    }
}

async fn skip(
    h: &common::Harness,
    name: &str,
    step: &str,
    status: axum::http::StatusCode,
) -> serde_json::Value {
    let (got, body) = h
        .send(common::post_json(
            &format!("/v1/hosts/{name}/setup/steps/{step}/skip"),
            serde_json::json!({}),
        ))
        .await;
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(got, status, "skip {step}: {body}");
    body
}

#[tokio::test]
async fn skipped_failed_tool_and_sign_in_steps_let_setup_complete_and_connect_the_machine() {
    use axum::http::StatusCode;
    use serde_json::{Value, json};
    let remote = tempfile::tempdir().unwrap();
    worker_with_broken_github_and_no_codex_sign_in(remote.path());
    let mut header = vec![0u8; 64];
    header[..6].copy_from_slice(b"\x7fELF\x02\x01");
    header[16] = 2;
    header[18] = 62;
    let (cli, daemon) = (
        remote.path().join("source-cli"),
        remote.path().join("source-daemon"),
    );
    std::fs::write(&cli, &header).unwrap();
    std::fs::write(&daemon, &header).unwrap();
    let hold = Hold::new(remote.path().join("hold"));
    let ssh = common::ssh::stub_ssh(json!({"setup":{"home":remote.path()},"exit":0}));
    let h = harness().ssh(&ssh).await;
    let request = json!({
        "ssh":"ssh://worker@box", "source":{"kind":"local","cli":cli,"daemon":daemon},
        "tools":{"agents":["codex-acp"],"github":true}
    });
    let _: Value = h
        .json(
            common::post_json("/v1/hosts/skipper/setup", request),
            StatusCode::ACCEPTED,
        )
        .await;
    let gh_runs = remote.path().join("gh-runs");
    common::eventually(common::TIMEOUT, "the GitHub CLI check runs", || async {
        gh_runs.exists()
    })
    .await;
    let refused = skip(&h, "skipper", "tool:github", StatusCode::CONFLICT).await;
    assert_eq!(refused["error"]["code"], "conflict", "{refused}");
    hold.release().await;

    let stopped = setup_ends(&h, "skipper").await;
    assert_eq!(stopped["state"], "incomplete", "{stopped}");
    let failed = [
        "tool:github",
        "authentication:github",
        "authentication:codex-acp",
    ];
    for name in failed {
        let step = step_of(&stopped, name);
        assert!(step["error"].is_string(), "{step}");
        assert_eq!(step["skipped"], false, "{step}");
    }
    let hosts: Vec<HostDto> = h.get("/v1/hosts").await;
    assert!(!hosts.iter().any(|h| h.name == "skipper"));

    for (step, status) in [
        ("tool:codex-acp", StatusCode::BAD_REQUEST),
        ("tool:git", StatusCode::BAD_REQUEST),
        ("service", StatusCode::BAD_REQUEST),
        ("tool:nothing", StatusCode::NOT_FOUND),
    ] {
        let refused = skip(&h, "skipper", step, status).await;
        assert!(refused["error"]["message"].is_string(), "{refused}");
    }
    skip(&h, "nobody", "tool:github", StatusCode::NOT_FOUND).await;

    let mut progress = Value::Null;
    for name in failed {
        progress = skip(&h, "skipper", name, StatusCode::OK).await;
        let (before, after) = (step_of(&stopped, name), step_of(&progress, name));
        assert_eq!(after["skipped"], true, "{after}");
        assert_eq!(after["group"], "skipped", "{after}");
        assert_eq!(after["error"], before["error"], "{after}");
        assert_eq!(after["manual"], before["manual"], "{after}");
    }
    assert_eq!(progress["state"], "incomplete", "{progress}");
    let codex_runs = || std::fs::read_to_string(remote.path().join("codex-runs")).unwrap();
    let (codex_before, gh_before) = (codex_runs(), std::fs::read_to_string(&gh_runs).unwrap());

    let _: Value = h
        .json(
            common::post_json("/v1/hosts/skipper/setup/resume", json!({})),
            StatusCode::ACCEPTED,
        )
        .await;
    let done = setup_ends(&h, "skipper").await;
    assert_eq!(done["state"], "complete", "{done}");
    for name in failed {
        let step = step_of(&done, name);
        assert_eq!(step["skipped"], true, "{step}");
        assert_eq!(step["group"], "skipped", "{step}");
        assert!(step["error"].is_string(), "{step}");
    }
    assert_eq!(
        codex_runs(),
        codex_before,
        "a skipped sign-in check ran again"
    );
    assert_eq!(
        std::fs::read_to_string(&gh_runs).unwrap(),
        gh_before,
        "a skipped GitHub step ran again"
    );
    let hosts: Vec<HostDto> = h.get("/v1/hosts").await;
    assert_eq!(hosts.iter().filter(|h| h.name == "skipper").count(), 1);
}

#[tokio::test]
async fn a_skipped_failed_step_of_an_update_lets_the_update_complete() {
    use axum::http::StatusCode;
    let f = UpdateFixture::new(serde_json::json!({})).await;
    let mut request = f.request();
    request["tools"] = serde_json::json!({"agents":["unknown-agent"]});
    for step in ["tool:unknown-agent", "authentication:unknown-agent"] {
        f.start(request.clone()).await;
        let stopped = f.terminal().await;
        assert_eq!(stopped["state"], "incomplete", "{stopped}");
        assert!(step_of(&stopped, step)["error"].is_string(), "{stopped}");
        let skipped = skip(&f.h, "builder", step, StatusCode::OK).await;
        assert_eq!(step_of(&skipped, step)["skipped"], true, "{skipped}");
        assert_eq!(step_of(&skipped, step)["group"], "skipped", "{skipped}");
    }
    f.start(request).await;
    let done = f.terminal().await;
    assert_eq!(done["state"], "complete", "{done}");
    for step in ["tool:unknown-agent", "authentication:unknown-agent"] {
        assert_eq!(step_of(&done, step)["skipped"], true, "{done}");
    }
}

/// A worker bin directory with Git and one stub per `(name, script)`.
fn worker_bin(remote: &std::path::Path, dir: &str, tools: &[(&str, &str)]) -> std::path::PathBuf {
    let bin = remote.join(dir);
    std::fs::create_dir(&bin).unwrap();
    std::os::unix::fs::symlink("/usr/bin/git", bin.join("git")).unwrap();
    for (name, script) in tools {
        let stub = common::shared_script(&format!("#!/bin/sh\n{script}\n"));
        std::os::unix::fs::symlink(stub, bin.join(name)).unwrap();
    }
    bin
}

/// Run a setup of `request` to the end of its steps, and return its progress.
async fn setup_to_the_end(
    h: &common::Harness,
    name: &str,
    request: serde_json::Value,
) -> serde_json::Value {
    use serde_json::Value;
    let _: Value = h
        .json(
            common::post_json(&format!("/v1/hosts/{name}/setup"), request),
            axum::http::StatusCode::ACCEPTED,
        )
        .await;
    common::eventually(common::TIMEOUT, "setup reaches its end", || async {
        let state: Value = h.get(&format!("/v1/hosts/{name}/setup")).await;
        matches!(
            state["state"].as_str(),
            Some("complete" | "failed" | "incomplete")
        )
    })
    .await;
    h.get(&format!("/v1/hosts/{name}/setup")).await
}

fn step<'v>(state: &'v serde_json::Value, name: &str) -> &'v serde_json::Value {
    state["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == name)
        .unwrap_or_else(|| panic!("no step {name} in {state}"))
}

#[tokio::test]
async fn setup_reads_the_interactive_shell_path_first_and_falls_back_in_order() {
    use serde_json::json;
    futures_util::future::join_all(
        [
            // Criterion 1: the interactive shell wins when it gives a marked PATH.
            (json!({}), "/interactive/tools"),
            // Each failure of the interactive shell falls back to the login shell.
            (json!({"interactive":"fail"}), "/login/tools"),
            (json!({"interactive":"unmarked"}), "/login/tools"),
            (json!({"interactive":"empty"}), "/login/tools"),
            // Both shells failing falls back to the SSH session.
            (json!({"shell_fail":true}), "/fallback/tools"),
        ]
        .map(|(mut setup, expected)| async move {
            let remote = tempfile::tempdir().unwrap();
            let local = tempfile::tempdir().unwrap();
            let mut binary = vec![0; 64];
            binary[..6].copy_from_slice(b"\x7fELF\x02\x01");
            binary[16] = 2;
            binary[18] = 62;
            let cli = local.path().join("ariadne");
            let daemon = local.path().join("ariadned");
            std::fs::write(&cli, &binary).unwrap();
            std::fs::write(&daemon, &binary).unwrap();
            setup["home"] = json!(remote.path());
            setup["path"] = json!("/login/tools:/usr/bin:/bin");
            setup["interactive_path"] = json!("/interactive/tools:/usr/bin:/bin");
            setup["inherited_path"] = json!("/fallback/tools:/usr/bin:/bin");
            let ssh = common::ssh::stub_ssh(json!({"setup":setup, "exit":0}));
            let h = harness().ssh(&ssh).await;
            let state = setup_to_the_end(
                &h,
                "order",
                json!({"ssh":"ssh://box", "source":{"kind":"local", "cli":cli, "daemon":daemon}}),
            )
            .await;
            assert_eq!(state["state"], "complete", "{setup}: {state}");
            let unit = std::fs::read_to_string(
                remote.path().join(".config/systemd/user/ariadned.service"),
            )
            .unwrap();
            let prefix = format!("Environment=\"PATH={}/.local/bin:", remote.path().display());
            assert!(unit.contains(&prefix), "{setup}: {unit}");
            assert!(
                unit.contains(&format!(":{expected}:/usr/bin:/bin\"")),
                "{setup}: {unit}"
            );
            h.state.hosts.shutdown().await;
        }),
    )
    .await;
}

#[tokio::test]
async fn setup_rejects_a_control_character_in_the_interactive_shell_path() {
    use serde_json::json;
    let remote = tempfile::tempdir().unwrap();
    let setup = json!({"home":remote.path(), "interactive_path":"/tools\n/bin"});
    let ssh = common::ssh::stub_ssh(json!({"setup":setup, "exit":0}));
    let h = harness().ssh(&ssh).await;
    let state = setup_to_the_end(&h, "control", json!({"ssh":"ssh://box", "source":{"kind":"local", "cli":"/missing/ariadne", "daemon":"/missing/ariadned"}})).await;
    assert_eq!(state["state"], "failed", "{state}");
    assert!(
        state["steps"][0]["error"]
            .as_str()
            .unwrap()
            .contains("PATH")
    );
    assert!(!remote.path().join(".local/bin/ariadne").exists());
}

#[tokio::test]
async fn inspection_finds_runtimes_agents_and_provider_clis_on_the_interactive_shell_path() {
    use serde_json::{Value, json};
    let remote = tempfile::tempdir().unwrap();
    // The worker layout of a user with nvm: the tools are only on the interactive PATH.
    let bin = worker_bin(
        remote.path(),
        "nvm-bin",
        &[
            ("node", "exit 0"),
            ("npm", "exit 0"),
            // A command that does not run is not a runtime.
            ("uv", "exit 1"),
            ("claude-agent-acp", "exit 0"),
            ("codex-acp", "exit 0"),
            ("claude", "exit 0"),
            ("codex", "exit 0"),
        ],
    );
    let ssh = common::ssh::stub_ssh(
        json!({"setup":{"home":remote.path(), "path":"/usr/bin:/bin", "interactive_path":bin},"exit":0}),
    );
    let h = harness().ssh(&ssh).await;
    let result: Value = h
        .json(
            common::post_json(
                "/v1/hosts/builder/setup/inspect",
                json!({"ssh":"ssh://worker@box"}),
            ),
            axum::http::StatusCode::OK,
        )
        .await;
    let satisfied = |id: &str| group_of(&result["checks"], id)["satisfied"].clone();
    assert_eq!(satisfied("node"), true);
    assert_eq!(satisfied("npm"), true);
    assert_eq!(satisfied("uv"), false, "{result}");
    for id in ["codex-acp", "claude-acp"] {
        assert_eq!(
            group_of(&result["agents"], id)["group"],
            "already_present",
            "{id}"
        );
    }
    assert_eq!(group_of(&result["agents"], "fast-agent")["group"], "manual");
}

#[tokio::test]
async fn setup_reuses_agents_and_provider_clis_that_the_user_installed_by_hand() {
    use serde_json::{Value, json};
    let remote = tempfile::tempdir().unwrap();
    // No node, npm or Homebrew: only the commands themselves.
    let bin = worker_bin(
        remote.path(),
        "hand-bin",
        &[
            ("claude-agent-acp", "exit 0"),
            ("codex-acp", "exit 0"),
            ("claude", "exit 0"),
            ("codex", "exit 0"),
        ],
    );
    let ssh = common::ssh::stub_ssh(
        json!({"setup":{"home":remote.path(), "path":"/usr/bin:/bin", "interactive_path":bin},"exit":0}),
    );
    let h = harness().ssh(&ssh).await;
    let request = json!({
        "ssh":"ssh://worker@box", "source":{"kind":"local","cli":"/missing","daemon":"/missing"},
        "tools":{"agents":["codex-acp", "claude-acp"],"github":false}
    });
    let review: Value = h
        .json(
            common::post_json("/v1/hosts/hand/setup/review", request.clone()),
            axum::http::StatusCode::OK,
        )
        .await;
    for id in ["tool:codex-acp", "tool:claude-acp"] {
        assert_eq!(group_of(&review["plan"], id)["group"], "already_present");
    }
    let state = setup_to_the_end(&h, "hand", request).await;
    for name in ["tool:codex-acp", "tool:claude-acp"] {
        let item = step(&state, name);
        assert_eq!(item["complete"], true, "{state}");
        assert_eq!(item["group"], "already_present", "{state}");
    }
    assert!(!remote.path().join(".local/lib").exists());
}

#[tokio::test]
async fn a_failed_agent_step_names_what_is_missing() {
    use serde_json::json;
    let npm_fails = "[ \"$1\" != --version ] || exit 0; exit 1";
    let node_works = [("node", "exit 0"), ("npm", npm_fails)];
    type Case<'a> = (&'a str, &'a [(&'a str, &'a str)], &'a str);
    let cases: [Case; 6] = [
        ("codex-acp", &[], "Node.js and npm are missing"),
        ("fast-agent", &[], "uv"),
        ("codex-acp", &node_works, "ACP package"),
        (
            "codex-acp",
            &[
                ("node", "exit 0"),
                ("npm", npm_fails),
                ("codex-acp", "exit 0"),
            ],
            "provider CLI",
        ),
        // A command that exists but does not run is missing.
        (
            "codex-acp",
            &[
                ("node", "exit 0"),
                ("npm", npm_fails),
                ("codex-acp", "exit 1"),
                ("codex", "exit 0"),
            ],
            "ACP package",
        ),
        (
            "codex-acp",
            &[
                ("node", "exit 0"),
                ("npm", npm_fails),
                ("codex-acp", "exit 0"),
                ("codex", "exit 1"),
            ],
            "provider CLI",
        ),
    ];
    // One run per case, side by side, so the test takes as long as its slowest case.
    futures_util::future::join_all(cases.map(|(agent, tools, expected)| async move {
        let remote = tempfile::tempdir().unwrap();
        let bin = worker_bin(remote.path(), "bin", tools);
        let ssh =
            common::ssh::stub_ssh(json!({"setup":{"home":remote.path(), "path":bin},"exit":0}));
        let h = harness().ssh(&ssh).await;
        let state = setup_to_the_end(
            &h,
            "missing",
            json!({
                "ssh":"ssh://worker@box", "source":{"kind":"local","cli":"/missing","daemon":"/missing"},
                "tools":{"agents":[agent],"github":false}
            }),
        )
        .await;
        let error = step(&state, &format!("tool:{agent}"))["error"]
            .as_str()
            .unwrap_or_else(|| panic!("{agent}: no error in {state}"))
            .to_owned();
        assert!(error.contains(expected), "{agent}: {error}");
        h.state.hosts.shutdown().await;
    }))
    .await;
}

#[tokio::test]
async fn inspection_does_not_count_an_agent_or_provider_command_that_does_not_run() {
    use serde_json::json;
    let remote = tempfile::tempdir().unwrap();
    let bin = worker_bin(
        remote.path(),
        "broken-bin",
        &[
            ("node", "exit 0"),
            ("npm", "exit 0"),
            ("claude-agent-acp", "exit 1"),
            ("codex-acp", "exit 0"),
            ("claude", "exit 0"),
            ("codex", "exit 1"),
        ],
    );
    let ssh = common::ssh::stub_ssh(json!({"setup":{"home":remote.path(), "path":bin},"exit":0}));
    let h = harness().ssh(&ssh).await;
    let result: serde_json::Value = h
        .json(
            common::post_json(
                "/v1/hosts/builder/setup/inspect",
                json!({"ssh":"ssh://worker@box"}),
            ),
            axum::http::StatusCode::OK,
        )
        .await;
    // The agent command does not run for Claude, and the provider command does not run for Codex.
    for id in ["claude-acp", "codex-acp"] {
        assert_eq!(
            group_of(&result["agents"], id)["group"],
            "ariadne_installs",
            "{id}"
        );
    }
}

#[tokio::test]
async fn an_agent_whose_command_differs_from_its_package_name_is_found_by_its_command() {
    use serde_json::json;
    let remote = tempfile::tempdir().unwrap();
    // Kilo's package is `@kilocode/cli` and installs `kilo`. An unrelated
    // `cli` and `code` on the PATH are no proof of Kilo or of MiniMax Code.
    let bin = worker_bin(
        remote.path(),
        "named-bin",
        &[
            ("node", "exit 0"),
            ("npm", "exit 0"),
            ("kilo", "exit 0"),
            ("gemini", "exit 0"),
            ("cli", "exit 0"),
            ("code", "exit 0"),
        ],
    );
    let ssh = common::ssh::stub_ssh(json!({"setup":{"home":remote.path(), "path":bin},"exit":0}));
    let h = harness().ssh(&ssh).await;
    let result: serde_json::Value = h
        .json(
            common::post_json(
                "/v1/hosts/builder/setup/inspect",
                json!({"ssh":"ssh://worker@box"}),
            ),
            axum::http::StatusCode::OK,
        )
        .await;
    assert_eq!(
        group_of(&result["agents"], "kilo")["group"],
        "already_present"
    );
    // `@google/gemini-cli` installs `gemini`, which is not the package name.
    assert_eq!(
        group_of(&result["agents"], "gemini")["group"],
        "already_present"
    );
    assert_eq!(
        group_of(&result["agents"], "minimax-code")["group"],
        "ariadne_installs"
    );
}
