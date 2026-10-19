//! The PromQL evaluator behind `/api/v1/query` and `/api/v1/query_range`.
//!
//! A query is parsed with `promql-parser` and checked against the supported
//! subset first, so an unsupported function or operator is refused by name
//! before any read. The series its selectors name are then loaded once, for
//! the whole span the query needs, through the telemetry store's read API.
//! The tree is evaluated at each step over that snapshot, with no further
//! read.
//!
//! Beyond the raw retention a series has only hourly rollups. An hour with no
//! raw sample is read from its rollup: an instant selector sees the rollup's
//! `last`, and `min_over_time`, `max_over_time`, `avg_over_time`,
//! `sum_over_time` and `count_over_time` fold its columns in whole.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use promql_parser::label::Matcher;
use promql_parser::parser::{
    self, AggregateExpr, AtModifier, BinaryExpr, Call, Expr, LabelModifier, Offset,
    VectorMatchCardinality, VectorSelector,
};

use ariadne_store::telemetry::{MetricKind, SeriesMeta, TelemetryStore, TimeRange};

use crate::telemetry::{RESOURCE_SERVICE_LABEL, exported_key};

/// A series' labels, `__name__` included.
pub(super) type Labels = BTreeMap<String, String>;

pub(super) const NAME: &str = "__name__";
/// How far back an instant selector looks for a sample.
pub(super) const LOOKBACK_MS: i64 = 5 * 60 * 1000;
const HOUR_MS: i64 = 60 * 60 * 1000;
/// Prometheus refuses a range query with more points than this per series.
pub(super) const MAX_POINTS: i64 = 11_000;
/// The service of a series stored before the exporter put its resource's
/// service name on it.
const DEFAULT_SERVICE: &str = "ariadned";

const FUNCTIONS: &[&str] = &[
    "rate",
    "irate",
    "increase",
    "delta",
    "idelta",
    "avg_over_time",
    "sum_over_time",
    "min_over_time",
    "max_over_time",
    "count_over_time",
    "last_over_time",
    "quantile_over_time",
    "changes",
    "histogram_quantile",
    "abs",
    "ceil",
    "floor",
    "round",
    "clamp",
    "clamp_min",
    "clamp_max",
    "absent",
    "time",
    "vector",
    "scalar",
    "label_replace",
    "label_join",
    "sort",
    "sort_desc",
];
const AGGREGATIONS: &[&str] = &[
    "sum", "avg", "min", "max", "count", "topk", "bottomk", "quantile",
];
const OPERATORS: &[&str] = &[
    "+", "-", "*", "/", "%", "^", "==", "!=", ">", "<", ">=", "<=", "and", "or", "unless",
];

#[derive(Debug, PartialEq)]
pub(super) struct EvalError(pub(super) String);

type Result<T> = std::result::Result<T, EvalError>;

fn fail<T>(message: impl Into<String>) -> Result<T> {
    Err(EvalError(message.into()))
}

// ---- naming ----

/// A metric or label name with every character Prometheus does not allow
/// replaced by `_`: the OpenTelemetry dots, mostly.
pub(super) fn sanitize(name: &str) -> String {
    let mut out: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    out
}

/// The Prometheus suffix of an OpenTelemetry (UCUM) unit, if it has one.
fn unit_suffix(unit: Option<&str>) -> Option<&'static str> {
    Some(match unit? {
        "s" => "seconds",
        "ms" => "milliseconds",
        "us" => "microseconds",
        "ns" => "nanoseconds",
        "min" => "minutes",
        "h" => "hours",
        "d" => "days",
        "By" => "bytes",
        "KiBy" => "kibibytes",
        "MiBy" => "mebibytes",
        "GiBy" => "gibibytes",
        "KBy" => "kilobytes",
        "MBy" => "megabytes",
        "GBy" => "gigabytes",
        "%" => "percent",
        _ => return None,
    })
}

/// The unit metadata reports: its Prometheus suffix, or empty.
pub(super) fn unit_name(unit: Option<&str>) -> &'static str {
    unit_suffix(unit).unwrap_or("")
}

fn with_unit(stem: &str, unit: Option<&str>) -> String {
    let mut name = sanitize(stem);
    if let Some(suffix) = unit_suffix(unit)
        && !name.ends_with(&format!("_{suffix}"))
    {
        name.push('_');
        name.push_str(suffix);
    }
    name
}

/// The Prometheus name of a stored series. `component` is the histogram
/// suffix (`_bucket`, `_sum`, `_count`) when the series is one, so the unit
/// goes before it: `ariadne.task.lead_time` in seconds reads
/// `ariadne_task_lead_time_seconds_bucket`.
pub(super) fn prometheus_name(
    name: &str,
    kind: MetricKind,
    unit: Option<&str>,
    component: Option<&str>,
) -> String {
    if let Some(suffix) = component {
        let stem = name.strip_suffix(suffix).unwrap_or(name);
        return format!("{}{suffix}", with_unit(stem, unit));
    }
    let mut out = with_unit(name, unit);
    if kind == MetricKind::Counter && !out.ends_with("_total") {
        out.push_str("_total");
    }
    out
}

/// One stored series as the API presents it.
#[derive(Debug, Clone)]
pub(super) struct Entry {
    pub(super) id: String,
    pub(super) labels: Labels,
    /// The family a histogram component belongs to, and the family's type.
    pub(super) family: String,
    pub(super) kind: &'static str,
    pub(super) unit: Option<String>,
    pub(super) help: Option<String>,
}

/// Every stored series under its Prometheus name and labels. A histogram is
/// stored as `_bucket` series with an `le` label beside `_sum` and `_count`;
/// a `_sum` or `_count` counts as a component only beside such buckets.
pub(super) fn translate(series: Vec<SeriesMeta>) -> Vec<Entry> {
    let histograms: BTreeSet<String> = series
        .iter()
        .filter(|meta| meta.labels.get("le").is_some())
        .filter_map(|meta| meta.name.strip_suffix("_bucket").map(str::to_string))
        .collect();
    series
        .into_iter()
        .map(|meta| {
            let component = ["_bucket", "_sum", "_count"].into_iter().find(|suffix| {
                meta.name
                    .strip_suffix(suffix)
                    .is_some_and(|stem| histograms.contains(stem))
            });
            let name = prometheus_name(&meta.name, meta.kind, meta.unit.as_deref(), component);
            let family = match component {
                Some(suffix) => name.strip_suffix(suffix).unwrap_or(&name).to_string(),
                None => name.clone(),
            };
            let kind = if component.is_some() {
                "histogram"
            } else {
                meta.kind.as_str()
            };
            // `service` and `job` are the resource's service name, read from the
            // exporter's reserved key alone; an attribute of either name moves
            // to an `exported_` label, so no attribute can set them.
            let raw = meta.labels.as_object();
            let service = raw
                .and_then(|object| object.get(RESOURCE_SERVICE_LABEL))
                .and_then(serde_json::Value::as_str)
                .unwrap_or(DEFAULT_SERVICE)
                .to_string();
            let mut labels: Labels = raw
                .into_iter()
                .flatten()
                .filter(|(key, _)| key.as_str() != RESOURCE_SERVICE_LABEL)
                .map(|(key, value)| {
                    let value = match value {
                        serde_json::Value::String(text) => text.clone(),
                        other => other.to_string(),
                    };
                    (sanitize(key), value)
                })
                .collect();
            for reserved in ["service", "job"] {
                if let Some(value) = labels.remove(reserved) {
                    let moved = exported_key(reserved, |key| labels.contains_key(key));
                    labels.insert(moved, value);
                }
            }
            labels.insert("service".into(), service.clone());
            labels.insert("job".into(), service);
            labels.insert(NAME.into(), name);
            Entry {
                id: meta.id,
                labels,
                family,
                kind,
                unit: meta.unit,
                help: meta.description,
            }
        })
        .collect()
}

pub(super) async fn catalog(store: &TelemetryStore) -> Result<Vec<Entry>> {
    let series = store
        .list_series(&[])
        .await
        .map_err(|error| EvalError(error.to_string()))?;
    Ok(translate(series))
}

/// Whether a series' labels satisfy a selector's name and matchers.
pub(super) fn selects(selector: &VectorSelector, labels: &Labels) -> bool {
    let value = |name: &str| labels.get(name).map(String::as_str).unwrap_or("");
    selector
        .name
        .as_deref()
        .is_none_or(|name| value(NAME) == name)
        && selector
            .matchers
            .matchers
            .iter()
            .all(|matcher: &Matcher| matcher.is_match(value(&matcher.name)))
}

