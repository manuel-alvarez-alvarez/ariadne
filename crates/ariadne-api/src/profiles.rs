//! Profile DTOs.

use ariadne_core::{AgentKind, PromptKind, Role};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ProfileDto {
    pub id: String,
    pub name: String,
    pub role: Role,
    /// None = auto: resolved at spawn time to the first installed agent CLI
    /// (claude_code, then codex, then opencode).
    pub agent_kind: Option<AgentKind>,
    pub model: Option<String>,
    pub system_prompt: String,
    /// Extra argv flags appended when spawning the agent CLI.
    pub extra_flags: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateProfileRequest {
    #[schema(example = "rust-engineer")]
    pub name: String,
    pub role: Role,
    /// Omit for auto: the first installed agent CLI is used at spawn time.
    pub agent_kind: Option<AgentKind>,
    pub model: Option<String>,
    pub system_prompt: String,
    #[serde(default)]
    pub extra_flags: Vec<String>,
}

/// Partial update; absent fields stay unchanged.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct UpdateProfileRequest {
    pub name: Option<String>,
    /// New agent kind, or "auto" to clear it (resolve the first installed
    /// CLI at spawn time). Absent = unchanged.
    pub agent_kind: Option<String>,
    /// New model, or "default" (or empty) to clear it back to the agent's
    /// default. Absent = unchanged.
    pub model: Option<String>,
    pub system_prompt: Option<String>,
    pub extra_flags: Option<Vec<String>>,
}

/// One of the briefing prompts a profile owns beside its system prompt.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ProfilePromptDto {
    pub kind: PromptKind,
    /// Template text with `{placeholder}` tokens the daemon fills in.
    pub content: String,
    pub updated_at: String,
}

/// Body of `PUT /v1/profiles/{id}/prompts/{kind}`: the whole new text.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpdateProfilePromptRequest {
    pub content: String,
}
