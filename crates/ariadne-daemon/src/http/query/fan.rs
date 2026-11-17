//! The query routes under `/v1/hosts/{host}/...` (037): the same Prometheus
//! and Loki routes, answered by one configured host, by this daemon (`local`),
//! or by every ready host at once (`all`).
//!
//! A named host's answer is relayed as it came. `all` runs the request on
//! each ready host and merges the answers into the shape one daemon returns,
//! with a `host` label on every series, stream and label set.

use std::collections::BTreeSet;

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use futures_util::future::join_all;
use serde_json::{Map, Value, json};
use tower::ServiceExt;
use utoipa::openapi::path::{HttpMethod, OperationBuilder, ParameterBuilder, ParameterIn};
use utoipa::openapi::response::ResponseBuilder;
use utoipa::openapi::{Object, OpenApi, Required};

use ariadne_api::hosts::{HostDto, HostStatus};
use ariadne_client::{Client, ForwardedResponse};

use super::super::AppState;
use super::super::error::ApiError;

/// A query route: its path under the host prefix, its methods and the query
/// parameters it reads.
struct Route {
    path: &'static str,
    methods: &'static [&'static str],
    params: &'static [&'static str],
}

const ROUTES: &[Route] = &[
    Route {
        path: "-/healthy",
        methods: &["GET"],
        params: &[],
    },
    Route {
        path: "api/v1/query",
        methods: &["GET", "POST"],
        params: &["query", "time"],
    },
    Route {
        path: "api/v1/parse_query",
        methods: &["GET", "POST"],
        params: &["query"],
    },
    Route {
        path: "api/v1/query_range",
        methods: &["GET", "POST"],
        params: &["query", "start", "end", "step"],
    },
    Route {
        path: "api/v1/labels",
        methods: &["GET", "POST"],
        params: &["match[]", "start", "end"],
    },
    Route {
        path: "api/v1/label/{name}/values",
        methods: &["GET"],
        params: &["match[]", "start", "end"],
    },
    Route {
        path: "api/v1/series",
        methods: &["GET", "POST"],
        params: &["match[]", "start", "end"],
    },
    Route {
        path: "api/v1/metadata",
        methods: &["GET"],
        params: &["metric", "limit"],
    },
    Route {
        path: "loki/api/v1/query",
        methods: &["GET"],
        params: &["query", "time", "limit", "direction"],
    },
    Route {
        path: "loki/api/v1/query_range",
        methods: &["GET"],
        params: &["query", "start", "end", "limit", "direction", "step"],
    },
    Route {
        path: "loki/api/v1/labels",
        methods: &["GET"],
        params: &["start", "end"],
    },
    Route {
        path: "loki/api/v1/label/{name}/values",
        methods: &["GET"],
        params: &["start", "end"],
    },
    Route {
        path: "loki/api/v1/series",
        methods: &["GET"],
        params: &["match[]", "start", "end"],
    },
    Route {
        path: "loki/api/v1/index/stats",
        methods: &["GET"],
        params: &[],
    },
    Route {
        path: "loki/api/v1/index/volume",
        methods: &["GET"],
        params: &[],
    },
];

pub(super) fn routes() -> Router<AppState> {
    Router::new().route("/v1/hosts/{host}/{*rest}", get(fan).post(fan))
}

/// The route of `rest` and the label name it carries, if it is one of
/// [`ROUTES`].
fn route(rest: &str) -> Option<(&'static Route, Option<String>)> {
    let segments: Vec<&str> = rest.split('/').collect();
    ROUTES.iter().find_map(|route| {
        let pattern: Vec<&str> = route.path.split('/').collect();
        if pattern.len() != segments.len() {
            return None;
        }
        let mut name = None;
        for (want, got) in pattern.iter().zip(&segments) {
            if *want == "{name}" {
                name = Some((*got).to_string());
            } else if want != got {
                return None;
            }
        }
        Some((route, name))
    })
}

