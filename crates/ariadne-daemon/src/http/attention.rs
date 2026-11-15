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

#[utoipa::path(post, path = "/v1/attention/{id}/dismiss", tag = "attention",
    params(("id" = String, Path, description = "Attention item id")),
    responses((status = 204), (status = 404, description = "Item no longer exists")))]
pub(crate) async fn dismiss(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<axum::http::StatusCode, super::error::ApiError> {
    let _guard = state.store.lock_attention().await;
    let list = crate::attention::collect_raw(&state.store, &state.launcher).await;
    let item = list
        .items
        .iter()
        .find(|item| item.id == id)
        .ok_or_else(|| {
            super::error::ApiError::new(
                axum::http::StatusCode::NOT_FOUND,
                "not_found",
                "Attention item no longer exists",
            )
        })?;
    if item.occurrence.is_empty() {
        return Err(super::error::ApiError::new(
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "unavailable",
            "The attention occurrence could not be read",
        ));
    }
    state.store.dismiss_attention(&id, &item.occurrence).await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}