/// Parse a `match[]` selector.
pub(super) fn parse_selector(text: &str) -> Result<VectorSelector> {
    match parser::parse(text).map_err(EvalError)? {
        Expr::VectorSelector(selector) => {
            check_selector(&selector)?;
            if selector.at.is_some() {
                return fail("unsupported modifier @ in a series selector");
            }
            Ok(selector)
        }
        _ => fail(format!("invalid series selector {text:?}")),
    }
}

// ---- parsing and the supported subset ----

pub(super) fn parse(text: &str) -> Result<Expr> {
    if text.trim().is_empty() {
        return fail("missing query parameter");
    }
    let expr = parser::parse(text).map_err(EvalError)?;
    check(&expr)?;
    Ok(expr)
}

fn check_selector(selector: &VectorSelector) -> Result<()> {
    if !selector.matchers.or_matchers.is_empty() {
        return fail("unsupported operator or inside a selector");
    }
    Ok(())
}

/// Refuse, by name, anything outside the supported subset.
fn check(expr: &Expr) -> Result<()> {
    match expr {
        Expr::Aggregate(aggregate) => {
            let op = aggregate.op.to_string();
            if !AGGREGATIONS.contains(&op.as_str()) {
                return fail(format!("unsupported aggregation {op}"));
            }
            check(&aggregate.expr)?;
            aggregate.param.as_deref().map(check).transpose()?;
            Ok(())
        }
        Expr::Unary(unary) => check(&unary.expr),
        Expr::Binary(binary) => {
            let op = binary.op.to_string();
            if !OPERATORS.contains(&op.as_str()) {
                return fail(format!("unsupported operator {op}"));
            }
            if let Some(modifier) = &binary.modifier
                && modifier.fill_values != Default::default()
            {
                return fail("unsupported modifier fill");
            }
            check(&binary.lhs)?;
            check(&binary.rhs)
        }
        Expr::Paren(paren) => check(&paren.expr),
        Expr::Subquery(_) => fail("unsupported subquery"),
        Expr::NumberLiteral(_) | Expr::StringLiteral(_) => Ok(()),
        Expr::VectorSelector(selector) => check_selector(selector),
        Expr::MatrixSelector(matrix) => check_selector(&matrix.vs),
        Expr::Call(call) => {
            if !FUNCTIONS.contains(&call.func.name) {
                return fail(format!("unsupported function {}", call.func.name));
            }
            call.args.args.iter().try_for_each(|arg| check(arg))
        }
        Expr::Extension(_) => fail("unsupported expression"),
    }
}

fn offset_ms(offset: &Option<Offset>) -> i64 {
    match offset {
        Some(Offset::Pos(duration)) => duration.as_millis() as i64,
        Some(Offset::Neg(duration)) => -(duration.as_millis() as i64),
        None => 0,
    }
}

/// The time a selector reads at when its query is evaluated at `t`: `t`
/// itself, or the time its `@` pins — the query's own `start` or `end`, or
/// a timestamp — less its offset, as Prometheus reads it.
fn read_time(selector: &VectorSelector, t: i64, start: i64, end: i64) -> i64 {
    let pinned = match &selector.at {
        None => t,
        Some(AtModifier::Start) => start,
        Some(AtModifier::End) => end,
        Some(AtModifier::At(time)) => match time.duration_since(std::time::UNIX_EPOCH) {
            Ok(since) => since.as_millis() as i64,
            Err(before) => -(before.duration().as_millis() as i64),
        },
    };
    pinned - offset_ms(&selector.offset)
}

/// Every selector in the tree, with how far back of its evaluation time it
/// reads (its range and offset; the lookback is added by the loader).
fn selectors<'a>(expr: &'a Expr, out: &mut Vec<(&'a VectorSelector, i64)>) {
    match expr {
        Expr::Aggregate(aggregate) => {
            selectors(&aggregate.expr, out);
            if let Some(param) = &aggregate.param {
                selectors(param, out);
            }
        }
        Expr::Unary(unary) => selectors(&unary.expr, out),
        Expr::Binary(binary) => {
            selectors(&binary.lhs, out);
            selectors(&binary.rhs, out);
        }
        Expr::Paren(paren) => selectors(&paren.expr, out),
        Expr::VectorSelector(selector) => out.push((selector, 0)),
        Expr::MatrixSelector(matrix) => out.push((&matrix.vs, matrix.range.as_millis() as i64)),
        Expr::Call(call) => call.args.args.iter().for_each(|arg| selectors(arg, out)),
        _ => {}
    }
}

// ---- data ----

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Rollup {
    pub(super) start: i64,
    pub(super) min: f64,
    pub(super) max: f64,
    pub(super) sum: f64,
    pub(super) count: f64,
    pub(super) first: f64,
    pub(super) last: f64,
}

/// One series' points over the loaded span: raw samples, oldest first, and
/// the rollups of the hours that have no raw sample left.
#[derive(Debug, Clone, Default)]
pub(super) struct Series {
    pub(super) labels: Labels,
    pub(super) points: Vec<(i64, f64)>,
    pub(super) rollups: Vec<Rollup>,
}

impl Series {
    /// Keep a rollup only for an hour no raw sample is in.
    pub(super) fn new(labels: Labels, points: Vec<(i64, f64)>, rollups: Vec<Rollup>) -> Self {
        let raw_hours: BTreeSet<i64> = points
            .iter()
            .map(|(t, _)| t.div_euclid(HOUR_MS) * HOUR_MS)
            .collect();
        let rollups = rollups
            .into_iter()
            .filter(|rollup| !raw_hours.contains(&rollup.start))
            .collect();
        Self {
            labels,
            points,
            rollups,
        }
    }

    /// The value at `t`: the latest raw sample within the lookback, else the
    /// `last` of the rollup of the hour `t` is in, or that just ended.
    fn at(&self, t: i64) -> Option<f64> {
        let end = self.points.partition_point(|(time, _)| *time <= t);
        if end > 0 && self.points[end - 1].0 > t - LOOKBACK_MS {
            return Some(self.points[end - 1].1);
        }
        self.rollups
            .iter()
            .rev()
            .find(|rollup| rollup.start <= t && t < rollup.start + HOUR_MS + LOOKBACK_MS)
            .map(|rollup| rollup.last)
    }

    /// What lies in the window `(from, to]`.
    fn window(&self, from: i64, to: i64) -> Window {
        let lo = self.points.partition_point(|(time, _)| *time <= from);
        let hi = self.points.partition_point(|(time, _)| *time <= to);
        Window {
            points: self.points[lo..hi].to_vec(),
            rollups: self
                .rollups
                .iter()
                .filter(|rollup| rollup.start + HOUR_MS > from && rollup.start <= to)
                .copied()
                .collect(),
        }
    }
}

#[derive(Debug, Default)]
struct Window {
    points: Vec<(i64, f64)>,
    rollups: Vec<Rollup>,
}

impl Window {
    /// Raw points with each rollup spread into its first and last value.
    fn expanded(&self) -> Vec<(i64, f64)> {
        let mut all = self.points.clone();
        for rollup in &self.rollups {
            all.push((rollup.start, rollup.first));
            if rollup.count > 1.0 {
                all.push((rollup.start + HOUR_MS - 1, rollup.last));
            }
        }
        all.sort_by_key(|(time, _)| *time);
        all
    }

    fn is_empty(&self) -> bool {
        self.points.is_empty() && self.rollups.is_empty()
    }

    fn count(&self) -> f64 {
        self.points.len() as f64 + self.rollups.iter().map(|r| r.count).sum::<f64>()
    }

    fn sum(&self) -> f64 {
        self.points.iter().map(|(_, v)| v).sum::<f64>()
            + self.rollups.iter().map(|r| r.sum).sum::<f64>()
    }

    fn min(&self) -> f64 {
        let raw = self.points.iter().map(|(_, v)| *v);
        raw.chain(self.rollups.iter().map(|r| r.min))
            .fold(f64::INFINITY, f64::min)
    }

    fn max(&self) -> f64 {
        let raw = self.points.iter().map(|(_, v)| *v);
        raw.chain(self.rollups.iter().map(|r| r.max))
            .fold(f64::NEG_INFINITY, f64::max)
    }
}

