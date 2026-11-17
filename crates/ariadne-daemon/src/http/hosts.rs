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
