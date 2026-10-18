//! `GET /v1/stats/time`: How long does it take?

use axum::extract::{Query, State};
use chrono::Utc;
use utoipa::OpenApi;

use ariadne_api::stats::{LeadTimeDto, PersonWaitDto, StatsQuery, StatusTimeDto, TimeStatsDto};
use ariadne_store::{LeadTime, PersonWait, StatusTime, TimeStats};

use super::stats_filter;
use crate::http::AppState;
use crate::http::error::{ApiResult, Json};

/// This family's part of the API document.
#[derive(OpenApi)]
#[openapi(
    paths(time),
    components(schemas(TimeStatsDto, LeadTimeDto, StatusTimeDto, PersonWaitDto))
)]
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

fn dto(stats: TimeStats) -> TimeStatsDto {
    TimeStatsDto {
        tasks: stats.tasks,
        lead_time: lead_time_dto(stats.lead_time),
        in_status: stats.in_status.into_iter().map(status_time_dto).collect(),
        waiting_on_person: person_wait_dto(stats.waiting_on_person),
    }
}

fn lead_time_dto(stats: LeadTime) -> LeadTimeDto {
    LeadTimeDto {
        median_secs: stats.median_secs,
        p90_secs: stats.p90_secs,
        mean_secs: stats.mean_secs,
    }
}

fn status_time_dto(row: StatusTime) -> StatusTimeDto {
    StatusTimeDto {
        status: row.status,
        total_secs: row.total_secs,
        median_secs: row.median_secs,
        share: row.share,
    }
}

fn person_wait_dto(wait: PersonWait) -> PersonWaitDto {
    PersonWaitDto {
        prompts: wait.prompts,
        total_secs: wait.total_secs,
        median_secs: wait.median_secs,
    }
}
