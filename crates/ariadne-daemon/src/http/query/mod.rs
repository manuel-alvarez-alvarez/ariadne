//! Prometheus and Loki compatible query routes over the telemetry store
//! (034), so Perses's and Grafana's standard plugins read the daemon's
//! metrics and logs as they would a Prometheus or a Loki.
//!
//! The store knows nothing of either protocol. This module is the adapter:
//! it reads through the store's read API only, presents its series under the
//! Prometheus naming rules ([`promql`]) and its log records as Loki streams
//! ([`logql`]), and answers in each protocol's own JSON shape.

mod logql;
mod promql;

use std::collections::{BTreeMap, BTreeSet};

use axum::extract::{Path, RawQuery, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use chrono::Utc;
use serde_json::{Value as JsonValue, json};

use ariadne_store::telemetry::{LogFilter, LogRecord};

use super::AppState;
use promql::{Labels, Value};

type ApiResult = Result<Json<JsonValue>, QueryError>;

/// A refused query: 400 and the Prometheus error envelope, whose `error`
/// names what was wrong.
#[derive(Debug)]
pub(super) struct QueryError(String);

impl IntoResponse for QueryError {
    fn into_response(self) -> Response {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"status": "error", "errorType": "bad_data", "error": self.0})),
        )
            .into_response()
    }
}

impl From<promql::EvalError> for QueryError {
    fn from(error: promql::EvalError) -> Self {
        Self(error.0)
    }
}

impl From<logql::LogqlError> for QueryError {
    fn from(error: logql::LogqlError) -> Self {
        Self(error.0)
    }
}

fn success(data: JsonValue) -> Json<JsonValue> {
    Json(json!({"status": "success", "data": data}))
}

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/-/healthy", get(healthy))
        .route("/api/v1/query", get(prom_query).post(prom_query))
        .route(
            "/api/v1/parse_query",
            get(prom_parse_query).post(prom_parse_query),
        )
        .route("/api/v1/query_range", get(prom_range).post(prom_range))
        .route("/api/v1/labels", get(prom_labels).post(prom_labels))
        .route("/api/v1/label/{name}/values", get(prom_label_values))
        .route("/api/v1/series", get(prom_series).post(prom_series))
        .route("/api/v1/metadata", get(prom_metadata))
        .route("/loki/api/v1/query", get(loki_query))
        .route("/loki/api/v1/query_range", get(loki_range))
        .route("/loki/api/v1/labels", get(loki_labels))
        .route("/loki/api/v1/label/{name}/values", get(loki_label_values))
        .route("/loki/api/v1/series", get(loki_series))
        .route("/loki/api/v1/index/stats", get(loki_index_stats))
        .route("/loki/api/v1/index/volume", get(loki_index_volume))
}

// ---- parameters ----

/// A request's parameters: the query string, then a form-encoded body, as
/// the Perses client sends a `POST`. A repeated key (`match[]`) keeps every
/// value.
#[derive(Debug, Default)]
struct Params(Vec<(String, String)>);

impl Params {
    fn new(query: Option<String>, body: &str) -> Result<Self, QueryError> {
        let mut pairs: Vec<(String, String)> =
            serde_urlencoded::from_str(query.as_deref().unwrap_or(""))
                .map_err(|error| QueryError(format!("invalid query string: {error}")))?;
        if !body.is_empty() {
            let form: Vec<(String, String)> = serde_urlencoded::from_str(body)
                .map_err(|error| QueryError(format!("invalid form body: {error}")))?;
            pairs.extend(form);
        }
        Ok(Self(pairs))
    }

