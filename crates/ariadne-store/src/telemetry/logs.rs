//! The `log_records` table: one row per OpenTelemetry log record, read back
//! by `GET /v1/logs` and a later Loki-shaped query task.

use ariadne_core::id::new_id;
use serde_json::Value;
use sqlx::Row;
use sqlx::sqlite::SqliteRow;

use super::{TelemetryStore, normalize_timestamp};
use crate::{Result, StoreError};

/// A record as it is written. `ts` is the event time and `observed_ts` is
/// when the pipeline received it — distinct because a batched exporter can
/// flush well after the event itself (OpenTelemetry's `Timestamp` and
/// `ObservedTimestamp`).
#[derive(Debug, Clone)]
pub struct NewLogRecord {
    pub ts: String,
    pub observed_ts: String,
    pub severity_number: Option<i64>,
    pub severity_text: Option<String>,
    pub body: Option<String>,
    /// The emitting resource's `service.name`.
    pub service: Option<String>,
    /// The instrumentation scope (for the daemon's own `tracing` bridge,
    /// its crate name).
    pub scope: Option<String>,
    /// Module path, kept separate from `scope` for a target-shaped filter.
    pub target: Option<String>,
    pub attributes: Value,
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
}

/// A record as it is read back.
#[derive(Debug, Clone)]
pub struct LogRecord {
    pub id: String,
    pub ts: String,
    pub observed_ts: String,
    pub severity_number: Option<i64>,
    pub severity_text: Option<String>,
    pub body: Option<String>,
    pub service: Option<String>,
    pub scope: Option<String>,
    pub target: Option<String>,
    pub attributes: Value,
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
}

/// Every read goes through here, so every read parses `attributes` back
/// into JSON — the same shape [`crate::events::AgentEvent`] unpacks its
/// payload with.
impl sqlx::FromRow<'_, SqliteRow> for LogRecord {
    fn from_row(row: &SqliteRow) -> sqlx::Result<Self> {
        let attributes: String = row.try_get("attributes")?;
        Ok(Self {
            id: row.try_get("id")?,
            ts: row.try_get("ts")?,
            observed_ts: row.try_get("observed_ts")?,
            severity_number: row.try_get("severity_number")?,
            severity_text: row.try_get("severity_text")?,
            body: row.try_get("body")?,
            service: row.try_get("service")?,
            scope: row.try_get("scope")?,
            target: row.try_get("target")?,
            attributes: serde_json::from_str(&attributes).unwrap_or_default(),
            trace_id: row.try_get("trace_id")?,
            span_id: row.try_get("span_id")?,
        })
    }
}

/// How a page of log records is narrowed. Wide enough for both
/// `GET /v1/logs` (`tail`, `service`, `level`, `since`) and a Loki-shaped
/// query (`before_id`, for paging backward through history).
#[derive(Debug, Clone, Default)]
pub struct LogFilter {
    /// Return only the last N records of what the rest of the filter
    /// selects.
    pub tail: Option<usize>,
    pub service: Option<String>,
    /// Only records at or above this level (`"info"`, `"warn"`, ...).
    pub min_level: Option<String>,
    /// Only records at or after this RFC 3339 timestamp.
    pub since: Option<String>,
    /// Only records with an id below this one — ids are ULIDs, so this
    /// pages backward through history in order.
    pub before_id: Option<String>,
}

/// The OpenTelemetry "short name" severity number a level floor maps to:
/// the lowest number of its band (`TRACE`=1, `DEBUG`=5, `INFO`=9, `WARN`=13,
/// `ERROR`=17, `FATAL`=21), so `severity_number >= floor` is "at or above".
pub fn severity_number_for_level(level: &str) -> Option<i64> {
    match level.to_ascii_uppercase().as_str() {
        "TRACE" => Some(1),
        "DEBUG" => Some(5),
        "INFO" => Some(9),
        "WARN" | "WARNING" => Some(13),
        "ERROR" => Some(17),
        "FATAL" | "CRITICAL" => Some(21),
        _ => None,
    }
}

