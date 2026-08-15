//! Profile endpoints.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::Deserialize;
use utoipa::IntoParams;

use ariadne_api::profiles::{CreateProfileRequest, ProfileDto, UpdateProfileRequest};
use ariadne_core::Role;
use ariadne_store::{NewProfile, ProfileUpdate};

use super::AppState;
use super::convert::profile_dto;
use super::error::{ApiError, ApiResult};

#[derive(Debug, Default, Deserialize, IntoParams)]
pub struct ProfileListQuery {
    /// Filter by role.
    pub role: Option<Role>,
}

/// Create a profile.
#[utoipa::path(post, path = "/v1/profiles", tag = "profiles",
    request_body = CreateProfileRequest,
    responses(
        (status = 201, body = ProfileDto),
        (status = 409, description = "name already exists")
    ))]
pub async fn create(
    State(state): State<AppState>,
    Json(req): Json<CreateProfileRequest>,
) -> ApiResult<(StatusCode, Json<ProfileDto>)> {
    let profile = state
        .store
        .create_profile(NewProfile {
            name: req.name,
            role: req.role,
            agent_kind: req.agent_kind,
            model: req.model,
            system_prompt: req.system_prompt,
            extra_flags: req.extra_flags,
        })
        .await?;
    Ok((StatusCode::CREATED, Json(profile_dto(profile))))
}

/// List profiles.
#[utoipa::path(get, path = "/v1/profiles", tag = "profiles",
    params(ProfileListQuery),
    responses((status = 200, body = [ProfileDto])))]
pub async fn list(
    State(state): State<AppState>,
    Query(q): Query<ProfileListQuery>,
) -> ApiResult<Json<Vec<ProfileDto>>> {
    let profiles = state.store.list_profiles(q.role).await?;
    Ok(Json(profiles.into_iter().map(profile_dto).collect()))
}

/// Get a profile by id or unique name.
#[utoipa::path(get, path = "/v1/profiles/{id}", tag = "profiles",
    params(("id" = String, Path, description = "profile id or name")),
    responses((status = 200, body = ProfileDto), (status = 404)))]
pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<ProfileDto>> {
    Ok(Json(profile_dto(state.store.resolve_profile(&id).await?)))
}

/// Update a profile.
#[utoipa::path(put, path = "/v1/profiles/{id}", tag = "profiles",
    request_body = UpdateProfileRequest,
    params(("id" = String, Path, description = "profile id")),
    responses((status = 200, body = ProfileDto), (status = 404)))]
pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateProfileRequest>,
) -> ApiResult<Json<ProfileDto>> {
    let profile = state
        .store
        .update_profile(
            &id,
            ProfileUpdate {
                name: req.name,
                agent_kind: match req.agent_kind.as_deref() {
                    None => None,
                    Some("auto") => Some(None),
                    Some(kind) => Some(Some(kind.parse().map_err(ApiError::bad_request)?)),
                },
                model: req.model.map(|m| match m.as_str() {
                    "" | "default" => None,
                    _ => Some(m),
                }),
                system_prompt: req.system_prompt,
                extra_flags: req.extra_flags,
            },
        )
        .await?;
    Ok(Json(profile_dto(profile)))
}

/// Delete a profile (409 while referenced).
#[utoipa::path(delete, path = "/v1/profiles/{id}", tag = "profiles",
    params(("id" = String, Path, description = "profile id")),
    responses((status = 204), (status = 404), (status = 409)))]
pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    state.store.delete_profile(&id).await?;
    Ok(StatusCode::NO_CONTENT)
}
