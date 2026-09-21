//! Knowledge base endpoints: a repository's index status, its reindex, the
//! six questions every seat asks the index, and the interactions between
//! one repository and the others.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};

use ariadne_api::knowledge::{
    KnowledgeContextDto, KnowledgeContextMoreDto, KnowledgeDetail, KnowledgeEdgeDto,
    KnowledgeEndpointDto, KnowledgeFailureDto, KnowledgeGraphConfidence, KnowledgeGraphDto,
    KnowledgeGraphEdgeDto, KnowledgeGraphNodeDto, KnowledgeGraphQuery, KnowledgeHitDto,
    KnowledgeImpactCallerDto, KnowledgeImpactDto, KnowledgeImpactQuery,
    KnowledgeInteractionGroupDto, KnowledgeInteractionsQuery, KnowledgeLanguageDto,
    KnowledgeMapDto, KnowledgeMapQuery, KnowledgeOutlineEntryDto, KnowledgeOutlineQuery,
    KnowledgePathDto, KnowledgePathHopDto, KnowledgePathQuery, KnowledgeRefDto,
    KnowledgeRelatedDto, KnowledgeSearchQuery, KnowledgeState, KnowledgeStatusDto,
    KnowledgeSymbolDto, KnowledgeSymbolQuery,
};
use ariadne_core::Seat;
use ariadne_knowledge::store::{CONTEXT_LIMIT, INTERACTION_KINDS};
use ariadne_knowledge::{InteractionEnd, KnowledgeStore, Readiness, Related, SearchQuery, index};
use ariadne_store::{Repository, author_branch};

use super::AppState;
use super::caller::{CallCtx, call_ctx};
use super::error::{ApiError, ApiResult, Json};
use crate::knowledge::ref_is_gone;

/// What a call is refused with while `knowledge_enabled = false`.
const DISABLED: &str =
    "the knowledge base is disabled: set knowledge_enabled = true in config.toml";

#[utoipa::path(get, path = "/v1/repositories/{id}/knowledge", tag = "knowledge",
    params(("id" = String, Path, description = "repository id")),
    responses((status = 200, body = KnowledgeStatusDto), (status = 404),
              (status = 500, description = "the knowledge store failed")))]
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
              (status = 409, description = "the knowledge base is disabled"),
              (status = 500, description = "the knowledge store failed")))]
