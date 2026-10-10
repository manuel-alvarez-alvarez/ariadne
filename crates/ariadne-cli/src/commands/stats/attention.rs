//! `ariadne stats attention`: how much the work needed you.

use anyhow::Result;

use ariadne_api::stats::{AttentionStatsDto, StatsQuery};
use ariadne_client::Client;

use crate::commands::query_path;
use crate::output::{Column, Format, View, col, duration, kv_block, print, render_table, view};

const DECIDER_COLUMNS: &[Column] = &[
    col("DECIDED BY", 20),
    col("TOTAL", 8),
    col("ALLOW", 8),
    col("DENY", 8),
    col("CANCELLED", 10),
    col("MEAN WAIT", 12),
];

const FLAG_COLUMNS: &[Column] = &[col("REASON", 24), col("RAISED", 8), col("MEAN WAIT", 12)];

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
    let stats: AttentionStatsDto = client
        .get_json(&query_path("/v1/stats/attention", query)?)
        .await?;
    let text = match format {
        Format::Table => Some(rendered(&stats, view)?),
        Format::Json => None,
    };
    print(format, &stats, || {
        if let Some(text) = text {
            println!("{text}");
        }
    })
}

/// The totals as the aligned key-value block, then the deciders table and
/// the flags table, one blank line between each. A `--columns` neither
/// table has is an error here, not a table quietly dropped.
fn rendered(stats: &AttentionStatsDto, view: &View) -> Result<String> {
    let totals = kv_block(&attention_kv(stats), view);
    let deciders = render_table(DECIDER_COLUMNS, &decider_rows(stats), view)?;
    let flags = render_table(FLAG_COLUMNS, &flag_rows(stats), view)?;
    Ok(format!("{totals}\n\n{deciders}\n\n{flags}"))
}

fn attention_kv(stats: &AttentionStatsDto) -> Vec<(&'static str, String)> {
    vec![
        ("permissions", stats.permissions.total.to_string()),
        (
            "person share",
            format_percent(stats.permissions.person_share),
        ),
        ("sessions failed", stats.sessions_failed.to_string()),
        ("sessions stalled", stats.sessions_stalled.to_string()),
        ("exhaustions", stats.exhaustions.to_string()),
        ("interventions", stats.interventions.total.to_string()),
        (
            "person time",
            duration(stats.interventions.person_secs as u64),
        ),
    ]
}

fn format_percent(value: f64) -> String {
    format!("{:.0}%", value * 100.0)
}
fn format_ms(value: f64) -> String {
    format!("{value:.0}ms")
}
fn format_secs(value: f64) -> String {
    format!("{value:.0}s")
}

fn decider_rows(stats: &AttentionStatsDto) -> Vec<Vec<String>> {
    stats
        .permissions
        .by_decider
        .iter()
        .map(|row| {
            vec![
                row.decided_by.clone(),
                row.total.to_string(),
                row.allowed.to_string(),
                row.denied.to_string(),
                row.cancelled.to_string(),
                format_ms(row.mean_wait_ms),
            ]
        })
        .collect()
}

