//! Host selection through the executable and a real registry route.
use std::sync::{Arc, Mutex};

use axum::extract::{Path, Request};
use axum::http::StatusCode;
use axum::{
    Json, Router,
    routing::{get, post},
};
use serde_json::json;

#[tokio::test]
async fn named_hosts_and_guards_use_the_local_registry() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let remote_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let remote_endpoint = format!("http://{}", remote_listener.local_addr().unwrap());
    let remote_app = Router::new().route(
        "/v1/dashboards/special",
        get(|| async {
            Json(json!({
                "name":"special", "document":{"title":"remote-only"}, "builtin":false,
                "document_is_default":false, "created_at":"2026-01-01", "updated_at":"2026-01-01"
            }))
        }),
    );
    let remote =
        tokio::spawn(async move { axum::serve(remote_listener, remote_app).await.unwrap() });
    let registry = json!([
        {"name":"local","kind":"local","status":"ready","endpoint":endpoint,"error":null,"remote":null,"sync":{"state":"in_sync","error":null,"missing_repositories":[],"skipped_permissions":0}},
        {"name":"build","kind":"direct","status":"ready","endpoint":remote_endpoint,"error":null,"remote":null,"sync":{"state":"in_sync","error":null,"missing_repositories":[],"skipped_permissions":0}},
        {"name":"offline","kind":"ssh","status":"down","endpoint":null,"error":"no forward","remote":null,"sync":{"state":"in_sync","error":null,"missing_repositories":[],"skipped_permissions":0}}
    ]);
    let app = Router::new().route("/v1/hosts", get(move || async move { Json(registry) }));
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    for (host, args, code, expected) in [
        ("local", vec!["host", "ls"], 0, "offline"),
        (
            "build",
            vec!["dashboard", "get", "special"],
            0,
            "remote-only",
        ),
        ("all", vec!["dashboard", "get", "special"], 0, "remote-only"),
        ("all", vec!["dashboard", "get", "absent"], 4, "no host"),
        ("build", vec!["skill", "reset", "coding"], 2, "synced"),
        ("build", vec!["daemon", "stop"], 2, "--host local"),
        ("all", vec!["daemon", "stop"], 2, "--host local"),
    ] {
        let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_ariadne"))
            .args(["--endpoint", &endpoint, "--host", host])
            .args(args)
            .env("ARIADNE_TELEMETRY", "off")
            .output()
            .await
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(code),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(text.contains(expected), "{text}");
    }
    server.abort();
    remote.abort();
}

#[tokio::test]
async fn all_hosts_keep_uppercase_rows_and_attention_items() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let registry = json!([
        {"name":"local","kind":"local","status":"ready","endpoint":endpoint,"error":null,"remote":null,"sync":{"state":"in_sync","error":null,"missing_repositories":[],"skipped_permissions":0}}
    ]);
    let skill = json!([{
        "name":"test","seat":"task","summary":"BUILD","document":"","document_is_default":false,
        "builtin":false,"created_at":"2026-11-17T00:00:00Z","updated_at":"2026-11-17T00:00:00Z"
    }]);
    let attention = json!({"complete":true,"items":[{
        "id":"blocked","producer":"recovery","reason":"access","summary":"Sign in",
        "required_action":"Sign in","since":"2026-11-17T00:00:00Z","affected":[],
        "target":{"kind":"settings","section":"agents"}
    }]});
    let app = Router::new()
        .route("/v1/hosts", get(move || async move { Json(registry) }))
        .route("/v1/skills", get(move || async move { Json(skill) }))
        .route("/v1/attention", get(move || async move { Json(attention) }));
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    for (args, expected) in [
        (vec!["skill", "ls", "--columns", "summary"], "local   BUILD"),
        (vec!["attention", "--format", "json"], "\"id\": \"blocked\""),
    ] {
        let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_ariadne"))
            .args(["--endpoint", &endpoint, "--host", "all"])
            .args(args)
            .env("ARIADNE_TELEMETRY", "off")
            .output()
            .await
            .unwrap();
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(text.contains(expected), "{text}");
    }
    let quiet = tokio::process::Command::new(env!("CARGO_BIN_EXE_ariadne"))
        .args([
            "--endpoint",
            &endpoint,
            "--host",
            "all",
            "--quiet",
            "skill",
            "ls",
        ])
        .env("ARIADNE_TELEMETRY", "off")
        .output()
        .await
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&quiet.stdout), "test\n");
    server.abort();
}

#[tokio::test]
async fn query_host_all_reads_the_merged_answer_and_prints_a_host_column() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let merged = json!({
        "status": "success",
        "data": {"resultType": "vector", "result": [
            {"metric": {"host": "local", "seat": "agent"}, "value": [1.0, "30"]},
            {"metric": {"host": "build", "seat": "agent"}, "value": [1.0, "7"]}
        ]},
        "warnings": ["offline: host offline is down"]
    });
    let app = Router::new().route(
        "/v1/hosts/all/api/v1/query",
        get(move || async move { Json(merged) }),
    );
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_ariadne"))
        .args(["--endpoint", &endpoint, "--host", "all", "query", "up"])
        .env("ARIADNE_TELEMETRY", "off")
        .output()
        .await
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(0), "{stderr}");
    let header = stdout.lines().next().unwrap();
    assert_eq!(
        header.split_whitespace().collect::<Vec<_>>(),
        ["HOST", "SEAT", "VALUE"]
    );
    let first: Vec<&str> = stdout.lines().nth(1).unwrap().split_whitespace().collect();
    assert_eq!(first[0], "local", "{stdout}");
    assert!(stdout.contains("build"), "{stdout}");
    assert!(stderr.contains("offline: host offline is down"), "{stderr}");
    server.abort();
}

/// The AI permission status a stub daemon answers with, on `device`.
fn ai_status(device: &str) -> serde_json::Value {
    json!({
        "enabled": true, "allow_threshold": 0.2, "deny_threshold": 0.8,
        "thresholds_default": false, "flavour": "4b", "device": device,
        "hardware": {"os": "linux", "arch": "x86_64", "memory_bytes": 1u64 << 34, "gpu": null},
        "flavours": [], "python": {"path": null, "version": null, "ok": false},
        "state": "ready", "installed_release": null, "latest_release": null,
        "weights_present": true, "endpoint": null, "last_refresh_at": null, "last_error": null
    })
}

#[tokio::test]
async fn permissions_device_is_written_on_the_named_host_only() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let remote_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let remote_endpoint = format!("http://{}", remote_listener.local_addr().unwrap());
    let local_puts = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let remote_puts = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let local_endpoint = endpoint.clone();
    let local_sink = Arc::clone(&local_puts);
    let remote_sink = Arc::clone(&remote_puts);
    let local_app = Router::new()
        .route(
            "/v1/hosts",
            get(move || async move {
                Json(json!([
                    {"name":"local","kind":"local","status":"ready","endpoint":local_endpoint,"error":null,"remote":null,"sync":{"state":"in_sync","error":null,"missing_repositories":[],"skipped_permissions":0}},
                    {"name":"build","kind":"direct","status":"ready","endpoint":remote_endpoint,"error":null,"remote":null,"sync":{"state":"in_sync","error":null,"missing_repositories":[],"skipped_permissions":0}},
                    {"name":"bare","kind":"direct","status":"ready","endpoint":null,"error":null,"remote":null,"sync":{"state":"in_sync","error":null,"missing_repositories":[],"skipped_permissions":0}},
                    {"name":"offline","kind":"ssh","status":"down","endpoint":null,"error":"no forward","remote":null,"sync":{"state":"in_sync","error":null,"missing_repositories":[],"skipped_permissions":0}}
                ]))
            }),
        )
        .route(
            "/v1/permissions/ai",
            get(|| async { Json(ai_status("mlx")) }).put(
                move |Json(body): Json<serde_json::Value>| {
                    let sink = Arc::clone(&local_sink);
                    async move {
                        sink.lock().unwrap().push(body);
                        Json(ai_status("mlx"))
                    }
                },
            ),
        );
    let remote_app = Router::new().route(
        "/v1/permissions/ai",
        get(|| async { Json(ai_status("cpu")) }).put(move |Json(body): Json<serde_json::Value>| {
            let sink = Arc::clone(&remote_sink);
            async move {
                sink.lock().unwrap().push(body);
                Json(ai_status("cpu"))
            }
        }),
    );
    let server = tokio::spawn(async move { axum::serve(listener, local_app).await.unwrap() });
    let remote =
        tokio::spawn(async move { axum::serve(remote_listener, remote_app).await.unwrap() });
    let run = |host: &'static str, args: &[&str]| {
        let endpoint = endpoint.clone();
        let args: Vec<String> = args.iter().map(|arg| arg.to_string()).collect();
        async move {
            tokio::process::Command::new(env!("CARGO_BIN_EXE_ariadne"))
                .args(["--endpoint", &endpoint, "--host", host])
                .args(args)
                .env("ARIADNE_TELEMETRY", "off")
                .output()
                .await
                .unwrap()
        }
    };

    // The device alone reaches the named host, and no other host.
    let output = run("build", &["permissions", "ai", "set", "--device", "cpu"]).await;
    assert_eq!(output.status.code(), Some(0), "{:?}", output);
    assert_eq!(*remote_puts.lock().unwrap(), vec![json!({"device": "cpu"})]);
    assert!(local_puts.lock().unwrap().is_empty());

    // Any other setting on a named host is refused before anything is sent.
    for args in [
        vec![
            "permissions",
            "ai",
            "set",
            "--flavour",
            "9b",
            "--device",
            "cpu",
        ],
        vec![
            "permissions",
            "ai",
            "set",
            "--allow-threshold",
            "0.2",
            "--device",
            "cpu",
        ],
        vec!["permissions", "ai", "set", "--default-thresholds"],
    ] {
        let output = run("build", &args).await;
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(2), "{stderr}");
        assert!(stderr.contains("written on local and synced"), "{stderr}");
        assert!(
            stderr.contains("only `permissions ai set --device` is per host"),
            "{stderr}"
        );
    }
    assert_eq!(remote_puts.lock().unwrap().len(), 1);

    // `show` on every host prints each host's device in registry order, and warns on the one that is down.
    let output = run("all", &["permissions", "ai", "show", "--format", "json"]).await;
    assert_eq!(output.status.code(), Some(0));
    let shown: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let rows: Vec<(String, String)> = shown
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            (
                row["host"].as_str().unwrap().to_string(),
                row["device"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            ("local".into(), "mlx".into()),
            ("build".into(), "cpu".into())
        ]
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("warning: offline:"), "{stderr}");
    assert!(
        stderr.contains("warning: bare: host bare has no endpoint"),
        "{stderr}"
    );
    // An `--endpoint` typed after the subcommand does not reach the children twice.
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_ariadne"))
        .args([
            "--host",
            "all",
            "permissions",
            "ai",
            "show",
            "--format",
            "json",
        ])
        .args(["--endpoint", &endpoint])
        .env("ARIADNE_TELEMETRY", "off")
        .output()
        .await
        .unwrap();
    assert_eq!(output.status.code(), Some(0), "{:?}", output);
    server.abort();
    remote.abort();
}

