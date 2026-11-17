//! The host list (037).
use super::AppState;
use super::error::{ApiResult, Json};
use ariadne_api::hosts::HostDto;

#[utoipa::path(get, path="/v1/hosts", tag="hosts", responses((status=200,body=[HostDto])))]
pub(super) async fn list(
    axum::extract::State(s): axum::extract::State<AppState>,
) -> ApiResult<Json<Vec<HostDto>>> {
    Ok(Json(s.hosts.list()))
}
