//! `ariadne stats work`: what got done.

use anyhow::Result;

use ariadne_api::stats::{StatsQuery, WorkBucketDto, WorkStatsDto, WorkTotalsDto};
use ariadne_client::Client;

use crate::commands::query_path;
use crate::output::{
    Column, Format, UNCAPPED, col, duration, kv_block, note, print, render_table, style, view,
};

/// The empty sentence: nothing moved in the span the filter kept.
const EMPTY: &str = "No work was done in that span.";

/// Columns of the buckets table.
const BUCKETS: &[Column] = &[
    col("from", UNCAPPED),
    col("finished", UNCAPPED),
    col("failed", UNCAPPED),
    col("cancelled", UNCAPPED),
    col("goals", UNCAPPED),
    col("landed", UNCAPPED),
];

pub(super) async fn run(client: &Client, query: &StatsQuery, format: Format) -> Result<()> {
    let stats: WorkStatsDto = client
        .get_json(&query_path("/v1/stats/work", query)?)
        .await?;
    print(format, &stats, || render(&stats))
}

fn render(stats: &WorkStatsDto) {
    match rendered(stats) {
        Some(text) => println!("{text}"),
        None => note(&style::paint(view().color, style::META, EMPTY)),
    }
}

/// The text a populated family prints for a person: the totals as
/// `label: value` lines, then the buckets table. `None` where there is
/// nothing to show.
fn rendered(stats: &WorkStatsDto) -> Option<String> {
    if stats.buckets.is_empty() {
        return None;
    }
    let totals = kv_block(&totals_kv(&stats.totals), view());
    let rows: Vec<Vec<String>> = stats.buckets.iter().map(bucket_row).collect();
    let table = render_table(BUCKETS, &rows, view()).unwrap_or_default();
    Some(format!("{totals}\n\n{table}"))
}

/// The totals as `label: value` lines, in the order [`WorkTotalsDto`] holds
/// them.
fn totals_kv(totals: &WorkTotalsDto) -> Vec<(&'static str, String)> {
    vec![
        ("goals completed", totals.goals_completed.to_string()),
        ("goals cancelled", totals.goals_cancelled.to_string()),
        (
            "median goal lead time",
            duration(totals.median_goal_lead_time_secs.max(0.0).round() as u64),
        ),
        ("tasks finished", totals.tasks_finished.to_string()),
        ("tasks failed", totals.tasks_failed.to_string()),
        ("tasks cancelled", totals.tasks_cancelled.to_string()),
        ("finish rate", percent(totals.finish_rate)),
        ("landed", totals.landed.to_string()),
    ]
}

/// A fraction as a table/block reads it: `73.3%`.
fn percent(fraction: f64) -> String {
    format!("{:.1}%", fraction * 100.0)
}

/// One row of the buckets table, in [`BUCKETS`]'s column order.
fn bucket_row(bucket: &WorkBucketDto) -> Vec<String> {
    vec![
        bucket_date(&bucket.start),
        bucket.tasks_finished.to_string(),
        bucket.tasks_failed.to_string(),
        bucket.tasks_cancelled.to_string(),
        bucket.goals_completed.to_string(),
        bucket.landed.to_string(),
    ]
}

