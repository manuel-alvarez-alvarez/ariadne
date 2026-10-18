//! OTLP/HTTP log and metric ingestion.

use std::collections::HashMap;
use std::io::Read;
use std::sync::{LazyLock, Mutex};

use axum::body::to_bytes;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use chrono::{SecondsFormat, Utc};
use flate2::read::GzDecoder;
use opentelemetry_proto::tonic::collector::{
    logs::v1 as collector_logs, metrics::v1 as collector_metrics,
};
use opentelemetry_proto::tonic::{common::v1 as common, metrics::v1 as metrics};
use prost::Message;
use serde::Serialize;
use serde_json::{Map, Value};

use ariadne_store::telemetry::{MetricKind, MetricPoint, NewLogRecord};

use super::AppState;

const MAX_BODY: usize = 8 * 1024 * 1024;
const PROTOBUF: &str = "application/x-protobuf";
const JSON: &str = "application/json";
static DELTA_SUMS: LazyLock<Mutex<HashMap<String, f64>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

enum WireFormat {
    Protobuf,
    Json,
}

fn wire_format(headers: &HeaderMap) -> Result<WireFormat, StatusCode> {
    match headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.split(';').next().unwrap_or("").trim())
    {
        Some(PROTOBUF) => Ok(WireFormat::Protobuf),
        Some(JSON) => Ok(WireFormat::Json),
        _ => Err(StatusCode::UNSUPPORTED_MEDIA_TYPE),
    }
}

async fn request_body(request: Request) -> Result<(WireFormat, Vec<u8>), StatusCode> {
    let (parts, body) = request.into_parts();
    let format = wire_format(&parts.headers)?;
    if parts
        .headers
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<usize>().ok())
        .is_some_and(|n| n > MAX_BODY)
    {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }
    let bytes = to_bytes(body, MAX_BODY)
        .await
        .map_err(|_| StatusCode::PAYLOAD_TOO_LARGE)?;
    let body = match parts
        .headers
        .get(header::CONTENT_ENCODING)
        .and_then(|v| v.to_str().ok())
    {
        None | Some("") | Some("identity") => bytes.to_vec(),
        Some("gzip") => {
            let mut decoded = Vec::new();
            GzDecoder::new(bytes.as_ref())
                .take((MAX_BODY + 1) as u64)
                .read_to_end(&mut decoded)
                .map_err(|_| StatusCode::BAD_REQUEST)?;
            if decoded.len() > MAX_BODY {
                return Err(StatusCode::PAYLOAD_TOO_LARGE);
            }
            decoded
        }
        _ => return Err(StatusCode::UNSUPPORTED_MEDIA_TYPE),
    };
    Ok((format, body))
}

fn decode<T: Message + serde::de::DeserializeOwned + Default>(
    format: &WireFormat,
    body: &[u8],
) -> Result<T, StatusCode> {
    match format {
        WireFormat::Protobuf => T::decode(body).map_err(|_| StatusCode::BAD_REQUEST),
        WireFormat::Json => serde_json::from_slice(body).map_err(|_| StatusCode::BAD_REQUEST),
    }
}

fn response<T: Message + Serialize>(format: WireFormat, value: &T) -> Response {
    match format {
        WireFormat::Protobuf => {
            ([(header::CONTENT_TYPE, PROTOBUF)], value.encode_to_vec()).into_response()
        }
        WireFormat::Json => (
            [(header::CONTENT_TYPE, JSON)],
            serde_json::to_vec(value).expect("OTLP response serializes"),
        )
            .into_response(),
    }
}

