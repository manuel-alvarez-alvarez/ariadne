//! Integration tests for the Prometheus and Loki query routes (034): points
//! and records written straight to the telemetry store, read back through
//! the HTTP API in each protocol's own shape.

use crate::common;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use serde_json::{Value, json};

use ariadne_daemon::http;
use ariadne_store::telemetry::{MetricKind, MetricPoint, NewLogRecord};

use common::{Harness, get, harness};

/// 2026-01-01T10:00:00Z, in Unix seconds.
const T: i64 = 1_767_261_600;

fn at(offset_seconds: i64) -> String {
    chrono::DateTime::from_timestamp(T + offset_seconds, 0)
        .unwrap()
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn point(
    name: &str,
    kind: MetricKind,
    unit: Option<&str>,
    labels: Value,
    t: i64,
    v: f64,
) -> MetricPoint {
    MetricPoint {
        name: name.into(),
        kind,
        unit: unit.map(str::to_string),
        description: Some(format!("{name} help")),
        labels,
        timestamp: at(t),
        value: v,
    }
}

async fn seed_metrics(h: &Harness) {
    let mut points = Vec::new();
    for (i, total) in [10.0, 20.0, 30.0].into_iter().enumerate() {
        let t = -120 + 60 * i as i64;
        points.push(point(
            "ariadne.tokens",
            MetricKind::Counter,
            Some("{token}"),
            json!({"seat": "author"}),
            t,
            total,
        ));
        points.push(point(
            "ariadne.tokens",
            MetricKind::Counter,
            Some("{token}"),
            json!({"seat": "reviewer"}),
            t,
            total / 10.0,
        ));
    }
    for (le, count) in [("1", 2.0), ("2", 6.0), ("+Inf", 8.0)] {
        points.push(point(
            "ariadne.task.lead_time_bucket",
            MetricKind::Counter,
            Some("s"),
            json!({"le": le}),
            0,
            count,
        ));
    }
    points.push(point(
        "ariadne.task.lead_time_sum",
        MetricKind::Counter,
        Some("s"),
        json!({}),
        0,
        11.0,
    ));
    points.push(point(
        "ariadne.task.lead_time_count",
        MetricKind::Counter,
        Some("s"),
        json!({}),
        0,
        8.0,
    ));
    h.telemetry_store.write_metrics(&points).await.unwrap();
}

async fn seed_logs(h: &Harness) {
    let record = |t: i64, level: &str, body: &str, target: &str| NewLogRecord {
        ts: at(t),
        observed_ts: at(t),
        severity_number: None,
        severity_text: Some(level.into()),
        body: Some(body.into()),
        service: Some("ariadned".into()),
        scope: Some("ariadne_daemon".into()),
        target: Some(target.into()),
        attributes: json!({}),
        trace_id: None,
        span_id: None,
    };
    h.telemetry_store
        .write_logs(&[
            record(-90, "INFO", "task started", "scheduler"),
            record(-60, "ERROR", "task 42 failed", "scheduler"),
            record(-30, "ERROR", "push failed", "git"),
        ])
        .await
        .unwrap();
}

async fn ok(h: &Harness, request: Request<Body>) -> Value {
    let body: Value = h.json(request, StatusCode::OK).await;
    assert_eq!(body["status"], "success", "{body}");
    body["data"].clone()
}

fn uri(path: &str, params: &[(&str, &str)]) -> String {
    format!("{path}?{}", serde_urlencoded::to_string(params).unwrap())
}

fn form(path: &str, params: &[(&str, &str)]) -> Request<Body> {
    Request::builder()
        .method(Method::POST)
        .uri(path)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(serde_urlencoded::to_string(params).unwrap()))
        .unwrap()
}

