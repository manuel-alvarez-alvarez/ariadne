//! The stats ledger: facts the daemon writes as things happen, and the
//! aggregates the stats are read with.
//!
//! A fact is appended once and never changed. Every aggregate reads
//! `stat_facts` and nothing else, so a stats read costs the facts it counts and
//! never a scan of `agent_events` — see `migrations/0001_init.sql`.
//!
//! This file holds what every family shares: the filter, the writers, the
//! `WHERE` clause of the filter, the median and the p90, and the bucket a fact
//! falls in on a time axis. Each family answers one question from a file of
//! its own.

mod attention;
mod models;
mod spend;
mod time;
mod work;

pub use attention::AttentionStats;
pub use models::{ModelStat, ModelStats};
pub use spend::{ModelSpend, PerFinishedTask, SpendBucket, SpendStats, SpendTotals};
pub use time::{LeadTime, PersonWait, StatusTime, TimeStats};
pub use work::WorkStats;

use std::collections::HashMap;

use ariadne_core::id::new_id;
use chrono::{DateTime, Datelike, Duration, NaiveTime, SecondsFormat, Timelike, Utc};

use crate::{Result, Store, StoreError, now};

use crate::AgentSession;

/// One fact on its way into the ledger. The store gives it its id and its
/// `created_at`.
#[derive(Debug, Clone, PartialEq)]
pub struct NewStatFact {
    pub kind: String,
    pub repo_id: Option<String>,
    pub goal_id: Option<String>,
    pub task_id: Option<String>,
    pub session_id: Option<String>,
    pub launch_id: Option<String>,
    pub seat: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    /// The skills the agent behind the fact loaded, in the order it loaded
    /// them.
    pub skills: Vec<String>,
    /// What the fact says, a JSON object whose keys the kind decides.
    pub data: serde_json::Value,
}

/// What every aggregate is narrowed by. Nothing set is every fact there is.
#[derive(Debug, Clone, Default)]
pub struct StatsFilter {
    /// Only facts written at or after this moment.
    pub since: Option<DateTime<Utc>>,
    /// Only facts about this repository.
    pub repo_id: Option<String>,
}

impl Store {
    /// Append one fact to the ledger.
    pub async fn record_fact(&self, fact: NewStatFact) -> Result<()> {
        let skills = serde_json::to_string(&fact.skills).expect("a list of names serializes");
        sqlx::query(
            "INSERT INTO stat_facts (id, kind, created_at, repo_id, goal_id, task_id, session_id,
                                     launch_id, seat, model, effort, skills, data)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(new_id())
        .bind(&fact.kind)
        .bind(now())
        .bind(&fact.repo_id)
        .bind(&fact.goal_id)
        .bind(&fact.task_id)
        .bind(&fact.session_id)
        .bind(&fact.launch_id)
        .bind(&fact.seat)
        .bind(&fact.model)
        .bind(&fact.effort)
        .bind(skills)
        .bind(fact.data.to_string())
        .execute(self.w())
        .await?;
        self.announce_fact(&fact).await
    }

    /// Append a fact unless the ledger already holds one of its kind for the
    /// same session and launch. Answers whether it was appended.
    ///
    /// A session is relaunched under its own id, so what one run of it did is
    /// told from the next by the launch. The check and the write are one
    /// statement on the one writer, so two callers that end the same run at
    /// once write one fact between them.
    pub async fn record_fact_once_per_launch(&self, fact: NewStatFact) -> Result<bool> {
        let skills = serde_json::to_string(&fact.skills).expect("a list of names serializes");
        let n = sqlx::query(
            "INSERT INTO stat_facts (id, kind, created_at, repo_id, goal_id, task_id, session_id,
                                     launch_id, seat, model, effort, skills, data)
             SELECT ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?
              WHERE NOT EXISTS (SELECT 1 FROM stat_facts
                                 WHERE kind = ? AND session_id IS ? AND launch_id IS ?)",
        )
        .bind(new_id())
        .bind(&fact.kind)
        .bind(now())
        .bind(&fact.repo_id)
        .bind(&fact.goal_id)
        .bind(&fact.task_id)
        .bind(&fact.session_id)
        .bind(&fact.launch_id)
        .bind(&fact.seat)
        .bind(&fact.model)
        .bind(&fact.effort)
        .bind(skills)
        .bind(fact.data.to_string())
        .bind(&fact.kind)
        .bind(&fact.session_id)
        .bind(&fact.launch_id)
        .execute(self.w())
        .await?
        .rows_affected();
        if n == 0 {
            return Ok(false);
        }
        self.announce_fact(&fact).await?;
        Ok(true)
    }

    /// Announce the session a fact is about once the fact is readable.
    ///
    /// The status write that ended the session was announced before the fact
    /// existed, so a client that refetched its stats on that event may hold
    /// the stats without it. This second announcement comes after the commit,
    /// and the stats a client refetches on it hold the fact. A fact about a
    /// session that is gone, or about none, announces nothing.
    async fn announce_fact(&self, fact: &NewStatFact) -> Result<()> {
        let Some(session_id) = &fact.session_id else {
            return Ok(());
        };
        match self.publish_session_update(session_id).await {
            Err(StoreError::NotFound { .. }) => Ok(()),
            other => other,
        }
    }
}