fn timestamp(nanos: u64) -> String {
    chrono::DateTime::<Utc>::from_timestamp_nanos(nanos as i64)
        .to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn value(any: Option<&common::AnyValue>) -> Value {
    use common::any_value::Value as Otlp;
    match any.and_then(|v| v.value.as_ref()) {
        Some(Otlp::StringValue(v)) => Value::String(v.clone()),
        Some(Otlp::BoolValue(v)) => Value::Bool(*v),
        Some(Otlp::IntValue(v)) => Value::Number((*v).into()),
        Some(Otlp::DoubleValue(v)) => serde_json::Number::from_f64(*v)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        Some(Otlp::BytesValue(v)) => Value::String(hex::encode(v)),
        Some(Otlp::ArrayValue(v)) => {
            Value::Array(v.values.iter().map(|v| value(Some(v))).collect())
        }
        Some(Otlp::KvlistValue(v)) => attributes(&v.values),
        Some(Otlp::StringValueStrindex(v)) => Value::Number((*v).into()),
        None => Value::Null,
    }
}

fn body_text(any: &common::AnyValue) -> String {
    match value(Some(any)) {
        Value::String(value) => value,
        value => value.to_string(),
    }
}

fn attributes(values: &[common::KeyValue]) -> Value {
    Value::Object(
        values
            .iter()
            .map(|v| (v.key.clone(), value(v.value.as_ref())))
            .collect::<Map<_, _>>(),
    )
}

fn metric_labels(
    resource: Option<&opentelemetry_proto::tonic::resource::v1::Resource>,
    values: &[common::KeyValue],
    service: &str,
) -> Value {
    let mut labels = resource
        .map(|resource| attributes(&resource.attributes))
        .unwrap_or_else(|| Value::Object(Map::new()));
    let object = labels
        .as_object_mut()
        .expect("resource attributes are an object");
    object.extend(
        attributes(values)
            .as_object()
            .expect("point attributes are an object")
            .clone(),
    );
    object.insert("service".to_string(), Value::String(service.to_string()));
    labels
}

fn service(resource: Option<&opentelemetry_proto::tonic::resource::v1::Resource>) -> String {
    resource
        .and_then(|r| r.attributes.iter().find(|a| a.key == "service.name"))
        .and_then(|a| value(a.value.as_ref()).as_str().map(str::to_string))
        .unwrap_or_else(|| "unknown".to_string())
}

fn id(bytes: &[u8], expected: usize) -> Option<String> {
    (bytes.len() == expected).then(|| hex::encode(bytes))
}

#[utoipa::path(post, path = "/otlp/v1/logs", tag = "telemetry",
    request_body = String, responses((status = 200, description = "OTLP ExportLogsServiceResponse. Accepts application/x-protobuf or application/json."), (status = 400), (status = 413), (status = 415)))]
pub(super) async fn logs(State(state): State<AppState>, request: Request) -> Response {
    let (format, body) = match request_body(request).await {
        Ok(value) => value,
        Err(status) => return status.into_response(),
    };
    let request: collector_logs::ExportLogsServiceRequest = match decode(&format, &body) {
        Ok(value) => value,
        Err(status) => return status.into_response(),
    };
    let mut records = Vec::new();
    for resource_logs in request.resource_logs {
        let service = service(resource_logs.resource.as_ref());
        for scope_logs in resource_logs.scope_logs {
            let scope = scope_logs
                .scope
                .as_ref()
                .map(|scope| scope.name.clone())
                .filter(|name| !name.is_empty());
            for record in scope_logs.log_records {
                records.push(NewLogRecord {
                    ts: timestamp(record.time_unix_nano),
                    observed_ts: timestamp(if record.observed_time_unix_nano == 0 {
                        record.time_unix_nano
                    } else {
                        record.observed_time_unix_nano
                    }),
                    severity_number: (record.severity_number != 0)
                        .then_some(record.severity_number as i64),
                    severity_text: (!record.severity_text.is_empty())
                        .then_some(record.severity_text),
                    body: record.body.as_ref().map(body_text),
                    service: Some(service.clone()),
                    scope: scope.clone(),
                    target: None,
                    attributes: attributes(&record.attributes),
                    trace_id: id(&record.trace_id, 16),
                    span_id: id(&record.span_id, 8),
                });
            }
        }
    }
    let services: Vec<String> = records
        .iter()
        .filter_map(|record| record.service.clone())
        .collect();
    if let Err(error) = state.telemetry_store.write_logs(&records).await {
        return super::error::ApiError::from(error).into_response();
    }
    crate::telemetry::record_ingested_count("log", &services);
    response(
        format,
        &collector_logs::ExportLogsServiceResponse {
            partial_success: None,
        },
    )
}

