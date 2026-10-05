//! `ariadne stats attention`: how much the work needed you.

use anyhow::Result;

use ariadne_api::stats::{AttentionStatsDto, StatsQuery};
use ariadne_client::Client;

use crate::commands::query_path;
use crate::output::{Format, col, duration, print, print_kv, print_table};

pub(super) async fn run(client: &Client, query: &StatsQuery, format: Format) -> Result<()> {
    let stats: AttentionStatsDto = client
        .get_json(&query_path("/v1/stats/attention", query)?)
        .await?;
    print(format, &stats, || print_attention(&stats))
}

fn print_attention(stats: &AttentionStatsDto) {
    print_kv(&attention_kv(stats));
    let _ = print_table(
        &[
            col("DECIDED BY", 20),
            col("TOTAL", 8),
            col("ALLOW", 8),
            col("DENY", 8),
            col("CANCELLED", 10),
            col("MEAN WAIT", 12),
        ],
        &decider_rows(stats),
    );
    let _ = print_table(
        &[col("REASON", 24), col("RAISED", 8), col("MEAN WAIT", 12)],
        &flag_rows(stats),
    );
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
}
