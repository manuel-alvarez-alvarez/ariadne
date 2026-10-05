//! `GET /v1/stats/models`: Which model does the job?

use axum::extract::{Query, State};
use chrono::Utc;
use utoipa::OpenApi;

use ariadne_api::stats::{ModelStatDto, ModelStatsDto, StatsQuery};
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
    Ok(Json(dto(state.store.model_stats(&filter).await?)))
}

fn dto(stats: ModelStats) -> ModelStatsDto {
    ModelStatsDto {
        items: stats
            .items
            .into_iter()
            .map(|row| ModelStatDto {
                model: row.model,
                seat: row.seat,
                tasks: row.tasks,
                goals: row.goals,
                tokens: row.tokens,
                time_secs: row.time_secs,
                messages: row.messages,
                rounds_per_task: row.rounds_per_task,
                changes_per_task: row.changes_per_task,
            })
            .collect(),
    }
}
