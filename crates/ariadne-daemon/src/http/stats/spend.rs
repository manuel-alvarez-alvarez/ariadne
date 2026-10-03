//! `GET /v1/stats/spend`: What did it spend?

use axum::extract::{Query, State};
use chrono::Utc;
use utoipa::OpenApi;

use ariadne_api::stats::{SpendStatsDto, StatsQuery};
use ariadne_store::SpendStats;

use super::stats_filter;
use crate::http::AppState;
use crate::http::error::{ApiResult, Json};

/// This family's part of the API document.
#[derive(OpenApi)]
#[openapi(paths(spend), components(schemas(SpendStatsDto)))]
pub(super) struct SpendApi;

#[utoipa::path(get, path = "/v1/stats/spend", tag = "stats",
    params(StatsQuery),
    responses((status = 200, body = SpendStatsDto),
              (status = 400, description = "`since` is neither a moment nor a span")))]
pub(super) async fn spend(
    State(state): State<AppState>,
    Query(query): Query<StatsQuery>,
) -> ApiResult<Json<SpendStatsDto>> {
    let filter = stats_filter(&query, Utc::now())?;
    Ok(Json(dto(state.store.spend_stats(&filter).await?)))
}

fn dto(_stats: SpendStats) -> SpendStatsDto {
    SpendStatsDto {}
}
