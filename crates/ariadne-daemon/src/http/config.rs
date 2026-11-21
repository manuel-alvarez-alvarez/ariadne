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

#[derive(serde::Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct MaintenanceRequest {
    active: bool,
}

#[derive(serde::Serialize, utoipa::ToSchema)]
pub(super) struct MaintenanceDto {
    ready: bool,
}

#[utoipa::path(post, path="/v1/maintenance", tag="hosts", request_body=MaintenanceRequest,
    responses((status=200,body=MaintenanceDto)))]
pub(super) async fn maintenance(
    State(s): State<AppState>,
    Json(request): Json<MaintenanceRequest>,
) -> ApiResult<Json<MaintenanceDto>> {
    s.launcher
        .cfg
        .portable
        .hold(&s.launcher.cfg, &s.store, request.active)
        .await
        .map(|ready| Json(MaintenanceDto { ready }))
        .map_err(|e| ApiError::bad_request(e.to_string()))
}