    fn get(&self, key: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(k, v)| k == key && !v.is_empty())
            .map(|(_, v)| v.as_str())
    }

    fn all(&self, key: &str) -> Vec<&str> {
        self.0
            .iter()
            .filter(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
            .collect()
    }

    /// A Prometheus time: Unix seconds or RFC 3339, as milliseconds.
    fn time(&self, key: &str, default: Option<i64>) -> Result<i64, QueryError> {
        match self.get(key) {
            None => default.ok_or_else(|| QueryError(format!("missing parameter {key:?}"))),
            Some(text) => parse_time(text).ok_or_else(|| {
                QueryError(format!(
                    "invalid parameter {key:?}: cannot parse {text:?} to a valid timestamp"
                ))
            }),
        }
    }

    /// A Loki time: nanoseconds, Unix seconds or RFC 3339, as nanoseconds,
    /// exact: a boundary between two stored milliseconds stays between them.
    fn loki_time(&self, key: &str, default: i64) -> Result<i64, QueryError> {
        let Some(text) = self.get(key) else {
            return Ok(default);
        };
        parse_time_ns(text)
            .ok_or_else(|| QueryError(format!("invalid parameter {key:?}: {text:?}")))
    }

    fn step(&self, start: i64, end: i64) -> Result<i64, QueryError> {
        let step = match self.get("step") {
            Some(text) => duration_ms(text).ok_or_else(|| {
                QueryError(format!(
                    "invalid parameter \"step\": cannot parse {text:?} to a valid duration"
                ))
            })?,
            // Loki's default, and the CLI's: the span over 250, in seconds.
            None => ((end - start).max(1) + 249_999) / 250_000 * 1000,
        };
        if step <= 0 {
            return Err(QueryError(
                "zero or negative query resolution step widths are not accepted. \
                 Try a positive integer"
                    .into(),
            ));
        }
        if end < start {
            return Err(QueryError(
                "end timestamp must not be before start time".into(),
            ));
        }
        if (end - start) / step >= promql::MAX_POINTS {
            return Err(QueryError(
                "exceeded maximum resolution of 11,000 points per timeseries. \
                 Try decreasing the query resolution (?step=XX)"
                    .into(),
            ));
        }
        Ok(step)
    }
}

fn parse_time(text: &str) -> Option<i64> {
    if let Ok(seconds) = text.parse::<f64>() {
        return seconds
            .is_finite()
            .then(|| (seconds * 1000.0).round() as i64);
    }
    chrono::DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|time| time.timestamp_millis())
}

/// A Loki time as nanoseconds: an integer of 16 digits or more is already
/// nanoseconds, another number is Unix seconds, read digit by digit so no
/// float rounds it, and anything else is RFC 3339.
fn parse_time_ns(text: &str) -> Option<i64> {
    if text.len() >= 16 && text.bytes().all(|b| b.is_ascii_digit()) {
        return text.parse().ok();
    }
    let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
    let negative = whole.starts_with('-');
    let digits = whole.strip_prefix('-').unwrap_or(whole);
    if !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit())
        && fraction.len() <= 9
        && fraction.bytes().all(|b| b.is_ascii_digit())
    {
        let seconds: i64 = digits.parse().ok()?;
        let nanos: i64 = format!("{fraction:0<9}").parse().ok()?;
        let ns = seconds.checked_mul(1_000_000_000)?.checked_add(nanos)?;
        return Some(if negative { -ns } else { ns });
    }
    chrono::DateTime::parse_from_rfc3339(text)
        .ok()?
        .timestamp_nanos_opt()
}

/// A Prometheus duration (`1h30m`, `250ms`) or a number of seconds, as
/// milliseconds.
pub(super) fn duration_ms(text: &str) -> Option<i64> {
    if let Ok(seconds) = text.parse::<f64>() {
        return seconds
            .is_finite()
            .then(|| (seconds * 1000.0).round() as i64);
    }
    let mut total = 0i64;
    let mut rest = text;
    if rest.is_empty() {
        return None;
    }
    while !rest.is_empty() {
        let digits = rest.find(|c: char| !c.is_ascii_digit())?;
        if digits == 0 {
            return None;
        }
        let number: i64 = rest[..digits].parse().ok()?;
        rest = &rest[digits..];
        let unit_len = rest
            .find(|c: char| c.is_ascii_digit())
            .unwrap_or(rest.len());
        let unit = match &rest[..unit_len] {
            "ms" => 1,
            "s" => 1000,
            "m" => 60_000,
            "h" => 3_600_000,
            "d" => 86_400_000,
            "w" => 604_800_000,
            "y" => 31_536_000_000,
            _ => return None,
        };
        total = total.checked_add(number.checked_mul(unit)?)?;
        rest = &rest[unit_len..];
    }
    Some(total)
}

const NS_PER_MS: i64 = 1_000_000;
const HOUR_NS: i64 = 3_600_000 * NS_PER_MS;

fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
}

/// A metric query's points, from nanoseconds to the milliseconds the
/// Prometheus shapes are written in.
fn points_to_ms(series: Vec<(Labels, Vec<(i64, f64)>)>) -> Vec<(Labels, Vec<(i64, f64)>)> {
    series
        .into_iter()
        .map(|(labels, points)| {
            let points = points
                .into_iter()
                .map(|(ns, v)| (ns.div_euclid(NS_PER_MS), v))
                .collect();
            (labels, points)
        })
        .collect()
}

