//! `GET /v1/stats/attention`: How much did it need me?

use axum::extract::{Query, State};
use chrono::Utc;
use utoipa::OpenApi;

use ariadne_api::stats::{AttentionStatsDto, StatsQuery};
use ariadne_store::AttentionStats;

use super::stats_filter;
use crate::http::AppState;
use crate::http::error::{ApiResult, Json};

/// This family's part of the API document.
#[derive(OpenApi)]
#[openapi(paths(attention), components(schemas(AttentionStatsDto)))]
pub(super) struct AttentionApi;

#[utoipa::path(get, path = "/v1/stats/attention", tag = "stats",
    params(StatsQuery),
    responses((status = 200, body = AttentionStatsDto),
              (status = 400, description = "`since` is neither a moment nor a span")))]
pub(super) async fn attention(
    State(state): State<AppState>,
    Query(query): Query<StatsQuery>,
) -> ApiResult<Json<AttentionStatsDto>> {
    let filter = stats_filter(&query, Utc::now())?;
    Ok(Json(dto(state.store.attention_stats(&filter).await?)))
}

fn dto(_stats: AttentionStats) -> AttentionStatsDto {
    AttentionStatsDto {}
}
