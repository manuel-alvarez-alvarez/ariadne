//! Stats endpoints: the aggregates of the stats ledger (023), one route per
//! family, each narrowed by the same [`StatsQuery`].

use axum::extract::{Query, State};
use chrono::{DateTime, Duration, Utc};

use ariadne_api::stats::{
    AuthorReviewStatDto, MessageStatDto, ModelStatDto, ModelStatsResponse, PermissionStatDto,
    ReviewStatsDto, ReviewerStatDto, SkillCountDto, StatsQuery, SwitchReasonCountDto,
    SwitchStatDto, SwitchStatsResponse, ToolModelStatDto, ToolStatDto, ToolStatsDto,
};
use ariadne_store::{ModelStatRow, ReviewStats, StatsFilter, SwitchStatRow};

use super::AppState;
use super::error::{ApiError, ApiResult, Json};

#[utoipa::path(get, path = "/v1/stats/models", tag = "stats",
    params(StatsQuery),
    responses((status = 200, body = ModelStatsResponse),
              (status = 400, description = "`since` is neither a moment nor a span")))]
pub(super) async fn models(
    State(state): State<AppState>,
    Query(query): Query<StatsQuery>,
) -> ApiResult<Json<ModelStatsResponse>> {
    let filter = stats_filter(&query, Utc::now())?;
    let items = state
        .store
        .model_stats(&filter)
        .await?
        .into_iter()
        .map(model_stat_dto)
        .collect();
    Ok(Json(ModelStatsResponse { items }))
}

#[utoipa::path(get, path = "/v1/stats/reviews", tag = "stats", params(StatsQuery), responses((status = 200, body = ReviewStatsDto), (status = 400)))]
pub(super) async fn reviews(
    State(state): State<AppState>,
    Query(query): Query<StatsQuery>,
) -> ApiResult<Json<ReviewStatsDto>> {
    let stats = state
        .store
        .review_stats(&stats_filter(&query, Utc::now())?)
        .await?;
    Ok(Json(review_stats_dto(stats)))
}

fn review_stats_dto(stats: ReviewStats) -> ReviewStatsDto {
    ReviewStatsDto {
        authors: stats
            .authors
            .into_iter()
            .map(|r| AuthorReviewStatDto {
                model: r.model,
                approvals: r.approvals,
                mean_rounds: r.mean_rounds,
                median_rounds: r.median_rounds,
                first_pass_rate: r.first_pass_rate,
            })
            .collect(),
        reviewers: stats
            .reviewers
            .into_iter()
            .map(|r| ReviewerStatDto {
                model: r.model,
                verdicts: r.verdicts,
                approve_share: r.approve_share,
                mean_latency_secs: r.mean_latency_secs,
            })
            .collect(),
        messages: stats
            .messages
            .into_iter()
            .map(|r| MessageStatDto {
                kind: r.kind,
                from_actor: r.from_actor,
                total: r.total,
                mean_per_task: r.mean_per_task,
            })
            .collect(),
    }
}

#[utoipa::path(get, path = "/v1/stats/tools", tag = "stats",
    params(StatsQuery),
    responses((status = 200, body = ToolStatsDto),
              (status = 400, description = "`since` is neither a moment nor a span")))]
pub(super) async fn tools(
    State(state): State<AppState>,
    Query(query): Query<StatsQuery>,
) -> ApiResult<Json<ToolStatsDto>> {
    let stats = state
        .store
        .tool_stats(&stats_filter(&query, Utc::now())?)
        .await?;
    Ok(Json(ToolStatsDto {
        tools: stats
            .tools
            .into_iter()
            .map(|row| ToolStatDto {
                tool_name: row.tool_name,
                calls: row.calls,
                errors: row.errors,
                median_duration_ms: row.median_duration_ms,
                p90_duration_ms: row.p90_duration_ms,
            })
            .collect(),
        models: stats
            .models
            .into_iter()
            .map(|row| ToolModelStatDto {
                model: row.model,
                calls: row.calls,
                mean_duration_ms: row.mean_duration_ms,
            })
            .collect(),
        permissions: stats
            .permissions
            .into_iter()
            .map(|row| PermissionStatDto {
                decided_by: row.decided_by,
                answer: row.answer,
                permissions: row.permissions,
                mean_wait_ms: row.mean_wait_ms,
            })
            .collect(),
    }))
}

