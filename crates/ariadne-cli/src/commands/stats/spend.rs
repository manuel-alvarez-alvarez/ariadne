//! `ariadne stats spend`: what the work spent.

use anyhow::Result;

use ariadne_api::stats::{BucketDto, ModelSpendDto, SpendBucketDto, SpendStatsDto, StatsQuery};
use ariadne_api::usage::TokenUsageDto;
use ariadne_client::Client;

use crate::commands::query_path;
use crate::output::{
    Column, Format, UNCAPPED, col, kv_block, note, print, render_table, style, tokens, usage_cell,
    view,
};

/// Columns of the `by_model` table. `tokens` is the usage cell every table
/// prints: the input, the cached share and the output in one glance.
const MODEL_COLUMNS: &[Column] = &[
    col("model", UNCAPPED),
    col("tokens", UNCAPPED),
    col("share", UNCAPPED),
];

/// Columns of the `buckets` table: one row per point of the time axis.
const BUCKET_COLUMNS: &[Column] = &[
    col("from", UNCAPPED),
    col("input", UNCAPPED),
    col("cached", UNCAPPED),
    col("output", UNCAPPED),
];

pub(super) async fn run(client: &Client, query: &StatsQuery, format: Format) -> Result<()> {
    let stats: SpendStatsDto = client
        .get_json(&query_path("/v1/stats/spend", query)?)
        .await?;
    let text = match format {
        Format::Table if !is_empty(&stats) => Some(render(&stats)?),
        _ => None,
    };
    print(format, &stats, || match text {
        Some(text) => println!("{text}"),
        None => note(&style::paint(
            view().color,
            style::META,
            "Nothing was spent in that span.",
        )),
    })
}

/// Nothing to spend: no session ended in the span, so the totals, the
/// per-model rows and the buckets all hold nothing either.
fn is_empty(stats: &SpendStatsDto) -> bool {
    stats.totals.sessions == 0
}

/// The totals and the per-task figures as the aligned key-value block every
/// family uses, then the `by_model` table and the `buckets` table, one
/// blank line between each.
fn render(stats: &SpendStatsDto) -> Result<String> {
    let totals = kv_block(&totals_kv(stats), view());
    let by_model = render_table(MODEL_COLUMNS, &model_rows(&stats.by_model), view())?;
    let buckets = render_table(
        BUCKET_COLUMNS,
        &bucket_rows(stats.bucket, &stats.buckets),
        view(),
    )?;
    Ok(format!("{totals}\n\n{by_model}\n\n{buckets}"))
}

/// The totals and the per-task figures as `(label, value)` pairs, in the
/// order a person reads them.
fn totals_kv(stats: &SpendStatsDto) -> Vec<(&'static str, String)> {
    let totals = &stats.totals;
    let per_task = &stats.per_finished_task;
    vec![
        ("sessions", totals.sessions.to_string()),
        ("input tokens", tokens(totals.input_tokens)),
        ("cached input tokens", tokens(totals.cached_input_tokens)),
        ("output tokens", tokens(totals.output_tokens)),
        ("cached share", percent(totals.cached_share)),
        ("finished tasks", per_task.tasks.to_string()),
        (
            "input tokens per task",
            tokens(per_task.input_tokens.round() as u64),
        ),
        (
            "output tokens per task",
            tokens(per_task.output_tokens.round() as u64),
        ),
    ]
}

fn model_rows(models: &[ModelSpendDto]) -> Vec<Vec<String>> {
    models
        .iter()
        .map(|m| {
            let usage = TokenUsageDto {
                input_tokens: m.input_tokens,
                cached_input_tokens: m.cached_input_tokens,
                output_tokens: m.output_tokens,
            };
            vec![m.model.clone(), usage_cell(&usage), percent(m.share)]
        })
        .collect()
}

fn bucket_rows(bucket: BucketDto, buckets: &[SpendBucketDto]) -> Vec<Vec<String>> {
    buckets
        .iter()
        .map(|b| {
            vec![
                bucket_date(bucket, &b.start),
                tokens(b.input_tokens),
                tokens(b.cached_input_tokens),
                tokens(b.output_tokens),
            ]
        })
        .collect()
}

