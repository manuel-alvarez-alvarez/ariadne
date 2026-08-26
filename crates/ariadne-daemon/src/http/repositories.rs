//! Repository endpoints.

use std::path::{Path as FsPath, PathBuf};

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;

use ariadne_api::repositories::{CreateRepositoryRequest, RepositoryDto, UpdateRepositoryRequest};
use ariadne_store::{NewRepository, RepositoryUpdate};

use super::AppState;
use super::convert::repository_dto;
use super::error::{ApiError, ApiResult};
use crate::gitwt::GitManager;

/// Create a repository.
#[utoipa::path(post, path = "/v1/repositories", tag = "repositories",
    request_body = CreateRepositoryRequest,
    responses(
        (status = 201, body = RepositoryDto),
        (status = 400, description = "not an absolute path, not a git work tree, or unknown branch"),
        (status = 409, description = "this path and base branch are already registered")
    ))]
pub async fn create(
    State(state): State<AppState>,
    Json(req): Json<CreateRepositoryRequest>,
) -> ApiResult<(StatusCode, Json<RepositoryDto>)> {
    let path = repo_path(&req.path)?;
    let base_branch = resolve_base_branch(&path, req.base_branch.as_deref()).await?;
    let repository = state
        .store
        .create_repository(NewRepository {
            path: req.path,
            base_branch,
            description: req.description,
            merge_strategy: req.merge_strategy.unwrap_or_default(),
        })
        .await?;
    Ok((StatusCode::CREATED, Json(repository_dto(repository))))
}

/// List repositories.
#[utoipa::path(get, path = "/v1/repositories", tag = "repositories",
    responses((status = 200, body = [RepositoryDto])))]
pub async fn list(State(state): State<AppState>) -> ApiResult<Json<Vec<RepositoryDto>>> {
    let repositories = state.store.list_repositories().await?;
    Ok(Json(repositories.into_iter().map(repository_dto).collect()))
}

/// Get a repository.
#[utoipa::path(get, path = "/v1/repositories/{id}", tag = "repositories",
    params(("id" = String, Path, description = "repository id")),
    responses((status = 200, body = RepositoryDto), (status = 404)))]
pub async fn get(
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
        (status = 400, description = "not an absolute path, not a git work tree, or unknown branch"),
        (status = 404),
        (status = 409, description = "this path and base branch are already registered")
    ))]
pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateRepositoryRequest>,
) -> ApiResult<Json<RepositoryDto>> {
    let current = state.store.get_repository(&id).await?;
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
    let repository = state
        .store
        .update_repository(
            &id,
            RepositoryUpdate {
                path: req.path,
                base_branch,
                description: req.description.map(|d| match d.is_empty() {
                    true => None,
                    false => Some(d),
                }),
                merge_strategy: req.merge_strategy,
            },
        )
        .await?;
    Ok(Json(repository_dto(repository)))
}

/// Delete a repository.
#[utoipa::path(delete, path = "/v1/repositories/{id}", tag = "repositories",
    params(("id" = String, Path, description = "repository id")),
    responses((status = 204), (status = 404)))]
pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    state.store.delete_repository(&id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Whatever git says about a checkout the caller named is a bad request:
/// they gave the path, and the answer is about the path.
fn bad(e: impl std::fmt::Display) -> ApiError {
    ApiError::bad_request(e.to_string())
}

/// An absolute path, the first of the checks goal creation makes before it
/// writes a repo down.
fn repo_path(raw: &str) -> Result<PathBuf, ApiError> {
    let path = PathBuf::from(raw);
    if !path.is_absolute() {
        return Err(ApiError::bad_request(format!(
            "repo path must be absolute: {raw}"
        )));
    }
    Ok(path)
}

/// The base branch to store: the given one once it is known to exist, or the
/// repo's current branch when none was given. Either way it must point at a
/// commit — a freshly `git init`ed repo has an unborn branch that worktrees
/// cannot be created from.
async fn resolve_base_branch(path: &FsPath, branch: Option<&str>) -> Result<String, ApiError> {
    let git = GitManager;
    git.validate_repo(path).await.map_err(bad)?;
    let base_branch = match branch {
        Some(b) => {
            if !git.branch_exists(path, b).await.map_err(bad)? {
                return Err(ApiError::bad_request(format!(
                    "branch {b} does not exist in {}",
                    path.display()
                )));
            }
            b.to_string()
        }
        None => git.current_branch(path).await.map_err(bad)?,
    };
    git.ensure_branch_has_commits(path, &base_branch)
        .await
        .map_err(bad)?;
    Ok(base_branch)
}
