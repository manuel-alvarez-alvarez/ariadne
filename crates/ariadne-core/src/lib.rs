//! Ariadne domain types.
//!
//! Pure domain layer: no IO, no async. Everything here is shared by the
//! daemon, the store, the API DTOs and the CLI.

pub mod codex_hooks;
pub mod id;
pub mod models;
pub mod spawn_plan;
pub mod state_machine;

pub use state_machine::{Actor, TaskStatus, TransitionError, check_transition};

use serde::{Deserialize, Serialize};

/// The role an agent plays in the orchestration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(
    feature = "clap",
    derive(clap::ValueEnum),
    value(rename_all = "snake_case")
)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Planner,
    Engineer,
    Reviewer,
    /// Takes an approved task over from the engineer and lands it on the base
    /// branch.
    Integrator,
}

impl Role {
    pub const ALL: [Role; 4] = [
        Role::Planner,
        Role::Engineer,
        Role::Reviewer,
        Role::Integrator,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            Role::Planner => "planner",
            Role::Engineer => "engineer",
            Role::Reviewer => "reviewer",
            Role::Integrator => "integrator",
        }
    }
}

impl std::str::FromStr for Role {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Role::ALL
            .into_iter()
            .find(|v| v.as_str() == s)
            .ok_or_else(|| format!("unknown role: {s}"))
    }
}

/// A prompt a profile owns beside its system prompt: the briefing an agent of
/// that role is started or resumed with. Each kind belongs to exactly one role
/// (see [`PromptKind::role`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(
    feature = "clap",
    derive(clap::ValueEnum),
    value(rename_all = "snake_case")
)]
#[serde(rename_all = "snake_case")]
pub enum PromptKind {
    /// Initial briefing of a planner session.
    PlannerBriefing,
    /// Initial briefing of an engineer session.
    EngineerBriefing,
    /// Engineer resume briefing carrying the reviewers' change requests.
    ChangesRequested,
    /// Initial briefing of a reviewer session.
    ReviewerBriefing,
    /// Reviewer resume briefing for a later round of the same task.
    ReviewerResume,
    /// Initial briefing of an integrator session: how the approved change is
    /// landed on its base branch.
    IntegrationInstructions,
    /// Integrator resume briefing for a later attempt at the same task.
    IntegrationResume,
}

