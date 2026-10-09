//! Ariadne domain types: what the daemon, the store, the API DTOs and the
//! CLI all have to agree on.
//!
//! Pure domain apart from [`probe`], which asks the host about the binaries
//! Ariadne runs — shared for the same reason as the rest, that two callers
//! answering the same question differently is the bug.

pub mod acp;
pub mod id;
pub mod models;
pub mod probe;
pub mod state_machine;
pub mod workflow;

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

/// Where an agent sits: the orchestrator of a goal, the agent of one workflow
/// column of a task, or the reviewer of a pull request (029).
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
    /// The one agent of a goal that plans it with the user.
    Orchestrator,
    /// The agent of one column of a task's workflow.
    Agent,
    /// The session that reviews a pull request the user was asked to review
    /// (029). It sits on no task.
    Reviewer,
}

wire_enum! { Seat, "seat", [
    Orchestrator = "orchestrator",
    Agent = "agent",
    Reviewer = "reviewer",
]}

/// How the ACP runtime answers a tool permission request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(
    feature = "clap",
    derive(clap::ValueEnum),
    value(rename_all = "kebab-case")
)]
#[serde(rename_all = "snake_case")]
pub enum PermissionMode {
    /// Select the allowing option without asking a person.
    Auto,
    /// Send every request to the session console and wait for an answer.
    Ask,
    /// Ask once for each repository, tool name and input; remember approvals.
    Learn,
    /// Let the AI permission model answer each request (022).
    Ai,
}

wire_enum! { PermissionMode, "permission mode", [
    Auto = "auto", Ask = "ask", Learn = "learn", Ai = "ai",
]}

/// The forge a repository's remote is on (025): which CLI speaks to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum ForgeKind {
    /// GitHub or a GitHub Enterprise host, through `gh`.
    Github,
    /// GitLab or a self-managed GitLab host, through `glab`.
    Gitlab,
}

wire_enum! { ForgeKind, "forge kind", [
    Github = "github", Gitlab = "gitlab",
]}

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
    /// First briefing of a column's agent: the task, and the column's
    /// instructions.
    StepBriefing,
    /// What a column's agent is briefed with when the task comes back to its
    /// column: forward, back, or a retry, with the reason.
    StepReturn,
    /// What the agent of the current column is nudged with when it has gone
    /// quiet.
    AgentResume,
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
}

wire_enum! { PromptKind, "prompt kind", [
    StepBriefing = "step_briefing",
    StepReturn = "step_return",
    AgentResume = "agent_resume",
    OrchestratorBriefing = "orchestrator_briefing",
    OrchestratorResume = "orchestrator_resume",
    GoalAttention = "goal_attention",
    IncomingMessage = "incoming_message",
]}

impl PromptKind {
    /// The seats briefed with this prompt.
    pub fn seats(&self) -> &'static [Seat] {
        match self {
            PromptKind::OrchestratorBriefing
            | PromptKind::OrchestratorResume
            | PromptKind::GoalAttention => &[Seat::Orchestrator],
            // Every seat on a goal can be written to, so every seat on a goal
            // is briefed with it. A pull request reviewer sits on none.
            PromptKind::IncomingMessage => &[Seat::Orchestrator, Seat::Agent],
            PromptKind::StepBriefing | PromptKind::StepReturn | PromptKind::AgentResume => {
                &[Seat::Agent]
            }
        }
    }

    /// The prompts a session of `seat` is briefed with, in briefing order.
    /// A pull request reviewer (029) is briefed with texts of its own, which
    /// are no lifecycle kind.
    pub fn for_seat(seat: Seat) -> &'static [PromptKind] {
        match seat {
            Seat::Agent => &[
                PromptKind::StepBriefing,
                PromptKind::StepReturn,
                PromptKind::AgentResume,
                PromptKind::IncomingMessage,
            ],
            Seat::Orchestrator => &[
                PromptKind::OrchestratorBriefing,
                PromptKind::OrchestratorResume,
                PromptKind::GoalAttention,
                PromptKind::IncomingMessage,
            ],
            Seat::Reviewer => &[],
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
            // The workflow the goal runs on, and its columns one line each:
            // what the orchestrator staffs every task against.
            PromptKind::OrchestratorBriefing => &[
                "goal_title",
                "goal_description",
                "workflow",
                "columns",
                "repositories",
            ],
            // A nudge says what is waiting and nothing else: the orchestrator
            // it reaches has read the goal already.
            PromptKind::OrchestratorResume => &["goal_title"],
            // What the tasks of this goal need: one line each, rendered by
            // the scheduler that noticed.
            PromptKind::GoalAttention => &["goal_title", "tasks"],
            // What was said, who said it, the task it is of, and whether to
            // say how to answer.
            PromptKind::IncomingMessage => &["from", "body", "answer_hint"],
            PromptKind::StepBriefing => &[
                "task_title",
                "task_description",
                "goal_title",
                "worktree_path",
                "branch",
                "base_branch",
                "repo_path",
                "step_id",
                "step_title",
                "step_description",
                "previous_summary",
                "dependencies",
            ],
            PromptKind::StepReturn => &["task_title", "step_title", "direction", "reason"],
            PromptKind::AgentResume => &["task_title", "step_title"],
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

/// The check `validate_template` makes: the names `template` would have
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

/// A template saved with `{token}`s the daemon has no value for, and the
/// prompt kind that would have had to fill them in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownPlaceholders {
    /// How the template is named in the message: the prompt kind's spelling.
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
    /// Agent process started, handshake under way.
    Starting,
    /// Agent actively working.
    Running,
    /// Agent finished its turn and is waiting (Stop / turn-complete / idle).
    Idle,
    /// Agent process exited.
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
    /// Agent process gone while its work is still active.
    Disconnected,
    /// No activity for too long.
    Stalled,
    /// The model cannot accept more work.
    Exhausted,
}

