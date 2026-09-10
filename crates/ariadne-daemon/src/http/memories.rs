//! Repository memory endpoints.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use chrono::{DateTime, SecondsFormat, Utc};

use ariadne_api::memories::{CreateMemoryRequest, MemoryDto, MemorySearchQuery};
use ariadne_store::NewMemory;

use super::AppState;
use super::caller::{call_ctx, ensure_repository_scope};
use super::convert::memory_dto;
use super::error::{ApiError, ApiResult, Json};

#[utoipa::path(post, path = "/v1/repositories/{repository_id}/memories", tag = "memories",
    request_body = CreateMemoryRequest,
    params(("repository_id" = String, Path, description = "repository id")),
    responses((status = 201, body = MemoryDto), (status = 400), (status = 403), (status = 404)))]
pub async fn create(
    State(state): State<AppState>,
    Path(repository_id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<CreateMemoryRequest>,
) -> ApiResult<(StatusCode, Json<MemoryDto>)> {
    let ctx = call_ctx(&state.store, &headers).await?;
    ensure_repository_scope(&state.store, &ctx, &repository_id).await?;
    let session = ctx
        .session
        .ok_or_else(|| ApiError::forbidden("saving memory requires an agent session"))?;
    let text = req.text.trim().to_string();
    if text.is_empty() {
        return Err(ApiError::bad_request("memory text must not be empty"));
    }
    let expires_at = DateTime::parse_from_rfc3339(&req.expires_at)
        .map_err(|_| ApiError::bad_request("expires_at must be an RFC 3339 time"))?
        .with_timezone(&Utc)
        .to_rfc3339_opts(SecondsFormat::Millis, true);
    let memory = state
        .store
        .create_memory(NewMemory {
            repository_id,
            text,
            source_session_id: session.id,
            source_task_id: session.task_id,
            source_goal_id: session.goal_id,
            expires_at,
        })
        .await?;
    Ok((StatusCode::CREATED, Json(memory_dto(memory))))
}

#[utoipa::path(get, path = "/v1/repositories/{repository_id}/memories", tag = "memories",
    params(("repository_id" = String, Path, description = "repository id")),
    responses((status = 200, body = [MemoryDto]), (status = 403), (status = 404)))]
pub async fn list(
    State(state): State<AppState>,
    Path(repository_id): Path<String>,
    headers: HeaderMap,
) -> ApiResult<Json<Vec<MemoryDto>>> {
    let ctx = call_ctx(&state.store, &headers).await?;
    ensure_repository_scope(&state.store, &ctx, &repository_id).await?;
    let memories = state.store.list_memories(&repository_id).await?;
    Ok(Json(memories.into_iter().map(memory_dto).collect()))
}

#[utoipa::path(get, path = "/v1/repositories/{repository_id}/memories/search", tag = "memories",
    params(("repository_id" = String, Path, description = "repository id"), MemorySearchQuery),
    responses((status = 200, body = [MemoryDto]), (status = 403), (status = 404)))]
pub async fn search(
    State(state): State<AppState>,
    Path(repository_id): Path<String>,
    headers: HeaderMap,
    Query(query): Query<MemorySearchQuery>,
) -> ApiResult<Json<Vec<MemoryDto>>> {
    let ctx = call_ctx(&state.store, &headers).await?;
    ensure_repository_scope(&state.store, &ctx, &repository_id).await?;
    let memories = state
        .store
        .search_memories(&repository_id, query.q.trim())
        .await?;
    Ok(Json(memories.into_iter().map(memory_dto).collect()))
}

#[utoipa::path(delete, path = "/v1/repositories/{repository_id}/memories/{id}", tag = "memories",
    params(("repository_id" = String, Path, description = "repository id"),
           ("id" = String, Path, description = "memory id")),
    responses((status = 204), (status = 403), (status = 404)))]
pub async fn delete(
    State(state): State<AppState>,
    Path((repository_id, id)): Path<(String, String)>,
    headers: HeaderMap,
) -> ApiResult<StatusCode> {
    let ctx = call_ctx(&state.store, &headers).await?;
    ensure_repository_scope(&state.store, &ctx, &repository_id).await?;
    state.store.delete_memory(&repository_id, &id).await?;
    Ok(StatusCode::NO_CONTENT)
}
