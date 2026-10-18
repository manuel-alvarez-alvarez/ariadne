//! The daemon's OpenTelemetry pipeline (033): the daemon's own `tracing`
//! events and recorded metrics flow through the OpenTelemetry SDK to the
//! telemetry store, rather than through a store call of their own. Bridges
//! `tracing` into an [`SdkLoggerProvider`] and sets an [`SdkMeterProvider`]
//! as the global one, so a later task records through
//! [`opentelemetry::global::meter`].
//!
//! The batch log processor and the metrics `PeriodicReader` each run on
//! their own plain thread. Each exporter owns the small current-thread Tokio
//! runtime it needs to finish its SQLite future there, so flushing from the
//! daemon's Tokio thread cannot wait on work that needs that same thread.

mod convert;
mod logs_exporter;
mod metrics_exporter;
pub mod retention;
mod stream;

use std::sync::OnceLock;

use opentelemetry::KeyValue;
use opentelemetry::metrics::Counter;
use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::logs::{SdkLogger, SdkLoggerProvider};
use opentelemetry_sdk::metrics::{PeriodicReader, SdkMeterProvider};

use ariadne_store::telemetry::TelemetryStore;

use logs_exporter::StoreLogExporter;
use metrics_exporter::StoreMetricExporter;

pub use stream::LogStream;
pub(crate) use stream::StreamedLog;

/// `service.name` every log and metric this daemon produces carries.
const SERVICE_NAME: &str = "ariadned";

/// Owns the two SDK providers so the daemon can shut them down on exit,
/// flushing whatever the last batch had not sent yet.
pub struct Telemetry {
    logger_provider: SdkLoggerProvider,
    meter_provider: SdkMeterProvider,
}

/// Build the pipeline. Returns the [`Telemetry`] handle — [`Telemetry::log_layer`]
/// is the `tracing_subscriber` layer to register alongside the daemon's
/// `fmt` layer, and [`Telemetry::shutdown`] flushes both providers on exit —
/// and the [`LogStream`] `/v1/logs/stream` follows.
///
pub fn init(store: TelemetryStore, service_version: &str) -> (Telemetry, LogStream) {
    let resource = Resource::builder()
        .with_service_name(SERVICE_NAME)
        .with_attribute(KeyValue::new(
            "service.version",
            service_version.to_string(),
        ))
        .build();

    let log_stream = LogStream::new();
    let log_exporter = StoreLogExporter::new(store.clone(), log_stream.clone());
    let logger_provider = SdkLoggerProvider::builder()
        .with_resource(resource.clone())
        .with_batch_exporter(log_exporter)
        .build();

    let metric_exporter = StoreMetricExporter::new(store);
    let reader = PeriodicReader::builder(metric_exporter).build();
    let meter_provider = SdkMeterProvider::builder()
        .with_resource(resource)
        .with_reader(reader)
        .build();
    opentelemetry::global::set_meter_provider(meter_provider.clone());

    (
        Telemetry {
            logger_provider,
            meter_provider,
        },
        log_stream,
    )
}

impl Telemetry {
    /// A fresh bridge layer over the same logger provider. A
    /// `tracing_subscriber::registry()` takes a layer by value, so this is
    /// callable more than once: the daemon makes one at startup, and a test
    /// harness that installs a thread-local subscriber per test
    /// (`tracing::subscriber::set_default`) makes a fresh one each time.
    pub fn log_layer(&self) -> OpenTelemetryTracingBridge<SdkLoggerProvider, SdkLogger> {
        OpenTelemetryTracingBridge::new(&self.logger_provider)
    }

    /// Export whatever is queued right now, without waiting for the batch
    /// timer or a full queue. For a test that wants its tracing event (and
    /// the pipeline counter it moves) visible immediately rather than
    /// polling `eventually` through the schedule delay.
    pub fn flush(&self) {
        if let Err(error) = self.logger_provider.force_flush() {
            tracing::warn!(%error, "could not flush the telemetry log pipeline");
        }
        if let Err(error) = self.meter_provider.force_flush() {
            tracing::warn!(%error, "could not flush the telemetry metric pipeline");
        }
    }

    /// Flush and shut both providers down, so the last batch of logs and
    /// metrics still reaches the store before the daemon exits. Blocking —
    /// each provider joins its own background thread — so call this from a
    /// context that can afford to block, such as just before `main` returns.
    pub fn shutdown(&self) {
        if let Err(error) = self.logger_provider.shutdown() {
            tracing::warn!(%error, "could not shut down the telemetry log pipeline cleanly");
        }
        if let Err(error) = self.meter_provider.shutdown() {
            tracing::warn!(%error, "could not shut down the telemetry metric pipeline cleanly");
        }
    }
}

/// The label key each stored metric point carries its resource's
/// `service.name` under. The `__` prefix is Prometheus's reserved one, so no
/// conventional attribute takes it; the metric exporter moves aside an
/// attribute that does, and the query routes (034) read the service from
/// this key alone.
pub(crate) const RESOURCE_SERVICE_LABEL: &str = "__resource_service_name__";

/// `key` with `exported_` added until `taken` holds no such key: where a
/// label that must not be overwritten moves to, the Prometheus way.
pub(crate) fn exported_key(key: &str, taken: impl Fn(&str) -> bool) -> String {
    let mut moved = format!("exported_{key}");
    while taken(&moved) {
        moved = format!("exported_{moved}");
    }
    moved
}

static TELEMETRY_RECORDS: OnceLock<Counter<u64>> = OnceLock::new();

/// Count `count` records of `signal` (`"log"` or `"metric"`) as exported,
/// on `ariadne.telemetry.records` — the one metric this task proves the
/// pipeline with. Recorded through the global meter, so it is itself
/// collected and written the same way as any later task's own metrics.
fn record_pipeline_count(signal: &'static str, service: &str, count: u64) {
    let counter = TELEMETRY_RECORDS.get_or_init(|| {
        opentelemetry::global::meter(SERVICE_NAME)
            .u64_counter("ariadne.telemetry.records")
            .with_description("Log and metric records written through the telemetry pipeline.")
            .build()
    });
    counter.add(
        count,
        &[
            KeyValue::new("signal", signal),
            KeyValue::new("service", service.to_string()),
        ],
    );
}
