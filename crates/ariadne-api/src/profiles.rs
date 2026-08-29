//! Profile DTOs.

use ariadne_core::{PromptKind, Role};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ProfileDto {
    pub id: String,
    pub name: String,
    pub role: Role,
    /// What this profile runs on, `<agent_kind>[:<model>]`: the agent CLI and,
    /// after a `:`, the model of it. None = auto: the first installed agent
    /// CLI (claude_code, then codex, then opencode), resolved at spawn time,
    /// on its own default model.
    #[schema(example = "claude_code:claude-opus-5")]
    pub model: Option<String>,
    /// The reasoning effort that model is run at, one of the efforts
    /// `GET /v1/models` lists for it. None = whatever the agent CLI runs it
    /// at on its own.
    #[schema(example = "high")]
    pub effort: Option<String>,
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
    /// What this profile runs on, `<agent_kind>[:<model>]` — the agent CLI
    /// and, after a `:`, the model of it: `codex`, `codex:gpt-5.3-codex`,
    /// `opencode:ollama/llama3:8b`. A string naming no agent CLI is refused.
    /// Omitted (or "default") = auto: the first installed agent CLI at spawn
    /// time, on its own default model.
    #[schema(example = "codex:gpt-5.3-codex")]
    pub model: Option<String>,
    /// The reasoning effort to run that model at, one of the efforts
    /// `GET /v1/models` lists for it; anything else is refused. Omitted (or
    /// "default") = whatever the agent CLI runs the model at on its own. An
    /// effort is run at a model, and a profile created on auto has none of its
    /// own, so an effort written where `model` names none is refused.
    #[serde(default)]
    #[schema(example = "high")]
    pub effort: Option<String>,
    /// Absent or null = the default of the role, which the profile then
    /// follows. Briefings are set afterwards, one `PUT` per kind.
    #[serde(default)]
    pub system_prompt: Option<String>,
}

/// Partial update; absent fields stay unchanged.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct UpdateProfileRequest {
    pub name: Option<String>,
    /// What this profile runs on, `<agent_kind>[:<model>]`, or "default" (or
    /// the empty string) to clear it back to auto — the first installed CLI at
    /// spawn time, on its own default model. Absent = unchanged.
    #[schema(example = "codex:gpt-5.3-codex")]
    pub model: Option<String>,
    /// The reasoning effort to run the model at: absent leaves it alone,
    /// "default" (or the empty string) puts it back on whatever the agent CLI
    /// runs the model at, and anything else is checked against the model it
    /// will run at — the one this request names, or the profile's own where it
    /// names none — and refused where that model does not take it. A `model`
    /// written without an effort runs at the CLI's own default: the effort
    /// belonged to the model that was left behind.
    #[schema(example = "high")]
    pub effort: Option<String>,
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
