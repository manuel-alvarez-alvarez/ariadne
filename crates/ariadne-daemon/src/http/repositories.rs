//! Repository endpoints.

use std::path::{Path as FsPath, PathBuf};

use axum::extract::{Path, State};
use axum::http::StatusCode;

use ariadne_api::repositories::{
    CreateRepositoryRequest, ForgeUpdate, RepositoryDto, UpdateRepositoryRequest,
};
use ariadne_core::PermissionMode;
use ariadne_store::{NewRepository, RepositoryUpdate, SetForgeIntegration};

use super::AppState;
use super::convert::repository_dto;
use super::error::{ApiError, ApiResult, Json};
use super::permissions::ai_needs_the_model;
use crate::forge::{self, ForgeClient};
use crate::gitwt::GitManager;

/// Create a repository.
#[utoipa::path(post, path = "/v1/repositories", tag = "repositories",
    request_body = CreateRepositoryRequest,
    responses(
        (status = 201, body = RepositoryDto),
        (status = 400, description = "not an absolute path, not a git work tree, \
                                      an unknown branch, or a forge pin that is no catalog model"),
        (status = 409, description = "this path and base branch are already registered, \
                                      `ai` was asked for while the AI permission model is off, \
                                      or the forge integration cannot be enabled")
    ))]
pub(super) async fn create(
    State(state): State<AppState>,
    Json(req): Json<CreateRepositoryRequest>,
) -> ApiResult<(StatusCode, Json<RepositoryDto>)> {
    if req.permission_mode == Some(PermissionMode::Ai) {
        ai_needs_the_model(&state).await?;
    }
    let path = repo_path(&req.path)?;
    let base_branch = resolve_base_branch(&path, req.base_branch.as_deref()).await?;
    let cfg = &state.launcher.cfg;
    let detected = forge::detect(cfg, &path).await;
    let row = forge::merged("", None, detected.as_ref());
    let row = forge_asked(&state, "", row, req.forge.as_ref()).await?;
    // One transaction: a forge the store refuses leaves no repository.
    let repository = state
        .store
        .create_repository_with_forge(
            NewRepository {
                default_workflow: req.default_workflow,
                path: req.path,
                base_branch,
                description: req.description,
                permission_mode: req.permission_mode,
                supervise_posts: req.supervise_posts,
            },
            row,
        )
        .await?;
    state.forge_poll.changed(&repository.id).await;
    Ok((StatusCode::CREATED, Json(repository_dto(repository))))
}

/// List repositories.
#[utoipa::path(get, path = "/v1/repositories", tag = "repositories",
    responses((status = 200, body = [RepositoryDto])))]
pub(super) async fn list(State(state): State<AppState>) -> ApiResult<Json<Vec<RepositoryDto>>> {
    let repositories = state.store.list_repositories().await?;
    Ok(Json(repositories.into_iter().map(repository_dto).collect()))
}

/// Get a repository.
#[utoipa::path(get, path = "/v1/repositories/{id}", tag = "repositories",
    params(("id" = String, Path, description = "repository id")),
    responses((status = 200, body = RepositoryDto), (status = 404)))]
pub(super) async fn get(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<RepositoryDto>> {
    Ok(Json(repository_dto(state.store.get_repository(&id).await?)))
}

/// Update a repository.
#[utoipa::path(put, path = "/v1/repositories/{id}", tag = "repositories",
    request_body = UpdateRepositoryRequest,
    params(("id" = String, Path, description = "repository id")),
    responses(
        (status = 200, body = RepositoryDto),
        (status = 400, description = "not an absolute path, not a git work tree, \
                                      an unknown branch, or a forge pin that is no catalog model"),
        (status = 404),
        (status = 409, description = "this path and base branch are already registered, \
                                      `ai` was asked for while the AI permission model is off, \
                                      or the forge integration cannot be enabled")
    ))]