impl PromptKind {
    pub const ALL: [PromptKind; 7] = [
        PromptKind::PlannerBriefing,
        PromptKind::EngineerBriefing,
        PromptKind::ChangesRequested,
        PromptKind::ReviewerBriefing,
        PromptKind::ReviewerResume,
        PromptKind::IntegrationInstructions,
        PromptKind::IntegrationResume,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            PromptKind::PlannerBriefing => "planner_briefing",
            PromptKind::EngineerBriefing => "engineer_briefing",
            PromptKind::ChangesRequested => "changes_requested",
            PromptKind::ReviewerBriefing => "reviewer_briefing",
            PromptKind::ReviewerResume => "reviewer_resume",
            PromptKind::IntegrationInstructions => "integration_instructions",
            PromptKind::IntegrationResume => "integration_resume",
        }
    }

    /// The role whose profiles own this prompt.
    pub fn role(&self) -> Role {
        match self {
            PromptKind::PlannerBriefing => Role::Planner,
            PromptKind::EngineerBriefing | PromptKind::ChangesRequested => Role::Engineer,
            PromptKind::ReviewerBriefing | PromptKind::ReviewerResume => Role::Reviewer,
            PromptKind::IntegrationInstructions | PromptKind::IntegrationResume => Role::Integrator,
        }
    }

    /// The prompts a profile of `role` owns, in briefing order.
    pub fn for_role(role: Role) -> &'static [PromptKind] {
        match role {
            Role::Planner => &[PromptKind::PlannerBriefing],
            Role::Engineer => &[PromptKind::EngineerBriefing, PromptKind::ChangesRequested],
            Role::Reviewer => &[PromptKind::ReviewerBriefing, PromptKind::ReviewerResume],
            Role::Integrator => &[
                PromptKind::IntegrationInstructions,
                PromptKind::IntegrationResume,
            ],
        }
    }

    /// The placeholders the daemon fills in when it renders this kind's
    /// template, in the order its briefing builder passes them.
    ///
    /// This list is the contract between the templates and the `prompts`
    /// builders in the daemon: a `{token}` outside it is one nothing will ever
    /// substitute, which is why [`PromptKind::validate_template`] refuses it
    /// when a template is saved. Adding a value to a builder means adding its
    /// name here.
    pub fn placeholders(&self) -> &'static [&'static str] {
        match self {
            PromptKind::PlannerBriefing => &[
                "goal_title",
                "goal_description",
                "repositories",
                "max_tasks",
                "required_approvals",
            ],
            PromptKind::EngineerBriefing => &[
                "task_title",
                "task_description",
                "goal_title",
                "worktree_path",
                "branch",
                "base_branch",
                "repo_path",
                "dependencies",
            ],
            PromptKind::ChangesRequested => &["feedback"],
            PromptKind::ReviewerBriefing => &[
                "task_title",
                "review_round",
                "task_description",
                "goal_title",
                "branch",
                "base_branch",
                "repo_path",
                "summary",
            ],
            // Fewer than the initial briefing: a resumed reviewer is told what
            // moved under it, and the goal and the repository are things it
            // already read last round.
            PromptKind::ReviewerResume => &["review_round", "task_title", "branch", "summary"],
            PromptKind::IntegrationInstructions => &[
                "task_title",
                "task_description",
                "goal_title",
                "worktree_path",
                "branch",
                "base_branch",
                "repo_path",
            ],
            // Fewer than the initial briefing, for the reviewer's reason: a
            // resumed integrator is told what moved under it, and the task it
            // is landing is one it has already read.
            PromptKind::IntegrationResume => &["task_title", "branch", "base_branch", "repo_path"],
        }
    }

    /// Refuse a template that names a placeholder this kind has no value for.
    ///
    /// Rendering is lenient by design — an unknown `{token}` reaches the agent
    /// as literal text rather than failing its spawn — so a typo like
    /// `{task_titel}` is invisible until someone reads a briefing. Saving is
    /// where it is caught instead, and only there: this is never called on the
    /// way to an agent.
    ///
    /// Only what rendering would treat as a placeholder is checked, and of
    /// that only plain identifiers. A brace that never closes, a `{}` and a
    /// JSON snippet are all text as far as rendering is concerned, so they are
    /// text here too.
    pub fn validate_template(&self, template: &str) -> Result<(), UnknownPlaceholders> {
        let mut unknown: Vec<String> = Vec::new();
        for name in placeholder_names(template) {
            if !is_identifier(name)
                || self.placeholders().contains(&name)
                || unknown.iter().any(|seen| seen == name)
            {
                continue;
            }
            unknown.push(name.to_string());
        }
        if unknown.is_empty() {
            return Ok(());
        }
        Err(UnknownPlaceholders {
            kind: *self,
            unknown,
        })
    }
}

/// A template saved with `{token}`s the daemon has no value for, and the kind
/// that would have had to fill them in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownPlaceholders {
    pub kind: PromptKind,
    /// The offending names, without braces, in the order they appear.
    pub unknown: Vec<String>,
}

impl std::fmt::Display for UnknownPlaceholders {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let braced = |names: &mut dyn Iterator<Item = &str>| {
            names
                .map(|n| format!("{{{n}}}"))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let unknown = braced(&mut self.unknown.iter().map(String::as_str));
        let allowed = braced(&mut self.kind.placeholders().iter().copied());
        let plural = if self.unknown.len() == 1 {
            "placeholder"
        } else {
            "placeholders"
        };
        write!(
            f,
            "the {} template has no value for {plural} {unknown}; \
             the ones it can use are {allowed}",
            self.kind.as_str()
        )
    }
}

impl std::error::Error for UnknownPlaceholders {}

/// The names rendering would look up in a template, in order and with repeats.
///
/// Deliberately the same scan as the daemon's `render`: a name runs from a `{`
/// to the next `}` with no `{` in between, and anything else is text.
fn placeholder_names(template: &str) -> Vec<&str> {
    let mut names = Vec::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        match after.find(['{', '}']) {
            Some(end) if after.as_bytes()[end] == b'}' => {
                names.push(&after[..end]);
                rest = &after[end + 1..];
            }
            // An unclosed brace, or one closed only after another `{`: not a
            // placeholder, so the scan carries on from just after it.
            _ => rest = after,
        }
    }
    names
}