fn number(value: &Option<metrics::number_data_point::Value>) -> Option<f64> {
    match value {
        Some(metrics::number_data_point::Value::AsDouble(v)) => Some(*v),
        Some(metrics::number_data_point::Value::AsInt(v)) => Some(*v as f64),
        None => None,
    }
}
fn metadata(metric: &metrics::Metric) -> (Option<String>, Option<String>) {
    (
        (!metric.unit.is_empty()).then(|| metric.unit.clone()),
        (!metric.description.is_empty()).then(|| metric.description.clone()),
    )
}
fn metric_point(
    name: String,
    kind: MetricKind,
    unit: &Option<String>,
    description: &Option<String>,
    labels: Value,
    nanos: u64,
    value: f64,
) -> MetricPoint {
    MetricPoint {
        name,
        kind,
        unit: unit.clone(),
        description: description.clone(),
        labels,
        timestamp: timestamp(nanos),
        value,
    }
}
fn accumulated_delta(name: &str, labels: &Value, value: f64) -> f64 {
    let key = format!("{name}:{}", labels);
    let mut sums = DELTA_SUMS.lock().unwrap_or_else(|error| error.into_inner());
    let total = sums.entry(key).or_default();
    *total += value;
    *total
}

fn add_histogram(
    points: &mut Vec<MetricPoint>,
    metric: &metrics::Metric,
    point: &metrics::HistogramDataPoint,
    unit: &Option<String>,
    description: &Option<String>,
    labels: Value,
) {
    let mut cumulative = 0_u64;
    for (index, count) in point.bucket_counts.iter().enumerate() {
        cumulative += count;
        let mut labels = labels.clone();
        labels.as_object_mut().expect("attribute object").insert(
            "le".into(),
            Value::String(
                point
                    .explicit_bounds
                    .get(index)
                    .map(ToString::to_string)
                    .unwrap_or_else(|| "+Inf".into()),
            ),
        );
        points.push(metric_point(
            format!("{}_bucket", metric.name),
            MetricKind::Histogram,
            unit,
            description,
            labels,
            point.time_unix_nano,
            cumulative as f64,
        ));
    }
    if let Some(sum) = point.sum {
        points.push(metric_point(
            format!("{}_sum", metric.name),
            MetricKind::Histogram,
            unit,
            description,
            labels.clone(),
            point.time_unix_nano,
            sum,
        ));
    }
    points.push(metric_point(
        format!("{}_count", metric.name),
        MetricKind::Histogram,
        unit,
        description,
        labels,
        point.time_unix_nano,
        point.count as f64,
    ));
}

fn add_summary(
    points: &mut Vec<MetricPoint>,
    metric: &metrics::Metric,
    point: &metrics::SummaryDataPoint,
    unit: &Option<String>,
    description: &Option<String>,
    labels: Value,
) {
    points.push(metric_point(
        format!("{}_sum", metric.name),
        MetricKind::Histogram,
        unit,
        description,
        labels.clone(),
        point.time_unix_nano,
        point.sum,
    ));
    points.push(metric_point(
        format!("{}_count", metric.name),
        MetricKind::Histogram,
        unit,
        description,
        labels.clone(),
        point.time_unix_nano,
        point.count as f64,
    ));
    for quantile in &point.quantile_values {
        let mut labels = labels.clone();
        labels.as_object_mut().expect("attribute object").insert(
            "quantile".into(),
            Value::String(quantile.quantile.to_string()),
        );
        points.push(metric_point(
            metric.name.clone(),
            MetricKind::Histogram,
            unit,
            description,
            labels,
            point.time_unix_nano,
            quantile.value,
        ));
    }
}

