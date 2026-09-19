//! Memory DTOs: the facts kept about one repository, or about every one.

use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MemoryDto {
    pub id: String,
    /// The repository the fact is about, or null when it is global.
    pub repository_id: Option<String>,
    pub text: String,
    /// The session that saved the fact, and its task and goal. All null when
    /// the user saved it.
    pub source_session_id: Option<String>,
    pub source_task_id: Option<String>,
    pub source_goal_id: Option<String>,
    pub created_at: String,
    /// The RFC 3339 time after which this entry stays hidden, or null when it
    /// never expires.
    pub expires_at: Option<String>,
}

/// Payload of `memory_deleted`: the entry that went, and its scope.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MemoryDeletedDto {
    pub id: String,
    /// The repository the entry was about, or null when it was global.
    pub repository_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateMemoryRequest {
    pub text: String,
    /// The repository the fact is about. Omit it to save a global fact.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository_id: Option<String>,
    /// The RFC 3339 time after which this entry stays hidden. Omit it for an
    /// entry that never expires.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
}

/// Which memories a list or a search reads.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum MemoryScope {
    /// The named repository alone.
    Repository,
    /// The memories of no repository.
    Global,
    /// The named repository and the global memories.
    #[default]
    All,
}

/// Query of `GET /v1/memories`.
#[derive(Debug, Clone, Default, Deserialize, Serialize, IntoParams)]
#[serde(deny_unknown_fields)]
pub struct MemoryListQuery {
    /// Read the memories of this repository. Omit it for every repository the
    /// caller may read.
    pub repository: Option<String>,
    /// `all` (default), `repository` or `global`.
    pub scope: Option<MemoryScope>,
}

/// Query of `GET /v1/memories/search`.
#[derive(Debug, Clone, Default, Deserialize, Serialize, IntoParams)]
#[serde(deny_unknown_fields)]
pub struct MemorySearchQuery {
    /// Find entries that hold any word, or a prefix of a word, in this text.
    pub q: String,
    /// Search the memories of this repository. Omit it for every repository
    /// the caller may read.
    pub repository: Option<String>,
    /// `all` (default), `repository` or `global`.
    pub scope: Option<MemoryScope>,
}

/// Search hits and whether newest memories stand in for a word match.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MemorySearchResult {
    /// Matching memories, or newest active memories when `fallback` is true.
    pub hits: Vec<MemoryDto>,
    /// True where no word matched and the newest memories are the answer.
    pub fallback: bool,
}
