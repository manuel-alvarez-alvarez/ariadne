//! `GET /v1/stats/spend`: What did it spend?

use axum::extract::{Query, State};
use chrono::Utc;
use utoipa::OpenApi;

use ariadne_api::stats::{
    BucketDto, ModelSpendDto, PerFinishedTaskDto, SpendBucketDto, SpendStatsDto, SpendTotalsDto,
    StatsQuery,
};
use ariadne_store::{Bucket, ModelSpend, PerFinishedTask, SpendBucket, SpendStats, SpendTotals};

use super::stats_filter;
use crate::http::AppState;
use crate::http::error::{ApiResult, Json};

/// This family's part of the API document.
#[derive(OpenApi)]
#[openapi(paths(spend), components(schemas(SpendStatsDto)))]
pub(super) struct SpendApi;

#[utoipa::path(get, path = "/v1/stats/spend", tag = "stats",
    params(StatsQuery),
    responses((status = 200, body = SpendStatsDto),
              (status = 400, description = "`since` is neither a moment nor a span")))]
pub(super) async fn spend(
    State(state): State<AppState>,
    Query(query): Query<StatsQuery>,
) -> ApiResult<Json<SpendStatsDto>> {
    let filter = stats_filter(&query, Utc::now())?;
    Ok(Json(dto(state.store.spend_stats(&filter).await?)))
}

fn dto(stats: SpendStats) -> SpendStatsDto {
    SpendStatsDto {
        totals: totals_dto(stats.totals),
        per_finished_task: per_finished_task_dto(stats.per_finished_task),
        bucket: bucket_dto(stats.bucket),
        buckets: stats.buckets.into_iter().map(bucket_row_dto).collect(),
        by_model: stats.by_model.into_iter().map(model_dto).collect(),
    }
}

fn totals_dto(totals: SpendTotals) -> SpendTotalsDto {
    SpendTotalsDto {
        sessions: totals.sessions,
        input_tokens: totals.input_tokens,
        cached_input_tokens: totals.cached_input_tokens,
        output_tokens: totals.output_tokens,
        cached_share: totals.cached_share,
    }
}

fn per_finished_task_dto(per_task: PerFinishedTask) -> PerFinishedTaskDto {
    PerFinishedTaskDto {
        tasks: per_task.tasks,
        input_tokens: per_task.input_tokens,
        output_tokens: per_task.output_tokens,
    }
}

fn bucket_dto(bucket: Bucket) -> BucketDto {
    match bucket {
        Bucket::Hour => BucketDto::Hour,
        Bucket::Day => BucketDto::Day,
        Bucket::Week => BucketDto::Week,
    }
}

fn bucket_row_dto(bucket: SpendBucket) -> SpendBucketDto {
    SpendBucketDto {
        start: bucket.start,
        input_tokens: bucket.input_tokens,
        cached_input_tokens: bucket.cached_input_tokens,
        output_tokens: bucket.output_tokens,
    }
}

fn model_dto(model: ModelSpend) -> ModelSpendDto {
    ModelSpendDto {
        model: model.model,
        input_tokens: model.input_tokens,
        cached_input_tokens: model.cached_input_tokens,
        output_tokens: model.output_tokens,
        share: model.share,
    }
}