#[tokio::test]
async fn host_ls_shows_remote_and_sync_state_of_each_host() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let registry = json!([
        {"name":"local","kind":"local","status":"ready","endpoint":endpoint,"error":null,"remote":null,"sync":{"state":"in_sync","error":null,"missing_repositories":[],"skipped_permissions":0}},
        {"name":"far","kind":"ssh","status":"ready","endpoint":null,"error":null,"remote":"user@far:7000","sync":{"state":"failed","error":"push refused: disk full","missing_repositories":[{"id":"a","forge":"o/a"},{"id":"b","forge":null}],"skipped_permissions":3}}
    ]);
    let app = Router::new().route("/v1/hosts", get(move || async move { Json(registry) }));
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let run = |args: Vec<&'static str>| {
        let endpoint = endpoint.clone();
        async move {
            let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_ariadne"))
                .args(["--endpoint", &endpoint])
                .args(args)
                .env("ARIADNE_TELEMETRY", "off")
                .output()
                .await
                .unwrap();
            String::from_utf8_lossy(&output.stdout).into_owned()
        }
    };
    let table = run(vec!["host", "ls", "--no-trunc"]).await;
    for expected in [
        "SYNC",
        "MISSING",
        "SKIPPED",
        "REMOTE",
        "failed",
        "in_sync",
        "user@far:7000",
        "push refused: disk full",
    ] {
        assert!(table.contains(expected), "{table}");
    }
    let far = table.lines().find(|l| l.starts_with("far")).unwrap();
    assert!(far.split_whitespace().any(|w| w == "2"), "{far}");
    assert!(far.split_whitespace().any(|w| w == "3"), "{far}");
    let picked = run(vec!["host", "ls", "--columns", "name,sync,missing,skipped"]).await;
    let lines: Vec<_> = picked
        .lines()
        .map(|l| l.split_whitespace().collect::<Vec<_>>())
        .collect();
    assert_eq!(lines[0], ["NAME", "SYNC", "MISSING", "SKIPPED"], "{picked}");
    assert_eq!(lines[1], ["local", "in_sync", "0", "0"], "{picked}");
    assert_eq!(lines[2], ["far", "failed", "2", "3"], "{picked}");
    let json = run(vec!["host", "ls", "--format", "json"]).await;
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed[1]["sync"]["skipped_permissions"], 3);
    assert_eq!(parsed[1]["remote"], "user@far:7000");
    server.abort();
}

/// Every request a stub daemon gets, as `METHOD /path`, answered with a 404.
fn recorder(seen: &Arc<Mutex<Vec<String>>>) -> Router {
    let seen = Arc::clone(seen);
    Router::new().fallback(move |request: Request| {
        let seen = Arc::clone(&seen);
        async move {
            seen.lock()
                .unwrap()
                .push(format!("{} {}", request.method(), request.uri().path()));
            StatusCode::NOT_FOUND
        }
    })
}

/// Each command takes the hosts its declared kind allows (spec 014, rules 44 to 48).
#[tokio::test]
async fn each_command_takes_the_hosts_of_its_kind() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let remote_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let remote_endpoint = format!("http://{}", remote_listener.local_addr().unwrap());
    let sync =
        json!({"state":"in_sync","error":null,"missing_repositories":[],"skipped_permissions":0});
    let registry = json!([
        {"name":"local","kind":"local","status":"ready","endpoint":endpoint,"error":null,"remote":null,"sync":sync},
        {"name":"build","kind":"direct","status":"ready","endpoint":remote_endpoint,"error":null,"remote":null,"sync":sync},
        {"name":"offline","kind":"ssh","status":"down","endpoint":null,"error":"no forward","remote":null,"sync":sync}
    ]);
    let local_seen = Arc::new(Mutex::new(Vec::new()));
    let remote_seen = Arc::new(Mutex::new(Vec::new()));
    let local_app =
        recorder(&local_seen).route("/v1/hosts", get(move || async move { Json(registry) }));
    let remote_app = recorder(&remote_seen);
    let server = tokio::spawn(async move { axum::serve(listener, local_app).await.unwrap() });
    let remote =
        tokio::spawn(async move { axum::serve(remote_listener, remote_app).await.unwrap() });
    let draft = tempfile::NamedTempFile::new().unwrap();
    let draft = draft.path().to_str().unwrap().to_owned();
    let run = |host: &str, args: &[&str]| {
        let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_ariadne"));
        command
            .args(["--endpoint", &endpoint, "--host", host])
            .args(args)
            .env("ARIADNE_TELEMETRY", "off");
        async move {
            let output = command.output().await.unwrap();
            let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
            (output.status.code(), stderr)
        }
    };

    // Reads that are not lists are not refused as writes, on one host or on all.
    for host in ["build", "all"] {
        for args in [
            vec!["events"],
            vec!["doctor"],
            vec!["forge", "tunnel"],
            vec!["workflow", "check", "--file", &draft],
            vec![
                "permissions",
                "ai",
                "test",
                "--tool",
                "Read",
                "--input",
                "{}",
            ],
            vec!["completions", "bash"],
        ] {
            let (_, stderr) = run(host, &args).await;
            assert!(!stderr.contains("write"), "{host} {args:?}: {stderr}");
            assert!(!stderr.contains("synced"), "{host} {args:?}: {stderr}");
        }
    }

    // A repository's own path and branch are written on the named host.
    local_seen.lock().unwrap().clear();
    for (args, request) in [
        (
            vec!["repo", "add", "/src/api", "--branch", "main"],
            "POST /v1/repositories",
        ),
        (vec!["repo", "rm", "r1", "--yes"], "GET /v1/repositories"),
        (
            vec!["repo", "update", "r1", "--path", "/src/api"],
            "GET /v1/repositories",
        ),
    ] {
        remote_seen.lock().unwrap().clear();
        let (code, stderr) = run("build", &args).await;
        assert_ne!(code, Some(2), "{args:?}: {stderr}");
        let seen = remote_seen.lock().unwrap().clone();
        assert!(
            seen.first().is_some_and(|first| first.starts_with(request)),
            "{args:?}: {seen:?}"
        );
    }
    assert!(
        local_seen.lock().unwrap().is_empty(),
        "{:?}",
        local_seen.lock().unwrap()
    );

    remote_seen.lock().unwrap().clear();
    for (host, args, expected) in [
        // A synced setting of a repository stays on local.
        (
            "build",
            vec!["repo", "add", "/src/api", "--description", "api"],
            "`--description` is written on local and synced",
        ),
        (
            "build",
            vec!["repo", "update", "r1", "--branch", "next"],
            "`--branch` is written on local and synced",
        ),
        // `host ls` reads the registry of the local daemon alone.
        ("build", vec!["host", "ls"], "registry of the local daemon"),
        ("all", vec!["host", "ls"], "registry of the local daemon"),
        // An unknown host names the configured hosts; one not ready says why.
        (
            "missing",
            vec!["task", "ls"],
            "no host is named missing; the hosts are local (ready), build (ready), offline (down)",
        ),
        (
            "missing",
            vec!["skill", "reset", "coding"],
            "no host is named missing; the hosts are local (ready), build (ready), offline (down)",
        ),
        (
            "build",
            vec!["skill", "reset", "coding"],
            "run it with --host local",
        ),
        (
            "offline",
            vec!["task", "ls"],
            "host offline is down: no forward",
        ),
        (
            "offline",
            vec!["skill", "reset", "coding"],
            "host offline is down: no forward",
        ),
        // A write on every host names only the hosts that take it.
        (
            "all",
            vec!["task", "cancel", "t1"],
            "select one host for this write: local, build\n",
        ),
        (
            "all",
            vec!["repo", "add", "/src/api"],
            "select one host for this write: local, build\n",
        ),
        (
            "all",
            vec!["repo", "add", "/src/api", "--workflow", "w"],
            "select one host for this write: local\n",
        ),
        (
            "all",
            vec!["skill", "reset", "coding"],
            "run it with --host local",
        ),
    ] {
        let (code, stderr) = run(host, &args).await;
        assert_eq!(code, Some(2), "{host} {args:?}: {stderr}");
        assert!(stderr.contains(expected), "{host} {args:?}: {stderr}");
    }
    assert!(
        remote_seen.lock().unwrap().is_empty(),
        "{:?}",
        remote_seen.lock().unwrap()
    );

    // A warning names a status in lowercase.
    let (_, stderr) = run("all", &["task", "ls"]).await;
    assert!(stderr.contains("warning: offline: down"), "{stderr}");
    server.abort();
    remote.abort();
}