async fn fan(
    State(state): State<AppState>,
    Path((host, rest)): Path<(String, String)>,
    method: Method,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let (route, name) = route(&rest).ok_or_else(|| {
        ApiError::new(
            StatusCode::NOT_FOUND,
            "not_found",
            format!("no route {rest}"),
        )
    })?;
    if !route.methods.contains(&method.as_str()) {
        return Err(ApiError::new(
            StatusCode::METHOD_NOT_ALLOWED,
            "method_not_allowed",
            format!("{method} is not served on {rest}"),
        ));
    }
    let request = Relay {
        method,
        path: match query {
            Some(query) => format!("/{rest}?{query}"),
            None => format!("/{rest}"),
        },
        content_type: headers
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string),
        body: body.to_vec(),
    };
    let hosts = state.hosts.list();
    if host != "all" {
        let target = hosts
            .iter()
            .find(|candidate| candidate.name == host)
            .ok_or_else(|| {
                ApiError::new(
                    StatusCode::NOT_FOUND,
                    "not_found",
                    format!("unknown host {host}"),
                )
            })?;
        if target.status != HostStatus::Ready {
            return Err(unavailable(target));
        }
        return match request.send(&state, target).await {
            Ok(answer) => Ok(relayed(answer)),
            Err(error) => Err(ApiError::new(
                StatusCode::BAD_GATEWAY,
                "host_unreachable",
                format!("{}: {error}", target.name),
            )),
        };
    }

    let mut warnings = Vec::new();
    let mut targets = Vec::new();
    for target in &hosts {
        if target.status == HostStatus::Ready {
            targets.push(target);
        } else {
            warnings.push(unavailable(target).body.error.message);
        }
    }
    let results = join_all(targets.iter().map(|target| request.send(&state, target))).await;
    let mut answers = Vec::new();
    let mut refusal = None;
    for (target, result) in targets.iter().zip(results) {
        match result {
            Ok(answer) if (200..300).contains(&answer.status) => answers.push((target, answer)),
            Ok(answer) => {
                warnings.push(format!("{}: {}", target.name, sentence(&answer)));
                if (400..500).contains(&answer.status) && refusal.is_none() {
                    refusal = Some(answer);
                }
            }
            Err(error) => warnings.push(format!("{}: {error}", target.name)),
        }
    }
    if answers.is_empty() {
        return match refusal {
            // Every host refused the request itself: say so as one would.
            Some(answer) => Ok(relayed(answer)),
            None => Err(ApiError::new(
                StatusCode::BAD_GATEWAY,
                "host_unreachable",
                format!("no host answered: {}", warnings.join("; ")),
            )),
        };
    }
    if route.path == "-/healthy" || route.path == "api/v1/parse_query" {
        // Nothing here depends on a host: the first answer is the answer.
        return Ok(relayed(answers.swap_remove(0).1));
    }
    let mut parsed = Vec::new();
    for (target, answer) in answers {
        match serde_json::from_slice::<Value>(&answer.body) {
            Ok(value) => parsed.push((target.name.clone(), value)),
            Err(error) => warnings.push(format!("{}: unreadable answer: {error}", target.name)),
        }
    }
    if parsed.is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            "host_unreachable",
            format!("no host answered: {}", warnings.join("; ")),
        ));
    }
    let data = merge(route.path, name.as_deref(), &parsed);
    let mut answer = json!({"status": "success", "data": data});
    if !warnings.is_empty() {
        answer["warnings"] = json!(warnings);
    }
    Ok(axum::Json(answer).into_response())
}

fn unavailable(host: &HostDto) -> ApiError {
    ApiError::new(
        StatusCode::SERVICE_UNAVAILABLE,
        "host_unavailable",
        format!(
            "host {} is {}",
            host.name,
            format!("{:?}", host.status).to_lowercase()
        ),
    )
}

