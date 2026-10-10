//! `ariadne stats time`: how long the work takes.

use anyhow::Result;

use ariadne_api::stats::{StatsQuery, TimeStatsDto};
use ariadne_client::Client;

use crate::commands::query_path;
use crate::output::{
    Format, View, col, duration, kv_block, note, print, render_table, style, view,
};

const IN_STATUS: &[crate::output::Column] = &[
    col("STATUS", 20),
    col("TOTAL", 12),
    col("MEDIAN", 12),
    col("SHARE", 8),
];

pub(super) async fn run(client: &Client, query: &StatsQuery, format: Format) -> Result<()> {
    run_against(client, query, format, view()).await
}

/// `run`, against a given [`View`] rather than the one this process
/// settled on: what a test drives, with a `--columns` of its own, so that
/// proving an unknown one refuses needs no shared process-wide state.
async fn run_against(
    client: &Client,
    query: &StatsQuery,
    format: Format,
    view: &View,
) -> Result<()> {
    let stats: TimeStatsDto = client
        .get_json(&query_path("/v1/stats/time", query)?)
        .await?;
    let text = match format {
        Format::Table => rendered(&stats, view)?,
        Format::Json => None,
    };
    print(format, &stats, || match text {
        Some(text) => println!("{text}"),
        None => note(&style::paint(
            view.color,
            style::META,
            "No work was timed in that span.",
        )),
    })
}

/// The totals as the aligned key-value block, then the per-status table,
/// one blank line between them. `Ok(None)` where nothing was timed. A
/// `--columns` the table does not have is an error here, not a table
/// quietly dropped.
fn rendered(stats: &TimeStatsDto, view: &View) -> Result<Option<String>> {
    if stats.tasks == 0 {
        return Ok(None);
    }
    let totals = kv_block(&totals_kv(stats), view);
    let table = render_table(IN_STATUS, &in_status_rows(stats), view)?;
    Ok(Some(format!("{totals}\n\n{table}")))
}

fn totals_kv(stats: &TimeStatsDto) -> Vec<(&'static str, String)> {
    vec![
        (
            "Median lead time",
            duration(stats.lead_time.median_secs as u64),
        ),
        ("P90 lead time", duration(stats.lead_time.p90_secs as u64)),
        (
            "Time waiting on a person",
            duration(stats.waiting_on_person.total_secs as u64),
        ),
        (
            "Prompts waited on",
            stats.waiting_on_person.prompts.to_string(),
        ),
    ]
}

fn in_status_rows(stats: &TimeStatsDto) -> Vec<Vec<String>> {
    stats
        .in_status
        .iter()
        .map(|row| {
            vec![
                row.status.clone(),
                duration(row.total_secs as u64),
                duration(row.median_secs as u64),
                format!("{:.0}%", row.share * 100.0),
            ]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use ariadne_api::stats::{LeadTimeDto, PersonWaitDto, StatusTimeDto};
    use axum::Json;
    use axum::routing::get;

    use super::*;

    #[test]
    fn the_table_has_a_row_per_status_with_duration_and_share() {
        let stats = TimeStatsDto {
            tasks: 1,
            lead_time: LeadTimeDto::default(),
            in_status: vec![StatusTimeDto {
                status: "in_progress".into(),
                total_secs: 90.0,
                median_secs: 30.0,
                share: 0.75,
            }],
            waiting_on_person: PersonWaitDto::default(),
        };
        assert_eq!(
            in_status_rows(&stats),
            vec![vec![
                String::from("in_progress"),
                String::from("1m 30s"),
                String::from("30s"),
                String::from("75%")
            ]]
        );
    }

    /// The totals block and the per-status table are one blank line apart,
    /// the expected block and table built independently of [`totals_kv`]
    /// and [`in_status_rows`] so a dropped separator shows up as a
    /// mismatch.
    #[test]
    fn rendered_puts_one_blank_line_between_the_totals_and_the_table() {
        let stats = TimeStatsDto {
            tasks: 1,
            lead_time: LeadTimeDto {
                median_secs: 30.0,
                p90_secs: 90.0,
                mean_secs: 45.0,
            },
            in_status: vec![StatusTimeDto {
                status: "in_progress".into(),
                total_secs: 90.0,
                median_secs: 30.0,
                share: 0.75,
            }],
            waiting_on_person: PersonWaitDto {
                prompts: 2,
                total_secs: 60.0,
                median_secs: 30.0,
            },
        };
        let view = View::plain();
        let expected_totals = kv_block(
            &[
                ("Median lead time", "30s".to_string()),
                ("P90 lead time", "1m 30s".to_string()),
                ("Time waiting on a person", "1m 0s".to_string()),
                ("Prompts waited on", "2".to_string()),
            ],
            &view,
        );
        let expected_table = render_table(
            IN_STATUS,
            &[vec![
                "in_progress".to_string(),
                "1m 30s".to_string(),
                "30s".to_string(),
                "75%".to_string(),
            ]],
            &view,
        )
        .unwrap();
        assert_eq!(
            rendered(&stats, &view).unwrap().unwrap(),
            format!("{expected_totals}\n\n{expected_table}")
        );
    }

    fn populated() -> TimeStatsDto {
        TimeStatsDto {
            tasks: 1,
            lead_time: LeadTimeDto::default(),
            in_status: vec![StatusTimeDto {
                status: "in_progress".into(),
                total_secs: 90.0,
                median_secs: 30.0,
                share: 0.75,
            }],
            waiting_on_person: PersonWaitDto::default(),
        }
    }

    async fn serve_populated() -> Client {
        async fn handler() -> Json<TimeStatsDto> {
            Json(populated())
        }
        let app = axum::Router::new().route("/v1/stats/time", get(handler));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Client::tcp(format!("http://{address}"))
    }

    /// `--columns` naming a column `IN_STATUS` does not have is a refusal
    /// that reaches `run`'s caller, not a table quietly left out: with
    /// populated statistics, `run` must not swallow `render_table`'s error
    /// behind an empty default and return success anyway. The `--columns`
    /// is a `View` built for this test alone, never the process-wide one,
    /// so the test needs no ordering against any other and proves nothing
    /// about it.
    #[tokio::test]
    async fn an_unknown_column_refuses_rather_than_hiding_the_table() {
        let client = serve_populated().await;
        let query = StatsQuery {
            since: None,
            repo: None,
        };
        let view = View {
            columns: vec!["nonexistent".into()],
            ..View::plain()
        };
        let err = run_against(&client, &query, Format::Table, &view)
            .await
            .expect_err("an unknown column refuses rather than printing");
        assert!(err.to_string().contains("nonexistent"), "{err}");
    }
}
