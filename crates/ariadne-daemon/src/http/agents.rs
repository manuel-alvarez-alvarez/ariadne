//! Agent-kind configuration endpoints.
//!
//! What each coding-agent CLI is launched with, editable in one place: the
//! flags a profile used to carry belong to the agent, not to the persona.
//! Every spawn and resume reads them, so an edit lands on the next launch.

use axum::Json;
use axum::extract::{Path, State};

use ariadne_api::agents::{AgentConfigDto, UpdateAgentConfigRequest};
use ariadne_core::AgentKind;

use super::AppState;
use super::convert::agent_config_dto;
use super::error::{ApiError, ApiResult};

/// Every agent kind's flags, current and default.
#[utoipa::path(get, path = "/v1/agents", tag = "agents",
    responses((status = 200, body = [AgentConfigDto])))]
pub async fn list(State(state): State<AppState>) -> ApiResult<Json<Vec<AgentConfigDto>>> {
    let configs = state.store.list_agent_configs().await?;
    Ok(Json(configs.into_iter().map(agent_config_dto).collect()))
}

/// Replace an agent kind's flags.
///
/// The list is replaced whole, and an empty one is a legitimate answer.
/// Restoring the defaults is this same call with the `default_flags` the GET
/// hands out — nothing else to learn, and nothing that can drift from them.
#[utoipa::path(put, path = "/v1/agents/{kind}", tag = "agents",
    request_body = UpdateAgentConfigRequest,
    params(("kind" = String, Path, description = "claude_code, codex or opencode")),
    responses(
        (status = 200, body = AgentConfigDto),
        (status = 400, description = "unknown agent kind")
    ))]
pub async fn update(
    State(state): State<AppState>,
    Path(kind): Path<String>,
    Json(req): Json<UpdateAgentConfigRequest>,
) -> ApiResult<Json<AgentConfigDto>> {
    let kind = parse_kind(&kind)?;
    let config = state
        .store
        .update_agent_config(kind, req.extra_flags)
        .await?;
    Ok(Json(agent_config_dto(config)))
}

/// The agent kind named in the path, or a 400 naming the ones there are.
fn parse_kind(raw: &str) -> Result<AgentKind, ApiError> {
    raw.parse::<AgentKind>().map_err(|_| {
        let known = AgentKind::ALL
            .iter()
            .map(|k| k.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        ApiError::bad_request(format!(
            "unknown agent kind: {raw} (expected one of {known})"
        ))
    })
}