/// A bucket's RFC 3339 start as a table reads a date: local, day precision.
fn bucket_date(start: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(start)
        .map(|t| {
            t.with_timezone(&chrono::Local)
                .format("%Y-%m-%d")
                .to_string()
        })
        .unwrap_or_else(|_| start.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    use axum::Json;
    use axum::routing::get;

    fn totals() -> WorkTotalsDto {
        WorkTotalsDto {
            goals_completed: 2,
            goals_cancelled: 1,
            median_goal_lead_time_secs: 3_600.0,
            tasks_finished: 7,
            tasks_failed: 2,
            tasks_cancelled: 1,
            finish_rate: 0.7,
            landed: 5,
        }
    }

    /// A populated DTO: one bucket, the totals above.
    fn populated() -> WorkStatsDto {
        WorkStatsDto {
            totals: totals(),
            bucket: "day".into(),
            buckets: vec![WorkBucketDto {
                start: "2026-09-28T00:00:00Z".into(),
                tasks_finished: 3,
                tasks_failed: 1,
                tasks_cancelled: 0,
                goals_completed: 1,
                landed: 2,
            }],
        }
    }

    /// The totals print as one `label: value` line each, in the DTO's own
    /// field order.
    #[test]
    fn the_totals_print_as_label_value_lines_in_field_order() {
        let pairs = totals_kv(&totals());
        assert_eq!(
            pairs.iter().map(|(k, _)| *k).collect::<Vec<_>>(),
            [
                "goals completed",
                "goals cancelled",
                "median goal lead time",
                "tasks finished",
                "tasks failed",
                "tasks cancelled",
                "finish rate",
                "landed",
            ]
        );
        assert_eq!(pairs[0].1, "2");
        assert_eq!(pairs[1].1, "1");
        assert_eq!(pairs[2].1, "1h 0m");
        assert_eq!(pairs[3].1, "7");
        assert_eq!(pairs[6].1, "70.0%");
        assert_eq!(pairs[7].1, "5");
    }

    /// A bucket's row carries every count in the table's column order, and
    /// its start as a bare date.
    #[test]
    fn a_bucket_row_carries_every_count_in_column_order() {
        let bucket = WorkBucketDto {
            start: "2026-09-28T00:00:00Z".into(),
            tasks_finished: 3,
            tasks_failed: 1,
            tasks_cancelled: 0,
            goals_completed: 1,
            landed: 2,
        };
        let row = bucket_row(&bucket);
        assert_eq!(row[1..], ["3", "1", "0", "1", "2"]);
        assert!(
            row[0].starts_with("2026-09-2") || row[0].starts_with("2026-09-28"),
            "{row:?}"
        );
    }

    /// A span with no bucket at all renders nothing: `render` prints the one
    /// empty sentence instead, never a table with no rows.
    #[test]
    fn an_empty_span_renders_nothing() {
        assert_eq!(rendered(&WorkStatsDto::default()), None);
    }

    /// A populated DTO renders as the totals, blank-line separated from the
    /// buckets table — the exact text `render` prints for a person, built
    /// independently of [`totals_kv`] and [`bucket_row`] so a wrong label or
    /// a dropped column shows up as a mismatch here.
    #[test]
    fn rendered_prints_the_totals_then_the_buckets_table() {
        let text = rendered(&populated()).expect("a populated span renders something");
        let expected_totals = kv_block(
            &[
                ("goals completed", "2".to_string()),
                ("goals cancelled", "1".to_string()),
                ("median goal lead time", "1h 0m".to_string()),
                ("tasks finished", "7".to_string()),
                ("tasks failed", "2".to_string()),
                ("tasks cancelled", "1".to_string()),
                ("finish rate", "70.0%".to_string()),
                ("landed", "5".to_string()),
            ],
            view(),
        );
        let expected_table = render_table(
            BUCKETS,
            &[vec![
                bucket_date("2026-09-28T00:00:00Z"),
                "3".into(),
                "1".into(),
                "0".into(),
                "1".into(),
                "2".into(),
            ]],
            view(),
        )
        .unwrap();
        assert_eq!(text, format!("{expected_totals}\n\n{expected_table}"));
    }

    /// `WorkStatsDto` serializes with every total and bucket field under its
    /// own name: what `--format json` prints is this value, verbatim.
    #[test]
    fn the_dto_serializes_with_every_total_and_bucket_field() {
        let value: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&populated()).expect("a DTO serializes"))
                .expect("the serialized DTO parses back");
        assert_eq!(value["bucket"], "day");
        assert_eq!(value["totals"]["tasks_finished"], 7);
        assert_eq!(value["totals"]["finish_rate"], 0.7);
        assert_eq!(value["totals"]["landed"], 5);
        assert_eq!(value["buckets"][0]["start"], "2026-09-28T00:00:00Z");
        assert_eq!(value["buckets"][0]["tasks_finished"], 3);
        assert_eq!(value["buckets"][0]["landed"], 2);
    }

    /// A daemon serving a populated DTO: `run` fetches it and prints it in
    /// either format with no error, the full fetch-decode-render pipeline
    /// `rendered` and the JSON test above cannot reach on their own.
    async fn serve_populated() -> Client {
        async fn handler() -> Json<WorkStatsDto> {
            Json(populated())
        }
        let app = axum::Router::new().route("/v1/stats/work", get(handler));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Client::tcp(format!("http://{address}"))
    }

    #[tokio::test]
    async fn run_fetches_and_prints_a_populated_dto_in_both_formats() {
        let client = serve_populated().await;
        let query = StatsQuery {
            since: None,
            repo: None,
        };
        run(&client, &query, Format::Json).await.unwrap();
        run(&client, &query, Format::Table).await.unwrap();
    }
}