fn relayed(answer: ForwardedResponse) -> Response {
    let mut response = (
        StatusCode::from_u16(answer.status).unwrap_or(StatusCode::BAD_GATEWAY),
        answer.body,
    )
        .into_response();
    if let Some(content_type) = answer.content_type.and_then(|value| value.parse().ok()) {
        response
            .headers_mut()
            .insert(header::CONTENT_TYPE, content_type);
    }
    response
}

/// Why a host's answer was a failure, in a sentence.
fn sentence(answer: &ForwardedResponse) -> String {
    let body: Value = serde_json::from_slice(&answer.body).unwrap_or(Value::Null);
    body["error"]
        .as_str()
        .or_else(|| body["error"]["message"].as_str())
        .map_or_else(|| format!("status {}", answer.status), str::to_string)
}

/// One request, ready to run on any host.
struct Relay {
    method: Method,
    path: String,
    content_type: Option<String>,
    body: Vec<u8>,
}

impl Relay {
    /// Run on `host`: this daemon's own query routes for `local`, the
    /// host's endpoint for any other.
    async fn send(&self, state: &AppState, host: &HostDto) -> Result<ForwardedResponse, String> {
        if host.name == "local" {
            let mut request = Request::builder()
                .method(self.method.clone())
                .uri(&self.path);
            if let Some(content_type) = &self.content_type {
                request = request.header(header::CONTENT_TYPE, content_type);
            }
            let request = request
                .body(Body::from(self.body.clone()))
                .map_err(|error| error.to_string())?;
            let Ok(response) = super::routes()
                .with_state(state.clone())
                .oneshot(request)
                .await;
            let status = response.status().as_u16();
            let content_type = response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .map(str::to_string);
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .map_err(|error| error.to_string())?
                .to_vec();
            return Ok(ForwardedResponse {
                status,
                content_type,
                body,
            });
        }
        let endpoint = host.endpoint.as_deref().ok_or("no endpoint")?;
        let client = if endpoint.starts_with("http://") || endpoint.starts_with("https://") {
            Client::tcp(endpoint)
        } else {
            Client::unix(endpoint)
        };
        client
            .forward(
                self.method.clone(),
                &self.path,
                self.content_type.as_deref(),
                self.body.clone(),
            )
            .await
            .map_err(|error| error.to_string())
    }
}

// ---- merging ----

/// Put the host on a label set. An attribute that is already called `host`
/// moves to `exported_host`, as `service` does (034, rule 8).
fn label(labels: &mut Value, host: &str) {
    if let Some(map) = labels.as_object_mut() {
        if let Some(old) = map.remove("host") {
            map.insert("exported_host".into(), old);
        }
        map.insert("host".into(), host.into());
    }
}

/// The `data` of the combined answer. Every answer is a success envelope.
fn merge(path: &str, name: Option<&str>, answers: &[(String, Value)]) -> Value {
    let data = |answer: &Value| answer["data"].clone();
    if path.ends_with("/labels") {
        let mut names: BTreeSet<String> = ["host".to_string()].into();
        for (_, answer) in answers {
            names.extend(strings(&data(answer)));
        }
        return json!(names);
    }
    if path.contains("/label/{name}/values") {
        if name == Some("host") {
            let hosts: BTreeSet<&str> = answers.iter().map(|(host, _)| host.as_str()).collect();
            return json!(hosts);
        }
        let mut values = BTreeSet::new();
        for (_, answer) in answers {
            values.extend(strings(&data(answer)));
        }
        return json!(values);
    }
    if path.ends_with("/series") {
        let mut sets = Vec::new();
        for (host, answer) in answers {
            for mut set in data(answer).as_array().cloned().unwrap_or_default() {
                label(&mut set, host);
                sets.push(set);
            }
        }
        return json!(sets);
    }
    if path == "api/v1/metadata" {
        let mut merged: Map<String, Value> = Map::new();
        for (_, answer) in answers {
            for (metric, entries) in data(answer).as_object().into_iter().flatten() {
                let slot = merged.entry(metric.clone()).or_insert_with(|| json!([]));
                for entry in entries.as_array().into_iter().flatten() {
                    if !slot.as_array().is_some_and(|have| have.contains(entry)) {
                        slot.as_array_mut().expect("an array").push(entry.clone());
                    }
                }
            }
        }
        return Value::Object(merged);
    }
    if path.ends_with("/index/stats") {
        let mut sum = Map::new();
        for (_, answer) in answers {
            for (key, value) in data(answer).as_object().into_iter().flatten() {
                let total =
                    sum.get(key).and_then(Value::as_u64).unwrap_or(0) + value.as_u64().unwrap_or(0);
                sum.insert(key.clone(), json!(total));
            }
        }
        return Value::Object(sum);
    }
    results(answers)
}

