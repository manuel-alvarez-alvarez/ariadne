//! `metric_series` and `metric_samples`: one row per time series and one
//! per raw point. A histogram has no table of its own — it is stored the
//! Prometheus way, as three ordinary series sharing a name stem: `_bucket`
//! (one per `le` label), `_sum` and `_count`. Decomposing a histogram point
//! into those three is the exporter's job (it already holds the bucket
//! boundaries OpenTelemetry gives it); this module stores whatever
//! [`MetricPoint`]s it is handed.

use std::collections::hash_map::DefaultHasher;
use std::future::Future;
use std::hash::{Hash, Hasher};

use ariadne_core::id::new_id;
use chrono::Timelike;
use serde_json::Value;
use sqlx::Row;

use super::labels::{LabelMatcher, matches_all};
use super::{TelemetryStore, normalize_timestamp};
use crate::{Result, StoreError, now};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricKind {
    Gauge,
    Counter,
    Histogram,
}

impl MetricKind {
    pub fn as_str(self) -> &'static str {
        match self {
            MetricKind::Gauge => "gauge",
            MetricKind::Counter => "counter",
            MetricKind::Histogram => "histogram",
        }
    }

    fn parse(s: &str) -> Result<Self> {
        match s {
            "gauge" => Ok(MetricKind::Gauge),
            "counter" => Ok(MetricKind::Counter),
            "histogram" => Ok(MetricKind::Histogram),
            other => Err(StoreError::Invalid(format!("unknown metric kind: {other}"))),
        }
    }
}

/// One point to write: a series identity (name, kind, unit, labels) and the
/// value at a timestamp. A repeat of the same name and labels finds its
/// existing series rather than creating another.
#[derive(Debug, Clone)]
pub struct MetricPoint {
    pub name: String,
    pub kind: MetricKind,
    pub unit: Option<String>,
    pub description: Option<String>,
    /// A JSON object; a non-object is stored as `{}`.
    pub labels: Value,
    pub timestamp: String,
    pub value: f64,
}

/// A series' identity and metadata, as read back.
#[derive(Debug, Clone)]
pub struct SeriesMeta {
    pub id: String,
    pub name: String,
    pub kind: MetricKind,
    pub unit: Option<String>,
    pub description: Option<String>,
    pub labels: Value,
}