fn seconds(ms: i64) -> JsonValue {
    json!(ms as f64 / 1000.0)
}

/// A sample value as Prometheus writes it: a string, with `NaN` and the
/// infinities spelled its way.
fn number(value: f64) -> String {
    if value.is_nan() {
        "NaN".into()
    } else if value == f64::INFINITY {
        "+Inf".into()
    } else if value == f64::NEG_INFINITY {
        "-Inf".into()
    } else {
        value.to_string()
    }
}

fn matrix(series: Vec<(Labels, Vec<(i64, f64)>)>) -> JsonValue {
    json!({
        "resultType": "matrix",
        "result": series
            .into_iter()
            .map(|(metric, points)| json!({
                "metric": metric,
                "values": points
                    .into_iter()
                    .map(|(t, v)| json!([seconds(t), number(v)]))
                    .collect::<Vec<_>>(),
            }))
            .collect::<Vec<_>>(),
    })
}

fn vector(samples: Vec<(Labels, f64)>, t: i64) -> JsonValue {
    json!({
        "resultType": "vector",
        "result": samples
            .into_iter()
            .map(|(metric, value)| json!({"metric": metric, "value": [seconds(t), number(value)]}))
            .collect::<Vec<_>>(),
    })
}

// ---- Prometheus ----

async fn healthy() -> &'static str {
    "Prometheus Server is Healthy.\n"
}

async fn prom_query(
    State(state): State<AppState>,
    RawQuery(query): RawQuery,
    body: String,
) -> ApiResult {
    let params = Params::new(query, &body)?;
    let t = params.time("time", Some(now_ms()))?;
    let expr = promql::parse(params.get("query").unwrap_or(""))?;
    let series = promql::load(&state.telemetry_store, &expr, t, t).await?;
    let value = promql::Evaluator::new(&series, t, t).eval(&expr, t)?;
    Ok(success(match value {
        Value::Scalar(v) => json!({"resultType": "scalar", "result": [seconds(t), number(v)]}),
        Value::String(s) => json!({"resultType": "string", "result": [seconds(t), s]}),
        Value::Vector(samples) => vector(
            samples.into_iter().map(|s| (s.labels, s.value)).collect(),
            t,
        ),
        Value::Matrix(series) => matrix(series),
    }))
}

/// Parse a PromQL expression into the JSON AST that the Prometheus editor's
/// tree view consumes. Parsing intentionally does not apply the evaluator's
/// smaller supported subset: this endpoint describes syntax only.
async fn prom_parse_query(RawQuery(query): RawQuery, body: String) -> ApiResult {
    let params = Params::new(query, &body)?;
    let expr = promql::parse_ast(params.get("query").unwrap_or(""))?;
    let ast = serde_json::to_value(expr)
        .map_err(|error| QueryError(format!("cannot serialize PromQL AST: {error}")))?;
    Ok(success(ast))
}

async fn prom_range(
    State(state): State<AppState>,
    RawQuery(query): RawQuery,
    body: String,
) -> ApiResult {
    let params = Params::new(query, &body)?;
    let start = params.time("start", None)?;
    let end = params.time("end", None)?;
    let step = params.step(start, end)?;
    let expr = promql::parse(params.get("query").unwrap_or(""))?;
    let series = promql::load(&state.telemetry_store, &expr, start, end).await?;
    let evaluator = promql::Evaluator::new(&series, start, end);
    let mut out: BTreeMap<Labels, Vec<(i64, f64)>> = BTreeMap::new();
    let mut t = start;
    while t <= end {
        match evaluator.eval(&expr, t)? {
            Value::Scalar(v) => out.entry(Labels::new()).or_default().push((t, v)),
            Value::Vector(samples) => {
                for sample in samples {
                    out.entry(sample.labels)
                        .or_default()
                        .push((t, sample.value));
                }
            }
            other => {
                let kind = if matches!(other, Value::String(_)) {
                    "string"
                } else {
                    "range vector"
                };
                return Err(QueryError(format!(
                    "invalid expression type \"{kind}\" for range query, \
                     must be Scalar or instant Vector"
                )));
            }
        }
        t += step;
    }
    Ok(success(matrix(out.into_iter().collect())))
}

