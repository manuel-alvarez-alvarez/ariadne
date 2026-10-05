//! The `models` family: which model does the job.

use std::collections::{BTreeMap, BTreeSet};

use ariadne_core::TokenUsage;
use serde_json::Value;
use sqlx::Row;

use super::{StatsFilter, median, narrowed};
use crate::{Result, Store};

/// Models compared within each seat.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ModelStats {
    pub items: Vec<ModelStat>,
}

/// Session measures and the measures specific to this seat.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ModelStat {
    pub model: String,
    pub seat: Option<String>,
    pub sessions: u64,
    pub failed_sessions: u64,
    pub stalled_sessions: u64,
    pub exhaustions: u64,
    pub usage: TokenUsage,
    pub cached_share: f64,
    pub mean_lifetime_secs: f64,
    pub total_lifetime_secs: f64,
    pub interventions: ModelInterventions,
    pub author: Option<AuthorModelStat>,
    pub reviewer: Option<ReviewerModelStat>,
}

/// The times a person stepped in for this model in this seat.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ModelInterventions {
    pub permissions: u64,
    pub questions: u64,
    pub stalls: u64,
    pub total: u64,
    pub person_secs: f64,
}

/// Outcomes attributed to an author model.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AuthorModelStat {
    pub tasks_finished: u64,
    pub tasks_failed: u64,
    pub tasks_cancelled: u64,
    pub finish_rate: f64,
    pub first_pass_rate: f64,
    pub mean_review_rounds: f64,
    pub contests_entered: u64,
    pub contests_won: u64,
    pub win_rate: f64,
    pub tokens_per_finished_task: f64,
    pub median_lead_time_secs: f64,
    pub interventions_per_finished_task: Option<f64>,
}

/// Verdicts attributed to a reviewer model.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ReviewerModelStat {
    pub verdicts: u64,
    pub approve_share: f64,
    pub mean_latency_secs: f64,
}

/// Intermediate totals retain task identities until all filtered facts are read.
#[derive(Default)]
struct Totals {
    row: ModelStat,
    lifetime: f64,
    review_rounds: f64,
    reviewed_tasks: BTreeSet<String>,
    first_pass_tasks: BTreeSet<String>,
    finished_tasks: BTreeSet<String>,
    task_tokens: BTreeMap<String, u64>,
    lead_times: Vec<f64>,
    approvals: u64,
    latency: f64,
}

type Rows = BTreeMap<(u8, String), Totals>;

fn row<'a>(rows: &'a mut Rows, model: &str, seat: Option<&str>) -> &'a mut Totals {
    let order = match seat {
        Some("orchestrator") => 0,
        Some("author") => 1,
        Some("reviewer") => 2,
        _ => 3,
    };
    rows.entry((order, model.into())).or_insert_with(|| Totals {
        row: ModelStat {
            model: model.into(),
            seat: seat.map(str::to_owned),
            author: (seat == Some("author")).then(AuthorModelStat::default),
            reviewer: (seat == Some("reviewer")).then(ReviewerModelStat::default),
            ..ModelStat::default()
        },
        ..Totals::default()
    })
}

fn ratio(numerator: f64, denominator: u64) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator / denominator as f64
    }
}