fn strings(data: &Value) -> impl Iterator<Item = String> + '_ {
    data.as_array()
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str().map(str::to_string))
}

/// The `data` of a query: every answer's series or streams, each with its
/// host. A scalar or a string has no labels to carry the host, so it comes
/// back as a one-sample vector per host.
fn results(answers: &[(String, Value)]) -> Value {
    let mut combined = answers[0].1["data"].clone();
    let kind = combined["resultType"]
        .as_str()
        .unwrap_or("vector")
        .to_string();
    let mut items = Vec::new();
    for (host, answer) in answers {
        let data = &answer["data"];
        match kind.as_str() {
            "scalar" | "string" => {
                items.push(json!({"metric": {"host": host}, "value": data["result"]}))
            }
            other => {
                let key = if other == "streams" {
                    "stream"
                } else {
                    "metric"
                };
                for mut item in data["result"].as_array().cloned().unwrap_or_default() {
                    label(&mut item[key], host);
                    items.push(item);
                }
            }
        }
    }
    combined["resultType"] = match kind.as_str() {
        "scalar" | "string" => json!("vector"),
        _ => json!(kind),
    };
    combined["result"] = json!(items);
    combined
}

// ---- OpenAPI ----

/// Adds the query routes of each host to the OpenAPI document.
pub(in crate::http) struct QueryRoutes;

impl utoipa::Modify for QueryRoutes {
    fn modify(&self, openapi: &mut OpenApi) {
        for route in ROUTES {
            for method in route.methods {
                let mut operation = OperationBuilder::new()
                    .tag("hosts")
                    .summary(Some(format!(
                        "{method} /{} on one host or on all",
                        route.path
                    )))
                    .description(Some(
                        "`{host}` is a host name, `local` or `all`. With `all` the answers of the \
                         ready hosts are merged, each series, stream and label set gets a `host` \
                         label, and failed hosts are listed in `warnings`.",
                    ))
                    .parameter(
                        ParameterBuilder::new()
                            .name("host")
                            .parameter_in(ParameterIn::Path)
                            .required(Required::True)
                            .schema(Some(Object::with_type(utoipa::openapi::Type::String))),
                    );
                if route.path.contains("{name}") {
                    operation = operation.parameter(
                        ParameterBuilder::new()
                            .name("name")
                            .parameter_in(ParameterIn::Path)
                            .required(Required::True)
                            .schema(Some(Object::with_type(utoipa::openapi::Type::String))),
                    );
                }
                for param in route.params {
                    operation = operation.parameter(
                        ParameterBuilder::new()
                            .name(*param)
                            .parameter_in(ParameterIn::Query)
                            .required(Required::False)
                            .schema(Some(Object::with_type(utoipa::openapi::Type::String))),
                    );
                }
                for (status, text) in [
                    (
                        "200",
                        "The host's answer, or the merged answers of every host",
                    ),
                    ("404", "Unknown host"),
                    ("503", "The host is not ready"),
                ] {
                    operation =
                        operation.response(status, ResponseBuilder::new().description(text));
                }
                let http = if *method == "POST" {
                    HttpMethod::Post
                } else {
                    HttpMethod::Get
                };
                openapi.paths.add_path_operation(
                    format!("/v1/hosts/{{host}}/{}", route.path),
                    vec![http],
                    operation.build(),
                );
            }
        }
    }
}
