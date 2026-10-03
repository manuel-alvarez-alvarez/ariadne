//! The stats ledger: facts the daemon writes as things happen, and the
//! aggregates the stats are read with.
//!
//! A fact is appended once and never changed. Every aggregate here reads
//! `stat_facts` and nothing else, so a stats read costs the facts it counts and
//! never a scan of `agent_events` — see `migrations/0001_init.sql`.

use std::collections::{HashMap, HashSet};

use ariadne_core::TokenUsage;
use ariadne_core::id::new_id;
use chrono::{DateTime, SecondsFormat, Utc};

use crate::usage::usage_of;
use crate::{Result, Store, StoreError, now};

/// The fact a session writes when one run of it ends.
const SESSION_ENDED: &str = "session_ended";

/// The fact a session writes as it leaves one model for another.
const SWITCH: &str = "switch";

/// The fact a task writes when it reaches one of its three endings.
const TASK_ENDED: &str = "task_ended";

/// The fact a contested task's pick writes once the winner is settled.
const PICK: &str = "pick";

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
}

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

/// How one author model did at ending tasks, and at winning the contests it
/// entered, over the `task_ended` and `pick` facts the filter keeps.
#[derive(Debug, Clone, PartialEq)]
pub struct OutcomeStatRow {
    pub model: String,
    pub finished: u64,
    pub failed: u64,
    pub cancelled: u64,
    /// `finished` over every ending counted, 0 where none were.
    pub finish_rate: f64,
    pub median_lead_time_secs: f64,
    pub mean_lead_time_secs: f64,
    pub mean_review_requests: f64,
    /// Contests this model's author was staffed in, as the winner or a loser.
    pub contests_entered: u64,
    pub contests_won: u64,
    /// `contests_won` over `contests_entered`, 0 where none were entered.
    pub win_rate: f64,
}

/// The same figures, summed across every model the filter keeps.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct OutcomeTotals {
    pub finished: u64,
    pub failed: u64,
    pub cancelled: u64,
    pub finish_rate: f64,
    pub median_lead_time_secs: f64,
    pub mean_lead_time_secs: f64,
    pub mean_review_requests: f64,
    pub contests_entered: u64,
    pub contests_won: u64,
    pub win_rate: f64,
}

/// Answer of [`Store::outcome_stats`]: one row per author model, ordered by
/// model, and the totals across all of them.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct OutcomeStats {
    pub rows: Vec<OutcomeStatRow>,
    pub totals: OutcomeTotals,
}

/// What one model's `task_ended` facts and `pick` facts add up to, before the
/// rates are taken.
#[derive(Debug, Clone, Default)]
struct Outcome {
    finished: u64,
    failed: u64,
    cancelled: u64,
    lead_times: Vec<f64>,
    review_requests: Vec<f64>,
    contests_entered: u64,
    contests_won: u64,
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

/// How one model did over the `switch` facts the filter keeps: the switches
/// that left it, and the switches that arrived on it.
#[derive(Debug, Clone, PartialEq)]
pub struct SwitchStatRow {
    pub model: String,
    /// Sessions that left this model.
    pub switches: u64,
    /// Each reason a switch left this model, with how many gave it. The
    /// most common first, and by name where two tie.
    pub by_reason: Vec<(String, u64)>,
    /// Of `switches`, those left for `exhausted`.
    pub exhaustions: u64,
    /// The automatic share of `switches`, 0 to 1; 0 where it had none.
    pub automatic_share: f64,
    /// Sessions that arrived on this model from another.
    pub arrivals: u64,
}

/// The switch family: one row per model a `switch` fact names, left or
/// arrived on, and the totals over every one of them.
#[derive(Debug, Clone, PartialEq)]
pub struct SwitchStats {
    pub items: Vec<SwitchStatRow>,
    pub switches: u64,
    pub exhaustions: u64,
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

