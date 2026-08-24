//! Profile DTOs.

use ariadne_core::{AgentKind, PromptKind, Role};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

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
    /// Briefing prompts to seed instead of the role defaults. A kind listed
    /// here replaces its default; every other kind of the role is seeded as
    /// usual. Absent or empty = the role defaults, unedited.
    #[serde(default)]
    pub prompts: Vec<NewProfilePrompt>,
}

/// One prompt override in [`CreateProfileRequest`]: the kind spelled as on the
/// prompt routes, and the text to seed instead of the role default.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct NewProfilePrompt {
    #[schema(example = "engineer_briefing")]
    pub kind: String,
    pub content: String,
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

/// Response of `GET /v1/roles/{role}/prompt-defaults`: what a profile of that
/// role is seeded with, read without creating or touching anything.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RolePromptDefaultsDto {
    pub role: Role,
    pub system_prompt: String,
    /// The role's briefing prompts, in briefing order.
    pub prompts: Vec<PromptDefaultDto>,
}

/// One built-in prompt default. Unlike [`ProfilePromptDto`] it belongs to no
/// profile, so there is nothing to timestamp.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PromptDefaultDto {
    pub kind: PromptKind,
    /// Template text with `{placeholder}` tokens the daemon fills in.
    pub content: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, IntoParams)]
pub struct ProfileListQuery {
    /// Filter by role.
    pub role: Option<Role>,
}
