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
    /// The last run of a ref failed; `failures` names the ref and says why.
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
    /// The refs whose last run failed, each with why. A good run of one ref
    /// leaves the failure of another here.
    pub failures: Vec<KnowledgeFailureDto>,
}

/// One ref whose last index run failed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct KnowledgeFailureDto {
    pub git_ref: String,
    pub error: String,
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
    /// `interface`, `type`, `macro`, `constant`, `test`, `heading`.
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

/// How much `GET /v1/knowledge/symbol` answers with.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeDetail {
    /// The definition and its signature.
    #[default]
    Outline,
    /// The text of the definition too.
    Source,
    /// The callers, callees, implementations and tests too.
    Context,
}

/// Query of `GET /v1/knowledge/symbol`.
#[derive(Debug, Clone, Default, Deserialize, Serialize, IntoParams)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeSymbolQuery {
    /// The name of the definition, spelled in full.
    pub name: String,
    /// One repository id. Omit it for the caller's repositories.
    pub repository: Option<String>,
    /// The branch to read. Omit it for the caller's own.
    pub git_ref: Option<String>,
    /// `outline` (default), `source` or `context`.
    pub detail: Option<KnowledgeDetail>,
}

/// One end of an edge: a caller, a callee, an implementation or a test.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct KnowledgeRelatedDto {
    pub repository_id: String,
    pub path: String,
    /// The line the definition starts on, 1-based.
    pub line: i64,
    pub name: String,
    /// `exact` where one definition matched the name, `heuristic` where
    /// several did.
    pub confidence: String,
    /// Which step answered the name: `file`, `directory`, `import` or
    /// `repository` within one repository, and `name` across two. Null where
    /// the end is a definition itself rather than the other end of an edge.
    pub step: Option<String>,
    /// How many definitions matched at that step.
    pub candidates: i64,
}

/// What one definition is joined to, on `detail=context`. Each list holds
/// the definition's own repository first, then the other repositories'
/// ends.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct KnowledgeContextDto {
    /// Up to 20 of each.
    pub callers: Vec<KnowledgeRelatedDto>,
    pub callees: Vec<KnowledgeRelatedDto>,
    pub implementations: Vec<KnowledgeRelatedDto>,
    /// What names it without calling it: a type annotation, an import, a
    /// reference from another repository.
    #[serde(default)]
    pub references: Vec<KnowledgeRelatedDto>,
    /// The tests at most two edges away.
    pub tests: Vec<KnowledgeRelatedDto>,
    /// How many entries each list held past its cap, 0 where the list is
    /// whole.
    #[serde(default)]
    pub more: KnowledgeContextMoreDto,
}

/// How many entries each list of a [`KnowledgeContextDto`] held back past
/// its cap, 0 where the list is whole.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct KnowledgeContextMoreDto {
    pub callers: i64,
    pub callees: i64,
    pub implementations: i64,
    pub references: i64,
    pub tests: i64,
}

/// One definition of a name.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct KnowledgeSymbolDto {
    pub repository_id: String,
    pub path: String,
    /// 1-based, inclusive.
    pub start_line: i64,
    pub end_line: i64,
    pub kind: String,
    pub name: String,
    pub signature: String,
    pub doc: Option<String>,
    /// The text of the definition, on `detail=source`.
    pub source: Option<String>,
    /// On `detail=context`.
    pub context: Option<KnowledgeContextDto>,
}

/// Query of `GET /v1/knowledge/impact`.
#[derive(Debug, Clone, Default, Deserialize, Serialize, IntoParams)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeImpactQuery {
    /// One repository id.
    pub repository: String,
    /// The branch to read. Omit it for the caller's own.
    pub git_ref: Option<String>,
    /// The name of the changed definition. Pass this or `diff`, never both.
    pub symbol: Option<String>,
    /// `<base>..<head>`: every definition the diff changed.
    pub diff: Option<String>,
    /// How far to walk the callers (default 2, max 4).
    pub depth: Option<i64>,
}

impl KnowledgeImpactQuery {
    pub const DEFAULT_DEPTH: i64 = 2;
    pub const MAX_DEPTH: i64 = 4;

    pub fn depth(&self) -> i64 {
        self.depth
            .unwrap_or(Self::DEFAULT_DEPTH)
            .clamp(1, Self::MAX_DEPTH)
    }
}

/// One caller of a changed definition, and how far from it.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct KnowledgeImpactCallerDto {
    /// 1 is a direct caller.
    pub depth: i64,
    pub repository_id: String,
    pub path: String,
    pub line: i64,
    pub name: String,
    pub confidence: String,
    /// Which step answered the name the call was resolved by.
    pub step: String,
    /// How many definitions matched at that step.
    pub candidates: i64,
}

/// What one changed definition reaches.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct KnowledgeImpactDto {
    pub symbol: KnowledgeRelatedDto,
    pub callers: Vec<KnowledgeImpactCallerDto>,
    /// The definitions the walk did not go past, each with more than 200
    /// callers.
    pub stopped: Vec<String>,
}

/// Query of `GET /v1/knowledge/path`.
#[derive(Debug, Clone, Default, Deserialize, Serialize, IntoParams)]
#[serde(deny_unknown_fields)]
pub struct KnowledgePathQuery {
    /// One repository id.
    pub repository: String,
    /// The name of every starting definition.
    pub from: String,
    /// The name of every ending definition.
    pub to: String,
    /// The branch to read. Omit it for the caller's own.
    pub git_ref: Option<String>,
    /// How far to walk the directed edges (default 6, max 10).
    pub depth: Option<i64>,
}