/// Write the fact that a session's attention flag was taken down. The caller
/// supplies the row as it stood at the clear, so a relaunch cannot change the
/// run the fact names before the ledger sees it.
pub(crate) async fn record_attention_clear(
    store: &Store,
    session: AgentSession,
    reason: String,
    attention_since: String,
) -> Result<()> {
    let wait_secs = DateTime::parse_from_rfc3339(&attention_since)
        .map(|since| {
            (Utc::now() - since.with_timezone(&Utc))
                .num_seconds()
                .max(0)
        })
        .unwrap_or(0);
    let fact = attention_fact(
        store,
        session,
        serde_json::json!({
            "reason": reason,
            "wait_secs": wait_secs,
        }),
    )
    .await?;
    store.record_fact(fact).await
}

/// The session columns a store-owned attention fact carries. This is the
/// store counterpart of the daemon's session fact helper: clears originate in
/// the store, before a daemon caller can omit one of their paths.
async fn attention_fact(
    store: &Store,
    session: AgentSession,
    data: serde_json::Value,
) -> Result<NewStatFact> {
    let repo_id = match (&session.task_id, &session.goal_id) {
        (Some(task_id), _) => Some(store.get_task(task_id).await?.repo_id),
        (None, Some(goal_id)) => match store.list_goal_repositories(goal_id).await?.as_slice() {
            [only] => Some(only.id.clone()),
            _ => None,
        },
        (None, None) => None,
    };
    let skills = match &session.task_agent_id {
        Some(agent_id) => store
            .agent_skills(agent_id)
            .await?
            .into_iter()
            .map(|skill| skill.name)
            .collect(),
        None => Vec::new(),
    };
    Ok(NewStatFact {
        kind: "attention".into(),
        repo_id,
        goal_id: session.goal_id,
        task_id: session.task_id,
        session_id: Some(session.id),
        launch_id: session.launch_id,
        seat: session.seat,
        model: Some(session.model),
        effort: session.effort,
        skills,
        data,
    })
}

/// The clauses a filter adds to a `WHERE`, and the values they bind, in order.
/// The clauses are literals; every value goes in as a binding.
fn narrowed(filter: &StatsFilter) -> (String, Vec<String>) {
    let mut sql = String::new();
    let mut binds = Vec::new();
    if let Some(since) = filter.since {
        // The stored format, so the text compares in time order.
        sql.push_str(" AND created_at >= ?");
        binds.push(since.to_rfc3339_opts(SecondsFormat::Millis, true));
    }
    if let Some(repo_id) = &filter.repo_id {
        sql.push_str(" AND repo_id = ?");
        binds.push(repo_id.clone());
    }
    (sql, binds)
}

/// The step of a time axis: one bar per hour over a very short span, one per
/// day over a short one, one per week over a long one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bucket {
    Hour,
    Day,
    Week,
}

impl Bucket {
    /// The longest span drawn an hour at a time.
    const HOURLY_SPAN_DAYS: i64 = 2;
    /// The longest span drawn a day at a time.
    const DAILY_SPAN_DAYS: i64 = 31;

    /// The bucket of a span that starts at `start` and runs to `now`: `Hour`
    /// where `start` is 2 days or less back, `Day` where it is 31 days or
    /// less back, `Week` where it is further back or absent.
    fn for_span_at(start: Option<DateTime<Utc>>, now: DateTime<Utc>) -> Bucket {
        match start {
            Some(start) if now - start <= Duration::days(Self::HOURLY_SPAN_DAYS) => Bucket::Hour,
            Some(start) if now - start <= Duration::days(Self::DAILY_SPAN_DAYS) => Bucket::Day,
            _ => Bucket::Week,
        }
    }

