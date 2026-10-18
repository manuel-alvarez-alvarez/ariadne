//! The `spend` family: what it spent — tokens over time, by model, and per
//! finished task. Never a cost: the ledger holds tokens alone.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};

use super::{Bucket, StatsFilter, filled_buckets, narrowed};
use crate::{Result, Store};

/// What the `spend` family answers: what did it spend?
#[derive(Debug, Clone, PartialEq)]
pub struct SpendStats {
    pub totals: SpendTotals,
    pub per_finished_task: PerFinishedTask,
    /// The step every bucket below is drawn at: `since` to now where there is
    /// one, the first kept fact to now otherwise.
    pub bucket: Bucket,
    /// One row per bucket from `since`, or the first fact where there is
    /// none, to now, zeros included.
    pub buckets: Vec<SpendBucket>,
    /// One row per model, every seat pooled, heaviest first.
    pub by_model: Vec<ModelSpend>,
}

/// Tokens spent across every `session_ended` fact the filter keeps.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SpendTotals {
    pub sessions: u64,
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub output_tokens: u64,
    /// `cached_input_tokens` over `input_tokens`; 0 where `input_tokens` is 0.
    pub cached_share: f64,
}

/// What a finished task spends, on average: the tokens of every session of a
/// task that finished, divided by how many tasks finished. 0 where none.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PerFinishedTask {
    pub tasks: u64,
    pub input_tokens: f64,
    pub output_tokens: f64,
}

/// What was spent in one bucket of the time axis.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SpendBucket {
    /// The RFC 3339 start of the bucket.
    pub start: String,
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub output_tokens: u64,
}

/// What one model spent, every seat that ran on it pooled together.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelSpend {
    pub model: String,
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub output_tokens: u64,
    /// `input_tokens + output_tokens` over the same total of every model.
    pub share: f64,
}

/// One `session_ended` fact's own usage, read out of the ledger.
struct SessionSpend {
    created_at: DateTime<Utc>,
    task_id: Option<String>,
    model: Option<String>,
    input_tokens: u64,
    cached_input_tokens: u64,
    output_tokens: u64,
}

impl Store {
    /// What it spent, over the facts the filter keeps.
    pub async fn spend_stats(&self, filter: &StatsFilter) -> Result<SpendStats> {
        let sessions = self.session_spends(filter).await?;
        let finished_tasks = self.finished_task_ids(filter).await?;
        let now = Utc::now();
        let first_fact = sessions.first().map(|s| s.created_at);
        let bucket = Bucket::for_span_at(filter.since.or(first_fact), now);

        Ok(SpendStats {
            totals: totals_of(&sessions),
            per_finished_task: per_finished_task_of(&sessions, &finished_tasks),
            bucket,
            buckets: buckets_of(&sessions, bucket, filter.since, now),
            by_model: by_model_of(&sessions),
        })
    }

    /// Every `session_ended` fact the filter keeps, oldest first.
    async fn session_spends(&self, filter: &StatsFilter) -> Result<Vec<SessionSpend>> {
        let (clause, binds) = narrowed(filter);
        let sql = format!(
            "SELECT created_at, task_id, model, data FROM stat_facts
              WHERE kind = 'session_ended'{clause} ORDER BY created_at"
        );
        let mut query = sqlx::query_as::<_, (String, Option<String>, Option<String>, String)>(
            sqlx::AssertSqlSafe(sql),
        );
        for bind in &binds {
            query = query.bind(bind);
        }
        let rows = query.fetch_all(self.r()).await?;
        Ok(rows
            .into_iter()
            .map(|(created_at, task_id, model, data)| {
                let data: serde_json::Value =
                    serde_json::from_str(&data).expect("fact data is a JSON object");
                SessionSpend {
                    created_at: parse_rfc3339(&created_at),
                    task_id,
                    model,
                    input_tokens: token_field(&data, "input_tokens"),
                    cached_input_tokens: token_field(&data, "cached_input_tokens"),
                    output_tokens: token_field(&data, "output_tokens"),
                }
            })
            .collect())
    }

