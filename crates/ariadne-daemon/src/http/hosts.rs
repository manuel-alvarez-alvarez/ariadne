//! The host list and the reconnect of one host (037).
use super::AppState;
use super::error::{ApiError, ApiResult, Json};
use crate::hosts::ReconnectError;
use ariadne_api::hosts::HostDto;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::Deserialize;

#[utoipa::path(get, path="/v1/hosts", tag="hosts", responses((status=200,body=[HostDto])))]
pub(super) async fn list(State(s): State<AppState>) -> ApiResult<Json<Vec<HostDto>>> {
    Ok(Json(s.hosts.list()))
}

/// Make one configured host reconnect at once, and answer the host once it
/// has acted on it.
#[utoipa::path(post, path = "/v1/hosts/{name}/reconnect", tag = "hosts",
    params(("name" = String, Path, description = "host name; not `local`")),
    responses(
        (status = 200, body = HostDto),
        (status = 400, description = "the name is `local`"),
        (status = 404, description = "no configured host has the name"),
    ))]
pub(super) async fn reconnect(
    State(s): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<Json<HostDto>> {
    match s.hosts.reconnect(&name).await {
        Ok(host) => Ok(Json(host)),
        Err(ReconnectError::Local) => Err(ApiError::bad_request(
            "local is this daemon: there is nothing to reconnect",
        )),
        Err(ReconnectError::Unknown) => Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "not_found",
            format!("unknown host {name}"),
        )),
    }
}

#[utoipa::path(post, path="/v1/hosts/{name}/setup", tag="hosts", params(("name"=String, Path)), request_body=ariadne_api::hosts::SetupRequest, responses((status=202,body=ariadne_api::hosts::SetupDto)))]
pub(super) async fn setup(
    State(s): State<AppState>,
    Path(name): Path<String>,
    Json(request): Json<ariadne_api::hosts::SetupRequest>,
) -> ApiResult<(StatusCode, Json<ariadne_api::hosts::SetupDto>)> {
    let progress = s
        .hosts
        .setup(name, request)
        .map_err(ApiError::bad_request)?;
    Ok((StatusCode::ACCEPTED, Json(progress)))
}

#[utoipa::path(get, path="/v1/hosts/{name}/setup", tag="hosts", params(("name"=String, Path)), responses((status=200,body=ariadne_api::hosts::SetupDto), (status=404)))]
pub(super) async fn setup_status(
    State(s): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<Json<ariadne_api::hosts::SetupDto>> {
    s.hosts
        .setup_status(&name)
        .map(Json)
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "not_found", "unknown worker setup"))
}

#[derive(serde::Deserialize, utoipa::ToSchema)]
pub(super) struct CreateHost {
    name: String,
    endpoint: String,
    remote: Option<String>,
    color: Option<ariadne_api::hosts::HostColor>,
}

#[derive(Default, serde::Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct PatchHost {
    #[serde(default, deserialize_with = "present")]
    #[schema(nullable = false)]
    endpoint: Option<Option<String>>,
    #[serde(default, deserialize_with = "present")]
    remote: Option<Option<String>>,
    #[serde(default, deserialize_with = "present")]
    color: Option<Option<ariadne_api::hosts::HostColor>>,
}

fn present<'de, D: serde::Deserializer<'de>, T: serde::Deserialize<'de>>(
    d: D,
) -> Result<Option<Option<T>>, D::Error> {
    Option::<T>::deserialize(d).map(Some)
}

fn validate(name: &str, endpoint: &str, remote: Option<String>) -> ApiResult<()> {
    let mut file = ariadne_client::endpoint::FileConfig::default();
    file.hosts.0.push((
        name.into(),
        ariadne_client::endpoint::HostConfig {
            endpoint: endpoint.into(),
            remote,
        },
    ));
    crate::config::validate_hosts(&file).map_err(|e| ApiError::bad_request(e.to_string()))
}

fn unknown() -> ApiError {
    ApiError::new(StatusCode::NOT_FOUND, "unknown_host", "unknown host")
}

#[utoipa::path(post, path="/v1/hosts", tag="hosts", request_body=CreateHost, responses((status=201,body=HostDto),(status=400),(status=409)))]
pub(super) async fn create(
    State(s): State<AppState>,
    Json(body): Json<CreateHost>,
) -> ApiResult<(StatusCode, Json<HostDto>)> {
    let _mutation = s.hosts.mutation().await;
    validate(&body.name, &body.endpoint, body.remote.clone())?;
    if s.hosts.list().iter().any(|h| h.name == body.name) {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "conflict",
            "host name exists",
        ));
    }
    let host = s
        .hosts
        .save_connection(
            &body.name,
            Some(&body.endpoint),
            body.remote.as_deref(),
            body.color,
        )
        .await
        .map_err(ApiError::bad_request)?;
    Ok((StatusCode::CREATED, Json(host)))
}

#[utoipa::path(patch, path="/v1/hosts/{name}", tag="hosts", params(("name"=String,Path)), request_body=PatchHost, responses((status=200,body=HostDto),(status=400),(status=404)))]
pub(super) async fn patch(
    State(s): State<AppState>,
    Path(name): Path<String>,
    Json(body): Json<PatchHost>,
) -> ApiResult<Json<HostDto>> {
    let _mutation = s.hosts.mutation().await;
    let old = s
        .hosts
        .list()
        .into_iter()
        .find(|h| h.name == name)
        .ok_or_else(unknown)?;
    if name == "local" && (body.endpoint.is_some() || body.remote.is_some()) {
        return Err(ApiError::bad_request("local accepts only color"));
    }
    if body.endpoint == Some(None) {
        return Err(ApiError::bad_request("endpoint cannot be null"));
    }
    let connection = s.hosts.connection(&name);
    let endpoint = body
        .endpoint
        .flatten()
        .or_else(|| connection.as_ref().map(|c| c.endpoint.clone()));
    let remote = body
        .remote
        .unwrap_or_else(|| connection.and_then(|c| c.remote));
    if let Some(endpoint) = &endpoint {
        validate(&name, endpoint, remote.clone())?;
    }
    s.hosts
        .save_connection(
            &name,
            endpoint.as_deref(),
            remote.as_deref(),
            body.color.unwrap_or(old.color),
        )
        .await
        .map(Json)
        .map_err(ApiError::bad_request)
}

#[utoipa::path(delete, path="/v1/hosts/{name}", tag="hosts", params(("name"=String,Path)), responses((status=204),(status=400),(status=404)))]
pub(super) async fn delete(
    State(s): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<StatusCode> {
    let _mutation = s.hosts.mutation().await;
    if name == "local" {
        return Err(ApiError::bad_request("local cannot be removed"));
    }
    if !s.hosts.list().iter().any(|h| h.name == name) {
        return Err(unknown());
    }
    s.hosts
        .remove_connection(&name)
        .await
        .map_err(ApiError::bad_request)?;
    Ok(StatusCode::NO_CONTENT)
}
