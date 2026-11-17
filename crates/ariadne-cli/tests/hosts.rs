//! Host selection through the executable and a real registry route.
use std::sync::{Arc, Mutex};

use axum::{
    Json, Router,
    extract::Path,
    http::StatusCode,
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
        ("missing", vec!["host", "ls"], 2, "offline (down)"),
        ("offline", vec!["host", "ls"], 2, "build (ready)"),
        ("build", vec!["skill", "reset", "coding"], 2, "synced"),
        ("build", vec!["daemon", "stop"], 2, "local"),
        ("all", vec!["daemon", "stop"], 2, "build"),
        (
            "all",
            vec!["host", "ls", "--format", "json"],
            0,
            "\"host\": \"local\"",
        ),
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
