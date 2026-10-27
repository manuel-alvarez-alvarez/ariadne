//! Small OTLP/HTTP exporters for CLI command facts.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use ariadne_client::Client;
use opentelemetry::KeyValue;
use opentelemetry::metrics::{Counter, Histogram, MeterProvider as _};
use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use opentelemetry_proto::tonic::collector::logs::v1::ExportLogsServiceRequest;
use opentelemetry_proto::tonic::collector::metrics::v1::ExportMetricsServiceRequest;
use opentelemetry_proto::transform::common::tonic::ResourceAttributesWithSchema;
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::error::{OTelSdkError, OTelSdkResult};
use opentelemetry_sdk::logs::{LogBatch, LogExporter, SdkLoggerProvider};
use opentelemetry_sdk::metrics::data::ResourceMetrics;
use opentelemetry_sdk::metrics::exporter::PushMetricExporter;
use opentelemetry_sdk::metrics::{PeriodicReader, SdkMeterProvider, Temporality};
use prost::Message;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::Layer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

const CONTENT_TYPE: &str = "application/x-protobuf";
const FLUSH_TIMEOUT: Duration = Duration::from_millis(250);

pub(crate) struct Telemetry {
    logs: SdkLoggerProvider,
    metrics: SdkMeterProvider,
    commands: Counter<u64>,
    duration: Histogram<f64>,
}

impl Telemetry {
    pub(crate) fn enabled(client: Arc<Client>) -> Option<Self> {
        Self::with_setting(client, std::env::var("ARIADNE_TELEMETRY").ok().as_deref())
    }

    fn with_setting(client: Arc<Client>, setting: Option<&str>) -> Option<Self> {
        if disabled(setting) {
            return None;
        }

        let resource = Resource::builder_empty()
            .with_attributes([
                KeyValue::new("service.name", "ariadne-cli"),
                KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
            ])
            .build();
        let logs = SdkLoggerProvider::builder()
            .with_resource(resource.clone())
            .with_batch_exporter(LogExporterImpl::new(Arc::clone(&client)))
            .build();
        let metrics = SdkMeterProvider::builder()
            .with_resource(resource)
            .with_reader(
                PeriodicReader::builder(PushMetricExporterImpl::new(client))
                    .with_interval(Duration::from_secs(30))
                    .build(),
            )
            .build();
        let meter = metrics.meter("ariadne-cli");
        let commands = meter.u64_counter("ariadne.cli.commands").build();
        let duration = meter
            .f64_histogram("ariadne.cli.command.duration")
            .with_unit("s")
            .build();

        let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
        tracing_subscriber::registry()
            .with(OpenTelemetryTracingBridge::new(&logs).with_filter(filter))
            .try_init()
            .ok()?;

        Some(Self {
            logs,
            metrics,
            commands,
            duration,
        })
    }

    pub(crate) fn command(&self, command: &str, outcome: &str, elapsed: Duration) {
        tracing::info!(
            command,
            outcome,
            elapsed_ms = elapsed.as_millis(),
            "command finished"
        );
        self.commands.add(
            1,
            &[
                KeyValue::new("command", command.to_string()),
                KeyValue::new("outcome", outcome.to_string()),
            ],
        );
        self.duration.record(
            elapsed.as_secs_f64(),
            &[KeyValue::new("command", command.to_string())],
        );
    }

    pub(crate) async fn flush(self) {
        let _ = tokio::time::timeout(
            FLUSH_TIMEOUT,
            tokio::task::spawn_blocking(move || {
                let _ = self.logs.force_flush();
                let _ = self.metrics.force_flush();
                let _ = self.logs.shutdown_with_timeout(FLUSH_TIMEOUT);
                let _ = self.metrics.shutdown_with_timeout(FLUSH_TIMEOUT);
            }),
        )
        .await;
    }
}

pub(crate) struct LogExporterImpl {
    client: Arc<Client>,
    runtime: tokio::runtime::Handle,
    resource: Mutex<Resource>,
}

impl std::fmt::Debug for LogExporterImpl {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LogExporterImpl")
            .finish_non_exhaustive()
    }
}

impl LogExporterImpl {
    fn new(client: Arc<Client>) -> Self {
        Self {
            client,
            runtime: tokio::runtime::Handle::current(),
            resource: Mutex::new(Resource::builder_empty().build()),
        }
    }
}

impl LogExporter for LogExporterImpl {
    fn export(
        &self,
        batch: LogBatch<'_>,
    ) -> impl std::future::Future<Output = OTelSdkResult> + Send {
        let request = ExportLogsServiceRequest {
            resource_logs:
                opentelemetry_proto::transform::logs::tonic::group_logs_by_resource_and_scope(
                    &batch,
                    &ResourceAttributesWithSchema::from(
                        &*self.resource.lock().expect("telemetry resource lock"),
                    ),
                ),
        };
        let mut body = Vec::new();
        let result = request
            .encode(&mut body)
            .map_err(|error| OTelSdkError::InternalFailure(error.to_string()));
        let client = Arc::clone(&self.client);
        let runtime = self.runtime.clone();
        async move {
            result?;
            runtime
                .spawn(async move { client.post_bytes("/otlp/v1/logs", CONTENT_TYPE, body).await })
                .await
                .map_err(|error| OTelSdkError::InternalFailure(error.to_string()))?
                .map_err(export_error)
        }
    }