/// Whether a name is one a placeholder could plausibly be spelled with:
/// `{"repo": "x"}` and `{}` are not typos to be corrected, they are text.
fn is_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

impl std::str::FromStr for PromptKind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        PromptKind::ALL
            .into_iter()
            .find(|v| v.as_str() == s)
            .ok_or_else(|| format!("unknown prompt kind: {s}"))
    }
}

/// Which coding-agent CLI a profile runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum AgentKind {
    ClaudeCode,
    Codex,
    Opencode,
}

impl AgentKind {
    pub const ALL: [AgentKind; 3] = [AgentKind::ClaudeCode, AgentKind::Codex, AgentKind::Opencode];

    pub fn as_str(&self) -> &'static str {
        match self {
            AgentKind::ClaudeCode => "claude_code",
            AgentKind::Codex => "codex",
            AgentKind::Opencode => "opencode",
        }
    }

    /// The executable this agent CLI is launched as, and the name anything
    /// looking for it on a `PATH` searches for.
    pub fn binary(&self) -> &'static str {
        match self {
            AgentKind::ClaudeCode => "claude",
            AgentKind::Codex => "codex",
            AgentKind::Opencode => "opencode",
        }
    }

    /// The argv flags Ariadne launches this agent CLI with out of the box:
    /// the permission bypass each one spells its own way, so an agent working
    /// unattended in a throwaway worktree is not left waiting at a prompt.
    ///
    /// This is what a fresh database seeds the agent's flag list with and what
    /// restoring the defaults puts back; from there the list is the user's,
    /// edited over `/v1/agents`. Only flags a user may reasonably drop belong
    /// here — the structural ones (session ids, MCP and hook config, the
    /// system prompt, the model) are the adapters' own and are not negotiable.
    pub fn default_flags(&self) -> &'static [&'static str] {
        match self {
            AgentKind::ClaudeCode => &["--dangerously-skip-permissions"],
            AgentKind::Codex => &["--dangerously-bypass-approvals-and-sandbox"],
            // "auto-approve permissions that are not explicitly denied
            // (dangerous!)" — `opencode --help`, v1.18.15. The generated
            // `opencode.json` already allows the tools; this covers whatever
            // asks for approval outside it.
            AgentKind::Opencode => &["--auto"],
        }
    }
}

impl std::str::FromStr for AgentKind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        AgentKind::ALL
            .into_iter()
            .find(|v| v.as_str() == s)
            .ok_or_else(|| format!("unknown agent kind: {s}"))
    }
}

/// Goal lifecycle status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(
    feature = "clap",
    derive(clap::ValueEnum),
    value(rename_all = "snake_case")
)]
#[serde(rename_all = "snake_case")]
pub enum GoalStatus {
    /// Planner session active; tasks being defined.
    Planning,
    /// Plan finalized; tasks executing.
    Active,
    /// All tasks merged (or goal-level completion recorded).
    Completed,
    Cancelled,
}

impl GoalStatus {
    pub const ALL: [GoalStatus; 4] = [
        GoalStatus::Planning,
        GoalStatus::Active,
        GoalStatus::Completed,
        GoalStatus::Cancelled,
    ];

    /// Nothing more will happen to this goal: no session of its own, no task
    /// left to move it. What may be deleted, and what cancelling refuses.
    pub fn is_terminal(&self) -> bool {
        matches!(self, GoalStatus::Completed | GoalStatus::Cancelled)
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            GoalStatus::Planning => "planning",
            GoalStatus::Active => "active",
            GoalStatus::Completed => "completed",
            GoalStatus::Cancelled => "cancelled",
        }
    }
}

impl std::str::FromStr for GoalStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        GoalStatus::ALL
            .into_iter()
            .find(|v| v.as_str() == s)
            .ok_or_else(|| format!("unknown goal status: {s}"))
    }
}

/// Agent session lifecycle status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(
    feature = "clap",
    derive(clap::ValueEnum),
    value(rename_all = "snake_case")
)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    /// Tmux session created, agent booting.
    Starting,
    /// Agent actively working.
    Running,
    /// Agent finished its turn and is waiting (Stop / turn-complete / idle).
    Idle,
    /// Tmux session gone or agent process exited.
    Exited,
    /// Spawn or runtime failure.
    Failed,
}