/// Load every series the query's selectors name, over all it can read
/// between `start` and `end`.
pub(super) async fn load(
    store: &TelemetryStore,
    expr: &Expr,
    start: i64,
    end: i64,
) -> Result<Vec<Series>> {
    let mut wanted = Vec::new();
    selectors(expr, &mut wanted);
    if wanted.is_empty() {
        return Ok(Vec::new());
    }
    let mut from = start;
    let mut to = end;
    for (selector, range) in &wanted {
        from = from.min(read_time(selector, start, start, end) - range - LOOKBACK_MS - HOUR_MS);
        to = to.max(read_time(selector, end, start, end));
    }
    let span = TimeRange {
        since: rfc3339(from),
        until: rfc3339(to),
    };
    let mut out = Vec::new();
    for entry in catalog(store).await? {
        if !wanted
            .iter()
            .any(|(selector, _)| selects(selector, &entry.labels))
        {
            continue;
        }
        let read_error = |error: ariadne_store::StoreError| EvalError(error.to_string());
        let points = store
            .read_samples(&entry.id, span.clone())
            .await
            .map_err(read_error)?
            .into_iter()
            .filter_map(|sample| Some((millis(&sample.ts)?, sample.value)))
            .collect();
        let rollups = store
            .read_rollups(&entry.id, span.clone())
            .await
            .map_err(read_error)?
            .into_iter()
            .filter_map(|rollup| {
                Some(Rollup {
                    start: millis(&rollup.hour)?,
                    min: rollup.min,
                    max: rollup.max,
                    sum: rollup.sum,
                    count: rollup.count as f64,
                    first: rollup.first,
                    last: rollup.last,
                })
            })
            .collect();
        out.push(Series::new(entry.labels, points, rollups));
    }
    Ok(out)
}

/// The earliest and latest times a discovery bound may default to: the
/// Unix epoch and the last millisecond of year 9999, both inside RFC 3339.
pub(super) const MIN_TIME_MS: i64 = 0;
pub(super) const MAX_TIME_MS: i64 = 253_402_300_799_999;

/// Whether a series has a raw sample in `[start, end]`, or a rollup of an
/// hour that overlaps it.
pub(super) async fn has_points(
    store: &TelemetryStore,
    id: &str,
    start: i64,
    end: i64,
) -> Result<bool> {
    let (start, end) = (start.max(MIN_TIME_MS), end.min(MAX_TIME_MS));
    if end < start {
        return Ok(false);
    }
    let read_error = |error: ariadne_store::StoreError| EvalError(error.to_string());
    let raw = store
        .read_samples(
            id,
            TimeRange {
                since: rfc3339(start),
                until: rfc3339(end),
            },
        )
        .await
        .map_err(read_error)?;
    if !raw.is_empty() {
        return Ok(true);
    }
    let rollups = store
        .read_rollups(
            id,
            TimeRange {
                since: rfc3339(start.div_euclid(HOUR_MS) * HOUR_MS),
                until: rfc3339(end),
            },
        )
        .await
        .map_err(read_error)?;
    Ok(!rollups.is_empty())
}

pub(super) fn rfc3339(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .unwrap_or_default()
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn millis(ts: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(ts)
        .ok()
        .map(|time| time.timestamp_millis())
}

// ---- values ----

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Sample {
    pub(super) labels: Labels,
    pub(super) value: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum Value {
    Scalar(f64),
    String(String),
    Vector(Vec<Sample>),
    /// Only a bare range selector evaluates to this.
    Matrix(Vec<(Labels, Vec<(i64, f64)>)>),
}

impl Value {
    fn kind(&self) -> &'static str {
        match self {
            Value::Scalar(_) => "scalar",
            Value::String(_) => "string",
            Value::Vector(_) => "instant vector",
            Value::Matrix(_) => "range vector",
        }
    }
}

fn without_name(mut labels: Labels) -> Labels {
    labels.remove(NAME);
    labels
}

// ---- evaluation ----

pub(super) struct Evaluator<'a> {
    series: &'a [Series],
    /// The query's first and last evaluation time, which `@ start()` and
    /// `@ end()` pin a selector to: one time for an instant query.
    start: i64,
    end: i64,
}

impl<'a> Evaluator<'a> {
    pub(super) fn new(series: &'a [Series], start: i64, end: i64) -> Self {
        Self { series, start, end }
    }

    pub(super) fn eval(&self, expr: &Expr, t: i64) -> Result<Value> {
        match expr {
            Expr::NumberLiteral(number) => Ok(Value::Scalar(number.val)),
            Expr::StringLiteral(text) => Ok(Value::String(text.val.clone())),
            Expr::Paren(paren) => self.eval(&paren.expr, t),
            Expr::Unary(unary) => match self.eval(&unary.expr, t)? {
                Value::Scalar(value) => Ok(Value::Scalar(-value)),
                Value::Vector(samples) => Ok(Value::Vector(
                    samples
                        .into_iter()
                        .map(|sample| Sample {
                            labels: without_name(sample.labels),
                            value: -sample.value,
                        })
                        .collect(),
                )),
                other => fail(format!("unary minus over a {}", other.kind())),
            },
            Expr::VectorSelector(selector) => Ok(Value::Vector(self.instant(selector, t))),
            Expr::MatrixSelector(matrix) => {
                let range = matrix.range.as_millis() as i64;
                Ok(Value::Matrix(
                    self.windows(&matrix.vs, range, t)
                        .into_iter()
                        .map(|(labels, window)| (labels, window.expanded()))
                        .collect(),
                ))
            }
            Expr::Call(call) => self.call(call, t),
            Expr::Aggregate(aggregate) => self.aggregate(aggregate, t),
            Expr::Binary(binary) => self.binary(binary, t),
            Expr::Subquery(_) => fail("unsupported subquery"),
            Expr::Extension(_) => fail("unsupported expression"),
        }
    }

    fn instant(&self, selector: &VectorSelector, t: i64) -> Vec<Sample> {
        let at = read_time(selector, t, self.start, self.end);
        self.series
            .iter()
            .filter(|series| selects(selector, &series.labels))
            .filter_map(|series| {
                Some(Sample {
                    labels: series.labels.clone(),
                    value: series.at(at)?,
                })
            })
            .collect()
    }

    fn windows(&self, selector: &VectorSelector, range: i64, t: i64) -> Vec<(Labels, Window)> {
        let at = read_time(selector, t, self.start, self.end);
        self.series
            .iter()
            .filter(|series| selects(selector, &series.labels))
            .map(|series| (series.labels.clone(), series.window(at - range, at)))
            .filter(|(_, window)| !window.is_empty())
            .collect()
    }

    fn vector(&self, expr: &Expr, t: i64) -> Result<Vec<Sample>> {
        match self.eval(expr, t)? {
            Value::Vector(samples) => Ok(samples),
            other => fail(format!(
                "expected an instant vector, got a {}",
                other.kind()
            )),
        }
    }

    fn scalar(&self, expr: &Expr, t: i64) -> Result<f64> {
        match self.eval(expr, t)? {
            Value::Scalar(value) => Ok(value),
            other => fail(format!("expected a scalar, got a {}", other.kind())),
        }
    }

    fn string(&self, expr: &Expr, t: i64) -> Result<String> {
        match self.eval(expr, t)? {
            Value::String(value) => Ok(value),
            other => fail(format!("expected a string, got a {}", other.kind())),
        }
    }

