//! Model catalog endpoint.

use std::time::Duration;

use axum::Json;
use axum::extract::Query;

use ariadne_api::models::{ModelDto, ModelListQuery};
use ariadne_core::AgentKind;
use ariadne_core::models::curated_models;

/// Model candidates per agent CLI: curated catalogs for claude_code and
/// codex, live discovery (`opencode models`) for opencode. Without `agent`,
/// the union for all agents.
#[utoipa::path(get, path = "/v1/models", tag = "models",
    params(ModelListQuery),
    responses((status = 200, body = [ModelDto])))]
pub async fn list(Query(q): Query<ModelListQuery>) -> Json<Vec<ModelDto>> {
    let kinds: &[AgentKind] = match &q.agent {
        Some(kind) => std::slice::from_ref(kind),
        None => &AgentKind::ALL,
    };
    let mut out = Vec::new();
    for &kind in kinds {
        match kind {
            AgentKind::Opencode => out.extend(opencode_models().await),
            _ => out.extend(curated_models(kind).iter().map(|m| ModelDto {
                id: m.id.to_string(),
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
            id: m.to_string(),
            agent_kind: AgentKind::Opencode,
            description: None,
        })
        .collect()
}