pub(super) async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateRepositoryRequest>,
) -> ApiResult<Json<RepositoryDto>> {
    let current = state.store.get_repository(&id).await?;
    if req.permission_mode == Some(PermissionMode::Ai) {
        ai_needs_the_model(&state).await?;
    }
    // Only re-validated when the checkout or the branch actually moves: a
    // description edit has no business failing because the repo sits on a
    // disk that is not mounted right now.
    let base_branch = match (&req.path, &req.base_branch) {
        (None, None) => None,
        (path, branch) => {
            let raw = path.as_deref().unwrap_or(&current.path);
            let fs_path = repo_path(raw)?;
            let branch = branch.as_deref().unwrap_or(&current.base_branch);
            Some(resolve_base_branch(&fs_path, Some(branch)).await?)
        }
    };
    // The forge is read again on every edit: the remote may have moved since
    // the last one, and a moved path is another checkout altogether.
    let checkout = repo_path(req.path.as_deref().unwrap_or(&current.path))?;
    let detected = forge::detect(&state.launcher.cfg, &checkout).await;
    let row = forge::merged(&id, current.forge.as_ref(), detected.as_ref());
    let row = forge_asked(&state, &id, row, req.forge.as_ref()).await?;
    // One transaction: a forge the store refuses leaves the repository as
    // it was.
    let write = forge::change(&current, row);
    let repository = state
        .store
        .update_repository_with_forge(
            &id,
            RepositoryUpdate {
                default_workflow: req.default_workflow,
                path: req.path,
                base_branch,
                description: req.description.map(|d| match d.is_empty() {
                    true => None,
                    false => Some(d),
                }),
                permission_mode: req.permission_mode,
                supervise_posts: req.supervise_posts,
            },
            write,
        )
        .await?;
    forge::hooks::remove_replaced(
        &state.launcher.cfg,
        current.forge.as_ref(),
        repository.forge.as_ref(),
    )
    .await;
    // A disabled integration ends the work on its requests: the fetch's
    // sync wakes the scheduler for each (026).
    state.forge_poll.changed(&id).await;
    Ok(Json(repository_dto(repository)))
}

/// Delete a repository.
#[utoipa::path(delete, path = "/v1/repositories/{id}", tag = "repositories",
    params(("id" = String, Path, description = "repository id")),
    responses((status = 204), (status = 404)))]
pub(super) async fn delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    let previous = state.store.get_repository(&id).await?;
    state.store.delete_repository(&id).await?;
    forge::hooks::remove_replaced(&state.launcher.cfg, previous.forge.as_ref(), None).await;
    Ok(StatusCode::NO_CONTENT)
}

/// Whatever git says about a checkout the caller named is a bad request:
/// they gave the path, and the answer is about the path.
pub(super) fn bad(e: impl std::fmt::Display) -> ApiError {
    ApiError::bad_request(e.to_string())
}

/// An absolute path, the first of the checks goal creation makes before it
/// writes a repo down.
pub(super) fn repo_path(raw: &str) -> Result<PathBuf, ApiError> {
    let path = PathBuf::from(raw);
    if !path.is_absolute() {
        return Err(ApiError::bad_request(format!(
            "repo path must be absolute: {raw}"
        )));
    }
    Ok(path)
}

/// The base branch to store: the given one once it is known to the repository,
/// or the repo's current branch when none was given.
///
/// A freshly `git init`ed repository is registered like any other. Its branch
/// is unborn — HEAD names it and no ref exists yet — so the branch the caller
/// asks for counts as known when it is the one HEAD is on, and the first task
/// to commit is what gives the repository its first commit.
async fn resolve_base_branch(path: &FsPath, branch: Option<&str>) -> Result<String, ApiError> {
    let git = GitManager;
    git.validate_repo(path).await.map_err(bad)?;
    match branch {
        Some(b) => {
            let known = git.branch_exists(path, b).await.map_err(bad)?
                || git.current_branch(path).await.ok().as_deref() == Some(b);
            if !known {
                return Err(ApiError::bad_request(format!(
                    "branch {b} does not exist in {}",
                    path.display()
                )));
            }
            Ok(b.to_string())
        }
        None => git.current_branch(path).await.map_err(bad),
    }
}

/// The forge row a request leaves: `detected` — the remote as it reads now,
/// over what the row keeps — with the request's change on top. Refused
/// before anything is written: a pin that is no catalog model with 400, and
/// with 409 an enable the forge's CLI cannot back, or one another row of the
/// same forge repository already holds.
async fn forge_asked(
    state: &AppState,
    repo_id: &str,
    detected: Option<SetForgeIntegration>,
    update: Option<&ForgeUpdate>,
) -> ApiResult<Option<SetForgeIntegration>> {
    let Some(update) = update.filter(|u| u.enabled.is_some()) else {
        return Ok(detected);
    };
    let Some(mut row) = detected else {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "forge_unavailable",
            "this checkout has no forge remote to integrate with: Ariadne reads \
             `origin`, or the only remote where there is no `origin`",
        ));
    };
    match update.enabled {
        Some(true) if !row.enabled => {
            state
                .store
                .forge_enabled_elsewhere(repo_id, &row.host, &row.owner, &row.name)
                .await?;
            let client = ForgeClient::new(&state.launcher.cfg, row.kind);
            let refused = |message: String| {
                ApiError::new(StatusCode::CONFLICT, "forge_unauthenticated", message)
            };
            client.auth_status(&row.host).await.map_err(refused)?;
            row.login = Some(client.whoami(&row.host).await.map_err(refused)?);
            row.enabled = true;
        }
        Some(false) => {
            row.enabled = false;
            row.login = None;
        }
        _ => {}
    }
    Ok(Some(row))
}
