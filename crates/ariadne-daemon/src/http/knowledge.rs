//! Knowledge base endpoints: a repository's index status, its reindex, the
//! six questions every seat asks the index, and the interactions between
//! one repository and the others.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};

use ariadne_api::knowledge::{
    KnowledgeContextDto, KnowledgeContextMoreDto, KnowledgeDetail, KnowledgeEdgeDto,
    KnowledgeEndpointDto, KnowledgeGraphConfidence, KnowledgeGraphDto, KnowledgeGraphEdgeDto,
    KnowledgeGraphNodeDto, KnowledgeGraphQuery, KnowledgeHitDto, KnowledgeImpactCallerDto,
    KnowledgeImpactDto, KnowledgeImpactQuery, KnowledgeInteractionGroupDto,
    KnowledgeInteractionsQuery, KnowledgeLanguageDto, KnowledgeMapDto, KnowledgeMapQuery,
    KnowledgeOutlineEntryDto, KnowledgeOutlineQuery, KnowledgePathDto, KnowledgePathHopDto,
    KnowledgePathQuery, KnowledgeRefDto, KnowledgeRelatedDto, KnowledgeSearchQuery, KnowledgeState,
    KnowledgeStatusDto, KnowledgeSymbolDto, KnowledgeSymbolQuery,
};
use ariadne_knowledge::store::{CONTEXT_LIMIT, INTERACTION_KINDS};
use ariadne_knowledge::{InteractionEnd, KnowledgeStore, Related, SearchQuery, index};
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
pub(super) async fn status(
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
pub(super) async fn reindex(
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
pub(super) async fn search(
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
pub(super) async fn outline(
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

#[utoipa::path(get, path = "/v1/knowledge/symbol", tag = "knowledge",
    params(KnowledgeSymbolQuery),
    responses((status = 200, body = [KnowledgeSymbolDto]), (status = 400), (status = 404),
              (status = 409, description = "the knowledge base is disabled")))]
pub(super) async fn symbol(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<KnowledgeSymbolQuery>,
) -> ApiResult<Json<Vec<KnowledgeSymbolDto>>> {
    let ctx = call_ctx(&state.store, &headers).await?;
    let knowledge = enabled(&state)?;
    if query.name.trim().is_empty() {
        return Err(ApiError::bad_request("name must name a definition"));
    }
    let repositories = match &query.repository {
        Some(id) => vec![state.store.get_repository(id).await?],
        None => match &ctx.session {
            Some(session) => state.store.list_goal_repositories(&session.goal_id).await?,
            None => state.store.list_repositories().await?,
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
    let definitions = knowledge
        .definitions(query.name.trim(), &scopes)
        .await
        .map_err(|e| ApiError::conflict(e.to_string()))?;
    let detail = query.detail.unwrap_or_default();
    let mut answers = Vec::with_capacity(definitions.len());
    for definition in definitions {
        let source = match detail {
            KnowledgeDetail::Source => {
                let path = repositories
                    .iter()
                    .find(|repository| repository.id == definition.repository_id)
                    .map(|repository| repository.path.clone());
                match path {
                    Some(path) => lines_of(
                        &index::blob_text(std::path::Path::new(&path), &definition.blob)
                            .await
                            .map_err(|e| ApiError::conflict(e.to_string()))?,
                        definition.start_line,
                        definition.end_line,
                    ),
                    None => None,
                }
            }
            _ => None,
        };
        let context = match detail {
            KnowledgeDetail::Context => Some(
                knowledge
                    .context(
                        definition.id,
                        &definition.repository_id,
                        &definition.git_ref,
                        CONTEXT_LIMIT,
                    )
                    .await
                    .map(|context| KnowledgeContextDto {
                        callers: related(context.callers),
                        callees: related(context.callees),
                        implementations: related(context.implementations),
                        references: related(context.references),
                        tests: related(context.tests),
                        more: KnowledgeContextMoreDto {
                            callers: context.more.callers,
                            callees: context.more.callees,
                            implementations: context.more.implementations,
                            references: context.more.references,
                            tests: context.more.tests,
                        },
                    })
                    .map_err(|e| ApiError::conflict(e.to_string()))?,
            ),
            _ => None,
        };
        answers.push(KnowledgeSymbolDto {
            repository_id: definition.repository_id,
            path: definition.path,
            start_line: definition.start_line,
            end_line: definition.end_line,
            kind: definition.kind,
            name: definition.name,
            signature: definition.signature,
            doc: definition.doc,
            source,
            context,
        });
    }
    Ok(Json(answers))
}

#[utoipa::path(get, path = "/v1/knowledge/impact", tag = "knowledge",
    params(KnowledgeImpactQuery),
    responses((status = 200, body = [KnowledgeImpactDto]), (status = 400), (status = 404),
              (status = 409, description = "the knowledge base is disabled")))]
pub(super) async fn impact(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<KnowledgeImpactQuery>,
) -> ApiResult<Json<Vec<KnowledgeImpactDto>>> {
    let ctx = call_ctx(&state.store, &headers).await?;
    let knowledge = enabled(&state)?;
    let repository = state.store.get_repository(&query.repository).await?;
    let symbol = query
        .symbol
        .as_deref()
        .filter(|name| !name.trim().is_empty());
    let diff = query
        .diff
        .as_deref()
        .filter(|range| !range.trim().is_empty());
    // A diff names the ref it is about: its lines are the head's, so the
    // definitions have to be the head's too. Where the head is a ref the
    // index knows, it is what answers; where it is not, the caller's own ref
    // is all there is.
    let head = match (&query.git_ref, diff) {
        (None, Some(range)) => head_of(range),
        _ => None,
    };
    let named = match &head {
        Some(head) => knowledge
            .ref_commit(&repository.id, head)
            .await
            .map_err(|e| ApiError::conflict(e.to_string()))?
            .map(|_| head.as_str()),
        None => query.git_ref.as_deref(),
    };
    let git_ref = ref_for(&state, knowledge, &ctx, &repository, named).await?;
    // One or the other: a call that named both would get an answer to a
    // question it did not ask.
    let changed: Vec<(i64, Related)> = match (symbol, diff) {
        (Some(name), None) => knowledge
            .definitions(name.trim(), &[(repository.id.clone(), git_ref.clone())])
            .await
            .map_err(|e| ApiError::conflict(e.to_string()))?
            .into_iter()
            .map(|definition| {
                (
                    definition.id,
                    Related {
                        repository_id: definition.repository_id,
                        path: definition.path,
                        line: definition.start_line,
                        name: definition.name,
                        confidence: "exact".into(),
                        // The definition itself, which no step resolved.
                        step: None,
                        candidates: 1,
                    },
                )
            })
            .collect(),
        (None, Some(range)) => {
            let lines = index::changed_lines(std::path::Path::new(&repository.path), range.trim())
                .await
                .map_err(|e| ApiError::bad_request(e.to_string()))?;
            knowledge
                .symbols_in_lines(&repository.id, &git_ref, &lines)
                .await
                .map_err(|e| ApiError::conflict(e.to_string()))?
        }
        _ => {
            return Err(ApiError::bad_request(
                "pass symbol or diff, and only one of them",
            ));
        }
    };
    let depth = query.depth();
    let mut answers = Vec::with_capacity(changed.len());
    for (id, symbol) in changed {
        let (callers, stopped) = knowledge
            .impact(id, &repository.id, &git_ref, depth)
            .await
            .map_err(|e| ApiError::conflict(e.to_string()))?;
        answers.push(KnowledgeImpactDto {
            symbol: KnowledgeRelatedDto {
                repository_id: symbol.repository_id,
                path: symbol.path,
                line: symbol.line,
                name: symbol.name,
                confidence: symbol.confidence,
                step: symbol.step,
                candidates: symbol.candidates,
            },
            callers: callers
                .into_iter()
                .map(|caller| KnowledgeImpactCallerDto {
                    depth: caller.depth,
                    repository_id: caller.repository_id,
                    path: caller.path,
                    line: caller.line,
                    name: caller.name,
                    confidence: caller.confidence,
                    step: caller.step,
                    candidates: caller.candidates,
                })
                .collect(),
            stopped,
        });
    }
    Ok(Json(answers))
}

#[utoipa::path(get, path = "/v1/knowledge/path", tag = "knowledge",
    params(KnowledgePathQuery),
    responses((status = 200, body = KnowledgePathDto), (status = 400), (status = 404),
              (status = 409, description = "the knowledge base is disabled")))]