    fn set_resource(&mut self, resource: &Resource) {
        *self.resource.lock().expect("telemetry resource lock") = resource.clone();
    }
}

pub(crate) struct PushMetricExporterImpl {
    client: Arc<Client>,
    runtime: tokio::runtime::Handle,
}

impl std::fmt::Debug for PushMetricExporterImpl {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PushMetricExporterImpl")
            .finish_non_exhaustive()
    }
}

impl PushMetricExporterImpl {
    fn new(client: Arc<Client>) -> Self {
        Self {
            client,
            runtime: tokio::runtime::Handle::current(),
        }
    }
}

impl PushMetricExporter for PushMetricExporterImpl {
    fn export(
        &self,
        metrics: &ResourceMetrics,
    ) -> impl std::future::Future<Output = OTelSdkResult> + Send {
        let request = ExportMetricsServiceRequest::from(metrics);
        let mut body = Vec::new();
        let result = request
            .encode(&mut body)
            .map_err(|error| OTelSdkError::InternalFailure(error.to_string()));
        let client = Arc::clone(&self.client);
        let runtime = self.runtime.clone();
        async move {
            result?;
            runtime
                .spawn(async move {
                    client
                        .post_bytes("/otlp/v1/metrics", CONTENT_TYPE, body)
                        .await
                })
                .await
                .map_err(|error| OTelSdkError::InternalFailure(error.to_string()))?
                .map_err(export_error)
        }
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

fn export_error(error: ariadne_client::ClientError) -> OTelSdkError {
    OTelSdkError::InternalFailure(error.to_string())
}

fn disabled(value: Option<&str>) -> bool {
    value == Some("off")
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::Router;
    use axum::body::Bytes;
    use axum::extract::{Path, State};
    use axum::routing::post;
    use tokio::sync::mpsc;

    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn enabled_command_logs_keep_levels_targets_and_fields() {
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let app = Router::new()
            .route("/otlp/v1/{signal}", post(receive))
            .with_state(sender);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind stub");
        let address = listener.local_addr().expect("stub address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve stub");
        });

        let telemetry = Telemetry::enabled(Arc::new(Client::tcp(format!("http://{address}"))))
            .expect("telemetry starts");
        tracing::warn!(attempt = 2, "a warning for the stub");
        telemetry.command("task ls", "ok", Duration::from_millis(12));
        telemetry.flush().await;

        let mut signals = Vec::new();
        while let Ok((signal, body)) = receiver.try_recv() {
            signals.push(signal.clone());
            if signal == "logs" {
                let logs = ExportLogsServiceRequest::decode(body.as_ref()).unwrap();
                let records: Vec<_> = logs.resource_logs[0]
                    .scope_logs
                    .iter()
                    .flat_map(|scope| &scope.log_records)
                    .collect();
                assert!(records.iter().any(|record| {
                    record.severity_text == "INFO"
                        && record
                            .body
                            .as_ref()
                            .is_some_and(|body| format!("{body:?}").contains("command finished"))
                }));
                let warning = records
                    .iter()
                    .find(|record| record.severity_text == "WARN")
                    .unwrap();
                assert!(
                    warning
                        .attributes
                        .iter()
                        .any(|field| field.key == "attempt")
                );
                assert!(logs.resource_logs[0].scope_logs.iter().any(|scope| {
                    scope
                        .scope
                        .as_ref()
                        .is_some_and(|scope| scope.name.contains("telemetry"))
                }));
            }
        }
        server.abort();

        assert!(signals.contains(&"logs".to_string()));
        assert!(signals.contains(&"metrics".to_string()));
    }

    async fn receive(
        Path(signal): Path<String>,
        State(sender): State<mpsc::UnboundedSender<(String, Bytes)>>,
        body: Bytes,
    ) {
        sender.send((signal, body)).expect("test receiver open");
    }

    #[test]
    fn off_disables_telemetry() {
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let app = Router::new()
            .route("/otlp/v1/{signal}", post(receive))
            .with_state(sender);
        let runtime = tokio::runtime::Runtime::new().expect("test runtime");
        let listener = runtime
            .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
            .expect("bind stub");
        let address = listener.local_addr().expect("stub address");
        let server = runtime.spawn(async move {
            axum::serve(listener, app).await.expect("serve stub");
        });

        assert!(
            Telemetry::with_setting(
                Arc::new(Client::tcp(format!("http://{address}"))),
                Some("off"),
            )
            .is_none()
        );
        assert!(receiver.try_recv().is_err());
        server.abort();
    }
}
