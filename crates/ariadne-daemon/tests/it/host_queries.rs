//! The query routes of a host and of every host (037): the local daemon runs
//! a request on a configured host's daemon, or on all of them, and merges.
//! The far daemon is a second harness served on a loopback port.

use crate::common;

use axum::http::StatusCode;
use serde_json::{Value, json};

use ariadne_api::hosts::{HostDto, HostStatus};
use ariadne_store::telemetry::{MetricKind, MetricPoint, NewLogRecord};

use common::acp::{discovery_settled, registry_home, script, stub_acp_agent};
use common::{Harness, TIMEOUT, eventually, get, harness};

/// 2026-01-01T10:00:00Z, in Unix seconds.
const T: i64 = 1_767_261_600;

fn at(offset_seconds: i64) -> String {
    chrono::DateTime::from_timestamp(T + offset_seconds, 0)
        .unwrap()
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// One token counter and one log line, each with `tokens` and `line` to tell
/// the daemons apart.
async fn seed(h: &Harness, tokens: f64, line: &str) {
    h.telemetry_store
        .write_metrics(&[MetricPoint {
            name: "ariadne.tokens".into(),
            kind: MetricKind::Counter,
            unit: Some("{token}".into()),
            description: Some("tokens".into()),
            labels: json!({"seat": "agent"}),
            timestamp: at(-60),
            value: tokens,
        }])
        .await
        .unwrap();
    h.telemetry_store
        .write_logs(&[NewLogRecord {
            ts: at(-60),
            observed_ts: at(-60),
            severity_number: None,
            severity_text: Some("INFO".into()),
            body: Some(line.into()),
            service: Some("ariadned".into()),
            scope: None,
            target: None,
            attributes: json!({}),
            trace_id: None,
            span_id: None,
        }])
        .await
        .unwrap();
}

/// A local daemon with the hosts `far` (the second daemon), `dead` (nothing
/// listens) and `box` (an ssh forward that is not up), and the far daemon.
async fn pair() -> (Harness, Harness, tempfile::TempDir) {
    pair_with(harness().await).await
}

/// [`pair`] with `far` as the far daemon.
async fn pair_with(far: Harness) -> (Harness, Harness, tempfile::TempDir) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = far.router.clone();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(
        home.join("host-fixture.toml"),
        // No `ssh` runs: the forward of `box` cannot start.
        format!(
            "ssh_bin = \"{}\"\n\
             [hosts.far]\nendpoint = \"http://{address}\"\n\
             [hosts.dead]\nendpoint = \"http://127.0.0.1:1\"\n\
             [hosts.box]\nendpoint = \"ssh://me@box\"\n",
            dir.path().join("no-ssh").display()
        ),
    )
    .unwrap();
    std::fs::write(
        home.join("config.toml"),
        format!("ssh_bin = {:?}", dir.path().join("no-ssh")),
    )
    .unwrap();
    let local = harness().home(home).await;
    // The checks make `far` ready. `dead` and `box` never are.
    local.state.hosts.start();
    eventually(TIMEOUT, "the host far ready", || async {
        let hosts: Vec<HostDto> = local.get("/v1/hosts").await;
        hosts[1].status == HostStatus::Ready
    })
    .await;
    seed(&local, 30.0, "local line").await;
    seed(&far, 7.0, "far line").await;
    (local, far, dir)
}

fn query_uri(host: &str, path: &str, params: &[(&str, &str)]) -> String {
    format!(
        "/v1/hosts/{host}/{path}?{}",
        serde_urlencoded::to_string(params).unwrap()
    )
}

async fn data(h: &Harness, uri: &str) -> Value {
    let body: Value = h.get(uri).await;
    assert_eq!(body["status"], "success", "{body}");
    body
}