#[tokio::test]
async fn all_refuses_local_commands_without_a_daemon() {
    let home = tempfile::tempdir().unwrap();
    let endpoint = home.path().join("absent.sock");
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_ariadne"))
        .args([
            "--endpoint",
            endpoint.to_str().unwrap(),
            "--host",
            "all",
            "host",
            "ls",
        ])
        .env("ARIADNE_TELEMETRY", "off")
        .output()
        .await
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(
        stderr.contains("registry of the local daemon; run it with --host local"),
        "{stderr}"
    );
}

#[tokio::test]
async fn named_host_refuses_local_commands_when_registry_is_unreachable() {
    let home = tempfile::tempdir().unwrap();
    let endpoint = home.path().join("absent.sock");
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_ariadne"))
        .args([
            "--endpoint",
            endpoint.to_str().unwrap(),
            "--host",
            "build",
            "host",
            "ls",
        ])
        .env("ARIADNE_TELEMETRY", "off")
        .output()
        .await
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(
        stderr.contains("registry of the local daemon; run it with --host local"),
        "{stderr}"
    );
}

/// What fish-style completion offers (one candidate per line) for `words`.
async fn complete(words: &[&str]) -> Vec<String> {
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_ariadne"))
        .env("COMPLETE", "fish")
        .env("ARIADNE_TELEMETRY", "off")
        .env(
            "ARIADNE_HOME",
            std::env::temp_dir().join("ariadne-complete-nohome"),
        )
        .arg("--")
        .args(words)
        .output()
        .await
        .unwrap();
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| line.split('\t').next().unwrap().to_owned())
        // Flags are offered beside IDs on an empty word; only names matter here.
        .filter(|name| !name.starts_with('-'))
        .collect()
}

fn host(name: &str, status: &str, endpoint: Option<&str>) -> serde_json::Value {
    json!({"name":name,"kind":"direct","status":status,"endpoint":endpoint,"error":null,"remote":null,
        "sync":{"state":"in_sync","error":null,"missing_repositories":[],"skipped_permissions":0}})
}

#[tokio::test]
async fn completion_reads_the_typed_endpoint_and_the_typed_host() {
    async fn serve(registry: serde_json::Value, tasks: serde_json::Value) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let app = Router::new()
            .route("/v1/hosts", get(move || async move { Json(registry) }))
            .route("/v1/tasks", get(move || async move { Json(tasks) }));
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        endpoint
    }
    let task = |id: &str| json!({"id":id,"status":"open","title":"t"});
    let build = serve(json!([]), json!([task("01BUILD")])).await;
    let down = serve(json!([]), json!([task("01DOWN")])).await;
    let main = serve(json!([]), json!([task("01LOCAL")])).await;
    let registry = json!([
        host("local", "ready", Some(&main)),
        host("build", "ready", Some(&build)),
        host("offline", "down", Some(&down)),
    ]);
    let typed = serve(registry, json!([task("01LOCAL")])).await;
    let only_local = serve(json!([host("local", "ready", None)]), json!([])).await;

    // 1: the registry is the one of the daemon `--endpoint` names.
    let names = complete(&["ariadne", "--endpoint", &typed, "--host", ""]).await;
    assert!(names.contains(&"build".to_owned()), "{names:?}");
    assert!(names.contains(&"all".to_owned()), "{names:?}");
    // 2: a daemon that does not answer still offers `local` and `all`.
    let names = complete(&["ariadne", "--endpoint", "http://127.0.0.1:1", "--host", ""]).await;
    assert_eq!(sorted(names), ["all", "local"]);
    // 5: a registry with only `local` offers `local` and `all`.
    let names = complete(&["ariadne", "--endpoint", &only_local, "--host", ""]).await;
    assert_eq!(sorted(names), ["all", "local"]);
    // 3: an ID is read from the host `--host NAME` names.
    let ids = complete(&[
        "ariadne",
        "--endpoint",
        &typed,
        "--host",
        "build",
        "task",
        "inspect",
        "",
    ])
    .await;
    assert_eq!(ids, ["01BUILD"]);
    // 4: `--host all` reads every ready host and no other.
    let ids = complete(&[
        "ariadne",
        "--endpoint",
        &typed,
        "--host=all",
        "task",
        "inspect",
        "",
    ])
    .await;
    assert_eq!(sorted(ids), ["01BUILD", "01LOCAL"]);
}

#[tokio::test]
async fn completion_of_all_hosts_reads_them_together_within_one_budget() {
    // Both task handlers answer only once both requests have arrived. Read one
    // after another, the first never gets its answer and the budget runs out.
    async fn serve(
        registry: serde_json::Value,
        task: &'static str,
        both: Arc<tokio::sync::Barrier>,
    ) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let app = Router::new()
            .route("/v1/hosts", get(move || async move { Json(registry) }))
            .route(
                "/v1/tasks",
                get(move || async move {
                    both.wait().await;
                    Json(json!([{"id":task,"status":"open","title":"t"}]))
                }),
            );
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        endpoint
    }
    let both = Arc::new(tokio::sync::Barrier::new(2));
    let build = serve(json!([]), "01BUILD", both.clone()).await;
    let main = serve(json!([]), "01UNUSED", both.clone()).await;
    // `local` is read at the endpoint typed on the command line.
    let registry = json!([
        host("local", "ready", Some(&main)),
        host("build", "ready", Some(&build)),
    ]);
    let typed = serve(registry, "01LOCAL", both).await;
    let ids = complete(&[
        "ariadne",
        "--endpoint",
        &typed,
        "--host",
        "all",
        "task",
        "inspect",
        "",
    ])
    .await;
    assert_eq!(sorted(ids), ["01BUILD", "01LOCAL"]);
}

fn sorted(mut names: Vec<String>) -> Vec<String> {
    names.sort();
    names
}

async fn run_cli(args: &[&str]) -> (i32, String) {
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_ariadne"))
        .args(args)
        .env("ARIADNE_TELEMETRY", "off")
        .env_remove("ARIADNE_ENDPOINT")
        .env_remove("ARIADNE_SOCKET")
        .env_remove("ARIADNE_HOME")
        .output()
        .await
        .unwrap();
    (
        output.status.code().unwrap(),
        format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ),
    )
}

#[tokio::test]
async fn daemon_status_and_logs_on_a_named_host_show_only_that_host() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let remote_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let remote_endpoint = format!("http://{}", remote_listener.local_addr().unwrap());
    let remote_app = Router::new()
        .route(
            "/v1/health",
            get(|| async { Json(json!({"status":"ok","uptime_secs":5})) }),
        )
        .route(
            "/v1/version",
            get(|| async { Json(json!({"name":"ariadned","version":"9.9.9"})) }),
        );
    let remote =
        tokio::spawn(async move { axum::serve(remote_listener, remote_app).await.unwrap() });
    let sync =
        json!({"state":"in_sync","error":null,"missing_repositories":[],"skipped_permissions":0});
    let registry = json!([
        {"name":"local","kind":"local","status":"ready","endpoint":endpoint,"error":null,"remote":null,"sync":sync},
        {"name":"build","kind":"direct","status":"ready","endpoint":remote_endpoint,"error":null,"remote":null,"sync":sync}
    ]);
    let app = Router::new().route("/v1/hosts", get(move || async move { Json(registry) }));
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let (code, text) = run_cli(&[
        "--endpoint",
        &endpoint,
        "--host",
        "build",
        "daemon",
        "status",
    ])
    .await;
    assert_eq!(code, 0, "{text}");
    assert!(text.contains("9.9.9"), "{text}");
    assert!(!text.contains("tcp") && !text.contains("service"), "{text}");

    // The remote answers no /v1/logs: the command fails and names the reason.
    let (code, text) =
        run_cli(&["--endpoint", &endpoint, "--host", "build", "daemon", "logs"]).await;
    assert_ne!(code, 0, "{text}");
    assert!(!text.contains("reading the log file"), "{text}");

    server.abort();
    remote.abort();
}

#[tokio::test]
async fn daemon_status_and_logs_on_local_use_the_daemon_of_the_given_home() {
    let home = std::env::temp_dir().join(format!("ariadne-no-daemon-{}", std::process::id()));
    std::fs::create_dir_all(&home).unwrap();
    let home = home.to_str().unwrap();
    let (code, text) = run_cli(&["--host", "local", "daemon", "--home", home, "status"]).await;
    assert_ne!(code, 0, "{text}");
    assert!(text.contains(home), "{text}");
}

fn host_row(name: &str, kind: &str, status: &str, error: Option<&str>) -> serde_json::Value {
    json!({"name":name,"kind":kind,"status":status,"endpoint":null,"error":error,"remote":null,
        "sync":{"state":"in_sync","error":null,"missing_repositories":[],"skipped_permissions":0}})
}

