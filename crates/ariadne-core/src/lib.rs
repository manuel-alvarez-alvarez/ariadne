//! Ariadne domain types: what the daemon, the store, the API DTOs and the
//! CLI all have to agree on.
//!
//! Pure domain apart from [`probe`], which asks the host about the binaries
//! Ariadne runs — shared for the same reason as the rest, that two callers
//! answering the same question differently is the bug.

pub mod codex_hooks;
pub mod id;
pub mod models;
pub mod probe;
pub mod spawn_plan;
pub mod state_machine;

pub use models::TokenUsage;
pub use probe::{
    PROBE_TIMEOUT, PathState, is_executable, path_state, probe_auth, probe_status, probe_version,
    which,
};
pub use state_machine::{Actor, TaskStatus, TransitionError, check_transition};

use serde::{Deserialize, Serialize};

/// The three things every enum that crosses the wire answers to: the spelling
/// it is stored and transported under, the whole set of variants in the order
/// anything listing them uses, and the parse back. `$noun` is how a refused
/// string is named in the error.
macro_rules! wire_enum {
    ($name:ident, $noun:literal, [$($variant:ident = $text:literal),+ $(,)?]) => {
        impl $name {
            pub const ALL: [$name; [$(stringify!($variant)),+].len()] = [$($name::$variant),+];

            pub fn as_str(&self) -> &'static str {
                match self {
                    $($name::$variant => $text,)+
                }
            }
        }

        impl std::str::FromStr for $name {
            type Err = String;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                $name::ALL
                    .into_iter()
                    .find(|v| v.as_str() == s)
                    .ok_or_else(|| format!(concat!("unknown ", $noun, ": {}"), s))
            }
        }
    };
}
pub(crate) use wire_enum;

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
}

wire_enum! { Role, "role", [
    Planner = "planner",
    Engineer = "engineer",
    Reviewer = "reviewer",
]}

/// How a repository takes the change a task lands on its base branch: the one
/// thing about a repository the engineer that finishes a task has to be told,
/// since the commands it runs at the end differ entirely between the two.
///
/// Which forge a published request goes to is *not* here: `origin` says
/// whether it is GitHub or GitLab, and asking the remote at landing time
/// cannot go stale the way a second copy of the answer would.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(
    feature = "clap",
    derive(clap::ValueEnum),
    value(rename_all = "snake_case")
)]
#[serde(rename_all = "snake_case")]
pub enum MergeStrategy {
    /// Squashed onto the base branch with git alone, in the primary checkout.
    #[default]
    Direct,
    /// Published as a pull or merge request for a human to merge.
    PullRequest,
}

wire_enum! { MergeStrategy, "merge strategy", [
    Direct = "direct", PullRequest = "pull_request",
]}

/// A prompt a profile owns beside its system prompt: one of the texts an
/// agent of that role is started, resumed or nudged with. Every briefing a
/// profile carries is one of these, and each kind belongs to the role that
/// receives it (see [`PromptKind::roles`]).
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
    /// What a planner that has stopped planning is nudged with.
    PlannerResume,
    /// Initial briefing of an engineer session.
    EngineerBriefing,
    /// What an engineer with unfinished work is picked up with, whether its
    /// session ended or is merely sitting idle.
    EngineerResume,
    /// Engineer resume briefing carrying a round of requested changes, from
    /// the reviewers or from the people on a published request.
    ChangesRequested,
    /// Initial briefing of a reviewer session.
    ReviewerBriefing,
    /// What a reviewer that owes a verdict is picked up with.
    ReviewerResume,
    /// Landing briefing of a repository that squashes onto its base branch.
    LandingDirect,
    /// Landing briefing of a repository that takes a pull or merge request.
    LandingPullRequest,
}

wire_enum! { PromptKind, "prompt kind", [
    PlannerBriefing = "planner_briefing",
    PlannerResume = "planner_resume",
    EngineerBriefing = "engineer_briefing",
    EngineerResume = "engineer_resume",
    ChangesRequested = "changes_requested",
    LandingDirect = "landing_direct",
    LandingPullRequest = "landing_pull_request",
    ReviewerBriefing = "reviewer_briefing",
    ReviewerResume = "reviewer_resume",
]}

