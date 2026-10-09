//! The `models` family: which model does the job.

use std::collections::{BTreeMap, BTreeSet};

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
    /// The tasks this model's column ended `finished`.
    pub tasks_finished: u64,
}

/// Intermediate totals retain task and goal identities until all filtered facts are read.
#[derive(Default)]
struct Totals {
    row: ModelStat,
    tasks: BTreeSet<String>,
    goals: BTreeSet<String>,
}

type Rows = BTreeMap<(u8, String), Totals>;

fn row<'a>(rows: &'a mut Rows, model: &str, seat: Option<&str>) -> &'a mut Totals {
    let order = match seat {
        Some("orchestrator") => 0,
        Some("agent") => 1,
        _ => 2,
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

impl Store {
    /// Which model does the job, over the facts the filter keeps.
    pub async fn model_stats(&self, filter: &StatsFilter) -> Result<ModelStats> {
        let (clause, binds) = narrowed(filter);
        // Every fact that names a model gives it a row, even where it adds to no figure.
        // Only a person's answer and a flag a person cleared count; a `waiting_permission`
        // attention fact is left out, because its prompt is already a permission.
        let sql = format!(
            "SELECT kind, model, seat, goal_id, task_id, data FROM stat_facts
             WHERE (kind IN ('session_ended', 'message', 'task_ended', 'switch')
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
                "task_ended" if data["status"] == "finished" => totals.row.tasks_finished += 1,
                _ => {}
            }
        }
        let items = rows
            .into_values()
            .map(|mut totals| {
                totals.row.tasks = totals.tasks.len() as u64;
                totals.row.goals = totals.goals.len() as u64;
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
                seat: Some("agent"),
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
            ("agent", "g1", Some("t1"), 100, 40, 20, 30.0),
            ("agent", "g1", Some("t1"), 200, 200, 30, 60.5),
            ("agent", "g2", Some("t2"), 300, 0, 50, 10.0),
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
        for seat in ["agent", "agent", "orchestrator"] {
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
            find(&stats, "writer", Some("agent")),
            &ModelStat {
                model: "writer".into(),
                seat: Some("agent".into()),
                tasks: 2,
                goals: 2,
                tokens: 700,
                time_secs: 100.5,
                messages: 2,
                tasks_finished: 0,
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
                tasks_finished: 0,
            }
        );
    }

    /// A finished task counts for the model of the column it ended in, and
    /// a task that failed or was cancelled counts for nobody.
    #[tokio::test]
    async fn tasks_finished_counts_the_finished_endings_of_a_models_column() {
        let (_dir, store) = store().await;
        for (model, status) in [
            ("lander", "finished"),
            ("lander", "finished"),
            ("lander", "failed"),
            ("lander", "cancelled"),
            ("other", "finished"),
        ] {
            record(
                &store,
                Fact {
                    kind: "task_ended",
                    model: Some(model),
                    task: Some("t"),
                    data: json!({"status": status, "landed": status == "finished"}),
                    ..Fact::default()
                },
            )
            .await;
        }
        let stats = store.model_stats(&StatsFilter::default()).await.unwrap();
        assert_eq!(find(&stats, "lander", Some("agent")).tasks_finished, 2);
        assert_eq!(find(&stats, "other", Some("agent")).tasks_finished, 1);
    }

    #[tokio::test]
    async fn every_fact_is_filtered_by_time_and_repository() {
        let (_dir, store) = store().await;
        for kind in ["session_ended", "message", "task_ended"] {
            record(
                &store,
                Fact {
                    kind,
                    task: Some("old"),
                    data: json!({"status": "finished", "input_tokens": 900}),
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
        assert_eq!(find(&stats, "writer", Some("agent")).tokens, 900);
        assert_eq!(find(&stats, "writer", Some("agent")).tasks_finished, 1);
        for kind in ["session_ended", "task_ended"] {
            record(
                &store,
                Fact {
                    kind,
                    task: Some("new"),
                    data: json!({"status": "finished", "input_tokens": 10}),
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
        let row = find(&stats, "writer", Some("agent"));
        assert_eq!(
            (row.tasks, row.tokens, row.messages, row.tasks_finished),
            (1, 10, 0, 1)
        );
    }

    #[tokio::test]
    async fn rows_sort_by_seat_then_model() {
        let (_dir, store) = store().await;
        for (model, seat) in [
            ("z", None),
            ("z", Some("agent")),
            ("z", Some("orchestrator")),
            ("a", Some("orchestrator")),
            ("b", Some("agent")),
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
                (Some("agent"), "b"),
                (Some("agent"), "z"),
                (None, "z"),
            ]
        );
    }

    #[tokio::test]
    async fn models_named_in_other_facts_get_rows_under_both_filters() {
        let (_dir, store) = store().await;
        let facts = [
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
            seat: Some("agent".into()),
            ..ModelStat::default()
        };
        let named: Vec<ModelStat> = ["asked", "stuck", "switched"]
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
