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

/// Where an agent sits: the orchestrator of a goal, or the author or a
/// reviewer of one task.
///
/// A seat is a position, not an identity. Every agent below the orchestrator
/// is generic, and what it can do comes from the skills it loads; the seat is
/// only what the state machine and the launcher need to know about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(
    feature = "clap",
    derive(clap::ValueEnum),
    value(rename_all = "kebab-case")
)]
#[serde(rename_all = "snake_case")]
pub enum Seat {
    Orchestrator,
    Author,
    Reviewer,
}

wire_enum! { Seat, "seat", [
    Orchestrator = "orchestrator",
    Author = "author",
    Reviewer = "reviewer",
]}

/// How one task ends.
///
/// The one thing about the end of a task the author has to be told, since the
/// commands it runs differ entirely between the three. The orchestrator agrees
/// it with the user task by task: some work lands on the base branch, some
/// goes through a request the author then sees to its merge, and some has
/// nothing to land at all — a report filed, a document published, a release
/// cut. All three reach [`TaskStatus::Finished`]; landing is one way of
/// getting there rather than the meaning of being there.
///
/// Which forge a published request goes to is *not* here: `origin` says
/// whether it is GitHub or GitLab, and asking the remote at landing time
/// cannot go stale the way a second copy of the answer would.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(
    feature = "clap",
    derive(clap::ValueEnum),
    value(rename_all = "kebab-case")
)]
#[serde(rename_all = "snake_case")]
pub enum Landing {
    /// The author puts the change on the base branch itself.
    Merge,
    /// The author publishes a request and sees it through: it answers what is
    /// written on it, and the task ends when the request is merged.
    PullRequest,
    /// Nothing is landed: what the task produced is the whole of it — a
    /// published tag, a filed report, a document that lives elsewhere.
    None,
}

wire_enum! { Landing, "landing", [
    Merge = "merge", PullRequest = "pull_request", None = "none",
]}

impl Landing {
    /// The placeholders a landing briefing may name, whichever ending it is
    /// written for: the branch, the base and the checkout its commands act
    /// on, and the task they are landing.
    ///
    /// The contract between a repository's landing text and the daemon's
    /// `landing_briefing` builder, read the same way
    /// [`PromptKind::placeholders`] is read: a `{token}` outside this list is
    /// one nothing will ever substitute.
    pub const LANDING_PLACEHOLDERS: &'static [&'static str] =
        &["task_title", "branch", "base_branch", "repo_path"];

    /// Refuse a landing template that names a placeholder nothing fills in.
    ///
    /// The same check, and the same leniency about what is text, as
    /// [`PromptKind::validate_template`]: saving is the last moment anyone
    /// looks at a `{task_titel}`, since rendering carries it through to the
    /// agent as it stands.
    pub fn validate_landing_template(template: &str) -> Result<(), UnknownPlaceholders> {
        unknown_placeholders("landing", Self::LANDING_PLACEHOLDERS, template)
    }

    /// Whether this ending puts anything in the repository at all.
    ///
    /// The one that does not is why a repository's own landing text is not
    /// handed to every task: a repository's way of taking a change has
    /// nothing to say about a task that hands it none.
    pub fn lands_a_change(&self) -> bool {
        !matches!(self, Landing::None)
    }
}

/// A lifecycle briefing of Ariadne's own: one of the texts an agent is
/// started, resumed or nudged with. Each kind belongs to the seat that
/// receives it (see [`PromptKind::seats`]), and its text is a constant of the
/// code — no profile carries one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(
    feature = "clap",
    derive(clap::ValueEnum),
    value(rename_all = "kebab-case")
)]
#[serde(rename_all = "snake_case")]
pub enum PromptKind {
    /// Initial briefing of an orchestrator session.
    OrchestratorBriefing,
    /// What an orchestrator that has gone quiet is nudged with.
    OrchestratorResume,
    /// What the orchestrator of a goal under way is woken with when its
    /// tasks need it: one that failed, one that has gone quiet, or a goal
    /// with nothing left to do.
    GoalAttention,
    /// What one agent said to another, as the recipient reads it.
    IncomingMessage,
    /// Initial briefing of an author session.
    AuthorBriefing,
    /// What an author with unfinished work is picked up with, whether its
    /// session ended or is merely sitting idle.
    AuthorResume,
    /// Author resume briefing carrying a round of requested changes, from
    /// the reviewers or from the people on a published request.
    ChangesRequested,
    /// Initial briefing of a reviewer session.
    ReviewerBriefing,
    /// What a reviewer that owes a verdict is picked up with.
    ReviewerResume,
}

