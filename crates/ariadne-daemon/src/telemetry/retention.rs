//! The hourly telemetry retention job: rolls raw metric samples older than
//! an hour into `metric_rollups_1h`, deletes what has aged past its own
//! `telemetry_*_days` cutoff, then folds the telemetry file's write-ahead
//! log back in — the same shape `crate::checkpoint` runs for `ariadne.db`,
//! on its own clock rather than the commit path.

use std::time::Duration;

use chrono::{Duration as ChronoDuration, SecondsFormat, Utc};
use tracing::{debug, warn};

use ariadne_store::telemetry::TelemetryStore;

/// Start the retention pass, every `every`, until the daemon ends.
/// `samples_days`, `logs_days` and `rollups_days` are `Config`'s
/// `telemetry_samples_days`, `telemetry_logs_days` and
/// `telemetry_rollups_days`.
pub fn start(
    store: TelemetryStore,
    every: Duration,
    samples_days: i64,
    logs_days: i64,
    rollups_days: i64,
) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(every);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        // The first tick is immediate and there is nothing to retire yet.
        tick.tick().await;
        loop {
            tick.tick().await;
            retention_tick(&store, samples_days, logs_days, rollups_days).await;
        }
    });
}

fn cutoff(days: i64) -> String {
    (Utc::now() - ChronoDuration::days(days)).to_rfc3339_opts(SecondsFormat::Millis, true)
}

async fn retention_tick(
    store: &TelemetryStore,
    samples_days: i64,
    logs_days: i64,
    rollups_days: i64,
) {
    let rollup_before =
        (Utc::now() - ChronoDuration::hours(1)).to_rfc3339_opts(SecondsFormat::Millis, true);
    let samples_before = cutoff(samples_days);
    let logs_before = cutoff(logs_days);
    let rollups_before = cutoff(rollups_days);
    match store
        .enforce_retention(
            &rollup_before,
            &samples_before,
            &logs_before,
            &rollups_before,
        )
        .await
    {
        Ok(report) => debug!(
            store = "telemetry",
            rolled_up_samples = report.rolled_up_samples,
            deleted_samples = report.deleted_samples,
            deleted_logs = report.deleted_logs,
            deleted_rollups = report.deleted_rollups,
            "ran the telemetry retention pass"
        ),
        Err(error) => {
            warn!(store = "telemetry", %error, "could not run the telemetry retention pass")
        }
    }
    match store.checkpoint().await {
        Ok(()) => debug!(store = "telemetry", "folded the write-ahead log back in"),
        Err(error) => {
            warn!(store = "telemetry", %error, "could not fold the write-ahead log back in")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ariadne_store::telemetry::{MetricKind, MetricPoint, NewLogRecord};
    use serde_json::json;

    #[tokio::test]
    async fn one_tick_rolls_up_metrics_and_deletes_what_is_past_its_cutoff() {
        let dir = tempfile::tempdir().unwrap();
        let store = TelemetryStore::open(dir.path().join("telemetry.db"))
            .await
            .unwrap();

        let old =
            (Utc::now() - ChronoDuration::days(40)).to_rfc3339_opts(SecondsFormat::Millis, true);
        let recent = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
        store
            .write_metrics(&[MetricPoint {
                name: "x".to_string(),
                kind: MetricKind::Counter,
                unit: None,
                description: None,
                labels: json!({}),
                timestamp: old.clone(),
                value: 1.0,
            }])
            .await
            .unwrap();
        store
            .write_logs(&[
                NewLogRecord {
                    ts: old.clone(),
                    observed_ts: old,
                    severity_number: Some(9),
                    severity_text: Some("INFO".to_string()),
                    body: Some("old".to_string()),
                    service: Some("ariadned".to_string()),
                    scope: None,
                    target: None,
                    attributes: json!({}),
                    trace_id: None,
                    span_id: None,
                },
                NewLogRecord {
                    ts: recent.clone(),
                    observed_ts: recent,
                    severity_number: Some(9),
                    severity_text: Some("INFO".to_string()),
                    body: Some("recent".to_string()),
                    service: Some("ariadned".to_string()),
                    scope: None,
                    target: None,
                    attributes: json!({}),
                    trace_id: None,
                    span_id: None,
                },
            ])
            .await
            .unwrap();

        // Defaults from `docs/configuration.md`: 30 days of samples, 14 of
        // logs, 365 of rollups — the 40-day-old points are past all three,
        // the fresh log is past none of them.
        retention_tick(&store, 30, 14, 365).await;

        let remaining_logs = store.read_logs(Default::default()).await.unwrap();
        assert_eq!(remaining_logs.len(), 1);
        assert_eq!(remaining_logs[0].body.as_deref(), Some("recent"));

        let series_id = store.list_series(&[]).await.unwrap()[0].id.clone();
        let remaining_samples = store
            .read_samples(
                &series_id,
                ariadne_store::telemetry::TimeRange {
                    since: "1970-01-01T00:00:00.000Z".to_string(),
                    until: "2999-01-01T00:00:00.000Z".to_string(),
                },
            )
            .await
            .unwrap();
        assert!(
            remaining_samples.is_empty(),
            "rolled up, then the rollup itself is young enough to survive"
        );
    }
}
