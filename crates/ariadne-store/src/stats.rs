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

/// Aggregates of the review facts in the ledger.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ReviewStats {
    pub authors: Vec<AuthorReviewStatRow>,
    pub reviewers: Vec<ReviewerStatRow>,
    pub messages: Vec<MessageStatRow>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AuthorReviewStatRow {
    pub model: String,
    pub approvals: u64,
    pub mean_rounds: f64,
    pub median_rounds: f64,
    pub first_pass_rate: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReviewerStatRow {
    pub model: String,
    pub verdicts: u64,
    pub approve_share: f64,
    pub mean_latency_secs: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MessageStatRow {
    pub kind: String,
    pub from_actor: String,
    pub total: u64,
    pub mean_per_task: f64,
/// All aggregates over tool calls and permission answers in the selected
/// facts.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolStats {
    pub tools: Vec<ToolStatRow>,
    pub models: Vec<ToolModelStatRow>,
    pub permissions: Vec<PermissionStatRow>,
}

/// How often a tool ran and how long its completed calls took.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolStatRow {
    pub tool_name: String,
    pub calls: u64,
    pub errors: u64,
    pub median_duration_ms: f64,
    pub p90_duration_ms: f64,
}

/// How long tools took on one model.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolModelStatRow {
    pub model: String,
    pub calls: u64,
    pub mean_duration_ms: f64,
}

/// Permission answers, grouped by who answered and what the answer was.
#[derive(Debug, Clone, PartialEq)]
pub struct PermissionStatRow {
    pub decided_by: String,
    pub answer: String,
    pub permissions: u64,
    pub mean_wait_ms: f64,
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

