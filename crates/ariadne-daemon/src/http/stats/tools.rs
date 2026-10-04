//! `GET /v1/stats/tools`: What do the agents do?

use axum::extract::{Query, State};
use chrono::Utc;
use utoipa::OpenApi;

use ariadne_api::stats::{
    OtherToolsDto, StatsQuery, ToolKindStatDto, ToolStatDto, ToolStatsDto, ToolsStatsQuery,
};
use ariadne_store::ToolStats;

use super::stats_filter;
use crate::http::AppState;
use crate::http::error::{ApiError, ApiResult, Json};

/// The default count of top tools, where the query gives none.
const DEFAULT_LIMIT: u32 = 10;
/// The most top tools a query can ask for.
const MAX_LIMIT: u32 = 100;

/// This family's part of the API document.
#[derive(OpenApi)]
#[openapi(
    paths(tools),
    components(schemas(ToolStatsDto, ToolKindStatDto, ToolStatDto, OtherToolsDto))
)]
pub(super) struct ToolsApi;

#[utoipa::path(get, path = "/v1/stats/tools", tag = "stats",
    params(ToolsStatsQuery),
    responses((status = 200, body = ToolStatsDto),
              (status = 400, description = "`since` is neither a moment nor a span, or `limit` is not 1 to 100")))]
pub(super) async fn tools(
    State(state): State<AppState>,
    Query(query): Query<ToolsStatsQuery>,
) -> ApiResult<Json<ToolStatsDto>> {
    let filter = stats_filter(
        &StatsQuery {
            since: query.since,
            repo: query.repo,
        },
        Utc::now(),
    )?;
    let limit = match query.limit {
        None => DEFAULT_LIMIT,
        Some(limit) if (1..=MAX_LIMIT).contains(&limit) => limit,
        Some(limit) => {
            return Err(ApiError::bad_request(format!(
                "limit {limit} is not 1 to {MAX_LIMIT}"
            )));
        }
    };
    let stats = state.store.tools_stats(&filter, limit as usize).await?;
    Ok(Json(dto(stats)))
}

fn dto(stats: ToolStats) -> ToolStatsDto {
    ToolStatsDto {
        calls: stats.calls,
        errors: stats.errors,
        tools: stats.tools,
        by_kind: stats
            .by_kind
            .into_iter()
            .map(|row| ToolKindStatDto {
                kind: row.kind,
                calls: row.calls,
                errors: row.errors,
                median_duration_ms: row.median_duration_ms,
                p90_duration_ms: row.p90_duration_ms,
            })
            .collect(),
        top: stats
            .top
            .into_iter()
            .map(|row| ToolStatDto {
                tool_name: row.tool_name,
                kind: row.kind,
                calls: row.calls,
                errors: row.errors,
                median_duration_ms: row.median_duration_ms,
                p90_duration_ms: row.p90_duration_ms,
            })
            .collect(),
        other: OtherToolsDto {
            tools: stats.other.tools,
            calls: stats.other.calls,
            errors: stats.other.errors,
        },
    }
}