    /// `created_at`, as the moment its bucket starts: the top of its hour,
    /// midnight UTC of its day, or of the Monday of its week.
    fn moment_start_of(self, created_at: DateTime<Utc>) -> DateTime<Utc> {
        let day = created_at.date_naive();
        let date = match self {
            Bucket::Hour | Bucket::Day => day,
            Bucket::Week => day - Duration::days(day.weekday().num_days_from_monday().into()),
        };
        let time = match self {
            Bucket::Hour => NaiveTime::from_hms_opt(created_at.hour(), 0, 0)
                .expect("the hour of a moment is a valid hour"),
            Bucket::Day | Bucket::Week => {
                NaiveTime::from_hms_opt(0, 0, 0).expect("midnight is a time")
            }
        };
        date.and_time(time).and_utc()
    }

    /// The RFC 3339 start of the bucket `created_at` falls in.
    fn start_of(self, created_at: DateTime<Utc>) -> String {
        self.moment_start_of(created_at)
            .to_rfc3339_opts(SecondsFormat::Secs, true)
    }

    /// The step from one bucket's start to the next.
    fn step(self) -> Duration {
        match self {
            Bucket::Hour => Duration::hours(1),
            Bucket::Day => Duration::days(1),
            Bucket::Week => Duration::weeks(1),
        }
    }

    /// `"hour"`, `"day"` or `"week"`, as `WorkStats::bucket` reads it.
    fn as_str(self) -> &'static str {
        match self {
            Bucket::Hour => "hour",
            Bucket::Day => "day",
            Bucket::Week => "week",
        }
    }
}

/// Every bucket's RFC 3339 start from the one `start` falls in to the one
/// `end` falls in, in order.
fn bucket_starts(bucket: Bucket, start: DateTime<Utc>, end: DateTime<Utc>) -> Vec<String> {
    let mut cursor = bucket.moment_start_of(start);
    let end = bucket.moment_start_of(end);
    let step = bucket.step();
    let mut starts = Vec::new();
    while cursor <= end {
        starts.push(cursor.to_rfc3339_opts(SecondsFormat::Secs, true));
        cursor += step;
    }
    starts
}

/// Every bucket from the one `start` falls in to the one `end` falls in,
/// zeros included where `facts` holds nothing for it: the time axis
/// `work_stats` and `spend_stats` each zero-fill their own bucket rows onto.
/// `set_start` writes a fresh or a kept row's `start` field to the bucket's
/// own, since a zeroed row begins with none.
fn filled_buckets<T: Default>(
    bucket: Bucket,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    mut facts: HashMap<String, T>,
    set_start: impl Fn(&mut T, String),
) -> Vec<T> {
    bucket_starts(bucket, start, end)
        .into_iter()
        .map(|start| {
            let mut row = facts.remove(&start).unwrap_or_default();
            set_start(&mut row, start);
            row
        })
        .collect()
}

/// The median of `values`: the middle one, or the mean of the two middle
/// ones where the count is even. 0 for none.
fn median(values: &[f64]) -> f64 {
    let sorted = sorted(values);
    let middle = sorted.len() / 2;
    match sorted.len() {
        0 => 0.0,
        len if len.is_multiple_of(2) => (sorted[middle - 1] + sorted[middle]) / 2.0,
        _ => sorted[middle],
    }
}

/// The 90th percentile of `values` by nearest rank: the smallest value that
/// at least 90% of them are at or below. 0 for none.
fn p90(values: &[f64]) -> f64 {
    let sorted = sorted(values);
    match sorted.len() {
        0 => 0.0,
        len => sorted[(len * 90).div_ceil(100) - 1],
    }
}

