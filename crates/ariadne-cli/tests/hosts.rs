//! Host selection through the executable and a real registry route.
use std::sync::{Arc, Mutex};

use axum::{Json, Router, routing::get};
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