/// The series the request's `match[]` selectors name, or all of them.
async fn matched(state: &AppState, params: &Params) -> Result<Vec<promql::Entry>, QueryError> {
    let selectors = params
        .all("match[]")
        .into_iter()
        .map(promql::parse_selector)
        .collect::<Result<Vec<_>, _>>()?;
    // A bound narrows the list to the series with a sample, raw or rolled
    // up, between them.
    let bounds = match (params.get("start"), params.get("end")) {
        (None, None) => None,
        _ => Some((
            params.time("start", Some(promql::MIN_TIME_MS))?,
            params.time("end", Some(promql::MAX_TIME_MS))?,
        )),
    };
    let entries = promql::catalog(&state.telemetry_store).await?;
    let mut out = Vec::new();
    for entry in entries {
        let named = selectors.is_empty()
            || selectors
                .iter()
                .any(|selector| promql::selects(selector, &entry.labels));
        if !named {
            continue;
        }
        if let Some((start, end)) = bounds
            && !promql::has_points(&state.telemetry_store, &entry.id, start, end).await?
        {
            continue;
        }
        out.push(entry);
    }
    Ok(out)
}

async fn prom_labels(
    State(state): State<AppState>,
    RawQuery(query): RawQuery,
    body: String,
) -> ApiResult {
    let params = Params::new(query, &body)?;
    let names: BTreeSet<String> = matched(&state, &params)
        .await?
        .into_iter()
        .flat_map(|entry| entry.labels.into_keys())
        .collect();
    Ok(success(json!(names)))
}

async fn prom_label_values(
    State(state): State<AppState>,
    Path(name): Path<String>,
    RawQuery(query): RawQuery,
) -> ApiResult {
    let params = Params::new(query, "")?;
    let values: BTreeSet<String> = matched(&state, &params)
        .await?
        .into_iter()
        .filter_map(|entry| entry.labels.get(&name).cloned())
        .collect();
    Ok(success(json!(values)))
}

async fn prom_series(
    State(state): State<AppState>,
    RawQuery(query): RawQuery,
    body: String,
) -> ApiResult {
    let params = Params::new(query, &body)?;
    if params.all("match[]").is_empty() {
        return Err(QueryError("no match[] parameter provided".into()));
    }
    let series: Vec<Labels> = matched(&state, &params)
        .await?
        .into_iter()
        .map(|entry| entry.labels)
        .collect();
    Ok(success(json!(series)))
}

async fn prom_metadata(State(state): State<AppState>, RawQuery(query): RawQuery) -> ApiResult {
    let params = Params::new(query, "")?;
    let limit = params
        .get("limit")
        .and_then(|limit| limit.parse::<usize>().ok())
        .filter(|limit| *limit > 0);
    let mut out: BTreeMap<String, JsonValue> = BTreeMap::new();
    for entry in promql::catalog(&state.telemetry_store).await? {
        if params
            .get("metric")
            .is_some_and(|metric| metric != entry.family)
        {
            continue;
        }
        out.entry(entry.family.clone()).or_insert_with(|| {
            json!([{
                "type": entry.kind,
                "help": entry.help.clone().unwrap_or_default(),
                "unit": promql::unit_name(entry.unit.as_deref()),
            }])
        });
    }
    if let Some(limit) = limit {
        out = out.into_iter().take(limit).collect();
    }
    Ok(success(json!(out)))
}

// ---- Loki ----

/// A log record's stream labels. An empty one is left out, as Loki leaves
/// it out of a stream.
fn stream_labels(record: &LogRecord) -> Labels {
    [
        ("service", record.service.clone()),
        (
            "level",
            record
                .severity_text
                .as_ref()
                .map(|s| s.to_ascii_lowercase()),
        ),
        ("scope", record.scope.clone()),
        ("target", record.target.clone()),
    ]
    .into_iter()
    .filter_map(|(name, value)| Some((name.to_string(), value.filter(|v| !v.is_empty())?)))
    .collect()
}

/// Every log line in `[start, end)` (nanoseconds) that `keep` lets through.
/// The store is read from the millisecond at or before `start`; the exact
/// bounds then decide each line.
async fn read_lines(
    state: &AppState,
    start: i64,
    end: i64,
    keep: impl Fn(&Labels, &str) -> bool,
) -> Result<Vec<logql::Line>, QueryError> {
    let records = state
        .telemetry_store
        .read_logs(LogFilter {
            since: Some(promql::rfc3339(start.div_euclid(NS_PER_MS))),
            ..Default::default()
        })
        .await
        .map_err(|error| QueryError(error.to_string()))?;
    let mut lines = Vec::new();
    for record in records {
        let Ok(time) = chrono::DateTime::parse_from_rfc3339(&record.ts) else {
            continue;
        };
        let Some(ns) = time.timestamp_nanos_opt() else {
            continue;
        };
        if ns < start || ns >= end {
            continue;
        }
        let labels = stream_labels(&record);
        let line = ariadne_api::logs::render_message(record.body.as_deref(), &record.attributes);
        if keep(&labels, &line) {
            lines.push(logql::Line { ns, labels, line });
        }
    }
    lines.sort_by_key(|line| line.ns);
    Ok(lines)
}

