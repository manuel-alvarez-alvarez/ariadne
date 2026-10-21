//! The hourly retention pass: roll raw metric samples into their hourly
//! bucket once they are over an hour old, then drop raw samples, log records,
//! and rollups only when each has reached its own configured cutoff. Run by
//! a daemon job on a clock (`ariadne_daemon::telemetry_retention`), the same
//! way `crate::Store`'s
//! checkpoint runs off a daemon clock rather than off the commit path.

use super::metrics::rollup_old_samples_in_transaction;
use super::{TelemetryStore, normalize_timestamp};
use crate::Result;

/// What one retention pass did, for the job's own log line.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RetentionReport {
    /// Raw samples folded into `metric_rollups_1h`; the raw rows remain until
    /// `telemetry_samples_days` expires them.
    pub rolled_up_samples: u64,
    pub deleted_samples: u64,
    pub deleted_logs: u64,
    pub deleted_rollups: u64,
}

impl TelemetryStore {
    /// Roll up and delete in one immediate writer transaction: a sample must
    /// be considered for rollup before it can be the kind of row retention
    /// removes, and no old concurrent write can land in the gap.
    ///
    /// `rollup_before` is always "an hour ago" in practice; `samples_before`,
    /// `logs_before` and `rollups_before` are each the daemon's own
    /// `telemetry_*_days` floor, turned into an absolute timestamp by the
    /// caller. Kept as plain cutoffs here, not `Duration`s, so the store
    /// stays free of what "a day" means to the daemon's clock.
    pub async fn enforce_retention(
        &self,
        rollup_before: &str,
        samples_before: &str,
        logs_before: &str,
        rollups_before: &str,
    ) -> Result<RetentionReport> {
        let rollup_before = normalize_timestamp(rollup_before)?;
        let samples_before = normalize_timestamp(samples_before)?;
        let logs_before = normalize_timestamp(logs_before)?;
        let rollups_before = normalize_timestamp(rollups_before)?;
        let mut connection = self.w().acquire().await?;
        sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut *connection)
            .await?;
        let result = async {
            let rolled_up_samples = rollup_old_samples_in_transaction(
                &mut connection,
                &rollup_before,
                std::future::ready(()),
            )
            .await?;
            let deleted_samples = sqlx::query("DELETE FROM metric_samples WHERE ts < ?")
                .bind(&samples_before)
                .execute(&mut *connection)
                .await?
                .rows_affected();
            let deleted_logs = sqlx::query("DELETE FROM log_records WHERE ts < ?")
                .bind(&logs_before)
                .execute(&mut *connection)
                .await?
                .rows_affected();
            let deleted_rollups = sqlx::query("DELETE FROM metric_rollups_1h WHERE hour < ?")
                .bind(&rollups_before)
                .execute(&mut *connection)
                .await?
                .rows_affected();
            sqlx::query(
                "DELETE FROM metric_series
                 WHERE NOT EXISTS (
                     SELECT 1 FROM metric_samples WHERE metric_samples.series_id = metric_series.id
                 ) AND NOT EXISTS (
                     SELECT 1 FROM metric_rollups_1h
                     WHERE metric_rollups_1h.series_id = metric_series.id
                 )",
            )
            .execute(&mut *connection)
            .await?
            .rows_affected();
            Ok(RetentionReport {
                rolled_up_samples,
                deleted_samples,
                deleted_logs,
                deleted_rollups,
            })
        }
        .await;
        match result {
            Ok(report) => {
                sqlx::query("COMMIT").execute(&mut *connection).await?;
                Ok(report)
            }
            Err(error) => {
                let _ = sqlx::query("ROLLBACK").execute(&mut *connection).await;
                Err(error)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::logs::NewLogRecord;
    use super::super::metrics::{MetricKind, MetricPoint};
    use super::*;
    use serde_json::json;

    async fn store() -> (tempfile::TempDir, TelemetryStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = TelemetryStore::open(dir.path().join("telemetry.db"))
            .await
            .unwrap();
        (dir, store)
    }

    #[tokio::test]
    async fn one_pass_rolls_up_and_then_deletes_raw_samples_past_retention() {
        let (_dir, store) = store().await;
        store
            .write_metrics(&[MetricPoint {
                name: "x".to_string(),
                kind: MetricKind::Counter,
                unit: None,
                description: None,
                labels: json!({}),
                timestamp: "2026-01-01T00:00:00.000Z".to_string(),
                value: 1.0,
            }])
            .await
            .unwrap();
        store
            .write_logs(&[NewLogRecord {
                ts: "2026-01-01T00:00:00.000Z".to_string(),
                observed_ts: "2026-01-01T00:00:00.000Z".to_string(),
                severity_number: Some(9),
                severity_text: Some("INFO".to_string()),
                body: Some("hi".to_string()),
                service: Some("ariadned".to_string()),
                scope: None,
                target: None,
                attributes: json!({}),
                trace_id: None,
                span_id: None,
            }])
            .await
            .unwrap();

        let report = store
            .enforce_retention(
                "2099-01-01T00:00:00.000Z",
                "2099-01-01T00:00:00.000Z",
                "2099-01-01T00:00:00.000Z",
                "2099-01-01T00:00:00.000Z",
            )
            .await
            .unwrap();
        assert_eq!(report.rolled_up_samples, 1);
        assert_eq!(
            report.deleted_samples, 1,
            "raw retention owns sample deletion"
        );
        assert_eq!(report.deleted_logs, 1);
        assert_eq!(
            report.deleted_rollups, 1,
            "and the rollup it just wrote is itself past the cutoff"
        );
    }

    #[tokio::test]
    async fn a_two_hour_old_sample_survives_a_thirty_day_raw_retention_window() {
        let (_dir, store) = store().await;
        store
            .write_metrics(&[MetricPoint {
                name: "x".to_string(),
                kind: MetricKind::Counter,
                unit: None,
                description: None,
                labels: json!({}),
                timestamp: "2026-02-01T10:00:00Z".to_string(),
                value: 1.0,
            }])
            .await
            .unwrap();
        let series = store.list_series(&[]).await.unwrap().pop().unwrap();

        let report = store
            .enforce_retention(
                "2026-02-01T11:00:00Z",
                "2026-01-02T12:00:00Z",
                "2026-01-02T12:00:00Z",
                "2025-02-02T12:00:00Z",
            )
            .await
            .unwrap();

        assert_eq!(report.rolled_up_samples, 1);
        assert_eq!(report.deleted_samples, 0);
        assert_eq!(
            store
                .read_samples(
                    &series.id,
                    super::super::metrics::TimeRange {
                        since: "2026-02-01T00:00:00Z".to_string(),
                        until: "2026-02-02T00:00:00Z".to_string(),
                    },
                )
                .await
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn retention_removes_series_only_after_raw_and_hourly_data_expire() {
        let (_dir, store) = store().await;
        store
            .write_metrics(&[
                MetricPoint {
                    name: "expired".to_string(),
                    kind: MetricKind::Gauge,
                    unit: None,
                    description: None,
                    labels: json!({}),
                    timestamp: "2026-01-01T10:10:00Z".to_string(),
                    value: 1.0,
                },
                MetricPoint {
                    name: "rollup_only".to_string(),
                    kind: MetricKind::Gauge,
                    unit: None,
                    description: None,
                    labels: json!({}),
                    timestamp: "2026-01-02T10:10:00Z".to_string(),
                    value: 2.0,
                },
            ])
            .await
            .unwrap();

        store
            .enforce_retention(
                "2026-02-01T00:00:00Z",
                "2026-01-03T00:00:00Z",
                "2026-01-03T00:00:00Z",
                "2026-01-02T00:00:00Z",
            )
            .await
            .unwrap();

        let remaining: Vec<_> = store
            .list_series(&[])
            .await
            .unwrap()
            .into_iter()
            .map(|series| series.name)
            .collect();
        assert_eq!(remaining, ["rollup_only"]);
    }

    #[tokio::test]
    async fn rollups_keep_expired_history_and_merge_late_samples() {
        let (_dir, store) = store().await;
        store
            .write_metrics(&[
                MetricPoint {
                    name: "x".to_string(),
                    kind: MetricKind::Gauge,
                    unit: None,
                    description: None,
                    labels: json!({}),
                    timestamp: "2026-01-01T10:10:00Z".to_string(),
                    value: 1.0,
                },
                MetricPoint {
                    name: "x".to_string(),
                    kind: MetricKind::Gauge,
                    unit: None,
                    description: None,
                    labels: json!({}),
                    timestamp: "2026-01-01T10:40:00Z".to_string(),
                    value: 2.0,
                },
            ])
            .await
            .unwrap();
        let series = store.list_series(&[]).await.unwrap().pop().unwrap();

        store
            .enforce_retention(
                "2026-01-31T10:30:00Z",
                "2026-01-01T10:30:00Z",
                "2025-01-01T00:00:00Z",
                "2025-01-01T00:00:00Z",
            )
            .await
            .unwrap();
        store
            .enforce_retention(
                "2026-01-31T11:30:00Z",
                "2026-01-01T11:30:00Z",
                "2025-01-01T00:00:00Z",
                "2025-01-01T00:00:00Z",
            )
            .await
            .unwrap();

        store
            .write_metrics(&[
                MetricPoint {
                    name: "x".to_string(),
                    kind: MetricKind::Gauge,
                    unit: None,
                    description: None,
                    labels: json!({}),
                    timestamp: "2026-01-01T10:05:00Z".to_string(),
                    value: 0.0,
                },
                MetricPoint {
                    name: "x".to_string(),
                    kind: MetricKind::Gauge,
                    unit: None,
                    description: None,
                    labels: json!({}),
                    timestamp: "2026-01-01T10:50:00Z".to_string(),
                    value: 4.0,
                },
            ])
            .await
            .unwrap();
        store
            .enforce_retention(
                "2026-02-01T11:30:00Z",
                "2026-01-02T11:30:00Z",
                "2025-01-01T00:00:00Z",
                "2025-01-01T00:00:00Z",
            )
            .await
            .unwrap();

        let rollup = store
            .read_rollups(
                &series.id,
                super::super::metrics::TimeRange {
                    since: "2026-01-01T10:00:00Z".to_string(),
                    until: "2026-01-01T10:00:00Z".to_string(),
                },
            )
            .await
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(rollup.min, 0.0);
        assert_eq!(rollup.max, 4.0);
        assert_eq!(rollup.sum, 7.0);
        assert_eq!(rollup.count, 4);
        assert_eq!(rollup.first, 0.0);
        assert_eq!(rollup.last, 4.0);
    }
}
