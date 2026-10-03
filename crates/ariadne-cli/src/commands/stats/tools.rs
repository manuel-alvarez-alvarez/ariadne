//! `ariadne stats tools`: what the agents do.

use anyhow::Result;

use ariadne_api::stats::{StatsQuery, ToolStatsDto};
use ariadne_client::Client;

use super::print_family;
use crate::commands::query_path;
use crate::output::Format;

pub(super) async fn run(client: &Client, query: &StatsQuery, format: Format) -> Result<()> {
    let stats: ToolStatsDto = client
        .get_json(&query_path("/v1/stats/tools", query)?)
        .await?;
    print_family(format, &stats, "No tool ran in that span.")
}
