//! `GET /v1/stats/work`: What got done?

use axum::extract::{Query, State};
use chrono::Utc;
use utoipa::OpenApi;

use ariadne_api::stats::{StatsQuery, WorkBucketDto, WorkStatsDto, WorkTotalsDto};
use ariadne_store::WorkStats;

use super::stats_filter;
use crate::http::AppState;
use crate::http::error::{ApiResult, Json};

/// This family's part of the API document.
#[derive(OpenApi)]
#[openapi(
    paths(work),
    components(schemas(WorkStatsDto, WorkTotalsDto, WorkBucketDto))
)]
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

fn dto(stats: WorkStats) -> WorkStatsDto {
    WorkStatsDto {
        totals: WorkTotalsDto {
            goals_completed: stats.totals.goals_completed,
            goals_cancelled: stats.totals.goals_cancelled,
            median_goal_lead_time_secs: stats.totals.median_goal_lead_time_secs,
            tasks_finished: stats.totals.tasks_finished,
            tasks_failed: stats.totals.tasks_failed,
            tasks_cancelled: stats.totals.tasks_cancelled,
            finish_rate: stats.totals.finish_rate,
            landed: stats.totals.landed,
        },
        bucket: stats.bucket,
        buckets: stats
            .buckets
            .into_iter()
            .map(|b| WorkBucketDto {
                start: b.start,
                tasks_finished: b.tasks_finished,
                tasks_failed: b.tasks_failed,
                tasks_cancelled: b.tasks_cancelled,
                goals_completed: b.goals_completed,
                landed: b.landed,
            })
            .collect(),
    }
}