/// A registry whose hosts are `connecting` until reconnected: `build` then
/// turns `ready`, `lab` turns `down`, and `stuck` stays `connecting`.
async fn reconnect_registry(names: &[(&str, &str)]) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let state = Arc::new(Mutex::new(
        names
            .iter()
            .map(|(name, kind)| (name.to_string(), kind.to_string(), "connecting".to_string()))
            .collect::<Vec<_>>(),
    ));
    let posts = Arc::new(Mutex::new(Vec::new()));
    let list_state = Arc::clone(&state);
    let post_state = Arc::clone(&state);
    let post_log = Arc::clone(&posts);
    let app = Router::new()
        .route(
            "/v1/hosts",
            get(move || {
                let state = Arc::clone(&list_state);
                async move {
                    let rows: Vec<_> = state
                        .lock()
                        .unwrap()
                        .iter()
                        .map(|(name, kind, status)| {
                            let status = if kind == "local" { "ready" } else { status };
                            let error = (status == "down").then_some("no route");
                            host_row(name, kind, status, error)
                        })
                        .collect();
                    Json(rows)
                }
            }),
        )
        .route(
            "/v1/hosts/{name}/reconnect",
            post(move |Path(name): Path<String>| {
                let state = Arc::clone(&post_state);
                let log = Arc::clone(&post_log);
                async move {
                    log.lock().unwrap().push(name.clone());
                    let mut rows = state.lock().unwrap();
                    let Some(row) = rows.iter_mut().find(|row| row.0 == name) else {
                        return Err(StatusCode::NOT_FOUND);
                    };
                    row.2 = match name.as_str() {
                        "lab" => "down",
                        "stuck" => "connecting",
                        _ => "ready",
                    }
                    .to_string();
                    let error = (row.2 == "down").then_some("no route");
                    Ok(Json(host_row(&name, &row.1, &row.2.clone(), error)))
                }
            }),
        );
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (endpoint, posts)
}

#[tokio::test]
async fn reconnect_one_host_posts_for_it_and_prints_its_row() {
    let (endpoint, posts) =
        reconnect_registry(&[("local", "local"), ("build", "direct"), ("lab", "ssh")]).await;
    let (code, text) = run_cli(&["--endpoint", &endpoint, "host", "reconnect", "build"]).await;
    assert_eq!(code, 0, "{text}");
    assert!(text.contains("NAME") && text.contains("STATUS"), "{text}");
    assert!(text.contains("build") && text.contains("ready"), "{text}");
    assert!(!text.contains("lab"), "{text}");
    assert_eq!(*posts.lock().unwrap(), ["build"]);
}

#[tokio::test]
async fn reconnect_all_posts_for_each_named_host_and_fails_while_one_is_down() {
    let (endpoint, posts) =
        reconnect_registry(&[("local", "local"), ("build", "direct"), ("lab", "ssh")]).await;
    let (code, text) = run_cli(&["--endpoint", &endpoint, "host", "reconnect", "all"]).await;
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("build") && text.contains("ready"), "{text}");
    assert!(text.contains("lab") && text.contains("no route"), "{text}");
    assert!(!text.contains("local"), "{text}");
    let mut posted = posts.lock().unwrap().clone();
    posted.sort();
    assert_eq!(posted, ["build", "lab"]);
}

#[tokio::test]
async fn reconnect_json_prints_the_hosts_and_a_slow_host_fails_after_the_timeout() {
    let (endpoint, _) =
        reconnect_registry(&[("local", "local"), ("build", "direct"), ("stuck", "ssh")]).await;
    let (code, text) = run_cli(&[
        "--endpoint",
        &endpoint,
        "--format",
        "json",
        "host",
        "reconnect",
        "all",
        "--timeout",
        "1",
    ])
    .await;
    assert_eq!(code, 1, "{text}");
    let hosts: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(hosts[0]["name"], "build");
    assert_eq!(hosts[0]["status"], "ready");
    assert_eq!(hosts[1]["name"], "stuck");
    assert_eq!(hosts[1]["status"], "connecting");
    assert_eq!(hosts.as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn reconnect_refuses_local_unknown_names_and_host_selection() {
    let (endpoint, posts) =
        reconnect_registry(&[("local", "local"), ("build", "direct"), ("lab", "ssh")]).await;
    for (args, expected) in [
        (vec!["host", "reconnect", "local"], "local"),
        (vec!["host", "reconnect", "nope"], "build"),
        (
            vec!["--host", "build", "host", "reconnect", "build"],
            "--host",
        ),
        (vec!["--host", "all", "host", "reconnect", "all"], "--host"),
    ] {
        let mut full = vec!["--endpoint", endpoint.as_str()];
        full.extend(args.iter());
        let (code, text) = run_cli(&full).await;
        assert_eq!(code, 2, "{args:?}: {text}");
        assert!(text.contains(expected), "{args:?}: {text}");
    }
    let (_, text) = run_cli(&["--endpoint", &endpoint, "host", "reconnect", "nope"]).await;
    assert!(text.contains("lab"), "{text}");
    assert!(posts.lock().unwrap().is_empty());
}

#[tokio::test]
async fn reconnect_all_with_only_local_says_no_named_host_is_configured() {
    let (endpoint, posts) = reconnect_registry(&[("local", "local")]).await;
    let (code, text) = run_cli(&["--endpoint", &endpoint, "host", "reconnect", "all"]).await;
    assert_eq!(code, 2, "{text}");
    assert!(text.contains("no named host is configured"), "{text}");
    assert!(posts.lock().unwrap().is_empty());
}

/// A stream on `--host all` reaches stdout while the stream is still open (spec 014, rule 46).
#[tokio::test]
async fn all_forwards_a_stream_before_it_ends() {
    use tokio::io::AsyncBufReadExt;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let sync =
        json!({"state":"in_sync","error":null,"missing_repositories":[],"skipped_permissions":0});
    let registry = json!([
        {"name":"local","kind":"local","status":"ready","endpoint":endpoint,"error":null,"remote":null,"sync":sync},
        {"name":"build","kind":"direct","status":"ready","endpoint":endpoint,"error":null,"remote":null,"sync":sync}
    ]);
    let events = json!([{
        "id":"e1","session_id":"s1","task_id":null,"kind":"session_start",
        "payload":{},"summary":"started","created_at":"2026-01-01T00:00:00Z"
    }]);
    let app = Router::new()
        .route("/v1/hosts", get(move || async move { Json(registry) }))
        .route("/v1/events", get(move || async move { Json(events) }))
        .route(
            "/v1/events/stream",
            get(|| async {
                axum::response::Sse::new(futures_util::stream::pending::<
                    Result<axum::response::sse::Event, std::convert::Infallible>,
                >())
            }),
        );
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_ariadne"))
        .args([
            "--endpoint",
            &endpoint,
            "--host",
            "all",
            "events",
            "-f",
            "--format",
            "json",
        ])
        .env("ARIADNE_TELEMETRY", "off")
        .stdout(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut lines = tokio::io::BufReader::new(child.stdout.take().unwrap()).lines();
    let line = tokio::time::timeout(std::time::Duration::from_secs(30), lines.next_line())
        .await
        .expect("the event reaches stdout while the stream is open")
        .unwrap()
        .expect("a line");
    assert!(
        line.contains("session_start") || line.contains("started"),
        "{line}"
    );
    let first_host = serde_json::from_str::<serde_json::Value>(&line).unwrap()["host"].clone();
    let second = tokio::time::timeout(std::time::Duration::from_secs(30), lines.next_line())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let second_host = serde_json::from_str::<serde_json::Value>(&second).unwrap()["host"].clone();
    assert!(first_host == "local" || first_host == "build");
    assert!(second_host == "local" || second_host == "build");
    assert_ne!(first_host, second_host);
    child.kill().await.unwrap();
    server.abort();
}

/// A stub daemon for the lists of `--host all`: its sessions, goals, skills
/// and attention, and a barrier that `GET /v1/skills` waits at.
fn list_daemon(
    sessions: serde_json::Value,
    goals: serde_json::Value,
    attention: serde_json::Value,
    barrier: Arc<tokio::sync::Barrier>,
) -> Router {
    Router::new()
        .route("/v1/sessions", get(move || async move { Json(sessions) }))
        .route("/v1/goals", get(move || async move { Json(goals) }))
        .route("/v1/attention", get(move || async move { Json(attention) }))
        .route(
            "/v1/skills",
            get(move || {
                let barrier = Arc::clone(&barrier);
                async move {
                    // Each host answers only once the other host is asked too.
                    barrier.wait().await;
                    Json(json!([]))
                }
            }),
        )
}

/// The output of `ariadne --endpoint ENDPOINT --host all ARGS`.
async fn run_all(endpoint: &str, args: &[&str]) -> (i32, String, String) {
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_ariadne"))
        .args(["--endpoint", endpoint, "--host", "all"])
        .args(args)
        .env("ARIADNE_TELEMETRY", "off")
        .output()
        .await
        .unwrap();
    (
        output.status.code().unwrap(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// `--host all` draws one list of every ready host (spec 014, rules 46 and 46a).
#[tokio::test]
async fn all_draws_one_list_of_every_ready_host() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let remote_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let remote_endpoint = format!("http://{}", remote_listener.local_addr().unwrap());
    let registry = json!([
        host("local", "ready", Some(&endpoint)),
        host("build", "ready", Some(&remote_endpoint)),
        host("offline", "down", None),
        host("bare", "ready", None),
    ]);
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let local_app = list_daemon(
        json!({"sessions": [{"id": "s-local", "agent_id": "codex", "title": "Local work"}],
               "next_cursor": null, "total": 4}),
        json!([]),
        json!({"complete": true, "items": [{
            "id": "blocked", "producer": "recovery", "reason": "access", "summary": "Sign in",
            "required_action": "Sign in", "since": "2026-11-17T00:00:00Z", "affected": [],
            "target": {"kind": "settings", "section": "agents"}
        }]}),
        Arc::clone(&barrier),
    )
    .route("/v1/hosts", get(move || async move { Json(registry) }));
    let remote_app = list_daemon(
        json!({"sessions": [
                   {"id": "s-build-with-a-long-id", "agent_id": "claude", "title": "Build work"},
                   {"id": "s-build-2", "agent_id": "claude", "title": "More"}
               ],
               "next_cursor": "c2", "total": 8}),
        json!([{"id": "g-build", "title": "Only on build"}]),
        json!({"complete": false, "items": []}),
        barrier,
    );
    let server = tokio::spawn(async move { axum::serve(listener, local_app).await.unwrap() });
    let remote =
        tokio::spawn(async move { axum::serve(remote_listener, remote_app).await.unwrap() });

    // One table led by HOST, aligned across hosts, with one summary line.
    let (code, stdout, stderr) = run_all(&endpoint, &["session", "ls"]).await;
    assert_eq!(code, 0, "{stderr}");
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 5, "{stdout}");
    assert!(lines[0].starts_with("HOST "), "{stdout}");
    let title = lines[0].find("TITLE").unwrap();
    assert_eq!(lines[1].find("Local work"), Some(title), "{stdout}");
    assert_eq!(lines[2].find("Build work"), Some(title), "{stdout}");
    assert!(lines[1].starts_with("local "), "{stdout}");
    assert!(lines[3].starts_with("build "), "{stdout}");
    assert_eq!(lines[4], "3 of 12 sessions");
    // A host that is not ready, or has no endpoint, gets one warning.
    assert!(stderr.contains("warning: offline: down\n"), "{stderr}");
    assert!(
        stderr.contains("warning: bare: host bare has no endpoint\n"),
        "{stderr}"
    );
    assert!(
        stderr.contains("build: Next: ariadne session ls --cursor c2 --host build"),
        "{stderr}"
    );

    // JSON keeps the page object, with the sums and a cursor for each host.
    let (code, stdout, stderr) = run_all(&endpoint, &["--format", "json", "session", "ls"]).await;
    assert_eq!(code, 0, "{stderr}");
    let page: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(page["total"], 12);
    assert_eq!(page["next_cursor"], json!({"local": null, "build": "c2"}));
    assert_eq!(page["sessions"][0]["host"], "local");
    assert_eq!(page["sessions"][2]["host"], "build");

    // `-q` prints only IDs.
    let (_, stdout, _) = run_all(&endpoint, &["-q", "session", "ls"]).await;
    assert_eq!(stdout, "s-local\ns-build-with-a-long-id\ns-build-2\n");

    // A host that lacks the ID of a filter adds no rows and no warning.
    let (code, stdout, stderr) = run_all(&endpoint, &["session", "ls", "--goal", "g-build"]).await;
    assert_eq!(code, 0, "{stderr}");
    assert!(!stdout.contains("local "), "{stdout}");
    assert!(!stderr.contains("local"), "{stderr}");
    let (code, _, stderr) = run_all(&endpoint, &["session", "ls", "--goal", "g-none"]).await;
    assert_eq!(code, 4, "{stderr}");
    // A whole ID is checked on each host too, so an absent one is not found.
    for args in [
        ["task", "ls", "--goal", "01m15jmta93b130wka2qdn2p1x"],
        ["session", "ls", "--goal", "01m15jmta93b130wka2qdn2p1x"],
    ] {
        let (code, _, stderr) = run_all(&endpoint, &args).await;
        assert_eq!(code, 4, "{args:?}: {stderr}");
    }

    // A cursor belongs to one host.
    let (code, _, stderr) = run_all(&endpoint, &["session", "ls", "--cursor", "c2"]).await;
    assert_eq!(code, 2, "{stderr}");
    assert!(stderr.contains("--host NAME"), "{stderr}");

    // `attention` keeps its bullets, with the host of each item.
    let (code, stdout, stderr) = run_all(&endpoint, &["attention"]).await;
    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.starts_with("NEEDS ATTENTION\n"), "{stdout}");
    assert!(stdout.contains("  host: local\n"), "{stdout}");
    assert!(
        stdout.contains("  dismiss: ariadne attention dismiss blocked --host local\n"),
        "{stdout}"
    );
    assert!(
        stderr.contains("build: Some of what needs attention could not be read"),
        "{stderr}"
    );

    // An empty list on every host says so one time; both hosts are read at once.
    let (code, stdout, stderr) = run_all(&endpoint, &["skill", "ls"]).await;
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(
        stdout.lines().next().unwrap().split_whitespace().next(),
        Some("HOST")
    );
    assert_eq!(
        stderr.matches("No skills are available.").count(),
        1,
        "{stderr}"
    );
    server.abort();
    remote.abort();
}

/// With `local` alone, a list of every host still has its `HOST` column and
/// its `host` field.
#[tokio::test]
async fn all_with_local_alone_keeps_the_host_column_and_field() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let registry = json!([host("local", "ready", Some(&endpoint))]);
    let app = list_daemon(
        json!({"sessions": [{"id": "s-local", "agent_id": "codex"}], "next_cursor": null, "total": 1}),
        json!([]),
        json!({"complete": true, "items": []}),
        Arc::new(tokio::sync::Barrier::new(1)),
    )
    .route("/v1/hosts", get(move || async move { Json(registry) }));
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let (_, stdout, _) = run_all(&endpoint, &["session", "ls"]).await;
    assert!(stdout.starts_with("HOST "), "{stdout}");
    assert!(stdout.contains("\nlocal "), "{stdout}");
    let (_, stdout, _) = run_all(&endpoint, &["--format", "json", "session", "ls"]).await;
    let page: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(page["sessions"][0]["host"], "local");
    assert_eq!(page["next_cursor"], json!({"local": null}));
    server.abort();
}

#[tokio::test]
async fn query_on_an_unknown_or_unready_host_is_a_usage_error() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let sync =
        json!({"state":"in_sync","error":null,"missing_repositories":[],"skipped_permissions":0});
    let registry = json!([
        {"name":"local","kind":"local","status":"ready","endpoint":endpoint,"error":null,"remote":null,"sync":sync},
        {"name":"offline","kind":"ssh","status":"down","endpoint":null,"error":"no forward","remote":null,"sync":sync}
    ]);
    let app = Router::new().route("/v1/hosts", get(move || async move { Json(registry) }));
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    for (host, expected) in [
        ("missing", "local (ready), offline (down)"),
        ("offline", "is down"),
    ] {
        let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_ariadne"))
            .args(["--endpoint", &endpoint, "--host", host, "query", "up"])
            .env("ARIADNE_TELEMETRY", "off")
            .output()
            .await
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(2), "{stderr}");
        assert!(stderr.contains(expected), "{stderr}");
    }
    server.abort();
}

