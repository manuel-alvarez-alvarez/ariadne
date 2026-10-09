//! Workflow endpoints.
//!
//! A workflow is a document, so these mirror the skill endpoints — write one,
//! read them, replace the text, put a shipped one back, plus the delete that
//! only a workflow of the user's own takes — and add the one a skill has no
//! use for: parsing a document without saving it anywhere.

use ariadne_core::workflow;
use axum::extract::{Path, State};
use axum::http::StatusCode;

use ariadne_api::workflows::{
    CreateWorkflowRequest, ParseWorkflowRequest, ParsedWorkflowDto, UpdateWorkflowRequest,
    WorkflowDto,
};
use ariadne_store::{NewWorkflow, StoreError};

use super::AppState;
use super::convert::{workflow_dto, workflow_step_dto};
use super::error::{ApiError, ApiResult, Json};

/// Create a workflow of the user's own. It carries its own document: nothing
/// Ariadne ships answers to its name, so there is nothing behind it to fall
/// back to.
#[utoipa::path(post, path = "/v1/workflows", tag = "workflows",
    request_body = CreateWorkflowRequest,
    responses(
        (status = 201, body = WorkflowDto),
        (status = 409, description = "name already exists, or the document is invalid")
    ))]
pub(super) async fn create(
    State(state): State<AppState>,
    Json(req): Json<CreateWorkflowRequest>,
) -> ApiResult<(StatusCode, Json<WorkflowDto>)> {
    let workflow = state
        .store
        .create_workflow(NewWorkflow {
            name: req.name,
            document: req.document,
        })
        .await
        .map_err(workflow_error)?;
    Ok((StatusCode::CREATED, Json(workflow_dto(workflow))))
}

/// List every workflow, shipped and written, by name.
#[utoipa::path(get, path = "/v1/workflows", tag = "workflows",
    responses((status = 200, body = [WorkflowDto])))]
pub(super) async fn list(State(state): State<AppState>) -> ApiResult<Json<Vec<WorkflowDto>>> {
    let workflows = state.store.list_workflows().await?;
    Ok(Json(workflows.into_iter().map(workflow_dto).collect()))
}

/// Get one workflow by name.
#[utoipa::path(get, path = "/v1/workflows/{name}", tag = "workflows",
    params(("name" = String, Path, description = "workflow name")),
    responses((status = 200, body = WorkflowDto), (status = 404)))]
pub(super) async fn get(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<Json<WorkflowDto>> {
    let workflow = state
        .store
        .get_workflow(&name)
        .await
        .map_err(workflow_error)?;
    Ok(Json(workflow_dto(workflow)))
}

/// Write a new document over a workflow's.
#[utoipa::path(put, path = "/v1/workflows/{name}", tag = "workflows",
    request_body = UpdateWorkflowRequest,
    params(("name" = String, Path, description = "workflow name")),
    responses((status = 200, body = WorkflowDto), (status = 404), (status = 409)))]
pub(super) async fn update(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(req): Json<UpdateWorkflowRequest>,
) -> ApiResult<Json<WorkflowDto>> {
    let workflow = match req.document {
        Some(document) => state
            .store
            .set_workflow_document(&name, &document)
            .await
            .map_err(workflow_error)?,
        None => state
            .store
            .get_workflow(&name)
            .await
            .map_err(workflow_error)?,
    };
    Ok(Json(workflow_dto(workflow)))
}

/// Delete a workflow of the user's own (409 for a built-in).
#[utoipa::path(delete, path = "/v1/workflows/{name}", tag = "workflows",
    params(("name" = String, Path, description = "workflow name")),
    responses((status = 204), (status = 404), (status = 409)))]
pub(super) async fn delete(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<StatusCode> {
    state
        .store
        .delete_workflow(&name)
        .await
        .map_err(workflow_error)?;
    Ok(StatusCode::NO_CONTENT)
}

/// Put a built-in workflow back on the document Ariadne ships.
#[utoipa::path(post, path = "/v1/workflows/{name}/reset", tag = "workflows",
    params(("name" = String, Path, description = "workflow name")),
    responses((status = 200, body = WorkflowDto), (status = 404), (status = 409)))]
pub(super) async fn reset(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<Json<WorkflowDto>> {
    let workflow = state
        .store
        .reset_workflow(&name)
        .await
        .map_err(workflow_error)?;
    Ok(Json(workflow_dto(workflow)))
}

/// Parse a document without saving it anywhere: the picker a workflow editor
/// checks a draft against before it writes one.
#[utoipa::path(post, path = "/v1/workflows/parse", tag = "workflows",
    request_body = ParseWorkflowRequest,
    responses(
        (status = 200, body = ParsedWorkflowDto),
        (status = 400, description = "workflow_invalid, with details.line")
    ))]
pub(super) async fn parse(
    Json(req): Json<ParseWorkflowRequest>,
) -> ApiResult<Json<ParsedWorkflowDto>> {
    let parsed = workflow::parse(&req.document).map_err(|e| {
        ApiError::bad_request_with_details(
            "workflow_invalid",
            e.message.clone(),
            serde_json::json!({ "line": e.line }),
        )
    })?;
    Ok(Json(ParsedWorkflowDto {
        name: parsed.name,
        steps: parsed.steps.into_iter().map(workflow_step_dto).collect(),
    }))
}

/// `workflow_not_found` where the generic 404 code would otherwise read
/// `not_found`, the same way the skill and learned-permission routes narrow
/// it.
fn workflow_error(error: StoreError) -> ApiError {
    match error {
        StoreError::NotFound { .. } => ApiError::new(
            StatusCode::NOT_FOUND,
            "workflow_not_found",
            error.to_string(),
        ),
        other => other.into(),
    }
}