impl Store {
    /// Which model does the job, over the facts the filter keeps.
    pub async fn model_stats(&self, filter: &StatsFilter) -> Result<ModelStats> {
        let (clause, binds) = narrowed(filter);
        // Only a person's answer is an intervention. A `waiting_permission` attention
        // fact is left out, because its prompt already counts as a permission.
        let sql = format!(
            "SELECT kind, model, seat, task_id, data FROM stat_facts
             WHERE (kind IN ('session_ended', 'switch', 'task_ended', 'pick', 'verdict')
                    OR (kind = 'permission' AND json_extract(data, '$.decided_by') = 'console')
                    OR (kind = 'attention' AND json_extract(data, '$.reason')
                        IN ('waiting_input', 'waiting_user', 'stalled', 'agent_error'))){clause}"
        );
        // The only SQL fragments are literals from narrowed; every filter value is bound.
        let mut query = sqlx::query(sqlx::AssertSqlSafe(sql));
        for bind in binds {
            query = query.bind(bind);
        }
        let facts = query.fetch_all(self.r()).await?;
        let mut rows = Rows::new();
        for fact in facts {
            let kind: String = fact.try_get("kind")?;
            let model: Option<String> = fact.try_get("model")?;
            let seat: Option<String> = fact.try_get("seat")?;
            let task: Option<String> = fact.try_get("task_id")?;
            let data: sqlx::types::Json<Value> = fact.try_get("data")?;
            let number = |key: &str| data[key].as_u64().unwrap_or(0);
            let seconds = |key: &str| data[key].as_f64().unwrap_or(0.0);
            if let Some(model) = model.as_deref() {
                let totals = row(&mut rows, model, seat.as_deref());
                match kind.as_str() {
                    "session_ended" => {
                        totals.row.sessions += 1;
                        totals.row.failed_sessions += u64::from(data["status"] == "failed");
                        totals.row.stalled_sessions +=
                            u64::from(data["attention_reason"] == "stalled");
                        let usage = TokenUsage {
                            input_tokens: number("input_tokens"),
                            cached_input_tokens: number("cached_input_tokens"),
                            output_tokens: number("output_tokens"),
                        };
                        totals.row.usage += usage;
                        totals.lifetime += seconds("lifetime_secs");
                        if seat.as_deref() == Some("author")
                            && let Some(task) = &task
                        {
                            *totals.task_tokens.entry(task.clone()).or_default() +=
                                usage.input_tokens.saturating_add(usage.output_tokens);
                        }
                    }
                    "switch" => totals.row.exhaustions += u64::from(data["reason"] == "exhausted"),
                    "task_ended" => {
                        if let Some(author) = &mut totals.row.author {
                            match data["status"].as_str() {
                                Some("finished") => {
                                    author.tasks_finished += 1;
                                    totals.lead_times.push(seconds("lead_time_secs"));
                                    if let Some(task) = &task {
                                        totals.finished_tasks.insert(task.clone());
                                    }
                                }
                                Some("failed") => author.tasks_failed += 1,
                                Some("cancelled") => author.tasks_cancelled += 1,
                                _ => {}
                            }
                            totals.review_rounds += seconds("review_requests");
                        }
                    }
                    "permission" => {
                        totals.row.interventions.permissions += 1;
                        totals.row.interventions.person_secs += seconds("wait_ms") / 1_000.0;
                    }
                    "attention" => {
                        let interventions = &mut totals.row.interventions;
                        match data["reason"].as_str() {
                            Some("waiting_input" | "waiting_user") => interventions.questions += 1,
                            _ => interventions.stalls += 1,
                        }
                        interventions.person_secs += seconds("wait_secs");
                    }
                    "verdict" => {
                        if let Some(reviewer) = &mut totals.row.reviewer {
                            reviewer.verdicts += 1;
                            totals.approvals += u64::from(data["verdict"] == "approve");
                            totals.latency += seconds("latency_secs");
                        }
                    }
                    _ => {}
                }
            }
            if kind == "verdict"
                && let Some(model) = data["author_model"].as_str()
            {
                let totals = row(&mut rows, model, Some("author"));
                if let Some(task) = &task {
                    totals.reviewed_tasks.insert(task.clone());
                    if number("round") == 1 && data["verdict"] == "approve" {
                        totals.first_pass_tasks.insert(task.clone());
                    }
                }
            }
            if kind == "pick" {
                let winner = data["winner_model"].as_str();
                let mut entrants = BTreeSet::new();
                if let Some(winner) = winner {
                    entrants.insert(winner);
                }
                if let Some(losers) = data["loser_models"].as_array() {
                    entrants.extend(losers.iter().filter_map(Value::as_str));
                }
                for model in entrants {
                    let author = row(&mut rows, model, Some("author"))
                        .row
                        .author
                        .as_mut()
                        .unwrap();
                    author.contests_entered += 1;
                    author.contests_won += u64::from(Some(model) == winner);
                }
            }
        }
        let items = rows
            .into_values()
            .map(|mut totals| {
                let row = &mut totals.row;
                row.cached_share =
                    ratio(row.usage.cached_input_tokens as f64, row.usage.input_tokens);
                row.mean_lifetime_secs = ratio(totals.lifetime, row.sessions);
                row.total_lifetime_secs = totals.lifetime;
                let interventions = &mut row.interventions;
                interventions.total =
                    interventions.permissions + interventions.questions + interventions.stalls;
                if let Some(author) = &mut row.author {
                    let tasks =
                        author.tasks_finished + author.tasks_failed + author.tasks_cancelled;
                    author.finish_rate = ratio(author.tasks_finished as f64, tasks);
                    author.first_pass_rate = ratio(
                        totals.first_pass_tasks.len() as f64,
                        totals.reviewed_tasks.len() as u64,
                    );
                    author.mean_review_rounds = ratio(totals.review_rounds, tasks);
                    author.win_rate = ratio(author.contests_won as f64, author.contests_entered);
                    let tokens: u64 = totals
                        .finished_tasks
                        .iter()
                        .map(|task| totals.task_tokens.get(task).copied().unwrap_or(0))
                        .sum();
                    author.tokens_per_finished_task =
                        ratio(tokens as f64, totals.finished_tasks.len() as u64);
                    author.median_lead_time_secs = median(&totals.lead_times);
                    author.interventions_per_finished_task = (author.tasks_finished > 0)
                        .then(|| row.interventions.total as f64 / author.tasks_finished as f64);
                }
                if let Some(reviewer) = &mut row.reviewer {
                    reviewer.approve_share = ratio(totals.approvals as f64, reviewer.verdicts);
                    reviewer.mean_latency_secs = ratio(totals.latency, reviewer.verdicts);
                }
                totals.row
            })
            .collect();
        Ok(ModelStats { items })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NewStatFact;
    use serde_json::{Value, json};

    async fn fact(
        store: &Store,
        kind: &str,
        model: Option<&str>,
        seat: Option<&str>,
        task: &str,
        data: Value,
    ) {
        store
            .record_fact(NewStatFact {
                kind: kind.into(),
                model: model.map(str::to_owned),
                seat: seat.map(str::to_owned),
                repo_id: Some("repo".into()),
                task_id: Some(task.into()),
                goal_id: None,
                session_id: None,
                launch_id: None,
                effort: None,
                skills: vec![],
                data,
            })
            .await
            .unwrap();
    }

    async fn fixture() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("stats.db")).await.unwrap();
        for (task, status, rounds) in [
            ("done", "finished", 1),
            ("failed", "failed", 3),
            ("cancelled", "cancelled", 2),
        ] {
            fact(
                &store,
                "task_ended",
                Some("writer"),
                Some("author"),
                task,
                json!({"status": status, "review_requests": rounds}),
            )
            .await;
        }
        for (task, status, reason, input, cached, output, lifetime) in [
            ("done", "exited", "", 100, 40, 20, 30),
            ("done", "failed", "stalled", 200, 80, 40, 90),
            ("failed", "exited", "waiting_input", 900, 0, 100, 60),
        ] {
            fact(
                &store,
                "session_ended",
                Some("writer"),
                Some("author"),
                task,
                json!({"status": status, "attention_reason": reason, "input_tokens": input,
                "cached_input_tokens": cached, "output_tokens": output, "lifetime_secs": lifetime}),
            )
            .await;
        }
        for (task, round, verdict, latency) in [
            ("done", 1, "approve", 10),
            ("done", 1, "approve", 20),
            ("failed", 1, "changes_requested", 30),
            ("failed", 2, "approve", 60),
        ] {
            fact(&store, "verdict", Some("judge"), Some("reviewer"), task,
                json!({"author_model": "writer", "round": round, "verdict": verdict, "latency_secs": latency})).await;
        }
        fact(
            &store,
            "session_ended",
            Some("judge"),
            Some("reviewer"),
            "done",
            json!({"status":"failed", "attention_reason":"stalled", "input_tokens":50,
            "cached_input_tokens":25, "output_tokens":10, "lifetime_secs":120}),
        )
        .await;
        for reason in ["exhausted", "requested"] {
            fact(
                &store,
                "switch",
                Some("writer"),
                Some("author"),
                "done",
                json!({"reason":reason}),
            )
            .await;
        }
        for (winner, losers) in [
            ("writer", vec!["writer", "other", "other"]),
            ("other", vec!["writer", "writer"]),
        ] {
            fact(
                &store,
                "pick",
                Some(winner),
                Some("author"),
                "contest",
                json!({"winner_model":winner, "loser_models":losers}),
            )
            .await;
        }
        (dir, store)
    }

    #[tokio::test]
    async fn author_figures_combine_sessions_outcomes_reviews_and_contests() {
        let (_dir, store) = fixture().await;
        let stats = store.model_stats(&StatsFilter::default()).await.unwrap();
        let row = stats
            .items
            .iter()
            .find(|row| row.model == "writer")
            .unwrap();
        assert_eq!(
            (
                row.sessions,
                row.failed_sessions,
                row.stalled_sessions,
                row.exhaustions
            ),
            (3, 1, 1, 1)
        );
        assert_eq!(
            row.usage,
            TokenUsage {
                input_tokens: 1200,
                cached_input_tokens: 120,
                output_tokens: 160
            }
        );
        assert_eq!(row.cached_share, 0.1);
        assert_eq!(row.mean_lifetime_secs, 60.0);
        assert_eq!(
            row.author,
            Some(AuthorModelStat {
                tasks_finished: 1,
                tasks_failed: 1,
                tasks_cancelled: 1,
                finish_rate: 1.0 / 3.0,
                first_pass_rate: 0.5,
                mean_review_rounds: 2.0,
                contests_entered: 2,
                contests_won: 1,
                win_rate: 0.5,
                tokens_per_finished_task: 360.0,
                median_lead_time_secs: 0.0,
                interventions_per_finished_task: Some(0.0),
            })
        );
        assert_eq!(row.reviewer, None);
        let other = stats
            .items
            .iter()
            .find(|row| row.model == "other")
            .unwrap()
            .author
            .as_ref()
            .unwrap();
        assert_eq!(
            (other.contests_entered, other.contests_won, other.win_rate),
            (2, 1, 0.5)
        );
    }

    #[tokio::test]
    async fn reviewer_figures_count_each_verdict_and_its_latency() {
        let (_dir, store) = fixture().await;
        let stats = store.model_stats(&StatsFilter::default()).await.unwrap();
        let row = stats.items.iter().find(|row| row.model == "judge").unwrap();
        assert_eq!(
            (row.sessions, row.failed_sessions, row.stalled_sessions),
            (1, 1, 1)
        );
        assert_eq!(
            row.usage,
            TokenUsage {
                input_tokens: 50,
                cached_input_tokens: 25,
                output_tokens: 10
            }
        );
        assert_eq!((row.cached_share, row.mean_lifetime_secs), (0.5, 120.0));
        assert_eq!(
            row.reviewer,
            Some(ReviewerModelStat {
                verdicts: 4,
                approve_share: 0.75,
                mean_latency_secs: 30.0
            })
        );
        assert_eq!(row.author, None);
    }

    #[tokio::test]
    async fn every_fact_is_filtered_by_time_and_repository() {
        let (_dir, store) = fixture().await;
        sqlx::query("UPDATE stat_facts SET created_at = '2026-01-01T00:00:00.000Z'")
            .execute(store.w())
            .await
            .unwrap();
        for (repo, since) in [
            (Some("elsewhere"), None),
            (None, Some("2026-02-01T00:00:00Z")),
            (Some("repo"), Some("2026-02-01T00:00:00Z")),
        ] {
            let filter = StatsFilter {
                repo_id: repo.map(str::to_owned),
                since: since.map(|s| s.parse().unwrap()),
            };
            assert!(store.model_stats(&filter).await.unwrap().items.is_empty());
        }
        let filter = StatsFilter {
            repo_id: Some("repo".into()),
            since: Some("2026-01-01T00:00:00Z".parse().unwrap()),
        };
        assert_eq!(store.model_stats(&filter).await.unwrap().items.len(), 3);
        fact(
            &store,
            "session_ended",
            Some("writer"),
            Some("author"),
            "done",
            json!({"input_tokens":1000, "output_tokens":100}),
        )
        .await;
        let filter = StatsFilter {
            since: Some("2026-02-01T00:00:00Z".parse().unwrap()),
            repo_id: Some("repo".into()),
        };
        let stats = store.model_stats(&filter).await.unwrap();
        let row = &stats.items[0];
        assert_eq!(row.sessions, 1);
        assert_eq!(row.author.as_ref().unwrap().tokens_per_finished_task, 0.0);
    }

    #[tokio::test]
    async fn rows_include_models_named_only_in_payloads_and_sort_by_seat_then_model() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("stats.db")).await.unwrap();
        for (model, seat) in [
            ("z", None),
            ("z", Some("reviewer")),
            ("z", Some("author")),
            ("z", Some("orchestrator")),
            ("a", Some("orchestrator")),
        ] {
            fact(
                &store,
                "switch",
                Some(model),
                seat,
                "task",
                json!({"reason":"exhausted"}),
            )
            .await;
        }
        fact(
            &store,
            "verdict",
            None,
            Some("reviewer"),
            "task",
            json!({"author_model":"payload", "round":2, "verdict":"approve"}),
        )
        .await;
        fact(
            &store,
            "pick",
            None,
            None,
            "task",
            json!({"winner_model":"winner", "loser_models":["loser","loser"]}),
        )
        .await;
        fact(
            &store,
            "message",
            Some("ignored"),
            Some("author"),
            "task",
            json!({}),
        )
        .await;
        let stats = store.model_stats(&StatsFilter::default()).await.unwrap();
        assert_eq!(
            stats
                .items
                .iter()
                .map(|r| (r.seat.as_deref(), r.model.as_str()))
                .collect::<Vec<_>>(),
            vec![
                (Some("orchestrator"), "a"),
                (Some("orchestrator"), "z"),
                (Some("author"), "loser"),
                (Some("author"), "payload"),
                (Some("author"), "winner"),
                (Some("author"), "z"),
                (Some("reviewer"), "z"),
                (None, "z"),
            ]
        );
        let row = &stats.items[5];
        assert_eq!(row.author, Some(AuthorModelStat::default()));
        assert_eq!(
            (row.cached_share, row.mean_lifetime_secs, row.exhaustions),
            (0.0, 0.0, 1)
        );
        assert_eq!(stats.items[6].reviewer, Some(ReviewerModelStat::default()));
        assert_eq!(stats.items[3].author.as_ref().unwrap().first_pass_rate, 0.0);
    }

    #[tokio::test]
    async fn first_pass_counts_reviewed_tasks_once() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("stats.db")).await.unwrap();
        for (task, round, verdict) in [
            ("one", 1, "approve"),
            ("one", 1, "approve"),
            ("two", 1, "changes_requested"),
            ("two", 2, "approve"),
            ("three", 2, "approve"),
        ] {
            fact(&store, "verdict", Some("judge"), Some("reviewer"), task,
                json!({"author_model":"writer", "round":round, "verdict":verdict, "latency_secs":0})).await;
        }
        let stats = store.model_stats(&StatsFilter::default()).await.unwrap();
        let author = stats.items[0].author.as_ref().unwrap();
        assert_eq!(author.first_pass_rate, 1.0 / 3.0);
        assert_eq!(author.finish_rate, 0.0);
        assert_eq!(author.mean_review_rounds, 0.0);
        assert_eq!(author.win_rate, 0.0);
    }

    #[tokio::test]
    async fn finished_task_tokens_count_each_task_once_and_exclude_other_seats() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("stats.db")).await.unwrap();
        for task in ["one", "one", "without-session"] {
            fact(
                &store,
                "task_ended",
                Some("writer"),
                Some("author"),
                task,
                json!({"status":"finished", "review_requests":2}),
            )
            .await;
        }
        for (model, seat, task, input) in [
            ("writer", "author", "one", 100),
            ("writer", "author", "unfinished", 1000),
            ("writer", "reviewer", "one", 1000),
            ("other", "author", "one", 1000),
        ] {
            fact(
                &store,
                "session_ended",
                Some(model),
                Some(seat),
                task,
                json!({"input_tokens":input, "cached_input_tokens":input, "output_tokens":20}),
            )
            .await;
        }
        let stats = store.model_stats(&StatsFilter::default()).await.unwrap();
        let row = stats
            .items
            .iter()
            .find(|r| r.model == "writer" && r.seat.as_deref() == Some("author"))
            .unwrap();
        let author = row.author.as_ref().unwrap();
        assert_eq!(author.tokens_per_finished_task, 60.0);
        assert_eq!(author.tasks_finished, 3);
        assert_eq!(author.finish_rate, 1.0);
        assert_eq!(author.mean_review_rounds, 2.0);
        assert_eq!(row.cached_share, 1.0);
    }

    #[tokio::test]
    async fn interventions_count_what_a_person_answered_per_model_and_seat() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("stats.db")).await.unwrap();
        for (seat, decided_by, wait_ms) in [
            ("author", "console", 4_000),
            ("author", "console", 6_000),
            ("author", "ai", 50_000),
            ("author", "learned", 50_000),
            ("reviewer", "console", 1_000),
        ] {
            fact(
                &store,
                "permission",
                Some("writer"),
                Some(seat),
                "one",
                json!({"decided_by": decided_by, "wait_ms": wait_ms}),
            )
            .await;
        }
        for (seat, reason, wait_secs) in [
            ("author", "waiting_input", 20),
            ("author", "waiting_user", 30),
            ("author", "stalled", 100),
            ("author", "agent_error", 200),
            ("author", "waiting_permission", 9_000),
            ("reviewer", "stalled", 7),
        ] {
            fact(
                &store,
                "attention",
                Some("writer"),
                Some(seat),
                "one",
                json!({"reason": reason, "wait_secs": wait_secs}),
            )
            .await;
        }
        for (task, lead) in [("one", 300), ("two", 100), ("three", 1_000)] {
            fact(
                &store,
                "task_ended",
                Some("writer"),
                Some("author"),
                task,
                json!({"status": "finished", "lead_time_secs": lead}),
            )
            .await;
        }
        fact(
            &store,
            "task_ended",
            Some("writer"),
            Some("author"),
            "four",
            json!({"status": "failed", "lead_time_secs": 9_000}),
        )
        .await;
        for (seat, lifetime) in [("author", 40), ("author", 80), ("reviewer", 15)] {
            fact(
                &store,
                "session_ended",
                Some("writer"),
                Some(seat),
                "one",
                json!({"lifetime_secs": lifetime}),
            )
            .await;
        }
        fact(
            &store,
            "permission",
            Some("quiet"),
            Some("author"),
            "one",
            json!({"decided_by": "ai", "wait_ms": 1_000}),
        )
        .await;
        fact(
            &store,
            "attention",
            Some("quiet"),
            Some("author"),
            "one",
            json!({"reason": "waiting_permission", "wait_secs": 10}),
        )
        .await;

        let stats = store.model_stats(&StatsFilter::default()).await.unwrap();
        assert_eq!(
            stats
                .items
                .iter()
                .map(|r| (r.model.as_str(), r.seat.as_deref()))
                .collect::<Vec<_>>(),
            vec![("writer", Some("author")), ("writer", Some("reviewer"))]
        );
        let author = &stats.items[0];
        assert_eq!(
            author.interventions,
            ModelInterventions {
                permissions: 2,
                questions: 2,
                stalls: 2,
                total: 6,
                person_secs: 360.0,
            }
        );
        assert_eq!(author.total_lifetime_secs, 120.0);
        let figures = author.author.as_ref().unwrap();
        assert_eq!(figures.median_lead_time_secs, 300.0);
        assert_eq!(figures.interventions_per_finished_task, Some(2.0));
        let reviewer = &stats.items[1];
        assert_eq!(
            reviewer.interventions,
            ModelInterventions {
                permissions: 1,
                questions: 0,
                stalls: 1,
                total: 2,
                person_secs: 8.0,
            }
        );
        assert_eq!(reviewer.total_lifetime_secs, 15.0);
        assert_eq!(reviewer.author, None);
    }

    #[tokio::test]
    async fn the_intervention_rate_is_null_without_a_finished_task() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("stats.db")).await.unwrap();
        fact(
            &store,
            "attention",
            Some("writer"),
            Some("author"),
            "one",
            json!({"reason": "waiting_input", "wait_secs": 5}),
        )
        .await;
        fact(
            &store,
            "task_ended",
            Some("writer"),
            Some("author"),
            "one",
            json!({"status": "failed", "lead_time_secs": 50}),
        )
        .await;
        let stats = store.model_stats(&StatsFilter::default()).await.unwrap();
        let author = stats.items[0].author.as_ref().unwrap();
        assert_eq!(stats.items[0].interventions.total, 1);
        assert_eq!(author.interventions_per_finished_task, None);
        assert_eq!(author.median_lead_time_secs, 0.0);
    }
}
