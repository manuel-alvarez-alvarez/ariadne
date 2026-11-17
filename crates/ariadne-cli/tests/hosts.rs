//! Host selection through the executable and a real registry route.
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
        {"name":"local","kind":"local","status":"ready","endpoint":endpoint,"error":null,"remote":null},
        {"name":"build","kind":"direct","status":"ready","endpoint":remote_endpoint,"error":null,"remote":null},
        {"name":"offline","kind":"ssh","status":"down","endpoint":null,"error":"no forward","remote":null}
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
        ("all", vec!["query", "up"], 2, "query"),
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