    fn call(&self, call: &Call, t: i64) -> Result<Value> {
        let name = call.func.name;
        let args = &call.args.args;
        let arg = |index: usize| -> Result<&Expr> {
            args.get(index)
                .map(|arg| arg.as_ref())
                .ok_or_else(|| EvalError(format!("missing argument to {name}")))
        };
        let map = |samples: Vec<Sample>, f: &dyn Fn(f64) -> f64| {
            Value::Vector(
                samples
                    .into_iter()
                    .map(|sample| Sample {
                        labels: without_name(sample.labels),
                        value: f(sample.value),
                    })
                    .collect(),
            )
        };
        match name {
            "time" => Ok(Value::Scalar(t as f64 / 1000.0)),
            "vector" => Ok(Value::Vector(vec![Sample {
                labels: Labels::new(),
                value: self.scalar(arg(0)?, t)?,
            }])),
            "scalar" => {
                let samples = self.vector(arg(0)?, t)?;
                Ok(Value::Scalar(match samples.as_slice() {
                    [only] => only.value,
                    _ => f64::NAN,
                }))
            }
            "abs" => Ok(map(self.vector(arg(0)?, t)?, &f64::abs)),
            "ceil" => Ok(map(self.vector(arg(0)?, t)?, &f64::ceil)),
            "floor" => Ok(map(self.vector(arg(0)?, t)?, &f64::floor)),
            "round" => {
                let nearest = match args.get(1) {
                    Some(to) => self.scalar(to, t)?,
                    None => 1.0,
                };
                let inverse = 1.0 / nearest;
                Ok(map(self.vector(arg(0)?, t)?, &|v| {
                    (v * inverse + 0.5).floor() / inverse
                }))
            }
            "clamp" => {
                let (lo, hi) = (self.scalar(arg(1)?, t)?, self.scalar(arg(2)?, t)?);
                if lo > hi {
                    return Ok(Value::Vector(Vec::new()));
                }
                Ok(map(self.vector(arg(0)?, t)?, &|v| {
                    min_nan(max_nan(v, lo), hi)
                }))
            }
            "clamp_min" => {
                let lo = self.scalar(arg(1)?, t)?;
                Ok(map(self.vector(arg(0)?, t)?, &|v| max_nan(v, lo)))
            }
            "clamp_max" => {
                let hi = self.scalar(arg(1)?, t)?;
                Ok(map(self.vector(arg(0)?, t)?, &|v| min_nan(v, hi)))
            }
            "absent" => {
                if !self.vector(arg(0)?, t)?.is_empty() {
                    return Ok(Value::Vector(Vec::new()));
                }
                Ok(Value::Vector(vec![Sample {
                    labels: absent_labels(arg(0)?),
                    value: 1.0,
                }]))
            }
            "sort" | "sort_desc" => {
                let mut samples = self.vector(arg(0)?, t)?;
                samples.sort_by(|a, b| a.value.total_cmp(&b.value));
                if name == "sort_desc" {
                    samples.reverse();
                }
                Ok(Value::Vector(samples))
            }
            "label_replace" => {
                let samples = self.vector(arg(0)?, t)?;
                let destination = self.string(arg(1)?, t)?;
                let replacement = self.string(arg(2)?, t)?;
                let source = self.string(arg(3)?, t)?;
                let pattern = self.string(arg(4)?, t)?;
                let regex = regex::Regex::new(&format!("^(?s:{pattern})$"))
                    .map_err(|error| EvalError(format!("invalid regular expression: {error}")))?;
                Ok(Value::Vector(label_replace(
                    samples,
                    &destination,
                    &replacement,
                    &source,
                    &regex,
                )))
            }
            "label_join" => {
                let samples = self.vector(arg(0)?, t)?;
                let destination = self.string(arg(1)?, t)?;
                let separator = self.string(arg(2)?, t)?;
                let sources = args[3..]
                    .iter()
                    .map(|source| self.string(source, t))
                    .collect::<Result<Vec<_>>>()?;
                Ok(Value::Vector(label_join(
                    samples,
                    &destination,
                    &separator,
                    &sources,
                )))
            }
            "histogram_quantile" => {
                let q = self.scalar(arg(0)?, t)?;
                Ok(Value::Vector(histogram_quantile(
                    q,
                    self.vector(arg(1)?, t)?,
                )))
            }
            "quantile_over_time" => {
                let q = self.scalar(arg(0)?, t)?;
                self.over_time(arg(1)?, t, name, |window| {
                    quantile(q, window.expanded().into_iter().map(|(_, v)| v).collect())
                })
            }
            _ => self.over_time(arg(0)?, t, name, |window| range_function(name, window)),
        }
    }

    /// Apply a range function to each series' window of a range selector.
    fn over_time(
        &self,
        expr: &Expr,
        t: i64,
        name: &str,
        f: impl Fn(&WindowAt) -> Option<f64>,
    ) -> Result<Value> {
        let Expr::MatrixSelector(matrix) = expr else {
            return fail(format!("{name} expects a range vector"));
        };
        let range = matrix.range.as_millis() as i64;
        let at = read_time(&matrix.vs, t, self.start, self.end);
        let keep_name = name == "last_over_time";
        Ok(Value::Vector(
            self.windows(&matrix.vs, range, t)
                .into_iter()
                .filter_map(|(labels, window)| {
                    let value = f(&WindowAt {
                        window,
                        start: at - range,
                        end: at,
                    })?;
                    let labels = if keep_name {
                        labels
                    } else {
                        without_name(labels)
                    };
                    Some(Sample { labels, value })
                })
                .collect(),
        ))
    }

    fn aggregate(&self, aggregate: &AggregateExpr, t: i64) -> Result<Value> {
        let op = aggregate.op.to_string();
        let samples = self.vector(&aggregate.expr, t)?;
        let param = match &aggregate.param {
            Some(param) => Some(self.scalar(param, t)?),
            None => None,
        };
        let group_of = |labels: &Labels| -> Labels {
            match &aggregate.modifier {
                Some(LabelModifier::Include(by)) => labels
                    .iter()
                    .filter(|(key, _)| by.labels.contains(key))
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect(),
                Some(LabelModifier::Exclude(without)) => labels
                    .iter()
                    .filter(|(key, _)| *key != NAME && !without.labels.contains(key))
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect(),
                None => Labels::new(),
            }
        };
        let mut groups: BTreeMap<Labels, Vec<Sample>> = BTreeMap::new();
        for sample in samples {
            groups
                .entry(group_of(&sample.labels))
                .or_default()
                .push(sample);
        }
        let mut out = Vec::new();
        for (group, members) in groups {
            let values: Vec<f64> = members.iter().map(|sample| sample.value).collect();
            let value = match op.as_str() {
                "sum" => values.iter().sum(),
                "avg" => values.iter().sum::<f64>() / values.len() as f64,
                "min" => values.iter().copied().fold(f64::NAN, nan_min),
                "max" => values.iter().copied().fold(f64::NAN, nan_max),
                "count" => values.len() as f64,
                "quantile" => quantile(param.unwrap_or(f64::NAN), values).unwrap_or(f64::NAN),
                "topk" | "bottomk" => {
                    let k = param.unwrap_or(0.0);
                    if k.is_nan() || k < 1.0 {
                        continue;
                    }
                    let mut members = members;
                    members.sort_by(|a, b| {
                        let order = a.value.total_cmp(&b.value);
                        if op == "topk" { order.reverse() } else { order }
                    });
                    out.extend(members.into_iter().take(k as usize));
                    continue;
                }
                other => return fail(format!("unsupported aggregation {other}")),
            };
            out.push(Sample {
                labels: group,
                value,
            });
        }
        Ok(Value::Vector(out))
    }

    fn binary(&self, binary: &BinaryExpr, t: i64) -> Result<Value> {
        let op = binary.op.to_string();
        let return_bool = binary.modifier.as_ref().is_some_and(|m| m.return_bool);
        let comparison = binary.op.is_comparison_operator();
        let lhs = self.eval(&binary.lhs, t)?;
        let rhs = self.eval(&binary.rhs, t)?;
        match (lhs, rhs) {
            (Value::Scalar(l), Value::Scalar(r)) => {
                let value = apply(&op, l, r)?;
                Ok(Value::Scalar(if comparison {
                    value.map_or(0.0, |_| 1.0)
                } else {
                    value.unwrap_or(f64::NAN)
                }))
            }
            (Value::Vector(samples), Value::Scalar(scalar)) => Ok(Value::Vector(vector_scalar(
                &op,
                samples,
                scalar,
                false,
                comparison,
                return_bool,
            )?)),
            (Value::Scalar(scalar), Value::Vector(samples)) => Ok(Value::Vector(vector_scalar(
                &op,
                samples,
                scalar,
                true,
                comparison,
                return_bool,
            )?)),
            (Value::Vector(l), Value::Vector(r)) => {
                let modifier = binary.modifier.clone().unwrap_or_default();
                if binary.op.is_set_operator() {
                    return Ok(Value::Vector(set_operation(
                        &op,
                        l,
                        r,
                        modifier.matching.as_ref(),
                    )));
                }
                Ok(Value::Vector(vector_vector(
                    &op,
                    l,
                    r,
                    &modifier.card,
                    modifier.matching.as_ref(),
                    comparison,
                    return_bool,
                )?))
            }
            (l, r) => fail(format!(
                "operator {op} between a {} and a {}",
                l.kind(),
                r.kind()
            )),
        }
    }
}

struct WindowAt {
    window: Window,
    start: i64,
    end: i64,
}

impl std::ops::Deref for WindowAt {
    type Target = Window;
    fn deref(&self) -> &Window {
        &self.window
    }
}

/// Go's `math.Max`, which Prometheus's `clamp` uses: a `NaN` operand gives
/// `NaN`, where Rust's `f64::max` would drop it.
fn max_nan(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.max(b)
    }
}

