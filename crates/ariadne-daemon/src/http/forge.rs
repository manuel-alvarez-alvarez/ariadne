//! The webhook tunnel's switch and state (027), and detecting the forge a
//! checkout's remote is on ahead of registering it (025).

use axum::extract::State;

use ariadne_api::repositories::{
    DetectForgeRequest, ForgeDetectionDto, ForgeTunnelDto, SetTunnelRequest,
};

use super::AppState;
use super::error::{ApiResult, Json};
use super::repositories::{bad, repo_path};
use crate::gitwt::GitManager;

/// Detect the forge a checkout's remote is on, the same way registration
/// does. Null for a checkout with no usable forge remote.
#[utoipa::path(post, path = "/v1/forge/detect", tag = "repositories",
    request_body = DetectForgeRequest,
    responses(
        (status = 200, body = Option<ForgeDetectionDto>),
        (status = 400, description = "not an absolute path, or not a git work tree")
    ))]
pub(super) async fn detect(
    State(state): State<AppState>,
    Json(req): Json<DetectForgeRequest>,
) -> ApiResult<Json<Option<ForgeDetectionDto>>> {
    let path = repo_path(&req.path)?;
    GitManager.validate_repo(&path).await.map_err(bad)?;
    let detected = crate::forge::detect(&state.launcher.cfg, &path).await;
    Ok(Json(detected.map(|d| ForgeDetectionDto {
        kind: d.kind,
        host: d.repo.host,
        owner: d.repo.owner,
        name: d.repo.name,
        remote: d.remote,
    })))
}

/// The tunnel switch, the tunnel state and the bound listener address.
#[utoipa::path(get, path = "/v1/forge/tunnel", tag = "repositories",
    responses((status = 200, body = ForgeTunnelDto)))]
pub(super) async fn get_tunnel(State(state): State<AppState>) -> ApiResult<Json<ForgeTunnelDto>> {
    Ok(Json(state.tunnel.status().await?))
}

/// Turn the tunnel on or off. Off closes it, and every integration fetches on
/// its timer; the hooks stay registered.
#[utoipa::path(put, path = "/v1/forge/tunnel", tag = "repositories",
    request_body = SetTunnelRequest,
    responses((status = 200, body = ForgeTunnelDto)))]
pub(super) async fn set_tunnel(
    State(state): State<AppState>,
    Json(req): Json<SetTunnelRequest>,
) -> ApiResult<Json<ForgeTunnelDto>> {
    Ok(Json(state.tunnel.set_enabled(req.enabled).await?))
}
