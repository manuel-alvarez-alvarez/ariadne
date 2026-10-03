//! Stats endpoints: the aggregates of the stats ledger (023), one route per
//! family, each narrowed by the same [`StatsQuery`].
//!
//! Each family answers from a file of its own, which owns its handler and its
//! part of the API document. This file registers the six once.

mod attention;
mod models;
mod spend;
mod time;
mod tools;
mod work;

use axum::Router;
use axum::routing::get;
use chrono::{DateTime, Duration, Utc};
use utoipa::OpenApi;

use ariadne_api::stats::StatsQuery;
use ariadne_store::StatsFilter;

use super::AppState;
use super::error::{ApiError, ApiResult};

/// The six stats routes, in the order the screen shows the families.
pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/stats/work", get(work::work))
        .route("/v1/stats/time", get(time::time))
        .route("/v1/stats/spend", get(spend::spend))
        .route("/v1/stats/models", get(models::models))
        .route("/v1/stats/attention", get(attention::attention))
        .route("/v1/stats/tools", get(tools::tools))
}

/// The six families' paths and schemas, merged into one document.
pub(super) fn openapi() -> utoipa::openapi::OpenApi {
    [
        time::TimeApi::openapi(),
        spend::SpendApi::openapi(),
        models::ModelsApi::openapi(),
        attention::AttentionApi::openapi(),
        tools::ToolsApi::openapi(),
    ]
    .into_iter()
    .fold(
        work::WorkApi::openapi(),
        utoipa::openapi::OpenApi::merge_from,
    )
}

/// The store's filter for a stats query, `since` read against `now`. Every
/// stats route reads its query through this.
fn stats_filter(query: &StatsQuery, now: DateTime<Utc>) -> ApiResult<StatsFilter> {
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
