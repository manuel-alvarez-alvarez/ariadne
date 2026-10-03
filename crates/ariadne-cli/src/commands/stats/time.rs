//! `ariadne stats time`: how long the work takes.

use anyhow::Result;

use ariadne_api::stats::{StatsQuery, TimeStatsDto};
use ariadne_client::Client;

use super::print_family;
use crate::commands::query_path;
use crate::output::Format;

pub(super) async fn run(client: &Client, query: &StatsQuery, format: Format) -> Result<()> {
    let stats: TimeStatsDto = client
        .get_json(&query_path("/v1/stats/time", query)?)
        .await?;
    print_family(format, &stats, "No work was timed in that span.")
}
