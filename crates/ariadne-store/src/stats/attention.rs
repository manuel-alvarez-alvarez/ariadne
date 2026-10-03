//! The `attention` family: how much it needed a person.

use std::collections::BTreeMap;

use serde_json::Value;

use super::{StatsFilter, narrowed};
use crate::{Result, Store};

const FLAGS: [&str; 5] = [
    "waiting_permission",
    "waiting_input",
    "waiting_user",
    "stalled",
    "agent_error",
];

/// What the `attention` family answers.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AttentionStats {
    pub permissions: PermissionStats,
    pub flags: Vec<AttentionFlag>,
    pub sessions_failed: u64,
    pub sessions_stalled: u64,
    pub exhaustions: u64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PermissionStats {
    pub total: u64,
    pub person_share: f64,
    pub by_decider: Vec<PermissionDecider>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PermissionDecider {
    pub decided_by: String,
    pub total: u64,
    pub allowed: u64,
    pub denied: u64,
    pub cancelled: u64,
    pub mean_wait_ms: f64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AttentionFlag {
    pub reason: String,
    pub raised: u64,
    pub mean_wait_secs: f64,
}

#[derive(Default)]
struct DeciderTotals {
    total: u64,
    allowed: u64,
    denied: u64,
    cancelled: u64,
    wait_ms: u64,
}

#[derive(Default)]
struct FlagTotals {
    raised: u64,
    wait_secs: u64,
    clears: u64,
}

impl Store {
    /// How much it needed a person, over the facts the filter keeps.
    pub async fn attention_stats(&self, filter: &StatsFilter) -> Result<AttentionStats> {
        let (narrowed, binds) = narrowed(filter);
        let mut query = sqlx::query_as::<_, (String, String)>(sqlx::AssertSqlSafe(format!(
            "SELECT kind, data FROM stat_facts WHERE kind IN ('permission', 'attention', \
             'session_ended', 'switch'){narrowed}"
        )));
        for bind in binds {
            query = query.bind(bind);
        }
        let facts = query.fetch_all(self.r()).await?;

        let mut deciders = BTreeMap::<String, DeciderTotals>::new();
        let mut flags = FLAGS
            .into_iter()
            .map(|reason| (reason.to_string(), FlagTotals::default()))
            .collect::<BTreeMap<_, _>>();
        let mut person_answers = 0;
        let mut sessions_failed = 0;
        let mut sessions_stalled = 0;
        let mut exhaustions = 0;

        for (kind, data) in facts {
            let Ok(data) = serde_json::from_str::<Value>(&data) else {
                continue;
            };
            match kind.as_str() {
                "permission" => {
                    let Some(decided_by) = string(&data, "decided_by") else {
                        continue;
                    };
                    let totals = deciders.entry(decided_by.to_string()).or_default();
                    totals.total += 1;
                    totals.wait_ms += number(&data, "wait_ms");
                    match string(&data, "answer") {
                        Some("allow") => totals.allowed += 1,
                        Some("deny") => totals.denied += 1,
                        Some("cancelled") => totals.cancelled += 1,
                        _ => {}
                    }
                    if decided_by == "console" {
                        person_answers += 1;
                    }
                }
                "attention" => {
                    if let Some(totals) =
                        string(&data, "reason").and_then(|reason| flags.get_mut(reason))
                    {
                        totals.raised += 1;
                        totals.wait_secs += number(&data, "wait_secs");
                        totals.clears += 1;
                    }
                }
                "session_ended" => {
                    if string(&data, "status") == Some("failed") {
                        sessions_failed += 1;
                    }
                    if string(&data, "attention_reason") == Some("stalled") {
                        sessions_stalled += 1;
                    }
                    if let Some(totals) =
                        string(&data, "attention_reason").and_then(|reason| flags.get_mut(reason))
                    {
                        totals.raised += 1;
                    }
                }
                "switch" if string(&data, "reason") == Some("exhausted") => exhaustions += 1,
                _ => {}
            }
        }

        let total = deciders.values().map(|row| row.total).sum::<u64>();
        let mut by_decider = deciders
            .into_iter()
            .map(|(decided_by, row)| PermissionDecider {
                decided_by,
                total: row.total,
                allowed: row.allowed,
                denied: row.denied,
                cancelled: row.cancelled,
                mean_wait_ms: mean(row.wait_ms, row.total),
            })
            .collect::<Vec<_>>();
        by_decider.sort_by(|left, right| {
            right
                .total
                .cmp(&left.total)
                .then_with(|| left.decided_by.cmp(&right.decided_by))
        });
        Ok(AttentionStats {
            permissions: PermissionStats {
                total,
                person_share: mean(person_answers, total),
                by_decider,
            },
            flags: flags
                .into_iter()
                .map(|(reason, totals)| AttentionFlag {
                    reason,
                    raised: totals.raised,
                    mean_wait_secs: mean(totals.wait_secs, totals.clears),
                })
                .collect(),
            sessions_failed,
            sessions_stalled,
            exhaustions,
        })
    }
}

fn string<'a>(data: &'a Value, key: &str) -> Option<&'a str> {
    data.get(key)?.as_str()
}

fn number(data: &Value, key: &str) -> u64 {
    data.get(key).and_then(Value::as_u64).unwrap_or(0)
}

fn mean(sum: u64, count: u64) -> f64 {
    if count == 0 {
        0.0
    } else {
        sum as f64 / count as f64
    }
}