fn instant(host: &str) -> String {
    query_uri(
        host,
        "api/v1/query",
        &[
            ("query", r#"ariadne_tokens_total{seat="agent"}"#),
            ("time", &T.to_string()),
        ],
    )
}

#[tokio::test]
async fn a_named_host_answers_from_its_own_daemon_and_local_from_this_one() {
    let (local, _far, _dir) = pair().await;
    let far = data(&local, &instant("far")).await;
    assert_eq!(far["data"]["result"][0]["value"][1], "7");
    let own = data(&local, &instant("local")).await;
    assert_eq!(own["data"]["result"][0]["value"][1], "30");
    // The answer is the far daemon's, so it carries no host label.
    assert!(far["data"]["result"][0]["metric"]["host"].is_null());
}

#[tokio::test]
async fn an_unknown_host_is_404_and_a_host_that_is_not_ready_is_503() {
    let (local, _far, _dir) = pair().await;
    let error = local
        .error(get(&instant("nowhere")), StatusCode::NOT_FOUND)
        .await;
    assert!(error.error.message.contains("nowhere"), "{error:?}");

    let hosts: Vec<HostDto> = local.get("/v1/hosts").await;
    let status = hosts.iter().find(|h| h.name == "box").unwrap().status;
    let error = local
        .error(get(&instant("box")), StatusCode::SERVICE_UNAVAILABLE)
        .await;
    let status = format!("{status:?}").to_lowercase();
    assert!(error.error.message.contains(&status), "{error:?}");
}

#[tokio::test]
async fn all_merges_every_ready_host_with_a_host_label_and_lists_failures() {
    let (local, _far, _dir) = pair().await;
    let body = data(&local, &instant("all")).await;
    let mut hosts: Vec<(String, String)> = body["data"]["result"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            (
                s["metric"]["host"].as_str().unwrap().to_string(),
                s["value"][1].as_str().unwrap().to_string(),
            )
        })
        .collect();
    hosts.sort();
    assert_eq!(
        hosts,
        [("far".into(), "7".into()), ("local".into(), "30".into())]
    );
    assert_eq!(body["data"]["resultType"], "vector");
    let warnings = body["warnings"].to_string();
    assert!(warnings.contains("dead"), "{warnings}");
    assert!(warnings.contains("box"), "{warnings}");
}

#[tokio::test]
async fn all_labels_streams_series_and_label_values_with_the_host() {
    let (local, _far, _dir) = pair().await;
    let range = [
        ("query", r#"{service="ariadned"}"#.to_string()),
        ("start", (T - 300).to_string()),
        ("end", (T + 300).to_string()),
    ];
    let range: Vec<(&str, &str)> = range.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let streams = data(&local, &query_uri("all", "loki/api/v1/query_range", &range)).await;
    let mut seen: Vec<(String, String)> = streams["data"]["result"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            (
                s["stream"]["host"].as_str().unwrap().to_string(),
                s["values"][0][1].as_str().unwrap().to_string(),
            )
        })
        .collect();
    seen.sort();
    assert_eq!(
        seen,
        [
            ("far".into(), "far line".into()),
            ("local".into(), "local line".into())
        ]
    );

    let series = data(
        &local,
        &query_uri(
            "all",
            "api/v1/series",
            &[("match[]", "ariadne_tokens_total")],
        ),
    )
    .await;
    let mut hosts: Vec<&str> = series["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["host"].as_str().unwrap())
        .collect();
    hosts.sort();
    assert_eq!(hosts, ["far", "local"]);

    let labels = data(&local, &query_uri("all", "api/v1/labels", &[])).await;
    assert!(labels["data"].as_array().unwrap().contains(&json!("host")));
    let values = data(&local, &query_uri("all", "api/v1/label/host/values", &[])).await;
    assert_eq!(values["data"], json!(["far", "local"]));
}

#[tokio::test]
async fn a_query_every_host_refuses_answers_its_400() {
    let (local, _far, _dir) = pair().await;
    let (status, body) = local
        .send(get(&query_uri(
            "all",
            "api/v1/query",
            &[("query", "topk(")],
        )))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let body: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["errorType"], "bad_data", "{body}");
}