#[tokio::test]
async fn watched_attention_merges_ready_hosts_before_streams_end() {
    use tokio::io::AsyncBufReadExt;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let sync =
        json!({"state":"in_sync","error":null,"missing_repositories":[],"skipped_permissions":0});
    let registry = json!([
        {"name":"local","kind":"local","status":"ready","endpoint":endpoint,"error":null,"remote":null,"sync":sync},
        {"name":"build","kind":"direct","status":"ready","endpoint":endpoint,"error":null,"remote":null,"sync":sync}
    ]);
    let attention = json!({"complete":true,"items":[{
        "id":"blocked","producer":"recovery","reason":"access","summary":"Sign in",
        "required_action":"Sign in","since":"2026-11-17T00:00:00Z","affected":[],
        "target":{"kind":"settings","section":"agents"}
    }]});
    let changed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let change_read = changed.clone();
    let notify = Arc::new(tokio::sync::Notify::new());
    let events = notify.clone();
    let app = Router::new()
        .route("/v1/hosts", get(move || async move { Json(registry) }))
        .route(
            "/v1/attention",
            get(move || {
                let mut attention = attention.clone();
                if change_read.load(std::sync::atomic::Ordering::SeqCst) {
                    attention["items"][0]["summary"] = "Changed".into();
                }
                async move { Json(attention) }
            }),
        )
        .route(
            "/v1/events/stream",
            get(move || {
                let events = events.clone();
                async move {
                    use futures_util::StreamExt;
                    let update = futures_util::stream::once(async move {
                        events.notified().await;
                        Ok::<_, std::convert::Infallible>(
                            axum::response::sse::Event::default()
                                .event("task_updated")
                                .data("{}"),
                        )
                    });
                    axum::response::Sse::new(update.chain(futures_util::stream::pending()))
                }
            }),
        );
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_ariadne"))
        .args([
            "--endpoint",
            &endpoint,
            "--host",
            "all",
            "attention",
            "--watch",
            "--format",
            "json",
        ])
        .env("ARIADNE_TELEMETRY", "off")
        .stdout(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut lines = tokio::io::BufReader::new(child.stdout.take().unwrap()).lines();
    let read = async {
        let mut text = String::new();
        while let Some(line) = lines.next_line().await.unwrap() {
            text.push_str(&line);
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
                return value;
            }
        }
        panic!("watch ended before the merged snapshot");
    };
    let value = tokio::time::timeout(std::time::Duration::from_secs(30), read)
        .await
        .unwrap();
    assert_eq!(value["recovery"][0]["host"], "local");
    assert_eq!(value["recovery"][1]["host"], "build");
    changed.store(true, std::sync::atomic::Ordering::SeqCst);
    notify.notify_one();
    let redraw = async {
        let mut text = String::new();
        while let Some(line) = lines.next_line().await.unwrap() {
            text.push_str(&line);
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
                if value["recovery"][0]["summary"] == "Changed"
                    && value["recovery"][1]["summary"] == "Changed"
                {
                    return;
                }
                text.clear();
            }
        }
        panic!("watch ended before the change");
    };
    tokio::time::timeout(std::time::Duration::from_secs(30), redraw)
        .await
        .unwrap();
    child.kill().await.unwrap();
    server.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn daemon_follow_keeps_other_hosts_after_a_drop_and_stops_on_interrupt() {
    use futures_util::StreamExt;
    use tokio::io::AsyncBufReadExt;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let remote_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let remote_endpoint = format!("http://{}", remote_listener.local_addr().unwrap());
    let sync =
        json!({"state":"in_sync","error":null,"missing_repositories":[],"skipped_permissions":0});
    let registry = json!([
        {"name":"local","kind":"local","status":"ready","endpoint":endpoint,"error":null,"remote":null,"sync":sync},
        {"name":"build","kind":"direct","status":"ready","endpoint":remote_endpoint,"error":null,"remote":null,"sync":sync},
        {"name":"offline","kind":"ssh","status":"down","endpoint":null,"error":"down","remote":null,"sync":sync}
    ]);
    let logs = |stay: bool| {
        get(move || async move {
            let event = axum::response::sse::Event::default().event("delta").data(json!({
            "ts":"2026-11-17T00:00:00Z", "level":"INFO", "target":"test", "message":"live-line", "service":"ariadned"
        }).to_string());
            let stream = futures_util::stream::iter([Ok::<_, std::convert::Infallible>(event)])
                .chain(futures_util::stream::poll_fn(move |_| {
                    if stay {
                        std::task::Poll::Pending
                    } else {
                        std::task::Poll::Ready(None)
                    }
                }));
            axum::response::Sse::new(stream)
        })
    };
    let app = Router::new()
        .route("/v1/hosts", get(move || async move { Json(registry) }))
        .route("/v1/logs/stream", logs(true));
    let remote_app = Router::new().route("/v1/logs/stream", logs(false));
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let remote =
        tokio::spawn(async move { axum::serve(remote_listener, remote_app).await.unwrap() });
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_ariadne"))
        .args([
            "--endpoint",
            &endpoint,
            "--host",
            "all",
            "daemon",
            "logs",
            "-f",
        ])
        .env("ARIADNE_TELEMETRY", "off")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut lines = tokio::io::BufReader::new(child.stdout.take().unwrap()).lines();
    let mut errors = tokio::io::BufReader::new(child.stderr.take().unwrap()).lines();
    let read = async {
        let first = lines.next_line().await.unwrap().unwrap();
        let second = lines.next_line().await.unwrap().unwrap();
        assert!(first.contains("live-line") && second.contains("live-line"));
        assert!(first.starts_with("local") || second.starts_with("local"));
        assert!(first.starts_with("build") || second.starts_with("build"));
        let first = errors.next_line().await.unwrap().unwrap();
        let second = errors.next_line().await.unwrap().unwrap();
        assert!(first.contains("offline"), "{first}");
        assert!(second.contains("build"), "{second}");
    };
    tokio::time::timeout(std::time::Duration::from_secs(30), read)
        .await
        .unwrap();
    assert!(child.try_wait().unwrap().is_none());
    let status = tokio::process::Command::new("kill")
        .args(["-INT", &child.id().unwrap().to_string()])
        .status()
        .await
        .unwrap();
    assert!(status.success());
    let status = tokio::time::timeout(std::time::Duration::from_secs(30), child.wait())
        .await
        .unwrap()
        .unwrap();
    assert!(status.success());
    assert!(errors.next_line().await.unwrap().is_none());
    server.abort();
    remote.abort();
}

