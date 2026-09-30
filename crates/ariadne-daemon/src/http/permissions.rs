//! The AI permission settings behind the `ai` permission mode (022).
//!
//! Four endpoints over one settings row: read it, write it, run the install,
//! or test a request. Turning the model on starts an install, so the write
//! answers with `installing` rather than waiting for two gigabytes; the
//! `ai_permissions_updated` event says how it ended.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;

use ariadne_api::permissions::{
    AiPermissionsStatusDto, Device, LearnedPermissionDto, LearnedPermissionQuery,
    LearnedPermissionsResponse, TestAiPermissionRequest, TestAiPermissionResponse,
    UpdateAiPermissionsRequest,
};
use ariadne_store::{AiPermissionSettingsUpdate, StoreError};

use crate::ai_permissions::flavours;

use super::AppState;
use super::convert::learned_permission_dto;
use super::error::{ApiError, ApiResult, Json};

#[utoipa::path(get, path = "/v1/permissions/learned", tag = "permissions", params(LearnedPermissionQuery), responses((status = 200, body = LearnedPermissionsResponse)))]
pub(super) async fn list_learned(
    State(state): State<AppState>,
    Query(query): Query<LearnedPermissionQuery>,
) -> ApiResult<Json<LearnedPermissionsResponse>> {
    let items = state
        .store
        .list_learned_permissions(query.repository.as_deref())
        .await?
        .into_iter()
        .map(learned_permission_dto)
        .collect();
    Ok(Json(LearnedPermissionsResponse { items }))
}

#[utoipa::path(get, path = "/v1/permissions/learned/{id}", tag = "permissions", params(("id" = String, Path)), responses((status = 200, body = LearnedPermissionDto), (status = 404)))]
pub(super) async fn get_learned(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<LearnedPermissionDto>> {
    state
        .store
        .get_learned_permission(&id)
        .await
        .map(learned_permission_dto)
        .map(Json)
        .map_err(learned_error)
}

#[utoipa::path(delete, path = "/v1/permissions/learned/{id}", tag = "permissions", params(("id" = String, Path)), responses((status = 204), (status = 404)))]
pub(super) async fn delete_learned(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    state
        .store
        .delete_learned_permission(&id)
        .await
        .map_err(learned_error)?;
    Ok(StatusCode::NO_CONTENT)
}

fn learned_error(error: StoreError) -> ApiError {
    match error {
        StoreError::NotFound { .. } => ApiError::new(
            StatusCode::NOT_FOUND,
            "learned_permission_not_found",
            error.to_string(),
        ),
        other => other.into(),
    }
}

/// The AI permission settings, the interpreter probed afresh, and where the install
/// has got to.
#[utoipa::path(get, path = "/v1/permissions/ai", tag = "permissions",
    responses((status = 200, body = AiPermissionsStatusDto)))]
pub(super) async fn get(State(state): State<AppState>) -> ApiResult<Json<AiPermissionsStatusDto>> {
    Ok(Json(state.ai_permissions.status().await))
}

/// Change the AI permission settings. An absent field stays as it was.
///
/// Turning the model on is refused while the daemon has no Python 3.12 or 3.13 to
/// install into: the download is minutes and gigabytes, and it would fail at
/// the end of them. Turning it off keeps every file on disk, so turning it
/// back on repairs its pinned package and weights.
#[utoipa::path(put, path = "/v1/permissions/ai", tag = "permissions",
    request_body = UpdateAiPermissionsRequest,
    responses(
        (status = 200, body = AiPermissionsStatusDto),
        (status = 409, description = "no Python 3.12 or 3.13 to install into"),
        (status = 422, description = "invalid thresholds, or a flavour and device combination that cannot run")
    ))]