#[tokio::test]
async fn an_instant_query_answers_a_vector_under_prometheus_names() {
    let h = harness().await;
    seed_metrics(&h).await;
    let time = T.to_string();
    let data = ok(
        &h,
        get(&uri(
            "/api/v1/query",
            &[
                ("query", "sum by (seat) (ariadne_tokens_total)"),
                ("time", &time),
            ],
        )),
    )
    .await;
    assert_eq!(
        data,
        json!({"resultType": "vector", "result": [
            {"metric": {"seat": "author"}, "value": [T as f64, "30"]},
            {"metric": {"seat": "reviewer"}, "value": [T as f64, "3"]},
        ]})
    );

    let raw = ok(
        &h,
        get(&uri(
            "/api/v1/query",
            &[
                ("query", r#"ariadne_tokens_total{seat="author"}"#),
                ("time", &time),
            ],
        )),
    )
    .await;
    assert_eq!(
        raw["result"][0]["metric"],
        json!({"__name__": "ariadne_tokens_total", "seat": "author", "service": "ariadned", "job": "ariadned"})
    );

    let p50 = ok(
        &h,
        get(&uri(
            "/api/v1/query",
            &[
                (
                    "query",
                    "histogram_quantile(0.5, ariadne_task_lead_time_seconds_bucket)",
                ),
                ("time", &time),
            ],
        )),
    )
    .await;
    assert_eq!(p50["result"][0]["value"][1], "1.5");
}

#[tokio::test]
async fn a_form_posted_range_query_answers_a_matrix_per_step() {
    let h = harness().await;
    seed_metrics(&h).await;
    let (start, end) = ((T - 120).to_string(), T.to_string());
    let data = ok(
        &h,
        form(
            "/api/v1/query_range",
            &[
                ("query", r#"ariadne_tokens_total{seat="author"} * 2"#),
                ("start", &start),
                ("end", &end),
                ("step", "1m"),
            ],
        ),
    )
    .await;
    assert_eq!(data["resultType"], "matrix");
    assert_eq!(
        data["result"][0]["values"],
        json!([
            [(T - 120) as f64, "20"],
            [(T - 60) as f64, "40"],
            [T as f64, "60"]
        ])
    );
    assert!(data["result"][0]["metric"].get("__name__").is_none());
}

#[tokio::test]
async fn an_unsupported_query_answers_bad_data_naming_it() {
    let h = harness().await;
    let (status, body) = h
        .send(get(&uri(
            "/api/v1/query",
            &[("query", "stddev(ariadne_tokens_total)")],
        )))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let body: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["status"], "error");
    assert_eq!(body["errorType"], "bad_data");
    assert!(body["error"].as_str().unwrap().contains("stddev"), "{body}");
}

#[tokio::test]
async fn the_parse_query_route_answers_an_ast_or_bad_data() {
    let h = harness().await;
    let data = ok(
        &h,
        form(
            "/api/v1/parse_query",
            &[("query", r#"ariadne_tokens_total{seat="author"}"#)],
        ),
    )
    .await;
    assert_eq!(
        data,
        json!({
            "type": "vectorSelector",
            "name": "ariadne_tokens_total",
            "matchers": [{"name": "seat", "type": "=", "value": "author"}],
            "offset": 0,
            "startOrEnd": null,
            "timestamp": null,
        })
    );

    let (status, body) = h
        .send(get(&uri("/api/v1/parse_query", &[("query", "sum(")])))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let body: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["status"], "error");
    assert_eq!(body["errorType"], "bad_data");
}

#[tokio::test]
async fn discovery_routes_list_labels_series_and_metadata() {
    let h = harness().await;
    seed_metrics(&h).await;
    let labels = ok(
        &h,
        form("/api/v1/labels", &[("match[]", "ariadne_tokens_total")]),
    )
    .await;
    assert_eq!(labels, json!(["__name__", "job", "seat", "service"]));
    let seats = ok(&h, get("/api/v1/label/seat/values")).await;
    assert_eq!(seats, json!(["author", "reviewer"]));
    let series = ok(
        &h,
        get(&uri(
            "/api/v1/series",
            &[("match[]", r#"{__name__=~"ariadne_task.*_count"}"#)],
        )),
    )
    .await;
    assert_eq!(
        series[0]["__name__"],
        "ariadne_task_lead_time_seconds_count"
    );
    let metadata = ok(&h, get("/api/v1/metadata")).await;
    assert_eq!(metadata["ariadne_tokens_total"][0]["type"], "counter");
    assert_eq!(
        metadata["ariadne_task_lead_time_seconds"][0]["type"],
        "histogram"
    );
    let (status, body) = h.send(get("/-/healthy")).await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
}

#[tokio::test]
async fn loki_streams_carry_nanosecond_times_and_filtered_lines() {
    let h = harness().await;
    seed_logs(&h).await;
    let (start, end) = ((T - 3600).to_string(), T.to_string());
    let data = ok(
        &h,
        get(&uri(
            "/loki/api/v1/query_range",
            &[
                (
                    "query",
                    r#"{service="ariadned", level="error"} |= "failed" != "push""#,
                ),
                ("start", &start),
                ("end", &end),
            ],
        )),
    )
    .await;
    assert_eq!(data["resultType"], "streams");
    assert_eq!(
        data["result"],
        json!([{
            "stream": {"service": "ariadned", "level": "error", "scope": "ariadne_daemon", "target": "scheduler"},
            "values": [[format!("{}000000000", T - 60), "task 42 failed"]],
        }])
    );

    let counts = ok(
        &h,
        get(&uri(
            "/loki/api/v1/query_range",
            &[
                (
                    "query",
                    r#"sum by (level) (count_over_time({service="ariadned"}[1h]))"#,
                ),
                ("start", &end),
                ("end", &end),
                ("step", "60"),
            ],
        )),
    )
    .await;
    assert_eq!(
        counts["result"],
        json!([
            {"metric": {"level": "error"}, "values": [[T as f64, "2"]]},
            {"metric": {"level": "info"}, "values": [[T as f64, "1"]]},
        ])
    );

    let levels = ok(&h, get("/loki/api/v1/label/level/values")).await;
    assert_eq!(levels, json!(["error", "info"]));
    let names = ok(&h, get("/loki/api/v1/labels")).await;
    assert_eq!(names, json!(["level", "scope", "service", "target"]));
    let series = ok(
        &h,
        get(&uri(
            "/loki/api/v1/series",
            &[("match[]", r#"{target="git"}"#)],
        )),
    )
    .await;
    assert_eq!(series.as_array().unwrap().len(), 1);
    let stats: Value = h.get("/loki/api/v1/index/stats").await;
    assert_eq!(stats["streams"], 0);
    let volume = ok(&h, get("/loki/api/v1/index/volume")).await;
    assert_eq!(volume["result"], json!([]));

    let (status, body) = h
        .send(get(&uri(
            "/loki/api/v1/query_range",
            &[("query", r#"{service="x"} | json"#)],
        )))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(String::from_utf8_lossy(&body).contains("json"));
}

#[tokio::test]
async fn the_tcp_router_serves_the_query_routes_with_cors() {
    let h = harness().await;
    let app = http::tcp_router(h.state.clone(), &["http://perses.localhost:8080".into()]);
    let request = Request::builder()
        .method(Method::POST)
        .uri("/api/v1/query")
        .header(header::HOST, "127.0.0.1:7676")
        .header(header::ORIGIN, "http://perses.localhost:8080")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from("query=vector(1)"))
        .unwrap();
    let response = tower::ServiceExt::oneshot(app, request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .unwrap(),
        "http://perses.localhost:8080"
    );
}

#[tokio::test]
async fn discovery_bounds_list_only_series_with_a_sample_between_them() {
    let h = harness().await;
    seed_metrics(&h).await;
    let (after, later) = ((T + 100).to_string(), (T + 200).to_string());
    let outside = [("start", after.as_str()), ("end", later.as_str())];
    let series = ok(
        &h,
        get(&uri(
            "/api/v1/series",
            &[[("match[]", "ariadne_tokens_total")].as_slice(), &outside].concat(),
        )),
    )
    .await;
    assert_eq!(series, json!([]), "no sample after the series' last one");
    let labels = ok(&h, form("/api/v1/labels", &outside)).await;
    assert_eq!(labels, json!([]));
    let seats = ok(&h, get(&uri("/api/v1/label/seat/values", &outside))).await;
    assert_eq!(seats, json!([]));

    // Only the tokens series have a sample two minutes back; the histogram's
    // only sample is at T.
    let (from, to) = ((T - 130).to_string(), (T - 110).to_string());
    let inside = [("start", from.as_str()), ("end", to.as_str())];
    let names = ok(&h, get(&uri("/api/v1/label/__name__/values", &inside))).await;
    assert_eq!(names, json!(["ariadne_tokens_total"]));
    let seats = ok(&h, get(&uri("/api/v1/label/seat/values", &inside))).await;
    assert_eq!(seats, json!(["author", "reviewer"]));
    let only_start = ok(
        &h,
        get(&uri("/api/v1/label/__name__/values", &[("start", &after)])),
    )
    .await;
    assert_eq!(only_start, json!([]));
}

#[tokio::test]
async fn loki_bounds_keep_their_nanoseconds() {
    let h = harness().await;
    seed_logs(&h).await;
    // The line "task 42 failed" is stored at a whole millisecond, T - 60s.
    let at = (T - 60) as i128 * 1_000_000_000;
    let query = r#"{target="scheduler", level="error"}"#;
    let lines = |data: &Value| {
        data["result"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|s| s["values"].as_array().unwrap().clone())
            .map(|v| v[1].as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    };
    let (just_after, before) = ((at + 1).to_string(), (at - 1_000_000_000).to_string());
    let started_after = ok(
        &h,
        get(&uri(
            "/loki/api/v1/query_range",
            &[
                ("query", query),
                ("start", &just_after),
                ("end", &T.to_string()),
            ],
        )),
    )
    .await;
    assert!(
        lines(&started_after).is_empty(),
        "a start 1ns after the line leaves it out"
    );
    let ended_after = ok(
        &h,
        get(&uri(
            "/loki/api/v1/query_range",
            &[("query", query), ("start", &before), ("end", &just_after)],
        )),
    )
    .await;
    assert_eq!(
        lines(&ended_after),
        vec!["task 42 failed"],
        "an end 1ns after keeps it"
    );
    let ended_at = ok(
        &h,
        get(&uri(
            "/loki/api/v1/query_range",
            &[
                ("query", query),
                ("start", &before),
                ("end", &at.to_string()),
            ],
        )),
    )
    .await;
    assert!(lines(&ended_at).is_empty(), "the end is exclusive");
}