impl SessionStatus {
    pub const ALL: [SessionStatus; 5] = [
        SessionStatus::Starting,
        SessionStatus::Running,
        SessionStatus::Idle,
        SessionStatus::Exited,
        SessionStatus::Failed,
    ];

    pub fn is_live(&self) -> bool {
        matches!(
            self,
            SessionStatus::Starting | SessionStatus::Running | SessionStatus::Idle
        )
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            SessionStatus::Starting => "starting",
            SessionStatus::Running => "running",
            SessionStatus::Idle => "idle",
            SessionStatus::Exited => "exited",
            SessionStatus::Failed => "failed",
        }
    }
}

impl std::str::FromStr for SessionStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        SessionStatus::ALL
            .into_iter()
            .find(|v| v.as_str() == s)
            .ok_or_else(|| format!("unknown session status: {s}"))
    }
}

/// Why a live agent session needs the user's attention.
///
/// Orthogonal to [`SessionStatus`]: a session waiting on a permission prompt
/// is still `running` as far as its lifecycle goes, it just cannot make
/// progress until someone looks at it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum AttentionReason {
    /// Blocked on a permission / approval prompt.
    WaitingPermission,
    /// The agent asked the user something and is idle until answered.
    WaitingInput,
    /// The agent reported an error (API error, crash, `session.error`).
    AgentError,
    /// Tmux session or agent process gone while its work is still active.
    Disconnected,
    /// No activity for too long.
    Stalled,
}

impl AttentionReason {
    pub const ALL: [AttentionReason; 5] = [
        AttentionReason::WaitingPermission,
        AttentionReason::WaitingInput,
        AttentionReason::AgentError,
        AttentionReason::Disconnected,
        AttentionReason::Stalled,
    ];

    /// Whether this reason describes a dialog on the agent's own terminal.
    ///
    /// Only a live session can be sitting on one: a permission prompt and a
    /// question are things somebody types an answer into, and a pane that is
    /// gone has neither. The other reasons are the ones a session ends
    /// *carrying* — an error it reported, a disconnect, a stall — and they
    /// stay true after the agent has stopped.
    pub fn is_prompt(&self) -> bool {
        matches!(
            self,
            AttentionReason::WaitingPermission | AttentionReason::WaitingInput
        )
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            AttentionReason::WaitingPermission => "waiting_permission",
            AttentionReason::WaitingInput => "waiting_input",
            AttentionReason::AgentError => "agent_error",
            AttentionReason::Disconnected => "disconnected",
            AttentionReason::Stalled => "stalled",
        }
    }
}

impl std::str::FromStr for AttentionReason {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        AttentionReason::ALL
            .into_iter()
            .find(|v| v.as_str() == s)
            .ok_or_else(|| format!("unknown attention reason: {s}"))
    }
}

/// Review verdict for one reviewer in one round.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum ReviewVerdict {
    Approve,
    RequestChanges,
}

impl ReviewVerdict {
    pub fn as_str(&self) -> &'static str {
        match self {
            ReviewVerdict::Approve => "approve",
            ReviewVerdict::RequestChanges => "request_changes",
        }
    }
}

impl std::str::FromStr for ReviewVerdict {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "approve" => Ok(ReviewVerdict::Approve),
            "request_changes" => Ok(ReviewVerdict::RequestChanges),
            other => Err(format!("unknown review verdict: {other}")),
        }
    }
}

/// Author of a conversation message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum AuthorRole {
    Planner,
    Engineer,
    Reviewer,
    Integrator,
    User,
    System,
}

impl AuthorRole {
    pub const ALL: [AuthorRole; 6] = [
        AuthorRole::Planner,
        AuthorRole::Engineer,
        AuthorRole::Reviewer,
        AuthorRole::Integrator,
        AuthorRole::User,
        AuthorRole::System,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            AuthorRole::Planner => "planner",
            AuthorRole::Engineer => "engineer",
            AuthorRole::Reviewer => "reviewer",
            AuthorRole::Integrator => "integrator",
            AuthorRole::User => "user",
            AuthorRole::System => "system",
        }
    }
}

