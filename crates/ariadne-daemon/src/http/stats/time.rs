//! `GET /v1/stats/time`: How long does it take?

use axum::extract::{Query, State};
use chrono::Utc;
use utoipa::OpenApi;

use ariadne_api::stats::{StatsQuery, TimeStatsDto};
use ariadne_store::TimeStats;

use super::stats_filter;
use crate::http::AppState;
use crate::http::error::{ApiResult, Json};

/// This family's part of the API document.
#[derive(OpenApi)]
#[openapi(paths(time), components(schemas(TimeStatsDto)))]
pub(super) struct TimeApi;

#[utoipa::path(get, path = "/v1/stats/time", tag = "stats",
    params(StatsQuery),
    responses((status = 200, body = TimeStatsDto),
              (status = 400, description = "`since` is neither a moment nor a span")))]
pub(super) async fn time(
    State(state): State<AppState>,
    Query(query): Query<StatsQuery>,
) -> ApiResult<Json<TimeStatsDto>> {
    let filter = stats_filter(&query, Utc::now())?;
    Ok(Json(dto(state.store.time_stats(&filter).await?)))
}

fn dto(_stats: TimeStats) -> TimeStatsDto {
    TimeStatsDto {}
}