/// A stub daemon on a port of its own: its endpoint, and the task that serves it.
async fn serve(app: Router) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (endpoint, server)
}

/// A daemon that fails every request with its own error.
fn broken_daemon() -> Router {
    Router::new().fallback(|| async {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"code": "internal", "message": "the disk is full"}})),
        )
    })
}

fn dashboard(name: &str, title: &str) -> serde_json::Value {
    json!({
        "name": name, "document": {"title": title}, "builtin": false,
        "document_is_default": false, "created_at": "2026-01-01", "updated_at": "2026-01-01"
    })
}

/// A local daemon that serves `registry` and the dashboards in `dashboards`.
fn local_daemon(
    registry: Arc<Mutex<serde_json::Value>>,
    dashboards: Vec<serde_json::Value>,
) -> Router {
    named_dashboards(dashboards).route(
        "/v1/hosts",
        get(move || {
            let registry = Arc::clone(&registry);
            async move { Json(registry.lock().unwrap().clone()) }
        }),
    )
}

fn named_dashboards(dashboards: Vec<serde_json::Value>) -> Router {
    Router::new().route(
        "/v1/dashboards/{name}",
        get(move |Path(name): Path<String>| {
            let found = dashboards
                .iter()
                .find(|d| d["name"] == name.as_str())
                .cloned();
            async move {
                match found {
                    Some(found) => Ok(Json(found)),
                    None => Err((
                        StatusCode::NOT_FOUND,
                        Json(
                            json!({"error": {"code": "not_found", "message": "no such dashboard"}}),
                        ),
                    )),
                }
            }
        }),
    )
}

/// A read of one ID with `--host all` names the one host that has it, says
/// the true error of each host that fails, and refuses an ID on two hosts
/// (spec 014, rule 46b).
#[tokio::test]
async fn all_reads_one_id_on_the_host_that_has_it() {
    let registry = Arc::new(Mutex::new(json!([])));
    let (endpoint, server) = serve(local_daemon(
        Arc::clone(&registry),
        vec![dashboard("shared", "on local")],
    ))
    .await;
    let (build, remote) = serve(named_dashboards(vec![
        dashboard("shared", "on build"),
        dashboard("theirs", "only on build"),
    ]))
    .await;
    let (broken, failing) = serve(broken_daemon()).await;
    let mut local = host("local", "ready", Some(&endpoint));
    local["kind"] = "local".into();
    *registry.lock().unwrap() = json!([
        local.clone(),
        host("build", "ready", Some(&build)),
        host("broken", "ready", Some(&broken)),
        host("offline", "down", None),
    ]);

    // The host that has the ID answers, and is named on the first line.
    let (code, stdout, stderr) = run_all(&endpoint, &["dashboard", "get", "theirs"]).await;
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout.lines().next(), Some("host: build"), "{stdout}");
    assert!(stdout.contains("only on build"), "{stdout}");
    // The host that fails and the host that is down each get one warning.
    assert_eq!(
        stderr.lines().collect::<Vec<_>>(),
        [
            "warning: broken: the disk is full",
            "warning: offline: down"
        ],
        "{stderr}"
    );

    // In JSON, the host is a field of the answer.
    let (code, stdout, stderr) = run_all(
        &endpoint,
        &["dashboard", "get", "theirs", "--format", "json"],
    )
    .await;
    assert_eq!(code, 0, "{stderr}");
    let shown: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(shown["host"], "build", "{stdout}");
    assert_eq!(shown["title"], "only on build", "{stdout}");

    // An ID on two hosts is refused, and the refusal names both.
    let (code, stdout, stderr) = run_all(&endpoint, &["dashboard", "get", "shared"]).await;
    assert_eq!(code, 2, "{stderr}");
    assert!(stdout.is_empty(), "{stdout}");
    assert!(
        stderr.contains("the ID matches on hosts local, build"),
        "{stderr}"
    );

    // A host that fails is not a host that lacks the ID.
    let (code, _, stderr) = run_all(&endpoint, &["dashboard", "get", "absent"]).await;
    assert_ne!(code, 0, "{stderr}");
    assert!(
        stderr.contains("warning: broken: the disk is full"),
        "{stderr}"
    );
    assert!(!stderr.contains("no host has that ID"), "{stderr}");

    // Only when each ready host answers not found is the ID on no host.
    *registry.lock().unwrap() = json!([
        local,
        host("build", "ready", Some(&build)),
        host("offline", "down", None),
    ]);
    let (code, _, stderr) = run_all(&endpoint, &["dashboard", "get", "absent"]).await;
    assert_eq!(code, 4, "{stderr}");
    assert!(stderr.contains("no host has that ID"), "{stderr}");
    server.abort();
    remote.abort();
    failing.abort();
}

/// A stub daemon that answers `version`, `skill get` and `daemon logs` as
/// the daemon `name`.
fn answering_daemon(name: &'static str) -> Router {
    Router::new()
        .route(
            "/v1/version",
            get(move || async move { Json(json!({"name": "ariadned", "version": name})) }),
        )
        .route(
            "/v1/skills/{skill}",
            get(move |Path(skill): Path<String>| async move {
                Json(json!({
                    "name": skill, "seat": "task", "summary": "s",
                    "document": format!("the {skill} skill of {name}\n"),
                    "document_is_default": false, "builtin": false,
                    "created_at": "2026-11-17T00:00:00Z", "updated_at": "2026-11-17T00:00:00Z"
                }))
            }),
        )
        .route(
            "/v1/logs",
            get(move || async move {
                Json(json!({"lines": [{
                    "ts": "2026-11-17T00:00:00Z", "level": "INFO", "target": "ariadne_daemon",
                    "message": format!("started on {name}"), "service": "ariadned"
                }]}))
            }),
        )
}

/// A read that every host answers prints one block for each ready host,
/// headed by its name, or a JSON array with a `host` on each item (spec 014,
/// rule 46b). `daemon logs` leads each line with its host.
#[tokio::test]
async fn all_reads_each_host_in_one_block_per_host() {
    let registry = Arc::new(Mutex::new(json!([])));
    let local_app = answering_daemon("1.0.0").merge(local_daemon(Arc::clone(&registry), vec![]));
    let (endpoint, server) = serve(local_app).await;
    let (build, remote) = serve(answering_daemon("2.0.0")).await;
    let (broken, failing) = serve(broken_daemon()).await;
    let mut local = host("local", "ready", Some(&endpoint));
    local["kind"] = "local".into();
    *registry.lock().unwrap() = json!([
        local,
        host("build", "ready", Some(&build)),
        host("broken", "ready", Some(&broken)),
        host("offline", "down", None),
    ]);

    let (code, stdout, stderr) = run_all(&endpoint, &["skill", "get", "coding"]).await;
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(
        stdout,
        "local\nthe coding skill of 1.0.0\n\nbuild\nthe coding skill of 2.0.0\n"
    );
    assert_eq!(
        stderr.lines().collect::<Vec<_>>(),
        [
            "warning: broken: the disk is full",
            "warning: offline: down"
        ],
        "{stderr}"
    );

    let (code, stdout, stderr) = run_all(&endpoint, &["version", "--format", "json"]).await;
    assert_eq!(code, 0, "{stderr}");
    let shown: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    // `version` answers for a daemon that fails, with the daemon's error.
    let rows: Vec<(&str, &serde_json::Value)> = shown
        .as_array()
        .unwrap()
        .iter()
        .map(|row| (row["host"].as_str().unwrap(), &row["daemon"]))
        .collect();
    assert_eq!(
        rows,
        [
            ("local", &json!({"name": "ariadned", "version": "1.0.0"})),
            ("build", &json!({"name": "ariadned", "version": "2.0.0"})),
            ("broken", &json!({"error": "the disk is full"})),
        ],
        "{stdout}"
    );

    let (code, stdout, stderr) = run_all(&endpoint, &["daemon", "logs"]).await;
    assert_eq!(code, 0, "{stderr}");
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 2, "{stdout}");
    assert!(lines[0].starts_with("local     "), "{stdout}");
    assert!(lines[0].ends_with("started on 1.0.0"), "{stdout}");
    assert!(lines[1].starts_with("build     "), "{stdout}");
    assert!(lines[1].ends_with("started on 2.0.0"), "{stdout}");
    assert!(stderr.contains("warning: broken:"), "{stderr}");
    server.abort();
    remote.abort();
    failing.abort();
}

