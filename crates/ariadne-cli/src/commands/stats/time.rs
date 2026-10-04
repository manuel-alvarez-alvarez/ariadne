//! `ariadne stats time`: how long the work takes.

use anyhow::Result;

use ariadne_api::stats::{StatsQuery, TimeStatsDto};
use ariadne_client::Client;

use crate::commands::query_path;
use crate::output::{Format, col, duration, note, print, print_kv, print_table, style, view};

const IN_STATUS: &[crate::output::Column] = &[
    col("STATUS", 20),
    col("TOTAL", 12),
    col("MEDIAN", 12),
    col("SHARE", 8),
];

pub(super) async fn run(client: &Client, query: &StatsQuery, format: Format) -> Result<()> {
    let stats: TimeStatsDto = client
        .get_json(&query_path("/v1/stats/time", query)?)
        .await?;
    print(format, &stats, || {
        if stats.tasks == 0 {
            note(&style::paint(
                view().color,
                style::META,
                "No work was timed in that span.",
            ));
            return;
        }
        print_kv(&[
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
        ]);
        let rows = in_status_rows(&stats);
        print_table(IN_STATUS, &rows).expect("time stats table renders");
    })
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
}
