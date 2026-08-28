//! What the agent CLIs are: the flags each is launched with, and the models
//! each can be pointed at.
//!
//! Two endpoints of one subject, and each of them a `list`: an operation id
//! is the handler's name, so the two live in a module apiece rather than
//! sharing one namespace and renaming an id the UI is generated from.

/// Agent-kind configuration endpoints.
///
/// What each coding-agent CLI is launched with, editable in one place: the
/// flags a profile used to carry belong to the agent, not to the persona.
/// Every spawn and resume reads them, so an edit lands on the next launch.
pub mod agents {
    use axum::Json;
    use axum::extract::{Path, State};

    use ariadne_api::agents::{AgentConfigDto, UpdateAgentConfigRequest};
    use ariadne_core::AgentKind;

    use crate::http::AppState;
    use crate::http::convert::agent_config_dto;
    use crate::http::error::{ApiError, ApiResult};

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
}

/// Model catalog endpoint.
pub mod models {
    use std::time::Duration;

    use axum::Json;

    use ariadne_api::models::ModelDto;
    use ariadne_core::AgentKind;
    use ariadne_core::models::{ModelRef, curated_models};

    /// Everything an agent can be pinned to, `<agent_kind>[:<model>]` apiece:
    /// each agent CLI on its own — that CLI on its own default model — and
    /// then the models of it, curated for claude_code and codex, discovered
    /// live (`opencode models`) for opencode.
    ///
    /// The union always, and grouped by agent CLI: a model is chosen by one
    /// string that carries its CLI, so nothing scopes this catalog any more.
    #[utoipa::path(get, path = "/v1/models", tag = "models",
        responses((status = 200, body = [ModelDto])))]
    pub async fn list() -> Json<Vec<ModelDto>> {
        let mut out = Vec::new();
        for kind in AgentKind::ALL {
            out.push(ModelDto {
                id: ModelRef::of(kind).to_string(),
                agent_kind: kind,
                description: Some(format!("{} on its own default model", kind.as_str())),
            });
            match kind {
                AgentKind::Opencode => out.extend(opencode_models().await),
                _ => out.extend(curated_models(kind).iter().map(|m| ModelDto {
                    id: qualified(kind, m.id),
                    agent_kind: kind,
                    description: Some(m.description.to_string()),
                })),
            }
        }
        Json(out)
    }

    /// OpenCode lists its models natively (`opencode models`, provider/model).
    /// Fail-soft: a missing or hung binary yields no models, never an error.
    async fn opencode_models() -> Vec<ModelDto> {
        let output = tokio::time::timeout(
            Duration::from_secs(3),
            tokio::process::Command::new("opencode")
                .arg("models")
                .kill_on_drop(true)
                .output(),
        )
        .await;
        let Ok(Ok(output)) = output else {
            return Vec::new();
        };
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && l.contains('/'))
            .map(|m| ModelDto {
                id: qualified(AgentKind::Opencode, m),
                agent_kind: AgentKind::Opencode,
                description: None,
            })
            .collect()
    }

    /// One catalog entry's id: the model as its agent CLI runs it, which is
    /// what a request writes to pin it.
    fn qualified(kind: AgentKind, model: &str) -> String {
        ModelRef {
            agent_kind: kind,
            model: Some(model.to_string()),
        }
        .to_string()
    }
}