/// Up to `limit` lines, newest first unless the direction is `forward`,
/// grouped into streams.
fn streams(mut lines: Vec<logql::Line>, limit: usize, forward: bool) -> JsonValue {
    if !forward {
        lines.reverse();
    }
    lines.truncate(limit);
    let mut grouped: BTreeMap<Labels, Vec<JsonValue>> = BTreeMap::new();
    for line in lines {
        grouped
            .entry(line.labels)
            .or_default()
            .push(json!([line.ns.to_string(), line.line]));
    }
    json!({
        "resultType": "streams",
        "result": grouped
            .into_iter()
            .map(|(stream, values)| json!({"stream": stream, "values": values}))
            .collect::<Vec<_>>(),
        "stats": {},
    })
}

fn limit_and_direction(params: &Params) -> Result<(usize, bool), QueryError> {
    let limit = match params.get("limit") {
        Some(text) => text
            .parse::<usize>()
            .map_err(|_| QueryError(format!("invalid parameter \"limit\": {text:?}")))?,
        None => 100,
    };
    let forward = match params.get("direction") {
        None | Some("backward") | Some("BACKWARD") => false,
        Some("forward") | Some("FORWARD") => true,
        Some(other) => {
            return Err(QueryError(format!(
                "invalid parameter \"direction\": {other:?}"
            )));
        }
    };
    Ok((limit, forward))
}

async fn loki_range(State(state): State<AppState>, RawQuery(query): RawQuery) -> ApiResult {
    let params = Params::new(query, "")?;
    let end = params.loki_time("end", now_ms() * NS_PER_MS)?;
    let start = params.loki_time("start", end - HOUR_NS)?;
    let query = logql::parse(params.get("query").unwrap_or(""))?;
    match query {
        logql::Query::Log(log) => {
            let (limit, forward) = limit_and_direction(&params)?;
            let lines = read_lines(&state, start, end, |labels, line| {
                log.selects(labels) && log.keeps(labels, line)
            })
            .await?;
            Ok(success(streams(lines, limit, forward)))
        }
        logql::Query::Metric(metric) => {
            let step = params.step(start.div_euclid(NS_PER_MS), end.div_euclid(NS_PER_MS))?;
            let range = metric.range_ms * NS_PER_MS;
            let lines = read_lines(&state, start - range, end + 1, |labels, line| {
                metric.log.selects(labels) && metric.log.keeps(labels, line)
            })
            .await?;
            let points = logql::evaluate(&metric, &lines, start, end, step * NS_PER_MS);
            let mut out = matrix(points_to_ms(points));
            out["stats"] = json!({});
            Ok(success(out))
        }
    }
}

async fn loki_query(State(state): State<AppState>, RawQuery(query): RawQuery) -> ApiResult {
    let params = Params::new(query, "")?;
    let t = params.loki_time("time", now_ms() * NS_PER_MS)?;
    let query = logql::parse(params.get("query").unwrap_or(""))?;
    match query {
        logql::Query::Log(log) => {
            let (limit, forward) = limit_and_direction(&params)?;
            let lines = read_lines(&state, t - HOUR_NS, t + 1, |labels, line| {
                log.selects(labels) && log.keeps(labels, line)
            })
            .await?;
            Ok(success(streams(lines, limit, forward)))
        }
        logql::Query::Metric(metric) => {
            let range = metric.range_ms * NS_PER_MS;
            let lines = read_lines(&state, t - range, t + 1, |labels, line| {
                metric.log.selects(labels) && metric.log.keeps(labels, line)
            })
            .await?;
            let samples = logql::evaluate(&metric, &lines, t, t, 1)
                .into_iter()
                .filter_map(|(labels, points)| Some((labels, points.first()?.1)))
                .collect();
            let mut out = vector(samples, t.div_euclid(NS_PER_MS));
            out["stats"] = json!({});
            Ok(success(out))
        }
    }
}

