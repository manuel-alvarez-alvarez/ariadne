//! `GET /v1/stats/attention`: How much did it need me?

use axum::extract::{Query, State};
use chrono::Utc;
use utoipa::OpenApi;

use ariadne_api::stats::{
    AttentionFlagDto, AttentionStatsDto, PermissionDeciderDto, PermissionStatsDto, StatsQuery,
};
use ariadne_store::AttentionStats;

use super::stats_filter;
use crate::http::AppState;
use crate::http::error::{ApiResult, Json};

/// This family's part of the API document.
#[derive(OpenApi)]
#[openapi(
    paths(attention),
    components(schemas(
        AttentionStatsDto,
        PermissionStatsDto,
        PermissionDeciderDto,
        AttentionFlagDto
    ))
)]
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

fn dto(stats: AttentionStats) -> AttentionStatsDto {
    AttentionStatsDto {
        permissions: PermissionStatsDto {
            total: stats.permissions.total,
            person_share: stats.permissions.person_share,
            by_decider: stats
                .permissions
                .by_decider
                .into_iter()
                .map(|row| PermissionDeciderDto {
                    decided_by: row.decided_by,
                    total: row.total,
                    allowed: row.allowed,
                    denied: row.denied,
                    cancelled: row.cancelled,
                    mean_wait_ms: row.mean_wait_ms,
                })
                .collect(),
        },
        flags: stats
            .flags
            .into_iter()
            .map(|row| AttentionFlagDto {
                reason: row.reason,
                raised: row.raised,
                mean_wait_secs: row.mean_wait_secs,
            })
            .collect(),
        sessions_failed: stats.sessions_failed,
        sessions_stalled: stats.sessions_stalled,
        exhaustions: stats.exhaustions,
    }
}