impl TelemetryStore {
    /// Write a batch of log records in one transaction.
    pub async fn write_logs(&self, records: &[NewLogRecord]) -> Result<Vec<LogRecord>> {
        if records.is_empty() {
            return Ok(Vec::new());
        }
        let mut tx = self.w().begin().await?;
        let mut written = Vec::with_capacity(records.len());
        for record in records {
            let ts = normalize_timestamp(&record.ts)?;
            let observed_ts = normalize_timestamp(&record.observed_ts)?;
            let id = new_id();
            sqlx::query(
                "INSERT INTO log_records
                    (id, ts, observed_ts, severity_number, severity_text, body,
                     service, scope, target, attributes, trace_id, span_id)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&id)
            .bind(&ts)
            .bind(&observed_ts)
            .bind(record.severity_number)
            .bind(&record.severity_text)
            .bind(&record.body)
            .bind(&record.service)
            .bind(&record.scope)
            .bind(&record.target)
            .bind(record.attributes.to_string())
            .bind(&record.trace_id)
            .bind(&record.span_id)
            .execute(&mut *tx)
            .await?;
            written.push(LogRecord {
                id,
                ts,
                observed_ts,
                severity_number: record.severity_number,
                severity_text: record.severity_text.clone(),
                body: record.body.clone(),
                service: record.service.clone(),
                scope: record.scope.clone(),
                target: record.target.clone(),
                attributes: record.attributes.clone(),
                trace_id: record.trace_id.clone(),
                span_id: record.span_id.clone(),
            });
        }
        tx.commit().await?;
        Ok(written)
    }

    /// A page of log records, oldest first, narrowed by whichever filters
    /// were set. `tail` is applied last, after every other narrowing.
    pub async fn read_logs(&self, filter: LogFilter) -> Result<Vec<LogRecord>> {
        let mut sql = String::from("SELECT * FROM log_records WHERE 1=1");
        let mut binds: Vec<String> = Vec::new();
        if let Some(service) = &filter.service {
            sql.push_str(" AND service = ?");
            binds.push(service.clone());
        }
        if let Some(level) = &filter.min_level {
            let floor = severity_number_for_level(level)
                .ok_or_else(|| StoreError::Invalid(format!("unknown log level: {level}")))?;
            sql.push_str(&format!(" AND severity_number >= {floor}"));
        }
        if let Some(since) = &filter.since {
            sql.push_str(" AND ts >= ?");
            binds.push(normalize_timestamp(since)?);
        }
        if let Some(before_id) = &filter.before_id {
            sql.push_str(" AND id < ?");
            binds.push(before_id.clone());
        }
        sql.push_str(" ORDER BY id");

        let mut query = sqlx::query_as::<_, LogRecord>(sqlx::AssertSqlSafe(sql));
        for bind in &binds {
            query = query.bind(bind);
        }
        let mut records = query.fetch_all(self.r()).await?;
        if let Some(tail) = filter.tail
            && records.len() > tail
        {
            records.drain(..records.len() - tail);
        }
        Ok(records)
    }

