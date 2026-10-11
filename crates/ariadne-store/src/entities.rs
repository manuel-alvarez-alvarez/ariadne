//! Row types. Enum-typed columns are stored as TEXT and surfaced as `String`;
//! [`enum_columns`] below is where each of them is read back as its
//! `ariadne-core` enum.

use std::str::FromStr;

use ariadne_core::{
    Actor, AttentionReason, ForgeKind, GoalStatus, MessageKind, PermissionMode, Seat,
    SessionStatus, TaskStatus,
};

use crate::defaults::{
    ORCHESTRATION_SKILL, PR_BABYSIT_SKILL, PR_REVIEWER_SKILL, default_skill_document,
    default_workflow_document, skill_summary,
};

/// The typed reading of a TEXT column that holds a core enum. The accessor
/// and the column share a name; brackets mark a nullable column, which reads
/// back as `Option`.
///
/// A spelling the enum does not know is a schema violation rather than an
/// input error — nothing outside this crate writes these columns — so it
/// panics instead of widening every caller's error type.
macro_rules! enum_columns {
    ($($entity:ident { $($name:ident: $ty:tt),+ $(,)? })+) => {
        $(impl $entity {
            $(enum_columns!(@one $name: $ty);)+
        })+
    };
    (@one $name:ident: [$ty:ty]) => {
        pub fn $name(&self) -> Option<$ty> {
            self.$name.as_deref().map(|v| {
                <$ty>::from_str(v)
                    .unwrap_or_else(|_| panic!(concat!("invalid ", stringify!($name), " in db")))
            })
        }
    };
    (@one $name:ident: $ty:ty) => {
        pub fn $name(&self) -> $ty {
            <$ty>::from_str(&self.$name)
                .unwrap_or_else(|_| panic!(concat!("invalid ", stringify!($name), " in db")))
        }
    };
}

enum_columns! {
    Goal { status: GoalStatus }
    Task { status: TaskStatus }
    ForgeIntegration { kind: ForgeKind }
    AgentSession {
        seat: [Seat],
        status: SessionStatus,
        attention_reason: [AttentionReason],
    }
}

/// A skill: one document that tells a generic agent how to do one kind of
/// work. Its name is its identity — an agent loads it by name — and the
/// document is the whole `SKILL.md`, frontmatter included.
///
/// A `NULL` document is a built-in still on the text Ariadne ships, which is
/// why rewording a shipped skill reaches every database without a migration.
/// A skill the user wrote has no default behind it and carries its own text.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Skill {
    pub name: String,
    /// The document set on this skill, or NULL while a built-in runs on the
    /// text Ariadne ships. Read through [`Skill::document_text`].
    pub document: Option<String>,
    pub builtin: i64,
    pub created_at: String,
    pub updated_at: String,
}

impl Skill {
    /// Whether Ariadne ships this skill, and so whether it has a default to
    /// be reset to and refuses deletion.
    pub fn is_builtin(&self) -> bool {
        self.builtin != 0
    }

    /// The document an agent loading this skill reads: the one set on it, or
    /// the text Ariadne ships under its name.
    ///
    /// The empty string is unreachable through the store — the schema refuses
    /// a non-built-in with no document, and a built-in always has one to fall
    /// back on — and is here so that a row written by a future build that this
    /// one no longer ships reads as an empty skill rather than a panic.
    pub fn document_text(&self) -> &str {
        self.document
            .as_deref()
            .or_else(|| default_skill_document(&self.name))
            .unwrap_or("")
    }

    /// Whether [`Skill::document_text`] is the shipped text rather than one
    /// somebody wrote.
    pub fn document_is_default(&self) -> bool {
        self.document.is_none()
    }

    /// The one line the index in an agent's system prompt carries: the
    /// `description` of the document's frontmatter, or the name where the
    /// document names none.
    pub fn summary(&self) -> &str {
        skill_summary(self.document_text()).unwrap_or(&self.name)
    }

