//! `ariadne query`: one PromQL expression, evaluated by the daemon's
//! Prometheus API (034) and printed as a table.
//!
//! An instant query runs at now, or at `--at`. `--since` runs a range query
//! from then to now, one point per `--step`. The table has one column per
//! label of the result, then `VALUE`; a range adds `TIME` first and prints one
//! row per point. `--format json` prints the daemon's answer whole.

use std::collections::BTreeSet;

use anyhow::{Result, bail};
use ariadne_api::hosts::{HostDto, HostStatus};
use ariadne_client::{Client, ClientError};
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use serde_json::Value;

use crate::error::Failure;
use crate::output::{self, Column, Format, UNCAPPED, col};

pub(crate) async fn run(
    client: &Client,
    expr: String,
    at: Option<String>,
    since: Option<String>,
    step: Option<String>,
    host: Option<&str>,
    format: Format,
) -> Result<()> {
    if let Some(name) = host.filter(|name| !matches!(*name, "local" | "all")) {
        let hosts: Vec<HostDto> = client.get_json("/v1/hosts").await?;
        check_host(name, &hosts)?;
    }
    let path = on_host(
        host,
        &path(
            &expr,
            at.as_deref(),
            since.as_deref(),
            step.as_deref(),
            Utc::now(),
        )?,
    );
    let answer: Value = match client.get_json(&path).await {
        Ok(answer) => answer,
        Err(ClientError::Api { message, .. }) if prometheus_error(&message).is_some() => {
            bail!(prometheus_error(&message).unwrap_or_default())
        }
        Err(error) => return Err(error.into()),
    };
    if format == Format::Json {
        return output::print_json(&answer);
    }
    let (columns, rows) = table(&answer["data"], host == Some("all"));
    let (text, notes) = output::list_text(&columns, &rows, output::view(), "no series")?;
    if !text.is_empty() {
        outln!("{text}");
    }
    for message in notes {
        output::note(&message);
    }
    // `--host all` names the hosts that did not answer (037).
    for warning in answer["warnings"].as_array().into_iter().flatten() {
        output::note(&format!(
            "warning: {}",
            warning.as_str().unwrap_or_default()
        ));
    }
    Ok(())
}

/// A usage error, as the other commands give, for a host that is not in the
/// registry or not ready.
fn check_host(name: &str, hosts: &[HostDto]) -> Result<()> {
    let names = hosts
        .iter()
        .map(|host| format!("{} ({})", host.name, status(host.status)))
        .collect::<Vec<_>>()
        .join(", ");
    match hosts.iter().find(|host| host.name == name) {
        None => Err(Failure::usage(format!("unknown host {name}; hosts: {names}")).err()),
        Some(host) if host.status != HostStatus::Ready => Err(Failure::usage(format!(
            "host {name} is {}; hosts: {names}",
            status(host.status)
        ))
        .err()),
        Some(_) => Ok(()),
    }
}

fn status(status: HostStatus) -> String {
    format!("{status:?}").to_lowercase()
}

/// The route as the local daemon serves it for `host`: under
/// `/v1/hosts/{host}` (037), or as it is without `--host`.
fn on_host(host: Option<&str>, path: &str) -> String {
    match host {
        Some(host) => format!("/v1/hosts/{}{path}", crate::commands::path_segment(host)),
        None => path.to_string(),
    }
}

/// The route and parameters the flags ask for, relative to `now`.
fn path(
    expr: &str,
    at: Option<&str>,
    since: Option<&str>,
    step: Option<&str>,
    now: DateTime<Utc>,
) -> Result<String> {
    let mut pairs = vec![("query", expr.to_string())];
    let route = match since {
        None => {
            if let Some(at) = at {
                pairs.push(("time", rfc3339(moment(at, now)?)));
            }
            "/api/v1/query"
        }
        Some(since) => {
            let start = moment(since, now)?;
            if start >= now {
                bail!("--since {since:?} is not before now");
            }
            let step = match step {
                Some(step) => span(step)?,
                None => default_step(now - start),
            };
            if step <= Duration::zero() {
                bail!("--step must be longer than zero");
            }
            pairs.push(("start", rfc3339(start)));
            pairs.push(("end", rfc3339(now)));
            pairs.push((
                "step",
                format!("{}", step.num_milliseconds() as f64 / 1000.0),
            ));
            "/api/v1/query_range"
        }
    };
    Ok(format!("{route}?{}", serde_urlencoded::to_string(pairs)?))
}

/// The span over 250, rounded up to a whole second.
fn default_step(span: Duration) -> Duration {
    let millis = span.num_milliseconds().max(1);
    Duration::seconds((millis + 249_999) / 250_000)
}

