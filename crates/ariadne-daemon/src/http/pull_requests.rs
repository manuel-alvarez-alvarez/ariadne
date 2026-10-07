//! The ledger and live forge search.
use super::{
    AppState,
    convert::pull_request_dto,
    error::{ApiError, ApiResult, Json},
};
use crate::forge::{ForgeClient, PullRequestRef, pulls};
use ariadne_api::pull_requests::{
    AddPullRequestRequest, PullRequestDto, PullRequestListQuery, PullRequestMatchDto,
};
use ariadne_store::{ForgeIntegration, PullRequestFilter};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::Deserialize;
use utoipa::IntoParams;

fn forge_error(error: String) -> ApiError {
    ApiError::new(StatusCode::BAD_GATEWAY, "forge_error", error)
}
async fn integration(state: &AppState, id: &str) -> ApiResult<ForgeIntegration> {
    state
        .store
        .get_repository(id)
        .await?
        .forge
        .filter(|f| f.enabled)
        .ok_or_else(|| ApiError::conflict("the repository's forge integration is off"))
}
fn slug(forge: &ForgeIntegration) -> String {
    format!("{}/{}/{}", forge.host, forge.owner, forge.name)
}

#[utoipa::path(get, path = "/v1/pull-requests", tag = "pull-requests", params(PullRequestListQuery), responses((status = 200, body = [PullRequestDto])))]
pub(super) async fn list(
    State(state): State<AppState>,
    Query(q): Query<PullRequestListQuery>,
) -> ApiResult<Json<Vec<PullRequestDto>>> {
    if q.role
        .as_deref()
        .is_some_and(|r| !matches!(r, "author" | "reviewer"))
    {
        return Err(ApiError::bad_request("role must be author or reviewer"));
    }
    let selected = q.state.as_deref().unwrap_or("open");
    if !matches!(selected, "open" | "merged" | "closed" | "all") {
        return Err(ApiError::bad_request(
            "state must be open, merged, closed or all",
        ));
    }
    let rows = state
        .store
        .list_pull_requests(PullRequestFilter {
            repository_id: q.repo,
            role: q.role,
            state: (selected != "all").then(|| selected.to_owned()),
        })
        .await?;
    Ok(Json(rows.into_iter().map(pull_request_dto).collect()))
}
#[utoipa::path(get, path = "/v1/pull-requests/{id}", tag = "pull-requests", params(("id" = String, Path)), responses((status = 200, body = PullRequestDto), (status = 404)))]
pub(super) async fn get(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<PullRequestDto>> {
    Ok(Json(pull_request_dto(
        state.store.get_pull_request(&id).await?,
    )))
}
#[utoipa::path(post, path = "/v1/pull-requests", tag = "pull-requests", request_body = AddPullRequestRequest, responses((status = 201, body = PullRequestDto), (status = 200, body = PullRequestDto), (status = 404), (status = 409)))]
pub(super) async fn add(
    State(state): State<AppState>,
    Json(req): Json<AddPullRequestRequest>,
) -> ApiResult<(StatusCode, Json<PullRequestDto>)> {
    let (forge, number) = match (req.repository_id, req.number, req.url) {
        (Some(id), Some(number), None) if number > 0 => (integration(&state, &id).await?, number),
        (None, None, Some(url)) => state
            .store
            .enabled_forge_integrations()
            .await?
            .into_iter()
            .find_map(|forge| {
                PullRequestRef::parse(&url, &forge).map(|reference| (forge, reference.number))
            })
            .ok_or_else(|| {
                ApiError::new(
                    StatusCode::NOT_FOUND,
                    "pull_request_not_found",
                    "no enabled repository matches this URL",
                )
            })?,
        _ => {
            return Err(ApiError::bad_request(
                "provide a URL or a repository_id and positive number",
            ));
        }
    };
    let pull = ForgeClient::for_repository(&state.launcher.cfg, &forge)
        .pull_request(&slug(&forge), number)
        .await
        .map_err(forge_error)?;
    if pull.number != number {
        return Err(forge_error(
            "the forge returned another request number".into(),
        ));
    }
    let (row, created) = pulls::record(&state.store, &forge, pull, "user", None, None)
        .await
        .map_err(forge_error)?;
    Ok((
        if created {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        },
        Json(pull_request_dto(row)),
    ))
}
#[utoipa::path(delete, path = "/v1/pull-requests/{id}", tag = "pull-requests", params(("id" = String, Path)), responses((status = 204), (status = 404), (status = 409)))]
pub(super) async fn delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    state.store.delete_pull_request(&id).await?;
    Ok(StatusCode::NO_CONTENT)
}
#[derive(Debug, Default, Deserialize, IntoParams)]
pub(super) struct RefreshQuery {
    repo: Option<String>,
}
#[utoipa::path(post, path = "/v1/pull-requests/refresh", tag = "pull-requests", params(RefreshQuery), responses((status = 202), (status = 409)))]
pub(super) async fn refresh(
    State(state): State<AppState>,
    Query(q): Query<RefreshQuery>,
) -> ApiResult<StatusCode> {
    match q.repo {
        Some(id) => {
            integration(&state, &id).await?;
            state.forge_poll.wake(&id);
        }
        None => {
            for forge in state.store.enabled_forge_integrations().await? {
                state.forge_poll.wake(&forge.repository_id);
            }
        }
    }
    Ok(StatusCode::ACCEPTED)
}
#[derive(Debug, Default, Deserialize, IntoParams)]
pub(super) struct SearchQuery {
    #[serde(default)]
    q: String,
}
#[utoipa::path(get, path = "/v1/repositories/{id}/pull-requests/search", tag = "pull-requests", params(("id" = String, Path), SearchQuery), responses((status = 200, body = [PullRequestMatchDto]), (status = 409)))]
pub(super) async fn search(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<SearchQuery>,
) -> ApiResult<Json<Vec<PullRequestMatchDto>>> {
    let forge = integration(&state, &id).await?;
    let rows = state
        .store
        .list_pull_requests(PullRequestFilter {
            repository_id: Some(id),
            ..Default::default()
        })
        .await?;
    let matches = ForgeClient::for_repository(&state.launcher.cfg, &forge)
        .search_pull_requests(&slug(&forge), &q.q)
        .await
        .map_err(forge_error)?;
    let mut result = Vec::new();
    for pull in matches {
        let Some(reference) =
            PullRequestRef::parse(&pull.url, &forge).filter(|r| r.number == pull.number)
        else {
            return Err(forge_error(
                "the forge returned a request outside this repository".into(),
            ));
        };
        result.push(PullRequestMatchDto {
            number: reference.number,
            role: pulls::role(&pull.author_login, &forge).into(),
            url: pull.url,
            title: pull.title,
            author_login: pull.author_login,
            tracked: rows.iter().any(|row| row.number == reference.number),
        });
    }
    Ok(Json(result))
}
