//! The AI permission settings behind the `ai` permission mode (022).
//!
//! Three endpoints over one settings row: read it, write it, and run the
//! install again. Turning the model on starts an install, so the write answers
//! with `installing` rather than waiting for two gigabytes; the
//! `ai_permissions_updated` event says how it ended.

use axum::extract::State;
use axum::http::StatusCode;

use ariadne_api::permissions::{AiPermissionsStatusDto, UpdateAiPermissionsRequest};
use ariadne_store::AiPermissionSettingsUpdate;

use super::AppState;
use super::error::{ApiError, ApiResult, Json};

/// The AI permission settings, the interpreter probed afresh, and where the install
/// has got to.
#[utoipa::path(get, path = "/v1/permissions/ai", tag = "permissions",
    responses((status = 200, body = AiPermissionsStatusDto)))]
pub(super) async fn get(State(state): State<AppState>) -> ApiResult<Json<AiPermissionsStatusDto>> {
    Ok(Json(state.ai_permissions.status().await))
}

/// Change the AI permission settings. An absent field stays as it was.
///
/// Turning the model on is refused while the daemon has no Python 3.10 or newer to
/// install into: the download is minutes and gigabytes, and it would fail at
/// the end of them. Turning it off keeps every file on disk, so turning it
/// back on costs nothing but the release check.
#[utoipa::path(put, path = "/v1/permissions/ai", tag = "permissions",
    request_body = UpdateAiPermissionsRequest,
    responses(
        (status = 200, body = AiPermissionsStatusDto),
        (status = 409, description = "no Python 3.10 or newer to install into"),
        (status = 422, description = "a threshold outside 0..=1, a schedule that is not HH:MM, \
                                      or a prompt over 4000 characters")
    ))]
pub(super) async fn update(
    State(state): State<AppState>,
    Json(req): Json<UpdateAiPermissionsRequest>,
) -> ApiResult<Json<AiPermissionsStatusDto>> {
    if let Some(threshold) = req.threshold
        && !(0.0..=1.0).contains(&threshold)
    {
        return Err(invalid(format!(
            "threshold must be between 0 and 1, not {threshold}"
        )));
    }
    if let Some(Some(schedule)) = &req.schedule
        && !is_clock_time(schedule)
    {
        return Err(invalid(format!(
            "schedule must be HH:MM in 24-hour time, not `{schedule}`"
        )));
    }

    for (field, prompt) in [
        ("question", &req.question),
        ("allow_criteria", &req.allow_criteria),
        ("review_criteria", &req.review_criteria),
    ] {
        if let Some(Some(text)) = prompt
            && text.chars().count() > MAX_PROMPT
        {
            return Err(invalid(format!(
                "{field} must be at most {MAX_PROMPT} characters, not {}",
                text.chars().count()
            )));
        }
    }

    let before = state.ai_permissions.status().await;
    let turning_on = req.enabled == Some(true) && !before.enabled;
    if turning_on && !before.python.ok {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "python_unavailable",
            python_unavailable(&before),
        ));
    }

    let update = AiPermissionSettingsUpdate {
        enabled: req.enabled,
        checkpoints: req.checkpoints.map(|c| c.as_str().to_string()),
        threshold: req.threshold,
        schedule: req.schedule,
        question: req.question.map(stored_prompt),
        allow_criteria: req.allow_criteria.map(stored_prompt),
        review_criteria: req.review_criteria.map(stored_prompt),
        // Turning the model off leaves the files where they are and says so;
        // turning it on is the install's own state to write.
        state: (req.enabled == Some(false) && before.enabled).then(|| "disabled".to_string()),
        ..Default::default()
    };
    let status = state.ai_permissions.write(update).await;
    // Turning the model off while an install runs does not stop the install: its
    // end is written, and the model stays off (`AiPermissions::install`).
    if !turning_on {
        return Ok(Json(status));
    }
    match state.ai_permissions.install().await {
        Some(started) => Ok(Json(started)),
        // Turned off and on again while the first install still runs: that
        // install is the one to wait for.
        None => Ok(Json(state.ai_permissions.rejoin_install().await)),
    }
}

/// Run the install again: the release check, the wheel and the checkpoints
/// the settings name now.
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

/// The longest prompt text a write takes, in characters.
const MAX_PROMPT: usize = 4000;

/// What a prompt field stores: `null` and a blank text both restore the
/// built-in text, so neither sends the model an empty question.
fn stored_prompt(prompt: Option<String>) -> Option<String> {
    prompt.filter(|text| !text.trim().is_empty())
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
            "the AI permission model needs Python 3.10 or newer; {path} is {version}. \
             Set `python_bin` in config.toml to a newer one"
        ),
        _ => {
            "the AI permission model needs Python 3.10 or newer, and this daemon found no python3 \
              on its PATH. Set `python_bin` in config.toml"
                .to_string()
        }
    }
}

/// Whether a schedule is `HH:MM` in 24-hour time. Exactly two digits, a
/// colon, and two more: `3:30` and `03:30:00` are refused, so every stored
/// schedule sorts and compares as text.
fn is_clock_time(schedule: &str) -> bool {
    let Some((hours, minutes)) = schedule.split_once(':') else {
        return false;
    };
    let two_digits = |part: &str| part.len() == 2 && part.bytes().all(|b| b.is_ascii_digit());
    two_digits(hours)
        && two_digits(minutes)
        && hours.parse::<u32>().is_ok_and(|h| h < 24)
        && minutes.parse::<u32>().is_ok_and(|m| m < 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Midnight and the last minute of the day are times; a missing zero, a
    /// seconds field, an hour that does not exist and empty text are not.
    #[test]
    fn a_schedule_is_two_digits_a_colon_and_two_digits() {
        for good in ["00:00", "03:30", "23:59", "09:05"] {
            assert!(is_clock_time(good), "{good}");
        }
        for bad in [
            "25:00", "03:60", "3:30", "03:30:00", "0330", "", ":", "ab:cd",
        ] {
            assert!(!is_clock_time(bad), "{bad}");
        }
    }
}
