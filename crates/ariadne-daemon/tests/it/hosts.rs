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
            status: HostStatus::Connecting,
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
    assert!(doc["paths"]["/v1/hosts/{name}/setup"]["post"].is_object());
    assert!(doc["paths"]["/v1/hosts/{name}/setup"]["get"].is_object());
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
    for os in ["Linux", "Darwin"] {
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
            state["steps"][4]["error"]
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
        );
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
                post_json("/v1/hosts/builder/setup", request),
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
        );
        assert!(restored.list().iter().any(|h| h.name == "builder"));
        assert_eq!(
            std::fs::read_to_string(&unit).unwrap(),
            "# machine-specific service settings\n"
        );
        assert_eq!(std::fs::read_to_string(&plist).unwrap(), old_plist);
    }
}

#[tokio::test]
async fn setup_checks_each_platform_before_it_transfers_a_binary() {
    use common::{TIMEOUT, eventually, post_json};
    use serde_json::{Value, json};
    for (os, arch, header) in [
        (
            "Linux",
            "x86_64",
            vec![
                0x7f, b'E', b'L', b'F', 2, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 62, 0,
            ],
        ),
        (
            "Linux",
            "aarch64",
            vec![
                0x7f, b'E', b'L', b'F', 2, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 183, 0,
            ],
        ),
        (
            "Darwin",
            "x86_64",
            vec![0xcf, 0xfa, 0xed, 0xfe, 7, 0, 0, 1, 3, 0, 0, 0, 2, 0, 0, 0],
        ),
        (
            "Darwin",
            "arm64",
            vec![0xcf, 0xfa, 0xed, 0xfe, 12, 0, 0, 1, 0, 0, 0, 0, 2, 0, 0, 0],
        ),
    ] {
        let remote = tempfile::tempdir().unwrap();
        let local = tempfile::tempdir().unwrap();
        let cli = local.path().join("ariadne");
        let daemon = local.path().join("ariadned");
        let mut binary = header;
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
        let request =
            json!({"ssh":"ssh://box", "source":{"kind":"local", "cli":cli, "daemon":daemon}});
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
        assert_eq!(
            ssh.runs().len(),
            1,
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
    assert_eq!(ssh.runs().len(), 1);
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
    for path in [
        "/tools\n",
        "/tools\n/other",
        "/tools\0/bin",
        "/tools\t/bin",
        "/tools\x7f/bin",
        "/tools\u{85}/bin",
        "/tools\r/bin",
    ] {
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
    }
}

#[tokio::test]
async fn setup_uses_the_ssh_path_when_the_login_shell_fails_or_returns_no_path() {
    use common::{TIMEOUT, eventually, post_json};
    use serde_json::{Value, json};
    for mut setup in [
        json!({"shell_fail":true}),
        json!({"missing_markers":true, "path":"/unmarked/tools:/usr/bin:/bin", "banner":"Welcome!\n"}),
        json!({"path":"", "banner":"Welcome!"}),
    ] {
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
        assert!(
            unit.contains(&format!(
                "Environment=\"PATH={}/.local/bin:",
                remote.path().display()
            )),
            "{unit}"
        );
        assert!(unit.contains(":/fallback/tools:/usr/bin:/bin\""), "{unit}");
        h.state.hosts.shutdown().await;
    }
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