/// A document piped to `workflow check --host all` reaches every ready host,
/// not only the first one (spec 014, rule 46b).
#[tokio::test]
async fn all_reads_each_host_gives_a_piped_document_to_each_host() {
    fn parser() -> Router {
        Router::new().route(
            "/v1/workflows/parse",
            post(|Json(body): Json<serde_json::Value>| async move {
                let document = body["document"].as_str().unwrap_or_default().to_owned();
                if document.trim().is_empty() {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(json!({"error": {
                            "code": "workflow_invalid", "message": "the document is empty",
                            "details": {"line": 1}
                        }})),
                    );
                }
                (
                    StatusCode::OK,
                    Json(json!({"name": "x", "steps": [
                        {"id": "a", "title": "A", "description": "", "skills": [],
                         "ranks": [], "gate": null}
                    ]})),
                )
            }),
        )
    }
    let registry = Arc::new(Mutex::new(json!([])));
    let local_app = parser().merge(local_daemon(Arc::clone(&registry), vec![]));
    let (endpoint, server) = serve(local_app).await;
    let (build, remote) = serve(parser()).await;
    let mut local = host("local", "ready", Some(&endpoint));
    local["kind"] = "local".into();
    *registry.lock().unwrap() = json!([local, host("build", "ready", Some(&build))]);

    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_ariadne"))
        .args(["--endpoint", &endpoint, "--host", "all"])
        .args(["workflow", "check", "--format", "json"])
        .env("ARIADNE_TELEMETRY", "off")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    {
        use tokio::io::AsyncWriteExt;
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all(b"workflow x\n  a[A]\n").await.unwrap();
    }
    let output = child.wait_with_output().await.unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(0), "{stderr}");
    let shown: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let hosts: Vec<&str> = shown
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["host"].as_str().unwrap())
        .collect();
    assert_eq!(hosts, ["local", "build"], "{shown} {stderr}");
    assert_eq!(stderr, "", "{stderr}");
    server.abort();
    remote.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn watched_snapshot_failure_warns_once_and_stops_reading_that_host() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncBufReadExt, AsyncReadExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let remote_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let remote_endpoint = format!("http://{}", remote_listener.local_addr().unwrap());
    let mut local_host = host_row("local", "local", "ready", None);
    local_host["endpoint"] = endpoint.clone().into();
    let mut remote_host = host_row("build", "direct", "ready", None);
    remote_host["endpoint"] = remote_endpoint.into();
    let registry = json!([local_host, remote_host]);
    let (changes, local_events) = tokio::sync::mpsc::unbounded_channel::<()>();
    let local_events = Arc::new(Mutex::new(Some(local_events)));
    let dropped = Arc::new(tokio::sync::Notify::new());
    let remote_events = dropped.clone();
    let requests = Arc::new(AtomicUsize::new(0));
    let remote_requests = requests.clone();
    let app = Router::new()
        .route("/v1/hosts", get(move || async move { Json(registry) }))
        .route(
            "/v1/attention",
            get(|| async { Json(json!({"complete":true,"items":[]})) }),
        )
        .route(
            "/v1/events/stream",
            get(move || {
                let events = local_events.lock().unwrap().take().unwrap();
                async move {
                    axum::response::Sse::new(futures_util::stream::unfold(
                        events,
                        |mut events| async move {
                            events.recv().await?;
                            Some((
                                Ok::<_, std::convert::Infallible>(
                                    axum::response::sse::Event::default()
                                        .event("task_updated")
                                        .data("{}"),
                                ),
                                events,
                            ))
                        },
                    ))
                }
            }),
        );
    let remote_app = Router::new()
        .route(
            "/v1/attention",
            get(move || {
                remote_requests.fetch_add(1, Ordering::SeqCst);
                dropped.notify_one();
                async { StatusCode::SERVICE_UNAVAILABLE }
            }),
        )
        .route(
            "/v1/events/stream",
            get(move || {
                let events = remote_events.clone();
                async move {
                    axum::response::Sse::new(futures_util::stream::once(async move {
                        events.notified().await;
                        Ok::<_, std::convert::Infallible>(
                            axum::response::sse::Event::default()
                                .event("task_updated")
                                .data("{}"),
                        )
                    }))
                }
            }),
        );
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let remote =
        tokio::spawn(async move { axum::serve(remote_listener, remote_app).await.unwrap() });
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_ariadne"))
        .args([
            "--endpoint",
            &endpoint,
            "--host",
            "all",
            "attention",
            "--watch",
            "--format",
            "json",
        ])
        .env("ARIADNE_TELEMETRY", "off")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut lines = tokio::io::BufReader::new(child.stdout.take().unwrap()).lines();
    let mut stderr = child.stderr.take().unwrap();
    let follow = async {
        for _ in 0..4 {
            let mut text = String::new();
            loop {
                text.push_str(&lines.next_line().await.unwrap().unwrap());
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
                    assert_eq!(value["recovery"], json!([]));
                    break;
                }
            }
            changes.send(()).unwrap();
        }
        let status = tokio::process::Command::new("kill")
            .args(["-INT", &child.id().unwrap().to_string()])
            .status()
            .await
            .unwrap();
        assert!(status.success());
        assert!(child.wait().await.unwrap().success());
        let mut errors = String::new();
        stderr.read_to_string(&mut errors).await.unwrap();
        errors
    };
    let errors = tokio::time::timeout(std::time::Duration::from_secs(30), follow)
        .await
        .unwrap();
    server.abort();
    remote.abort();
    assert_eq!(errors.matches("warning: build:").count(), 1, "{errors}");
    assert_eq!(requests.load(Ordering::SeqCst), 1);
}

/// A daemon that has the one task `id` among its sessions, tasks and goals.
fn task_daemon(id: &'static str) -> Router {
    Router::new()
        .route(
            "/v1/sessions",
            get(|| async { Json(json!({"sessions": [], "next_cursor": null, "total": 0})) }),
        )
        .route(
            "/v1/tasks",
            get(move || async move { Json(json!([{"id": id, "title": "work"}])) }),
        )
        .route("/v1/goals", get(|| async { Json(json!([])) }))
        .route(
            "/v1/tasks/{id}",
            get(move || async move { Json(json!({"id": id, "title": "work"})) }),
        )
}

/// `attach --host all` warns for each host that is not ready, and refuses a
/// prefix that matches on two hosts (spec 014, rule 46b).
#[tokio::test]
async fn all_attach_refuses_a_prefix_on_two_hosts() {
    let registry = Arc::new(Mutex::new(json!([])));
    let local_app = task_daemon("01aaaaaaaaaaaaaaaaaaaaaaaa")
        .merge(local_daemon(Arc::clone(&registry), vec![]));
    let (endpoint, server) = serve(local_app).await;
    let (build, remote) = serve(task_daemon("01aaaaaaaaaaaaaaaaaaaaaaab")).await;
    let mut local = host("local", "ready", Some(&endpoint));
    local["kind"] = "local".into();
    *registry.lock().unwrap() = json!([
        local,
        host("build", "ready", Some(&build)),
        host("offline", "down", None),
    ]);
    let (code, _, stderr) = run_all(&endpoint, &["attach", "01aaaa"]).await;
    assert_eq!(code, 2, "{stderr}");
    assert!(stderr.contains("warning: offline: down"), "{stderr}");
    assert!(
        stderr.contains("the ID matches on hosts local, build"),
        "{stderr}"
    );
    server.abort();
    remote.abort();
}

/// With only `local` in the registry, the reads of every host keep their
/// form: the `host:` line, the heading and the `host` field (spec 014, rule
/// 46b).
#[tokio::test]
async fn all_reads_with_local_alone_keep_the_host() {
    let registry = Arc::new(Mutex::new(json!([])));
    let local_app = answering_daemon("1.0.0").merge(local_daemon(
        Arc::clone(&registry),
        vec![dashboard("mine", "on local")],
    ));
    let (endpoint, server) = serve(local_app).await;
    let mut local = host("local", "ready", Some(&endpoint));
    local["kind"] = "local".into();
    *registry.lock().unwrap() = json!([local]);

    let (code, stdout, stderr) = run_all(&endpoint, &["dashboard", "get", "mine"]).await;
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout.lines().next(), Some("host: local"), "{stdout}");
    let (code, stdout, stderr) = run_all(&endpoint, &["skill", "get", "coding"]).await;
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "local\nthe coding skill of 1.0.0\n");
    let (code, stdout, stderr) =
        run_all(&endpoint, &["skill", "get", "coding", "--format", "json"]).await;
    assert_eq!(code, 0, "{stderr}");
    let shown: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(shown[0]["host"], "local", "{stdout}");
    assert_eq!(shown.as_array().unwrap().len(), 1, "{stdout}");
    server.abort();
}