#[derive(Debug, Clone)]
pub struct MetricMetadata {
    pub name: String,
    pub kind: MetricKind,
    pub unit: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Sample {
    pub ts: String,
    pub value: f64,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RollupPoint {
    pub hour: String,
    pub min: f64,
    pub max: f64,
    pub sum: f64,
    pub count: i64,
    pub first: f64,
    pub last: f64,
}

/// An inclusive time range, both ends RFC 3339.
#[derive(Debug, Clone)]
pub struct TimeRange {
    pub since: String,
    pub until: String,
}

/// The labels a series is stored and looked up under: its JSON text (sorted
/// keys, since `serde_json`'s default `Value::Object` is a `BTreeMap`) and
/// a hash of that text, cheap enough to index for the find-or-create on
/// every write.
fn canonical_labels(labels: &Value) -> (String, String) {
    let text = if labels.is_object() {
        labels.to_string()
    } else {
        "{}".to_string()
    };
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    (text, format!("{:016x}", hasher.finish()))
}

/// The first 13 characters of a canonical `now()` timestamp
/// (`"2026-01-01T10"`) are its UTC hour; this is the bucket
/// `metric_rollups_1h` groups raw samples into.
fn hour_bucket(ts: &str) -> Result<String> {
    let timestamp = chrono::DateTime::parse_from_rfc3339(ts)
        .map_err(|error| {
            StoreError::Invalid(format!("invalid RFC 3339 timestamp {ts:?}: {error}"))
        })?
        .with_timezone(&chrono::Utc);
    timestamp
        .with_minute(0)
        .and_then(|timestamp| timestamp.with_second(0))
        .and_then(|timestamp| timestamp.with_nanosecond(0))
        .map(|timestamp| timestamp.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        .ok_or_else(|| StoreError::Invalid(format!("cannot make an hourly bucket for {ts:?}")))
}

impl TelemetryStore {
    /// Write a batch of metric points in one transaction, creating any
    /// series a (name, labels) pair has not been seen under yet.
    pub async fn write_metrics(&self, points: &[MetricPoint]) -> Result<()> {
        if points.is_empty() {
            return Ok(());
        }
        let mut tx = self.w().begin().await?;
        for point in points {
            let timestamp = normalize_timestamp(&point.timestamp)?;
            let (labels_text, label_hash) = canonical_labels(&point.labels);
            let series_id: Option<String> = sqlx::query_scalar(
                "SELECT id FROM metric_series WHERE name = ? AND label_hash = ?",
            )
            .bind(&point.name)
            .bind(&label_hash)
            .fetch_optional(&mut *tx)
            .await?;
            let series_id = match series_id {
                Some(id) => id,
                None => {
                    let id = new_id();
                    sqlx::query(
                        "INSERT INTO metric_series
                            (id, name, kind, unit, description, labels, label_hash, created_at)
                         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                    )
                    .bind(&id)
                    .bind(&point.name)
                    .bind(point.kind.as_str())
                    .bind(&point.unit)
                    .bind(&point.description)
                    .bind(&labels_text)
                    .bind(&label_hash)
                    .bind(now())
                    .execute(&mut *tx)
                    .await?;
                    id
                }
            };
            sqlx::query(
                "INSERT INTO metric_samples (id, series_id, ts, value) VALUES (?, ?, ?, ?)",
            )
            .bind(new_id())
            .bind(&series_id)
            .bind(timestamp)
            .bind(point.value)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Every series whose name and labels satisfy every matcher — an empty
    /// list lists every series, as an empty PromQL selector matches
    /// everything.
    pub async fn list_series(&self, matchers: &[LabelMatcher]) -> Result<Vec<SeriesMeta>> {
        let rows =
            sqlx::query("SELECT id, name, kind, unit, description, labels FROM metric_series")
                .fetch_all(self.r())
                .await?;
        let mut series = Vec::new();
        for row in rows {
            let name: String = row.try_get("name")?;
            let labels_text: String = row.try_get("labels")?;
            let labels: Value = serde_json::from_str(&labels_text).unwrap_or_default();
            if matches_all(matchers, &name, &labels)? {
                let kind: String = row.try_get("kind")?;
                series.push(SeriesMeta {
                    id: row.try_get("id")?,
                    name,
                    kind: MetricKind::parse(&kind)?,
                    unit: row.try_get("unit")?,
                    description: row.try_get("description")?,
                    labels,
                });
            }
        }
        Ok(series)
    }

    /// Every distinct label name across series matching `matchers` —
    /// `__name__` included, since it is as much a label as any other in
    /// PromQL's model.
    pub async fn label_names(&self, matchers: &[LabelMatcher]) -> Result<Vec<String>> {
        let mut names: std::collections::BTreeSet<String> =
            [super::labels::NAME_LABEL.to_string()].into();
        for series in self.list_series(matchers).await? {
            if let Some(object) = series.labels.as_object() {
                names.extend(object.keys().cloned());
            }
        }
        Ok(names.into_iter().collect())
    }

    /// Every distinct value `label` takes across series matching `matchers`.
    pub async fn label_values(
        &self,
        label: &str,
        matchers: &[LabelMatcher],
    ) -> Result<Vec<String>> {
        let mut values: std::collections::BTreeSet<String> = Default::default();
        for series in self.list_series(matchers).await? {
            if label == super::labels::NAME_LABEL {
                values.insert(series.name);
            } else if let Some(value) = series.labels.get(label).and_then(Value::as_str) {
                values.insert(value.to_string());
            }
        }
        Ok(values.into_iter().collect())
    }

    /// The kind, unit and description of `name` — read off whichever of its
    /// series was created first, since every series of one metric name is
    /// expected to agree on them.
    pub async fn metric_metadata(&self, name: &str) -> Result<Option<MetricMetadata>> {
        let row: Option<(String, String, Option<String>, Option<String>)> = sqlx::query_as(
            "SELECT name, kind, unit, description FROM metric_series
             WHERE name = ? ORDER BY created_at LIMIT 1",
        )
        .bind(name)
        .fetch_optional(self.r())
        .await?;
        row.map(|(name, kind, unit, description)| {
            Ok(MetricMetadata {
                name,
                kind: MetricKind::parse(&kind)?,
                unit,
                description,
            })
        })
        .transpose()
    }

    /// Raw samples of `series_id` in `range`, oldest first.
    pub async fn read_samples(&self, series_id: &str, range: TimeRange) -> Result<Vec<Sample>> {
        let since = normalize_timestamp(&range.since)?;
        let until = normalize_timestamp(&range.until)?;
        Ok(sqlx::query_as::<_, Sample>(
            "SELECT ts, value FROM metric_samples
             WHERE series_id = ? AND ts >= ? AND ts <= ? ORDER BY ts",
        )
        .bind(series_id)
        .bind(since)
        .bind(until)
        .fetch_all(self.r())
        .await?)
    }

    /// Hourly rollups of `series_id` in `range`, oldest first.
    pub async fn read_rollups(
        &self,
        series_id: &str,
        range: TimeRange,
    ) -> Result<Vec<RollupPoint>> {
        let since = normalize_timestamp(&range.since)?;
        let until = normalize_timestamp(&range.until)?;
        Ok(sqlx::query_as::<_, RollupPoint>(
            "SELECT hour, min, max, sum, count, first, last FROM metric_rollups_1h
             WHERE series_id = ? AND hour >= ? AND hour <= ? ORDER BY hour",
        )
        .bind(series_id)
        .bind(since)
        .bind(until)
        .fetch_all(self.r())
        .await?)
    }

    /// Roll every raw sample older than `cutoff` into its hourly bucket of
    /// [`metric_rollups_1h`]. Raw rows remain until `delete_old_samples`
    /// reaches their configured retention cutoff.
    /// Returns how many raw samples were rolled up.
    ///
    /// Grouping happens in Rust rather than SQL: the backlog a healthy
    /// hourly job faces is small, and computing `first`/`last` per
    /// `(series, hour)` group in plain SQLite needs a correlated subquery
    /// per group, which a simple fold over one ordered read avoids.
    #[cfg(test)]
    pub(crate) async fn rollup_old_samples(&self, cutoff: &str) -> Result<u64> {
        self.rollup_old_samples_after_select(cutoff, std::future::ready(()))
            .await
    }

    /// The hook is only used by the concurrency test below. It runs while
    /// the immediate transaction owns SQLite's writer lock, after its
    /// snapshot has been read and before the idempotent rollup is written.
    #[cfg(test)]
    async fn rollup_old_samples_after_select<F>(&self, cutoff: &str, after_select: F) -> Result<u64>
    where
        F: Future<Output = ()>,
    {
        let cutoff = normalize_timestamp(cutoff)?;
        let mut connection = self.w().acquire().await?;
        // Reserve the one SQLite writer before reading. A concurrent writer
        // either commits before this snapshot and enters the rollup, or waits
        // for this pass and remains raw for the next one.
        sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut *connection)
            .await?;
        let result =
            rollup_old_samples_in_transaction(&mut connection, &cutoff, after_select).await;
        match result {
            Ok(rolled) => {
                sqlx::query("COMMIT").execute(&mut *connection).await?;
                Ok(rolled)
            }
            Err(error) => {
                let _ = sqlx::query("ROLLBACK").execute(&mut *connection).await;
                Err(error)
            }
        }
    }

    /// Delete samples (already rolled up, or not yet a full hour old and so
    /// never rolled) older than `before`. Part of the hourly retention job
    /// (`telemetry_samples_days`).
    #[cfg(test)]
    pub(crate) async fn delete_old_samples(&self, before: &str) -> Result<u64> {
        let before = normalize_timestamp(before)?;
        Ok(sqlx::query("DELETE FROM metric_samples WHERE ts < ?")
            .bind(before)
            .execute(self.w())
            .await?
            .rows_affected())
    }

    /// Delete hourly rollups older than `before`
    /// (`telemetry_rollups_days`).
    #[cfg(test)]
    pub(crate) async fn delete_old_rollups(&self, before: &str) -> Result<u64> {
        let before = normalize_timestamp(before)?;
        Ok(sqlx::query("DELETE FROM metric_rollups_1h WHERE hour < ?")
            .bind(before)
            .execute(self.w())
            .await?
            .rows_affected())
    }
}

/// Fold each not-yet-rolled raw sample before `cutoff` into its hourly row,
/// using the caller's already-exclusive writer transaction. The persisted
/// marker makes repeats idempotent and preserves already-expired history
/// while late samples merge into that hour.
pub(super) async fn rollup_old_samples_in_transaction<F>(
    connection: &mut sqlx::SqliteConnection,
    cutoff: &str,
    after_select: F,
) -> Result<u64>
where
    F: Future<Output = ()>,
{
    let rows: Vec<(String, String, String, f64)> = sqlx::query_as(
        "SELECT id, series_id, ts, value FROM metric_samples
         WHERE ts < ? AND rolled_up = 0 ORDER BY series_id, ts, id",
    )
    .bind(cutoff)
    .fetch_all(&mut *connection)
    .await?;
    if rows.is_empty() {
        return Ok(0);
    }
    after_select.await;

    struct Agg {
        min: f64,
        max: f64,
        sum: f64,
        count: i64,
        first: f64,
        last: f64,
        first_ts: String,
        last_ts: String,
    }

    let mut groups: std::collections::BTreeMap<(String, String), Agg> = Default::default();
    for (_, series_id, ts, value) in &rows {
        let key = (series_id.clone(), hour_bucket(ts)?);
        groups
            .entry(key)
            .and_modify(|agg| {
                agg.min = agg.min.min(*value);
                agg.max = agg.max.max(*value);
                agg.sum += value;
                agg.count += 1;
                agg.last = *value;
                agg.last_ts = ts.clone();
            })
            .or_insert(Agg {
                min: *value,
                max: *value,
                sum: *value,
                count: 1,
                first: *value,
                last: *value,
                first_ts: ts.clone(),
                last_ts: ts.clone(),
            });
    }

    for ((series_id, hour), agg) in &groups {
        sqlx::query(
            "INSERT INTO metric_rollups_1h
                    (series_id, hour, min, max, sum, count, first, last, first_ts, last_ts)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT (series_id, hour) DO UPDATE SET
                min = MIN(metric_rollups_1h.min, excluded.min),
                max = MAX(metric_rollups_1h.max, excluded.max),
                sum = metric_rollups_1h.sum + excluded.sum,
                count = metric_rollups_1h.count + excluded.count,
                first = CASE WHEN excluded.first_ts < metric_rollups_1h.first_ts
                    THEN excluded.first ELSE metric_rollups_1h.first END,
                last = CASE WHEN excluded.last_ts > metric_rollups_1h.last_ts
                    THEN excluded.last ELSE metric_rollups_1h.last END,
                first_ts = MIN(metric_rollups_1h.first_ts, excluded.first_ts),
                last_ts = MAX(metric_rollups_1h.last_ts, excluded.last_ts)",
        )
        .bind(series_id)
        .bind(hour)
        .bind(agg.min)
        .bind(agg.max)
        .bind(agg.sum)
        .bind(agg.count)
        .bind(agg.first)
        .bind(agg.last)
        .bind(&agg.first_ts)
        .bind(&agg.last_ts)
        .execute(&mut *connection)
        .await?;
    }
    for (id, _, _, _) in &rows {
        sqlx::query("UPDATE metric_samples SET rolled_up = 1 WHERE id = ?")
            .bind(id)
            .execute(&mut *connection)
            .await?;
    }
    Ok(rows.len() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::labels::MatchOp;
    use serde_json::json;
    use std::sync::Arc;
    use tokio::sync::Notify;

    async fn store() -> (tempfile::TempDir, TelemetryStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = TelemetryStore::open(dir.path().join("telemetry.db"))
            .await
            .unwrap();
        (dir, store)
    }

    fn point(name: &str, labels: Value, ts: &str, value: f64) -> MetricPoint {
        MetricPoint {
            name: name.to_string(),
            kind: MetricKind::Counter,
            unit: Some("1".to_string()),
            description: Some("a test metric".to_string()),
            labels,
            timestamp: ts.to_string(),
            value,
        }
    }

    #[tokio::test]
    async fn repeat_points_of_the_same_name_and_labels_share_one_series() {
        let (_dir, store) = store().await;
        store
            .write_metrics(&[
                point(
                    "ariadne.telemetry.records",
                    json!({"signal": "log"}),
                    "2026-01-01T00:00:00.000Z",
                    1.0,
                ),
                point(
                    "ariadne.telemetry.records",
                    json!({"signal": "log"}),
                    "2026-01-01T00:00:01.000Z",
                    2.0,
                ),
                point(
                    "ariadne.telemetry.records",
                    json!({"signal": "metric"}),
                    "2026-01-01T00:00:00.000Z",
                    5.0,
                ),
            ])
            .await
            .unwrap();

        let series = store.list_series(&[]).await.unwrap();
        assert_eq!(series.len(), 2, "two distinct label sets, two series");
        let log_series = series.iter().find(|s| s.labels["signal"] == "log").unwrap();
        let samples = store
            .read_samples(
                &log_series.id,
                TimeRange {
                    since: "2026-01-01T00:00:00.000Z".to_string(),
                    until: "2026-01-01T00:00:01.000Z".to_string(),
                },
            )
            .await
            .unwrap();
        assert_eq!(samples.len(), 2, "both points landed on the one series");
    }

    #[tokio::test]
    async fn list_series_is_narrowed_by_label_matchers() {
        let (_dir, store) = store().await;
        store
            .write_metrics(&[
                point(
                    "ariadne.telemetry.records",
                    json!({"signal": "log"}),
                    "2026-01-01T00:00:00.000Z",
                    1.0,
                ),
                point(
                    "ariadne.telemetry.records",
                    json!({"signal": "metric"}),
                    "2026-01-01T00:00:00.000Z",
                    1.0,
                ),
                point(
                    "ariadne.other.metric",
                    json!({}),
                    "2026-01-01T00:00:00.000Z",
                    1.0,
                ),
            ])
            .await
            .unwrap();

        let by_name = store
            .list_series(&[LabelMatcher::new(
                super::super::labels::NAME_LABEL,
                MatchOp::Eq,
                "ariadne.telemetry.records",
            )])
            .await
            .unwrap();
        assert_eq!(by_name.len(), 2);

        let by_label = store
            .list_series(&[LabelMatcher::new("signal", MatchOp::Eq, "log")])
            .await
            .unwrap();
        assert_eq!(by_label.len(), 1);
        assert_eq!(by_label[0].name, "ariadne.telemetry.records");
    }

    #[tokio::test]
    async fn label_names_and_values_are_read_across_matching_series() {
        let (_dir, store) = store().await;
        store
            .write_metrics(&[
                point(
                    "ariadne.telemetry.records",
                    json!({"signal": "log"}),
                    "2026-01-01T00:00:00.000Z",
                    1.0,
                ),
                point(
                    "ariadne.telemetry.records",
                    json!({"signal": "metric"}),
                    "2026-01-01T00:00:00.000Z",
                    1.0,
                ),
            ])
            .await
            .unwrap();

        let names = store.label_names(&[]).await.unwrap();
        assert!(names.contains(&"signal".to_string()));
        assert!(names.contains(&super::super::labels::NAME_LABEL.to_string()));

        let mut values = store.label_values("signal", &[]).await.unwrap();
        values.sort();
        assert_eq!(values, vec!["log".to_string(), "metric".to_string()]);
    }

    #[tokio::test]
    async fn metadata_reports_kind_and_unit() {
        let (_dir, store) = store().await;
        store
            .write_metrics(&[point(
                "ariadne.telemetry.records",
                json!({}),
                "2026-01-01T00:00:00.000Z",
                1.0,
            )])
            .await
            .unwrap();
        let metadata = store
            .metric_metadata("ariadne.telemetry.records")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(metadata.kind, MetricKind::Counter);
        assert_eq!(metadata.unit.as_deref(), Some("1"));
        assert!(
            store
                .metric_metadata("nothing.like.this")
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn rollup_keeps_raw_samples_until_their_retention_cutoff() {
        let (_dir, store) = store().await;
        store
            .write_metrics(&[
                point("x", json!({}), "2026-01-01T10:00:00.000Z", 1.0),
                point("x", json!({}), "2026-01-01T10:30:00.000Z", 3.0),
                point("x", json!({}), "2026-01-01T10:59:59.000Z", 2.0),
                // Outside the rollup cutoff: stays unrepresented in the rollup.
                point("x", json!({}), "2026-01-01T11:55:00.000Z", 9.0),
            ])
            .await
            .unwrap();
        let series_id = store.list_series(&[]).await.unwrap()[0].id.clone();

        let rolled = store
            .rollup_old_samples("2026-01-01T11:00:00.000Z")
            .await
            .unwrap();
        assert_eq!(rolled, 3);

        let rollups = store
            .read_rollups(
                &series_id,
                TimeRange {
                    since: "2026-01-01T00:00:00.000Z".to_string(),
                    until: "2026-01-02T00:00:00.000Z".to_string(),
                },
            )
            .await
            .unwrap();
        assert_eq!(rollups.len(), 1);
        let hour = &rollups[0];
        assert_eq!(hour.hour, "2026-01-01T10:00:00.000Z");
        assert_eq!(hour.min, 1.0);
        assert_eq!(hour.max, 3.0);
        assert_eq!(hour.sum, 6.0);
        assert_eq!(hour.count, 3);
        assert_eq!(hour.first, 1.0);
        assert_eq!(hour.last, 2.0);

        let remaining = store
            .read_samples(
                &series_id,
                TimeRange {
                    since: "2026-01-01T00:00:00.000Z".to_string(),
                    until: "2026-01-02T00:00:00.000Z".to_string(),
                },
            )
            .await
            .unwrap();
        assert_eq!(
            remaining.len(),
            4,
            "raw retention is independent of hourly rollup"
        );
        assert_eq!(remaining[3].value, 9.0);

        assert_eq!(
            store
                .rollup_old_samples("2026-01-01T11:00:00.000Z")
                .await
                .unwrap(),
            0,
            "a repeat finds no unrolled raw samples to add twice"
        );
        let repeated = store
            .read_rollups(
                &series_id,
                TimeRange {
                    since: "2026-01-01T10:00:00Z".to_string(),
                    until: "2026-01-01T10:00:00.000Z".to_string(),
                },
            )
            .await
            .unwrap();
        assert_eq!(repeated[0].sum, 6.0);
    }

    #[tokio::test]
    async fn equivalent_timestamp_forms_share_inclusive_range_boundaries() {
        let (_dir, store) = store().await;
        store
            .write_metrics(&[point("x", json!({}), "2026-01-01T12:00:00.001+02:00", 1.0)])
            .await
            .unwrap();
        let series_id = store.list_series(&[]).await.unwrap()[0].id.clone();
        let samples = store
            .read_samples(
                &series_id,
                TimeRange {
                    since: "2026-01-01T10:00:00Z".to_string(),
                    until: "2026-01-01T10:00:00.001Z".to_string(),
                },
            )
            .await
            .unwrap();
        assert_eq!(samples.len(), 1);

        store
            .rollup_old_samples("2026-01-01T11:00:00Z")
            .await
            .unwrap();
        let rollups = store
            .read_rollups(
                &series_id,
                TimeRange {
                    since: "2026-01-01T10:00:00Z".to_string(),
                    until: "2026-01-01T10:00:00.000Z".to_string(),
                },
            )
            .await
            .unwrap();
        assert_eq!(rollups.len(), 1);
    }

    #[tokio::test]
    async fn an_invalid_timestamp_is_refused_before_its_metric_is_written() {
        let (_dir, store) = store().await;
        let error = store
            .write_metrics(&[point("requests", json!({}), "not-a-timestamp", 1.0)])
            .await
            .unwrap_err();
        assert!(error.to_string().contains("invalid RFC 3339 timestamp"));
        assert!(store.list_series(&[]).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_writer_waiting_after_rollup_selection_keeps_its_raw_sample() {
        let dir = tempfile::tempdir().unwrap();
        let store = TelemetryStore::open(dir.path().join("telemetry.db"))
            .await
            .unwrap();
        let writer_store = store.clone();
        store
            .write_metrics(&[point("x", json!({}), "2026-01-01T10:00:00Z", 1.0)])
            .await
            .unwrap();
        let selected = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let rolling_store = store.clone();
        let rolling_selected = selected.clone();
        let rolling_release = release.clone();
        let rolling = tokio::spawn(async move {
            rolling_store
                .rollup_old_samples_after_select("2026-01-01T11:00:00Z", async move {
                    rolling_selected.notify_one();
                    rolling_release.notified().await;
                })
                .await
        });
        selected.notified().await;

        let writer = tokio::spawn(async move {
            writer_store
                .write_metrics(&[point("x", json!({}), "2026-01-01T10:30:00Z", 2.0)])
                .await
        });
        release.notify_one();
        assert_eq!(rolling.await.unwrap().unwrap(), 1);
        writer.await.unwrap().unwrap();

        let series_id = store.list_series(&[]).await.unwrap()[0].id.clone();
        let raw = store
            .read_samples(
                &series_id,
                TimeRange {
                    since: "2026-01-01T10:00:00Z".to_string(),
                    until: "2026-01-01T11:00:00Z".to_string(),
                },
            )
            .await
            .unwrap();
        assert_eq!(raw.len(), 2, "the writer's sample was not deleted unseen");
        store
            .rollup_old_samples("2026-01-01T11:00:00Z")
            .await
            .unwrap();
        let rollups = store
            .read_rollups(
                &series_id,
                TimeRange {
                    since: "2026-01-01T10:00:00Z".to_string(),
                    until: "2026-01-01T10:00:00Z".to_string(),
                },
            )
            .await
            .unwrap();
        assert_eq!(rollups[0].sum, 3.0, "the next rollup includes the writer");
    }

    #[tokio::test]
    async fn delete_old_samples_removes_a_raw_sample_that_was_never_rolled_up() {
        let (_dir, store) = store().await;
        store
            .write_metrics(&[point("x", json!({}), "2026-01-01T00:00:00.000Z", 1.0)])
            .await
            .unwrap();
        assert_eq!(
            store
                .delete_old_samples("2026-01-02T00:00:00.000Z")
                .await
                .unwrap(),
            1
        );
    }

    #[tokio::test]
    async fn delete_old_rollups_keeps_only_the_rollup_within_its_cutoff() {
        let (_dir, store) = store().await;
        store
            .write_metrics(&[point("x", json!({}), "2026-02-01T00:00:00.000Z", 2.0)])
            .await
            .unwrap();
        store
            .rollup_old_samples("2026-02-01T01:00:00.000Z")
            .await
            .unwrap();
        store
            .write_metrics(&[point("x", json!({}), "2026-03-01T00:00:00.000Z", 3.0)])
            .await
            .unwrap();
        store
            .rollup_old_samples("2026-03-01T01:00:00.000Z")
            .await
            .unwrap();

        let series_id = store.list_series(&[]).await.unwrap()[0].id.clone();
        assert_eq!(
            store
                .delete_old_rollups("2026-02-15T00:00:00.000Z")
                .await
                .unwrap(),
            1
        );
        let remaining = store
            .read_rollups(
                &series_id,
                TimeRange {
                    since: "2026-01-01T00:00:00.000Z".to_string(),
                    until: "2026-04-01T00:00:00.000Z".to_string(),
                },
            )
            .await
            .unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].hour, "2026-03-01T00:00:00.000Z");
    }
}