#[tokio::test]
async fn all_keeps_the_flat_shape_of_the_index_stats() {
    let (local, _far, _dir) = pair().await;
    let body: Value = local.get("/v1/hosts/all/loki/api/v1/index/stats").await;
    for key in ["streams", "chunks", "entries", "bytes"] {
        assert_eq!(body[key], 0, "{body}");
    }
    assert!(body["data"].is_null(), "{body}");
    assert!(body["warnings"].to_string().contains("dead"), "{body}");
}

#[tokio::test]
async fn all_keeps_the_warnings_on_a_parsed_query() {
    let (local, _far, _dir) = pair().await;
    let body = data(
        &local,
        &query_uri("all", "api/v1/parse_query", &[("query", "up")]),
    )
    .await;
    assert!(body["warnings"].to_string().contains("dead"), "{body}");
}

#[tokio::test]
async fn all_keeps_an_attribute_already_named_exported_host() {
    let (local, _far, _dir) = pair().await;
    local
        .telemetry_store
        .write_metrics(&[MetricPoint {
            name: "collision".into(),
            kind: MetricKind::Counter,
            unit: None,
            description: None,
            labels: json!({"host": "original", "exported_host": "keep-me"}),
            timestamp: at(-60),
            value: 1.0,
        }])
        .await
        .unwrap();
    let series = data(
        &local,
        &query_uri("all", "api/v1/series", &[("match[]", "collision_total")]),
    )
    .await;
    let set = &series["data"][0];
    assert_eq!(set["host"], "local", "{series}");
    assert_eq!(set["exported_host"], "keep-me", "{series}");
    assert_eq!(set["exported_exported_host"], "original", "{series}");
}

#[tokio::test]
async fn openapi_carries_the_host_query_routes() {
    let h = harness().await;
    let doc: Value = h.get("/api-docs/openapi.json").await;
    for path in [
        "/v1/hosts/{host}/api/v1/query",
        "/v1/hosts/{host}/api/v1/label/{name}/values",
        "/v1/hosts/{host}/loki/api/v1/query_range",
        "/v1/hosts/{host}/-/healthy",
    ] {
        assert!(doc["paths"][path]["get"].is_object(), "{path}");
    }
    assert!(doc["paths"]["/v1/hosts/{host}/api/v1/query"]["post"].is_object());
}

#[tokio::test]
async fn a_host_serves_its_models_and_an_unknown_or_unready_one_is_refused() {
    // The far daemon discovers two models from its own agent. The local one
    // has none, so only the far daemon's catalog can answer with them.
    let stub_dir = tempfile::tempdir().unwrap();
    let mut offer = script();
    offer["config_options"] = json!([{
        "id": "model-id", "name": "Model", "category": "model", "type": "select",
        "currentValue": "old-model",
        "options": [
            {"value": "old-model", "name": "The old one"},
            {"value": "new-model", "name": "The new one"},
        ],
    }]);
    let stub = stub_acp_agent(stub_dir.path(), offer);
    let far = harness().home(registry_home(&stub)).discover_agents().await;
    discovery_settled(&far, &stub).await;
    let (local, far, _dir) = pair_with(far).await;

    let answer: Vec<Value> = local.get("/v1/hosts/far/models").await;
    let want: Vec<Value> = far.get("/v1/models").await;
    let ids: Vec<&str> = answer.iter().map(|m| m["id"].as_str().unwrap()).collect();
    assert!(ids.iter().any(|id| id.ends_with(":old-model")), "{ids:?}");
    assert!(ids.iter().any(|id| id.ends_with(":new-model")), "{ids:?}");
    assert_eq!(answer, want);

    let own: Vec<Value> = local.get("/v1/hosts/local/models").await;
    let local_want: Vec<Value> = local.get("/v1/models").await;
    assert_eq!(own, local_want);
    assert_ne!(own, answer);

    local
        .error(get("/v1/hosts/nowhere/models"), StatusCode::NOT_FOUND)
        .await;
    let error = local
        .error(
            get("/v1/hosts/dead/models"),
            StatusCode::SERVICE_UNAVAILABLE,
        )
        .await;
    assert!(
        error.error.message.contains("connecting") || error.error.message.contains("down"),
        "{error:?}"
    );
}