/// The date an RFC 3339 bucket start falls on, UTC, with the hour added for
/// an hour bucket: a day or a week bucket has no room for the time of a
/// moment that is always midnight.
fn bucket_date(bucket: BucketDto, rfc3339: &str) -> String {
    match bucket {
        BucketDto::Hour => rfc3339.get(0..16).unwrap_or(rfc3339).replacen('T', " ", 1),
        BucketDto::Day | BucketDto::Week => {
            rfc3339.split('T').next().unwrap_or(rfc3339).to_string()
        }
    }
}

/// A share as a table cell, to one decimal place: `89.1%`.
fn percent(share: f64) -> String {
    format!("{:.1}%", (share * 1000.0).round() / 10.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    use ariadne_api::stats::{PerFinishedTaskDto, SpendTotalsDto};

    fn stats() -> SpendStatsDto {
        SpendStatsDto {
            totals: SpendTotalsDto {
                sessions: 3,
                input_tokens: 1_000_000,
                cached_input_tokens: 800_000,
                output_tokens: 100_000,
                cached_share: 0.8,
            },
            per_finished_task: PerFinishedTaskDto {
                tasks: 2,
                input_tokens: 500_000.0,
                output_tokens: 50_000.0,
            },
            bucket: BucketDto::Day,
            buckets: vec![SpendBucketDto {
                start: "2026-10-01T00:00:00Z".into(),
                input_tokens: 1_000_000,
                cached_input_tokens: 800_000,
                output_tokens: 100_000,
            }],
            by_model: vec![ModelSpendDto {
                model: "stub:heavy".into(),
                input_tokens: 1_000_000,
                cached_input_tokens: 800_000,
                output_tokens: 100_000,
                share: 1.0,
            }],
        }
    }

    /// The totals and the per-task figures render as the same aligned
    /// key-value block every family uses, not `label: value` lines — the
    /// expected block is built from hand-typed pairs, independent of
    /// [`totals_kv`], so a wrong label or value shows up as a mismatch.
    #[test]
    fn the_totals_render_as_the_shared_kv_block() {
        let expected = kv_block(
            &[
                ("sessions", "3".to_string()),
                ("input tokens", "1M".to_string()),
                ("cached input tokens", "800k".to_string()),
                ("output tokens", "100k".to_string()),
                ("cached share", "80.0%".to_string()),
                ("finished tasks", "2".to_string()),
                ("input tokens per task", "500k".to_string()),
                ("output tokens per task", "50k".to_string()),
            ],
            view(),
        );
        assert_eq!(kv_block(&totals_kv(&stats()), view()), expected);
        assert!(render(&stats()).unwrap().starts_with(&expected));
        assert!(!render(&stats()).unwrap().contains("sessions: 3"));
    }

    /// The rendered text carries a row for the model and a row for the
    /// bucket, the model's usage as the one cell every table prints, and
    /// one blank line between the totals block and each table.
    #[test]
    fn the_table_prints_a_model_row_and_a_bucket_row() {
        let text = render(&stats()).unwrap();
        assert!(text.contains("stub:heavy"), "{text}");
        assert!(text.contains("↑1M 80.0% ↓100k"), "{text}");
        assert!(text.contains("2026-10-01"), "{text}");
        assert_eq!(
            text.matches("\n\n").count(),
            2,
            "expected two blank lines between three blocks: {text}"
        );
    }

    /// Nothing ended in the span is the one thing that makes the family
    /// empty, whatever the finished-task average says on its own.
    #[test]
    fn a_family_with_no_ended_session_is_empty() {
        assert!(is_empty(&SpendStatsDto::default()));
        assert!(!is_empty(&stats()));
    }

    /// An hour bucket's date carries its time of day too, where a day or a
    /// week bucket carries the bare date.
    #[test]
    fn bucket_date_carries_the_time_of_day_for_an_hour_bucket() {
        assert_eq!(
            bucket_date(BucketDto::Hour, "2026-10-01T14:00:00Z"),
            "2026-10-01 14:00"
        );
        assert_eq!(
            bucket_date(BucketDto::Day, "2026-10-01T14:00:00Z"),
            "2026-10-01"
        );
        assert_eq!(
            bucket_date(BucketDto::Week, "2026-10-01T14:00:00Z"),
            "2026-10-01"
        );
    }
}