pub(super) async fn path(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<KnowledgePathQuery>,
) -> ApiResult<Json<KnowledgePathDto>> {
    let ctx = call_ctx(&state.store, &headers).await?;
    let knowledge = enabled(&state)?;
    if query.from.trim().is_empty() || query.to.trim().is_empty() {
        return Err(ApiError::bad_request(
            "from and to must each name a definition",
        ));
    }
    let repository = state.store.get_repository(&query.repository).await?;
    let git_ref = ref_for(
        &state,
        knowledge,
        &ctx,
        &repository,
        query.git_ref.as_deref(),
    )
    .await?;
    let starts = knowledge
        .definitions(
            query.from.trim(),
            &[(repository.id.clone(), git_ref.clone())],
        )
        .await
        .map_err(|e| ApiError::conflict(e.to_string()))?;

    // The destination may sit across an edge in another repository. Read
    // each other repository at its own ref, as rule 20 does.
    let repositories = state.store.list_repositories().await?;
    let mut end_scopes = Vec::with_capacity(repositories.len());
    for candidate in &repositories {
        let candidate_ref = match candidate.id == repository.id {
            true => git_ref.clone(),
            false => ref_for(&state, knowledge, &ctx, candidate, None).await?,
        };
        end_scopes.push((candidate.id.clone(), candidate_ref));
    }
    let ends = knowledge
        .definitions(query.to.trim(), &end_scopes)
        .await
        .map_err(|e| ApiError::conflict(e.to_string()))?;
    let hops = knowledge
        .path(&starts, &ends, query.depth())
        .await
        .map_err(|e| ApiError::conflict(e.to_string()))?
        .into_iter()
        .map(|hop| KnowledgePathHopDto {
            repository_id: hop.repository_id,
            path: hop.path,
            line: hop.line,
            kind: hop.kind,
            name: hop.name,
            edge_kind: hop.edge_kind,
            confidence: hop.confidence,
        })
        .collect();
    Ok(Json(KnowledgePathDto { hops }))
}

