//! The `models` family: which model does the job.

use std::collections::{BTreeMap, BTreeSet};

use ariadne_core::MessageKind;
use serde_json::Value;
use sqlx::Row;

use super::{StatsFilter, narrowed};
use crate::{Result, Store};

/// Models compared within each seat.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ModelStats {
    pub items: Vec<ModelStat>,
}

/// What one model did in one seat.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ModelStat {
    pub model: String,
    pub seat: Option<String>,
    /// Distinct tasks with a session of this model in this seat.
    pub tasks: u64,
    /// Distinct goals with a session of this model in this seat.
    pub goals: u64,
    /// Input and output tokens of the ended sessions. Cached tokens are part of input.
    pub tokens: u64,
    /// The sum of the session lifetimes.
    pub time_secs: f64,
    /// The messages this model sent in this seat.
    pub messages: u64,
    /// Authors only: the mean review requests over finished tasks, zero without one.
    pub rounds_per_task: Option<f64>,
    /// Reviewers only: changes requested over the tasks given a verdict, zero without one.
    pub changes_per_task: Option<f64>,
}

/// Intermediate totals retain task and goal identities until all filtered facts are read.
#[derive(Default)]
struct Totals {
    row: ModelStat,
    tasks: BTreeSet<String>,
    goals: BTreeSet<String>,
    rounds: f64,
    finished: u64,
    changes: u64,
    reviewed_tasks: BTreeSet<String>,
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
        // Every fact that names a model gives it a row, even where it adds to no figure.
        // Only a person's answer and a flag a person cleared count; a `waiting_permission`
        // attention fact is left out, because its prompt is already a permission.
        let sql = format!(
            "SELECT kind, model, seat, goal_id, task_id, data FROM stat_facts
             WHERE (kind IN ('session_ended', 'message', 'task_ended', 'verdict', 'switch', 'pick')
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
            let goal: Option<String> = fact.try_get("goal_id")?;
            let task: Option<String> = fact.try_get("task_id")?;
            let data: sqlx::types::Json<Value> = fact.try_get("data")?;
            // A verdict names its author's model, and a pick names every entrant.
            let authors: Vec<&str> = match kind.as_str() {
                "verdict" => data["author_model"].as_str().into_iter().collect(),
                "pick" => data["winner_model"]
                    .as_str()
                    .into_iter()
                    .chain(
                        data["loser_models"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(Value::as_str),
                    )
                    .collect(),
                _ => Vec::new(),
            };
            for author in authors {
                row(&mut rows, author, Some("author"));
            }
            let Some(model) = model.as_deref() else {
                continue;
            };
            let number = |key: &str| data[key].as_u64().unwrap_or(0);
            let totals = row(&mut rows, model, seat.as_deref());
            match kind.as_str() {
                "session_ended" => {
                    totals.tasks.extend(task);
                    totals.goals.extend(goal);
                    totals.row.tokens = totals
                        .row
                        .tokens
                        .saturating_add(number("input_tokens"))
                        .saturating_add(number("output_tokens"));
                    totals.row.time_secs += data["lifetime_secs"].as_f64().unwrap_or(0.0);
                }
                "message" => totals.row.messages += 1,
                "task_ended" if data["status"] == "finished" => {
                    totals.finished += 1;
                    totals.rounds += number("review_requests") as f64;
                }
                "verdict" => {
                    totals.changes += u64::from(
                        data["verdict"].as_str() == Some(MessageKind::RequestChanges.as_str()),
                    );
                    totals.reviewed_tasks.extend(task);
                }
                _ => {}
            }
        }
        let items = rows
            .into_values()
            .map(|mut totals| {
                let row = &mut totals.row;
                row.tasks = totals.tasks.len() as u64;
                row.goals = totals.goals.len() as u64;
                match row.seat.as_deref() {
                    Some("author") => {
                        row.rounds_per_task = Some(ratio(totals.rounds, totals.finished))
                    }
                    Some("reviewer") => {
                        row.changes_per_task = Some(ratio(
                            totals.changes as f64,
                            totals.reviewed_tasks.len() as u64,
                        ))
                    }
                    _ => {}
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

    struct Fact<'a> {
        kind: &'a str,
        model: Option<&'a str>,
        seat: Option<&'a str>,
        goal: Option<&'a str>,
        task: Option<&'a str>,
        data: Value,
    }

    impl Default for Fact<'_> {
        fn default() -> Self {
            Self {
                kind: "session_ended",
                model: Some("writer"),
                seat: Some("author"),
                goal: None,
                task: None,
                data: json!({}),
            }
        }
    }

    async fn record(store: &Store, fact: Fact<'_>) {
        store
            .record_fact(NewStatFact {
                kind: fact.kind.into(),
                model: fact.model.map(str::to_owned),
                seat: fact.seat.map(str::to_owned),
                repo_id: Some("repo".into()),
                task_id: fact.task.map(str::to_owned),
                goal_id: fact.goal.map(str::to_owned),
                session_id: None,
                launch_id: None,
                effort: None,
                skills: vec![],
                data: fact.data,
            })
            .await
            .unwrap();
    }

    async fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("stats.db")).await.unwrap();
        (dir, store)
    }

    fn find<'a>(stats: &'a ModelStats, model: &str, seat: Option<&str>) -> &'a ModelStat {
        stats
            .items
            .iter()
            .find(|r| r.model == model && r.seat.as_deref() == seat)
            .unwrap()
    }

    #[tokio::test]
    async fn each_figure_counts_the_sessions_and_messages_of_its_model_and_seat() {
        let (_dir, store) = store().await;
        for (seat, goal, task, input, cached, output, lifetime) in [
            ("author", "g1", Some("t1"), 100, 40, 20, 30.0),
            ("author", "g1", Some("t1"), 200, 200, 30, 60.5),
            ("author", "g2", Some("t2"), 300, 0, 50, 10.0),
            ("orchestrator", "g1", None, 1_000, 0, 100, 500.0),
        ] {
            record(
                &store,
                Fact {
                    seat: Some(seat),
                    goal: Some(goal),
                    task,
                    data: json!({"input_tokens": input, "cached_input_tokens": cached,
                    "output_tokens": output, "lifetime_secs": lifetime}),
                    ..Fact::default()
                },
            )
            .await;
        }
        for seat in ["author", "author", "orchestrator"] {
            record(
                &store,
                Fact {
                    kind: "message",
                    seat: Some(seat),
                    task: Some("t3"),
                    ..Fact::default()
                },
            )
            .await;
        }
        record(
            &store,
            Fact {
                kind: "message",
                model: None,
                ..Fact::default()
            },
        )
        .await;

        let stats = store.model_stats(&StatsFilter::default()).await.unwrap();
        assert_eq!(stats.items.len(), 2);
        assert_eq!(
            find(&stats, "writer", Some("author")),
            &ModelStat {
                model: "writer".into(),
                seat: Some("author".into()),
                tasks: 2,
                goals: 2,
                tokens: 700,
                time_secs: 100.5,
                messages: 2,
                rounds_per_task: Some(0.0),
                changes_per_task: None,
            }
        );
        assert_eq!(
            find(&stats, "writer", Some("orchestrator")),
            &ModelStat {
                model: "writer".into(),
                seat: Some("orchestrator".into()),
                tasks: 0,
                goals: 1,
                tokens: 1_100,
                time_secs: 500.0,
                messages: 1,
                rounds_per_task: None,
                changes_per_task: None,
            }
        );
    }

    #[tokio::test]
    async fn rounds_per_task_is_the_mean_review_requests_of_finished_tasks_for_authors_only() {
        let (_dir, store) = store().await;
        for (model, seat, status, rounds) in [
            ("writer", Some("author"), "finished", 1),
            ("writer", Some("author"), "finished", 4),
            ("writer", Some("author"), "failed", 9),
            ("writer", Some("author"), "cancelled", 9),
            ("other", Some("author"), "finished", 7),
            ("writer", None, "finished", 5),
        ] {
            record(
                &store,
                Fact {
                    kind: "task_ended",
                    model: Some(model),
                    seat,
                    task: Some("t"),
                    data: json!({"status": status, "review_requests": rounds}),
                    ..Fact::default()
                },
            )
            .await;
        }
        let stats = store.model_stats(&StatsFilter::default()).await.unwrap();
        assert_eq!(
            find(&stats, "writer", Some("author")).rounds_per_task,
            Some(2.5)
        );
        assert_eq!(
            find(&stats, "other", Some("author")).rounds_per_task,
            Some(7.0)
        );
        let seatless = find(&stats, "writer", None);
        assert_eq!(
            (seatless.rounds_per_task, seatless.changes_per_task),
            (None, None)
        );
        assert_eq!(
            find(&stats, "writer", Some("author")).changes_per_task,
            None
        );
    }

    #[tokio::test]
    async fn changes_per_task_divides_changes_requested_by_the_tasks_reviewed_for_reviewers_only() {
        let (_dir, store) = store().await;
        for (model, task, verdict) in [
            ("judge", "t1", "request_changes"),
            ("judge", "t1", "request_changes"),
            ("judge", "t1", "approve"),
            ("judge", "t2", "approve"),
            ("judge", "t3", "request_changes"),
            ("judge", "t4", "approve"),
            ("other", "t1", "approve"),
        ] {
            record(
                &store,
                Fact {
                    kind: "verdict",
                    model: Some(model),
                    seat: Some("reviewer"),
                    task: Some(task),
                    data: json!({"verdict": verdict, "author_model": "writer"}),
                    ..Fact::default()
                },
            )
            .await;
        }
        let stats = store.model_stats(&StatsFilter::default()).await.unwrap();
        let judge = find(&stats, "judge", Some("reviewer"));
        assert_eq!(
            (judge.changes_per_task, judge.rounds_per_task),
            (Some(0.75), None)
        );
        assert_eq!(
            find(&stats, "other", Some("reviewer")).changes_per_task,
            Some(0.0)
        );
        // The author model the verdicts name gets a row of its own.
        assert_eq!(
            find(&stats, "writer", Some("author")).rounds_per_task,
            Some(0.0)
        );
    }

    #[tokio::test]
    async fn the_review_figures_are_zero_without_a_denominator() {
        let (_dir, store) = store().await;
        for (seat, kind) in [("author", "message"), ("reviewer", "session_ended")] {
            record(
                &store,
                Fact {
                    kind,
                    seat: Some(seat),
                    task: Some("t"),
                    ..Fact::default()
                },
            )
            .await;
        }
        let stats = store.model_stats(&StatsFilter::default()).await.unwrap();
        assert_eq!(
            find(&stats, "writer", Some("author")).rounds_per_task,
            Some(0.0)
        );
        assert_eq!(
            find(&stats, "writer", Some("reviewer")).changes_per_task,
            Some(0.0)
        );
    }

    #[tokio::test]
    async fn every_fact_is_filtered_by_time_and_repository() {
        let (_dir, store) = store().await;
        for kind in ["session_ended", "message", "task_ended", "verdict"] {
            record(
                &store,
                Fact {
                    kind,
                    seat: Some(if kind == "verdict" {
                        "reviewer"
                    } else {
                        "author"
                    }),
                    task: Some("old"),
                    data: json!({"status": "finished", "review_requests": 9, "input_tokens": 900,
                        "verdict": "request_changes"}),
                    ..Fact::default()
                },
            )
            .await;
        }
        sqlx::query("UPDATE stat_facts SET created_at = '2026-01-01T00:00:00.000Z'")
            .execute(store.w())
            .await
            .unwrap();
        for (repo, since) in [
            (Some("elsewhere"), None),
            (None, Some("2026-01-01T00:00:00.001Z")),
        ] {
            let filter = StatsFilter {
                repo_id: repo.map(str::to_owned),
                since: since.map(|s| s.parse().unwrap()),
            };
            assert!(store.model_stats(&filter).await.unwrap().items.is_empty());
        }
        let boundary = StatsFilter {
            repo_id: Some("repo".into()),
            since: Some("2026-01-01T00:00:00Z".parse().unwrap()),
        };
        let stats = store.model_stats(&boundary).await.unwrap();
        assert_eq!(find(&stats, "writer", Some("author")).tokens, 900);
        assert_eq!(
            find(&stats, "writer", Some("reviewer")).changes_per_task,
            Some(1.0)
        );
        for kind in ["session_ended", "task_ended", "verdict"] {
            record(
                &store,
                Fact {
                    kind,
                    seat: Some(if kind == "verdict" {
                        "reviewer"
                    } else {
                        "author"
                    }),
                    task: Some("new"),
                    data: json!({"status": "finished", "review_requests": 1, "input_tokens": 10,
                        "verdict": "approve"}),
                    ..Fact::default()
                },
            )
            .await;
        }
        let recent = StatsFilter {
            repo_id: Some("repo".into()),
            since: Some("2026-02-01T00:00:00Z".parse().unwrap()),
        };
        let stats = store.model_stats(&recent).await.unwrap();
        let row = find(&stats, "writer", Some("author"));
        assert_eq!(
            (row.tasks, row.tokens, row.messages, row.rounds_per_task),
            (1, 10, 0, Some(1.0))
        );
        assert_eq!(
            find(&stats, "writer", Some("reviewer")).changes_per_task,
            Some(0.0)
        );
    }

    #[tokio::test]
    async fn rows_sort_by_seat_then_model() {
        let (_dir, store) = store().await;
        for (model, seat) in [
            ("z", None),
            ("z", Some("reviewer")),
            ("z", Some("author")),
            ("z", Some("orchestrator")),
            ("a", Some("orchestrator")),
            ("b", Some("author")),
        ] {
            record(
                &store,
                Fact {
                    model: Some(model),
                    seat,
                    ..Fact::default()
                },
            )
            .await;
        }
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
                (Some("author"), "b"),
                (Some("author"), "z"),
                (Some("reviewer"), "z"),
                (None, "z"),
            ]
        );
    }

    #[tokio::test]
    async fn models_named_only_in_payloads_and_other_facts_get_rows_under_both_filters() {
        let (_dir, store) = store().await;
        let facts = [
            Fact {
                kind: "verdict",
                model: None,
                seat: Some("reviewer"),
                data: json!({"author_model": "payload", "verdict": "approve"}),
                ..Fact::default()
            },
            Fact {
                kind: "pick",
                model: None,
                seat: None,
                data: json!({"winner_model": "winner", "loser_models": ["loser", "loser"]}),
                ..Fact::default()
            },
            Fact {
                kind: "switch",
                model: Some("switched"),
                data: json!({"reason": "exhausted"}),
                ..Fact::default()
            },
            Fact {
                kind: "permission",
                model: Some("asked"),
                data: json!({"decided_by": "console", "wait_ms": 1_000}),
                ..Fact::default()
            },
            Fact {
                kind: "attention",
                model: Some("stuck"),
                data: json!({"reason": "stalled", "wait_secs": 5}),
                ..Fact::default()
            },
            Fact {
                kind: "permission",
                model: Some("ai-only"),
                data: json!({"decided_by": "ai", "wait_ms": 1_000}),
                ..Fact::default()
            },
            Fact {
                kind: "attention",
                model: Some("prompted"),
                data: json!({"reason": "waiting_permission", "wait_secs": 5}),
                ..Fact::default()
            },
        ];
        for fact in facts {
            record(&store, fact).await;
        }
        let empty = ModelStat {
            seat: Some("author".into()),
            rounds_per_task: Some(0.0),
            ..ModelStat::default()
        };
        let named: Vec<ModelStat> = ["asked", "loser", "payload", "stuck", "switched", "winner"]
            .into_iter()
            .map(|model| ModelStat {
                model: model.into(),
                ..empty.clone()
            })
            .collect();
        let all = store.model_stats(&StatsFilter::default()).await.unwrap();
        assert_eq!(all.items, named);
        let kept = StatsFilter {
            repo_id: Some("repo".into()),
            since: Some("2000-01-01T00:00:00Z".parse().unwrap()),
        };
        assert_eq!(store.model_stats(&kept).await.unwrap().items, named);
        for filter in [
            StatsFilter {
                repo_id: Some("elsewhere".into()),
                since: None,
            },
            StatsFilter {
                repo_id: None,
                since: Some("2999-01-01T00:00:00Z".parse().unwrap()),
            },
        ] {
            assert!(store.model_stats(&filter).await.unwrap().items.is_empty());
        }
    }
}