impl std::str::FromStr for AuthorRole {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        AuthorRole::ALL
            .into_iter()
            .find(|v| v.as_str() == s)
            .ok_or_else(|| format!("unknown author role: {s}"))
    }
}

/// Who a conversation message is addressed to: one agent profile, or the
/// human user. Orthogonal to the author role, and optional — a message with
/// no recipient is addressed to the thread rather than to anyone in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum RecipientKind {
    Profile,
    User,
}

impl RecipientKind {
    pub const ALL: [RecipientKind; 2] = [RecipientKind::Profile, RecipientKind::User];

    pub fn as_str(&self) -> &'static str {
        match self {
            RecipientKind::Profile => "profile",
            RecipientKind::User => "user",
        }
    }
}

impl std::str::FromStr for RecipientKind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        RecipientKind::ALL
            .into_iter()
            .find(|v| v.as_str() == s)
            .ok_or_else(|| format!("unknown recipient kind: {s}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The names a kind's briefing builder passes are the names its template
    /// may use — no more, and none missing.
    #[test]
    fn every_kind_names_the_placeholders_its_briefing_fills_in() {
        for kind in PromptKind::ALL {
            let allowed = kind.placeholders();
            assert!(!allowed.is_empty(), "{} has no placeholders", kind.as_str());
            let template = allowed
                .iter()
                .map(|name| format!("{{{name}}}"))
                .collect::<Vec<_>>()
                .join(" ");
            assert_eq!(kind.validate_template(&template), Ok(()));
        }
    }

    #[test]
    fn a_typo_is_refused_with_the_token_and_the_allowed_set() {
        let err = PromptKind::EngineerBriefing
            .validate_template("# {task_titel}\n\n{task_description}")
            .unwrap_err();
        assert_eq!(err.unknown, ["task_titel"]);
        let message = err.to_string();
        assert!(message.contains("engineer_briefing"), "{message}");
        assert!(message.contains("{task_titel}"), "{message}");
        assert!(message.contains("{task_title}"), "{message}");
        assert!(message.contains("{dependencies}"), "{message}");
    }

    /// Every offending token is named, once each: a template is fixed in one
    /// pass, not one save per typo.
    #[test]
    fn every_unknown_token_is_named_once() {
        let err = PromptKind::ChangesRequested
            .validate_template("{feedback} {who} {what} {who}")
            .unwrap_err();
        assert_eq!(err.unknown, ["who", "what"]);
        assert!(err.to_string().contains("{who}, {what}"), "{err}");
    }

    /// A placeholder of another kind's briefing is still one this kind cannot
    /// fill in: the sets are per kind, not one pool.
    #[test]
    fn a_placeholder_of_another_kind_is_unknown_here() {
        assert!(
            PromptKind::PlannerBriefing
                .validate_template("Plan {goal_title} for {task_title}.")
                .is_err()
        );
        // The reviewer's resume is briefed with less than its first round.
        assert!(
            PromptKind::ReviewerResume
                .validate_template("Round {review_round} of {task_title} in {repo_path}.")
                .is_err()
        );
        assert_eq!(
            PromptKind::ReviewerBriefing
                .validate_template("Round {review_round} of {task_title} in {repo_path}."),
            Ok(())
        );
    }

    /// Whatever rendering treats as text, validation treats as text: braces
    /// that never close, empty names, JSON, non-identifier noise. Rejecting
    /// those would refuse templates that render exactly as written.
    #[test]
    fn what_is_not_a_placeholder_is_not_checked() {
        for template in [
            "",
            "Just read the diff.",
            "{unclosed and {task_title}",
            "} {task_title}",
            "{}",
            "{{{{",
            "{ü}",
            "Answer with {\"verdict\": \"approve\"} and nothing else.",
            "The set is {task_title, branch}.",
            r"printf '%s' {} \;",
        ] {
            assert_eq!(
                PromptKind::EngineerBriefing.validate_template(template),
                Ok(()),
                "refused text that renders as itself: {template}"
            );
        }
    }

    /// A briefing that uses none of its placeholders is a developer's call,
    /// not a mistake: dropping a value has never been an error.
    #[test]
    fn a_template_may_use_none_of_its_placeholders() {
        assert_eq!(
            PromptKind::IntegrationInstructions.validate_template("Land it yourself."),
            Ok(())
        );
    }
}