    /// The seat this skill serves, read off the name and stored nowhere:
    /// `orchestration` is the orchestrator's own playbook, `pr-reviewer` is
    /// loaded by the daemon itself for a review session, `pr-babysit` is the
    /// one pull request skill a workflow column stages, and every other
    /// skill is a task agent's to be staffed on.
    pub fn seat(&self) -> SkillSeat {
        SkillSeat::of(&self.name)
    }
}

/// Where a skill's work sits: the orchestrator's seat, a task agent's, or a
/// pull request's.
///
/// A skill's seat is a fact of its name, not of any row — which is what lets
/// the shipped playbook stay a skill like the rest, resettable and editable,
/// without a column saying whose it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillSeat {
    /// Loaded by every orchestrator session, staffable on nothing.
    Orchestrator,
    /// Staffed on the agent of a workflow column.
    Task,
    /// A skill of a pull request: `pr-babysit` is staged by the `pr` column
    /// of `develop-review-pr` (030), and `pr-reviewer` is loaded by the
    /// daemon onto the session that reviews a request (029) and staffs no
    /// task agent.
    PullRequest,
}

impl SkillSeat {
    /// The seat of the skill called `name`.
    pub fn of(name: &str) -> Self {
        match name {
            ORCHESTRATION_SKILL => Self::Orchestrator,
            PR_BABYSIT_SKILL | PR_REVIEWER_SKILL => Self::PullRequest,
            _ => Self::Task,
        }
    }
}

/// A workflow: a linear kanban of columns that stages one agent per column
/// through a task. Its name is its identity, and the document is the whole
/// of the syntax `ariadne_core::workflow::parse` reads.
///
/// A `NULL` document is a built-in still on the text Ariadne ships, the same
/// way a [`Skill`]'s is.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Workflow {
    pub name: String,
    /// The document set on this workflow, or NULL while a built-in runs on
    /// the text Ariadne ships. Read through [`Workflow::document_text`].
    pub document: Option<String>,
    pub builtin: i64,
    pub created_at: String,
    pub updated_at: String,
}

impl Workflow {
    /// Whether Ariadne ships this workflow, and so whether it has a default
    /// to be reset to and refuses deletion.
    pub fn is_builtin(&self) -> bool {
        self.builtin != 0
    }

    /// The document that stages an agent: the one set on it, or the text
    /// Ariadne ships under its name.
    pub fn document_text(&self) -> &str {
        self.document
            .as_deref()
            .or_else(|| default_workflow_document(&self.name))
            .unwrap_or("")
    }

    /// Whether [`Workflow::document_text`] is the shipped text rather than
    /// one somebody wrote.
    pub fn document_is_default(&self) -> bool {
        self.document.is_none()
    }

    /// The columns [`Workflow::document_text`] parses into.
    ///
    /// A row only ever holds a document a save already proved valid
    /// ([`crate::Store::create_workflow`], [`crate::Store::set_workflow_document`]),
    /// so a row that fails to parse is a schema violation rather than an
    /// input error.
    pub fn steps(&self) -> Vec<ariadne_core::workflow::WorkflowStep> {
        ariadne_core::workflow::parse(self.document_text())
            .unwrap_or_else(|e| panic!("invalid workflow document in db: {e}"))
            .steps
    }
}

/// The flags one registry agent is launched with, shared by every session
/// that runs on it. A row exists only once somebody has set flags; an agent
/// with none is launched with its registry command alone.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AgentConfig {
    /// The id of the agent in the ACP registry.
    pub agent_id: String,
    /// JSON array of argv strings.
    pub extra_flags: String,
    pub updated_at: String,
}

impl AgentConfig {
    pub fn extra_flags(&self) -> Vec<String> {
        serde_json::from_str(&self.extra_flags).unwrap_or_default()
    }
}

/// The catalog discovery last read from one registry agent, and the command
/// and version it was read from.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AcpCatalog {
    /// The id of the agent in the ACP registry.
    pub agent_id: String,
    /// JSON array of argv strings.
    pub command: String,
    pub version: String,
    /// JSON, opaque to the store: discovery writes it and reads it back.
    pub catalog: String,
    pub read_at: String,
}