#[utoipa::path(get, path = "/v1/knowledge/map", tag = "knowledge",
    params(KnowledgeMapQuery),
    responses((status = 200, body = KnowledgeMapDto), (status = 404),
              (status = 409, description = "the knowledge base is disabled")))]
pub(super) async fn map(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<KnowledgeMapQuery>,
) -> ApiResult<Json<KnowledgeMapDto>> {
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
    let toward = query.path.as_deref().filter(|path| !path.trim().is_empty());
    let map = knowledge
        .map(
            &repository.id,
            &git_ref,
            toward,
            query.budget().max(1) as usize,
        )
        .await
        .map_err(|e| ApiError::conflict(e.to_string()))?;
    Ok(Json(KnowledgeMapDto {
        repository_id: repository.id,
        git_ref,
        tokens: map.tokens(),
        files: map.files,
        files_left: map.files_left,
        text: map.text,
    }))
}

#[utoipa::path(get, path = "/v1/knowledge/graph", tag = "knowledge",
    params(KnowledgeGraphQuery),
    responses((status = 200, body = KnowledgeGraphDto), (status = 404),
              (status = 409, description = "the knowledge base is disabled")))]
pub(super) async fn graph(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<KnowledgeGraphQuery>,
) -> ApiResult<Json<KnowledgeGraphDto>> {
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
    let graph = knowledge
        .graph(&repository.id, &git_ref, query.limit() as usize)
        .await
        .map_err(|e| ApiError::conflict(e.to_string()))?;
    Ok(Json(KnowledgeGraphDto {
        repository_id: repository.id,
        git_ref,
        nodes: graph
            .nodes
            .into_iter()
            .map(|node| KnowledgeGraphNodeDto {
                path: node.path,
                language: node.language,
                symbols: node.symbols,
            })
            .collect(),
        edges: graph
            .edges
            .into_iter()
            .map(|edge| KnowledgeGraphEdgeDto {
                from: edge.from,
                to: edge.to,
                kind: edge.kind,
                count: edge.count,
                confidence: match edge.confidence.as_str() {
                    "exact" => KnowledgeGraphConfidence::Exact,
                    _ => KnowledgeGraphConfidence::Heuristic,
                },
            })
            .collect(),
        truncated: graph.truncated,
        total_nodes: graph.total_nodes,
    }))
}

