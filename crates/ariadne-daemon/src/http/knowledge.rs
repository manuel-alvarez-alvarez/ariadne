//! Knowledge base endpoints: a repository's index status, its reindex, and
//! the two questions every seat asks the index.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};

use ariadne_api::knowledge::{
    KnowledgeHitDto, KnowledgeLanguageDto, KnowledgeOutlineEntryDto, KnowledgeOutlineQuery,
    KnowledgeRefDto, KnowledgeSearchQuery, KnowledgeState, KnowledgeStatusDto,
};
use ariadne_knowledge::{KnowledgeStore, SearchQuery};
use ariadne_store::Repository;

use super::AppState;
use super::caller::{CallCtx, call_ctx};
use super::error::{ApiError, ApiResult, Json};

/// What a call is refused with while `knowledge_enabled = false`.
const DISABLED: &str =
    "the knowledge base is disabled: set knowledge_enabled = true in config.toml";

#[utoipa::path(get, path = "/v1/repositories/{id}/knowledge", tag = "knowledge",
    params(("id" = String, Path, description = "repository id")),
    responses((status = 200, body = KnowledgeStatusDto), (status = 404)))]
pub async fn status(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<KnowledgeStatusDto>> {
    state.store.get_repository(&id).await?;
    Ok(Json(status_dto(&state, &id).await?))
}

#[utoipa::path(post, path = "/v1/repositories/{id}/knowledge/reindex", tag = "knowledge",
    params(("id" = String, Path, description = "repository id")),
    responses((status = 202, body = KnowledgeStatusDto), (status = 404),
              (status = 409, description = "the knowledge base is disabled")))]
pub async fn reindex(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<(StatusCode, Json<KnowledgeStatusDto>)> {
    state.store.get_repository(&id).await?;
    enabled(&state)?;
    state
        .knowledge
        .reindex(&id)
        .await
        .map_err(|e| ApiError::conflict(e.to_string()))?;
    Ok((StatusCode::ACCEPTED, Json(status_dto(&state, &id).await?)))
}

#[utoipa::path(get, path = "/v1/knowledge/search", tag = "knowledge",
    params(KnowledgeSearchQuery),
    responses((status = 200, body = [KnowledgeHitDto]), (status = 400), (status = 404),
              (status = 409, description = "the knowledge base is disabled")))]
pub async fn search(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<KnowledgeSearchQuery>,
) -> ApiResult<Json<Vec<KnowledgeHitDto>>> {
    let ctx = call_ctx(&state.store, &headers).await?;
    let knowledge = enabled(&state)?;
    if query.q.trim().is_empty() {
        return Err(ApiError::bad_request("q must name something to find"));
    }
    let repositories = match &query.repository {
        Some(id) => vec![state.store.get_repository(id).await?],
        None => match &ctx.session {
            Some(session) if query.all != Some(true) => {
                state.store.list_goal_repositories(&session.goal_id).await?
            }
            _ => state.store.list_repositories().await?,
        },
    };
    let mut scopes = Vec::with_capacity(repositories.len());
    for repository in &repositories {
        let git_ref = ref_for(
            &state,
            knowledge,
            &ctx,
            repository,
            query.git_ref.as_deref(),
        )
        .await?;
        scopes.push((repository.id.clone(), git_ref));
    }
    let hits = knowledge
        .search(&SearchQuery {
            q: query.q.clone(),
            scopes,
            kind: query.kind.clone().filter(|kind| !kind.is_empty()),
            path: query.path.clone().filter(|path| !path.is_empty()),
            limit: query.limit(),
        })
        .await
        .map_err(|e| ApiError::conflict(e.to_string()))?;
    Ok(Json(
        hits.into_iter()
            .map(|hit| KnowledgeHitDto {
                repository_id: hit.repository_id,
                path: hit.path,
                line: hit.line,
                kind: hit.kind,
                name: hit.name,
                signature: hit.signature,
            })
            .collect(),
    ))
}