fn flag_rows(stats: &AttentionStatsDto) -> Vec<Vec<String>> {
    stats
        .flags
        .iter()
        .map(|row| {
            vec![
                row.reason.clone(),
                row.raised.to_string(),
                format_secs(row.mean_wait_secs),
            ]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ariadne_api::stats::{
        AttentionFlagDto, AttentionInterventionsDto, PermissionDeciderDto, PermissionStatsDto,
    };

    #[test]
    fn attention_table_has_a_decider_and_a_reason_row() {
        let stats = AttentionStatsDto {
            permissions: PermissionStatsDto {
                total: 1,
                person_share: 1.0,
                by_decider: vec![PermissionDeciderDto {
                    decided_by: "console".into(),
                    total: 1,
                    allowed: 1,
                    denied: 0,
                    cancelled: 0,
                    mean_wait_ms: 25.0,
                }],
            },
            flags: vec![AttentionFlagDto {
                reason: "waiting_input".into(),
                raised: 2,
                mean_wait_secs: 5.0,
            }],
            ..Default::default()
        };
        assert_eq!(
            decider_rows(&stats),
            [["console", "1", "1", "0", "0", "25ms"]]
        );
        assert_eq!(flag_rows(&stats), [["waiting_input", "2", "5s"]]);
        assert_eq!(
            serde_json::to_value(&stats).unwrap()["permissions"]["total"],
            1
        );
    }

    #[test]
    fn the_kv_lines_carry_the_interventions_total_and_the_person_time() {
        let stats = AttentionStatsDto {
            interventions: AttentionInterventionsDto {
                permissions: 2,
                questions: 1,
                stalls: 1,
                total: 4,
                person_secs: 90.0,
            },
            ..Default::default()
        };
        let kv = attention_kv(&stats);
        assert!(kv.contains(&("interventions", "4".to_string())));
        assert!(kv.contains(&("person time", "1m 30s".to_string())));
    }

    /// The totals block, the deciders table and the flags table are one
    /// blank line apart, the expected text built independently of
    /// [`attention_kv`], [`decider_rows`] and [`flag_rows`] so a dropped
    /// separator shows up as a mismatch.
    #[test]
    fn rendered_puts_one_blank_line_between_the_totals_and_each_table() {
        let stats = AttentionStatsDto {
            permissions: PermissionStatsDto {
                total: 1,
                person_share: 1.0,
                by_decider: vec![PermissionDeciderDto {
                    decided_by: "console".into(),
                    total: 1,
                    allowed: 1,
                    denied: 0,
                    cancelled: 0,
                    mean_wait_ms: 25.0,
                }],
            },
            flags: vec![AttentionFlagDto {
                reason: "waiting_input".into(),
                raised: 2,
                mean_wait_secs: 5.0,
            }],
            sessions_failed: 1,
            sessions_stalled: 0,
            exhaustions: 0,
            interventions: AttentionInterventionsDto {
                permissions: 1,
                questions: 0,
                stalls: 0,
                total: 1,
                person_secs: 30.0,
            },
        };
        let view = View::plain();
        let expected_totals = kv_block(
            &[
                ("permissions", "1".to_string()),
                ("person share", "100%".to_string()),
                ("sessions failed", "1".to_string()),
                ("sessions stalled", "0".to_string()),
                ("exhaustions", "0".to_string()),
                ("interventions", "1".to_string()),
                ("person time", "30s".to_string()),
            ],
            &view,
        );
        let expected_deciders = render_table(
            DECIDER_COLUMNS,
            &[vec![
                "console".to_string(),
                "1".to_string(),
                "1".to_string(),
                "0".to_string(),
                "0".to_string(),
                "25ms".to_string(),
            ]],
            &view,
        )
        .unwrap();
        let expected_flags = render_table(
            FLAG_COLUMNS,
            &[vec![
                "waiting_input".to_string(),
                "2".to_string(),
                "5s".to_string(),
            ]],
            &view,
        )
        .unwrap();
        assert_eq!(
            rendered(&stats, &view).unwrap(),
            format!("{expected_totals}\n\n{expected_deciders}\n\n{expected_flags}")
        );
    }

    fn populated() -> AttentionStatsDto {
        AttentionStatsDto {
            permissions: PermissionStatsDto {
                total: 1,
                person_share: 1.0,
                by_decider: vec![PermissionDeciderDto {
                    decided_by: "console".into(),
                    total: 1,
                    allowed: 1,
                    denied: 0,
                    cancelled: 0,
                    mean_wait_ms: 25.0,
                }],
            },
            flags: vec![AttentionFlagDto {
                reason: "waiting_input".into(),
                raised: 2,
                mean_wait_secs: 5.0,
            }],
            ..Default::default()
        }
    }

    async fn serve_populated() -> Client {
        async fn handler() -> axum::Json<AttentionStatsDto> {
            axum::Json(populated())
        }
        let app = axum::Router::new().route("/v1/stats/attention", axum::routing::get(handler));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Client::tcp(format!("http://{address}"))
    }

    /// `--columns` naming a column neither table has is a refusal that
    /// reaches `run`'s caller, not a table quietly left out: with
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
