//! The `work` family: what got done.

use std::collections::HashMap;

use chrono::{DateTime, Utc};

use super::{Bucket, StatsFilter, filled_buckets, median, narrowed};
use crate::{Result, Store};

/// What `work_stats` answers: what got done, over the facts a filter keeps.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WorkStats {
    pub totals: WorkTotals,
    /// The bucket a [`WorkBucket`] falls on, `"hour"`, `"day"` or `"week"`.
    pub bucket: String,
    /// One row per bucket from `since`, or the first fact where there is
    /// none, to now, zeros included.
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

        let now = Utc::now();
        let first_fact = rows.iter().find_map(|row| {
            DateTime::parse_from_rfc3339(&row.created_at)
                .ok()
                .map(|dt| dt.with_timezone(&Utc))
        });
        let bucket = Bucket::for_span_at(filter.since.or(first_fact), now);
        let mut totals = WorkTotals::default();
        let mut lead_times: Vec<f64> = Vec::new();
        let mut buckets: HashMap<String, WorkBucket> = HashMap::new();

        for row in &rows {
            let Ok(created_at) = DateTime::parse_from_rfc3339(&row.created_at) else {
                continue;
            };
            let created_at = created_at.with_timezone(&Utc);
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

        let buckets = match filter.since.or(first_fact) {
            Some(start) => {
                filled_buckets(bucket, start, now, buckets, |row, start| row.start = start)
            }
            None => Vec::new(),
        };

        Ok(WorkStats {
            totals,
            bucket: bucket.as_str().to_string(),
            buckets,
        })
    }
}

/// The bucket of `start`, creating it zeroed if this is the first fact seen
/// there.
fn bump<'a>(buckets: &'a mut HashMap<String, WorkBucket>, start: &str) -> &'a mut WorkBucket {
    buckets
        .entry(start.to_string())
        .or_insert_with(|| WorkBucket {
            start: start.to_string(),
            ..Default::default()
        })
}

#[cfg(test)]
mod tests {
    use chrono::Duration;

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
    }

    /// A `since` of a day or less buckets by the hour, filled with zeros from
    /// `since` to now — about 24 hourly buckets for a `since` of 24 hours.
    #[tokio::test]
    async fn work_stats_buckets_by_hour_under_a_since_of_a_day_or_less() {
        let (store, _dir) = test_store().await;
        let now = Utc::now();
        seed(
            &store,
            vec![fact(
                "task_ended",
                &(now - Duration::hours(2)).to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                serde_json::json!({"status": "finished", "landing": "none"}),
            )],
        )
        .await;

        let filter = StatsFilter {
            since: Some(now - Duration::hours(24)),
            repo_id: None,
        };
        let stats = store.work_stats(&filter).await.unwrap();
        assert_eq!(stats.bucket, "hour");
        assert!(
            (23..=26).contains(&stats.buckets.len()),
            "{:?}",
            stats.buckets.len()
        );
        let fact_bucket_start = Bucket::Hour.start_of(now - Duration::hours(2));
        let fact_bucket = stats
            .buckets
            .iter()
            .find(|b| b.start == fact_bucket_start)
            .expect("the fact's own hour has a bucket");
        assert_eq!(fact_bucket.tasks_finished, 1);
        assert!(
            stats.buckets.iter().any(|b| b.tasks_finished == 0),
            "{:?}",
            stats.buckets
        );
    }

    /// A `since` with no kept fact in it still fills every bucket from
    /// `since` to now, zeros throughout: the axis does not wait on a fact
    /// to exist.
    #[tokio::test]
    async fn work_stats_fills_buckets_from_since_to_now_with_no_kept_fact() {
        let (store, _dir) = test_store().await;
        let now = Utc::now();

        let filter = StatsFilter {
            since: Some(now - Duration::hours(3)),
            repo_id: None,
        };
        let stats = store.work_stats(&filter).await.unwrap();
        assert_eq!(stats.bucket, "hour");
        assert!(
            (3..=4).contains(&stats.buckets.len()),
            "{:?}",
            stats.buckets.len()
        );
        assert!(
            stats.buckets.iter().all(|b| b.tasks_finished == 0),
            "{:?}",
            stats.buckets
        );
    }

    /// A `since` of 7 days buckets by the day, filled with zeros from `since`
    /// to now — 7 or 8 daily buckets.
    #[tokio::test]
    async fn work_stats_buckets_by_day_under_a_since_of_seven_days() {
        let (store, _dir) = test_store().await;
        let now = Utc::now();
        seed(
            &store,
            vec![fact(
                "task_ended",
                &(now - Duration::days(3)).to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                serde_json::json!({"status": "finished", "landing": "none"}),
            )],
        )
        .await;

        let filter = StatsFilter {
            since: Some(now - Duration::days(7)),
            repo_id: None,
        };
        let stats = store.work_stats(&filter).await.unwrap();
        assert_eq!(stats.bucket, "day");
        assert!(
            (7..=8).contains(&stats.buckets.len()),
            "{:?}",
            stats.buckets.len()
        );
        assert!(
            stats.buckets.iter().any(|b| b.tasks_finished == 0),
            "{:?}",
            stats.buckets
        );
    }

    /// With no `since`, facts from the last 10 days bucket by the day, not
    /// by the week: a span this short stays under the day threshold.
    #[tokio::test]
    async fn work_stats_buckets_by_day_with_no_since_and_facts_within_ten_days() {
        let (store, _dir) = test_store().await;
        let now = Utc::now();
        seed(
            &store,
            vec![
                fact(
                    "task_ended",
                    &(now - Duration::days(9)).to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                    serde_json::json!({"status": "finished", "landing": "none"}),
                ),
                fact(
                    "task_ended",
                    &(now - Duration::days(1)).to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                    serde_json::json!({"status": "finished", "landing": "none"}),
                ),
            ],
        )
        .await;

        let stats = store.work_stats(&StatsFilter::default()).await.unwrap();
        assert_eq!(stats.bucket, "day");
        assert!(stats.buckets.len() > 1, "{:?}", stats.buckets);
    }

    /// With no `since`, facts spread over more than 60 days bucket by the
    /// week.
    #[tokio::test]
    async fn work_stats_buckets_by_week_with_no_since_and_facts_over_sixty_days() {
        let (store, _dir) = test_store().await;
        let now = Utc::now();
        seed(
            &store,
            vec![
                fact(
                    "task_ended",
                    &(now - Duration::days(65))
                        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                    serde_json::json!({"status": "finished", "landing": "none"}),
                ),
                fact(
                    "task_ended",
                    &(now - Duration::days(1)).to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                    serde_json::json!({"status": "finished", "landing": "none"}),
                ),
            ],
        )
        .await;

        let stats = store.work_stats(&StatsFilter::default()).await.unwrap();
        assert_eq!(stats.bucket, "week");
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
