//! The `tools` family: what the agents do.

use std::collections::HashMap;

use super::{StatsFilter, median, narrowed, p90};
use crate::{Result, Store};

/// What the `tools` family answers: the tool mix by kind, the top tools, and
/// the errors.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ToolStats {
    pub calls: u64,
    pub errors: u64,
    /// The count of distinct tool names.
    pub tools: u64,
    /// One row per kind, the most calls first.
    pub by_kind: Vec<ToolKindStats>,
    /// The tools with the most calls, up to the limit asked for.
    pub top: Vec<ToolStatRow>,
    /// The tools beyond `top`, summed.
    pub other: OtherTools,
}

/// How a kind of tool call performed: how often it ran, how many failed, and
/// how long its calls took.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolKindStats {
    pub kind: String,
    pub calls: u64,
    pub errors: u64,
    pub median_duration_ms: f64,
    pub p90_duration_ms: f64,
}

/// How often one tool ran, how many of its calls failed, and how long they
/// took.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolStatRow {
    pub tool_name: String,
    pub kind: String,
    pub calls: u64,
    pub errors: u64,
    pub median_duration_ms: f64,
    pub p90_duration_ms: f64,
}

/// The tools beyond the limit, summed into one row.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OtherTools {
    pub tools: u64,
    pub calls: u64,
    pub errors: u64,
}

/// One ended tool call, as the ledger holds it.
struct ToolCallFact {
    tool_name: String,
    kind: String,
    duration_ms: f64,
    failed: bool,
}

/// A tool's calls as they fold in: its kinds counted apart, since the same
/// stable name can carry more than one — a row names it by whichever kind
/// most of its calls carry.
#[derive(Default)]
struct ToolAgg {
    calls: u64,
    errors: u64,
    durations: Vec<f64>,
    kinds: HashMap<String, u64>,
}

impl Store {
    /// What the agents do, over the facts the filter keeps: the tool mix by
    /// kind, the `limit` tools with the most calls, and the rest summed into
    /// `other`. A `tool_call` fact with no `kind` predates the stable name
    /// and is not counted.
    pub async fn tools_stats(&self, filter: &StatsFilter, limit: usize) -> Result<ToolStats> {
        let (narrow, binds) = narrowed(filter);
        let mut query =
            sqlx::query_as::<_, (String, String, i64, i64)>(sqlx::AssertSqlSafe(format!(
                "SELECT json_extract(data, '$.tool_name'), json_extract(data, '$.kind'),
                        json_extract(data, '$.duration_ms'), json_extract(data, '$.ok')
                   FROM stat_facts
                  WHERE kind = 'tool_call' AND json_extract(data, '$.kind') IS NOT NULL{narrow}"
            )));
        for bind in &binds {
            query = query.bind(bind);
        }
        let rows: Vec<ToolCallFact> = query
            .fetch_all(self.r())
            .await?
            .into_iter()
            .map(|(tool_name, kind, duration_ms, ok)| ToolCallFact {
                tool_name,
                kind,
                duration_ms: duration_ms.max(0) as f64,
                failed: ok == 0,
            })
            .collect();

        let calls = rows.len() as u64;
        let errors = rows.iter().filter(|row| row.failed).count() as u64;

        let mut by_kind: HashMap<String, (u64, u64, Vec<f64>)> = HashMap::new();
        let mut by_tool: HashMap<String, ToolAgg> = HashMap::new();
        for row in &rows {
            let kind_row = by_kind.entry(row.kind.clone()).or_default();
            kind_row.0 += 1;
            kind_row.1 += u64::from(row.failed);
            kind_row.2.push(row.duration_ms);

            let tool_row = by_tool.entry(row.tool_name.clone()).or_default();
            tool_row.calls += 1;
            tool_row.errors += u64::from(row.failed);
            tool_row.durations.push(row.duration_ms);
            *tool_row.kinds.entry(row.kind.clone()).or_default() += 1;
        }

        let mut by_kind: Vec<ToolKindStats> = by_kind
            .into_iter()
            .map(|(kind, (calls, errors, durations))| ToolKindStats {
                kind,
                calls,
                errors,
                median_duration_ms: median(&durations),
                p90_duration_ms: p90(&durations),
            })
            .collect();
        by_kind.sort_by(|a, b| b.calls.cmp(&a.calls).then_with(|| a.kind.cmp(&b.kind)));

        let mut tools: Vec<ToolStatRow> = by_tool
            .into_iter()
            .map(|(tool_name, agg)| {
                let mut kinds: Vec<(String, u64)> = agg.kinds.into_iter().collect();
                kinds.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
                ToolStatRow {
                    tool_name,
                    kind: kinds
                        .into_iter()
                        .next()
                        .map(|(kind, _)| kind)
                        .unwrap_or_default(),
                    calls: agg.calls,
                    errors: agg.errors,
                    median_duration_ms: median(&agg.durations),
                    p90_duration_ms: p90(&agg.durations),
                }
            })
            .collect();
        tools.sort_by(|a, b| {
            b.calls
                .cmp(&a.calls)
                .then_with(|| a.tool_name.cmp(&b.tool_name))
        });

        let distinct_tools = tools.len() as u64;
        let rest = if tools.len() > limit {
            tools.split_off(limit)
        } else {
            Vec::new()
        };
        let other = OtherTools {
            tools: rest.len() as u64,
            calls: rest.iter().map(|row| row.calls).sum(),
            errors: rest.iter().map(|row| row.errors).sum(),
        };

        Ok(ToolStats {
            calls,
            errors,
            tools: distinct_tools,
            by_kind,
            top: tools,
            other,
        })
    }
}
