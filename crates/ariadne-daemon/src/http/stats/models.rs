//! `GET /v1/stats/models`: Which model does the job?

use axum::extract::{Query, State};
use chrono::Utc;
use utoipa::OpenApi;

use ariadne_api::stats::{
    AuthorModelStatDto, ModelInterventionsDto, ModelStatDto, ModelStatsDto, ReviewerModelStatDto,
    StatsQuery,
};
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
                sessions: row.sessions,
                failed_sessions: row.failed_sessions,
                stalled_sessions: row.stalled_sessions,
                exhaustions: row.exhaustions,
                usage: row.usage.into(),
                cached_share: row.cached_share,
                mean_lifetime_secs: row.mean_lifetime_secs,
                total_lifetime_secs: row.total_lifetime_secs,
                interventions: ModelInterventionsDto {
                    permissions: row.interventions.permissions,
                    questions: row.interventions.questions,
                    stalls: row.interventions.stalls,
                    total: row.interventions.total,
                    person_secs: row.interventions.person_secs,
                },
                author: row.author.map(|a| AuthorModelStatDto {
                    tasks_finished: a.tasks_finished,
                    tasks_failed: a.tasks_failed,
                    tasks_cancelled: a.tasks_cancelled,
                    finish_rate: a.finish_rate,
                    first_pass_rate: a.first_pass_rate,
                    mean_review_rounds: a.mean_review_rounds,
                    contests_entered: a.contests_entered,
                    contests_won: a.contests_won,
                    win_rate: a.win_rate,
                    tokens_per_finished_task: a.tokens_per_finished_task,
                    median_lead_time_secs: a.median_lead_time_secs,
                    interventions_per_finished_task: a.interventions_per_finished_task,
                }),
                reviewer: row.reviewer.map(|r| ReviewerModelStatDto {
                    verdicts: r.verdicts,
                    approve_share: r.approve_share,
                    mean_latency_secs: r.mean_latency_secs,
                }),
            })
            .collect(),
    }
}