#[tokio::test]
async fn worker_setup_and_status_show_each_completed_step_and_failure() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let progress = json!({"name":"worker", "state":"incomplete", "steps":[
        {"name":"install", "complete":true, "error":null},
        {"name":"service", "complete":false, "error":"Enable user services"},
        {"name":"tool:codex-acp", "complete":true, "error":null},
        {"name":"authentication:github", "complete":false, "error":"Sign in separately on the worker"}
    ]});
    let answer = progress.clone();
    let received = Arc::new(Mutex::new(None));
    let body = received.clone();
    let app = Router::new().route(
        "/v1/hosts/worker/setup",
        get(move || async move { Json(progress) }).post(
            move |Json(request): Json<serde_json::Value>| async move {
                *body.lock().unwrap() = Some(request);
                (StatusCode::ACCEPTED, Json(answer))
            },
        ),
    );
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("request.json");
    std::fs::write(&file, json!({"ssh":"ssh://me@worker", "source":{"kind":"local", "cli":"/tmp/ariadne", "daemon":"/tmp/ariadned"}, "tools":{"agents":["codex-acp"],"github":true}, "credentials":["codex"]}).to_string()).unwrap();
    for args in [
        vec![
            "host",
            "setup",
            "worker",
            "--request",
            file.to_str().unwrap(),
        ],
        vec!["host", "setup-status", "worker"],
    ] {
        let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_ariadne"))
            .args(["--endpoint", &endpoint])
            .args(args)
            .env("ARIADNE_TELEMETRY", "off")
            .output()
            .await
            .unwrap();
        assert!(!output.status.success());
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(text.contains("install: complete"), "{text}");
        assert!(text.contains("Enable user services"), "{text}");
        assert!(text.contains("tool:codex-acp: complete"), "{text}");
        assert!(text.contains("Sign in separately on the worker"), "{text}");
    }
    assert_eq!(
        received.lock().unwrap().as_ref().unwrap()["ssh"],
        "ssh://me@worker"
    );
    let request = received.lock().unwrap().clone().unwrap();
    assert_eq!(
        request["tools"],
        json!({"agents":["codex-acp"], "github":true})
    );
    assert_eq!(request["credentials"], json!(["codex"]));
    server.abort();
}

type Seen = Arc<Mutex<Vec<(String, String, serde_json::Value)>>>;

/// A daemon that records each host write it gets and answers it with the
/// row of `build`, a 404 for the name `gone` and a 409 for `dupe`.
async fn writes_registry() -> (String, Seen) {
    use axum::http::Method;
    use axum::response::IntoResponse;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let seen: Seen = Arc::default();
    let log = Arc::clone(&seen);
    let app = Router::new().fallback(move |method: Method, request: Request| {
        let log = Arc::clone(&log);
        async move {
            let path = request.uri().path().to_owned();
            let bytes = axum::body::to_bytes(request.into_body(), usize::MAX)
                .await
                .unwrap();
            let body = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
            log.lock()
                .unwrap()
                .push((method.to_string(), path.clone(), body));
            if path.ends_with("/gone") || path.ends_with("/dupe") {
                let status = if path.ends_with("/gone") {
                    StatusCode::NOT_FOUND
                } else {
                    StatusCode::CONFLICT
                };
                return (
                    status,
                    Json(json!({"error":{"code":"unknown_host","message":"no host named gone"}})),
                )
                    .into_response();
            }
            let mut row = host_row("build", "direct", "ready", None);
            row["color"] = json!("teal");
            match method {
                Method::POST => (StatusCode::CREATED, Json(row)).into_response(),
                Method::PATCH => Json(row).into_response(),
                _ => StatusCode::NO_CONTENT.into_response(),
            }
        }
    });
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (endpoint, seen)
}

/// Spec 014, rule 52.
#[tokio::test]
async fn host_add_posts_the_host_and_prints_its_row() {
    let (endpoint, seen) = writes_registry().await;
    let (code, text) = run_cli(&[
        "--endpoint",
        &endpoint,
        "host",
        "add",
        "build",
        "--endpoint",
        "http://10.0.0.2:7676",
        "--color",
        "teal",
    ])
    .await;
    assert_eq!(code, 0, "{text}");
    assert!(text.lines().last().unwrap().starts_with("build"), "{text}");
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].0, "POST");
    assert_eq!(seen[0].1, "/v1/hosts");
    assert_eq!(seen[0].2["name"], "build");
    assert_eq!(seen[0].2["endpoint"], "http://10.0.0.2:7676");
    assert_eq!(seen[0].2["color"], "teal");
}

/// Spec 014, rule 53.
#[tokio::test]
async fn host_edit_patches_only_the_given_fields_and_null_clears() {
    let (endpoint, seen) = writes_registry().await;
    for args in [
        vec!["--endpoint", "http://10.0.0.3:7676"],
        vec!["--remote", "127.0.0.1:7676", "--color", "pink"],
        vec!["--no-remote", "--no-color"],
    ] {
        let mut full = vec!["--endpoint", endpoint.as_str(), "host", "edit", "build"];
        full.extend(args.iter());
        let (code, text) = run_cli(&full).await;
        assert_eq!(code, 0, "{args:?}: {text}");
        assert!(text.lines().last().unwrap().starts_with("build"), "{text}");
    }
    let seen = seen.lock().unwrap();
    assert!(
        seen.iter()
            .all(|(m, p, _)| m == "PATCH" && p == "/v1/hosts/build")
    );
    assert_eq!(seen[0].2, json!({"endpoint": "http://10.0.0.3:7676"}));
    assert_eq!(
        seen[1].2,
        json!({"remote": "127.0.0.1:7676", "color": "pink"})
    );
    assert_eq!(seen[2].2, json!({"remote": null, "color": null}));
}

/// Spec 014, rule 53.
#[tokio::test]
async fn host_edit_with_no_flag_is_refused_before_any_request() {
    let (endpoint, seen) = writes_registry().await;
    let (code, text) = run_cli(&["--endpoint", &endpoint, "host", "edit", "build"]).await;
    assert_eq!(code, 2, "{text}");
    assert!(text.contains("nothing to change"), "{text}");
    assert!(seen.lock().unwrap().is_empty());
}

/// Spec 014, rule 54.
#[tokio::test]
async fn host_flags_refuse_a_colour_outside_the_eight_and_a_clashing_pair() {
    let (endpoint, seen) = writes_registry().await;
    for args in [
        vec!["host", "add", "x", "--endpoint", "e", "--color", "mauve"],
        vec!["host", "edit", "x", "--color", "mauve"],
        vec!["host", "edit", "x", "--color", "red", "--no-color"],
        vec!["host", "edit", "x", "--remote", "r", "--no-remote"],
    ] {
        let mut full = vec!["--endpoint", endpoint.as_str()];
        full.extend(args.iter());
        let (code, text) = run_cli(&full).await;
        assert_eq!(code, 2, "{args:?}: {text}");
    }
    assert!(seen.lock().unwrap().is_empty());
}

/// Spec 014, rule 55.
#[tokio::test]
async fn host_rm_deletes_the_host_and_prints_its_name() {
    let (endpoint, seen) = writes_registry().await;
    let (code, text) = run_cli(&["--endpoint", &endpoint, "host", "rm", "build"]).await;
    assert_eq!(code, 0, "{text}");
    assert_eq!(text.trim(), "build");
    let seen = seen.lock().unwrap();
    assert_eq!(
        (seen[0].0.as_str(), seen[0].1.as_str()),
        ("DELETE", "/v1/hosts/build")
    );
}

/// Spec 014, rule 56.
#[tokio::test]
async fn host_writes_print_the_daemon_error_and_json_prints_the_dto() {
    let (endpoint, _) = writes_registry().await;
    let (code, text) = run_cli(&["--endpoint", &endpoint, "host", "rm", "gone"]).await;
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("no host named gone"), "{text}");
    let (code, text) = run_cli(&[
        "--endpoint",
        &endpoint,
        "--format",
        "json",
        "host",
        "rm",
        "gone",
    ])
    .await;
    assert_eq!(code, 1, "{text}");
    let envelope: serde_json::Value = serde_json::from_str(text.trim()).unwrap();
    assert_eq!(envelope["error"]["code"], "unknown_host", "{text}");
    assert_eq!(envelope["error"]["status"], 404, "{text}");
    assert_eq!(envelope["error"]["message"], "no host named gone", "{text}");
    let (code, text) = run_cli(&[
        "--endpoint",
        &endpoint,
        "host",
        "edit",
        "dupe",
        "--color",
        "red",
    ])
    .await;
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("no host named gone"), "{text}");
    let (code, text) = run_cli(&[
        "--endpoint",
        &endpoint,
        "--format",
        "json",
        "host",
        "edit",
        "build",
        "--color",
        "teal",
    ])
    .await;
    assert_eq!(code, 0, "{text}");
    let host: ariadne_api::hosts::HostDto = serde_json::from_str(&text).unwrap();
    assert_eq!(host.name, "build");
    assert_eq!(host.color, Some(ariadne_api::hosts::HostColor::Teal));
}

/// Spec 014, rule 57: `--host` does not move a registry write off `local`.
#[tokio::test]
async fn host_writes_refuse_host_all_and_run_on_local_for_host_local() {
    let (endpoint, seen) = writes_registry().await;
    let (code, text) = run_cli(&[
        "--endpoint",
        &endpoint,
        "--host",
        "all",
        "host",
        "rm",
        "build",
    ])
    .await;
    assert_eq!(code, 2, "{text}");
    assert!(seen.lock().unwrap().is_empty());
}