pub(super) async fn reindex(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<(StatusCode, Json<KnowledgeStatusDto>)> {
    let repository = state.store.get_repository(&id).await?;
    enabled(&state)?;
    state
        .knowledge
        .reindex(&id, &repository.base_branch)
        .await
        .map_err(internal)?;
    Ok((StatusCode::ACCEPTED, Json(status_dto(&state, &id).await?)))
}

#[utoipa::path(get, path = "/v1/knowledge/search", tag = "knowledge",
    params(KnowledgeSearchQuery),
    responses((status = 200, body = [KnowledgeHitDto]), (status = 400), (status = 404),
              (status = 409, description = "the knowledge base is disabled, or the index of the ref is not ready"),
              (status = 500, description = "the knowledge store or git failed")))]
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
        ready(knowledge, repository, &git_ref).await?;
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
        .map_err(internal)?;
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
              (status = 409, description = "the knowledge base is disabled, or the index of the ref is not ready"),
              (status = 500, description = "the knowledge store or git failed")))]
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
    ready(knowledge, &repository, &git_ref).await?;
    let entries = knowledge
        .outline(&repository.id, &git_ref, &query.path)
        .await
        .map_err(internal)?
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
              (status = 409, description = "the knowledge base is disabled, or the index of the ref is not ready"),
              (status = 500, description = "the knowledge store or git failed")))]
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
        ready(knowledge, repository, &git_ref).await?;
        scopes.push((repository.id.clone(), git_ref));
    }
    let definitions = knowledge
        .definitions(query.name.trim(), &scopes)
        .await
        .map_err(internal)?;
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
                            .map_err(internal)?,
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
                    .map_err(internal)?,
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
              (status = 409, description = "the knowledge base is disabled, or the index of the ref is not ready"),
              (status = 500, description = "the knowledge store or git failed")))]
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
    if matches!((symbol, diff), (Some(_), Some(_)) | (None, None)) {
        return Err(ApiError::bad_request(
            "pass symbol or diff, and only one of them",
        ));
    }
    let (ends, diff_lines) = match diff {
        Some(range) => {
            let Some(ends) = ends_of(range.trim()) else {
                return Err(ApiError::bad_request(format!(
                    "a diff is `<base>..<head>`, not {:?}",
                    range.trim()
                )));
            };
            let repo = std::path::Path::new(&repository.path);
            let lines = match index::changed_lines(repo, range.trim()).await {
                Ok(lines) => lines,
                // A wrong range is the caller's; a git that failed on a
                // right one is the daemon's.
                Err(e) => {
                    return Err(match an_end_is_gone(repo, ends).await {
                        true => ApiError::bad_request(e.to_string()),
                        false => internal(e),
                    });
                }
            };
            (Some(ends), Some(lines))
        }
        None => (None, None),
    };
    // A diff names the ref it is about: its lines are the head's, so the
    // definitions have to be the head's too.
    let head = ends
        .as_ref()
        .and_then(|[_, head]| (!head.is_empty()).then(|| (*head).to_string()));
    let named = match &head {
        Some(head) => {
            let direct = knowledge
                .ref_commit(&repository.id, head)
                .await
                .map_err(internal)?;
            let (indexed, commit) = match direct {
                Some(commit) => (head.to_string(), commit),
                // `git diff` also accepts a commit as its head. A ref at that
                // commit has the same definitions, so it is safe to use.
                None => {
                    let mut matching = None;
                    for git_ref in knowledge
                        .ref_names(&repository.id)
                        .await
                        .map_err(internal)?
                    {
                        if knowledge
                            .ref_commit(&repository.id, &git_ref)
                            .await
                            .map_err(internal)?
                            .as_deref()
                            == Some(head)
                        {
                            matching = Some((git_ref, head.to_string()));
                            break;
                        }
                    }
                    match matching {
                        Some(git_ref) => git_ref,
                        None => {
                            ready(knowledge, &repository, head).await?;
                            let commit = knowledge
                                .ref_commit(&repository.id, head)
                                .await
                                .map_err(internal)?
                                .ok_or_else(|| {
                                    ApiError::conflict(format!(
                                        "the index of {head} changed while it became ready; try again"
                                    ))
                                })?;
                            (head.to_string(), commit)
                        }
                    }
                }
            };
            if let Some(git_ref) = query
                .git_ref
                .as_deref()
                .filter(|git_ref| !git_ref.is_empty())
            {
                let named_commit = knowledge
                    .ref_commit(&repository.id, git_ref)
                    .await
                    .map_err(internal)?;
                let named_commit = match named_commit {
                    Some(commit) => commit,
                    None => {
                        ready(knowledge, &repository, git_ref).await?;
                        knowledge
                            .ref_commit(&repository.id, git_ref)
                            .await
                            .map_err(internal)?
                            .ok_or_else(|| {
                                ApiError::conflict(format!(
                                    "the index of {git_ref} changed while it became ready; try again"
                                ))
                            })?
                    }
                };
                if named_commit != commit {
                    return Err(ApiError::bad_request(format!(
                        "diff head {head} does not match git_ref {git_ref}"
                    )));
                }
                Some(git_ref.to_string())
            } else {
                Some(indexed)
            }
        }
        None => query.git_ref.clone(),
    };
    let git_ref = ref_for(&state, knowledge, &ctx, &repository, named.as_deref()).await?;
    ready(knowledge, &repository, &git_ref).await?;
    // One or the other: a call that named both would get an answer to a
    // question it did not ask.
    let changed: Vec<(i64, Related)> = match (symbol, diff) {
        (Some(name), None) => knowledge
            .definitions(name.trim(), &[(repository.id.clone(), git_ref.clone())])
            .await
            .map_err(internal)?
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
        (None, Some(_)) => {
            let lines = diff_lines.as_ref().expect("a diff has changed lines");
            knowledge
                .symbols_in_lines(&repository.id, &git_ref, lines)
                .await
                .map_err(internal)?
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
            .map_err(internal)?;
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
              (status = 409, description = "the knowledge base is disabled, or the index of the ref is not ready"),
              (status = 500, description = "the knowledge store or git failed")))]
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
    ready(knowledge, &repository, &git_ref).await?;
    let starts = knowledge
        .definitions(
            query.from.trim(),
            &[(repository.id.clone(), git_ref.clone())],
        )
        .await
        .map_err(internal)?;

    // The destination may sit across an edge in another repository. Read
    // each other repository at its own ref, as rule 20 does. Another
    // repository whose ref is not ready holds no end: one repository with no
    // index must not refuse every path of every other one.
    let repositories = state.store.list_repositories().await?;
    let mut end_scopes = Vec::with_capacity(repositories.len());
    for candidate in &repositories {
        if candidate.id == repository.id {
            end_scopes.push((candidate.id.clone(), git_ref.clone()));
            continue;
        }
        let candidate_ref = ref_for(&state, knowledge, &ctx, candidate, None).await?;
        let readiness = knowledge
            .readiness(&candidate.id, &candidate_ref)
            .await
            .map_err(internal)?;
        if readiness == Readiness::Ready {
            end_scopes.push((candidate.id.clone(), candidate_ref));
        }
    }
    let ends = knowledge
        .definitions(query.to.trim(), &end_scopes)
        .await
        .map_err(internal)?;
    let hops = knowledge
        .path(&starts, &ends, query.depth())
        .await
        .map_err(internal)?
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
              (status = 409, description = "the knowledge base is disabled, or the index of the ref is not ready"),
              (status = 500, description = "the knowledge store or git failed")))]
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
    ready(knowledge, &repository, &git_ref).await?;
    let toward = query.path.as_deref().filter(|path| !path.trim().is_empty());
    let map = knowledge
        .map(
            &repository.id,
            &git_ref,
            toward,
            query.budget().max(1) as usize,
        )
        .await
        .map_err(internal)?;
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
              (status = 409, description = "the knowledge base is disabled, or the index of the ref is not ready"),
              (status = 500, description = "the knowledge store or git failed")))]
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
    ready(knowledge, &repository, &git_ref).await?;
    let graph = knowledge
        .graph(&repository.id, &git_ref, query.limit() as usize)
        .await
        .map_err(internal)?;
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
              (status = 409, description = "the knowledge base is disabled, or the index of the ref is not ready"),
              (status = 500, description = "the knowledge store or git failed")))]
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
    ready(knowledge, &repository, &git_ref).await?;
    let found = knowledge
        .interactions(&repository.id, &git_ref)
        .await
        .map_err(internal)?;
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