impl PromptKind {
    /// The landing briefing a repository on `strategy` hands its engineer.
    ///
    /// One kind per strategy rather than one text with two halves: the daemon
    /// knows which it is when it renders, so the engineer is handed the
    /// procedure it runs and nothing of the other.
    pub fn landing_for(strategy: MergeStrategy) -> PromptKind {
        match strategy {
            MergeStrategy::Direct => PromptKind::LandingDirect,
            MergeStrategy::PullRequest => PromptKind::LandingPullRequest,
        }
    }

    /// The roles whose profiles own this prompt.
    pub fn roles(&self) -> &'static [Role] {
        match self {
            PromptKind::PlannerBriefing | PromptKind::PlannerResume => &[Role::Planner],
            PromptKind::EngineerBriefing
            | PromptKind::EngineerResume
            | PromptKind::ChangesRequested
            | PromptKind::LandingDirect
            | PromptKind::LandingPullRequest => &[Role::Engineer],
            PromptKind::ReviewerBriefing | PromptKind::ReviewerResume => &[Role::Reviewer],
        }
    }

    /// Whether a profile of `role` owns this prompt.
    pub fn owned_by(&self, role: Role) -> bool {
        self.roles().contains(&role)
    }

    /// The prompts a profile of `role` owns, in briefing order.
    pub fn for_role(role: Role) -> &'static [PromptKind] {
        match role {
            Role::Planner => &[PromptKind::PlannerBriefing, PromptKind::PlannerResume],
            Role::Engineer => &[
                PromptKind::EngineerBriefing,
                PromptKind::EngineerResume,
                PromptKind::ChangesRequested,
                PromptKind::LandingDirect,
                PromptKind::LandingPullRequest,
            ],
            Role::Reviewer => &[PromptKind::ReviewerBriefing, PromptKind::ReviewerResume],
        }
    }

    /// The placeholders the daemon fills in when it renders this kind's
    /// template.
    ///
    /// The contract between the templates and the daemon's `prompts`
    /// builders: a `{token}` outside this list is one nothing will ever
    /// substitute, which is what [`PromptKind::validate_template`] refuses.
    /// Adding a value to a builder means adding its name here.
    pub fn placeholders(&self) -> &'static [&'static str] {
        match self {
            PromptKind::PlannerBriefing => &[
                "goal_title",
                "goal_description",
                "repositories",
                "max_tasks",
                "required_approvals",
            ],
            // A nudge says what is waiting and nothing else: the planner it
            // reaches has read the goal already.
            PromptKind::PlannerResume => &["goal_title"],
            PromptKind::EngineerBriefing => &[
                "task_title",
                "task_description",
                "goal_title",
                "worktree_path",
                "branch",
                "base_branch",
                "repo_path",
                // How that repository takes the change the task ends in,
                // said once at the start as well as in the landing briefing:
                // a branch that will be published is written differently
                // from one that is squashed away.
                "merge_strategy",
                "dependencies",
            ],
            PromptKind::EngineerResume => &["task_title", "branch"],
            PromptKind::ChangesRequested => &["feedback"],
            // Which procedure applies is the kind, not a value inside it, so
            // a landing briefing names only what its commands act on.
            PromptKind::LandingDirect | PromptKind::LandingPullRequest => {
                &["task_title", "branch", "base_branch", "repo_path"]
            }
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
        }
    }

    /// Refuse a template that names a placeholder this kind has no value for.
    ///
    /// Rendering is lenient by design — an unknown `{token}` reaches the agent
    /// as literal text rather than failing its spawn — so a typo like
    /// `{task_titel}` is invisible until someone reads a briefing. Saving is
    /// where it is caught instead, and only there.
    ///
    /// Only what rendering would treat as a placeholder is checked, and of
    /// that only plain identifiers: an unclosed brace, a `{}` and a JSON
    /// snippet are text to rendering, so they are text here too.
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
            // An unclosed brace, or one closed only after another `{`: text.
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

/// Which coding-agent CLI a profile runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum AgentKind {
    ClaudeCode,
    Codex,
    Opencode,
}

wire_enum! { AgentKind, "agent kind", [
    ClaudeCode = "claude_code",
    Codex = "codex",
    Opencode = "opencode",
]}