wire_enum! { PromptKind, "prompt kind", [
    OrchestratorBriefing = "orchestrator_briefing",
    OrchestratorResume = "orchestrator_resume",
    GoalAttention = "goal_attention",
    IncomingMessage = "incoming_message",
    AuthorBriefing = "author_briefing",
    AuthorResume = "author_resume",
    ChangesRequested = "changes_requested",
    ReviewerBriefing = "reviewer_briefing",
    ReviewerResume = "reviewer_resume",
]}

impl PromptKind {
    /// The seats briefed with this prompt.
    pub fn seats(&self) -> &'static [Seat] {
        match self {
            PromptKind::OrchestratorBriefing
            | PromptKind::OrchestratorResume
            | PromptKind::GoalAttention => &[Seat::Orchestrator],
            // Every seat can be written to, so every seat is briefed with it.
            PromptKind::IncomingMessage => &[Seat::Orchestrator, Seat::Author, Seat::Reviewer],
            PromptKind::AuthorBriefing
            | PromptKind::AuthorResume
            | PromptKind::ChangesRequested => &[Seat::Author],
            PromptKind::ReviewerBriefing | PromptKind::ReviewerResume => &[Seat::Reviewer],
        }
    }

    /// The prompts a session of `seat` is briefed with, in briefing order.
    pub fn for_seat(seat: Seat) -> &'static [PromptKind] {
        match seat {
            Seat::Orchestrator => &[
                PromptKind::OrchestratorBriefing,
                PromptKind::OrchestratorResume,
                PromptKind::GoalAttention,
                PromptKind::IncomingMessage,
            ],
            Seat::Author => &[
                PromptKind::AuthorBriefing,
                PromptKind::AuthorResume,
                PromptKind::ChangesRequested,
                PromptKind::IncomingMessage,
            ],
            Seat::Reviewer => &[
                PromptKind::ReviewerBriefing,
                PromptKind::ReviewerResume,
                PromptKind::IncomingMessage,
            ],
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
            PromptKind::OrchestratorBriefing => &[
                "goal_title",
                "goal_description",
                "repositories",
                "max_tasks",
            ],
            // A nudge says what is waiting and nothing else: the orchestrator
            // it reaches has read the goal already.
            PromptKind::OrchestratorResume => &["goal_title"],
            // What the tasks of this goal need: one line each, rendered by
            // the scheduler that noticed.
            PromptKind::GoalAttention => &["goal_title", "tasks"],
            // What was said, who said it, and the id an answer names.
            PromptKind::IncomingMessage => &["from", "message_id", "body"],
            PromptKind::AuthorBriefing => &[
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
                "landing",
                "dependencies",
            ],
            PromptKind::AuthorResume => &["task_title", "branch"],
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
        }
    }

    /// Refuse a template that names a placeholder this kind has no value for.
    ///
    /// Rendering is lenient by design — an unknown `{token}` reaches the agent
    /// as literal text rather than failing its spawn — so a typo like
    /// `{task_titel}` is invisible until someone reads a briefing. This is
    /// what holds the built-in templates to the values their assembler fills
    /// in.
    ///
    /// Only what rendering would treat as a placeholder is checked, and of
    /// that only plain identifiers: an unclosed brace, a `{}` and a JSON
    /// snippet are text to rendering, so they are text here too.
    pub fn validate_template(&self, template: &str) -> Result<(), UnknownPlaceholders> {
        unknown_placeholders(self.as_str(), self.placeholders(), template)
    }
}

