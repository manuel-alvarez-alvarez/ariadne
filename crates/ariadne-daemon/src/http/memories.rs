//! Memory endpoints: the facts kept about one repository, or about every one.
//!
//! A user call reads, writes and deletes in every scope. An agent session is
//! held to the repositories of its task or goal, and to the global memories,
//! which it reads but never writes.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use chrono::{DateTime, SecondsFormat, Utc};

use ariadne_api::memories::{
    CreateMemoryRequest, MemoryDto, MemoryListQuery, MemoryScope as ApiMemoryScope,
    MemorySearchQuery, MemorySearchResult,
};
use ariadne_store::{MemoryScope, NewMemory, Store, StoreError};

use super::AppState;
use super::caller::{CallCtx, call_ctx, ensure_repository_scope, session_repositories};
use super::convert::memory_dto;
use super::error::{ApiError, ApiResult, Json};

/// The most facts one task or goal session may save.
const MEMORY_SOURCE_LIMIT: i64 = 2;
/// The newest facts to return when no word matched a search.
const MEMORY_SEARCH_FALLBACK_LIMIT: usize = 5;

#[utoipa::path(post, path = "/v1/memories", tag = "memories",
    request_body = CreateMemoryRequest,
    responses((status = 201, body = MemoryDto), (status = 400), (status = 403), (status = 404)))]
pub async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CreateMemoryRequest>,
) -> ApiResult<(StatusCode, Json<MemoryDto>)> {
    let ctx = call_ctx(&state.store, &headers).await?;
    let text = req.text.trim().to_string();
    if text.is_empty() {
        return Err(ApiError::bad_request("memory text must not be empty"));
    }
    let expires_at = match &req.expires_at {
        Some(expires_at) => Some(
            DateTime::parse_from_rfc3339(expires_at)
                .map_err(|_| ApiError::bad_request("expires_at must be an RFC 3339 time"))?
                .with_timezone(&Utc)
                .to_rfc3339_opts(SecondsFormat::Millis, true),
        ),
        None => None,
    };
    if let Some(session) = &ctx.session {
        let Some(repository_id) = &req.repository_id else {
            return Err(ApiError::forbidden(format!(
                "session {} saves a memory for a repository of its own work only",
                session.id
            )));
        };
        ensure_repository_scope(&state.store, &ctx, repository_id).await?;
    }
    let session = ctx.session;
    let new = NewMemory {
        repository_id: req.repository_id,
        text,
        source_session_id: session.as_ref().map(|s| s.id.clone()),
        source_task_id: session.as_ref().and_then(|s| s.task_id.clone()),
        source_goal_id: session.as_ref().map(|s| s.goal_id.clone()),
        expires_at,
    };
    let memory = match &session {
        Some(session) => state
            .store
            .create_memory_with_source_limit(new, MEMORY_SOURCE_LIMIT)
            .await
            .map_err(|error| source_limit_error(error, session))?,
        None => state.store.create_memory(new).await?,
    };
    Ok((StatusCode::CREATED, Json(memory_dto(memory))))
}

/// Turn the store's atomic source-limit refusal into the agent's next step.
fn source_limit_error(error: StoreError, session: &ariadne_store::AgentSession) -> ApiError {
    match error {
        StoreError::Conflict(message) if message == "memory source limit reached" => {
            if session.task_id.is_some() {
                ApiError::conflict(
                    "This task reached its memory limit. Keep the one fact that matters.",
                )
            } else {
                ApiError::conflict(
                    "This goal reached its memory limit. Keep the one fact that matters.",
                )
            }
        }
        error => error.into(),
    }
}

#[utoipa::path(get, path = "/v1/memories", tag = "memories",
    params(MemoryListQuery),
    responses((status = 200, body = [MemoryDto]), (status = 400), (status = 403), (status = 404)))]
pub async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<MemoryListQuery>,
) -> ApiResult<Json<Vec<MemoryDto>>> {
    let ctx = call_ctx(&state.store, &headers).await?;
    let scope = read_scope(
        &state.store,
        &ctx,
        query.repository.as_deref(),
        query.scope.unwrap_or_default(),
    )
    .await?;
    let memories = state.store.list_memories(&scope).await?;
    Ok(Json(memories.into_iter().map(memory_dto).collect()))
}

#[utoipa::path(get, path = "/v1/memories/search", tag = "memories",
    params(MemorySearchQuery),
    responses((status = 200, body = MemorySearchResult), (status = 400), (status = 403), (status = 404)))]
pub async fn search(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<MemorySearchQuery>,
) -> ApiResult<Json<MemorySearchResult>> {
    let ctx = call_ctx(&state.store, &headers).await?;
    let scope = read_scope(
        &state.store,
        &ctx,
        query.repository.as_deref(),
        query.scope.unwrap_or_default(),
    )
    .await?;
    let query = query.q.trim();
    let mut memories = state.store.search_memories(&scope, query).await?;
    let fallback = memories.is_empty();
    if fallback {
        memories = state.store.list_memories(&scope).await?;
        memories.truncate(MEMORY_SEARCH_FALLBACK_LIMIT);
    }
    Ok(Json(MemorySearchResult {
        hits: memories.into_iter().map(memory_dto).collect(),
        fallback,
    }))
}

#[utoipa::path(delete, path = "/v1/memories/{id}", tag = "memories",
    params(("id" = String, Path, description = "memory id")),
    responses((status = 204), (status = 403), (status = 404)))]
pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> ApiResult<StatusCode> {
    let ctx = call_ctx(&state.store, &headers).await?;
    let memory = state.store.get_memory(&id).await?;
    if let Some(session) = &ctx.session {
        let Some(repository_id) = &memory.repository_id else {
            return Err(ApiError::forbidden(format!(
                "session {} deletes a memory of a repository of its own work only",
                session.id
            )));
        };
        ensure_repository_scope(&state.store, &ctx, repository_id).await?;
    }
    state.store.delete_memory(&id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// What a caller reads, from the repository and the scope it named.
async fn read_scope(
    store: &Store,
    ctx: &CallCtx,
    repository: Option<&str>,
    scope: ApiMemoryScope,
) -> ApiResult<MemoryScope> {
    if let Some(repository_id) = repository {
        store.get_repository(repository_id).await?;
        ensure_repository_scope(store, ctx, repository_id).await?;
    }
    let of = |id: &str, global: bool| MemoryScope::Repositories {
        ids: vec![id.to_string()],
        global,
    };
    Ok(match (scope, repository) {
        (ApiMemoryScope::Global, _) => MemoryScope::Global,
        (ApiMemoryScope::Repository, Some(id)) => of(id, false),
        (ApiMemoryScope::Repository, None) => {
            return Err(ApiError::bad_request(
                "scope repository needs a repository to read",
            ));
        }
        (ApiMemoryScope::All, Some(id)) => of(id, true),
        (ApiMemoryScope::All, None) => match &ctx.session {
            None => MemoryScope::All,
            Some(session) => MemoryScope::Repositories {
                ids: session_repositories(store, session).await?,
                global: true,
            },
        },
    })
}
