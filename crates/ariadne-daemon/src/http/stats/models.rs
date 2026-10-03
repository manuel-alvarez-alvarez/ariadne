//! `GET /v1/stats/models`: Which model does the job?

use axum::extract::{Query, State};
use chrono::Utc;
use utoipa::OpenApi;

use ariadne_api::stats::{ModelStatsDto, StatsQuery};
use ariadne_store::ModelStats;

use super::stats_filter;
use crate::http::AppState;
use crate::http::error::{ApiResult, Json};

/// This family's part of the API document.
#[derive(OpenApi)]
#[openapi(paths(models), components(schemas(ModelStatsDto)))]
pub(super) struct ModelsApi;

#[utoipa::path(get, path = "/v1/stats/models", tag = "stats",
    params(StatsQuery),
    responses((status = 200, body = ModelStatsDto),
              (status = 400, description = "`since` is neither a moment nor a span")))]
pub(super) async fn models(
    State(state): State<AppState>,
    Query(query): Query<StatsQuery>,
) -> ApiResult<Json<ModelStatsDto>> {
    let filter = stats_filter(&query, Utc::now())?;
    Ok(Json(dto(state.store.models_stats(&filter).await?)))
}

fn dto(_stats: ModelStats) -> ModelStatsDto {
    ModelStatsDto {}
}