    /// The ids of every task named by a `finished` `task_ended` fact the
    /// filter keeps.
    async fn finished_task_ids(&self, filter: &StatsFilter) -> Result<HashSet<String>> {
        let (clause, binds) = narrowed(filter);
        let sql = format!("SELECT task_id, data FROM stat_facts WHERE kind = 'task_ended'{clause}");
        let mut query = sqlx::query_as::<_, (Option<String>, String)>(sqlx::AssertSqlSafe(sql));
        for bind in &binds {
            query = query.bind(bind);
        }
        let rows = query.fetch_all(self.r()).await?;
        Ok(rows
            .into_iter()
            .filter_map(|(task_id, data)| {
                let data: serde_json::Value =
                    serde_json::from_str(&data).expect("fact data is a JSON object");
                match (task_id, data["status"].as_str()) {
                    (Some(task_id), Some("finished")) => Some(task_id),
                    _ => None,
                }
            })
            .collect())
    }
}

fn parse_rfc3339(moment: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(moment)
        .expect("stored as rfc3339")
        .with_timezone(&Utc)
}

fn token_field(data: &serde_json::Value, key: &str) -> u64 {
    data[key].as_u64().unwrap_or(0)
}

fn cached_share(input_tokens: u64, cached_input_tokens: u64) -> f64 {
    match input_tokens {
        0 => 0.0,
        input => cached_input_tokens as f64 / input as f64,
    }
}

fn totals_of(sessions: &[SessionSpend]) -> SpendTotals {
    let input_tokens: u64 = sessions.iter().map(|s| s.input_tokens).sum();
    let cached_input_tokens: u64 = sessions.iter().map(|s| s.cached_input_tokens).sum();
    let output_tokens: u64 = sessions.iter().map(|s| s.output_tokens).sum();
    SpendTotals {
        sessions: sessions.len() as u64,
        input_tokens,
        cached_input_tokens,
        output_tokens,
        cached_share: cached_share(input_tokens, cached_input_tokens),
    }
}

fn per_finished_task_of(sessions: &[SessionSpend], finished: &HashSet<String>) -> PerFinishedTask {
    let tasks = finished.len() as u64;
    if tasks == 0 {
        return PerFinishedTask::default();
    }
    let (input, output) = sessions
        .iter()
        .filter(|s| s.task_id.as_ref().is_some_and(|id| finished.contains(id)))
        .fold((0u64, 0u64), |(input, output), s| {
            (input + s.input_tokens, output + s.output_tokens)
        });
    PerFinishedTask {
        tasks,
        input_tokens: input as f64 / tasks as f64,
        output_tokens: output as f64 / tasks as f64,
    }
}

/// One row per bucket from `since`, or the first fact where there is none,
/// to `now`, zeros included. Empty where there is neither a `since` nor a
/// fact to draw an axis from.
fn buckets_of(
    sessions: &[SessionSpend],
    bucket: Bucket,
    since: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> Vec<SpendBucket> {
    let Some(start) = since.or_else(|| sessions.first().map(|s| s.created_at)) else {
        return Vec::new();
    };
    let mut buckets: HashMap<String, SpendBucket> = HashMap::new();
    for session in sessions {
        let entry = bump(&mut buckets, &bucket.start_of(session.created_at));
        entry.input_tokens += session.input_tokens;
        entry.cached_input_tokens += session.cached_input_tokens;
        entry.output_tokens += session.output_tokens;
    }

    filled_buckets(bucket, start, now, buckets, |row, start| row.start = start)
}

/// The bucket of `start`, creating it zeroed if this is the first fact seen
/// there.
fn bump<'a>(buckets: &'a mut HashMap<String, SpendBucket>, start: &str) -> &'a mut SpendBucket {
    buckets
        .entry(start.to_string())
        .or_insert_with(|| SpendBucket {
            start: start.to_string(),
            ..Default::default()
        })
}