/// Go's `math.Min`, the same way.
fn min_nan(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.min(b)
    }
}

fn nan_min(acc: f64, value: f64) -> f64 {
    if acc.is_nan() || value < acc {
        value
    } else {
        acc
    }
}

fn nan_max(acc: f64, value: f64) -> f64 {
    if acc.is_nan() || value > acc {
        value
    } else {
        acc
    }
}

fn range_function(name: &str, window: &WindowAt) -> Option<f64> {
    match name {
        "rate" => extrapolated(window, true, true),
        "increase" => extrapolated(window, true, false),
        "delta" => extrapolated(window, false, false),
        "irate" | "idelta" => {
            let points = window.expanded();
            let [.., (t0, v0), (t1, v1)] = points.as_slice() else {
                return None;
            };
            let change = if name == "irate" && v1 < v0 {
                *v1
            } else {
                v1 - v0
            };
            if name == "idelta" {
                return Some(change);
            }
            if t1 == t0 {
                return None;
            }
            Some(change * 1000.0 / (t1 - t0) as f64)
        }
        "avg_over_time" => Some(window.sum() / window.count()),
        "sum_over_time" => Some(window.sum()),
        "min_over_time" => Some(window.min()),
        "max_over_time" => Some(window.max()),
        "count_over_time" => Some(window.count()),
        "last_over_time" => window.expanded().last().map(|(_, v)| *v),
        "changes" => {
            let points = window.expanded();
            Some(
                points
                    .windows(2)
                    .filter(|pair| {
                        pair[0].1 != pair[1].1 && !(pair[0].1.is_nan() && pair[1].1.is_nan())
                    })
                    .count() as f64,
            )
        }
        _ => None,
    }
}

/// Prometheus's `extrapolatedRate`: the change over the window, with counter
/// resets added back for a counter, extrapolated to the window's edges.
fn extrapolated(window: &WindowAt, counter: bool, per_second: bool) -> Option<f64> {
    let points = window.expanded();
    let (first, last) = (points.first()?, points.last()?);
    if points.len() < 2 || last.0 == first.0 {
        return None;
    }
    let mut result = last.1 - first.1;
    if counter {
        let mut previous = first.1;
        for (_, value) in &points[1..] {
            if *value < previous {
                result += previous;
            }
            previous = *value;
        }
    }
    let sampled = (last.0 - first.0) as f64 / 1000.0;
    let mut to_start = (first.0 - window.start) as f64 / 1000.0;
    let to_end = (window.end - last.0) as f64 / 1000.0;
    let average = sampled / (points.len() - 1) as f64;
    if counter && result > 0.0 && first.1 >= 0.0 {
        to_start = to_start.min(sampled * (first.1 / result));
    }
    let threshold = average * 1.1;
    let mut interval = sampled;
    interval += if to_start < threshold {
        to_start
    } else {
        average / 2.0
    };
    interval += if to_end < threshold {
        to_end
    } else {
        average / 2.0
    };
    result *= interval / sampled;
    if per_second {
        result /= (window.end - window.start) as f64 / 1000.0;
    }
    Some(result)
}

/// The φ-quantile of `values`, interpolated the way Prometheus does.
fn quantile(q: f64, mut values: Vec<f64>) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    if q.is_nan() {
        return Some(f64::NAN);
    }
    if q < 0.0 {
        return Some(f64::NEG_INFINITY);
    }
    if q > 1.0 {
        return Some(f64::INFINITY);
    }
    values.sort_by(f64::total_cmp);
    let rank = q * (values.len() - 1) as f64;
    let lower = rank.floor() as usize;
    let upper = (lower + 1).min(values.len() - 1);
    let weight = rank - rank.floor();
    Some(values[lower] * (1.0 - weight) + values[upper] * weight)
}

/// Prometheus's `bucketQuantile` over each group of `_bucket` samples.
fn histogram_quantile(q: f64, samples: Vec<Sample>) -> Vec<Sample> {
    let mut groups: BTreeMap<Labels, Vec<(f64, f64)>> = BTreeMap::new();
    for sample in samples {
        let Some(le) = sample.labels.get("le").and_then(|le| parse_float(le)) else {
            continue;
        };
        let mut labels = without_name(sample.labels);
        labels.remove("le");
        groups.entry(labels).or_default().push((le, sample.value));
    }
    groups
        .into_iter()
        .map(|(labels, buckets)| Sample {
            labels,
            value: bucket_quantile(q, buckets),
        })
        .collect()
}

pub(super) fn bucket_quantile(q: f64, mut buckets: Vec<(f64, f64)>) -> f64 {
    if q.is_nan() {
        return f64::NAN;
    }
    if q < 0.0 {
        return f64::NEG_INFINITY;
    }
    if q > 1.0 {
        return f64::INFINITY;
    }
    buckets.sort_by(|a, b| a.0.total_cmp(&b.0));
    if buckets.len() < 2 || buckets.last().is_none_or(|(le, _)| *le != f64::INFINITY) {
        return f64::NAN;
    }
    // Counts must not fall as `le` rises; a scrape race can make them.
    for i in 1..buckets.len() {
        if buckets[i].1 < buckets[i - 1].1 {
            buckets[i].1 = buckets[i - 1].1;
        }
    }
    let observations = buckets[buckets.len() - 1].1;
    if observations == 0.0 {
        return f64::NAN;
    }
    let mut rank = q * observations;
    let b = buckets
        .iter()
        .position(|(_, count)| *count >= rank)
        .unwrap_or(buckets.len() - 1);
    if b == buckets.len() - 1 {
        return buckets[buckets.len() - 2].0;
    }
    if b == 0 && buckets[0].0 <= 0.0 {
        return buckets[0].0;
    }
    let (mut start, end, mut count) = (0.0, buckets[b].0, buckets[b].1);
    if b > 0 {
        start = buckets[b - 1].0;
        count -= buckets[b - 1].1;
        rank -= buckets[b - 1].1;
    }
    start + (end - start) * (rank / count)
}

pub(super) fn parse_float(text: &str) -> Option<f64> {
    match text {
        "+Inf" | "Inf" | "inf" => Some(f64::INFINITY),
        "-Inf" | "-inf" => Some(f64::NEG_INFINITY),
        "NaN" => Some(f64::NAN),
        other => other.parse().ok(),
    }
}

/// The labels `absent` gives its one sample: the equality matchers of the
/// selector it was handed, if it was handed one.
fn absent_labels(expr: &Expr) -> Labels {
    let selector = match expr {
        Expr::VectorSelector(selector) => selector,
        Expr::MatrixSelector(matrix) => &matrix.vs,
        _ => return Labels::new(),
    };
    let mut labels = Labels::new();
    for matcher in &selector.matchers.matchers {
        if matcher.name != NAME && matches!(matcher.op, promql_parser::label::MatchOp::Equal) {
            labels.insert(matcher.name.clone(), matcher.value.clone());
        }
    }
    labels
}

pub(super) fn label_replace(
    samples: Vec<Sample>,
    destination: &str,
    replacement: &str,
    source: &str,
    regex: &regex::Regex,
) -> Vec<Sample> {
    samples
        .into_iter()
        .map(|mut sample| {
            let value = sample.labels.get(source).cloned().unwrap_or_default();
            if let Some(captures) = regex.captures(&value) {
                let mut replaced = String::new();
                captures.expand(replacement, &mut replaced);
                if replaced.is_empty() {
                    sample.labels.remove(destination);
                } else {
                    sample.labels.insert(destination.to_string(), replaced);
                }
            }
            sample
        })
        .collect()
}

pub(super) fn label_join(
    samples: Vec<Sample>,
    destination: &str,
    separator: &str,
    sources: &[String],
) -> Vec<Sample> {
    samples
        .into_iter()
        .map(|mut sample| {
            let joined = sources
                .iter()
                .map(|source| sample.labels.get(source).map(String::as_str).unwrap_or(""))
                .collect::<Vec<_>>()
                .join(separator);
            if joined.is_empty() {
                sample.labels.remove(destination);
            } else {
                sample.labels.insert(destination.to_string(), joined);
            }
            sample
        })
        .collect()
}