pub(super) async fn update(
    State(state): State<AppState>,
    Json(req): Json<UpdateAiPermissionsRequest>,
) -> ApiResult<Json<AiPermissionsStatusDto>> {
    for (name, threshold) in [
        ("allow threshold", req.allow_threshold),
        ("deny threshold", req.deny_threshold),
    ] {
        if let Some(threshold) = threshold
            && !(0.0..=1.0).contains(&threshold)
        {
            return Err(invalid(format!(
                "{name} must be between 0 and 1, not {threshold}"
            )));
        }
    }

    let before = state.ai_permissions.status().await;
    let allow_threshold = req.allow_threshold.unwrap_or(before.allow_threshold);
    let deny_threshold = req.deny_threshold.unwrap_or(before.deny_threshold);
    if allow_threshold >= deny_threshold {
        return Err(invalid(format!(
            "allow threshold must be less than deny threshold, not {allow_threshold} and {deny_threshold}"
        )));
    }
    let turning_on = req.enabled == Some(true) && !before.enabled;
    if turning_on && !before.python.ok {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "python_unavailable",
            python_unavailable(&before),
        ));
    }

    let chosen = match (req.flavour, req.device) {
        (None, None) => None,
        (flavour, device) => {
            let hardware = state.ai_permissions.hardware().await;
            let flavour = match flavour {
                Some(flavour) => flavour,
                None => state.ai_permissions.stored_flavour().await,
            };
            let device = match device {
                Some(device) => device,
                None => flavours::best_device(&hardware, flavour).unwrap_or(Device::Cpu),
            };
            let options = flavours::options(&hardware);
            let entry = options
                .iter()
                .find(|option| option.flavour == flavour)
                .expect("every flavour is listed");
            let option = entry
                .devices
                .iter()
                .find(|option| option.device == device)
                .expect("every device is listed");
            if !option.can_run {
                return Err(ApiError::new(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "flavour_unsupported",
                    option
                        .reason
                        .clone()
                        .unwrap_or_else(|| "that combination cannot run here".to_string()),
                ));
            }
            Some((flavour, device))
        }
    };

    let update = AiPermissionSettingsUpdate {
        enabled: req.enabled,
        allow_threshold: req.allow_threshold,
        deny_threshold: req.deny_threshold,
        flavour: chosen.map(|(flavour, _)| flavour.as_str().to_string()),
        device: chosen.map(|(_, device)| device.as_str().to_string()),
        // Turning the model off leaves the files where they are and says so;
        // turning it on is the install's own state to write.
        state: (req.enabled == Some(false) && before.enabled).then(|| "disabled".to_string()),
        ..Default::default()
    };
    // A new flavour or device on a model that stays on is installed at once;
    // on a model that is off, it is only stored until the model is turned on.
    let switching = before.enabled
        && req.enabled != Some(false)
        && chosen.is_some_and(|pair| pair != (before.flavour, before.device));
    let status = state.ai_permissions.write(update).await;
    // Turning the model off while an install runs does not stop the install: its
    // end is written, and the model stays off (`AiPermissions::install`).
    if !turning_on && !switching {
        return Ok(Json(status));
    }
    match state.ai_permissions.install().await {
        Some(started) => Ok(Json(started)),
        // Turned off and on again, or switched, while an install still runs:
        // that install is the one to wait for, and it installs the stored
        // choice before it ends.
        None => Ok(Json(state.ai_permissions.rejoin_install().await)),
    }
}

/// Run the install again: the pinned package, adapter and base.
#[utoipa::path(post, path = "/v1/permissions/ai/refresh", tag = "permissions",
    responses(
        (status = 202, body = AiPermissionsStatusDto),
        (status = 409, description = "the AI permission model is off, or an install is already running")
    ))]
pub(super) async fn refresh(
    State(state): State<AppState>,
) -> ApiResult<(StatusCode, Json<AiPermissionsStatusDto>)> {
    let status = state.ai_permissions.status().await;
    if !status.enabled {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "ai_disabled",
            "the AI permission model is off; turn it on before refreshing it",
        ));
    }
    let started = state.ai_permissions.install().await.ok_or_else(|| {
        ApiError::new(
            StatusCode::CONFLICT,
            "ai_busy",
            "an AI permission model install is already running; wait for it to end",
        )
    })?;
    Ok((StatusCode::ACCEPTED, Json(started)))
}

/// Score one request with the AI permission model without selecting an option
/// or changing any permission state.
#[utoipa::path(post, path = "/v1/permissions/ai/test", tag = "permissions",
    request_body = TestAiPermissionRequest,
    responses(
        (status = 200, body = TestAiPermissionResponse),
        (status = 409, description = "the AI permission model is off"),
        (status = 422, description = "the tool is empty")
    ))]