/// The two ends of a `<base>..<head>` range, or `None` for any other form:
/// no `..`, the three dots of a merge-base diff, an end with a space, or an
/// end that could read as a flag. Git reads each end as it is given, so none
/// is trimmed here. An empty end is `HEAD`, as git reads it.
fn ends_of(range: &str) -> Option<[&str; 2]> {
    let (base, head) = range.split_once("..")?;
    let right = |end: &str| {
        !end.starts_with(['-', '.']) && !end.ends_with('.') && !end.contains(char::is_whitespace)
    };
    (right(base) && right(head)).then_some([base, head])
}

/// Whether an end of a diff range names no commit of `repo`, which is the
/// caller's error. Git fails on a right range too — a repository it cannot
/// read, a git that did not start — and that is no wrong request.
async fn an_end_is_gone(repo: &std::path::Path, ends: [&str; 2]) -> bool {
    for end in ends {
        if !end.is_empty() && ref_is_gone(repo, end).await {
            return true;
        }
    }
    false
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

/// A failure of the store or of git: the daemon's own, and no fault of the
/// request. A 5xx, so that no client reads it as a wrong argument.
fn internal(e: anyhow::Error) -> ApiError {
    tracing::error!(error = %format!("{e:#}"), "the knowledge base failed");
    ApiError::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal_error",
        format!("the knowledge base failed: {e:#}"),
    )
}

/// Refuse a read of a ref that cannot answer it: one with no index yet, one
/// in its first index run, or one whose last run failed. The refusal says
/// which, and names the repository and the ref. An empty answer from such a
/// ref would read as a fact. A ref that has an index answers while a run
/// updates it.
async fn ready(
    knowledge: &KnowledgeStore,
    repository: &Repository,
    git_ref: &str,
) -> ApiResult<()> {
    let reason = match knowledge
        .readiness(&repository.id, git_ref)
        .await
        .map_err(internal)?
    {
        Readiness::Ready => return Ok(()),
        Readiness::NoIndex => "no index run of this ref has started".to_string(),
        Readiness::FirstIndex => {
            "the first index run of this ref is in progress, try again later".to_string()
        }
        Readiness::Failed(error) => format!("the last index run of this ref failed: {error}"),
    };
    Err(ApiError::new(
        StatusCode::CONFLICT,
        "knowledge_not_ready",
        format!(
            "the index of {git_ref} in repository {} ({}) is not ready: {reason}",
            repository.path, repository.id
        ),
    ))
}

/// The ref a caller reads a repository at: the one it named, else its own.
///
/// An author task session's own is its author branch, in the task's
/// repository. A reviewer reads the first author branch. Until that branch
/// is indexed, the base branch is what answers. Everyone else reads the base
/// branch.
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
    if let Some(session) = ctx.session.as_ref()
        && let Some(task_id) = session.task_id.as_deref()
    {
        let task = state.store.get_task(task_id).await?;
        let branch = if session.seat() == Seat::Author {
            match session.task_agent_id.as_deref() {
                Some(agent_id) => {
                    let author = state.store.get_task_agent(agent_id).await?;
                    author_branch(&task.branch, author.ordinal)
                }
                None => task.branch.clone(),
            }
        } else {
            task.branch.clone()
        };
        if task.repo_id == repository.id
            && knowledge
                .ref_commit(&repository.id, &branch)
                .await
                .map_err(internal)?
                .is_some()
        {
            return Ok(branch);
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
            failures: Vec::new(),
        });
    };
    let status = knowledge.status(repository_id).await.map_err(internal)?;
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
        failures: status
            .failures
            .into_iter()
            .map(|failure| KnowledgeFailureDto {
                git_ref: failure.git_ref,
                error: failure.error,
            })
            .collect(),
    })
}
