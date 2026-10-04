//! `ariadne stats tools`: what the agents do.

use anyhow::Result;

use ariadne_api::stats::{StatsQuery, ToolKindStatDto, ToolStatDto, ToolStatsDto, ToolsStatsQuery};
use ariadne_client::Client;

use crate::commands::query_path;
use crate::output::{
    Column, Format, UNCAPPED, col, note, print_json, print_kv, print_table, style, view,
};

const BY_KIND: &[Column] = &[
    col("kind", UNCAPPED).title(),
    col("calls", UNCAPPED),
    col("errors", UNCAPPED),
    col("median", UNCAPPED),
    col("p90", UNCAPPED),
];

const TOP: &[Column] = &[
    col("tool", UNCAPPED).title(),
    col("kind", UNCAPPED),
    col("calls", UNCAPPED),
    col("errors", UNCAPPED),
    col("median", UNCAPPED),
    col("p90", UNCAPPED),
];

pub(super) async fn run(
    client: &Client,
    query: &StatsQuery,
    limit: Option<u32>,
    format: Format,
) -> Result<()> {
    let query = ToolsStatsQuery {
        since: query.since.clone(),
        repo: query.repo.clone(),
        limit,
    };
    let stats: ToolStatsDto = client
        .get_json(&query_path("/v1/stats/tools", &query)?)
        .await?;
    if format == Format::Json {
        return print_json(&stats);
    }
    if stats.calls == 0 {
        note(&style::paint(
            view().color,
            style::META,
            "No tool ran in that span.",
        ));
        return Ok(());
    }
    print_kv(&[
        ("calls", stats.calls.to_string()),
        ("errors", stats.errors.to_string()),
        ("tools", stats.tools.to_string()),
    ]);
    println!();
    print_table(
        BY_KIND,
        &stats.by_kind.iter().map(by_kind_row).collect::<Vec<_>>(),
    )?;
    println!();
    let mut rows: Vec<Vec<String>> = stats.top.iter().map(top_row).collect();
    if stats.other.tools > 0 {
        rows.push(vec![
            "other".to_string(),
            "-".to_string(),
            stats.other.calls.to_string(),
            stats.other.errors.to_string(),
            "-".to_string(),
            "-".to_string(),
        ]);
    }
    print_table(TOP, &rows)
}

fn milliseconds(value: f64) -> String {
    format!("{}ms", value.round())
}

fn by_kind_row(row: &ToolKindStatDto) -> Vec<String> {
    vec![
        row.kind.clone(),
        row.calls.to_string(),
        row.errors.to_string(),
        milliseconds(row.median_duration_ms),
        milliseconds(row.p90_duration_ms),
    ]
}

fn top_row(row: &ToolStatDto) -> Vec<String> {
    vec![
        row.tool_name.clone(),
        row.kind.clone(),
        row.calls.to_string(),
        row.errors.to_string(),
        milliseconds(row.median_duration_ms),
        milliseconds(row.p90_duration_ms),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    use ariadne_api::stats::OtherToolsDto;

    use axum::Router;
    use axum::routing::get;

    use crate::output::{View, render_table};

    fn stats() -> ToolStatsDto {
        ToolStatsDto {
            calls: 5,
            errors: 1,
            tools: 3,
            by_kind: vec![ToolKindStatDto {
                kind: "execute".into(),
                calls: 4,
                errors: 1,
                median_duration_ms: 25.0,
                p90_duration_ms: 100.0,
            }],
            top: vec![ToolStatDto {
                tool_name: "Bash".into(),
                kind: "execute".into(),
                calls: 4,
                errors: 1,
                median_duration_ms: 25.0,
                p90_duration_ms: 100.0,
            }],
            other: OtherToolsDto {
                tools: 1,
                calls: 1,
                errors: 0,
            },
        }
    }

    async fn serve() -> Client {
        async fn handler() -> axum::Json<ToolStatsDto> {
            axum::Json(stats())
        }
        let app = Router::new().route("/v1/stats/tools", get(handler));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Client::tcp(format!("http://{address}"))
    }

    /// `--format json` reads the DTO off the route whole, and the table
    /// prints a row per kind and the top tools with an `other` row.
    #[tokio::test]
    async fn json_prints_the_dto_and_the_table_groups_its_rows() {
        let client = serve().await;
        run(
            &client,
            &StatsQuery {
                since: None,
                repo: None,
            },
            None,
            Format::Json,
        )
        .await
        .unwrap();

        let by_kind =
            render_table(BY_KIND, &[by_kind_row(&stats().by_kind[0])], &View::plain()).unwrap();
        assert!(
            by_kind.contains("KIND") && by_kind.contains("execute"),
            "{by_kind}"
        );

        let mut rows: Vec<Vec<String>> = stats().top.iter().map(top_row).collect();
        rows.push(vec![
            "other".into(),
            "-".into(),
            "1".into(),
            "0".into(),
            "-".into(),
            "-".into(),
        ]);
        let top = render_table(TOP, &rows, &View::plain()).unwrap();
        assert!(
            top.contains("TOOL") && top.contains("Bash") && top.contains("other"),
            "{top}"
        );
    }
}
