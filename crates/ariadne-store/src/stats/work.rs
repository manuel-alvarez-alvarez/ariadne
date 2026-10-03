//! The `work` family: what got done.

use std::collections::BTreeMap;

use chrono::{DateTime, Duration, Utc};

use super::{Bucket, StatsFilter, median, narrowed};
use crate::{Result, Store};

/// What `work_stats` answers: what got done, over the facts a filter keeps.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WorkStats {
    pub totals: WorkTotals,
    /// The bucket a [`WorkBucket`] falls on, `"day"` or `"week"`
    /// ([`Bucket::for_span`]).
    pub bucket: String,
    /// One row per bucket from the first fact to the last, zeros included.
    pub buckets: Vec<WorkBucket>,
}

/// The counts `work_stats` answers over the whole span a filter keeps.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WorkTotals {
    pub goals_completed: i64,
    pub goals_cancelled: i64,
    pub median_goal_lead_time_secs: f64,
    pub tasks_finished: i64,
    pub tasks_failed: i64,
    pub tasks_cancelled: i64,
    /// `tasks_finished` over the three endings; 0 where there are none.
    pub finish_rate: f64,
    /// Finished tasks whose `landing` was `merge` or `pull_request`.
    pub landed: i64,
}

/// One bucket of the time axis: the counts of the facts that fall in it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WorkBucket {
    /// The RFC 3339 start of the bucket.
    pub start: String,
    pub tasks_finished: i64,
    pub tasks_failed: i64,
    pub tasks_cancelled: i64,
    pub goals_completed: i64,
    pub landed: i64,
}

/// A `stat_facts` row read for this aggregate: only what it needs.
#[derive(Debug, sqlx::FromRow)]
struct FactRow {
    kind: String,
    created_at: String,
    data: String,
}

impl Store {
    /// What got done, over the facts the filter keeps: goals completed and
    /// cancelled, tasks finished, failed and cancelled, and changes landed.
    pub async fn work_stats(&self, filter: &StatsFilter) -> Result<WorkStats> {
        let (clause, binds) = narrowed(filter);
        let sql = format!(
            "SELECT kind, created_at, data FROM stat_facts
              WHERE kind IN ('task_ended', 'goal_ended'){clause}
              ORDER BY created_at"
        );
        let mut q = sqlx::query_as::<_, FactRow>(sqlx::AssertSqlSafe(sql));
        for bind in &binds {
            q = q.bind(bind);
        }
        let rows = q.fetch_all(self.r()).await?;

        let bucket = Bucket::for_span(filter.since);
        let mut totals = WorkTotals::default();
        let mut lead_times: Vec<f64> = Vec::new();
        let mut buckets: BTreeMap<String, WorkBucket> = BTreeMap::new();
        let mut span: Option<(DateTime<Utc>, DateTime<Utc>)> = None;

        for row in &rows {
            let Ok(created_at) = DateTime::parse_from_rfc3339(&row.created_at) else {
                continue;
            };
            let created_at = created_at.with_timezone(&Utc);
            span = Some(match span {
                None => (created_at, created_at),
                Some((min, max)) => (min.min(created_at), max.max(created_at)),
            });
            let data: serde_json::Value = serde_json::from_str(&row.data).unwrap_or_default();
            let status = data["status"].as_str().unwrap_or_default();
            let start = bucket.start_of(created_at);
            match (row.kind.as_str(), status) {
                ("task_ended", "finished") => {
                    totals.tasks_finished += 1;
                    bump(&mut buckets, &start).tasks_finished += 1;
                    if matches!(
                        data["landing"].as_str(),
                        Some("merge") | Some("pull_request")
                    ) {
                        totals.landed += 1;
                        bump(&mut buckets, &start).landed += 1;
                    }
                }
                ("task_ended", "failed") => {
                    totals.tasks_failed += 1;
                    bump(&mut buckets, &start).tasks_failed += 1;
                }
                ("task_ended", "cancelled") => {
                    totals.tasks_cancelled += 1;
                    bump(&mut buckets, &start).tasks_cancelled += 1;
                }
                ("goal_ended", "completed") => {
                    totals.goals_completed += 1;
                    bump(&mut buckets, &start).goals_completed += 1;
                    if let Some(secs) = data["lead_time_secs"].as_f64() {
                        lead_times.push(secs);
                    }
                }
                ("goal_ended", "cancelled") => {
                    totals.goals_cancelled += 1;
                }
                _ => {}
            }
        }

        totals.median_goal_lead_time_secs = median(&lead_times);
        let ended = totals.tasks_finished + totals.tasks_failed + totals.tasks_cancelled;
        totals.finish_rate = if ended == 0 {
            0.0
        } else {
            totals.tasks_finished as f64 / ended as f64
        };

        let buckets = match span {
            Some((min, max)) => filled_buckets(bucket, min, max, buckets),
            None => Vec::new(),
        };

        Ok(WorkStats {
            totals,
            bucket: match bucket {
                Bucket::Day => "day".to_string(),
                Bucket::Week => "week".to_string(),
            },
            buckets,
        })
    }
}