#[utoipa::path(get, path = "/v1/knowledge/outline", tag = "knowledge",
    params(KnowledgeOutlineQuery),
    responses((status = 200, body = [KnowledgeOutlineEntryDto]),
              (status = 404, description = "no such repository, or the path is not indexed at the ref"),
              (status = 409, description = "the knowledge base is disabled")))]
pub async fn outline(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<KnowledgeOutlineQuery>,
) -> ApiResult<Json<Vec<KnowledgeOutlineEntryDto>>> {
    let ctx = call_ctx(&state.store, &headers).await?;
    let knowledge = enabled(&state)?;
    let repository = state.store.get_repository(&query.repository).await?;
    let git_ref = ref_for(
        &state,
        knowledge,
        &ctx,
        &repository,
        query.git_ref.as_deref(),
    )
    .await?;
    let entries = knowledge
        .outline(&repository.id, &git_ref, &query.path)
        .await
        .map_err(|e| ApiError::conflict(e.to_string()))?
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::NOT_FOUND,
                "not_found",
                format!(
                    "{} is not indexed at {git_ref} in repository {}",
                    query.path, repository.id
                ),
            )
        })?;
    Ok(Json(
        entries
            .into_iter()
            .map(|entry| KnowledgeOutlineEntryDto {
                kind: entry.kind,
                name: entry.name,
                start_line: entry.start_line,
                end_line: entry.end_line,
                signature: entry.signature,
            })
            .collect(),
    ))
}

/// The store, or the refusal a disabled knowledge base answers with.
fn enabled(state: &AppState) -> ApiResult<&KnowledgeStore> {
    state
        .knowledge
        .store()
        .ok_or_else(|| ApiError::conflict(DISABLED))
}

/// The ref a caller reads a repository at: the one it named, else its own.
///
/// A task session's own is its task branch, in the task's repository —
/// where that branch has moved and been indexed; until then the branch is
/// the base branch's tree, and the base branch is what answers. Everyone
/// else reads the base branch.
async fn ref_for(
    state: &AppState,
    knowledge: &KnowledgeStore,
    ctx: &CallCtx,
    repository: &Repository,
    named: Option<&str>,
) -> ApiResult<String> {
    if let Some(git_ref) = named.filter(|git_ref| !git_ref.is_empty()) {
        return Ok(git_ref.to_string());
    }
    if let Some(task_id) = ctx
        .session
        .as_ref()
        .and_then(|session| session.task_id.as_deref())
    {
        let task = state.store.get_task(task_id).await?;
        if task.repo_id == repository.id
            && knowledge
                .ref_commit(&repository.id, &task.branch)
                .await
                .map_err(|e| ApiError::conflict(e.to_string()))?
                .is_some()
        {
            return Ok(task.branch);
        }
    }
    Ok(repository.base_branch.clone())
}

async fn status_dto(state: &AppState, repository_id: &str) -> ApiResult<KnowledgeStatusDto> {
    let Some(knowledge) = state.knowledge.store() else {
        return Ok(KnowledgeStatusDto {
            repository_id: repository_id.to_string(),
            state: KnowledgeState::Disabled,
            refs: Vec::new(),
            files: 0,
            symbols: 0,
            languages: Vec::new(),
            error: None,
        });
    };
    let status = knowledge
        .status(repository_id)
        .await
        .map_err(|e| ApiError::conflict(e.to_string()))?;
    Ok(KnowledgeStatusDto {
        repository_id: repository_id.to_string(),
        state: match status.state {
            ariadne_knowledge::State::Idle => KnowledgeState::Idle,
            ariadne_knowledge::State::Indexing => KnowledgeState::Indexing,
            ariadne_knowledge::State::Failed => KnowledgeState::Failed,
        },
        refs: status
            .refs
            .into_iter()
            .map(|r| KnowledgeRefDto {
                git_ref: r.git_ref,
                commit: r.commit,
                indexed_at: r.indexed_at,
                files: r.files,
                symbols: r.symbols,
            })
            .collect(),
        files: status.files,
        symbols: status.symbols,
        languages: status
            .languages
            .into_iter()
            .map(|l| KnowledgeLanguageDto {
                language: l.language,
                files: l.files,
            })
            .collect(),
        error: status.error,
    })
}
