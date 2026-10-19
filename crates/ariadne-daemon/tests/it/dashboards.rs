//! Integration tests for dashboard CRUD and validation.

use crate::common;
use ariadne_api::dashboards::DashboardDto;
use ariadne_api::stream::DomainEvent;
use ariadne_store::telemetry::{MetricKind, MetricPoint};
use axum::http::StatusCode;
use common::{Harness, delete, get, harness, next_event, post_json, put_json};
use serde_json::{Value, json};

fn document(name: &str) -> serde_json::Value {
    serde_json::json!({"kind":"Dashboard","metadata":{"name":name},"spec":{}})
}

#[tokio::test]
async fn dashboard_routes_keep_the_document_and_publish_fat_events() {
    let h = harness().await;
    let mut rx = h.bus.subscribe();
    let created: DashboardDto = h
        .json(
            post_json(
                "/v1/dashboards",
                serde_json::json!({"name":"mine","document":document("mine")}),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert_eq!(created.name, "mine");
    assert!(!created.builtin);
    assert_eq!(created.document, document("mine"));
    let event = next_event(&mut rx, |e| e.event.kind() == "dashboard_created").await;
    assert!(matches!(event.event, DomainEvent::DashboardCreated(_)));
    let updated: DashboardDto = h
        .json(
            put_json(
                "/v1/dashboards/mine",
                serde_json::json!({"document":document("mine")}),
            ),
            StatusCode::OK,
        )
        .await;
    assert!(!updated.document_is_default);
    assert_eq!(
        h.send(delete("/v1/dashboards/mine")).await.0,
        StatusCode::NO_CONTENT
    );
    let event = next_event(&mut rx, |e| e.event.kind() == "dashboard_deleted").await;
    let DomainEvent::DashboardDeleted(deleted) = event.event else {
        unreachable!("matched on kind above");
    };
    assert_eq!(deleted.name, "mine");
}

#[tokio::test]
async fn dashboard_documents_must_be_named_perses_dashboards() {
    let h = harness().await;
    let err = h.error(post_json("/v1/dashboards", serde_json::json!({"name":"mine","document":{"kind":"NotDashboard","metadata":{"name":"other"},"spec":[]}})), StatusCode::UNPROCESSABLE_ENTITY).await;
    assert_eq!(err.error.code, "invalid_request");
    assert!(err.error.message.contains("kind"));
    assert_eq!(
        h.send(get("/v1/dashboards/default")).await.0,
        StatusCode::OK
    );
    let err = h
        .error(delete("/v1/dashboards/default"), StatusCode::CONFLICT)
        .await;
    assert_eq!(err.error.code, "dashboard_builtin");
}

#[tokio::test]
async fn resetting_a_built_in_dashboard_restores_its_shipped_document() {
    let h = harness().await;
    let override_document = document("default");
    let updated: DashboardDto = h
        .json(
            put_json(
                "/v1/dashboards/default",
                serde_json::json!({"document": override_document}),
            ),
            StatusCode::OK,
        )
        .await;
    assert!(!updated.document_is_default);

    let reset: DashboardDto = h
        .json(
            post_json("/v1/dashboards/default/reset", serde_json::json!({})),
            StatusCode::OK,
        )
        .await;
    assert!(reset.document_is_default);
    assert_ne!(reset.document, document("default"));

    let _: DashboardDto = h
        .json(
            post_json(
                "/v1/dashboards",
                serde_json::json!({"name":"mine","document":document("mine")}),
            ),
            StatusCode::CREATED,
        )
        .await;
    let err = h
        .error(
            post_json("/v1/dashboards/mine/reset", serde_json::json!({})),
            StatusCode::CONFLICT,
        )
        .await;
    assert_eq!(err.error.code, "conflict");
}

/// 2026-02-01T10:00:00Z, in Unix seconds.
const Q: i64 = 1_769_940_000;

fn at(offset_seconds: i64) -> String {
    chrono::DateTime::from_timestamp(Q + offset_seconds, 0)
        .unwrap()
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// Two samples of the same value, 4 and 2 minutes before `Q`: enough for
/// `increase()` over the sweep's 5 minute window to read a real `0`, never
/// an absent series.
fn steady(name: &str, unit: Option<&str>, labels: Value, value: f64) -> Vec<MetricPoint> {
    [-240, -120]
        .into_iter()
        .map(|offset| MetricPoint {
            name: name.into(),
            kind: MetricKind::Counter,
            unit: unit.map(str::to_string),
            description: None,
            labels: labels.clone(),
            timestamp: at(offset),
            value,
        })
        .collect()
}

/// The series behind the shipped dashboard's three 0/0 divisions (Success
/// rate, Cached input, Prompt cache hit ratio) and its one empty-histogram
/// quantile family (Median lead time, and the percentile lines of Lead time
/// per $bucket), held flat rather than absent over the sweep below (036).
async fn seed_zero_increase_series(h: &Harness) {
    let mut points = Vec::new();
    points.extend(steady(
        "ariadne.tasks_ended",
        None,
        json!({"status": "finished", "repo": "ariadne", "model": "claude-sonnet-5"}),
        5.0,
    ));
    points.extend(steady(
        "ariadne.tokens",
        None,
        json!({"kind": "input", "repo": "ariadne", "model": "claude-sonnet-5"}),
        100.0,
    ));
    points.extend(steady(
        "ariadne.tokens",
        None,
        json!({"kind": "cached_input", "repo": "ariadne", "model": "claude-sonnet-5"}),
        80.0,
    ));
    for (le, count) in [("1", 2.0), ("5", 6.0), ("+Inf", 8.0)] {
        points.extend(steady(
            "ariadne.task.lead_time_bucket",
            Some("s"),
            json!({"status": "finished", "repo": "ariadne", "le": le}),
            count,
        ));
        // The daemon's own health histograms (http-latency, scheduler-tick):
        // held flat the same way, so their `rate()` reads a real `0`, never
        // an absent series, over the sweep's window.
        points.extend(steady(
            "http.server.request.duration_bucket",
            Some("s"),
            json!({"le": le}),
            count,
        ));
        points.extend(steady(
            "ariadne.scheduler_tick_duration_bucket",
            Some("s"),
            json!({"le": le}),
            count,
        ));
    }
    h.telemetry_store.write_metrics(&points).await.unwrap();
}

fn uri(path: &str, params: &[(&str, &str)]) -> String {
    format!("{path}?{}", serde_urlencoded::to_string(params).unwrap())
}

async fn instant_query(h: &Harness, query: &str, time: &str) -> Value {
    let body: Value = h
        .json(
            get(&uri("/api/v1/query", &[("query", query), ("time", time)])),
            StatusCode::OK,
        )
        .await;
    assert_eq!(body["status"], "success", "{query}: {body}");
    body["data"].clone()
}

#[tokio::test]
async fn no_panel_query_in_the_shipped_dashboard_gives_nan_over_an_empty_range() {
    let h = harness().await;
    seed_zero_increase_series(&h).await;

    let dashboard: DashboardDto = h.json(get("/v1/dashboards/default"), StatusCode::OK).await;
    let panels = dashboard.document["spec"]["panels"]
        .as_object()
        .expect("the shipped document carries a panels object");
    let time = Q.to_string();

    for (name, panel) in panels {
        for query in panel["spec"]["queries"].as_array().into_iter().flatten() {
            let plugin = &query["spec"]["plugin"];
            // A Loki query carries no PromQL.
            if plugin["kind"] != "PrometheusTimeSeriesQuery" {
                continue;
            }
            let raw = plugin["spec"]["query"].as_str().expect("a query string");
            let promql = raw
                .replace("${repo:regexliteral}", ".*")
                .replace("${model:regexliteral}", ".*")
                .replace("$__range", "5m")
                .replace("$__rate_interval", "5m")
                .replace("$bucket", "5m");
            let data = instant_query(&h, &promql, &time).await;
            assert!(
                !data.to_string().contains("NaN"),
                "{name} gave NaN for {raw:?}: {data}"
            );
        }
    }
}
