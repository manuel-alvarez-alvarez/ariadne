//! Skill endpoints.
//!
//! A skill is a document, so these are the four things one can do to a
//! document — write one, read them, replace the text, put a shipped one back —
//! plus the delete that only a skill of the user's own takes.

use axum::extract::{Path, State};
use axum::http::StatusCode;

use ariadne_api::skills::{CreateSkillRequest, SkillDto, UpdateSkillRequest};
use ariadne_store::NewSkill;

use super::AppState;
use super::convert::skill_dto;
use super::error::{ApiResult, Json};

/// Create a skill of the user's own.
///
/// It carries its own document: nothing Ariadne ships answers to its name, so
/// there is nothing behind it to fall back to.
#[utoipa::path(post, path = "/v1/skills", tag = "skills",
    request_body = CreateSkillRequest,
    responses(
        (status = 201, body = SkillDto),
        (status = 409, description = "name already exists")
    ))]
pub async fn create(
    State(state): State<AppState>,
    Json(req): Json<CreateSkillRequest>,
) -> ApiResult<(StatusCode, Json<SkillDto>)> {
    let skill = state
        .store
        .create_skill(NewSkill {
            name: req.name,
            document: req.document,
        })
        .await?;
    Ok((StatusCode::CREATED, Json(skill_dto(skill))))
}

/// List every skill, shipped and written, by name.
#[utoipa::path(get, path = "/v1/skills", tag = "skills",
    responses((status = 200, body = [SkillDto])))]
pub async fn list(State(state): State<AppState>) -> ApiResult<Json<Vec<SkillDto>>> {
    let skills = state.store.list_skills().await?;
    Ok(Json(skills.into_iter().map(skill_dto).collect()))
}

/// Get one skill by name.
#[utoipa::path(get, path = "/v1/skills/{name}", tag = "skills",
    params(("name" = String, Path, description = "skill name")),
    responses((status = 200, body = SkillDto), (status = 404)))]
pub async fn get(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<Json<SkillDto>> {
    Ok(Json(skill_dto(state.store.get_skill(&name).await?)))
}

/// Write a new document over a skill's.
#[utoipa::path(put, path = "/v1/skills/{name}", tag = "skills",
    request_body = UpdateSkillRequest,
    params(("name" = String, Path, description = "skill name")),
    responses((status = 200, body = SkillDto), (status = 404)))]
pub async fn update(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(req): Json<UpdateSkillRequest>,
) -> ApiResult<Json<SkillDto>> {
    let skill = match req.document {
        Some(document) => state.store.set_skill_document(&name, &document).await?,
        None => state.store.get_skill(&name).await?,
    };
    Ok(Json(skill_dto(skill)))
}

/// Delete a skill of the user's own (409 for a built-in, or while loaded).
#[utoipa::path(delete, path = "/v1/skills/{name}", tag = "skills",
    params(("name" = String, Path, description = "skill name")),
    responses((status = 204), (status = 404), (status = 409)))]
pub async fn delete(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<StatusCode> {
    state.store.delete_skill(&name).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Put a built-in skill back on the document Ariadne ships.
#[utoipa::path(post, path = "/v1/skills/{name}/document/reset", tag = "skills",
    params(("name" = String, Path, description = "skill name")),
    responses((status = 200, body = SkillDto), (status = 404), (status = 409)))]
pub async fn reset_document(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<Json<SkillDto>> {
    Ok(Json(skill_dto(state.store.reset_skill(&name).await?)))
}
