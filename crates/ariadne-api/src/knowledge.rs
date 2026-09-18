//! Knowledge base DTOs: the symbol index over every registered repository.

use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

/// Where a repository's index stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeState {
    /// Indexed, and nothing is running.
    Idle,
    /// An index run is under way.
    Indexing,
    /// The last run failed; `error` says why.
    Failed,
    /// `knowledge_enabled = false`: nothing is indexed and nothing runs.
    Disabled,
}

/// One ref indexed for a repository.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct KnowledgeRefDto {
    pub git_ref: String,
    /// The commit the ref was last read at.
    pub commit: String,
    pub indexed_at: String,
    pub files: i64,
    pub symbols: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct KnowledgeLanguageDto {
    pub language: String,
    pub files: i64,
}

/// Response of `GET /v1/repositories/{id}/knowledge`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct KnowledgeStatusDto {
    pub repository_id: String,
    pub state: KnowledgeState,
    pub refs: Vec<KnowledgeRefDto>,
    /// Distinct paths indexed across the repository's refs.
    pub files: i64,
    /// Symbols of the files those paths hold.
    pub symbols: i64,
    pub languages: Vec<KnowledgeLanguageDto>,
    /// Why the last run failed, on a `failed` repository.
    pub error: Option<String>,
}

/// Query of `GET /v1/knowledge/search`.
#[derive(Debug, Clone, Default, Deserialize, Serialize, IntoParams)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeSearchQuery {
    /// The identifier to find. Words, camelCase and snake_case parts all
    /// match, each as a prefix.
    pub q: String,
    /// One repository id. Omit it for the caller's repositories: an agent
    /// session's goal, or every repository for a user.
    pub repository: Option<String>,
    /// Search every registered repository. Only an agent session needs it.
    pub all: Option<bool>,
    /// The branch to read. Omit it for the caller's own: a task session's
    /// branch, else the base branch of each repository.
    pub git_ref: Option<String>,
    /// Only symbols of this kind: `function`, `method`, `class`, `module`,
    /// `interface`, `macro`, `constant`, `test`, `heading`.
    pub kind: Option<String>,
    /// Only paths that contain this text.
    pub path: Option<String>,
    /// How many results at most (default 20, max 50).
    pub limit: Option<i64>,
}

impl KnowledgeSearchQuery {
    pub const DEFAULT_LIMIT: i64 = 20;
    pub const MAX_LIMIT: i64 = 50;

    pub fn limit(&self) -> i64 {
        self.limit
            .unwrap_or(Self::DEFAULT_LIMIT)
            .clamp(1, Self::MAX_LIMIT)
    }
}

/// One search answer.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct KnowledgeHitDto {
    pub repository_id: String,
    pub path: String,
    /// The line the definition starts on, 1-based.
    pub line: i64,
    pub kind: String,
    pub name: String,
    pub signature: String,
}

/// Query of `GET /v1/knowledge/outline`.
#[derive(Debug, Clone, Default, Deserialize, Serialize, IntoParams)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeOutlineQuery {
    pub repository: String,
    /// The path of the file, relative to the repository root.
    pub path: String,
    /// The branch to read. Omit it for the caller's own: a task session's
    /// branch, else the base branch.
    pub git_ref: Option<String>,
}

/// One definition of an outline.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct KnowledgeOutlineEntryDto {
    pub kind: String,
    pub name: String,
    /// 1-based, inclusive.
    pub start_line: i64,
    pub end_line: i64,
    pub signature: String,
}

/// Payload of `knowledge_indexed`: one ref of one repository was read.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct KnowledgeIndexedDto {
    pub repository_id: String,
    pub git_ref: String,
    pub commit: String,
    pub files: i64,
    pub symbols: i64,
}

/// Payload of `knowledge_failed`: an index run of a repository failed.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct KnowledgeFailedDto {
    pub repository_id: String,
    pub error: String,
}
