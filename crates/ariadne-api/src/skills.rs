//! Skill DTOs.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SkillDto {
    /// Kebab-case; how an agent loads the skill and how a task names it.
    #[schema(example = "code-review")]
    pub name: String,
    /// The one line the skill says about itself, read off the `description`
    /// of its frontmatter. It is what an agent sees before it opens the
    /// document, and what a listing shows.
    pub summary: String,
    /// The whole `SKILL.md`: YAML frontmatter naming the skill and describing
    /// it, then the body. This is the text set on the skill, or the one
    /// Ariadne ships while a built-in has none of its own.
    pub document: String,
    /// Whether `document` is the shipped text rather than one somebody wrote.
    pub document_is_default: bool,
    /// Whether Ariadne ships this skill. A built-in is reset rather than
    /// deleted; a skill of the user's own is deleted rather than reset.
    pub builtin: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateSkillRequest {
    /// Kebab-case, and free: a name Ariadne already ships is refused.
    #[schema(example = "api-design")]
    pub name: String,
    /// The whole `SKILL.md`, frontmatter included. A skill of the user's own
    /// has no shipped text behind it, so it carries its own.
    pub document: String,
}

/// Partial update; absent fields stay unchanged.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateSkillRequest {
    /// The new document. Absent = unchanged; putting a built-in back on the
    /// text Ariadne ships is `POST /v1/skills/{name}/document/reset`.
    pub document: Option<String>,
}
