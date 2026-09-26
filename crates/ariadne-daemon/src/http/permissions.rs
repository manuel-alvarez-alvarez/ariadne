//! The Laya settings behind the `ai` permission mode (022).
//!
//! Three endpoints over one settings row: read it, write it, and run the
//! install again. Turning Laya on starts an install, so the write answers
//! with `installing` rather than waiting for two gigabytes; the
//! `laya_updated` event says how it ended.

use axum::extract::State;
use axum::http::StatusCode;

use ariadne_api::permissions::{LayaStatusDto, UpdateLayaRequest};
use ariadne_store::LayaUpdate;

use super::AppState;
use super::error::{ApiError, ApiResult, Json};

/// The Laya settings, the interpreter probed afresh, and where the install
/// has got to.
#[utoipa::path(get, path = "/v1/permissions/laya", tag = "permissions",
    responses((status = 200, body = LayaStatusDto)))]
pub(super) async fn get(State(state): State<AppState>) -> ApiResult<Json<LayaStatusDto>> {
    Ok(Json(state.laya.status().await))
}

/// Change the Laya settings. An absent field stays as it was.
///
/// Turning Laya on is refused while the daemon has no Python 3.10 or newer to
/// install into: the download is minutes and gigabytes, and it would fail at
/// the end of them. Turning it off keeps every file on disk, so turning it
/// back on costs nothing but the release check.
#[utoipa::path(put, path = "/v1/permissions/laya", tag = "permissions",
    request_body = UpdateLayaRequest,
    responses(
        (status = 200, body = LayaStatusDto),
        (status = 409, description = "no Python 3.10 or newer to install into"),
        (status = 422, description = "a threshold outside 0..=1, or a schedule that is not HH:MM")
    ))]
pub(super) async fn update(
    State(state): State<AppState>,
    Json(req): Json<UpdateLayaRequest>,
) -> ApiResult<Json<LayaStatusDto>> {
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

    let before = state.laya.status().await;
    let turning_on = req.enabled == Some(true) && !before.enabled;
    if turning_on && !before.python.ok {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "python_unavailable",
            python_unavailable(&before),
        ));
    }

    let update = LayaUpdate {
        enabled: req.enabled,
        checkpoints: req.checkpoints.map(|c| c.as_str().to_string()),
        threshold: req.threshold,
        schedule: req.schedule,
        // Turning Laya off leaves the files where they are and says so;
        // turning it on is the install's own state to write.
        state: (req.enabled == Some(false) && before.enabled).then(|| "disabled".to_string()),
        ..Default::default()
    };
    let status = state.laya.write(update).await;
    // Turning Laya off while an install runs does not stop the install: its
    // end is written, and Laya stays off (`Laya::install`).
    if !turning_on {
        return Ok(Json(status));
    }
    match state.laya.install().await {
        Some(started) => Ok(Json(started)),
        // Turned off and on again while the first install still runs: that
        // install is the one to wait for.
        None => Ok(Json(state.laya.rejoin_install().await)),
    }
}

/// Run the install again: the release check, the wheel and the checkpoints
/// the settings name now.
#[utoipa::path(post, path = "/v1/permissions/laya/refresh", tag = "permissions",
    responses(
        (status = 202, body = LayaStatusDto),
        (status = 409, description = "Laya is off, or an install is already running")
    ))]
pub(super) async fn refresh(
    State(state): State<AppState>,
) -> ApiResult<(StatusCode, Json<LayaStatusDto>)> {
    let status = state.laya.status().await;
    if !status.enabled {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "laya_disabled",
            "Laya is off; turn it on before refreshing it",
        ));
    }
    let started = state.laya.install().await.ok_or_else(|| {
        ApiError::new(
            StatusCode::CONFLICT,
            "laya_busy",
            "a Laya install is already running; wait for it to end",
        )
    })?;
    Ok((StatusCode::ACCEPTED, Json(started)))
}

/// Refuse a repository set to `ai` while Laya is off.
///
/// The mode would answer nothing: `ai` asks Laya, and a disabled Laya has no
/// server and may have no install. It is refused where it is set rather than
/// at the first permission request, which is an hour into an agent's work.
pub(super) async fn ai_needs_laya(state: &AppState) -> Result<(), ApiError> {
    if state.laya.status().await.enabled {
        return Ok(());
    }
    Err(ApiError::new(
        StatusCode::CONFLICT,
        "laya_disabled",
        "the `ai` permission mode needs Laya; turn it on with \
         `ariadne permissions enable` first",
    ))
}

/// A refusal of what the body says, as against what the daemon is in: the
/// same code and status every DTO-level refusal carries.
fn invalid(message: String) -> ApiError {
    ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, "invalid_request", message)
}

/// Why Laya cannot be turned on, naming the interpreter that was found.
fn python_unavailable(status: &LayaStatusDto) -> String {
    match (&status.python.path, &status.python.version) {
        (Some(path), Some(version)) => format!(
            "Laya needs Python 3.10 or newer; {path} is {version}. \
             Set `python_bin` in config.toml to a newer one"
        ),
        _ => "Laya needs Python 3.10 or newer, and this daemon found no python3 \
              on its PATH. Set `python_bin` in config.toml"
            .to_string(),
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
