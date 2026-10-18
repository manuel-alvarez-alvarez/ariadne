//! OTLP/HTTP ingestion reaches the telemetry store.

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use serde_json::json;

use ariadne_store::telemetry::{LogFilter, TimeRange};

use crate::common::harness;

fn otlp(path: &str, body: serde_json::Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

#[tokio::test]
async fn otlp_logs_and_metrics_are_written_under_the_resource_service() {
    let h = harness().await;
    let logs = json!({"resourceLogs":[{"resource":{"attributes":[{"key":"service.name","value":{"stringValue":"client"}}]},"scopeLogs":[{"scope":{"name":"client-scope"},"logRecords":[{"timeUnixNano":"1760000000000000000","severityNumber":9,"severityText":"INFO","body":{"stringValue":"received"},"attributes":[{"key":"request.id","value":{"stringValue":"abc"}}],"traceId":"00112233445566778899aabbccddeeff","spanId":"0011223344556677"}]}]}]});
    let (status, _) = h.send(otlp("/otlp/v1/logs", logs)).await;
    assert_eq!(status, StatusCode::OK);
    let written = h
        .telemetry_store
        .read_logs(LogFilter {
            service: Some("client".to_string()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(written.len(), 1);
    assert_eq!(written[0].body.as_deref(), Some("received"));
    assert_eq!(written[0].scope.as_deref(), Some("client-scope"));
    assert_eq!(
        written[0].trace_id.as_deref(),
        Some("00112233445566778899aabbccddeeff")
    );

    let metrics = json!({"resourceMetrics":[{"resource":{"attributes":[{"key":"service.name","value":{"stringValue":"client"}}]},"scopeMetrics":[{"metrics":[{"name":"client.temperature","gauge":{"dataPoints":[{"timeUnixNano":"1760000000000000000","asDouble":12.5,"attributes":[{"key":"room","value":{"stringValue":"lab"}}]}]}}]}]}]});
    let (status, _) = h.send(otlp("/otlp/v1/metrics", metrics)).await;
    assert_eq!(status, StatusCode::OK);
    let series = h.telemetry_store.list_series(&[]).await.unwrap();
    let metric = series
        .iter()
        .find(|series| series.name == "client.temperature")
        .unwrap();
    assert_eq!(metric.labels["service"], "client");
    let samples = h
        .telemetry_store
        .read_samples(
            &metric.id,
            TimeRange {
                since: "2025-01-01T00:00:00Z".to_string(),
                until: "2027-01-01T00:00:00Z".to_string(),
            },
        )
        .await
        .unwrap();
    assert_eq!(samples[0].value, 12.5);
}

#[tokio::test]
async fn otlp_refuses_bad_media_and_documents_both_routes() {
    let h = harness().await;
    let request = Request::builder()
        .method("POST")
        .uri("/otlp/v1/logs")
        .header(header::CONTENT_TYPE, "text/plain")
        .body(Body::from("no"))
        .unwrap();
    let (status, _) = h.send(request).await;
    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    let doc: serde_json::Value = h.get("/api-docs/openapi.json").await;
    assert!(doc["paths"]["/otlp/v1/logs"]["post"].is_object());
    assert!(doc["paths"]["/otlp/v1/metrics"]["post"].is_object());
}