    /// How authors, reviewers and the channel performed, from review facts only.
    pub async fn review_stats(&self, filter: &StatsFilter) -> Result<ReviewStats> {
        let (narrow, binds) = narrowed(filter);
        let mut verdicts = sqlx::query_as::<_, (Option<String>, String, i64, f64)>(sqlx::AssertSqlSafe(format!(
            "SELECT json_extract(data, '$.author_model'), json_extract(data, '$.verdict'),
                    json_extract(data, '$.round'), CAST(json_extract(data, '$.latency_secs') AS REAL)
               FROM stat_facts WHERE kind = 'verdict'{narrow} ORDER BY id"
        )));
        for bind in &binds {
            verdicts = verdicts.bind(bind);
        }
        let verdicts = verdicts.fetch_all(self.r()).await?;
        let mut authors: HashMap<String, Vec<i64>> = HashMap::new();
        let mut reviewers: HashMap<String, Vec<(String, f64)>> = HashMap::new();
        for (author, verdict, round, _latency) in verdicts {
            if verdict == "approve"
                && let Some(author) = author
            {
                authors.entry(author).or_default().push(round);
            }
            // The fact's model is the reviewer model.
            // Re-read it below with the same narrow predicate to preserve the ledger-only rule.
        }
        let mut reviewer_rows = sqlx::query_as::<_, (String, String, f64)>(sqlx::AssertSqlSafe(format!(
            "SELECT model, json_extract(data, '$.verdict'), CAST(json_extract(data, '$.latency_secs') AS REAL)
               FROM stat_facts WHERE kind = 'verdict' AND model IS NOT NULL{narrow} ORDER BY model"
        )));
        for bind in &binds {
            reviewer_rows = reviewer_rows.bind(bind);
        }
        for (model, verdict, latency) in reviewer_rows.fetch_all(self.r()).await? {
            reviewers.entry(model).or_default().push((verdict, latency));
        }
        let mut author_rows: Vec<_> = authors
            .into_iter()
            .map(|(model, mut rounds)| {
                rounds.sort_unstable();
                let n = rounds.len() as f64;
                let median = if rounds.len() % 2 == 0 {
                    (rounds[rounds.len() / 2 - 1] + rounds[rounds.len() / 2]) as f64 / 2.0
                } else {
                    rounds[rounds.len() / 2] as f64
                };
                AuthorReviewStatRow {
                    model,
                    approvals: rounds.len() as u64,
                    mean_rounds: rounds.iter().sum::<i64>() as f64 / n,
                    median_rounds: median,
                    first_pass_rate: rounds.iter().filter(|&&r| r == 1).count() as f64 / n,
                }
            })
            .collect();
        author_rows.sort_by(|a, b| a.model.cmp(&b.model));
        let mut reviewer_rows: Vec<_> = reviewers
            .into_iter()
            .map(|(model, rows)| {
                let n = rows.len() as f64;
                ReviewerStatRow {
                    model,
                    verdicts: rows.len() as u64,
                    approve_share: rows.iter().filter(|(v, _)| v == "approve").count() as f64 / n,
                    mean_latency_secs: rows.iter().map(|(_, l)| l).sum::<f64>() / n,
                }
            })
            .collect();
        reviewer_rows.sort_by(|a, b| a.model.cmp(&b.model));
        let mut messages = sqlx::query_as::<_, (String, String, i64, f64)>(sqlx::AssertSqlSafe(format!(
            "SELECT json_extract(data, '$.kind'), json_extract(data, '$.from_actor'), COUNT(*), COUNT(*) * 1.0 / COUNT(DISTINCT task_id)
               FROM stat_facts WHERE kind = 'message' AND task_id IS NOT NULL{narrow}
           GROUP BY json_extract(data, '$.kind'), json_extract(data, '$.from_actor') ORDER BY 1, 2"
        )));
        for bind in &binds {
            messages = messages.bind(bind);
        }
        let messages = messages
            .fetch_all(self.r())
            .await?
            .into_iter()
            .map(|(kind, from_actor, total, mean_per_task)| MessageStatRow {
                kind,
                from_actor,
                total: total as u64,
                mean_per_task,
            })
            .collect();
        Ok(ReviewStats {
            authors: author_rows,
            reviewers: reviewer_rows,
            messages,
        })
    }
    /// How tools and permission requests performed over the facts the filter
    /// keeps. Durations are ordered in Rust because SQLite has no portable
    /// percentile aggregate: the median averages the two middle values and
    /// p90 is the nearest-rank value.
    pub async fn tool_stats(&self, filter: &StatsFilter) -> Result<ToolStats> {
        let (narrow, binds) = narrowed(filter);
        let mut calls =
            sqlx::query_as::<_, (String, Option<String>, i64, i64)>(sqlx::AssertSqlSafe(format!(
                "SELECT json_extract(data, '$.tool_name'), model,
                        json_extract(data, '$.duration_ms'), json_extract(data, '$.ok')
                   FROM stat_facts WHERE kind = 'tool_call'{narrow}"
            )));
        for bind in &binds {
            calls = calls.bind(bind);
        }
        let calls = calls.fetch_all(self.r()).await?;
        let mut by_tool: HashMap<String, (u64, u64, Vec<i64>)> = HashMap::new();
        let mut by_model: HashMap<String, (u64, i64)> = HashMap::new();
        for (tool_name, model, duration_ms, ok) in calls {
            let tool = tool_name;
            let duration = duration_ms.max(0);
            let row = by_tool.entry(tool).or_default();
            row.0 += 1;
            row.1 += (ok == 0) as u64;
            row.2.push(duration);
            if let Some(model) = model {
                let row = by_model.entry(model).or_default();
                row.0 += 1;
                row.1 += duration;
            }
        }
        let mut tools: Vec<_> = by_tool
            .into_iter()
            .map(|(tool_name, (calls, errors, mut durations))| {
                durations.sort_unstable();
                ToolStatRow {
                    tool_name,
                    calls,
                    errors,
                    median_duration_ms: median(&durations),
                    p90_duration_ms: percentile90(&durations),
                }
            })
            .collect();
        tools.sort_by(|a, b| a.tool_name.cmp(&b.tool_name));
        let mut models: Vec<_> = by_model
            .into_iter()
            .map(|(model, (calls, total))| ToolModelStatRow {
                model,
                calls,
                mean_duration_ms: total as f64 / calls as f64,
            })
            .collect();
        models.sort_by(|a, b| a.model.cmp(&b.model));

        let mut permissions =
            sqlx::query_as::<_, (String, String, i64, Option<f64>)>(sqlx::AssertSqlSafe(format!(
                "SELECT json_extract(data, '$.decided_by'), json_extract(data, '$.answer'),
                        COUNT(*), AVG(json_extract(data, '$.wait_ms'))
                   FROM stat_facts WHERE kind = 'permission'{narrow}
               GROUP BY json_extract(data, '$.decided_by'), json_extract(data, '$.answer')
               ORDER BY json_extract(data, '$.decided_by'), json_extract(data, '$.answer')"
            )));
        for bind in &binds {
            permissions = permissions.bind(bind);
        }
        let permissions = permissions
            .fetch_all(self.r())
            .await?
            .into_iter()
            .map(
                |(decided_by, answer, permissions, mean_wait_ms)| PermissionStatRow {
                    decided_by,
                    answer,
                    permissions: permissions.max(0) as u64,
                    mean_wait_ms: mean_wait_ms.unwrap_or(0.0),
                },
            )
            .collect();
        Ok(ToolStats {
            tools,
            models,
            permissions,
        })
    }
}

fn median(values: &[i64]) -> f64 {
    let middle = values.len() / 2;
    if values.len().is_multiple_of(2) {
        (values[middle - 1] + values[middle]) as f64 / 2.0
    } else {
        values[middle] as f64
    }
}

fn percentile90(values: &[i64]) -> f64 {
    values[((values.len() * 90).div_ceil(100)).saturating_sub(1)] as f64
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