/// The stream label sets of the records in the request's span, or all.
async fn log_streams(state: &AppState, params: &Params) -> Result<BTreeSet<Labels>, QueryError> {
    let start = params.loki_time("start", 0)?;
    let end = params.loki_time("end", i64::MAX)?;
    Ok(read_lines(state, start, end, |_, _| true)
        .await?
        .into_iter()
        .map(|line| line.labels)
        .collect())
}

async fn loki_labels(State(state): State<AppState>, RawQuery(query): RawQuery) -> ApiResult {
    let params = Params::new(query, "")?;
    let names: BTreeSet<String> = log_streams(&state, &params)
        .await?
        .into_iter()
        .flat_map(|labels| labels.into_keys())
        .collect();
    Ok(success(json!(names)))
}

async fn loki_label_values(
    State(state): State<AppState>,
    Path(name): Path<String>,
    RawQuery(query): RawQuery,
) -> ApiResult {
    let params = Params::new(query, "")?;
    let values: BTreeSet<String> = log_streams(&state, &params)
        .await?
        .into_iter()
        .filter_map(|labels| labels.get(&name).cloned())
        .collect();
    Ok(success(json!(values)))
}

async fn loki_series(State(state): State<AppState>, RawQuery(query): RawQuery) -> ApiResult {
    let params = Params::new(query, "")?;
    let selectors = params
        .all("match[]")
        .into_iter()
        .chain(params.all("match"))
        .map(|text| match logql::parse(text)? {
            logql::Query::Log(log) => Ok(log),
            logql::Query::Metric(_) => Err(QueryError(format!(
                "invalid series selector {text:?}: a metric query"
            ))),
        })
        .collect::<Result<Vec<_>, QueryError>>()?;
    let series: Vec<Labels> = log_streams(&state, &params)
        .await?
        .into_iter()
        .filter(|labels| selectors.is_empty() || selectors.iter().any(|s| s.selects(labels)))
        .collect();
    Ok(success(json!(series)))
}

/// Grafana asks for index statistics; the store keeps no index, so this
/// answers the shape with nothing in it.
async fn loki_index_stats() -> Json<JsonValue> {
    Json(json!({"streams": 0, "chunks": 0, "entries": 0, "bytes": 0}))
}

async fn loki_index_volume() -> Json<JsonValue> {
    success(json!({"resultType": "vector", "result": []}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parameters_merge_the_query_string_and_a_form_body() {
        let params = Params::new(
            Some("match[]=up&time=10".into()),
            "match%5B%5D=down&query=sum(x)",
        )
        .unwrap();
        assert_eq!(params.all("match[]"), vec!["up", "down"]);
        assert_eq!(params.get("query"), Some("sum(x)"));
        assert_eq!(params.time("time", None).unwrap(), 10_000);
    }

    #[test]
    fn times_and_durations_take_every_spelling() {
        assert_eq!(parse_time("1.5"), Some(1_500));
        assert_eq!(parse_time("1970-01-01T00:00:02Z"), Some(2_000));
        assert_eq!(duration_ms("1h30m"), Some(5_400_000));
        assert_eq!(duration_ms("250ms"), Some(250));
        assert_eq!(duration_ms("15"), Some(15_000));
        assert_eq!(duration_ms("5x"), None);
        let params = Params::new(Some("start=1000000001000000000".into()), "").unwrap();
        assert_eq!(
            params.loki_time("start", 0).unwrap(),
            1_000_000_001_000_000_000
        );
        assert_eq!(parse_time_ns("1.000000001"), Some(1_000_000_001));
        assert_eq!(
            parse_time_ns("1970-01-01T00:00:01.000000002Z"),
            Some(1_000_000_002)
        );
    }

    #[test]
    fn the_default_step_is_the_span_over_250_in_whole_seconds() {
        let params = Params::default();
        assert_eq!(params.step(0, 3_600_000).unwrap(), 15_000);
        assert_eq!(params.step(0, 1_000).unwrap(), 1_000);
        let fine = Params::new(Some("step=1ms".into()), "").unwrap();
        assert!(fine.step(0, 3_600_000).unwrap_err().0.contains("11,000"));
    }

    #[test]
    fn values_are_strings_with_prometheus_spellings() {
        assert_eq!(number(1.0), "1");
        assert_eq!(number(0.25), "0.25");
        assert_eq!(number(f64::NAN), "NaN");
        assert_eq!(number(f64::NEG_INFINITY), "-Inf");
    }
}