/// The check both `validate` calls make: the names `template` would have
/// looked up, minus what rendering treats as text and minus `allowed`.
/// `whose` names the template in the message.
fn unknown_placeholders(
    whose: &'static str,
    allowed: &'static [&'static str],
    template: &str,
) -> Result<(), UnknownPlaceholders> {
    let mut unknown: Vec<String> = Vec::new();
    for name in placeholder_names(template) {
        if !is_identifier(name)
            || allowed.contains(&name)
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
        whose,
        allowed,
        unknown,
    })
}

/// A template saved with `{token}`s the daemon has no value for, and what
/// would have had to fill them in: a prompt kind, or a landing briefing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownPlaceholders {
    /// How the template is named in the message: a prompt kind's spelling, or
    /// `landing` for a repository's landing briefing.
    pub whose: &'static str,
    /// The names the template was allowed to use.
    pub allowed: &'static [&'static str],
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
        let allowed = braced(&mut self.allowed.iter().copied());
        let plural = if self.unknown.len() == 1 {
            "placeholder"
        } else {
            "placeholders"
        };
        write!(
            f,
            "the {} template has no value for {plural} {unknown}; \
             the ones it can use are {allowed}",
            self.whose
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

/// Roughly what a model is, as a picker and an orchestrator compare models: the
/// capability class it belongs to, across every agent CLI at once.
///
/// One ladder for the whole catalog, so a claude_code entry and a codex entry
/// that sit at the same rung really are alternatives for the same work.
/// `Unknown` is what an entry nothing has been written about says — a model
/// discovered at runtime, or an agent CLI on whatever model it defaults to —
/// and it is a genuine answer rather than a missing one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum ModelTier {
    /// The deepest reasoning there is, at the price of it.
    Frontier,
    /// Heavy reasoning, a rung below the current frontier.
    Strong,
    /// The everyday working model: capable, and affordable enough to run all
    /// day.
    Balanced,
    /// Built for latency and volume rather than for depth.
    Fast,
    /// Nothing here knows.
    Unknown,
}

wire_enum! { ModelTier, "model tier", [
    Frontier = "frontier",
    Strong = "strong",
    Balanced = "balanced",
    Fast = "fast",
    Unknown = "unknown",
]}

/// Goal lifecycle status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(
    feature = "clap",
    derive(clap::ValueEnum),
    value(rename_all = "kebab-case")
)]
#[serde(rename_all = "snake_case")]
pub enum GoalStatus {
    /// Orchestrator session active; tasks being defined, and nothing running
    /// yet.
    Planning,
    /// Plan finalized by the orchestrator, once the user validated it in the
    /// goal thread; tasks executing.
    Active,
    /// All tasks finished (or goal-level completion recorded).
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
    value(rename_all = "kebab-case")
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

    /// Whether this reason is one only the user can answer.
    ///
    /// The other half of whose flag a reason is. Every one but this is the
    /// agent's own — its detectors raise it, its next event takes it down,
    /// and a relaunch is the recovery for it. This one is raised on the
    /// session the work is with for something no agent can do: a message
    /// written to the user, a published request that is theirs to merge.
    /// Nothing an agent reports settles it, which is why nothing an agent
    /// raises replaces it either.
    pub fn is_for_the_user(&self) -> bool {
        matches!(self, AttentionReason::WaitingUser)
    }
}

/// What one agent is saying to another.
///
/// Agents talk to each other through one channel, and this is what tells the
/// six things they say apart. A verdict used to be a row of its own; it is a
/// message like the rest now, which is what makes "the reviewer asked the
/// author something" possible at all — before, the only thing a reviewer
/// could say was approve or request changes.
///
/// The kind is what the daemon reads. Two of them move the task
/// ([`TaskStatus`]), one of them is answered, and the rest are said and left.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(
    feature = "clap",
    derive(clap::ValueEnum),
    value(rename_all = "kebab-case")
)]
#[serde(rename_all = "snake_case")]
pub enum MessageKind {
    /// Something the sender needs answered before it can go on.
    Question,
    /// The answer to one.
    Answer,
    /// The author asking a reviewer to look at what it wrote.
    ReviewRequest,
    /// A reviewer's verdict: the round is closed for that reviewer.
    Approve,
    /// A reviewer's verdict: the author starts again on this feedback.
    RequestChanges,
    /// Anything worth saying that nobody has to answer.
    Note,
}

