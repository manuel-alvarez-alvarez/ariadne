//! What the agents are: the flags each registry agent is launched with, the
//! registry itself, and the models each agent can be pointed at.
//!
//! Three endpoints of one subject, and each of them a `list`: an operation id
//! is the handler's name, so they live in a module apiece rather than sharing
//! one namespace and renaming an id the UI is generated from.

/// Agent configuration endpoints.
///
/// What each registry agent is launched with, editable in one place: the
/// flags ride behind the agent's registry command on every spawn and resume,
/// so an edit lands on the next launch.
pub mod agents {
    use axum::extract::{Path, State};

    use ariadne_api::agents::{AgentConfigDto, UpdateAgentConfigRequest};

    use crate::http::AppState;
    use crate::http::error::{ApiError, ApiResult, Json};

    /// Every registry agent's flags, in registry order. An agent nobody set
    /// flags for is listed with none.
    #[utoipa::path(get, path = "/v1/agents", tag = "agents",
        responses((status = 200, body = [AgentConfigDto])))]
    pub async fn list(State(state): State<AppState>) -> ApiResult<Json<Vec<AgentConfigDto>>> {
        let mut out = Vec::new();
        for agent in state.agent_registry.agents().await {
            let extra_flags = state.store.agent_flags(&agent.id).await?;
            out.push(AgentConfigDto {
                agent_id: agent.id,
                extra_flags,
                default_flags: Vec::new(),
            });
        }
        Ok(Json(out))
    }

    /// Replace a registry agent's flags.
    ///
    /// The list is replaced whole, and an empty one is a legitimate answer.
    /// Restoring the defaults is this same call with the `default_flags` the
    /// GET hands out — nothing else to learn, and nothing that can drift from
    /// them.
    #[utoipa::path(put, path = "/v1/agents/{id}", tag = "agents",
        request_body = UpdateAgentConfigRequest,
        params(("id" = String, Path, description = "the agent's registry id")),
        responses(
            (status = 200, body = AgentConfigDto),
            (status = 400, description = "no such agent in the registry")
        ))]
    pub async fn update(
        State(state): State<AppState>,
        Path(id): Path<String>,
        Json(req): Json<UpdateAgentConfigRequest>,
    ) -> ApiResult<Json<AgentConfigDto>> {
        if state.agent_registry.command_of(&id).is_none() {
            return Err(ApiError::bad_request(format!(
                "unknown agent: {id} — name an agent of the ACP registry \
                 (`GET /v1/acp-agents`)"
            )));
        }
        let config = state
            .store
            .update_agent_config(&id, req.extra_flags)
            .await?;
        Ok(Json(AgentConfigDto {
            agent_id: config.agent_id.clone(),
            extra_flags: config.extra_flags(),
            default_flags: Vec::new(),
        }))
    }
}

/// ACP agent registry endpoints.
pub mod acp_agents {
    use axum::extract::State;

    use ariadne_api::agents::AcpAgentDto;

    use crate::http::AppState;
    use crate::http::error::{ApiResult, Json};

    /// Every built-in and configured ACP agent with its cached probe result.
    #[utoipa::path(get, path = "/v1/acp-agents", tag = "acp-agents",
        responses((status = 200, body = [AcpAgentDto])))]
    pub async fn list(State(state): State<AppState>) -> ApiResult<Json<Vec<AcpAgentDto>>> {
        Ok(Json(state.agent_registry.agents().await))
    }

    /// Probe every registry entry and replace the cached discovery snapshot.
    #[utoipa::path(post, path = "/v1/acp-agents/refresh", tag = "acp-agents",
        responses((status = 200, body = [AcpAgentDto])))]
    pub async fn refresh(State(state): State<AppState>) -> ApiResult<Json<Vec<AcpAgentDto>>> {
        Ok(Json(state.agent_registry.refresh().await))
    }
}

