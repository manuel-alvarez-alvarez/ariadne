//! `GET /v1/stats/tools`: What do the agents do?

use axum::extract::{Query, State};
use chrono::Utc;
use utoipa::OpenApi;

use ariadne_api::stats::{StatsQuery, ToolStatsDto};
use ariadne_store::ToolStats;

use super::stats_filter;
use crate::http::AppState;
use crate::http::error::{ApiResult, Json};

/// This family's part of the API document.
#[derive(OpenApi)]
#[openapi(paths(tools), components(schemas(ToolStatsDto)))]
pub(super) struct ToolsApi;

#[utoipa::path(get, path = "/v1/stats/tools", tag = "stats",
    params(StatsQuery),
    responses((status = 200, body = ToolStatsDto),
              (status = 400, description = "`since` is neither a moment nor a span")))]
pub(super) async fn tools(
    State(state): State<AppState>,
    Query(query): Query<StatsQuery>,
) -> ApiResult<Json<ToolStatsDto>> {
    let filter = stats_filter(&query, Utc::now())?;
    Ok(Json(dto(state.store.tools_stats(&filter).await?)))
}

fn dto(_stats: ToolStats) -> ToolStatsDto {
    ToolStatsDto {}
}