/// The bucket of `start`, creating it zeroed if this is the first fact seen
/// there.
fn bump<'a>(buckets: &'a mut BTreeMap<String, WorkBucket>, start: &str) -> &'a mut WorkBucket {
    buckets
        .entry(start.to_string())
        .or_insert_with(|| WorkBucket {
            start: start.to_string(),
            ..Default::default()
        })
}

/// Every bucket from `min` to `max`, zeros included where `facts` holds
/// nothing for it.
fn filled_buckets(
    bucket: Bucket,
    min: DateTime<Utc>,
    max: DateTime<Utc>,
    mut facts: BTreeMap<String, WorkBucket>,
) -> Vec<WorkBucket> {
    let mut day = min.date_naive();
    let last = max.date_naive();
    while day <= last {
        let at = day
            .and_hms_opt(0, 0, 0)
            .expect("midnight is a time")
            .and_utc();
        let start = bucket.start_of(at);
        facts.entry(start.clone()).or_insert_with(|| WorkBucket {
            start,
            ..Default::default()
        });
        day += Duration::days(1);
    }
    facts.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_store() -> (Store, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("test.db")).await.unwrap();
        (store, dir)
    }

    fn at(moment: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(moment)
            .unwrap()
            .with_timezone(&Utc)
    }

    fn fact(kind: &str, created_at: &str, data: serde_json::Value) -> NewFact {
        NewFact {
            kind: kind.to_string(),
            created_at: created_at.to_string(),
            repo_id: None,
            data,
        }
    }

    struct NewFact {
        kind: String,
        created_at: String,
        repo_id: Option<String>,
        data: serde_json::Value,
    }

    async fn seed(store: &Store, facts: Vec<NewFact>) {
        for f in facts {
            sqlx::query(
                "INSERT INTO stat_facts (id, kind, created_at, repo_id, goal_id, task_id,
                                          session_id, launch_id, seat, model, effort, skills, data)
                 VALUES (?, ?, ?, ?, NULL, NULL, NULL, NULL, NULL, NULL, NULL, '[]', ?)",
            )
            .bind(ariadne_core::id::new_id())
            .bind(&f.kind)
            .bind(&f.created_at)
            .bind(&f.repo_id)
            .bind(f.data.to_string())
            .execute(store.w())
            .await
            .unwrap();
        }
    }

    /// `work_stats` counts a finished, a failed and a cancelled task, a
    /// completed goal and a landed change, each once.
    #[tokio::test]
    async fn work_stats_counts_tasks_finished_failed_cancelled_and_goals_and_landed() {
        let (store, _dir) = test_store().await;
        seed(
            &store,
            vec![
                fact(
                    "task_ended",
                    "2026-10-01T12:00:00Z",
                    serde_json::json!({"status": "finished", "landing": "merge", "lead_time_secs": 10}),
                ),
                fact(
                    "task_ended",
                    "2026-10-01T13:00:00Z",
                    serde_json::json!({"status": "failed", "landing": "merge"}),
                ),
                fact(
                    "task_ended",
                    "2026-10-01T14:00:00Z",
                    serde_json::json!({"status": "cancelled", "landing": "none"}),
                ),
                fact(
                    "goal_ended",
                    "2026-10-01T15:00:00Z",
                    serde_json::json!({"status": "completed", "lead_time_secs": 100}),
                ),
            ],
        )
        .await;

        let filter = StatsFilter {
            since: Some(at("2026-09-25T00:00:00Z")),
            repo_id: None,
        };
        let stats = store.work_stats(&filter).await.unwrap();
        assert_eq!(stats.totals.tasks_finished, 1);
        assert_eq!(stats.totals.tasks_failed, 1);
        assert_eq!(stats.totals.tasks_cancelled, 1);
        assert_eq!(stats.totals.goals_completed, 1);
        assert_eq!(stats.totals.landed, 1);
        assert_eq!(stats.totals.median_goal_lead_time_secs, 100.0);
        assert_eq!(stats.totals.finish_rate, 1.0 / 3.0);
        assert_eq!(stats.bucket, "day");
        assert_eq!(stats.buckets.len(), 1);
        let bucket = &stats.buckets[0];
        assert_eq!(bucket.start, "2026-10-01T00:00:00Z");
        assert_eq!(bucket.tasks_finished, 1);
        assert_eq!(bucket.tasks_failed, 1);
        assert_eq!(bucket.tasks_cancelled, 1);
        assert_eq!(bucket.goals_completed, 1);
        assert_eq!(bucket.landed, 1);
    }

    /// A span of 31 days or less buckets by day and includes every day in
    /// between, zeros included.
    #[tokio::test]
    async fn work_stats_buckets_by_day_under_a_short_since() {
        let (store, _dir) = test_store().await;
        seed(
            &store,
            vec![
                fact(
                    "task_ended",
                    "2026-10-01T12:00:00Z",
                    serde_json::json!({"status": "finished", "landing": "none"}),
                ),
                fact(
                    "task_ended",
                    "2026-10-03T12:00:00Z",
                    serde_json::json!({"status": "finished", "landing": "none"}),
                ),
            ],
        )
        .await;

        let short = StatsFilter {
            since: Some(at("2026-09-20T00:00:00Z")),
            repo_id: None,
        };
        let stats = store.work_stats(&short).await.unwrap();
        assert_eq!(stats.bucket, "day");
        // 2026-10-01 through 2026-10-03: three daily buckets, the middle one
        // zeroed.
        assert_eq!(stats.buckets.len(), 3);
        assert_eq!(stats.buckets[0].start, "2026-10-01T00:00:00Z");
        assert_eq!(stats.buckets[0].tasks_finished, 1);
        assert_eq!(stats.buckets[1].start, "2026-10-02T00:00:00Z");
        assert_eq!(stats.buckets[1].tasks_finished, 0);
        assert_eq!(stats.buckets[2].start, "2026-10-03T00:00:00Z");
        assert_eq!(stats.buckets[2].tasks_finished, 1);
    }

    /// With no `since` at all, the aggregate buckets by week, each starting
    /// on the Monday of the fact's week, zeros included for a week with
    /// nothing in it.
    #[tokio::test]
    async fn work_stats_buckets_by_week_with_no_since_from_the_monday_of_each_week() {
        let (store, _dir) = test_store().await;
        seed(
            &store,
            vec![
                // 2026-09-21 is a Monday; 2026-10-05 is the Monday two weeks
                // after it, with an empty week (starting 2026-09-28) between
                // them.
                fact(
                    "task_ended",
                    "2026-09-21T10:00:00Z",
                    serde_json::json!({"status": "finished", "landing": "none"}),
                ),
                fact(
                    "task_ended",
                    "2026-10-05T10:00:00Z",
                    serde_json::json!({"status": "finished", "landing": "none"}),
                ),
            ],
        )
        .await;

        let stats = store.work_stats(&StatsFilter::default()).await.unwrap();
        assert_eq!(stats.bucket, "week");
        assert_eq!(stats.buckets.len(), 3);
        assert_eq!(stats.buckets[0].start, "2026-09-21T00:00:00Z");
        assert_eq!(stats.buckets[0].tasks_finished, 1);
        assert_eq!(stats.buckets[1].start, "2026-09-28T00:00:00Z");
        assert_eq!(stats.buckets[1].tasks_finished, 0);
        assert_eq!(stats.buckets[2].start, "2026-10-05T00:00:00Z");
        assert_eq!(stats.buckets[2].tasks_finished, 1);
    }

    /// `since` and `repo_id` each narrow what is counted.
    #[tokio::test]
    async fn work_stats_honours_since_and_repo_id() {
        let (store, _dir) = test_store().await;
        seed(
            &store,
            vec![
                NewFact {
                    kind: "task_ended".into(),
                    created_at: "2026-09-01T00:00:00Z".into(),
                    repo_id: Some("01REPO_A".into()),
                    data: serde_json::json!({"status": "finished", "landing": "none"}),
                },
                NewFact {
                    kind: "task_ended".into(),
                    created_at: "2026-10-01T00:00:00Z".into(),
                    repo_id: Some("01REPO_B".into()),
                    data: serde_json::json!({"status": "finished", "landing": "none"}),
                },
            ],
        )
        .await;

        let since = StatsFilter {
            since: Some(at("2026-09-15T00:00:00Z")),
            repo_id: None,
        };
        assert_eq!(
            store
                .work_stats(&since)
                .await
                .unwrap()
                .totals
                .tasks_finished,
            1
        );

        let repo = StatsFilter {
            since: None,
            repo_id: Some("01REPO_A".into()),
        };
        assert_eq!(
            store.work_stats(&repo).await.unwrap().totals.tasks_finished,
            1
        );
    }
}