#[utoipa::path(get, path = "/v1/knowledge/interactions", tag = "knowledge",
    params(KnowledgeInteractionsQuery),
    responses((status = 200, body = [KnowledgeInteractionGroupDto]), (status = 404),
              (status = 409, description = "the knowledge base is disabled")))]
pub(super) async fn interactions(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<KnowledgeInteractionsQuery>,
) -> ApiResult<Json<Vec<KnowledgeInteractionGroupDto>>> {
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
    let found = knowledge
        .interactions(&repository.id, &git_ref)
        .await
        .map_err(|e| ApiError::conflict(e.to_string()))?;
    // One group per kind that has an edge, in the order the kinds are
    // listed in.
    let mut groups: Vec<KnowledgeInteractionGroupDto> = Vec::new();
    for kind in INTERACTION_KINDS {
        let edges: Vec<KnowledgeEdgeDto> = found
            .iter()
            .filter(|interaction| interaction.kind == kind)
            .map(|interaction| KnowledgeEdgeDto {
                from: endpoint(&interaction.from),
                to: endpoint(&interaction.to),
                confidence: interaction.confidence.clone(),
                step: interaction.step.clone(),
                candidates: interaction.candidates,
            })
            .collect();
        if !edges.is_empty() {
            groups.push(KnowledgeInteractionGroupDto {
                kind: kind.to_string(),
                edges,
            });
        }
    }
    Ok(Json(groups))
}

fn endpoint(end: &InteractionEnd) -> KnowledgeEndpointDto {
    KnowledgeEndpointDto {
        repository_id: end.repository_id.clone(),
        path: end.path.clone(),
        line: end.line,
        symbol: end.symbol.clone(),
    }
}

fn related(ends: Vec<Related>) -> Vec<KnowledgeRelatedDto> {
    ends.into_iter()
        .map(|end| KnowledgeRelatedDto {
            repository_id: end.repository_id,
            path: end.path,
            line: end.line,
            name: end.name,
            confidence: end.confidence,
            step: end.step,
            candidates: end.candidates,
        })
        .collect()
}

/// The head of a `<base>..<head>` range, where it names one.
fn head_of(range: &str) -> Option<String> {
    let head = range.trim().split_once("..")?.1.trim();
    (!head.is_empty()).then(|| head.to_string())
}

/// Lines `first` to `last` of a file, 1-based and inclusive.
fn lines_of(text: &str, first: i64, last: i64) -> Option<String> {
    let first = first.max(1) as usize;
    let last = last.max(first as i64) as usize;
    let taken: Vec<&str> = text
        .lines()
        .skip(first - 1)
        .take(last + 1 - first)
        .collect();
    (!taken.is_empty()).then(|| taken.join("\n"))
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