    /// Delete log records older than `before`. Part of the hourly retention
    /// job (`telemetry_logs_days`).
    #[cfg(test)]
    pub(crate) async fn delete_old_logs(&self, before: &str) -> Result<u64> {
        let before = normalize_timestamp(before)?;
        Ok(sqlx::query("DELETE FROM log_records WHERE ts < ?")
            .bind(before)
            .execute(self.w())
            .await?
            .rows_affected())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn record(ts: &str, service: &str, level: &str) -> NewLogRecord {
        NewLogRecord {
            ts: ts.to_string(),
            observed_ts: ts.to_string(),
            severity_number: severity_number_for_level(level),
            severity_text: Some(level.to_uppercase()),
            body: Some(format!("{service} says hi")),
            service: Some(service.to_string()),
            scope: Some("ariadne_daemon".to_string()),
            target: Some("ariadne_daemon::scheduler".to_string()),
            attributes: json!({}),
            trace_id: None,
            span_id: None,
        }
    }

    async fn store() -> (tempfile::TempDir, TelemetryStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = TelemetryStore::open(dir.path().join("telemetry.db"))
            .await
            .unwrap();
        (dir, store)
    }

    #[tokio::test]
    async fn written_records_read_back_oldest_first() {
        let (_dir, store) = store().await;
        store
            .write_logs(&[
                record("2026-01-01T00:00:00.000Z", "ariadned", "info"),
                record("2026-01-01T00:00:01.000Z", "ariadned", "warn"),
            ])
            .await
            .unwrap();

        let records = store.read_logs(LogFilter::default()).await.unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].body.as_deref(), Some("ariadned says hi"));
        assert_eq!(records[1].severity_text.as_deref(), Some("WARN"));
    }

    #[tokio::test]
    async fn tail_keeps_only_the_newest_n_records() {
        let (_dir, store) = store().await;
        for i in 0..5 {
            store
                .write_logs(&[record(
                    &format!("2026-01-01T00:00:0{i}.000Z"),
                    "ariadned",
                    "info",
                )])
                .await
                .unwrap();
        }
        let records = store
            .read_logs(LogFilter {
                tail: Some(2),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].ts, "2026-01-01T00:00:03.000Z");
        assert_eq!(records[1].ts, "2026-01-01T00:00:04.000Z");
    }

    #[tokio::test]
    async fn service_and_level_narrow_the_read() {
        let (_dir, store) = store().await;
        store
            .write_logs(&[
                record("2026-01-01T00:00:00.000Z", "ariadned", "info"),
                record("2026-01-01T00:00:01.000Z", "ariadne-cli", "warn"),
                record("2026-01-01T00:00:02.000Z", "ariadned", "error"),
            ])
            .await
            .unwrap();

        let by_service = store
            .read_logs(LogFilter {
                service: Some("ariadne-cli".to_string()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(by_service.len(), 1);
        assert_eq!(by_service[0].service.as_deref(), Some("ariadne-cli"));

        let at_or_above_warn = store
            .read_logs(LogFilter {
                min_level: Some("warn".to_string()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(
            at_or_above_warn.len(),
            2,
            "info is excluded, warn and error are not"
        );
    }

    #[tokio::test]
    async fn since_normalizes_fractional_precision_and_offsets() {
        let (_dir, store) = store().await;
        store
            .write_logs(&[record("2026-01-01T12:00:00.001+02:00", "ariadned", "info")])
            .await
            .unwrap();
        let records = store
            .read_logs(LogFilter {
                since: Some("2026-01-01T10:00:00Z".to_string()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].ts, "2026-01-01T10:00:00.001Z");
    }

    #[tokio::test]
    async fn an_unknown_level_is_refused_rather_than_silently_matching_nothing() {
        let (_dir, store) = store().await;
        let error = store
            .read_logs(LogFilter {
                min_level: Some("verbose".to_string()),
                ..Default::default()
            })
            .await
            .unwrap_err();
        assert!(matches!(error, StoreError::Invalid(_)));
    }

    #[tokio::test]
    async fn retention_deletes_only_records_older_than_the_cutoff() {
        let (_dir, store) = store().await;
        store
            .write_logs(&[
                record("2026-01-01T00:00:00.000Z", "ariadned", "info"),
                record("2026-01-02T00:00:00.000Z", "ariadned", "info"),
            ])
            .await
            .unwrap();

        let deleted = store
            .delete_old_logs("2026-01-01T12:00:00.000Z")
            .await
            .unwrap();
        assert_eq!(deleted, 1);
        let remaining = store.read_logs(LogFilter::default()).await.unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].ts, "2026-01-02T00:00:00.000Z");
    }
}