#[utoipa::path(post, path = "/otlp/v1/metrics", tag = "telemetry",
    request_body = String, responses((status = 200, description = "OTLP ExportMetricsServiceResponse. Accepts application/x-protobuf or application/json."), (status = 400), (status = 413), (status = 415)))]
pub(super) async fn metrics(State(state): State<AppState>, request: Request) -> Response {
    let (format, body) = match request_body(request).await {
        Ok(value) => value,
        Err(status) => return status.into_response(),
    };
    let request: collector_metrics::ExportMetricsServiceRequest = match decode(&format, &body) {
        Ok(value) => value,
        Err(status) => return status.into_response(),
    };
    let mut points = Vec::new();
    let mut services = Vec::new();
    let mut rejected = 0_i64;
    for resource_metrics in request.resource_metrics {
        let service = service(resource_metrics.resource.as_ref());
        for scope_metrics in resource_metrics.scope_metrics {
            for metric in scope_metrics.metrics {
                let before = points.len();
                let (unit, description) = metadata(&metric);
                match metric.data.as_ref() {
                    Some(metrics::metric::Data::Gauge(gauge)) => {
                        for point in &gauge.data_points {
                            if let Some(value) = number(&point.value) {
                                points.push(metric_point(
                                    metric.name.clone(),
                                    MetricKind::Gauge,
                                    &unit,
                                    &description,
                                    metric_labels(
                                        resource_metrics.resource.as_ref(),
                                        &point.attributes,
                                        &service,
                                    ),
                                    point.time_unix_nano,
                                    value,
                                ));
                            }
                        }
                    }
                    Some(metrics::metric::Data::Sum(sum)) => {
                        for point in &sum.data_points {
                            if let Some(value) = number(&point.value) {
                                let labels = metric_labels(
                                    resource_metrics.resource.as_ref(),
                                    &point.attributes,
                                    &service,
                                );
                                let value = if sum.aggregation_temporality
                                    == metrics::AggregationTemporality::Delta as i32
                                {
                                    accumulated_delta(&metric.name, &labels, value)
                                } else {
                                    value
                                };
                                points.push(metric_point(
                                    metric.name.clone(),
                                    MetricKind::Counter,
                                    &unit,
                                    &description,
                                    labels,
                                    point.time_unix_nano,
                                    value,
                                ));
                            }
                        }
                    }
                    Some(metrics::metric::Data::Histogram(histogram)) => {
                        for point in &histogram.data_points {
                            add_histogram(
                                &mut points,
                                &metric,
                                point,
                                &unit,
                                &description,
                                metric_labels(
                                    resource_metrics.resource.as_ref(),
                                    &point.attributes,
                                    &service,
                                ),
                            );
                        }
                    }
                    Some(metrics::metric::Data::Summary(summary)) => {
                        for point in &summary.data_points {
                            add_summary(
                                &mut points,
                                &metric,
                                point,
                                &unit,
                                &description,
                                metric_labels(
                                    resource_metrics.resource.as_ref(),
                                    &point.attributes,
                                    &service,
                                ),
                            );
                        }
                    }
                    Some(metrics::metric::Data::ExponentialHistogram(histogram)) => {
                        rejected += histogram.data_points.len() as i64
                    }
                    None => {}
                }
                services.extend(std::iter::repeat_n(service.clone(), points.len() - before));
            }
        }
    }
    if let Err(error) = state.telemetry_store.write_metrics(&points).await {
        return super::error::ApiError::from(error).into_response();
    }
    crate::telemetry::record_ingested_count("metric", &services);
    let partial_success = (rejected > 0).then(|| collector_metrics::ExportMetricsPartialSuccess {
        rejected_data_points: rejected,
        error_message: "exponential histograms are not supported".to_string(),
    });
    response(
        format,
        &collector_metrics::ExportMetricsServiceResponse { partial_success },
    )
}