/// Model catalog endpoint.
pub mod models {
    use axum::extract::State;

    use ariadne_api::models::{ModelDto, SetModelEnabledRequest};

    use crate::http::AppState;
    use crate::http::error::{ApiError, ApiResult, Json};

    /// Everything an agent can be pinned to, `<agent>:<model>` apiece: the
    /// models discovery found each registry agent offering, grouped by agent.
    /// No bare-agent entry: a model is required wherever an agent is pinned,
    /// so there is nothing an agent on its own could be staffed as.
    ///
    /// Each entry carries the efforts it can be run at, as the agent offered
    /// them, and which of them it runs by default. Every entry says whether
    /// an agent can be staffed on it. A model the user turned off stays in
    /// the list, off: a catalog that hid it would leave nothing to turn back
    /// on, and nothing to say why a pin naming it is refused.
    #[utoipa::path(get, path = "/v1/models", tag = "models",
        responses((status = 200, body = [ModelDto])))]
    pub async fn list(State(state): State<AppState>) -> ApiResult<Json<Vec<ModelDto>>> {
        let mut out = catalog(&state).await;
        let off = state.store.disabled_models().await?;
        for entry in &mut out {
            entry.enabled = !off.contains(&entry.id);
        }
        Ok(Json(out))
    }

    /// Turn one entry of the catalog on or off.
    ///
    /// The catalog is discovery, so this writes only the exception:
    /// an id nothing in the catalog carries is a 404, and the last entry left
    /// on cannot be turned off — a plan needs something to be staffed on, and
    /// a daemon that can staff nothing is not a state to leave a user in.
    #[utoipa::path(put, path = "/v1/models/enabled", tag = "models",
        request_body = SetModelEnabledRequest,
        responses(
            (status = 200, body = ModelDto),
            (status = 404, description = "no such model in the catalog"),
            (status = 409, description = "it is the last model left enabled")
        ))]
    pub async fn set_enabled(
        State(state): State<AppState>,
        Json(req): Json<SetModelEnabledRequest>,
    ) -> ApiResult<Json<ModelDto>> {
        let mut catalog = catalog(&state).await;
        let off = state.store.disabled_models().await?;
        if !catalog.iter().any(|entry| entry.id == req.id) {
            return Err(ApiError::new(
                axum::http::StatusCode::NOT_FOUND,
                "not_found",
                format!("no model `{}` in the catalog", req.id),
            ));
        }
        if !req.enabled
            && !catalog
                .iter()
                .any(|entry| entry.id != req.id && !off.contains(&entry.id))
        {
            return Err(ApiError::conflict(format!(
                "`{}` is the last model left enabled — turn another one on first",
                req.id
            )));
        }
        state.store.set_model_enabled(&req.id, req.enabled).await?;
        let mut entry = catalog
            .drain(..)
            .find(|entry| entry.id == req.id)
            .expect("the entry was found above");
        entry.enabled = req.enabled;
        Ok(Json(entry))
    }

    /// The catalog as discovery found it, before anything the user turned
    /// off is read over it: every entry comes back `enabled`. Discovery runs
    /// at daemon startup and on an explicit refresh; this reads its cache.
    async fn catalog(state: &AppState) -> Vec<ModelDto> {
        state.agent_registry.models().await
    }

    /// The efforts one model can be run at, as the catalog knows them: the
    /// registry's cached snapshot, which lists a model under its catalog id
    /// whole.
    ///
    /// None where nothing here lists the model — a hand-typed id — which is
    /// what [`ariadne_core::models::effort_error`] reads as "take any effort
    /// that is not blank".
    pub async fn efforts_of(
        registry: &crate::acp_discovery::AgentRegistry,
        model: &str,
    ) -> Option<Vec<String>> {
        registry
            .models()
            .await
            .into_iter()
            .find(|m| m.id == model)
            .map(|m| m.efforts.into_iter().map(|e| e.id).collect())
    }
}