/// `lhs op rhs`. A comparison answers `Some(lhs)` when it holds and `None`
/// when it does not; arithmetic always answers `Some`.
fn apply(op: &str, lhs: f64, rhs: f64) -> Result<Option<f64>> {
    let holds = |condition: bool| if condition { Some(lhs) } else { None };
    Ok(match op {
        "+" => Some(lhs + rhs),
        "-" => Some(lhs - rhs),
        "*" => Some(lhs * rhs),
        "/" => Some(lhs / rhs),
        "%" => Some(lhs % rhs),
        "^" => Some(lhs.powf(rhs)),
        "==" => holds(lhs == rhs),
        "!=" => holds(lhs != rhs),
        ">" => holds(lhs > rhs),
        "<" => holds(lhs < rhs),
        ">=" => holds(lhs >= rhs),
        "<=" => holds(lhs <= rhs),
        other => return fail(format!("unsupported operator {other}")),
    })
}

fn vector_scalar(
    op: &str,
    samples: Vec<Sample>,
    scalar: f64,
    scalar_first: bool,
    comparison: bool,
    return_bool: bool,
) -> Result<Vec<Sample>> {
    let mut out = Vec::new();
    for sample in samples {
        let (l, r) = if scalar_first {
            (scalar, sample.value)
        } else {
            (sample.value, scalar)
        };
        let result = apply(op, l, r)?;
        let value = match (comparison, return_bool, result) {
            (true, true, held) => held.map_or(0.0, |_| 1.0),
            // A filter keeps the vector side's value.
            (true, false, Some(_)) => sample.value,
            (true, false, None) => continue,
            (false, _, value) => value.unwrap_or(f64::NAN),
        };
        let labels = if comparison && !return_bool {
            sample.labels
        } else {
            without_name(sample.labels)
        };
        out.push(Sample { labels, value });
    }
    Ok(out)
}

/// The labels two sides are matched on: those named by `on`, or all but
/// those named by `ignoring` and the name.
fn signature(labels: &Labels, matching: Option<&LabelModifier>) -> Labels {
    match matching {
        Some(LabelModifier::Include(on)) => labels
            .iter()
            .filter(|(key, _)| on.labels.contains(key))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
        Some(LabelModifier::Exclude(ignoring)) => labels
            .iter()
            .filter(|(key, _)| *key != NAME && !ignoring.labels.contains(key))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
        None => without_name(labels.clone()),
    }
}

fn set_operation(
    op: &str,
    lhs: Vec<Sample>,
    rhs: Vec<Sample>,
    matching: Option<&LabelModifier>,
) -> Vec<Sample> {
    let right: BTreeSet<Labels> = rhs.iter().map(|s| signature(&s.labels, matching)).collect();
    match op {
        "and" => lhs
            .into_iter()
            .filter(|s| right.contains(&signature(&s.labels, matching)))
            .collect(),
        "unless" => lhs
            .into_iter()
            .filter(|s| !right.contains(&signature(&s.labels, matching)))
            .collect(),
        _ => {
            let left: BTreeSet<Labels> =
                lhs.iter().map(|s| signature(&s.labels, matching)).collect();
            let mut out = lhs;
            out.extend(
                rhs.into_iter()
                    .filter(|s| !left.contains(&signature(&s.labels, matching))),
            );
            out
        }
    }
}

fn vector_vector(
    op: &str,
    lhs: Vec<Sample>,
    rhs: Vec<Sample>,
    card: &VectorMatchCardinality,
    matching: Option<&LabelModifier>,
    comparison: bool,
    return_bool: bool,
) -> Result<Vec<Sample>> {
    // The "many" side is walked; the "one" side is looked up by signature.
    let (many, one, swapped, include) = match card {
        VectorMatchCardinality::OneToMany(include) => (rhs, lhs, true, Some(include)),
        VectorMatchCardinality::ManyToOne(include) => (lhs, rhs, false, Some(include)),
        _ => (lhs, rhs, false, None),
    };
    let mut lookup: HashMap<Labels, Sample> = HashMap::new();
    for sample in one {
        let key = signature(&sample.labels, matching);
        if lookup.insert(key, sample).is_some() {
            return fail(
                "found duplicate series for the match group on the side of one; \
                 many-to-many matching is not allowed",
            );
        }
    }
    let drop_name = !comparison || return_bool;
    let mut seen = BTreeSet::new();
    let mut results = BTreeSet::new();
    let mut out = Vec::new();
    for sample in many {
        let key = signature(&sample.labels, matching);
        let Some(other) = lookup.get(&key) else {
            continue;
        };
        if include.is_none() && !seen.insert(key.clone()) {
            return fail(
                "multiple matches for labels: many-to-one matching must be explicit \
                 (group_left/group_right)",
            );
        }
        let (l, r) = if swapped {
            (other.value, sample.value)
        } else {
            (sample.value, other.value)
        };
        let result = apply(op, l, r)?;
        let value = match (comparison, return_bool, result) {
            (true, true, held) => held.map_or(0.0, |_| 1.0),
            (true, false, Some(value)) => value,
            (true, false, None) => continue,
            (false, _, value) => value.unwrap_or(f64::NAN),
        };
        let mut labels = sample.labels.clone();
        if drop_name {
            labels.remove(NAME);
        }
        match include {
            None => labels = one_to_one_labels(labels, matching),
            Some(include) => {
                for name in &include.labels {
                    match other.labels.get(name) {
                        Some(value) if !value.is_empty() => {
                            labels.insert(name.clone(), value.clone());
                        }
                        _ => {
                            labels.remove(name);
                        }
                    }
                }
            }
        }
        // Two pairs that land on one label set would be one series with two
        // values; Prometheus refuses that rather than pick one.
        if !results.insert(labels.clone()) {
            return fail("multiple matches for labels: grouping labels must ensure unique matches");
        }
        out.push(Sample { labels, value });
    }
    Ok(out)
}