#[utoipa::path(get, path = "/v1/stats/switches", tag = "stats",
    params(StatsQuery),
    responses((status = 200, body = SwitchStatsResponse),
              (status = 400, description = "`since` is neither a moment nor a span")))]
pub(super) async fn switches(
    State(state): State<AppState>,
    Query(query): Query<StatsQuery>,
) -> ApiResult<Json<SwitchStatsResponse>> {
    let filter = stats_filter(&query, Utc::now())?;
    let stats = state.store.switch_stats(&filter).await?;
    Ok(Json(SwitchStatsResponse {
        items: stats.items.into_iter().map(switch_stat_dto).collect(),
        switches: stats.switches,
        exhaustions: stats.exhaustions,
    }))
}

/// The store's filter for a stats query, `since` read against `now`. Every
/// stats route reads its query through this.
pub(super) fn stats_filter(query: &StatsQuery, now: DateTime<Utc>) -> ApiResult<StatsFilter> {
    let since = match query.since.as_deref() {
        Some(since) => Some(parse_since(since, now).ok_or_else(|| {
            ApiError::bad_request(format!(
                "since {since:?} is neither an RFC 3339 moment nor a span such as 24h, 7d or 30d"
            ))
        })?),
        None => None,
    };
    Ok(StatsFilter {
        since,
        repo_id: query.repo.clone(),
    })
}

/// `since` as a moment: RFC 3339 as it is written, or `<n><unit>` back from
/// `now`, the unit `m`, `h`, `d` or `w`. None for anything else.
fn parse_since(since: &str, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    if let Ok(moment) = DateTime::parse_from_rfc3339(since) {
        return Some(moment.with_timezone(&Utc));
    }
    let unit = since.chars().last()?;
    let count: i64 = since[..since.len() - unit.len_utf8()].parse().ok()?;
    if count < 0 {
        return None;
    }
    let span = match unit {
        'm' => Duration::try_minutes(count)?,
        'h' => Duration::try_hours(count)?,
        'd' => Duration::try_days(count)?,
        'w' => Duration::try_weeks(count)?,
        _ => return None,
    };
    now.checked_sub_signed(span)
}

fn model_stat_dto(row: ModelStatRow) -> ModelStatDto {
    ModelStatDto {
        model: row.model,
        seat: row.seat,
        sessions: row.sessions,
        failed: row.failed,
        stalled: row.stalled,
        usage: row.usage.into(),
        cached_share: row.cached_share,
        mean_lifetime_secs: row.mean_lifetime_secs,
        skills: row
            .skills
            .into_iter()
            .map(|(name, sessions)| SkillCountDto { name, sessions })
            .collect(),
    }
}

fn switch_stat_dto(row: SwitchStatRow) -> SwitchStatDto {
    SwitchStatDto {
        model: row.model,
        switches: row.switches,
        by_reason: row
            .by_reason
            .into_iter()
            .map(|(reason, switches)| SwitchReasonCountDto { reason, switches })
            .collect(),
        exhaustions: row.exhaustions,
        automatic_share: row.automatic_share,
        arrivals: row.arrivals,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-10-03T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    fn at(moment: &str) -> Option<DateTime<Utc>> {
        Some(
            DateTime::parse_from_rfc3339(moment)
                .unwrap()
                .with_timezone(&Utc),
        )
    }

    /// A span counts back from now, in minutes, hours, days or weeks.
    #[test]
    fn a_span_counts_back_from_now() {
        assert_eq!(parse_since("30m", now()), at("2026-10-03T11:30:00Z"));
        assert_eq!(parse_since("24h", now()), at("2026-10-02T12:00:00Z"));
        assert_eq!(parse_since("7d", now()), at("2026-09-26T12:00:00Z"));
        assert_eq!(parse_since("2w", now()), at("2026-09-19T12:00:00Z"));
    }

    /// An RFC 3339 moment is taken as it is written, in any offset.
    #[test]
    fn a_moment_is_taken_as_written() {
        assert_eq!(
            parse_since("2026-09-01T02:00:00+02:00", now()),
            at("2026-09-01T00:00:00Z")
        );
    }

    /// Anything else is no moment: a unit nobody reads, a count missing or
    /// negative, a date without a time.
    #[test]
    fn anything_else_is_refused() {
        for bad in ["", "h", "7y", "-1d", "1.5h", "yesterday", "2026-09-01"] {
            assert_eq!(parse_since(bad, now()), None, "{bad:?}");
        }
    }
}
