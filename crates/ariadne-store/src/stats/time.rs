//! The `time` family: how long does it take?

use super::{StatsFilter, median, narrowed, p90};
use crate::{Result, Store};

const STATUSES: [&str; 6] = [
    "pending",
    "ready",
    "in_progress",
    "under_review",
    "changes_requested",
    "approved",
];

#[derive(Debug, Clone, Default, PartialEq)]
pub struct LeadTime {
    pub median_secs: f64,
    pub p90_secs: f64,
    pub mean_secs: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StatusTime {
    pub status: String,
    pub total_secs: f64,
    pub median_secs: f64,
    pub share: f64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PersonWait {
    pub prompts: u64,
    pub total_secs: f64,
    pub median_secs: f64,
}

/// What the `time` family answers.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TimeStats {
    pub tasks: u64,
    pub lead_time: LeadTime,
    pub in_status: Vec<StatusTime>,
    pub waiting_on_person: PersonWait,
}

impl Store {
    /// How long finished work took, where its time went, and how long it waited on a person.
    pub async fn time_stats(&self, filter: &StatsFilter) -> Result<TimeStats> {
        let (narrowing, binds) = narrowed(filter);
        let mut task_query = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT data FROM stat_facts WHERE kind = 'task_ended' AND json_extract(data, '$.status') = 'finished'{narrowing}"
        )));
        for bind in &binds {
            task_query = task_query.bind(bind);
        }
        let task_data: Vec<String> = task_query.fetch_all(self.r()).await?;
        let mut lead_times = Vec::with_capacity(task_data.len());
        let mut per_status: Vec<Vec<f64>> = (0..STATUSES.len()).map(|_| Vec::new()).collect();
        for data in task_data {
            let data: serde_json::Value = serde_json::from_str(&data).unwrap_or_default();
            lead_times.push(number_at(&data, "lead_time_secs"));
            let Some(status_secs) = data
                .get("status_secs")
                .and_then(serde_json::Value::as_object)
            else {
                continue;
            };
            for (index, status) in STATUSES.iter().enumerate() {
                if let Some(value) = status_secs.get(*status).and_then(serde_json::Value::as_f64) {
                    per_status[index].push(value.max(0.0));
                }
            }
        }
        let lead_total: f64 = lead_times.iter().sum();
        let statuses: Vec<StatusTime> = STATUSES
            .iter()
            .zip(per_status)
            .map(|(status, values)| StatusTime {
                status: (*status).to_string(),
                total_secs: values.iter().sum::<f64>().max(0.0),
                median_secs: median(&values),
                share: 0.0,
            })
            .collect();
        let status_total: f64 = statuses.iter().map(|row| row.total_secs).sum();
        let in_status = statuses
            .into_iter()
            .map(|row| StatusTime {
                share: if status_total == 0.0 {
                    0.0
                } else {
                    row.total_secs / status_total
                },
                ..row
            })
            .collect();

        let mut wait_query = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT data FROM stat_facts WHERE kind = 'permission' AND json_extract(data, '$.decided_by') = 'console'{narrowing}"
        )));
        for bind in &binds {
            wait_query = wait_query.bind(bind);
        }
        let waits: Vec<String> = wait_query.fetch_all(self.r()).await?;
        let waits: Vec<f64> = waits
            .into_iter()
            .map(|data| {
                number_at(&serde_json::from_str(&data).unwrap_or_default(), "wait_ms") / 1_000.0
            })
            .collect();
        Ok(TimeStats {
            tasks: lead_times.len() as u64,
            lead_time: LeadTime {
                median_secs: median(&lead_times),
                p90_secs: p90(&lead_times),
                mean_secs: if lead_times.is_empty() {
                    0.0
                } else {
                    lead_total / lead_times.len() as f64
                },
            },
            in_status,
            waiting_on_person: PersonWait {
                prompts: waits.len() as u64,
                total_secs: waits.iter().sum::<f64>().max(0.0),
                median_secs: median(&waits),
            },
        })
    }
}

fn number_at(data: &serde_json::Value, key: &str) -> f64 {
    data.get(key)
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(0.0)
        .max(0.0)
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, Utc};

    use super::*;
    use crate::NewStatFact;

    fn fact(kind: &str, repo_id: &str, data: serde_json::Value) -> NewStatFact {
        NewStatFact {
            kind: kind.into(),
            repo_id: Some(repo_id.into()),
            goal_id: None,
            task_id: None,
            session_id: None,
            launch_id: None,
            seat: None,
            model: None,
            effort: None,
            skills: vec![],
            data,
        }
    }

    #[tokio::test]
    async fn time_stats_measures_finished_tasks_statuses_and_person_waits() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("stats.db")).await.unwrap();
        store
            .record_fact(fact(
                "task_ended",
                "one",
                serde_json::json!({
                    "status": "finished", "lead_time_secs": 10,
                    "status_secs": {"pending": 2, "ready": 3, "in_progress": 5}
                }),
            ))
            .await
            .unwrap();
        store
            .record_fact(fact(
                "task_ended",
                "one",
                serde_json::json!({
                    "status": "finished", "lead_time_secs": 30,
                    "status_secs": {"pending": 4, "ready": 6, "in_progress": 20}
                }),
            ))
            .await
            .unwrap();
        store
            .record_fact(fact(
                "task_ended",
                "two",
                serde_json::json!({
                    "status": "finished", "lead_time_secs": 90
                }),
            ))
            .await
            .unwrap();
        store
            .record_fact(fact(
                "permission",
                "one",
                serde_json::json!({"decided_by": "console", "wait_ms": 1_000}),
            ))
            .await
            .unwrap();
        store
            .record_fact(fact(
                "permission",
                "one",
                serde_json::json!({"decided_by": "console", "wait_ms": 3_000}),
            ))
            .await
            .unwrap();
        store
            .record_fact(fact(
                "permission",
                "one",
                serde_json::json!({"decided_by": "ai", "wait_ms": 99_000}),
            ))
            .await
            .unwrap();

        let stats = store
            .time_stats(&StatsFilter {
                repo_id: Some("one".into()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(stats.tasks, 2);
        assert_eq!(
            stats.lead_time,
            LeadTime {
                median_secs: 20.0,
                p90_secs: 30.0,
                mean_secs: 20.0
            }
        );
        assert_eq!(
            stats.in_status[0],
            StatusTime {
                status: "pending".into(),
                total_secs: 6.0,
                median_secs: 3.0,
                share: 0.15
            }
        );
        assert_eq!(
            stats.in_status[2],
            StatusTime {
                status: "in_progress".into(),
                total_secs: 25.0,
                median_secs: 12.5,
                share: 0.625
            }
        );
        assert_eq!(
            stats.waiting_on_person,
            PersonWait {
                prompts: 2,
                total_secs: 4.0,
                median_secs: 2.0
            }
        );

        let future = StatsFilter {
            since: Some(Utc::now() + Duration::days(1)),
            repo_id: Some("one".into()),
        };
        let future = store.time_stats(&future).await.unwrap();
        assert_eq!(future.tasks, 0);
        assert_eq!(future.waiting_on_person, PersonWait::default());
    }
}
