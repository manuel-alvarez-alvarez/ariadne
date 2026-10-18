//! The custom [`LogExporter`] that writes every record the SDK batches into
//! the telemetry store, feeds the live `/v1/logs/stream` broadcast, and
//! counts the batch on `ariadne.telemetry.records`.

use std::sync::Mutex;
use std::time::SystemTime;

use chrono::{DateTime, SecondsFormat, Utc};
use opentelemetry::Key;
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::error::OTelSdkResult;
use opentelemetry_sdk::logs::{LogBatch, LogExporter, SdkLogRecord};

use ariadne_api::logs::LogLineDto;
use ariadne_store::telemetry::{LogRecord, NewLogRecord, TelemetryStore};

use super::convert::{any_value_to_body, log_attributes_to_json};
use super::stream::{LogStream, StreamedLog};

/// Writes every batched record to the telemetry store on the thread the SDK's
/// batch processor runs on. Its SQLite future uses a runtime local to that
/// worker thread, so force-flushing from the daemon's Tokio runtime cannot
/// deadlock waiting for the daemon runtime to make progress.
#[derive(Debug)]
pub(super) struct StoreLogExporter {
    store: TelemetryStore,
    stream: LogStream,
    /// Set once by `set_resource`, before the first export.
    service: Mutex<Option<String>>,
}

impl StoreLogExporter {
    pub(super) fn new(store: TelemetryStore, stream: LogStream) -> Self {
        Self {
            store,
            stream,
            service: Mutex::new(None),
        }
    }

    fn service(&self) -> Option<String> {
        self.service
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

fn system_time_to_rfc3339(time: SystemTime) -> String {
    DateTime::<Utc>::from(time).to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn to_new_record(record: &SdkLogRecord, service: Option<&str>) -> NewLogRecord {
    let now = system_time_to_rfc3339(SystemTime::now());
    let ts = record
        .timestamp()
        .map(system_time_to_rfc3339)
        .unwrap_or_else(|| now.clone());
    let observed_ts = record
        .observed_timestamp()
        .map(system_time_to_rfc3339)
        .unwrap_or(now);
    let trace_context = record.trace_context();
    NewLogRecord {
        ts,
        observed_ts,
        severity_number: record.severity_number().map(|s| s as i64),
        severity_text: record.severity_text().map(str::to_string),
        body: record.body().map(any_value_to_body),
        service: service.map(str::to_string),
        scope: None,
        target: record.target().map(|t| t.to_string()),
        attributes: log_attributes_to_json(record.attributes_iter()),
        trace_id: trace_context.map(|c| c.trace_id.to_string()),
        span_id: trace_context.map(|c| c.span_id.to_string()),
    }
}

fn to_log_line(record: &LogRecord) -> LogLineDto {
    LogLineDto {
        ts: record.ts.clone(),
        level: record.severity_text.clone().unwrap_or_default(),
        target: record.target.clone().unwrap_or_default(),
        message: ariadne_api::logs::render_message(record.body.as_deref(), &record.attributes),
        service: record.service.clone().unwrap_or_default(),
    }
}

impl LogExporter for StoreLogExporter {
    async fn export(&self, batch: LogBatch<'_>) -> OTelSdkResult {
        let service = self.service();
        let mut records = Vec::new();
        let mut scope_name = None;
        for (record, scope) in batch.iter() {
            if scope_name.is_none() && !scope.name().is_empty() {
                scope_name = Some(scope.name().to_string());
            }
            records.push(to_new_record(record, service.as_deref()));
        }
        if let Some(scope_name) = &scope_name {
            for record in &mut records {
                record.scope = Some(scope_name.clone());
            }
        }
        let count = records.len() as u64;
        if count == 0 {
            return Ok(());
        }

        let store = self.store.clone();
        let written = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| opentelemetry_sdk::error::OTelSdkError::InternalFailure(e.to_string()))?
            .block_on(store.write_logs(&records))
            .map_err(|e| opentelemetry_sdk::error::OTelSdkError::InternalFailure(e.to_string()))?;

        for record in written {
            self.stream.feed(StreamedLog {
                id: record.id.clone(),
                line: to_log_line(&record),
            });
        }
        super::record_pipeline_count("log", service.as_deref().unwrap_or("ariadned"), count);
        Ok(())
    }

    fn set_resource(&mut self, resource: &Resource) {
        let service = resource
            .get(&Key::new("service.name"))
            .map(|v| v.as_str().into_owned());
        *self.service.lock().unwrap_or_else(|e| e.into_inner()) = service;
    }
}
