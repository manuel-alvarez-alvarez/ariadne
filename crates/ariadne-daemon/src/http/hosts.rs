//! The host list and the reconnect of one host (037).
use super::AppState;
use super::error::{ApiError, ApiResult, Json};
use crate::hosts::ReconnectError;
use ariadne_api::hosts::HostDto;
use axum::extract::{Path, State};
use axum::http::StatusCode;

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