impl AcpCatalog {
    pub fn command(&self) -> Vec<String> {
        serde_json::from_str(&self.command).unwrap_or_default()
    }
}

/// The last accepted index download and its source.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct AcpRegistryIndex {
    pub url: String,
    pub document: String,
    pub fetched_at: String,
}

/// The one forge settings row (027).
#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct ForgeSettings {
    /// Whether the daemon opens a tunnel to the webhook listener.
    pub tunnel_enabled: bool,
    /// The subdomain the tunnel asks for; None until the first tunnel.
    pub tunnel_subdomain: Option<String>,
    pub updated_at: String,
}

/// The one AI permission settings row, and the state of the install behind it (022).
///
/// The spelling of `state` is the wire's own (`ariadne_api::permissions`).
#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct AiPermissionSettings {
    pub enabled: bool,
    /// Danger at or below this value is allowed, 0 to 1.
    pub allow_threshold: f64,
    /// Danger at or above this value is denied, 0 to 1.
    pub deny_threshold: f64,
    /// Whether the user set the pair above by hand. Where not, the pair in
    /// force is the default of the chosen flavour, and the stored one is unread.
    pub thresholds_hand_set: bool,
    /// `0.8b`, `4b`, `9b` or `27b`.
    pub flavour: String,
    /// `mlx`, `cuda` or `cpu`. `None` until the daemon fills it at startup.
    pub device: Option<String>,
    /// `disabled`, `installing`, `ready` or `failed`.
    pub state: String,
    pub installed_release: Option<String>,
    pub latest_release: Option<String>,
    pub weights_present: bool,
    pub last_refresh_at: Option<String>,
    pub last_error: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct LearnedPermission {
    pub id: String,
    pub repository_id: String,
    pub tool_name: String,
    /// The normalized input the row answers for.
    pub key: String,
    /// `once`, `command` or `family`.
    pub level: String,
    /// The command family, else the tool name.
    pub family: String,
    /// The derived risk tags of the request, as a JSON array.
    pub risk_tags: String,
    /// `repository` or `all`.
    pub scope: String,
    /// The ACP `toolCall` as JSON, its `rawInput` with sorted keys.
    pub tool_call: String,
    /// The ACP `options` as JSON.
    pub options: String,
    pub selected_option: String,
    /// The repository permission mode at the time of the decision.
    pub target: String,
    /// The model decision as JSON, when the model was called.
    pub output: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// A git repository registered once, globally, and named by id from there on.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Repository {
    /// The workflow a new goal runs on where its request names none
    /// (030): a name of the catalog.
    pub default_workflow: String,
    pub id: String,
    /// Absolute path of the checkout.
    pub path: String,
    pub base_branch: String,
    pub description: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    /// How the ACP permission requests of its sessions are answered, as
    /// [`PermissionMode`] spells it. Read through
    /// [`Repository::permission_mode`].
    pub permission_mode: String,
    /// The forge its remote is on, where it has a usable one (025). Not a
    /// column: every read of a repository through the store fills it.
    #[sqlx(skip)]
    pub forge: Option<ForgeIntegration>,
}

/// The forge a repository's remote is on, and whether Ariadne works with it
/// (025). `host`, `owner` and `name` are lower-cased.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct ForgeIntegration {
    pub repository_id: String,
    /// As [`ForgeKind`] spells it. Read through [`ForgeIntegration::kind`].
    pub kind: String,
    pub host: String,
    pub owner: String,
    pub name: String,
    /// The name of the remote it was read off, `origin` where there is one.
    pub remote: String,
    pub enabled: bool,
    /// The account the forge CLI is signed in as, stored on enable.
    pub login: Option<String>,
    /// The pin of the session that reviews a request. None starts none.
    pub review_model: Option<String>,
    pub review_effort: Option<String>,
    pub webhook_id: Option<i64>,
    pub webhook_secret: Option<String>,
    pub webhook_url: Option<String>,
    pub webhook_state: String,
    pub webhook_error: Option<String>,
    pub webhook_last_delivery_at: Option<String>,
    /// Why the last fetch of its requests failed; None once one succeeds.
    pub fetch_error: Option<String>,
    pub detected_at: String,
    pub updated_at: String,
}

