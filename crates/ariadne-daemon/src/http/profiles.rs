//! Profile endpoints.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;

use ariadne_api::profiles::{
    CreateProfileRequest, ProfileDto, ProfileListQuery, ProfilePromptDto,
    UpdateProfilePromptRequest, UpdateProfileRequest,
};
use ariadne_store::{NewProfile, ProfileUpdate, Store, parse_prompt_kind};

use super::AppState;
use super::convert::{profile_dto, profile_prompt_dto};
use super::error::{ApiError, ApiResult};

/// The id behind a path segment: every profile endpoint takes an id or a
/// unique name, as the `to` of a message does.
async fn resolve(store: &Store, spec: &str) -> ApiResult<String> {
    Ok(store.resolve_profile(spec).await?.id)
}

/// Create a profile.
///
/// It starts on the prompts of its role, every one of them the default: a
/// briefing is given to it afterwards, one `PUT` per kind.
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
    let id = resolve(&state.store, &id).await?;
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
    let id = resolve(&state.store, &id).await?;
    state.store.delete_profile(&id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// The profile's briefing prompts, in briefing order: each one as it takes
/// effect, saying whether that is the default of its kind or a text set here.
#[utoipa::path(get, path = "/v1/profiles/{id}/prompts", tag = "profiles",
    params(("id" = String, Path, description = "profile id or name")),
    responses((status = 200, body = [ProfilePromptDto]), (status = 404)))]
pub async fn list_prompts(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Vec<ProfilePromptDto>>> {
    let id = resolve(&state.store, &id).await?;
    let prompts = state.store.list_profile_prompts(&id).await?;
    Ok(Json(prompts.into_iter().map(profile_prompt_dto).collect()))
}

/// Set the text of one prompt, which is what makes it the profile's own. A
/// template may drop every `{placeholder}` of its kind, but not name one the
/// kind has no value for: that token would reach the agent as literal text, so
/// it is refused here.
#[utoipa::path(put, path = "/v1/profiles/{id}/prompts/{kind}", tag = "profiles",
    request_body = UpdateProfilePromptRequest,
    params(
        ("id" = String, Path, description = "profile id or name"),
        ("kind" = String, Path, description = "prompt kind, e.g. engineer_briefing"),
    ),
    responses(
        (status = 200, body = ProfilePromptDto),
        (status = 400, description = "unknown kind, a kind the profile's role does not own, \
                                      or a placeholder the kind cannot fill in"),
        (status = 404)
    ))]
pub async fn update_prompt(
    State(state): State<AppState>,
    Path((id, kind)): Path<(String, String)>,
    Json(req): Json<UpdateProfilePromptRequest>,
) -> ApiResult<Json<ProfilePromptDto>> {
    let id = resolve(&state.store, &id).await?;
    let kind = parse_prompt_kind(&kind)?;
    let prompt = state
        .store
        .update_profile_prompt(&id, kind, &req.content)
        .await?;
    Ok(Json(profile_prompt_dto(prompt)))
}

/// Put one prompt back on the default of its kind, dropping the text set on
/// the profile.
#[utoipa::path(post, path = "/v1/profiles/{id}/prompts/{kind}/reset", tag = "profiles",
    params(
        ("id" = String, Path, description = "profile id or name"),
        ("kind" = String, Path, description = "prompt kind, e.g. engineer_briefing"),
    ),
    responses(
        (status = 200, body = ProfilePromptDto),
        (status = 400, description = "unknown kind, or a kind the profile's role does not own"),
        (status = 404)
    ))]
pub async fn reset_prompt(
    State(state): State<AppState>,
    Path((id, kind)): Path<(String, String)>,
) -> ApiResult<Json<ProfilePromptDto>> {
    let id = resolve(&state.store, &id).await?;
    let kind = parse_prompt_kind(&kind)?;
    let prompt = state.store.reset_profile_prompt(&id, kind).await?;
    Ok(Json(profile_prompt_dto(prompt)))
}

/// Put the profile's system prompt back on the default of its role.
#[utoipa::path(post, path = "/v1/profiles/{id}/system-prompt/reset", tag = "profiles",
    params(("id" = String, Path, description = "profile id or name")),
    responses((status = 200, body = ProfileDto), (status = 404)))]
pub async fn reset_system_prompt(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<ProfileDto>> {
    let id = resolve(&state.store, &id).await?;
    let profile = state.store.reset_system_prompt(&id).await?;
    Ok(Json(profile_dto(profile)))
}
