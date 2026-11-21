//! Worker configuration transfer and status (037).
use super::{
    AppState,
    error::{ApiError, ApiResult, Json},
};
use ariadne_api::hosts::{ConfigSyncDto, PortableConfigDto};
use axum::extract::State;

#[utoipa::path(get, path="/v1/config/sync", tag="hosts", responses((status=200,body=ConfigSyncDto)))]
pub(super) async fn status(State(s): State<AppState>) -> ApiResult<Json<ConfigSyncDto>> {
    let cfg = &s.launcher.cfg;
    cfg.portable
        .status(cfg, &s.store)
        .await
        .map(Json)
        .map_err(|e| ApiError::bad_request(e.to_string()))
}

#[utoipa::path(put, path="/v1/config/sync", tag="hosts", request_body=PortableConfigDto,
    responses((status=200,body=ConfigSyncDto),(status=400)))]
pub(super) async fn apply(
    State(s): State<AppState>,
    Json(request): Json<PortableConfigDto>,
) -> ApiResult<Json<ConfigSyncDto>> {
    let cfg = &s.launcher.cfg;
    cfg.portable
        .apply(cfg, &s.store, request)
        .await
        .map(Json)
        .map_err(|e| ApiError::bad_request(e.to_string()))
}