fn rfc3339(time: DateTime<Utc>) -> String {
    time.to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// An RFC 3339 time, or a span before `now`.
fn moment(value: &str, now: DateTime<Utc>) -> Result<DateTime<Utc>> {
    if let Ok(time) = DateTime::parse_from_rfc3339(value) {
        return Ok(time.with_timezone(&Utc));
    }
    Ok(now - span(value)?)
}

/// A span such as `30s`, `15m`, `1h30m`, `7d` or `2w`.
fn span(value: &str) -> Result<Duration> {
    let invalid = || anyhow::anyhow!("invalid span {value:?}: use 30s, 15m, 1h30m, 7d or 2w");
    let mut rest = value.trim();
    if rest.is_empty() {
        return Err(invalid());
    }
    let mut total = 0.0;
    while !rest.is_empty() {
        let digits = rest
            .find(|c: char| !c.is_ascii_digit() && c != '.')
            .ok_or_else(invalid)?;
        let number: f64 = rest[..digits].parse().map_err(|_| invalid())?;
        rest = &rest[digits..];
        let unit = rest
            .find(|c: char| c.is_ascii_digit())
            .unwrap_or(rest.len());
        total += number
            * match &rest[..unit] {
                "ms" => 0.001,
                "s" => 1.0,
                "m" => 60.0,
                "h" => 3600.0,
                "d" => 86400.0,
                "w" => 604800.0,
                _ => return Err(invalid()),
            };
        rest = &rest[unit..];
    }
    Ok(Duration::milliseconds((total * 1000.0).ceil() as i64))
}

/// The `error` sentence of a Prometheus error envelope, if `body` is one.
fn prometheus_error(body: &str) -> Option<String> {
    let body: Value = serde_json::from_str(body).ok()?;
    if body["status"] != "error" {
        return None;
    }
    body["error"].as_str().map(str::to_string)
}

/// The columns and rows of a query's `data`. With `hosts` (`--host all`) the
/// `host` label is the first column, headed `HOST`.
fn table(data: &Value, hosts: bool) -> (Vec<Column>, Vec<Vec<String>>) {
    let result = &data["result"];
    let range = data["resultType"] == "matrix";
    if data["resultType"] == "scalar" || data["resultType"] == "string" {
        let value = result[1].as_str().unwrap_or_default().to_string();
        return (vec![col("VALUE", UNCAPPED)], vec![vec![value]]);
    }
    let series = result.as_array().cloned().unwrap_or_default();
    let mut labels: BTreeSet<String> = series
        .iter()
        .filter_map(|s| s["metric"].as_object())
        .flat_map(|metric| metric.keys().cloned())
        .collect();
    let host = hosts && labels.remove("host");
    let mut columns = Vec::new();
    if host {
        columns.push(col("HOST", 40).rank(1));
    }
    if range {
        columns.push(col("TIME", UNCAPPED));
    }
    // A table's headers are static; a query's are only known now, and the
    // process prints one table and exits.
    columns.extend(
        labels
            .iter()
            .map(|label| col(String::leak(label.clone()), 40).rank(1)),
    );
    columns.push(col("VALUE", UNCAPPED));
    let mut rows: Vec<(f64, Vec<String>)> = Vec::new();
    for s in &series {
        let cells: Vec<String> = labels
            .iter()
            .map(|label| s["metric"][label].as_str().unwrap_or("").to_string())
            .collect();
        let host_cell = s["metric"]["host"].as_str().unwrap_or("").to_string();
        let points = if range {
            s["values"].as_array().cloned().unwrap_or_default()
        } else {
            vec![s["value"].clone()]
        };
        for point in points {
            let t = point[0].as_f64().unwrap_or_default();
            let mut row = Vec::with_capacity(columns.len());
            if host {
                row.push(host_cell.clone());
            }
            if range {
                row.push(time_cell(t));
            }
            row.extend(cells.iter().cloned());
            row.push(point[1].as_str().unwrap_or_default().to_string());
            rows.push((t, row));
        }
    }
    // One row per time: a range reads down the clock, its series side by side.
    rows.sort_by(|a, b| a.0.total_cmp(&b.0));
    (columns, rows.into_iter().map(|(_, row)| row).collect())
}

fn time_cell(seconds: f64) -> String {
    DateTime::from_timestamp_millis((seconds * 1000.0).round() as i64)
        .map(|time| output::local_time(&rfc3339(time)))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::{View, render_table};
    use serde_json::json;

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-01-01T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    fn params(path: &str) -> Vec<(String, String)> {
        let query = path.split_once('?').unwrap().1;
        serde_urlencoded::from_str(query).unwrap()
    }

    #[test]
    fn parses_cli_spans_for_query_boundaries() {
        assert_eq!(span("1.5h").unwrap(), Duration::minutes(90));
        assert_eq!(span("1h30m").unwrap(), Duration::minutes(90));
        assert!(span("5x").is_err());
    }

    #[test]
    fn an_instant_query_runs_at_now_or_at_the_given_moment() {
        let path = path("up", None, None, None, now()).unwrap();
        assert!(path.starts_with("/api/v1/query?"));
        assert_eq!(params(&path), vec![("query".into(), "up".into())]);
        let moved = path_at("1h");
        assert!(moved.contains(&("time".into(), "2026-01-01T09:00:00.000Z".into())));
        let fixed = path_at("2026-01-01T08:00:00Z");
        assert!(fixed.contains(&("time".into(), "2026-01-01T08:00:00.000Z".into())));
    }

    fn path_at(at: &str) -> Vec<(String, String)> {
        params(&path("up", Some(at), None, None, now()).unwrap())
    }

    #[test]
    fn a_range_query_defaults_its_step_to_the_span_over_250() {
        let path = path("up", None, Some("1h"), None, now()).unwrap();
        assert!(path.starts_with("/api/v1/query_range?"));
        let params = params(&path);
        assert!(params.contains(&("start".into(), "2026-01-01T09:00:00.000Z".into())));
        assert!(params.contains(&("end".into(), "2026-01-01T10:00:00.000Z".into())));
        // 3600s / 250 = 14.4s, rounded up to 15s.
        assert!(params.contains(&("step".into(), "15".into())));
        let stepped = super::path("up", None, Some("1h"), Some("30s"), now()).unwrap();
        assert!(super::tests::params(&stepped).contains(&("step".into(), "30".into())));
    }

    #[test]
    fn a_host_moves_the_route_under_the_hosts_prefix() {
        assert_eq!(
            on_host(Some("all"), "/api/v1/query?query=up"),
            "/v1/hosts/all/api/v1/query?query=up"
        );
        assert_eq!(
            on_host(None, "/api/v1/query?query=up"),
            "/api/v1/query?query=up"
        );
    }

    #[test]
    fn a_daemon_error_reads_as_its_sentence() {
        let body =
            r#"{"status":"error","errorType":"bad_data","error":"unsupported function stddev"}"#;
        assert_eq!(
            prometheus_error(body).as_deref(),
            Some("unsupported function stddev")
        );
        assert_eq!(prometheus_error("not json"), None);
    }

    #[test]
    fn an_instant_table_has_a_column_per_label_then_value() {
        let data = json!({"resultType": "vector", "result": [
            {"metric": {"seat": "agent"}, "value": [1.0, "30"]},
            {"metric": {"seat": "reviewer", "job": "ariadned"}, "value": [1.0, "3"]},
        ]});
        let (columns, rows) = table(&data, false);
        let text = render_table(&columns, &rows, &View::plain()).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(
            lines[0].split_whitespace().collect::<Vec<_>>(),
            ["JOB", "SEAT", "VALUE"]
        );
        assert_eq!(rows[1], vec!["ariadned", "reviewer", "3"]);
    }

    #[test]
    fn a_range_table_has_one_row_per_time() {
        let data = json!({"resultType": "matrix", "result": [
            {"metric": {"seat": "agent"}, "values": [[1.0, "1"], [2.0, "2"]]},
            {"metric": {"seat": "reviewer"}, "values": [[1.0, "5"]]},
        ]});
        let (columns, rows) = table(&data, false);
        assert_eq!(
            columns.iter().map(|c| c.header).collect::<Vec<_>>(),
            ["TIME", "seat", "VALUE"]
        );
        let values: Vec<&str> = rows.iter().map(|row| row[2].as_str()).collect();
        assert_eq!(values, ["1", "5", "2"]);
    }

    #[test]
    fn host_all_puts_the_host_column_first() {
        let data = json!({"resultType": "matrix", "result": [
            {"metric": {"host": "build", "seat": "agent"}, "values": [[1.0, "1"]]},
        ]});
        let (columns, rows) = table(&data, true);
        assert_eq!(
            columns.iter().map(|c| c.header).collect::<Vec<_>>(),
            ["HOST", "TIME", "seat", "VALUE"]
        );
        assert_eq!(rows[0][0], "build");
        assert_eq!(rows[0][2], "agent");
    }

    #[test]
    fn a_host_that_is_unknown_or_not_ready_is_a_usage_error() {
        let host = |name: &str, status| {
            HostDto {
            color: None,
            name: name.into(),
            status,
            ..serde_json::from_value(json!({"name":"x","kind":"direct","status":"ready","endpoint":null,"error":null,"remote":null,"sync":{"state":"in_sync","error":null,"missing_repositories":[],"skipped_permissions":0}})).unwrap()
        }
        };
        let hosts = [
            host("local", HostStatus::Ready),
            host("off", HostStatus::Down),
        ];
        let unknown = check_host("nope", &hosts).unwrap_err().to_string();
        assert!(unknown.contains("local (ready), off (down)"), "{unknown}");
        let down = check_host("off", &hosts).unwrap_err().to_string();
        assert!(down.contains("is down"), "{down}");
        assert!(check_host("local", &hosts).is_ok());
    }

    #[test]
    fn a_scalar_is_one_value() {
        let (columns, rows) = table(
            &json!({"resultType": "scalar", "result": [1.0, "7"]}),
            false,
        );
        assert_eq!(columns.len(), 1);
        assert_eq!(rows, vec![vec!["7".to_string()]]);
    }
}