fn one_to_one_labels(labels: Labels, matching: Option<&LabelModifier>) -> Labels {
    match matching {
        Some(LabelModifier::Include(on)) => labels
            .into_iter()
            .filter(|(key, _)| on.labels.contains(key))
            .collect(),
        Some(LabelModifier::Exclude(ignoring)) => labels
            .into_iter()
            .filter(|(key, _)| !ignoring.labels.contains(key))
            .collect(),
        None => labels,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: i64 = 1_000_000_000_000;

    fn labels(pairs: &[(&str, &str)]) -> Labels {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn series(pairs: &[(&str, &str)], points: &[(i64, f64)]) -> Series {
        Series::new(labels(pairs), points.to_vec(), Vec::new())
    }

    fn eval(data: &[Series], query: &str) -> Result<Value> {
        Evaluator::new(data, T, T).eval(&parse(query)?, T)
    }

    fn vector(data: &[Series], query: &str) -> Vec<(Labels, f64)> {
        match eval(data, query).unwrap() {
            Value::Vector(samples) => samples.into_iter().map(|s| (s.labels, s.value)).collect(),
            other => panic!("{query}: {other:?}"),
        }
    }

    fn tokens() -> Vec<Series> {
        vec![
            series(
                &[
                    (NAME, "ariadne_tokens_total"),
                    ("seat", "author"),
                    ("job", "ariadned"),
                ],
                &[(T - 120_000, 10.0), (T - 60_000, 20.0), (T, 30.0)],
            ),
            series(
                &[
                    (NAME, "ariadne_tokens_total"),
                    ("seat", "reviewer"),
                    ("job", "ariadned"),
                ],
                &[(T - 120_000, 1.0), (T - 60_000, 2.0), (T, 3.0)],
            ),
        ]
    }

    #[test]
    fn counters_gain_total_and_units_precede_histogram_suffixes() {
        assert_eq!(
            prometheus_name("ariadne.tokens", MetricKind::Counter, Some("{token}"), None),
            "ariadne_tokens_total"
        );
        assert_eq!(
            prometheus_name(
                "ariadne.task.lead_time_bucket",
                MetricKind::Counter,
                Some("s"),
                Some("_bucket")
            ),
            "ariadne_task_lead_time_seconds_bucket"
        );
        assert_eq!(
            prometheus_name("db.size", MetricKind::Gauge, Some("By"), None),
            "db_size_bytes"
        );
    }

    #[test]
    fn translation_marks_histogram_components_and_adds_service_and_job() {
        let meta = |name: &str, labels: serde_json::Value| SeriesMeta {
            id: name.into(),
            name: name.into(),
            kind: MetricKind::Counter,
            unit: Some("s".into()),
            description: None,
            labels,
        };
        let entries = translate(vec![
            meta(
                "ariadne.task.lead_time_bucket",
                serde_json::json!({"le": "1", "repo.name": "x"}),
            ),
            meta("ariadne.task.lead_time_count", serde_json::json!({})),
            meta("other_count", serde_json::json!({})),
        ]);
        assert_eq!(
            entries[0].labels[NAME],
            "ariadne_task_lead_time_seconds_bucket"
        );
        assert_eq!(entries[0].labels["repo_name"], "x");
        assert_eq!(entries[0].labels["service"], "ariadned");
        assert_eq!(entries[0].labels["job"], "ariadned");
        assert_eq!(
            entries[1].labels[NAME],
            "ariadne_task_lead_time_seconds_count"
        );
        assert_eq!(entries[1].kind, "histogram");
        assert_eq!(entries[1].family, "ariadne_task_lead_time_seconds");
        assert_eq!(entries[2].labels[NAME], "other_count_seconds_total");
    }

    #[test]
    fn service_and_job_come_from_the_resource_alone() {
        let entries = translate(vec![SeriesMeta {
            id: "1".into(),
            name: "ariadne.telemetry.records".into(),
            kind: MetricKind::Counter,
            unit: None,
            description: None,
            labels: serde_json::json!({
                "__resource_service_name__": "worker",
                "service": "ariadne-cli",
                "job": "spoofed",
                "exported_job": "older",
            }),
        }]);
        let labels = &entries[0].labels;
        assert_eq!(labels["service"], "worker");
        assert_eq!(labels["job"], "worker");
        assert_eq!(labels["exported_service"], "ariadne-cli");
        assert_eq!(labels["exported_job"], "older");
        assert_eq!(labels["exported_exported_job"], "spoofed");
        assert!(!labels.contains_key(RESOURCE_SERVICE_LABEL));
    }

    #[test]
    fn selectors_apply_all_four_matchers_and_the_lookback() {
        let data = tokens();
        assert_eq!(
            vector(&data, r#"ariadne_tokens_total{seat="author"}"#).len(),
            1
        );
        assert_eq!(
            vector(&data, r#"ariadne_tokens_total{seat!="author"}"#).len(),
            1
        );
        assert_eq!(
            vector(&data, r#"ariadne_tokens_total{seat=~"auth.*"}"#).len(),
            1
        );
        assert_eq!(
            vector(&data, r#"ariadne_tokens_total{seat!~"auth"}"#).len(),
            2
        );
        let stale = [series(&[(NAME, "x")], &[(T - LOOKBACK_MS, 1.0)])];
        assert!(vector(&stale, "x").is_empty(), "a sample 5m old is stale");
    }

    #[test]
    fn offset_reads_an_earlier_sample() {
        let data = tokens();
        let author = r#"ariadne_tokens_total{seat="author"} offset 1m"#;
        assert_eq!(vector(&data, author)[0].1, 20.0);
    }

    #[test]
    fn at_pins_a_selector_to_the_query_start_end_or_a_time() {
        let data = tokens();
        // A range query over the three samples, evaluated at its first step.
        let evaluator = Evaluator::new(&data, T - 120_000, T);
        let first = |query: &str| match evaluator.eval(&parse(query).unwrap(), T - 120_000) {
            Ok(Value::Vector(samples)) => samples[0].value,
            other => panic!("{query}: {other:?}"),
        };
        let author = r#"ariadne_tokens_total{seat="author"}"#;
        assert_eq!(first(author), 10.0);
        assert_eq!(first(&format!("{author} @ end()")), 30.0);
        assert_eq!(first(&format!("{author} @ start()")), 10.0);
        let at_middle = (T - 60_000) / 1000;
        assert_eq!(first(&format!("{author} @ {at_middle}")), 20.0);
        assert_eq!(first(&format!("{author} @ end() offset 1m")), 20.0);
        assert_eq!(
            first(&format!("increase({author}[2m] @ end())")),
            20.0,
            "a window pinned to the end reads the whole range at every step"
        );
    }

    #[test]
    fn a_series_selector_refuses_at() {
        assert!(parse_selector("x @ end()").is_err());
        assert!(parse_selector("x").is_ok());
    }

    #[test]
    fn rate_and_increase_extrapolate_like_prometheus() {
        let data = tokens();
        let rate = vector(&data, r#"rate(ariadne_tokens_total{seat="author"}[2m])"#);
        // 20 over 120s of samples in a 120s window: the start extrapolates by
        // the duration to zero (120s * 10/20 = 60s is beyond the 0s gap).
        assert!((rate[0].1 - 20.0 / 120.0).abs() < 1e-9, "{rate:?}");
        assert!(!rate[0].0.contains_key(NAME));
        let resets = [series(
            &[(NAME, "c")],
            &[(T - 60_000, 5.0), (T - 30_000, 8.0), (T, 2.0)],
        )];
        // 5 → 8 → 2 is a rise of 5 with the reset added back; the start is
        // 60s from the window's edge, past 1.1 average gaps, so it extends
        // by half a gap: 5 * 75s / 60s.
        assert_eq!(vector(&resets, "increase(c[2m])")[0].1, 6.25);
        assert_eq!(vector(&resets, "irate(c[2m])")[0].1, 2.0 / 30.0);
        assert_eq!(vector(&resets, "idelta(c[2m])")[0].1, -6.0);
        assert_eq!(vector(&resets, "changes(c[2m])")[0].1, 2.0);
    }

    #[test]
    fn over_time_functions_reduce_each_window() {
        let data = [series(
            &[(NAME, "g")],
            &[(T - 60_000, 1.0), (T - 30_000, 4.0), (T, 7.0)],
        )];
        let one = |q: &str| vector(&data, q)[0].1;
        assert_eq!(one("avg_over_time(g[2m])"), 4.0);
        assert_eq!(one("sum_over_time(g[2m])"), 12.0);
        assert_eq!(one("min_over_time(g[2m])"), 1.0);
        assert_eq!(one("max_over_time(g[2m])"), 7.0);
        assert_eq!(one("count_over_time(g[2m])"), 3.0);
        assert_eq!(one("last_over_time(g[2m])"), 7.0);
        assert_eq!(one("quantile_over_time(0.5, g[2m])"), 4.0);
    }

    #[test]
    fn rollups_answer_beyond_the_raw_retention() {
        let hour = (T - 10 * HOUR_MS).div_euclid(HOUR_MS) * HOUR_MS;
        let rollup = Rollup {
            start: hour,
            min: 2.0,
            max: 9.0,
            sum: 30.0,
            count: 6.0,
            first: 3.0,
            last: 8.0,
        };
        let data = [Series::new(
            labels(&[(NAME, "g")]),
            Vec::new(),
            vec![rollup],
        )];
        let at = |query: &str, t: i64| match Evaluator::new(&data, t, t)
            .eval(&parse(query).unwrap(), t)
        {
            Ok(Value::Vector(samples)) => samples.first().map(|s| s.value),
            other => panic!("{other:?}"),
        };
        assert_eq!(
            at("g", hour + 30 * 60_000),
            Some(8.0),
            "the hour reads its last"
        );
        assert_eq!(at("min_over_time(g[1h])", hour + HOUR_MS), Some(2.0));
        assert_eq!(at("max_over_time(g[1h])", hour + HOUR_MS), Some(9.0));
        assert_eq!(at("avg_over_time(g[1h])", hour + HOUR_MS), Some(5.0));
        assert_eq!(at("count_over_time(g[1h])", hour + HOUR_MS), Some(6.0));
        let raw = Series::new(labels(&[]), vec![(hour + 1, 1.0)], vec![rollup]);
        assert!(raw.rollups.is_empty(), "a raw hour ignores its rollup");
    }

    #[test]
    fn aggregations_group_by_and_without() {
        let data = tokens();
        assert_eq!(
            vector(&data, "sum(ariadne_tokens_total)"),
            vec![(labels(&[]), 33.0)]
        );
        assert_eq!(
            vector(&data, "sum by (seat) (ariadne_tokens_total)"),
            vec![
                (labels(&[("seat", "author")]), 30.0),
                (labels(&[("seat", "reviewer")]), 3.0)
            ]
        );
        assert_eq!(
            vector(&data, "max without (seat) (ariadne_tokens_total)"),
            vec![(labels(&[("job", "ariadned")]), 30.0)]
        );
        let one = |q: &str| vector(&data, q)[0].1;
        assert_eq!(one("avg(ariadne_tokens_total)"), 16.5);
        assert_eq!(one("min(ariadne_tokens_total)"), 3.0);
        assert_eq!(one("count(ariadne_tokens_total)"), 2.0);
        assert_eq!(one("quantile(0.5, ariadne_tokens_total)"), 16.5);
        let top = vector(&data, "topk(1, ariadne_tokens_total)");
        assert_eq!((top.len(), top[0].0["seat"].as_str()), (1, "author"));
        let bottom = vector(&data, "bottomk(1, ariadne_tokens_total)");
        assert_eq!(bottom[0].0["seat"], "reviewer");
    }

    #[test]
    fn histogram_quantile_interpolates_within_a_bucket() {
        let bucket = |le: &str, v: f64| series(&[(NAME, "h_bucket"), ("le", le)], &[(T, v)]);
        let data = [bucket("1", 2.0), bucket("2", 6.0), bucket("+Inf", 8.0)];
        let p50 = vector(&data, "histogram_quantile(0.5, h_bucket)");
        assert_eq!(p50, vec![(labels(&[]), 1.5)]);
        assert_eq!(
            vector(&data, "histogram_quantile(0.99, h_bucket)")[0].1,
            2.0
        );
    }

    #[test]
    fn arithmetic_and_comparisons_with_scalars() {
        let data = tokens();
        let doubled = vector(&data, r#"ariadne_tokens_total{seat="author"} * 2"#);
        assert_eq!(
            doubled,
            vec![(labels(&[("seat", "author"), ("job", "ariadned")]), 60.0)]
        );
        let filtered = vector(&data, "ariadne_tokens_total > 10");
        assert_eq!(filtered.len(), 1);
        assert_eq!(
            filtered[0].0[NAME], "ariadne_tokens_total",
            "a filter keeps the name"
        );
        let flags = vector(&data, "ariadne_tokens_total > bool 10");
        assert_eq!(
            flags.iter().map(|s| s.1).collect::<Vec<_>>(),
            vec![1.0, 0.0]
        );
        assert_eq!(eval(&data, "2 ^ 3 - 1").unwrap(), Value::Scalar(7.0));
        assert_eq!(eval(&data, "1 < bool 2").unwrap(), Value::Scalar(1.0));
        assert_eq!(vector(&data, "-ariadne_tokens_total")[0].1, -30.0);
    }

    #[test]
    fn vector_matching_with_on_ignoring_and_groups() {
        let data = vec![
            series(&[(NAME, "a"), ("seat", "author"), ("x", "1")], &[(T, 6.0)]),
            series(
                &[(NAME, "a"), ("seat", "reviewer"), ("x", "1")],
                &[(T, 4.0)],
            ),
            series(&[(NAME, "b"), ("x", "1"), ("team", "core")], &[(T, 2.0)]),
        ];
        assert_eq!(
            vector(&data, "a / on (x) group_left (team) b"),
            vec![
                (
                    labels(&[("seat", "author"), ("x", "1"), ("team", "core")]),
                    3.0
                ),
                (
                    labels(&[("seat", "reviewer"), ("x", "1"), ("team", "core")]),
                    2.0
                )
            ]
        );
        assert_eq!(
            vector(&data, "b - on (x) group_right a"),
            vec![
                (labels(&[("seat", "author"), ("x", "1")]), -4.0),
                (labels(&[("seat", "reviewer"), ("x", "1")]), -2.0)
            ]
        );
        let one = vector(&data, r#"a{seat="author"} + ignoring (seat, team) b"#);
        assert_eq!(one, vec![(labels(&[("x", "1")]), 8.0)]);
        let error = eval(&data, "a + on (x) b").unwrap_err();
        assert!(error.0.contains("group_left"), "{error:?}");
        let compared = vector(&data, "a > on (x) group_left b");
        assert_eq!(compared.len(), 2);
    }

    #[test]
    fn a_grouped_match_that_collides_on_one_label_set_refuses() {
        let data = vec![
            series(&[(NAME, "a"), ("x", "1"), ("team", "a")], &[(T, 1.0)]),
            series(&[(NAME, "a"), ("x", "1"), ("team", "b")], &[(T, 2.0)]),
            series(&[(NAME, "b"), ("x", "1"), ("team", "core")], &[(T, 3.0)]),
        ];
        for query in [
            "a + on (x) group_left (team) b",
            "b + on (x) group_right (team) a",
        ] {
            let error = eval(&data, query).unwrap_err();
            assert!(error.0.contains("unique matches"), "{query}: {error:?}");
        }
        // Without the copied label the two results differ, so it answers.
        assert_eq!(vector(&data, "a + on (x) group_left b").len(), 2);
        assert_eq!(vector(&data, "b + on (x) group_right a").len(), 2);
    }

    #[test]
    fn clamp_keeps_nan_in_values_and_bounds() {
        let data = [series(&[(NAME, "g")], &[(T, 1.0)])];
        let one = |q: &str| vector(&data, q)[0].1;
        assert!(one("clamp_min(vector(0/0), 0)").is_nan());
        assert!(one("clamp_max(vector(0/0), 0)").is_nan());
        assert!(one("clamp(vector(0/0), 0, 2)").is_nan());
        assert!(one("clamp(g, 0/0, 2)").is_nan());
        assert!(one("clamp(g, 0, 0/0)").is_nan());
        assert!(one("clamp_min(g, 0/0)").is_nan());
        assert!(one("clamp_max(g, 0/0)").is_nan());
    }

    #[test]
    fn set_operators_match_on_labels() {
        let data = vec![
            series(&[(NAME, "a"), ("seat", "author")], &[(T, 1.0)]),
            series(&[(NAME, "a"), ("seat", "reviewer")], &[(T, 2.0)]),
            series(&[(NAME, "b"), ("seat", "author")], &[(T, 3.0)]),
            series(&[(NAME, "b"), ("seat", "keeper")], &[(T, 4.0)]),
        ];
        let seats = |q: &str| {
            vector(&data, q)
                .into_iter()
                .map(|(l, _)| l["seat"].clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(seats("a and b"), vec!["author"]);
        assert_eq!(seats("a unless b"), vec!["reviewer"]);
        assert_eq!(seats("a or b"), vec!["author", "reviewer", "keeper"]);
    }

    #[test]
    fn instant_functions_transform_each_sample() {
        let data = [series(&[(NAME, "g"), ("svc", "api-v2")], &[(T, -2.26)])];
        let one = |q: &str| vector(&data, q)[0].1;
        assert_eq!(one("abs(g)"), 2.26);
        assert_eq!(one("ceil(g)"), -2.0);
        assert_eq!(one("floor(g)"), -3.0);
        assert_eq!(one("round(g)"), -2.0);
        assert_eq!(one("round(g, 0.1)"), -2.3);
        assert_eq!(one("clamp(g, -1, 1)"), -1.0);
        assert_eq!(one("clamp_min(g, 0)"), 0.0);
        assert_eq!(one("clamp_max(g, -5)"), -5.0);
        let replaced = vector(
            &data,
            r#"label_replace(g, "base", "$1", "svc", "(.*)-v[0-9]+")"#,
        );
        assert_eq!(replaced[0].0["base"], "api");
        let unmatched = vector(&data, r#"label_replace(g, "base", "$1", "svc", "nope")"#);
        assert!(!unmatched[0].0.contains_key("base"));
        let joined = vector(&data, r#"label_join(g, "id", "/", "__name__", "svc")"#);
        assert_eq!(joined[0].0["id"], "g/api-v2");
    }

    #[test]
    fn absent_time_vector_scalar_and_sort() {
        let data = tokens();
        assert_eq!(
            vector(&data, r#"absent(nothing{seat="x", job=~"a"})"#),
            vec![(labels(&[("seat", "x")]), 1.0)]
        );
        assert!(vector(&data, "absent(ariadne_tokens_total)").is_empty());
        assert_eq!(
            eval(&data, "time()").unwrap(),
            Value::Scalar(T as f64 / 1000.0)
        );
        assert_eq!(vector(&data, "vector(2)"), vec![(labels(&[]), 2.0)]);
        assert_eq!(
            eval(&data, r#"scalar(ariadne_tokens_total{seat="author"})"#).unwrap(),
            Value::Scalar(30.0)
        );
        let sorted = vector(&data, "sort(ariadne_tokens_total)");
        assert_eq!(sorted[0].1, 3.0);
        let sorted = vector(&data, "sort_desc(ariadne_tokens_total)");
        assert_eq!(sorted[0].1, 30.0);
    }

    #[test]
    fn unsupported_functions_and_operators_are_named() {
        for (query, named) in [
            ("stddev(x)", "stddev"),
            ("predict_linear(x[5m], 1)", "predict_linear"),
            ("x atan2 y", "atan2"),
            ("max_over_time(x[5m:1m])", "subquery"),
            ("count_values(\"v\", x)", "count_values"),
        ] {
            let error = parse(query).unwrap_err();
            assert!(error.0.contains(named), "{query}: {error:?}");
        }
    }
}