    /// How each model did as sessions left it or arrived on it: one row per
    /// model a `switch` fact names, ordered by model.
    pub async fn switch_stats(&self, filter: &StatsFilter) -> Result<SwitchStats> {
        let (narrow, binds) = narrowed(filter);
        let mut q = sqlx::query_as::<_, (String, i64, i64, i64)>(sqlx::AssertSqlSafe(format!(
            "SELECT model, COUNT(*),
                    COALESCE(SUM(json_extract(data, '$.reason') = 'exhausted'), 0),
                    COALESCE(SUM(json_extract(data, '$.automatic') = 1), 0)
               FROM stat_facts
              WHERE kind = ? AND model IS NOT NULL{narrow}
           GROUP BY model"
        )))
        .bind(SWITCH);
        for bind in &binds {
            q = q.bind(bind);
        }
        let left_rows = q.fetch_all(self.r()).await?;

        let mut q = sqlx::query_as::<_, (String, String, i64)>(sqlx::AssertSqlSafe(format!(
            "SELECT model, json_extract(data, '$.reason'), COUNT(*)
               FROM stat_facts
              WHERE kind = ? AND model IS NOT NULL{narrow}
           GROUP BY model, json_extract(data, '$.reason')
           ORDER BY COUNT(*) DESC, json_extract(data, '$.reason')"
        )))
        .bind(SWITCH);
        for bind in &binds {
            q = q.bind(bind);
        }
        let mut reasons: HashMap<String, Vec<(String, u64)>> = HashMap::new();
        for (model, reason, count) in q.fetch_all(self.r()).await? {
            reasons
                .entry(model)
                .or_default()
                .push((reason, count.max(0) as u64));
        }

        let mut q = sqlx::query_as::<_, (String, i64)>(sqlx::AssertSqlSafe(format!(
            "SELECT json_extract(data, '$.to_model'), COUNT(*)
               FROM stat_facts
              WHERE kind = ?{narrow}
           GROUP BY json_extract(data, '$.to_model')"
        )))
        .bind(SWITCH);
        for bind in &binds {
            q = q.bind(bind);
        }
        let mut arrivals: HashMap<String, u64> = HashMap::new();
        for (model, count) in q.fetch_all(self.r()).await? {
            arrivals.insert(model, count.max(0) as u64);
        }

        let mut left: HashMap<String, (u64, u64, u64)> = HashMap::new();
        let mut models: Vec<String> = Vec::new();
        for (model, switches, exhaustions, automatic) in left_rows {
            models.push(model.clone());
            left.insert(
                model,
                (
                    switches.max(0) as u64,
                    exhaustions.max(0) as u64,
                    automatic.max(0) as u64,
                ),
            );
        }
        for model in arrivals.keys() {
            if !left.contains_key(model) {
                models.push(model.clone());
            }
        }
        models.sort();

        let mut total_switches = 0;
        let mut total_exhaustions = 0;
        let items = models
            .into_iter()
            .map(|model| {
                let (switches, exhaustions, automatic) =
                    left.get(&model).copied().unwrap_or((0, 0, 0));
                total_switches += switches;
                total_exhaustions += exhaustions;
                SwitchStatRow {
                    by_reason: reasons.remove(&model).unwrap_or_default(),
                    automatic_share: match switches {
                        0 => 0.0,
                        n => automatic as f64 / n as f64,
                    },
                    arrivals: arrivals.get(&model).copied().unwrap_or(0),
                    model,
                    switches,
                    exhaustions,
                }
            })
            .collect();

        Ok(SwitchStats {
            items,
            switches: total_switches,
            exhaustions: total_exhaustions,
        })
    }