fn sorted(values: &[f64]) -> Vec<f64> {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    sorted
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(moment: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(moment)
            .unwrap()
            .with_timezone(&Utc)
    }

    /// A span of 2 days or less is drawn by the hour, one of 31 days or less
    /// by the day, and a longer one, or none at all, by the week.
    #[test]
    fn a_short_span_is_bucketed_by_hour_a_medium_one_by_day_and_a_long_one_by_week() {
        let now = at("2026-10-04T12:00:00Z");
        let back_hours = |hours| Some(now - Duration::hours(hours));
        let back_days = |days| Some(now - Duration::days(days));
        assert_eq!(Bucket::for_span_at(back_hours(1), now), Bucket::Hour);
        assert_eq!(Bucket::for_span_at(back_days(2), now), Bucket::Hour);
        assert_eq!(Bucket::for_span_at(back_hours(49), now), Bucket::Day);
        assert_eq!(Bucket::for_span_at(back_days(31), now), Bucket::Day);
        assert_eq!(Bucket::for_span_at(back_days(32), now), Bucket::Week);
        assert_eq!(Bucket::for_span_at(None, now), Bucket::Week);
    }

    /// A fact falls in the hour it was written, in its day, or in the week
    /// that starts on the Monday before it, each at its own top: the hour,
    /// midnight UTC, or midnight UTC of that Monday.
    #[test]
    fn a_fact_falls_in_its_hour_its_day_or_its_week_from_monday() {
        // 2026-10-04 is a Sunday; its week starts on Monday 2026-09-28.
        let sunday = at("2026-10-04T23:59:59Z");
        assert_eq!(Bucket::Hour.start_of(sunday), "2026-10-04T23:00:00Z");
        assert_eq!(Bucket::Day.start_of(sunday), "2026-10-04T00:00:00Z");
        assert_eq!(Bucket::Week.start_of(sunday), "2026-09-28T00:00:00Z");
        let monday = at("2026-09-28T00:00:00Z");
        assert_eq!(Bucket::Week.start_of(monday), "2026-09-28T00:00:00Z");
    }

    /// The axis runs from the bucket `start` falls in to the one `end` falls
    /// in, a bucket a step apart, inclusive of both ends.
    #[test]
    fn bucket_starts_runs_from_the_first_bucket_to_the_last_inclusive() {
        let start = at("2026-10-04T10:30:00Z");
        let end = at("2026-10-04T12:05:00Z");
        assert_eq!(
            bucket_starts(Bucket::Hour, start, end),
            [
                "2026-10-04T10:00:00Z",
                "2026-10-04T11:00:00Z",
                "2026-10-04T12:00:00Z",
            ]
        );
    }

    /// Every bucket of the axis is in the answer, a kept fact's own row
    /// carried through and a missing one zeroed with its own start.
    #[test]
    fn filled_buckets_zero_fills_every_bucket_the_facts_do_not_reach() {
        let start = at("2026-10-04T10:00:00Z");
        let end = at("2026-10-04T12:00:00Z");
        let mut facts = HashMap::new();
        facts.insert("2026-10-04T11:00:00Z".to_string(), 7u32);
        let filled = filled_buckets(Bucket::Hour, start, end, facts, |_, _| {});
        assert_eq!(filled, [0, 7, 0]);
    }

    /// `since` and `repo_id` each add a clause, and their values go in as
    /// bindings in the same order.
    #[test]
    fn the_filter_narrows_by_moment_and_repository() {
        let filter = StatsFilter {
            since: Some(at("2026-10-04T12:00:00Z")),
            repo_id: Some("01REPO".into()),
        };
        let (sql, binds) = narrowed(&filter);
        assert_eq!(sql, " AND created_at >= ? AND repo_id = ?");
        assert_eq!(binds, ["2026-10-04T12:00:00.000Z", "01REPO"]);
        assert_eq!(narrowed(&StatsFilter::default()), (String::new(), vec![]));
    }

    /// The median averages the two middle values of an even count.
    #[test]
    fn the_median_averages_the_two_middle_values() {
        assert_eq!(median(&[30.0, 10.0, 100.0, 20.0]), 25.0);
        assert_eq!(median(&[3.0, 1.0, 2.0]), 2.0);
        assert_eq!(median(&[]), 0.0);
    }

    /// The p90 is the nearest-rank value.
    #[test]
    fn the_p90_is_the_nearest_rank_value() {
        assert_eq!(p90(&[10.0, 20.0, 30.0, 100.0]), 100.0);
        let tens: Vec<f64> = (1..=10).map(|n| f64::from(n) * 10.0).collect();
        assert_eq!(p90(&tens), 90.0);
        assert_eq!(p90(&[]), 0.0);
    }
}