impl Repository {
    /// How the ACP permission requests of its sessions are answered. A row
    /// written by a future build that spells it some other way reads as
    /// `ask`, the one mode that approves nothing on its own.
    pub fn permission_mode(&self) -> PermissionMode {
        self.permission_mode.parse().unwrap_or(PermissionMode::Ask)
    }
}

/// The model, and optionally the effort, that a goal's orchestrator or one
/// of a task's agents runs on.
///
/// The model is `<agent>:<model>` (`ariadne_core::models::ModelRef`): the id
/// of an agent in the ACP registry, and a model of it. It is required — no
/// agent default stands in for one. Only the effort may be left out, which
/// runs the model at whatever the agent runs it at. There is nothing behind a
/// pin to fall back to: what the orchestrator sized the agent at, or what the
/// user chose instead, is the whole of the answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentPin {
    pub model: String,
    /// None = whatever the agent runs that model at.
    pub effort: Option<String>,
}

impl AgentPin {
    /// The `(model, effort)` a row is written with.
    pub(crate) fn columns(pin: &AgentPin) -> (String, Option<String>) {
        (pin.model.clone(), pin.effort.clone())
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Goal {
    /// The workflow every task of this goal runs on (030): a name of the
    /// catalog, whose columns the goal snapshotted into its `goal_steps` when
    /// it was created. Read with [`crate::Store::goal_steps`].
    pub workflow: String,
    pub id: String,
    pub title: String,
    pub description: String,
    pub issue_url: Option<String>,
    pub status: String,
    /// Whether this goal has an orchestrator for its lifetime.
    pub orchestrated: bool,
    /// Model this goal's orchestrator runs on, `<agent>:<model>`.
    pub model: String,
    /// Effort that model is run at. None = whatever the agent runs it at.
    pub effort: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    /// The goal's failed and stalled task ids, each with the id of the
    /// `task_transitions` row that moved it to `failed` when the
    /// orchestrator's own `session/prompt` turn confirmed it (a JSON
    /// array of `[id, transition id]` pairs). See
    /// [`crate::Store::confirm_goal_orchestrator_answered`].
    pub orchestrator_answered_failed_task_ids: Option<String>,
    /// When `scheduler::goals::orchestrator_could_not_start` gave up on
    /// this goal's orchestrator, distinct from the `disconnected` flag a
    /// mere crash raises. `None` while automatic recovery still owns it.
    pub orchestrator_given_up_at: Option<String>,
    /// Whether that give-up was the watchdog's own exhausted-relaunch
    /// decision (`scheduler::quiet::relaunch_wedged`, a session that
    /// started and then stopped answering) rather than
    /// `orchestrator_could_not_start`'s (one that never got off the
    /// ground). Meaningless while `orchestrator_given_up_at` is `None`.
    pub orchestrator_given_up_wedged: bool,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Task {
    /// The column the task is in while it is `in_progress`; None before its
    /// first column and once it has ended.
    pub step: Option<String>,
    pub id: String,
    pub goal_id: String,
    pub repo_id: String,
    pub title: String,
    pub description: String,
    pub status: String,
    pub branch: String,
    pub worktree_path: Option<String>,
    pub stalled: i64,
    /// The commit the task landed as, where its last column reported one.
    pub merge_commit: Option<String>,
    /// URL of the pull or merge request this task's `pr` column opened, once
    /// it has. None for a task landed directly.
    pub pr_url: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl Task {
    pub fn is_stalled(&self) -> bool {
        self.stalled != 0
    }
}

/// One agent staffed on a task: the column it works, what it runs on, what
/// it was told, and — through [`crate::Store::agent_skills`] — what it knows.
///
/// The agent has no identity of its own. `step` says only which column of the
/// task's workflow it works, which is what the scheduler and the launcher
/// need; everything about the work itself comes from its skills.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TaskAgent {
    /// The id of the column this agent works, one of the task's goal's
    /// `goal_steps`.
    pub step: String,
    pub id: String,
    pub task_id: String,
    /// The order the orchestrator listed this agent in, 0-based.
    pub ordinal: i64,
    /// Model it runs on, `<agent>:<model>`.
    pub model: String,
    /// Effort that model is run at. None = whatever the agent runs it at.
    pub effort: Option<String>,
    /// What the orchestrator told this agent beyond the task itself, where it
    /// had anything to add. None = the task is the whole of it.
    pub brief: Option<String>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AgentSession {
    pub id: String,
    pub goal_id: Option<String>,
    pub task_id: Option<String>,
    pub seat: Option<String>,
    /// The staffed agent this session runs, or None for an orchestrator,
    /// which no task staffs.
    pub task_agent_id: Option<String>,
    /// Model this session runs on, `<agent>:<model>`. Taken from the pin its
    /// seat carries — the goal for an orchestrator, the staffed agent
    /// otherwise — when the session is created, and never rewritten, so no
    /// later edit moves a running conversation onto another model.
    pub model: String,
    /// Effort this session's model is run at, copied off the same pin as
    /// `model` and never rewritten either. None = the agent's own.
    pub effort: Option<String>,
    pub internal_session_id: Option<String>,
    pub worktree_path: Option<String>,
    pub status: String,
    /// Why this session needs the user's attention, if it does. Orthogonal to
    /// `status`: an agent blocked on a permission prompt is still running.
    pub attention_reason: Option<String>,
    /// When the current `attention_reason` was first raised.
    pub attention_since: Option<String>,
    pub last_activity_at: Option<String>,
    /// The latest context window position this agent reported. Both fields
    /// stay absent until it sends a usage update.
    pub context_used: Option<i64>,
    pub context_size: Option<i64>,
    /// When this session's agent process was last started. Every launch moves
    /// it, so it dates the run the session is in rather than the row.
    pub launched_at: Option<String>,
    /// Which launch that is: a fresh id per agent process, carried by the
    /// process itself (`ARIADNE_LAUNCH_ID`) so that what it reports can be
    /// told from what the process it replaced is still reporting. None where
    /// the session has never been launched.
    pub launch_id: Option<String>,
    pub created_at: String,
    pub ended_at: Option<String>,
    /// A loose session's own title: the first prompt of the conversation it
    /// resumed, or the first one typed into it. None on a task's or a goal's
    /// session, which goes by its work's title.
    pub title: Option<String>,
    /// The session this one replaced on its seat, when a switch started it:
    /// the same seat on another pin, in a new conversation.
    pub switched_from: Option<String>,
    /// The pull request this session watches (026), or None for a session of
    /// a goal, a task or nothing at all. A pull request session has no goal,
    /// no task and no staffed agent.
    pub pull_request_id: Option<String>,
}

impl AgentSession {
    /// Whether this session has said anything since the launch it is in.
    ///
    /// `last_activity_at` is stamped by what the agent reports and by the
    /// restart that puts a row back on its feet, so it is only news where it
    /// is later than the launch itself: an agent is heard from when it
    /// reports, and a revival is not the agent.
    pub fn heard_from(&self) -> bool {
        let stamped = |at: Option<&str>| {
            at.and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
                .map(|at| at.with_timezone(&chrono::Utc))
        };
        let Some(launched) = stamped(self.launched_at.as_deref()) else {
            return false;
        };
        stamped(self.last_activity_at.as_deref()).is_some_and(|heard| heard > launched)
    }

    /// Whether this session came up and died without ever being heard from.
    ///
    /// The launch worked and the agent did not: an agent that refuses the
    /// protocol, a model it will not take, a folder it will not open, a
    /// conversation it will not reopen. What tells it from a session that
    /// ended having done its work is that nothing was ever reported under
    /// this launch, and from one still starting that it is over.
    pub fn died_on_arrival(&self) -> bool {
        self.launched_at.is_some() && !self.status().is_live() && !self.heard_from()
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Message {
    pub id: String,
    pub goal_id: String,
    /// The task it is about, or None for a message about the goal itself.
    pub task_id: Option<String>,
    /// [`MessageKind`], as the wire spells it. Read through [`Message::kind`].
    pub kind: String,
    pub from_actor: String,
    /// The staffed agent that sent it, or None for the orchestrator, the
    /// daemon and the user.
    pub from_agent_id: Option<String>,
    /// The session it was sent from. None for a message the daemon wrote, and
    /// for one whose session has since been deleted.
    pub from_session: Option<String>,
    pub to_actor: String,
    /// The staffed agent it is for, or None for the orchestrator.
    pub to_agent_id: Option<String>,
    pub body: String,
    /// When it was handed to the recipient's agent, or None while it is still
    /// waiting for one to take it.
    pub delivered_at: Option<String>,
    pub created_at: String,
}

impl Message {
    /// What this message is, or None for a row written by a build that knows
    /// a kind this one does not. Such a row is carried and shown — the one
    /// place a spelling this build does not know must not panic, since a
    /// message is what an agent typed about.
    pub fn kind(&self) -> Option<MessageKind> {
        MessageKind::from_str(&self.kind).ok()
    }

    /// Who sent it, or None for a row spelling an actor this build does not
    /// know.
    pub fn from_actor(&self) -> Option<Actor> {
        Actor::from_str(&self.from_actor).ok()
    }

    /// Who it is for.
    pub fn to_actor(&self) -> Option<Actor> {
        Actor::from_str(&self.to_actor).ok()
    }

    /// Whether it is still waiting for the recipient's agent.
    pub fn is_delivered(&self) -> bool {
        self.delivered_at.is_some()
    }
}

/// One reported event. `payload` is the JSON it carries, as it was reported:
/// the row it is read from holds that text packed, and `store::events` is the
/// one place that packs and unpacks it.
#[derive(Debug, Clone)]
pub struct AgentEvent {
    pub id: String,
    pub session_id: Option<String>,
    pub task_id: Option<String>,
    pub kind: String,
    pub payload: String,
    pub created_at: String,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TaskTransition {
    pub from_step: Option<String>,
    pub to_step: Option<String>,
    pub id: String,
    pub task_id: String,
    pub from_status: String,
    pub to_status: String,
    pub actor: String,
    pub reason: Option<String>,
    pub created_at: String,
}

/// A pull request Ariadne works on (026), as the database keeps it: the
/// daemon's own bookkeeping of it, and nothing the forge holds. A row
/// exists while Ariadne works on the request — the request a task opened,
/// one that asks for the user's review on a repository with a review pin, or
/// one of the user's they asked Ariadne to review — and goes once the
/// request merged or closed and its work was taken down. What the request
/// says — its title, its branches, its checks, its comments — is read off
/// the forge on every fetch and held in memory alone ([`PullRequestLive`]).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct PullRequestRow {
    pub id: String,
    pub repository_id: String,
    pub number: i64,
    pub url: String,
    /// `author` for a request of the user's, `reviewer` for one that asks
    /// for their review.
    pub role: String,
    /// The task that opened the request: the agent of its `pr` column keeps
    /// it (030).
    pub origin_task_id: Option<String>,
    /// Whether the request's session reported it ready to merge.
    pub ready: bool,
    /// The names of the failed checks the request's session was told of.
    pub told_checks: String,
    /// Whether the session was told the head is behind its base.
    pub told_behind_base: bool,
    /// The review decision and the state the session was last told; None
    /// is the baseline of a fresh row, `none` and `open`.
    pub told_review_decision: Option<String>,
    pub told_state: Option<String>,
    /// The rolled-up check state the session was last told; None is the
    /// baseline of a fresh row, `none`.
    pub told_check_state: Option<String>,
    /// When the session was last handed news: the claim of its prompt.
    pub news_told_at: Option<String>,
    /// The head a reviewer session last posted a review on (029).
    pub reviewed_sha: Option<String>,
    /// The head the request's session was last told of (029).
    pub told_head_sha: Option<String>,
    /// Whether the user asked Ariadne to review a request of their own
    /// (029): a review session runs on it while it is open.
    pub review_asked: bool,
    /// The forge id of the one summary comment an Ariadne review keeps on
    /// the request, edited on every round (029); None before its first.
    pub summary_comment_id: Option<String>,
    /// The pin the user picked for the review they asked of a request of
    /// their own (029); None where nobody asked.
    pub review_model: Option<String>,
    pub review_effort: Option<String>,
    /// The skills the user picked for that review beside `pr-reviewer`, as a
    /// JSON list.
    #[sqlx(rename = "review_skills")]
    pub review_skills_json: String,
    pub created_at: String,
    pub updated_at: String,
    /// When `scheduler::pull_requests::start_pull_request_session` gave up
    /// on this request's reviewer session: its spawn-retry budget ran out,
    /// distinct from a crash the liveness sweep is about to retry. `None`
    /// while automatic recovery still owns it.
    pub reviewer_given_up_at: Option<String>,
    /// Whether that give-up was the watchdog's own exhausted-relaunch
    /// decision rather than `start_pull_request_session`'s own
    /// spawn-retry exhaustion — the same distinction, for the same
    /// reason, as `Goal::orchestrator_given_up_wedged`. Meaningless while
    /// `reviewer_given_up_at` is `None`.
    pub reviewer_given_up_wedged: bool,
    /// When `ready` last moved from false to true: the babysitting task's
    /// own claim that the request is ready to merge, read as the
    /// `pull_request` attention producer's `since` for the readiness item
    /// it raises once the forge's own evidence backs that claim up.
    /// Cleared the moment `ready` moves back to false, so a later claim
    /// gets its own, fresh `since` rather than the first one's.
    pub ready_confirmed_at: Option<String>,
}

/// What the forge says of a request, as the last read found it: held in
/// memory, never stored (026).
#[derive(Debug, Clone, Default)]
pub struct PullRequestLive {
    pub title: String,
    /// The request's description.
    pub body: String,
    pub author_login: String,
    /// `open`, `merged` or `closed`.
    pub state: String,
    pub draft: bool,
    pub head_branch: String,
    pub head_sha: String,
    pub head_repo: Option<String>,
    pub base_branch: String,
    /// The rolled-up checks: `pending`, `success`, `failure` or `none`.
    pub checks: String,
    pub review_decision: String,
    /// The forge's own mergeability of the head now: `clean`, `blocked`,
    /// `dirty` or `unknown`.
    pub mergeable: String,
    pub opened_at: String,
    /// When the forge last saw the request move.
    pub forge_updated_at: String,
    /// The checks that failed on the head: a JSON list of `{name, url,
    /// conclusion}`.
    pub failed_checks: String,
    /// Whether the base branch has commits the head does not.
    pub behind_base: bool,
    /// Whether the request asks for the user's review (029).
    pub review_requested: bool,
    /// The commit a merged request landed as (005).
    pub merge_sha: Option<String>,
    /// The threads that wait on the integration login.
    pub unanswered_comments: i64,
}

/// A request Ariadne works on, as the daemon reads it: its row, and what
/// the forge says of it now.
#[derive(Debug, Clone)]
pub struct PullRequest {
    pub id: String,
    pub repository_id: String,
    pub number: i64,
    pub url: String,
    pub title: String,
    pub body: String,
    pub author_login: String,
    pub state: String,
    pub draft: bool,
    pub head_branch: String,
    pub head_sha: String,
    pub head_repo: Option<String>,
    pub base_branch: String,
    pub checks: String,
    pub review_decision: String,
    pub mergeable: String,
    pub unanswered_comments: i64,
    pub origin_task_id: Option<String>,
    pub opened_at: String,
    pub forge_updated_at: String,
    pub role: String,
    pub ready: bool,
    pub created_at: String,
    pub updated_at: String,
    pub failed_checks: String,
    pub behind_base: bool,
    pub told_checks: String,
    pub told_behind_base: bool,
    pub told_review_decision: Option<String>,
    pub told_state: Option<String>,
    pub told_check_state: Option<String>,
    pub news_told_at: Option<String>,
    pub reviewed_sha: Option<String>,
    pub told_head_sha: Option<String>,
    pub review_requested: bool,
    pub review_asked: bool,
    pub merge_sha: Option<String>,
    pub summary_comment_id: Option<String>,
    pub review_model: Option<String>,
    pub review_effort: Option<String>,
    pub review_skills_json: String,
    pub reviewer_given_up_at: Option<String>,
    pub reviewer_given_up_wedged: bool,
    pub ready_confirmed_at: Option<String>,
}

impl PullRequest {
    /// The request `row` keeps, as the forge reads it now.
    pub fn of(row: PullRequestRow, live: PullRequestLive) -> Self {
        Self {
            id: row.id,
            repository_id: row.repository_id,
            number: row.number,
            url: row.url,
            title: live.title,
            body: live.body,
            author_login: live.author_login,
            state: live.state,
            draft: live.draft,
            head_branch: live.head_branch,
            head_sha: live.head_sha,
            head_repo: live.head_repo,
            base_branch: live.base_branch,
            checks: live.checks,
            review_decision: live.review_decision,
            mergeable: live.mergeable,
            unanswered_comments: live.unanswered_comments,
            origin_task_id: row.origin_task_id,
            opened_at: live.opened_at,
            forge_updated_at: live.forge_updated_at,
            role: row.role,
            ready: row.ready,
            created_at: row.created_at,
            updated_at: row.updated_at,
            failed_checks: live.failed_checks,
            behind_base: live.behind_base,
            told_checks: row.told_checks,
            told_behind_base: row.told_behind_base,
            told_review_decision: row.told_review_decision,
            told_state: row.told_state,
            told_check_state: row.told_check_state,
            news_told_at: row.news_told_at,
            reviewed_sha: row.reviewed_sha,
            told_head_sha: row.told_head_sha,
            review_requested: live.review_requested,
            review_asked: row.review_asked,
            merge_sha: live.merge_sha,
            summary_comment_id: row.summary_comment_id,
            review_model: row.review_model,
            review_effort: row.review_effort,
            review_skills_json: row.review_skills_json,
            reviewer_given_up_at: row.reviewer_given_up_at,
            reviewer_given_up_wedged: row.reviewer_given_up_wedged,
            ready_confirmed_at: row.ready_confirmed_at,
        }
    }

    /// The skills the user picked for the review of a request of their own
    /// (029), beside the `pr-reviewer` every review session loads.
    pub fn review_skills(&self) -> Vec<String> {
        serde_json::from_str(&self.review_skills_json).unwrap_or_default()
    }
}

/// What the database keeps of one comment of a request Ariadne works on
/// (026): whether its session was told of it, and whether an Ariadne review
/// posted it. Its text, its author and its thread are the forge's.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct CommentMark {
    pub pull_request_id: String,
    pub forge_id: String,
    pub told_at: Option<String>,
    pub from_review: bool,
}

/// One comment on a pull request, as the forge holds it and the daemon's
/// marks read beside it (026): a review comment on a line, a comment on the
/// conversation, or the body of a review.
#[derive(Debug, Clone)]
pub struct PullRequestComment {
    /// The forge's own id of the comment, which is also its id here.
    pub id: String,
    pub pull_request_id: String,
    pub forge_id: String,
    /// The forge's thread or discussion the comment is in.
    pub thread_id: String,
    pub kind: String,
    pub author_login: String,
    pub author_is_bot: bool,
    pub body: String,
    pub path: Option<String>,
    pub line: Option<i64>,
    pub in_reply_to: Option<String>,
    /// When the forge says the comment was written.
    pub created_at: String,
    /// A later comment in the thread is by the integration login.
    pub answered: bool,
    /// The forge reports the thread resolved.
    pub resolved: bool,
    /// When the comment was handed to the request's session.
    pub told_at: Option<String>,
    /// An Ariadne review session posted it (029): on a request of the
    /// user's own it is a finding for the task's author, not the user's.
    pub from_review: bool,
}

/// A column captured when a goal starts.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct GoalStep {
    pub goal_id: String,
    pub ordinal: i64,
    pub id: String,
    pub title: String,
    pub description: String,
    pub skills: String,
    pub rank: Option<String>,
    pub gate: Option<String>,
}
