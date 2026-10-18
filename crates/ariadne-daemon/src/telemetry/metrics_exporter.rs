//! The custom [`PushMetricExporter`] that writes every point the
//! `PeriodicReader` collects into the telemetry store, decomposing a
//! histogram into the Prometheus `_bucket`/`_sum`/`_count` series the
//! store expects (`ariadne_store::telemetry`'s own doc comment), and
//! counts the batch on `ariadne.telemetry.records`.

use std::time::{Duration, SystemTime};

use chrono::{DateTime, SecondsFormat, Utc};
use opentelemetry::Key;
use opentelemetry_sdk::error::OTelSdkResult;
use opentelemetry_sdk::metrics::Temporality;
use opentelemetry_sdk::metrics::data::{AggregatedMetrics, MetricData, ResourceMetrics};
use opentelemetry_sdk::metrics::exporter::PushMetricExporter;

use ariadne_store::telemetry::{MetricKind, MetricPoint, TelemetryStore};

use super::convert::metric_labels_to_json;

#[derive(Debug)]
pub(super) struct StoreMetricExporter {
    store: TelemetryStore,
}

impl StoreMetricExporter {
    pub(super) fn new(store: TelemetryStore) -> Self {
        Self { store }
    }
}

fn system_time_to_rfc3339(time: SystemTime) -> String {
    DateTime::<Utc>::from(time).to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// A metric data point's numeric value, whichever of `f64`/`u64`/`i64` the
/// instrument records it as.
trait AsF64: Copy {
    fn as_f64(self) -> f64;
}
impl AsF64 for f64 {
    fn as_f64(self) -> f64 {
        self
    }
}
impl AsF64 for u64 {
    fn as_f64(self) -> f64 {
        self as f64
    }
}
impl AsF64 for i64 {
    fn as_f64(self) -> f64 {
        self as f64
    }
}

/// Put the resource's service name on a point's labels under
/// [`super::RESOURCE_SERVICE_LABEL`], after its attributes, so the resource
/// always wins. An attribute that already uses that key moves to an
/// `exported_` key that is free, so no attribute value is lost.
fn stamp_service(labels: &mut serde_json::Value, service: &str) {
    let Some(object) = labels.as_object_mut() else {
        return;
    };
    if let Some(attribute) = object.remove(super::RESOURCE_SERVICE_LABEL) {
        let moved = super::exported_key(super::RESOURCE_SERVICE_LABEL, |key| {
            object.contains_key(key)
        });
        object.insert(moved, attribute);
    }
    object.insert(
        super::RESOURCE_SERVICE_LABEL.to_string(),
        serde_json::Value::String(service.to_string()),
    );
}

fn non_empty(s: &str) -> Option<String> {
    (!s.is_empty()).then(|| s.to_string())
}

/// Expand one instrument's aggregated data into the flat points the store
/// writes: one per gauge/sum data point, three per histogram bucket plus
/// its `_sum` and `_count`.
fn collect_points<T: AsF64>(
    name: &str,
    description: &str,
    unit: &str,
    data: &MetricData<T>,
    out: &mut Vec<MetricPoint>,
) {
    let unit = non_empty(unit);
    let description = non_empty(description);
    match data {
        MetricData::Gauge(gauge) => {
            let ts = system_time_to_rfc3339(gauge.time());
            for point in gauge.data_points() {
                out.push(MetricPoint {
                    name: name.to_string(),
                    kind: MetricKind::Gauge,
                    unit: unit.clone(),
                    description: description.clone(),
                    labels: metric_labels_to_json(point.attributes()),
                    timestamp: ts.clone(),
                    value: point.value().as_f64(),
                });
            }
        }
        MetricData::Sum(sum) => {
            let ts = system_time_to_rfc3339(sum.time());
            for point in sum.data_points() {
                out.push(MetricPoint {
                    name: name.to_string(),
                    kind: MetricKind::Counter,
                    unit: unit.clone(),
                    description: description.clone(),
                    labels: metric_labels_to_json(point.attributes()),
                    timestamp: ts.clone(),
                    value: point.value().as_f64(),
                });
            }
        }
        MetricData::Histogram(histogram) => {
            let ts = system_time_to_rfc3339(histogram.time());
            for point in histogram.data_points() {
                let labels = metric_labels_to_json(point.attributes());
                let bounds: Vec<f64> = point.bounds().collect();
                let mut cumulative = 0u64;
                for (i, bucket_count) in point.bucket_counts().enumerate() {
                    cumulative += bucket_count;
                    let le = bounds
                        .get(i)
                        .map(|bound| bound.to_string())
                        .unwrap_or_else(|| "+Inf".to_string());
                    let mut bucket_labels = labels.clone();
                    if let Some(object) = bucket_labels.as_object_mut() {
                        object.insert("le".to_string(), serde_json::Value::String(le));
                    }
                    out.push(MetricPoint {
                        name: format!("{name}_bucket"),
                        kind: MetricKind::Counter,
                        unit: unit.clone(),
                        description: description.clone(),
                        labels: bucket_labels,
                        timestamp: ts.clone(),
                        value: cumulative as f64,
                    });
                }
                out.push(MetricPoint {
                    name: format!("{name}_sum"),
                    kind: MetricKind::Counter,
                    unit: unit.clone(),
                    description: description.clone(),
                    labels: labels.clone(),
                    timestamp: ts.clone(),
                    value: point.sum().as_f64(),
                });
                out.push(MetricPoint {
                    name: format!("{name}_count"),
                    kind: MetricKind::Counter,
                    unit: unit.clone(),
                    description: description.clone(),
                    labels,
                    timestamp: ts.clone(),
                    value: point.count() as f64,
                });
            }
        }
        // Not instrumented anywhere in this build; nothing to decompose.
        MetricData::ExponentialHistogram(_) => {}
    }
}

impl PushMetricExporter for StoreMetricExporter {
    async fn export(&self, metrics: &ResourceMetrics) -> OTelSdkResult {
        let service = metrics
            .resource()
            .get(&Key::new("service.name"))
            .map(|v| v.as_str().into_owned());

        let mut points = Vec::new();
        for scope_metrics in metrics.scope_metrics() {
            for metric in scope_metrics.metrics() {
                match metric.data() {
                    AggregatedMetrics::F64(data) => collect_points(
                        metric.name(),
                        metric.description(),
                        metric.unit(),
                        data,
                        &mut points,
                    ),
                    AggregatedMetrics::U64(data) => collect_points(
                        metric.name(),
                        metric.description(),
                        metric.unit(),
                        data,
                        &mut points,
                    ),
                    AggregatedMetrics::I64(data) => collect_points(
                        metric.name(),
                        metric.description(),
                        metric.unit(),
                        data,
                        &mut points,
                    ),
                }
            }
        }

        let count = points.len() as u64;
        if count == 0 {
            return Ok(());
        }
        let service_name = service.as_deref().unwrap_or("ariadned");
        for point in &mut points {
            stamp_service(&mut point.labels, service_name);
        }
        let store = self.store.clone();
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| opentelemetry_sdk::error::OTelSdkError::InternalFailure(e.to_string()))?
            .block_on(store.write_metrics(&points))
            .map_err(|e| opentelemetry_sdk::error::OTelSdkError::InternalFailure(e.to_string()))?;

        super::record_pipeline_count("metric", service.as_deref().unwrap_or("ariadned"), count);
        Ok(())
    }

    fn force_flush(&self) -> OTelSdkResult {
        Ok(())
    }

    fn shutdown_with_timeout(&self, _timeout: Duration) -> OTelSdkResult {
        Ok(())
    }

    fn temporality(&self) -> Temporality {
        Temporality::Cumulative
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn the_resource_service_wins_and_a_colliding_attribute_moves_aside() {
        let mut plain = json!({"seat": "author"});
        stamp_service(&mut plain, "worker");
        assert_eq!(
            plain,
            json!({"seat": "author", "__resource_service_name__": "worker"})
        );

        let mut colliding = json!({
            "__resource_service_name__": "spoofed",
            "exported___resource_service_name__": "older",
        });
        stamp_service(&mut colliding, "worker");
        assert_eq!(
            colliding,
            json!({
                "__resource_service_name__": "worker",
                "exported___resource_service_name__": "older",
                "exported_exported___resource_service_name__": "spoofed",
            })
        );
    }
}
