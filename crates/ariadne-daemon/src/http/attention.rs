//! `GET /v1/attention`: the authoritative "Needs attention" list.

use axum::extract::State;

use ariadne_api::attention::AttentionListDto;

use super::AppState;
use super::error::Json;

/// Every item a human has something to do about right now: read off every
/// registered producer (`crate::attention`), not inferred here from a task
/// or a session's bare status.
#[utoipa::path(get, path = "/v1/attention", tag = "attention",
    responses((status = 200, body = AttentionListDto)))]
pub(crate) async fn list(State(state): State<AppState>) -> Json<AttentionListDto> {
    Json(crate::attention::collect(&state.store, &state.launcher).await)
}
