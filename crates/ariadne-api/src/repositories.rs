//! Repository DTOs.

use ariadne_core::MergeStrategy;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RepositoryDto {
    pub id: String,
    /// Absolute path of the checkout.
    pub path: String,
    pub base_branch: String,
    pub description: Option<String>,
    /// How a task lands on `base_branch` here.
    pub merge_strategy: MergeStrategy,
    /// The landing briefing the engineer of an approved task is handed here:
    /// the text set on this repository, or the built-in default of its merge
    /// strategy while it has none of its own.
    pub landing_prompt: String,
    /// Whether `landing_prompt` is that strategy default rather than a text
    /// set on this repository.
    pub landing_prompt_is_default: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateRepositoryRequest {
    /// Absolute path of an existing git work tree.
    #[schema(example = "/home/me/dev/ariadne")]
    pub path: String,
    /// Omit for the repo's currently checked-out branch.
    pub base_branch: Option<String>,
    pub description: Option<String>,
    /// Omit for `direct`.
    pub merge_strategy: Option<MergeStrategy>,
    /// The landing briefing this repository hands its engineer. Omitted or
    /// empty = the built-in default of `merge_strategy`, which
    /// `GET /v1/merge-strategies` hands out for prefilling. A briefing may
    /// use only the placeholders a landing text is rendered with
    /// (`{task_title}`, `{branch}`, `{base_branch}`, `{repo_path}`); one that
    /// names another is refused.
    #[serde(default)]
    pub landing_prompt: Option<String>,
}

/// Partial update; absent fields stay unchanged.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateRepositoryRequest {
    pub path: Option<String>,
    pub base_branch: Option<String>,
    /// New description, or empty to clear it. Absent = unchanged.
    pub description: Option<String>,
    pub merge_strategy: Option<MergeStrategy>,
    /// New landing briefing, or empty to put it back on the built-in default
    /// of the merge strategy in force. Absent = unchanged, which is also what
    /// a `merge_strategy` written on its own does to it: the words are the
    /// user's, and the reset is what asks for the new strategy's text. A
    /// briefing may use only the placeholders a landing text is rendered with
    /// (`{task_title}`, `{branch}`, `{base_branch}`, `{repo_path}`); one that
    /// names another is refused.
    pub landing_prompt: Option<String>,
}

/// One merge strategy and the landing briefing a repository on it runs on
/// while it has none of its own: what `GET /v1/merge-strategies` lists, so a
/// client can show and prefill a landing prompt before the repository exists.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MergeStrategyDto {
    pub merge_strategy: MergeStrategy,
    /// The built-in landing briefing of this strategy.
    pub landing_prompt: String,
}