wire_enum! { MessageKind, "message kind", [
    Question = "question",
    Answer = "answer",
    ReviewRequest = "review_request",
    Approve = "approve",
    RequestChanges = "request_changes",
    Note = "note",
]}

impl MessageKind {
    /// Whether this kind is a reviewer's verdict on a round.
    ///
    /// The two that are is what the daemon counts when it decides whether a
    /// round is closed, and one verdict per reviewer per round is what the
    /// store holds them to.
    pub fn is_verdict(&self) -> bool {
        matches!(self, MessageKind::Approve | MessageKind::RequestChanges)
    }

    /// Whether the sender is waiting for an answer to this.
    pub fn wants_an_answer(&self) -> bool {
        matches!(self, MessageKind::Question)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every kind briefs at least one seat, is listed among that seat's
    /// prompts, and is reachable from `ALL` by the name it is stored under:
    /// the three lists are one set read three ways, and a kind missing from
    /// any of them is a briefing nothing can render.
    #[test]
    fn every_kind_is_owned_listed_and_named() {
        use std::str::FromStr;

        for kind in PromptKind::ALL {
            let seats = kind.seats();
            assert!(!seats.is_empty(), "{} belongs to no seat", kind.as_str());
            for seat in seats {
                assert!(
                    PromptKind::for_seat(*seat).contains(&kind),
                    "{} is not among the {} prompts",
                    kind.as_str(),
                    seat.as_str()
                );
            }
            for seat in Seat::ALL.into_iter().filter(|r| !seats.contains(r)) {
                assert!(!PromptKind::for_seat(seat).contains(&kind));
            }
            assert_eq!(PromptKind::from_str(kind.as_str()), Ok(kind));
        }
        for seat in Seat::ALL {
            for kind in PromptKind::for_seat(seat) {
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
        let err = PromptKind::AuthorBriefing
            .validate_template("# {task_titel}\n\n{task_description}")
            .unwrap_err();
        assert_eq!(err.unknown, ["task_titel"]);
        let message = err.to_string();
        assert!(message.contains("author_briefing"), "{message}");
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
            PromptKind::OrchestratorBriefing
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
                PromptKind::AuthorBriefing.validate_template(template),
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
            PromptKind::AuthorResume.validate_template("Carry on."),
            Ok(())
        );
        assert_eq!(
            Landing::validate_landing_template("Land it yourself."),
            Ok(())
        );
    }

    /// A landing briefing is a repository's, not a kind's, and it is checked
    /// the same way: what the daemon fills in passes, a typo is named with
    /// the whole allowed set, and what renders as text is text here too.
    #[test]
    fn a_landing_template_names_only_what_the_landing_briefing_fills_in() {
        let all = Landing::LANDING_PLACEHOLDERS
            .iter()
            .map(|name| format!("{{{name}}}"))
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(Landing::validate_landing_template(&all), Ok(()));

        let err =
            Landing::validate_landing_template("Squash {branch} onto {base_brunch}.").unwrap_err();
        assert_eq!(err.unknown, ["base_brunch"]);
        let message = err.to_string();
        assert!(message.contains("landing"), "{message}");
        assert!(message.contains("{base_brunch}"), "{message}");
        assert!(message.contains("{base_branch}"), "{message}");
        assert!(message.contains("{repo_path}"), "{message}");

        // A placeholder of a profile's briefing is not one a landing text can
        // use: the sets are per template, not one pool.
        assert!(Landing::validate_landing_template("{task_description}").is_err());
        assert_eq!(
            Landing::validate_landing_template("Answer with {\"ok\": true} and {}."),
            Ok(())
        );
    }
}
