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
mod tools;
mod work;

pub use attention::AttentionStats;
pub use models::ModelStats;
pub use spend::SpendStats;
pub use time::TimeStats;
pub use tools::ToolStats;
pub use work::WorkStats;

use ariadne_core::id::new_id;
use chrono::{DateTime, Datelike, Duration, SecondsFormat, Utc};

use crate::{Result, Store, StoreError, now};

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

/// The clauses a filter adds to a `WHERE`, and the values they bind, in order.
/// The clauses are literals; every value goes in as a binding.
#[allow(
    dead_code,
    reason = "the six family tasks use it, and a later task removes this attribute once they land"
)]
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

/// The step of a time axis: one bar per day over a short span, one per week
/// over a long one.
#[allow(
    dead_code,
    reason = "the six family tasks use it, and a later task removes this attribute once they land"
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Bucket {
    Day,
    Week,
}

#[allow(
    dead_code,
    reason = "the six family tasks use it, and a later task removes this attribute once they land"
)]
impl Bucket {
    /// The longest span drawn a day at a time.
    const DAILY_SPAN_DAYS: i64 = 31;

    /// The bucket of a span that starts at `since`: `Day` where `since` is 31
    /// days or less back from now, `Week` where it is further back or absent.
    fn for_span(since: Option<DateTime<Utc>>) -> Bucket {
        Self::for_span_at(since, Utc::now())
    }

    /// [`Bucket::for_span`], read against `now`.
    fn for_span_at(since: Option<DateTime<Utc>>, now: DateTime<Utc>) -> Bucket {
        match since {
            Some(since) if now - since <= Duration::days(Self::DAILY_SPAN_DAYS) => Bucket::Day,
            _ => Bucket::Week,
        }
    }

    /// The RFC 3339 start of the bucket `created_at` falls in: midnight UTC
    /// of its day, or of the Monday of its week.
    fn start_of(self, created_at: DateTime<Utc>) -> String {
        let day = created_at.date_naive();
        let start = match self {
            Bucket::Day => day,
            Bucket::Week => day - Duration::days(day.weekday().num_days_from_monday().into()),
        };
        start
            .and_hms_opt(0, 0, 0)
            .expect("midnight is a time")
            .and_utc()
            .to_rfc3339_opts(SecondsFormat::Secs, true)
    }
}

/// The median of `values`: the middle one, or the mean of the two middle
/// ones where the count is even. 0 for none.
#[allow(
    dead_code,
    reason = "the six family tasks use it, and a later task removes this attribute once they land"
)]
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
#[allow(
    dead_code,
    reason = "the six family tasks use it, and a later task removes this attribute once they land"
)]
fn p90(values: &[f64]) -> f64 {
    let sorted = sorted(values);
    match sorted.len() {
        0 => 0.0,
        len => sorted[(len * 90).div_ceil(100) - 1],
    }
}

#[allow(
    dead_code,
    reason = "the six family tasks use it, and a later task removes this attribute once they land"
)]
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

    /// A span of 31 days or less is drawn by the day; a longer one, or all
    /// time, by the week.
    #[test]
    fn a_short_span_is_bucketed_by_day_and_a_long_one_by_week() {
        let now = at("2026-10-04T12:00:00Z");
        let back = |days| Some(now - Duration::days(days));
        assert_eq!(Bucket::for_span_at(back(1), now), Bucket::Day);
        assert_eq!(Bucket::for_span_at(back(31), now), Bucket::Day);
        assert_eq!(Bucket::for_span_at(back(32), now), Bucket::Week);
        assert_eq!(Bucket::for_span_at(None, now), Bucket::Week);
    }

    /// A fact falls in the day it was written, or in the week that starts on
    /// the Monday before it, both at midnight UTC.
    #[test]
    fn a_fact_falls_in_its_day_or_its_week_from_monday() {
        // 2026-10-04 is a Sunday; its week starts on Monday 2026-09-28.
        let sunday = at("2026-10-04T23:59:59Z");
        assert_eq!(Bucket::Day.start_of(sunday), "2026-10-04T00:00:00Z");
        assert_eq!(Bucket::Week.start_of(sunday), "2026-09-28T00:00:00Z");
        let monday = at("2026-09-28T00:00:00Z");
        assert_eq!(Bucket::Week.start_of(monday), "2026-09-28T00:00:00Z");
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
