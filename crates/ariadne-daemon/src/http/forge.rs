//! The webhook tunnel's switch and state (027).

use axum::extract::State;

use ariadne_api::repositories::{ForgeTunnelDto, SetTunnelRequest};

use super::AppState;
use super::error::{ApiResult, Json};

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