pub(super) async fn test(
    State(state): State<AppState>,
    Json(req): Json<TestAiPermissionRequest>,
) -> ApiResult<Json<TestAiPermissionResponse>> {
    if req.tool.is_empty() {
        return Err(invalid("tool must not be empty".to_string()));
    }
    let status = state.ai_permissions.status().await;
    if !status.enabled {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "ai_disabled",
            "the AI permission model is off; turn it on before testing a request",
        ));
    }
    let tool = req.tool.clone();
    let workspace = req.workspace.clone();
    let (tool_call, options) = crate::ai_permissions::decide::test_call(
        req.tool,
        req.kind,
        req.input,
        req.options.as_deref().unwrap_or_default(),
    );
    let prepared =
        crate::ai_permissions::decide::prepare(&tool_call, &options, workspace.as_deref());
    let decision = if let Some(decision) = prepared.hard_rule() {
        decision
    } else {
        match state.ai_permissions.live_once_started().await {
            Some(live) => {
                crate::ai_permissions::decide::decide(
                    &live,
                    &prepared,
                    state.ai_permissions.decision_timeout(),
                )
                .await
            }
            None => prepared.unanswered("unavailable"),
        }
    };
    let label = match &decision {
        crate::ai_permissions::decide::Decision::Allow { .. } => Some("allow".into()),
        crate::ai_permissions::decide::Decision::Ask { .. } => Some("ask".into()),
        crate::ai_permissions::decide::Decision::Deny { .. }
        | crate::ai_permissions::decide::Decision::Rule { .. } => Some("deny".into()),
        crate::ai_permissions::decide::Decision::Unanswered { .. } => None,
    };
    let score = decision.score();
    let derived = decision.derived();
    let response = TestAiPermissionResponse {
        label,
        danger: score.map(|score| score.danger),
        allow_threshold: score.map_or(status.allow_threshold, |score| score.allow_threshold),
        deny_threshold: score.map_or(status.deny_threshold, |score| score.deny_threshold),
        ai_error: match &decision {
            crate::ai_permissions::decide::Decision::Unanswered { reason, .. } => {
                Some((*reason).into())
            }
            _ => None,
        },
        operation: derived.operation.map(str::to_string),
        risk_tags: Some(derived.risk_tags.iter().map(ToString::to_string).collect()),
        rule: decision.rule().map(str::to_string),
        cap: decision.cap().map(str::to_string),
        probabilities: score.map(|score| score.probabilities.clone()),
    };
    tracing::info!(tool, label = ?response.label, danger = ?response.danger, ai_error = ?response.ai_error, "AI permission test");
    Ok(Json(response))
}

/// Refuse a repository set to `ai` while the AI permission model is off.
///
/// The mode would answer nothing: `ai` asks the model, and a disabled model has no
/// server and may have no install. It is refused where it is set rather than
/// at the first permission request, which is an hour into an agent's work.
pub(super) async fn ai_needs_the_model(state: &AppState) -> Result<(), ApiError> {
    if state.ai_permissions.status().await.enabled {
        return Ok(());
    }
    Err(ApiError::new(
        StatusCode::CONFLICT,
        "ai_disabled",
        "the `ai` permission mode needs the AI permission model; turn it on with \
         `ariadne permissions enable` first",
    ))
}

/// A refusal of what the body says, as against what the daemon is in: the
/// same code and status every DTO-level refusal carries.
fn invalid(message: String) -> ApiError {
    ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, "invalid_request", message)
}

/// Why the model cannot be turned on, naming the interpreter that was found.
fn python_unavailable(status: &AiPermissionsStatusDto) -> String {
    match (&status.python.path, &status.python.version) {
        (Some(path), Some(version)) => format!(
            "the AI permission model needs Python 3.12 or 3.13; {path} is {version}. \
             Set `python_bin` in config.toml to a newer one"
        ),
        _ => "the AI permission model needs Python 3.12 or 3.13, and this daemon found no Python \
              on its PATH. Set `python_bin` in config.toml"
            .to_string(),
    }
}