impl AgentKind {
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
    /// What a fresh database seeds the flag list with and what restoring the
    /// defaults puts back; from there the list is the user's. Only flags a
    /// user may reasonably drop belong here — the structural ones (session
    /// ids, MCP and hook config, the system prompt, the model) are the
    /// adapters' own.
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
    /// Planner session active; tasks being defined, and nothing running yet.
    Planning,
    /// Plan finalized by the planner, once the user validated it in the goal
    /// thread; tasks executing.
    Active,
    /// All tasks merged (or goal-level completion recorded).
    Completed,
    Cancelled,
}

wire_enum! { GoalStatus, "goal status", [
    Planning = "planning",
    Active = "active",
    Completed = "completed",
    Cancelled = "cancelled",
]}

impl GoalStatus {
    /// Nothing more will happen to this goal: what may be deleted, and what
    /// cancelling refuses.
    pub fn is_terminal(&self) -> bool {
        matches!(self, GoalStatus::Completed | GoalStatus::Cancelled)
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

wire_enum! { SessionStatus, "session status", [
    Starting = "starting",
    Running = "running",
    Idle = "idle",
    Exited = "exited",
    Failed = "failed",
]}

impl SessionStatus {
    pub fn is_live(&self) -> bool {
        matches!(
            self,
            SessionStatus::Starting | SessionStatus::Running | SessionStatus::Idle
        )
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
    /// Something addressed to the user that no agent can do for them: a
    /// message written to them, a published request that is theirs to merge.
    /// Raised on the session the work is with, but not by that agent and not
    /// the agent's to take down, which is what tells it apart from the two
    /// above.
    WaitingUser,
    /// The agent reported an error (API error, crash, `session.error`).
    AgentError,
    /// Tmux session or agent process gone while its work is still active.
    Disconnected,
    /// No activity for too long.
    Stalled,
}

wire_enum! { AttentionReason, "attention reason", [
    WaitingPermission = "waiting_permission",
    WaitingInput = "waiting_input",
    WaitingUser = "waiting_user",
    AgentError = "agent_error",
    Disconnected = "disconnected",
    Stalled = "stalled",
]}

impl AttentionReason {
    /// Whether this reason describes a dialog on the agent's own terminal.
    ///
    /// Only a live session can be sitting on one: a prompt is something
    /// somebody types an answer into, and a pane that is gone has none. The
    /// other reasons are the ones a session ends *carrying*, and they stay
    /// true after the agent has stopped.
    pub fn is_prompt(&self) -> bool {
        matches!(
            self,
            AttentionReason::WaitingPermission | AttentionReason::WaitingInput
        )
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

wire_enum! { ReviewVerdict, "review verdict", [
    Approve = "approve", RequestChanges = "request_changes",
]}

/// Author of a conversation message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum AuthorRole {
    Planner,
    Engineer,
    Reviewer,
    User,
    System,
}

wire_enum! { AuthorRole, "author role", [
    Planner = "planner",
    Engineer = "engineer",
    Reviewer = "reviewer",
    User = "user",
    System = "system",
]}

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

wire_enum! { RecipientKind, "recipient kind", [
    Profile = "profile", User = "user",
]}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every kind is owned by at least one role, is listed among that role's
    /// prompts, and is reachable from `ALL` by the name it is stored under:
    /// the three lists are one set read three ways, and a kind missing from
    /// any of them is a prompt nobody can edit.
    #[test]
    fn every_kind_is_owned_listed_and_named() {
        use std::str::FromStr;

        for kind in PromptKind::ALL {
            let roles = kind.roles();
            assert!(!roles.is_empty(), "{} belongs to no role", kind.as_str());
            for role in roles {
                assert!(kind.owned_by(*role));
                assert!(
                    PromptKind::for_role(*role).contains(&kind),
                    "{} is not among the {} prompts",
                    kind.as_str(),
                    role.as_str()
                );
            }
            for role in Role::ALL.into_iter().filter(|r| !roles.contains(r)) {
                assert!(!kind.owned_by(role));
                assert!(!PromptKind::for_role(role).contains(&kind));
            }
            assert_eq!(PromptKind::from_str(kind.as_str()), Ok(kind));
        }
        for role in Role::ALL {
            for kind in PromptKind::for_role(role) {
                assert!(
                    PromptKind::ALL.contains(kind),
                    "{} is not in ALL",
                    kind.as_str()
                );
            }
        }
    }

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
            PromptKind::LandingDirect.validate_template("Land it yourself."),
            Ok(())
        );
    }
}
