//! `GET /v1/stats/work`: What got done?

use axum::extract::{Query, State};
use chrono::Utc;
use utoipa::OpenApi;

use ariadne_api::stats::{StatsQuery, WorkStatsDto};
use ariadne_store::WorkStats;

use super::stats_filter;
use crate::http::AppState;
use crate::http::error::{ApiResult, Json};

/// This family's part of the API document.
#[derive(OpenApi)]
#[openapi(paths(work), components(schemas(WorkStatsDto)))]
pub(super) struct WorkApi;

#[utoipa::path(get, path = "/v1/stats/work", tag = "stats",
    params(StatsQuery),
    responses((status = 200, body = WorkStatsDto),
              (status = 400, description = "`since` is neither a moment nor a span")))]
pub(super) async fn work(
    State(state): State<AppState>,
    Query(query): Query<StatsQuery>,
) -> ApiResult<Json<WorkStatsDto>> {
    let filter = stats_filter(&query, Utc::now())?;
    Ok(Json(dto(state.store.work_stats(&filter).await?)))
}

fn dto(_stats: WorkStats) -> WorkStatsDto {
    WorkStatsDto {}
}
