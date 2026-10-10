//! `GET /v1/attention`: the authoritative list of items that need a human.
//!
//! An item exists only while a person has something to do about it:
//! automatic recovery and the orchestrator are left to the situations they
//! can carry on their own, and only a blocker they cannot clear — or one no
//! retry loop covers at all — reaches this list. Each item names its cause,
//! what was tried, who it affects, how long it has waited, and the one
//! action that clears it.
//!
//! This is produced by daemon-side producers, one per kind of blocker
//! (recovery, in this first path; an agent's own request and a pull
//! request's next step are later ones). A client reads this list rather than
//! inferring attention itself from tasks and sessions, so every surface
//! agrees on what needs a person and what the daemon is still trying.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// What kind of blocker an item is, in the one vocabulary every producer
/// shares. `Unknown` is for a blocker a producer raises without reliable
/// evidence of which of the other four it is — it is never grouped with
/// another item, unknown or not (009, "Unknown causes remain separate").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AttentionCause {
    /// Signed out, or otherwise refused by the service a credential answers.
    Access,
    /// A usage, rate or credit limit with no more automatic way around it.
    Quota,
    /// Something to fix in how the daemon or a tool is set up.
    Configuration,
    /// A machine resource — a descriptor limit, disk, memory — the daemon
    /// is waiting on rather than the agent.
    Resource,
    /// A blocker with no reliable cause to name.
    Unknown,
}

/// What an item is about: an entity whose work the blocker touches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AttentionSubjectKind {
    Goal,
    Task,
    Session,
    Repository,
}

/// Which producer raised an item — distinct from [`AttentionCause`], since
/// a cause describes *why* a blocker exists and a later producer (an
/// agent's own request, a pull request's next step) may share none of
/// recovery's causes, or raise `unknown` for a reason a client still needs
/// to tell apart from recovery's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AttentionProducer {
    /// `crate::attention::recovery` (`ariadne-daemon`): a blocker automatic
    /// recovery has given up on, or that no retry loop covers at all.
    Recovery,
    /// Not produced yet: a session's own question to the user.
    AgentRequest,
    /// Not produced yet: a pull request's next step.
    PullRequest,
}

/// One entity an item's blocker affects.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct AttentionSubjectDto {
    pub kind: AttentionSubjectKind,
    pub id: String,
    /// What to call it where there is room for one word more than the id —
    /// a task's title, a session's model.
    pub label: String,
}

/// Where opening an item takes a client. Opening a target never resolves
/// the item on its own — only the thing the item names doing so does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AttentionTarget {
    /// The console of the named session.
    Console { session_id: String },
    /// The named task.
    Task { task_id: String },
    /// The named pull request.
    PullRequest { pull_request_id: String },
    /// A settings section, named for the client's own routing.
    Settings { section: String },
}

/// One item on the Needs attention list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct AttentionItemDto {
    /// Stable across a restart and across the same cause recurring: derived
    /// from the cause and what it shares rather than issued fresh, so the
    /// same blocker is the same row for as long as it stands.
    pub id: String,
    pub producer: AttentionProducer,
    /// The shared contract's `reason`: why this blocker exists, in the one
    /// vocabulary every producer draws from (`AttentionCause`).
    pub reason: AttentionCause,
    /// What is blocked and why, in one line.
    pub summary: String,
    /// The one action that clears this item.
    pub required_action: String,
    /// When this blocker was first observed, RFC 3339.
    pub since: String,
    /// Every entity this blocker's affected work names. A grouped item
    /// (009, "A shared access or resource failure produces one grouped
    /// item") lists every one of them here rather than splitting into an
    /// item per entity.
    pub affected: Vec<AttentionSubjectDto>,
    pub target: AttentionTarget,
}

/// The whole list, as `GET /v1/attention` answers it.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AttentionListDto {
    pub items: Vec<AttentionItemDto>,
    /// False where a producer's read failed, so a partial list is never
    /// read as an all-clear: it still answers with whatever the other
    /// producers found.
    pub complete: bool,
}