/// One row per model, every seat pooled, heaviest (input plus output) first.
fn by_model_of(sessions: &[SessionSpend]) -> Vec<ModelSpend> {
    let mut totals: HashMap<String, (u64, u64, u64)> = HashMap::new();
    for session in sessions {
        let Some(model) = &session.model else {
            continue;
        };
        let entry = totals.entry(model.clone()).or_default();
        entry.0 += session.input_tokens;
        entry.1 += session.cached_input_tokens;
        entry.2 += session.output_tokens;
    }
    let grand_total: u64 = totals
        .values()
        .map(|(input, _, output)| input + output)
        .sum();
    let mut by_model: Vec<ModelSpend> = totals
        .into_iter()
        .map(
            |(model, (input_tokens, cached_input_tokens, output_tokens))| ModelSpend {
                model,
                input_tokens,
                cached_input_tokens,
                output_tokens,
                share: match grand_total {
                    0 => 0.0,
                    total => (input_tokens + output_tokens) as f64 / total as f64,
                },
            },
        )
        .collect();
    by_model.sort_by(|a, b| {
        (b.input_tokens + b.output_tokens).cmp(&(a.input_tokens + a.output_tokens))
    });
    by_model
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, SecondsFormat};

    use super::*;

    use crate::stats::NewStatFact;

    async fn store() -> (Store, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("test.db")).await.unwrap();
        (store, dir)
    }

    fn run(input: u64, cached: u64, output: u64) -> serde_json::Value {
        serde_json::json!({
            "status": "exited", "attention_reason": null, "lifetime_secs": 10,
            "turns": 1, "input_tokens": input, "cached_input_tokens": cached,
            "output_tokens": output,
        })
    }

    async fn ended(
        store: &Store,
        launch_id: &str,
        repo_id: &str,
        task_id: Option<&str>,
        model: &str,
        data: serde_json::Value,
    ) {
        ended_as(store, launch_id, repo_id, task_id, "agent", model, data).await;
    }

    /// The same, naming the seat the session ran in: what a model's row pools
    /// across.
    async fn ended_as(
        store: &Store,
        launch_id: &str,
        repo_id: &str,
        task_id: Option<&str>,
        seat: &str,
        model: &str,
        data: serde_json::Value,
    ) {
        store
            .record_fact(NewStatFact {
                kind: "session_ended".into(),
                repo_id: Some(repo_id.into()),
                goal_id: None,
                task_id: task_id.map(str::to_string),
                session_id: None,
                launch_id: Some(launch_id.into()),
                seat: Some(seat.into()),
                model: Some(model.into()),
                effort: None,
                skills: vec![],
                data,
            })
            .await
            .unwrap();
    }

    async fn task_ended(store: &Store, repo_id: &str, task_id: &str, status: &str) {
        store
            .record_fact(NewStatFact {
                kind: "task_ended".into(),
                repo_id: Some(repo_id.into()),
                goal_id: None,
                task_id: Some(task_id.into()),
                session_id: None,
                launch_id: None,
                seat: None,
                model: None,
                effort: None,
                skills: vec![],
                data: serde_json::json!({"status": status}),
            })
            .await
            .unwrap();
    }

    /// Move every fact whose launch names `launch_id` to `created_at`: the
    /// store writes it at the real time, and a bucket test needs facts spread
    /// over days or weeks it did not take to run.
    async fn backdate(dir: &std::path::Path, launch_id: &str, created_at: DateTime<Utc>) {
        let pool =
            sqlx::SqlitePool::connect(&format!("sqlite://{}", dir.join("test.db").display()))
                .await
                .unwrap();
        sqlx::query("UPDATE stat_facts SET created_at = ? WHERE launch_id = ?")
            .bind(created_at.to_rfc3339_opts(SecondsFormat::Millis, true))
            .bind(launch_id)
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;
    }

    fn at(moment: &str) -> DateTime<Utc> {
        parse_rfc3339(moment)
    }

    /// The totals sum every ended session's tokens, and the cached share is
    /// the cached input over the whole input; nothing ended answers zeros.
    #[tokio::test]
    async fn the_totals_sum_ended_sessions_and_compute_the_cached_share() {
        let (store, _dir) = store().await;
        assert_eq!(
            store
                .spend_stats(&StatsFilter::default())
                .await
                .unwrap()
                .totals,
            SpendTotals::default()
        );

        ended(&store, "l1", "01REPO", None, "stub:a", run(1000, 800, 100)).await;
        ended(&store, "l2", "01REPO", None, "stub:a", run(500, 0, 50)).await;

        let totals = store
            .spend_stats(&StatsFilter::default())
            .await
            .unwrap()
            .totals;
        assert_eq!(totals.sessions, 2);
        assert_eq!(totals.input_tokens, 1500);
        assert_eq!(totals.cached_input_tokens, 800);
        assert_eq!(totals.output_tokens, 150);
        assert!((totals.cached_share - 800.0 / 1500.0).abs() < 1e-9);
    }

    /// Every model's sessions are pooled across seats into one row, the three
    /// token counts carried whole, the heaviest model first and each row's
    /// share counted off the grand total of every model's input plus output.
    #[tokio::test]
    async fn models_are_pooled_across_seats_and_ranked_heaviest_first() {
        let (store, _dir) = store().await;
        ended_as(
            &store,
            "l1",
            "01REPO",
            None,
            "agent",
            "stub:heavy",
            run(900, 0, 100),
        )
        .await;
        ended_as(
            &store,
            "l2",
            "01REPO",
            None,
            "agent",
            "stub:light",
            run(90, 0, 10),
        )
        .await;
        ended_as(
            &store,
            "l3",
            "01REPO",
            None,
            "agent",
            "stub:heavy",
            run(0, 0, 0),
        )
        .await;

        let by_model = store
            .spend_stats(&StatsFilter::default())
            .await
            .unwrap()
            .by_model;
        assert_eq!(by_model.len(), 2);
        assert_eq!(by_model[0].model, "stub:heavy");
        assert_eq!(by_model[0].input_tokens, 900);
        assert_eq!(by_model[0].output_tokens, 100);
        assert!((by_model[0].share - 1000.0 / 1100.0).abs() < 1e-9);
        assert_eq!(by_model[1].model, "stub:light");
        assert!((by_model[1].share - 100.0 / 1100.0).abs() < 1e-9);
    }

    /// Every session of a task named by a `finished` `task_ended` fact counts
    /// toward the average; a task that failed, and a session naming no task,
    /// do not.
    #[tokio::test]
    async fn per_finished_task_averages_the_tokens_of_tasks_that_finished() {
        let (store, _dir) = store().await;
        assert_eq!(
            store
                .spend_stats(&StatsFilter::default())
                .await
                .unwrap()
                .per_finished_task,
            PerFinishedTask::default()
        );

        task_ended(&store, "01REPO", "01TASKA", "finished").await;
        task_ended(&store, "01REPO", "01TASKB", "finished").await;
        task_ended(&store, "01REPO", "01TASKC", "failed").await;
        ended(
            &store,
            "l1",
            "01REPO",
            Some("01TASKA"),
            "stub:a",
            run(1000, 0, 100),
        )
        .await;
        ended(
            &store,
            "l2",
            "01REPO",
            Some("01TASKB"),
            "stub:a",
            run(400, 0, 20),
        )
        .await;
        ended(
            &store,
            "l3",
            "01REPO",
            Some("01TASKC"),
            "stub:a",
            run(9000, 0, 9000),
        )
        .await;
        ended(&store, "l4", "01REPO", None, "stub:a", run(5000, 0, 5000)).await;

        let per_task = store
            .spend_stats(&StatsFilter::default())
            .await
            .unwrap()
            .per_finished_task;
        assert_eq!(per_task.tasks, 2);
        assert!((per_task.input_tokens - 700.0).abs() < 1e-9, "{per_task:?}");
        assert!((per_task.output_tokens - 60.0).abs() < 1e-9, "{per_task:?}");
    }

    /// A `since` of a day or less buckets by the hour, filled with zeros from
    /// `since` to now — about 24 hourly buckets for a `since` of 24 hours.
    #[tokio::test]
    async fn spend_stats_buckets_by_hour_under_a_since_of_a_day_or_less() {
        let (store, dir) = store().await;
        let now = Utc::now();
        ended(&store, "recent", "01REPO", None, "stub:a", run(100, 0, 0)).await;
        backdate(dir.path(), "recent", now - Duration::hours(2)).await;

        let filter = StatsFilter {
            since: Some(now - Duration::hours(24)),
            repo_id: None,
        };
        let stats = store.spend_stats(&filter).await.unwrap();
        assert_eq!(stats.bucket, Bucket::Hour);
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
        assert_eq!(fact_bucket.input_tokens, 100);
        assert!(
            stats.buckets.iter().any(|b| b.input_tokens == 0),
            "{:?}",
            stats.buckets
        );
    }

    /// A `since` with no kept fact in it still fills every bucket from
    /// `since` to now, zeros throughout: the axis does not wait on a fact
    /// to exist.
    #[tokio::test]
    async fn spend_stats_fills_buckets_from_since_to_now_with_no_kept_fact() {
        let (store, _dir) = store().await;
        let now = Utc::now();

        let filter = StatsFilter {
            since: Some(now - Duration::hours(3)),
            repo_id: None,
        };
        let stats = store.spend_stats(&filter).await.unwrap();
        assert_eq!(stats.bucket, Bucket::Hour);
        assert!(
            (3..=4).contains(&stats.buckets.len()),
            "{:?}",
            stats.buckets.len()
        );
        assert!(
            stats.buckets.iter().all(|b| b.input_tokens == 0),
            "{:?}",
            stats.buckets
        );
    }

    /// A `since` of 7 days buckets by the day, filled with zeros from `since`
    /// to now — 7 or 8 daily buckets.
    #[tokio::test]
    async fn spend_stats_buckets_by_day_under_a_since_of_seven_days() {
        let (store, dir) = store().await;
        let now = Utc::now();
        ended(&store, "recent", "01REPO", None, "stub:a", run(100, 0, 0)).await;
        backdate(dir.path(), "recent", now - Duration::days(3)).await;

        let filter = StatsFilter {
            since: Some(now - Duration::days(7)),
            repo_id: None,
        };
        let stats = store.spend_stats(&filter).await.unwrap();
        assert_eq!(stats.bucket, Bucket::Day);
        assert!(
            (7..=8).contains(&stats.buckets.len()),
            "{:?}",
            stats.buckets.len()
        );
        assert!(
            stats.buckets.iter().any(|b| b.input_tokens == 0),
            "{:?}",
            stats.buckets
        );
    }

    /// With no `since`, facts from the last 10 days bucket by the day, not
    /// by the week: a span this short stays under the day threshold.
    #[tokio::test]
    async fn spend_stats_buckets_by_day_with_no_since_and_facts_within_ten_days() {
        let (store, dir) = store().await;
        let now = Utc::now();
        ended(&store, "early", "01REPO", None, "stub:a", run(100, 0, 0)).await;
        ended(&store, "late", "01REPO", None, "stub:a", run(300, 0, 0)).await;
        backdate(dir.path(), "early", now - Duration::days(9)).await;
        backdate(dir.path(), "late", now - Duration::days(1)).await;

        let stats = store.spend_stats(&StatsFilter::default()).await.unwrap();
        assert_eq!(stats.bucket, Bucket::Day);
        assert!(stats.buckets.len() > 1, "{:?}", stats.buckets);
    }

    /// With no `since`, facts spread over more than 60 days bucket by the
    /// week.
    #[tokio::test]
    async fn spend_stats_buckets_by_week_with_no_since_and_facts_over_sixty_days() {
        let (store, dir) = store().await;
        let now = Utc::now();
        ended(&store, "early", "01REPO", None, "stub:a", run(100, 0, 0)).await;
        ended(&store, "late", "01REPO", None, "stub:a", run(300, 0, 0)).await;
        backdate(dir.path(), "early", now - Duration::days(65)).await;
        backdate(dir.path(), "late", now - Duration::days(1)).await;

        let stats = store.spend_stats(&StatsFilter::default()).await.unwrap();
        assert_eq!(stats.bucket, Bucket::Week);
    }

    /// `since` and `repo_id` each narrow every fact the aggregate reads: a
    /// fact outside either is in neither the totals nor the model rows.
    #[tokio::test]
    async fn since_and_repo_id_narrow_every_fact() {
        let (store, dir) = store().await;
        ended(&store, "old", "01REPO", None, "stub:a", run(1000, 0, 0)).await;
        backdate(dir.path(), "old", at("2020-01-01T00:00:00Z")).await;
        ended(
            &store,
            "other-repo",
            "01OTHER",
            None,
            "stub:a",
            run(2000, 0, 0),
        )
        .await;
        ended(&store, "kept", "01REPO", None, "stub:a", run(300, 0, 0)).await;

        let filter = StatsFilter {
            since: Some(at("2026-01-01T00:00:00Z")),
            repo_id: Some("01REPO".into()),
        };
        let totals = store.spend_stats(&filter).await.unwrap().totals;
        assert_eq!(totals.sessions, 1);
        assert_eq!(totals.input_tokens, 300);
    }
}
