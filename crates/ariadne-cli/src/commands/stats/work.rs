//! `ariadne stats work`: what got done.

use anyhow::Result;

use ariadne_api::stats::{StatsQuery, WorkStatsDto};
use ariadne_client::Client;

use super::print_family;
use crate::commands::query_path;
use crate::output::Format;

pub(super) async fn run(client: &Client, query: &StatsQuery, format: Format) -> Result<()> {
    let stats: WorkStatsDto = client
        .get_json(&query_path("/v1/stats/work", query)?)
        .await?;
    print_family(format, &stats, "No work was done in that span.")
}
