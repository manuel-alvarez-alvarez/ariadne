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
use super::error::ApiResult;
use super::pins::{self, Repin, Standing};

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
    // A profile chooses the same way everything else does, and its "nothing
    // chosen" is auto: no agent CLI, and so no model of one either — which is
    // also no model an effort of its own could be run at.
    let pin = pins::chosen(
        req.model.as_deref(),
        req.effort.as_deref(),
        Standing::auto(),
    )
    .await?;
    let profile = state
        .store
        .create_profile(NewProfile {
            name: req.name,
            role: req.role,
            agent_kind: pin.as_ref().map(|p| p.agent_kind),
            model: pin.as_ref().and_then(|p| p.model.clone()),
            effort: pin.and_then(|p| p.effort),
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
    // Read rather than resolved to an id alone: an effort written on its own
    // is run at the model this profile is on, so that is what it is checked
    // against.
    let current = state.store.resolve_profile(&id).await?;
    // A model moves all three columns together — a profile is on one model, at
    // one effort of it, and clearing the model puts the profile back on auto
    // with neither of the other two left behind. An effort on its own moves
    // only itself, leaving the model it is run at where it is.
    let (agent_kind, model, effort) = match pins::rechosen(
        req.model.as_deref(),
        req.effort.as_deref(),
        Standing {
            agent_kind: current.agent_kind(),
            model: current.model.as_deref(),
        },
    )
    .await?
    {
        Repin::Untouched => (None, None, None),
        Repin::Profile => (Some(None), Some(None), Some(None)),
        Repin::To(pin) => (
            Some(Some(pin.agent_kind)),
            Some(pin.model),
            Some(pin.effort),
        ),
        Repin::Effort(effort) => (None, None, Some(effort)),
    };
    let profile = state
        .store
        .update_profile(
            &current.id,
            ProfileUpdate {
                name: req.name,
                agent_kind,
                model,
                effort,
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
