//! The stats ledger: facts the daemon writes as things happen, and the
//! aggregates the stats are read with.
//!
//! A fact is appended once and never changed. Every aggregate here reads
//! `stat_facts` and nothing else, so a stats read costs the facts it counts and
//! never a scan of `agent_events` — see `migrations/0001_init.sql`.

use std::collections::HashMap;

use ariadne_core::TokenUsage;
use ariadne_core::id::new_id;
use chrono::{DateTime, SecondsFormat, Utc};

use crate::usage::usage_of;
use crate::{Result, Store, StoreError, now};

/// The fact a session writes when one run of it ends.
const SESSION_ENDED: &str = "session_ended";

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

/// How one model did in one seat, over the `session_ended` facts the filter
/// keeps.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelStatRow {
    pub model: String,
    /// None for a loose session, which sits in no seat.
    pub seat: Option<String>,
    /// How many session runs ended.
    pub sessions: u64,
    /// How many of them ended `failed`.
    pub failed: u64,
    /// How many of them ended flagged `stalled`.
    pub stalled: u64,
    /// The tokens those runs spent, summed.
    pub usage: TokenUsage,
    /// `cached_input_tokens` over `input_tokens`, 0 where nothing went in.
    pub cached_share: f64,
    /// The mean of the runs' lifetimes, in seconds.
    pub mean_lifetime_secs: f64,
    /// Each skill those runs loaded, with how many runs loaded it. The most
    /// loaded first, and by name where two tie.
    pub skills: Vec<(String, u64)>,
}

/// One row of the main aggregate, in the order its `SELECT` names them.
type ModelSums = (
    String,
    Option<String>,
    i64,
    i64,
    i64,
    i64,
    i64,
    i64,
    Option<f64>,
);

/// Where a row's skill counts are gathered: its `(model, seat)`, and each
/// skill with how many runs loaded it.
type SkillCounts = HashMap<(String, Option<String>), Vec<(String, u64)>>;

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

    /// How each model did in each seat: one row per `(model, seat)` that a
    /// `session_ended` fact names, ordered by model and then by seat.
    pub async fn model_stats(&self, filter: &StatsFilter) -> Result<Vec<ModelStatRow>> {
        let (narrow, binds) = narrowed(filter);
        let mut q = sqlx::query_as::<_, ModelSums>(sqlx::AssertSqlSafe(format!(
            "SELECT model, seat, COUNT(*),
                    COALESCE(SUM(json_extract(data, '$.status') = 'failed'), 0),
                    COALESCE(SUM(json_extract(data, '$.attention_reason') = 'stalled'), 0),
                    COALESCE(SUM(json_extract(data, '$.input_tokens')), 0),
                    COALESCE(SUM(json_extract(data, '$.cached_input_tokens')), 0),
                    COALESCE(SUM(json_extract(data, '$.output_tokens')), 0),
                    AVG(json_extract(data, '$.lifetime_secs'))
               FROM stat_facts
              WHERE kind = ? AND model IS NOT NULL{narrow}
           GROUP BY model, seat
           ORDER BY model, seat"
        )))
        .bind(SESSION_ENDED);
        for bind in &binds {
            q = q.bind(bind);
        }
        let rows = q.fetch_all(self.r()).await?;

        let mut q = sqlx::query_as::<_, (String, Option<String>, String, i64)>(
            sqlx::AssertSqlSafe(format!(
                "SELECT model, seat, skill.value, COUNT(*)
                   FROM stat_facts, json_each(stat_facts.skills) AS skill
                  WHERE kind = ? AND model IS NOT NULL{narrow}
               GROUP BY model, seat, skill.value
               ORDER BY COUNT(*) DESC, skill.value"
            )),
        )
        .bind(SESSION_ENDED);
        for bind in &binds {
            q = q.bind(bind);
        }
        let mut skills = SkillCounts::new();
        for (model, seat, skill, count) in q.fetch_all(self.r()).await? {
            skills
                .entry((model, seat))
                .or_default()
                .push((skill, count.max(0) as u64));
        }

        Ok(rows
            .into_iter()
            .map(
                |(model, seat, sessions, failed, stalled, input, cached, output, lifetime)| {
                    let usage = usage_of((input, cached, output));
                    let skills = skills
                        .remove(&(model.clone(), seat.clone()))
                        .unwrap_or_default();
                    ModelStatRow {
                        model,
                        seat,
                        sessions: sessions.max(0) as u64,
                        failed: failed.max(0) as u64,
                        stalled: stalled.max(0) as u64,
                        cached_share: share(&usage),
                        usage,
                        mean_lifetime_secs: lifetime.unwrap_or(0.0),
                        skills,
                    }
                },
            )
            .collect())
    }
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

/// The cache's share of the input, as a fraction.
fn share(usage: &TokenUsage) -> f64 {
    match usage.input_tokens {
        0 => 0.0,
        input => (usage.cached_input_tokens as f64 / input as f64).min(1.0),
    }
}
