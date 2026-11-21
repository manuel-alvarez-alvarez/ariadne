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
async fn repository_setup_preserves_unrelated_existing_destinations() {
    for has_file in [true, false] {
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
    use common::{TIMEOUT, eventually, post_json};
    use serde_json::{Value, json};
    let remote = tempfile::tempdir().unwrap();
    let gh = common::forge::stub_forge_cli(json!([
        {"args":["--version"], "stdout":"gh version 2.102.0\n"},
        {"args":["release", "download"], "exit":0},
        {"args":["attestation", "verify"], "exit":1, "stderr":"private token must not escape"}
    ]));
    let ssh = common::ssh::stub_ssh(json!({"setup":{"home":remote.path()}, "exit":0}));
    let h = harness().ssh(&ssh).forge_cli(&gh).await;
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
    let request = json!({"ssh":"ssh://box", "source":{"kind":"release", "repository":"acme/ariadne", "tag":"v1.0.0"}});
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
            "acme/ariadne/.github/workflows/release.yml"
        ]));
    assert!(
        verification
            .args
            .windows(2)
            .any(|pair| pair == ["--source-ref", "refs/tags/v1.0.0"])
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