impl KnowledgePathQuery {
    pub const DEFAULT_DEPTH: i64 = 6;
    pub const MAX_DEPTH: i64 = 10;

    pub fn depth(&self) -> i64 {
        self.depth
            .unwrap_or(Self::DEFAULT_DEPTH)
            .clamp(0, Self::MAX_DEPTH)
    }
}

/// One definition on a shortest path. The edge fields name the edge into it.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct KnowledgePathHopDto {
    pub repository_id: String,
    pub path: String,
    /// The line the definition starts on, 1-based.
    pub line: i64,
    pub kind: String,
    pub name: String,
    /// Empty on the first hop.
    pub edge_kind: Option<String>,
    /// Empty on the first hop; otherwise `exact` or `heuristic`.
    pub confidence: Option<String>,
}

/// The shortest directed path between two symbol names.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct KnowledgePathDto {
    pub hops: Vec<KnowledgePathHopDto>,
}

/// Query of `GET /v1/knowledge/map`.
#[derive(Debug, Clone, Default, Deserialize, Serialize, IntoParams)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeMapQuery {
    /// One repository id.
    pub repository: String,
    /// The branch to read. Omit it for the caller's own.
    pub git_ref: Option<String>,
    /// Rank the files around this one first, and the rest after them.
    pub path: Option<String>,
    /// How long the map may be, in tokens (default 1000, max 4000). One
    /// token is four characters.
    pub budget: Option<i64>,
}

impl KnowledgeMapQuery {
    pub const DEFAULT_BUDGET: i64 = 1000;
    pub const MAX_BUDGET: i64 = 4000;

    pub fn budget(&self) -> i64 {
        self.budget
            .unwrap_or(Self::DEFAULT_BUDGET)
            .clamp(1, Self::MAX_BUDGET)
    }
}

/// The map of one ref: the files that carry it, ranked, with the definitions
/// most of the ref points at.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct KnowledgeMapDto {
    pub repository_id: String,
    pub git_ref: String,
    /// The map itself, plain text and under the budget.
    pub text: String,
    /// How long the text is, in tokens.
    pub tokens: i64,
    /// How many files the text names.
    pub files: i64,
    /// The ranked files the text did not hold, 0 where the budget held
    /// every one of them.
    #[serde(default)]
    pub files_left: i64,
}

/// Query of `GET /v1/knowledge/graph`.
#[derive(Debug, Clone, Default, Deserialize, Serialize, IntoParams)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeGraphQuery {
    /// One repository id.
    pub repository: String,
    /// The branch to read. Omit it for the caller's own.
    pub git_ref: Option<String>,
    /// How many file nodes to return (default 2000, max 10000).
    pub limit: Option<i64>,
}

impl KnowledgeGraphQuery {
    pub const DEFAULT_LIMIT: i64 = 2000;
    pub const MAX_LIMIT: i64 = 10_000;

    pub fn limit(&self) -> i64 {
        self.limit
            .unwrap_or(Self::DEFAULT_LIMIT)
            .clamp(1, Self::MAX_LIMIT)
    }
}

/// One file in a repository graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct KnowledgeGraphNodeDto {
    pub path: String,
    pub language: String,
    pub symbols: i64,
}

/// How confidently every symbol edge in a file edge was resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeGraphConfidence {
    Exact,
    Heuristic,
}

/// One grouped edge between two files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct KnowledgeGraphEdgeDto {
    pub from: String,
    pub to: String,
    pub kind: String,
    /// How many symbol-level edges this file edge groups.
    pub count: i64,
    /// `exact` only when every grouped edge is exact.
    pub confidence: KnowledgeGraphConfidence,
}

/// The files and file-to-file edges of one repository ref.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct KnowledgeGraphDto {
    pub repository_id: String,
    pub git_ref: String,
    pub nodes: Vec<KnowledgeGraphNodeDto>,
    pub edges: Vec<KnowledgeGraphEdgeDto>,
    pub truncated: bool,
    pub total_nodes: i64,
}

/// Query of `GET /v1/knowledge/interactions`.
#[derive(Debug, Clone, Default, Deserialize, Serialize, IntoParams)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeInteractionsQuery {
    /// One repository id.
    pub repository: String,
    /// The branch to read. Omit it for the caller's own.
    pub git_ref: Option<String>,
}

/// One end of an interaction.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct KnowledgeEndpointDto {
    pub repository_id: String,
    pub path: String,
    /// 1-based.
    pub line: i64,
    /// The definition at that end, or what the edge is about where the end
    /// is no definition: the package, the route, the variable.
    pub symbol: String,
}

/// One edge between two repositories.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct KnowledgeEdgeDto {
    pub from: KnowledgeEndpointDto,
    pub to: KnowledgeEndpointDto,
    /// `exact` or `heuristic`.
    pub confidence: String,
    /// Which step joined the two ends: `path` or `name` for a dependency,
    /// `route` for a route use, `name` for a variable and for a reference.
    pub step: String,
    /// How many definitions matched at that step.
    pub candidates: i64,
}

/// The interactions of one kind: `depends_on`, `references`, `calls_route`
/// or `sets_env`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct KnowledgeInteractionGroupDto {
    pub kind: String,
    pub edges: Vec<KnowledgeEdgeDto>,
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

/// Payload of `knowledge_failed`: an index run of one ref of a repository
/// failed.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct KnowledgeFailedDto {
    pub repository_id: String,
    pub git_ref: String,
    pub error: String,
}
