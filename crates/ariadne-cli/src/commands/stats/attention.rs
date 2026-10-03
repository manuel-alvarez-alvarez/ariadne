//! `ariadne stats attention`: how much the work needed you.

use anyhow::Result;

use ariadne_api::stats::{AttentionStatsDto, StatsQuery};
use ariadne_client::Client;

use super::print_family;
use crate::commands::query_path;
use crate::output::Format;

pub(super) async fn run(client: &Client, query: &StatsQuery, format: Format) -> Result<()> {
    let stats: AttentionStatsDto = client
        .get_json(&query_path("/v1/stats/attention", query)?)
        .await?;
    print_family(format, &stats, "Nothing needed you in that span.")
}