wire_enum! { AttentionReason, "attention reason", [
    WaitingPermission = "waiting_permission",
    WaitingInput = "waiting_input",
    WaitingUser = "waiting_user",
    AgentError = "agent_error",
    Disconnected = "disconnected",
    Stalled = "stalled",
    Exhausted = "exhausted",
]}

impl AttentionReason {
    /// Whether this reason describes a question the live agent is waiting on.
    ///
    /// Only a live session can be sitting on one: a prompt is answered in the
    /// session's console, and an agent that is gone has none. The
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
/// Agents talk to each other through one channel, and one kind carries
/// everything they say on it, whether it asks something or answers it. There
/// is no `answer` kind and no `reply` tool: an answer is a message to whoever
/// asked, addressed the way the question was, so nothing threads. Each
/// message reaches its agent as a turn — which is why the tool that sends one
/// takes questions and answers and nothing else, no confirmation and no
/// thanks.
///
/// A review used to run on the channel too, as three kinds of its own; a
/// workflow column moves a task through the step routes now, and the one
/// kind left is what the agents say.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(
    feature = "clap",
    derive(clap::ValueEnum),
    value(rename_all = "kebab-case")
)]
#[serde(rename_all = "snake_case")]
pub enum MessageKind {
    /// A message: one agent asking another something, or answering what it
    /// was asked.
    Message,
}

wire_enum! { MessageKind, "message kind", [
    Message = "message",
]}

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
        let err = PromptKind::StepBriefing
            .validate_template("# {task_titel}\n\n{task_description}")
            .unwrap_err();
        assert_eq!(err.unknown, ["task_titel"]);
        let message = err.to_string();
        assert!(message.contains("step_briefing"), "{message}");
        assert!(message.contains("{task_titel}"), "{message}");
        assert!(message.contains("{task_title}"), "{message}");
        assert!(message.contains("{dependencies}"), "{message}");
    }

    /// Every offending token is named, once each: a template is fixed in one
    /// pass, not one save per typo.
    #[test]
    fn every_unknown_token_is_named_once() {
        let err = PromptKind::StepReturn
            .validate_template("{reason} {who} {what} {who}")
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
        // A nudged agent is briefed with less than a fresh one.
        assert!(
            PromptKind::AgentResume
                .validate_template("{task_title} in {repo_path} waits at {step_title}.")
                .is_err()
        );
        assert_eq!(
            PromptKind::StepBriefing
                .validate_template("{task_title} in {repo_path} waits at {step_title}."),
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
                PromptKind::StepBriefing.validate_template(template),
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
            PromptKind::AgentResume.validate_template("Carry on."),
            Ok(())
        );
    }

    /// The three seats are the orchestrator of a goal, the agent of a
    /// column and the reviewer of a pull request, in that order, and each
    /// one answers to the word the wire spells it with.
    #[test]
    fn the_seats_are_the_orchestrator_the_agent_and_the_pull_request_reviewer() {
        use std::str::FromStr;

        assert_eq!(Seat::ALL, [Seat::Orchestrator, Seat::Agent, Seat::Reviewer]);
        for (word, seat) in [
            ("orchestrator", Seat::Orchestrator),
            ("agent", Seat::Agent),
            ("reviewer", Seat::Reviewer),
        ] {
            assert_eq!(Seat::from_str(word), Ok(seat));
            assert_eq!(seat.as_str(), word);
        }
        assert!(Seat::from_str("author").is_err());
        assert_eq!(MessageKind::ALL, [MessageKind::Message]);
        assert!(MessageKind::from_str("approve").is_err());
    }
}
