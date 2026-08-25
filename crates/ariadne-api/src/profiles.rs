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
    /// The system prompt this profile is spawned with: the one set on it, or
    /// the default of its role while it has none of its own.
    pub system_prompt: String,
    /// Whether `system_prompt` is that role default rather than a text set on
    /// this profile.
    pub system_prompt_is_default: bool,
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
    /// Absent or null = the default of the role, which the profile then
    /// follows. Briefings are set afterwards, one `PUT` per kind.
    #[serde(default)]
    pub system_prompt: Option<String>,
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
    /// New system prompt. Absent = unchanged; putting it back on the role
    /// default is `POST /v1/profiles/{id}/system-prompt/reset`.
    pub system_prompt: Option<String>,
}

/// One of the briefing prompts a profile owns beside its system prompt, as it
/// takes effect.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ProfilePromptDto {
    pub kind: PromptKind,
    /// Template text with `{placeholder}` tokens the daemon fills in: the one
    /// set on the profile, or the default of the kind while it has none.
    pub content: String,
    /// Whether `content` is that default rather than a text set on this
    /// profile.
    pub is_default: bool,
    /// When the text set on the profile was last written; null while the
    /// default stands, which nothing dates.
    pub updated_at: Option<String>,
}

/// Body of `PUT /v1/profiles/{id}/prompts/{kind}`: the whole new text.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpdateProfilePromptRequest {
    pub content: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, IntoParams)]
pub struct ProfileListQuery {
    /// Filter by role.
    pub role: Option<Role>,
}