    /// How each author model ended its tasks, and how it did in the contests
    /// it was staffed in, over the `task_ended` and `pick` facts the filter
    /// keeps.
    pub async fn outcome_stats(&self, filter: &StatsFilter) -> Result<OutcomeStats> {
        let (narrow, binds) = narrowed(filter);

        let mut q =
            sqlx::query_as::<_, (Option<String>, Option<String>, Option<f64>, Option<f64>)>(
                sqlx::AssertSqlSafe(format!(
                    "SELECT model, json_extract(data, '$.status'),
                        CAST(json_extract(data, '$.lead_time_secs') AS REAL),
                        CAST(json_extract(data, '$.review_requests') AS REAL)
                   FROM stat_facts
                  WHERE kind = ?{narrow}"
                )),
            )
            .bind(TASK_ENDED);
        for bind in &binds {
            q = q.bind(bind);
        }
        let ended = q.fetch_all(self.r()).await?;

        let mut by_model: HashMap<String, Outcome> = HashMap::new();
        for (model, status, lead_time, review_requests) in ended {
            if let Some(model) = model {
                add_ending(
                    by_model.entry(model).or_default(),
                    status.as_deref(),
                    lead_time,
                    review_requests,
                );
            }
        }

        let mut q = sqlx::query_as::<_, (String,)>(sqlx::AssertSqlSafe(format!(
            "SELECT data FROM stat_facts WHERE kind = ?{narrow}"
        )))
        .bind(PICK);
        for bind in &binds {
            q = q.bind(bind);
        }
        for (data,) in q.fetch_all(self.r()).await? {
            add_contest(&mut by_model, &data);
        }

        // The totals are the sum of the rows below, never a count of its own:
        // a fact whose model could not be resolved names no row, and a model
        // entered in a contest only once however many of its own authors
        // that contest held.
        let totals = outcome_totals(by_model.values());
        let mut rows: Vec<OutcomeStatRow> = by_model
            .into_iter()
            .map(|(model, outcome)| outcome_row(model, &outcome))
            .collect();
        rows.sort_by(|a, b| a.model.cmp(&b.model));

        Ok(OutcomeStats { rows, totals })
    }
}

/// Fold one `pick` fact's `data` into the models it named: the winner and
/// every loser, each counted as entered once, however many of a contest's
/// authors shared it — two authors on the same model is one entry for that
/// model, not two.
fn add_contest(by_model: &mut HashMap<String, Outcome>, data: &str) {
    let Ok(data) = serde_json::from_str::<serde_json::Value>(data) else {
        return;
    };
    let Some(winner) = data["winner_model"].as_str() else {
        return;
    };
    let losers = data["loser_models"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str());
    let models: HashSet<&str> = std::iter::once(winner).chain(losers).collect();
    for model in models {
        let row = by_model.entry(model.to_string()).or_default();
        row.contests_entered += 1;
        if model == winner {
            row.contests_won += 1;
        }
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

/// Fold one `task_ended` fact's status, lead time and review requests into a
/// running outcome.
fn add_ending(
    outcome: &mut Outcome,
    status: Option<&str>,
    lead_time: Option<f64>,
    review_requests: Option<f64>,
) {
    match status {
        Some("finished") => outcome.finished += 1,
        Some("failed") => outcome.failed += 1,
        Some("cancelled") => outcome.cancelled += 1,
        _ => {}
    }
    if let Some(lead_time) = lead_time {
        outcome.lead_times.push(lead_time);
    }
    if let Some(review_requests) = review_requests {
        outcome.review_requests.push(review_requests);
    }
}

fn outcome_row(model: String, outcome: &Outcome) -> OutcomeStatRow {
    let ended = outcome.finished + outcome.failed + outcome.cancelled;
    OutcomeStatRow {
        model,
        finished: outcome.finished,
        failed: outcome.failed,
        cancelled: outcome.cancelled,
        finish_rate: rate(outcome.finished, ended),
        median_lead_time_secs: median_f64(&outcome.lead_times),
        mean_lead_time_secs: mean_f64(&outcome.lead_times),
        mean_review_requests: mean_f64(&outcome.review_requests),
        contests_entered: outcome.contests_entered,
        contests_won: outcome.contests_won,
        win_rate: rate(outcome.contests_won, outcome.contests_entered),
    }
}

/// The totals across every model row: the sum of their counts, and the mean
/// and the median over every one of their lead times and review requests
/// together — never a count taken on the side, which is what let a fact with
/// no row of its own, or one contest counted into two rows, throw the totals
/// out of step with what the rows actually add up to.
fn outcome_totals<'a>(rows: impl Iterator<Item = &'a Outcome>) -> OutcomeTotals {
    let mut sum = Outcome::default();
    for row in rows {
        sum.finished += row.finished;
        sum.failed += row.failed;
        sum.cancelled += row.cancelled;
        sum.contests_entered += row.contests_entered;
        sum.contests_won += row.contests_won;
        sum.lead_times.extend(&row.lead_times);
        sum.review_requests.extend(&row.review_requests);
    }
    let ended = sum.finished + sum.failed + sum.cancelled;
    OutcomeTotals {
        finished: sum.finished,
        failed: sum.failed,
        cancelled: sum.cancelled,
        finish_rate: rate(sum.finished, ended),
        median_lead_time_secs: median_f64(&sum.lead_times),
        mean_lead_time_secs: mean_f64(&sum.lead_times),
        mean_review_requests: mean_f64(&sum.review_requests),
        contests_entered: sum.contests_entered,
        contests_won: sum.contests_won,
        win_rate: rate(sum.contests_won, sum.contests_entered),
    }
}

/// `n` over `total`, 0 where there is nothing to take a share of.
fn rate(n: u64, total: u64) -> f64 {
    match total {
        0 => 0.0,
        total => n as f64 / total as f64,
    }
}

/// The arithmetic mean of `values`, 0 for none.
fn mean_f64(values: &[f64]) -> f64 {
    match values.len() {
        0 => 0.0,
        len => values.iter().sum::<f64>() / len as f64,
    }
}

/// The median of `values`, 0 for none.
fn median_f64(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mid = sorted.len() / 2;
    if sorted.len().is_multiple_of(2) {
        (sorted[mid - 1] + sorted[mid]) / 2.0
    } else {
        sorted[mid]
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
