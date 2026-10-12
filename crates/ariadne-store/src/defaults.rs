//! Built-in default texts: the lifecycle prompts, the skills and the
//! workflows Ariadne ships.
//!
//! The one place a default text lives. A lifecycle briefing is read from these
//! constants on every launch and every resume, and no row holds one of its
//! own. A skill is stored with a `NULL` document while it runs on the text
//! here, and a reset drops what was written over it rather than copying a
//! default in. Because nothing is ever copied into the database, rewording a
//! text here reaches every database that never edited it.
//!
//! Each rule is written once, in the layer it belongs to. A system prompt
//! states what a seat owes, from its first read to the call that ends its
//! turn, and says nothing about the work itself — what an agent can do comes
//! from its skills. A skill states how one kind of work is done; for the
//! orchestrator that work is Ariadne's own planning loop, so its playbook is
//! a skill too ([`ORCHESTRATION_SKILL`]). A workflow column states what its
//! agent does with the task, and the skills it does it with. A briefing
//! template carries the values of one goal, task or column and whatever is
//! only true of this moment — the reason a task came back to a column — and
//! nothing of the playbook that reached the agent. A resume is a nudge: where
//! the work stands and what ends it. What every session is told alike — that
//! Ariadne is reached through its MCP tools, whom a question reaches, and how
//! few turns to take — is the MCP server's `instructions`, which every
//! session already receives, and appears in no prompt here. What each seat
//! does when it cannot go on is one line of its own: the orchestrator asks
//! the user, a column's agent gives the task up or hands it back.
//!
//! Every text here is written in ASD-STE100 Simplified Technical English: one
//! instruction to a sentence, the imperative for an instruction, the active
//! voice, sentences that stay short, one meaning per word, a list for a
//! sequence of steps. It is what an agent misreads least and pays fewest
//! tokens for, and each playbook holds the agent to it in turn — the
//! orchestrator for its task descriptions, a column's agent for its step
//! reasons, commit text and failure reasons. That the rule holds for every
//! seat, and for every word an agent writes, is the MCP server's session
//! rules to say, so `STE` is all a text here spells.
//!
//! The texts are kept small on purpose. `size_caps_hold` keeps the lifecycle
//! prompts small, `skill_size_caps_hold` keeps the skills small, and
//! `every_default_text_is_simplified_technical_english` keeps the sentences
//! short across both.

use ariadne_core::{PromptKind, Seat};

/// A skill Ariadne ships: one document that tells a generic agent how to do
/// one kind of work.
///
/// The document is the whole `SKILL.md` — YAML frontmatter naming the skill
/// and describing it, then the body — in the format Claude Code and Codex both
/// read, so one text serves every agent CLI. It lives beside this crate under
/// `skills/<name>/SKILL.md` and is compiled in, which is what lets a rewritten
/// skill reach every database without a migration.
pub struct BuiltinSkill {
    pub name: &'static str,
    pub document: &'static str,
}

/// The one skill that is the orchestrator's rather than a task agent's: the
/// playbook a goal is planned and seen through on. The name is the whole of
/// the marking — [`crate::Skill::seat`] reads it, no column stores it — and
/// the launcher loads this skill for every orchestrator session.
pub const ORCHESTRATION_SKILL: &str = "orchestration";

/// The skill that keeps an open request moving (026, 030): the `pr` column
/// of `develop-review-pr` stages it, and its agent keeps the request until a
/// human merges or closes it. Its seat is a fact of the name, as the
/// orchestrator's is.
pub const PR_BABYSIT_SKILL: &str = "pr-babysit";

/// The skill of a reviewer pull request session (029): the daemon loads it
/// for every session it starts on an open request that asks for the user's
/// review. Its seat is a fact of the name, as `pr-babysit`'s is.
pub const PR_REVIEWER_SKILL: &str = "pr-reviewer";

/// The skills a fresh database is seeded with, grouped by what they are for:
/// orchestrating a goal, producing work, reviewing it, and operating what it
/// produced.
///
/// The catalog is the whole of what an agent can be, so adding a skill here is
/// adding a kind of work Ariadne knows how to staff. One skill is nobody's to
/// staff: [`ORCHESTRATION_SKILL`] belongs to the orchestrator's seat, and the
/// store refuses a task agent staffed on it.
pub const BUILTIN_SKILLS: [BuiltinSkill; 16] = [
    // Orchestrating.
    builtin(
        ORCHESTRATION_SKILL,
        include_str!("../skills/orchestration/SKILL.md"),
    ),
    // Producing.
    builtin(
        "spec-writing",
        include_str!("../skills/spec-writing/SKILL.md"),
    ),
    builtin("coding", include_str!("../skills/coding/SKILL.md")),
    builtin("debugging", include_str!("../skills/debugging/SKILL.md")),
    builtin(
        "refactoring",
        include_str!("../skills/refactoring/SKILL.md"),
    ),
    builtin(
        "documentation",
        include_str!("../skills/documentation/SKILL.md"),
    ),
    builtin("research", include_str!("../skills/research/SKILL.md")),
    // Reviewing.
    builtin(
        "code-review",
        include_str!("../skills/code-review/SKILL.md"),
    ),
    builtin(
        "spec-review",
        include_str!("../skills/spec-review/SKILL.md"),
    ),
    builtin(
        "performance-review",
        include_str!("../skills/performance-review/SKILL.md"),
    ),
    builtin(
        "architecture-review",
        include_str!("../skills/architecture-review/SKILL.md"),
    ),
    builtin(
        PR_REVIEWER_SKILL,
        include_str!("../skills/pr-reviewer/SKILL.md"),
    ),
    // Operating.
    builtin("migration", include_str!("../skills/migration/SKILL.md")),
    builtin(
        "conflict-resolution",
        include_str!("../skills/conflict-resolution/SKILL.md"),
    ),
    builtin(
        PR_BABYSIT_SKILL,
        include_str!("../skills/pr-babysit/SKILL.md"),
    ),
    // The `merge` column of the shipped `develop-review-merge` workflow
    // staffs this: land a reviewed task on the base branch itself.
    builtin("merge", include_str!("../skills/merge/SKILL.md")),
];

const fn builtin(name: &'static str, document: &'static str) -> BuiltinSkill {
    BuiltinSkill { name, document }
}

/// The skills that merged into another, each with the skill that took over
/// its work.
///
/// A merge is not a drop. The name here is gone from the catalog, but the
/// work it covered is still done, so the rows that named it are rewritten to
/// the name that does it now ([`crate::Store`] applies this on every open).
/// A task staffed before the merge then reads as the work it did, and the
/// row of the merged skill is left for the prune to take out.
///
/// `testing` merged into `coding`: 92 of 96 staffings of it sat beside
/// `coding` on the same agent, and an agent that wrote the code wrote the
/// tests with it. One document states both, and the orchestrator makes one
/// decision instead of two.
pub const MERGED_SKILLS: [(&str, &str); 1] = [("testing", "coding")];

/// The document Ariadne ships under `name`, or `None` where it ships none —
/// which is every skill the user wrote, and those carry their own text.
pub fn default_skill_document(name: &str) -> Option<&'static str> {
    BUILTIN_SKILLS
        .iter()
        .find(|s| s.name == name)
        .map(|s| s.document)
}

/// A workflow Ariadne ships: a linear kanban of columns, parsed by
/// [`ariadne_core::workflow::parse`]. Stored the same way a [`BuiltinSkill`]
/// is: a `NULL` document while it runs on the text here, so a rewording
/// reaches every database without a migration.
pub struct BuiltinWorkflow {
    pub name: &'static str,
    pub document: &'static str,
}

const fn builtin_workflow(name: &'static str, document: &'static str) -> BuiltinWorkflow {
    BuiltinWorkflow { name, document }
}

/// The workflow a repository registered without one runs its goals on, and
/// the one every database that predates workflows was moved onto: the task
/// lands on the base branch itself.
pub const DEFAULT_WORKFLOW: &str = "develop-review-merge";

/// The workflow that lands a task by a request a human merges or closes.
pub const PULL_REQUEST_WORKFLOW: &str = "develop-review-pr";

/// The two workflows Ariadne ships: one that lands a task on the base branch
/// itself, and one that lands it by a request a human merges or closes.
pub const BUILTIN_WORKFLOWS: [BuiltinWorkflow; 2] = [
    builtin_workflow(
        DEFAULT_WORKFLOW,
        include_str!("../workflows/develop-review-merge.workflow"),
    ),
    builtin_workflow(
        PULL_REQUEST_WORKFLOW,
        include_str!("../workflows/develop-review-pr.workflow"),
    ),
];

/// The document Ariadne ships under `name`, or `None` where it ships none —
/// which is every workflow the user wrote, and those carry their own text.
pub fn default_workflow_document(name: &str) -> Option<&'static str> {
    BUILTIN_WORKFLOWS
        .iter()
        .find(|w| w.name == name)
        .map(|w| w.document)
}

/// The one-line `description` of a `SKILL.md`, which is what the index in an
/// agent's system prompt carries and what a listing shows.
///
/// Read off the YAML frontmatter rather than stored beside it, so a rewritten
/// document cannot disagree with the summary of itself. A document with no
/// frontmatter, or none naming a description, has no summary; the caller says
/// what to show instead.
pub fn skill_summary(document: &str) -> Option<&str> {
    let rest = document.strip_prefix("---\n")?;
    let end = rest.find("\n---")?;
    rest[..end]
        .lines()
        .find_map(|line| line.trim().strip_prefix("description:"))
        .map(str::trim)
        .filter(|summary| !summary.is_empty())
}

/// The text of a skill `document` that an agent reads.
pub fn skill_text(document: &str) -> String {
    document.to_string()
}

/// The system prompt an agent in `seat` is spawned with.
///
/// It says what the seat owes and nothing about the work itself: what an
/// agent can do comes from the skills it loads, so this text is Ariadne's own
/// and no row overrides it. A pull request reviewer (029) has a seat text of
/// its own, [`pull_request_system_prompt`], and so does a request's keeper
/// (032), [`pull_request_keeper_system_prompt`].
pub fn default_system_prompt(seat: Seat) -> &'static str {
    match seat {
        Seat::Orchestrator => ORCHESTRATOR_SYSTEM_PROMPT,
        Seat::Agent => AGENT_SYSTEM_PROMPT,
        Seat::Reviewer => PULL_REQUEST_REVIEW_SYSTEM_PROMPT,
        Seat::Keeper => PULL_REQUEST_KEEPER_SYSTEM_PROMPT,
    }
}

/// The built-in text of `kind`: what every session of that lifecycle step is
/// briefed with, the same for every agent.
pub fn default_prompt_text(kind: PromptKind) -> &'static str {
    match kind {
        PromptKind::OrchestratorBriefing => ORCHESTRATOR_BRIEFING,
        PromptKind::OrchestratorResume => ORCHESTRATOR_RESUME,
        PromptKind::GoalAttention => GOAL_ATTENTION,
        PromptKind::IncomingMessage => INCOMING_MESSAGE,
        PromptKind::StepBriefing => STEP_BRIEFING,
        PromptKind::StepReturn => STEP_RETURN,
        PromptKind::AgentResume => AGENT_RESUME,
    }
}

/// The system prompt of a session that reviews a request (029): what the
/// seat owes, as [`default_system_prompt`] says it for the task seats. The
/// work itself is the [`PR_REVIEWER_SKILL`] document. A request of the
/// user's own has no session of its own: the `pr` column of its task keeps
/// it (030).
pub fn pull_request_system_prompt() -> &'static str {
    PULL_REQUEST_REVIEW_SYSTEM_PROMPT
}

/// The briefing a review session starts on: the request, and the values its
/// commands act on.
pub fn pull_request_briefing_prompt() -> &'static str {
    PULL_REQUEST_REVIEW_BRIEFING
}

/// The prompt the daemon wakes a session with when a request has news: one
/// line per thing it has not been told yet, rendered by `forge::news`. The
/// session is the review session of a request (029), or the agent of the
/// `pr` column of the task that opened it (030).
pub fn pull_request_news_prompt() -> &'static str {
    PULL_REQUEST_NEWS
}

/// The placeholders [`pull_request_briefing_prompt`] and
/// [`pull_request_news_prompt`] are rendered with, by the daemon's
/// `prompts` builders.
pub const PULL_REQUEST_PLACEHOLDERS: [&str; 10] = [
    "title",
    "url",
    "repo_path",
    "worktree_path",
    "head_branch",
    "base_branch",
    "login",
    "news",
    "head_sha",
    "reviewed_sha",
];

/// The system prompt of a session that keeps a request of the user's own
/// asked for by hand (032): what the seat owes, as [`default_system_prompt`]
/// says it for the task seats. The work itself is the [`PR_BABYSIT_SKILL`]
/// document, the same one the `pr` column of a task stages (030).
pub fn pull_request_keeper_system_prompt() -> &'static str {
    PULL_REQUEST_KEEPER_SYSTEM_PROMPT
}

/// The briefing a keeper session starts on: the request, its worktree and
/// branch, and how to push a fix to it.
pub fn pull_request_keeper_briefing_prompt() -> &'static str {
    PULL_REQUEST_KEEPER_BRIEFING
}

/// The placeholders [`pull_request_keeper_briefing_prompt`] is rendered
/// with, by the daemon's `prompts` builders. A keeper session is woken with
/// news on [`pull_request_news_prompt`], the same text a review session is.
pub const PULL_REQUEST_KEEPER_PLACEHOLDERS: [&str; 8] = [
    "title",
    "url",
    "repo_path",
    "worktree_path",
    "branch",
    "base_branch",
    "login",
    "push_command",
];

/// Orchestrator seat text: what the seat owes, and no step of the playbook.
///
/// The playbook — the phases from reading the goal to `complete_goal`, and
/// the one place `finalize_plan` is explained — is the
/// [`ORCHESTRATION_SKILL`] document, which the launcher loads for every
/// orchestrator session the way a task agent's skills reach it: indexed
/// beside this text, written into the run directory. So the playbook is
/// editable and resettable like any shipped skill, and this text is
/// Ariadne's own like the agent's.
///
/// What is owed is what no skill edit is allowed to take away: the plan is
/// made *with* the user — the orchestrator is the one seat that talks to
/// them — it writes no code, and a point it cannot settle goes to the user
/// rather than being decided alone. How to ask is the session rules' (013),
/// which every seat reads, so this text names no tool.
const ORCHESTRATOR_SYSTEM_PROMPT: &str =
    r#"Never write code. Plan the goal with the user. Decide no open point alone."#;

/// The one task seat text: what the agent of a column owes, whatever the
/// column is. The work of the column is its skills'; what each of the three
/// calls that move the task is for is said here and nowhere else. When the
/// turn ends is the call's own description to say, and how to ask the user
/// is the session rules'.
const AGENT_SYSTEM_PROMPT: &str = r#"Work only in the task's worktree, on its branch. Work on your current column alone. Read its skills first.
- Call `complete_step` when the column's work is done.
- Call `fail_step` when the previous column must fix something.
- Call `fail_task` when nobody can do the task as written.
- Never pass `--no-verify` or `-n` to git. A refused hook names the fix. Make it and commit again."#;

/// First briefing of a column's agent: the task, the column, the values its
/// commands act on, and what the column before it said.
const STEP_BRIEFING: &str = r#"# {task_title}: {step_title} ({step_id})
{task_description}
{step_description}
Goal: {goal_title}
Worktree: {worktree_path}
Branch: {branch}
Base: {base_branch}
Repo: {repo_path}
Previous: {previous_summary}
Dependencies: {dependencies}
Do this column now."#;

/// What a column's agent is briefed with when the task comes back to its
/// column: which way it came, and why.
const STEP_RETURN: &str = "Resume {task_title} at {step_title}.\nDirection: {direction}\n{reason}";

/// What the agent of the current column is nudged with when it has gone
/// quiet with the task still in front of it.
const AGENT_RESUME: &str = "Continue {task_title} at {step_title}.";

/// Seat text of a reviewer pull request session (029).
const PULL_REQUEST_REVIEW_SYSTEM_PROMPT: &str = r#"You review one open pull or merge request where the user is a requested reviewer. Work only in your worktree, detached at the head of the request. Commit nothing and push nothing. Ariadne wakes you with the request and its news. Review it as your skill says. Then end your turn."#;

/// Initial briefing of a reviewer pull request session (029).
const PULL_REQUEST_REVIEW_BRIEFING: &str = r#"# Review pull request: {title}

{url}

## Context
- Repo: {repo_path}
- Worktree (your cwd): {worktree_path}, at {head_sha}
- Branch: {head_branch} onto {base_branch}
- Your login: {login}
- Last reviewed sha: {reviewed_sha}

Review this request."#;

/// What a session is woken with when a request has news: the news since it
/// was last told, one line each.
const PULL_REQUEST_NEWS: &str = r#"News on "{title}":
{news}

Handle each item. Then end your turn."#;

/// Seat text of a session that keeps a request of the user's own asked for
/// by hand (032).
const PULL_REQUEST_KEEPER_SYSTEM_PROMPT: &str = r#"You keep one open request of the user's own. Work only in your worktree, on its branch. Commit a fix and push it as the briefing says. Ariadne wakes you with the news. It ends you on the merge or the close. Then end your turn."#;

/// Initial briefing of a keeper session (032): the request, the worktree and
/// branch it works on, and the command that pushes a fix to the request.
const PULL_REQUEST_KEEPER_BRIEFING: &str = r#"# Keep pull request: {title}

{url}

## Context
- Repo: {repo_path}
- Worktree (your cwd): {worktree_path}
- Branch: {branch}, onto {base_branch}
- Your login: {login}
- Push with: {push_command}

Keep this request."#;

/// Initial briefing of an orchestrator session: the goal, its workflow with
/// every column, and the repositories it works in.
///
/// No numbers. How many tasks a goal takes is what the conversation with the
/// user settles (003), and a cap written down before that conversation could
/// only be a guess the orchestrator then has to plan around.
///
/// The workflow is settled before planning starts — at the goal, not asked
/// task by task — so it is read here rather than asked for. Its columns are
/// what every task is staffed against, one agent each, which is why they are
/// in the one text that starts the plan.
const ORCHESTRATOR_BRIEFING: &str = r#"# Goal: {goal_title}

{goal_description}

Workflow: {workflow}
{columns}

## Repositories
{repositories}"#;

/// What an orchestrator that has gone quiet is picked up with, in both
/// situations there are.
///
/// While the goal is in planning it stands somewhere in the conversation —
/// a question waited on, a revision, a plan the user has not said yes to —
/// and only the orchestrator knows where. So the nudge names the shape of
/// the work rather than one step of it: a nudge that named `finalize_plan`
/// alone would push it to start a plan nobody agreed to, and one that named
/// a question alone would push it to ask again over an answer it has.
///
/// Once the goal is active the orchestrator is the one agent that outlives
/// its own hand-off, and what it is woken for is on the tasks: one that
/// failed, one that has gone quiet, or a goal with nothing left to do.
const ORCHESTRATOR_RESUME: &str = r#"Continue "{goal_title}" where it stands. Without an explicit yes on the plan, stay in the conversation that gets one. With one, call `finalize_plan`. Once the goal is under way, read `list_tasks`."#;

/// What the orchestrator of a goal under way is woken with.
///
/// The orchestrator outlives its own hand-off: it stays up for the whole
/// goal, so the user can ask it anything and so the daemon has somebody to
/// tell when a task needs a decision. That decision is the point of this
/// text — a task that failed can be retried, rewritten or given up on, and
/// only the orchestrator holds the plan those choices are made against. A
/// retry starts on the first column and needs an agent on every column,
/// which a task moved onto a workflow may lack.
///
/// The tasks are rendered by the scheduler that noticed them, one line each,
/// because what happened is the daemon's to say and what to do about it is
/// not.
const GOAL_ATTENTION: &str = r#"The tasks of "{goal_title}" need you:

{tasks}

Read them with `list_tasks`. Retry, rewrite or cancel each one. Call `complete_goal` when every task is done."#;

/// What one agent said to another, as it reaches the recipient's agent.
///
/// The agents talk to each other, and this is the whole of the transport: the
/// runtime hands the message to the agent as a prompt, so it reaches the agent
/// as a turn rather than as something it has to go and look for.
///
/// The sender is named by its column, the task it is of and that task's
/// title. A task staffs one agent per column, so the column names one
/// sender, and it is the address an answer goes to: `{answer_to}` names it.
/// A message with no task, such as the orchestrator's, is named by its seat
/// alone, and `{answer_to}` is empty: there is no address to name.
const INCOMING_MESSAGE: &str = r#"Message from {from}:

{body}

Answer only a question{answer_to}. Then go on with your work."#;

/// What the orchestrator is told when an exhausted agent moved to another
/// model on its own (021): the column, the task, both models, and that the
/// user hears of it. Rendered by the scheduler that made the switch.
const EXHAUSTED_SWITCH_NOTICE: &str = r#"The {column} agent of "{task_title}" moved from {old} to {new}. The old model ran out. Tell the user."#;

/// The notice an orchestrator is sent when the scheduler moved an exhausted
/// agent to another model.
pub fn exhausted_switch_notice() -> &'static str {
    EXHAUSTED_SWITCH_NOTICE
}

/// The two STE rules a text can be held to by reading it.
///
/// Every text an agent reads here is ASD-STE100 Simplified Technical English:
/// one instruction to a sentence, the imperative for an instruction, the
/// active voice, short sentences, one meaning per word. Two of those rules
/// are countable — how long a sentence runs, and which words it uses — and
/// both crates that hold agent-facing text count them the same way: the
/// defaults above, and the tool descriptions and session rules of the MCP
/// server in `ariadne-cli`.
pub mod ste {
    /// The words an agent-facing text never uses: the long spelling of a
    /// short word, and the ones that leave an instruction optional or vague.
    const BANNED: [&str; 6] = [
        "utilise",
        "prior to",
        "in order to",
        "ensure",
        "should",
        "may",
    ];

    /// The longest a sentence runs. STE holds a procedure to 20 words and a
    /// description to 25. Every text here is read as a procedure, and the
    /// session rules tell the agents 20, so 20 is the one number.
    pub const MAX_WORDS: usize = 20;

    /// The sentences of `text`, as the rules are read on them.
    ///
    /// A line is a statement of its own — a heading, a bullet, a numbered
    /// step — and a line holding several sentences is cut at every `.`, `!`
    /// or `?` that a space or the end of the line follows. The stop inside
    /// `AGENTS.md` is followed by a letter and cuts nothing, and the `1.` a
    /// step opens on is left alone. A `step 1.` that ends a sentence ends it.
    pub fn sentences(text: &str) -> Vec<&str> {
        let mut out = Vec::new();
        for line in text.lines() {
            let (mut start, mut previous) = (0, None);
            for (at, ch) in line.char_indices() {
                let end = at + ch.len_utf8();
                // A closing quote right after the stop belongs to the
                // sentence it closes.
                let end = match line[end..].strip_prefix('"') {
                    Some(_) => end + 1,
                    None => end,
                };
                if matches!(ch, '.' | '!' | '?')
                    && end > start
                    && !(previous.is_some_and(|c: char| c.is_ascii_digit())
                        && line[..at].trim().chars().all(|c| c.is_ascii_digit()))
                    && line[end..].chars().next().is_none_or(|c| c == ' ')
                {
                    out.push(line[start..end].trim());
                    start = end;
                }
                previous = Some(ch);
            }
            out.push(line[start..].trim());
        }
        out.retain(|sentence| !sentence.is_empty());
        out
    }

    /// A markdown document with each wrapped sentence on one line again, so
    /// [`sentences`] reads the sentence and not the fragments a wrap left.
    ///
    /// A line continues the one before it where neither is blank, a
    /// heading or a fence, and it opens no list item of its own. A fenced
    /// block is left out: it holds an example, not a sentence.
    #[cfg(test)]
    pub(crate) fn unwrapped_lines(document: &str) -> String {
        let opens_item = |line: &str| {
            line.starts_with("- ")
                || line
                    .split_once(". ")
                    .is_some_and(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
        };
        let mut lines: Vec<String> = Vec::new();
        let mut fenced = false;
        for line in document.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("```") {
                fenced = !fenced;
                lines.push(String::new());
                continue;
            }
            if fenced {
                continue;
            }
            let continues = lines
                .last()
                .is_some_and(|last| !last.trim().is_empty() && !last.starts_with('#'))
                && !trimmed.is_empty()
                && !trimmed.starts_with('#')
                && !opens_item(trimmed);
            match lines.last_mut() {
                Some(last) if continues => {
                    last.push(' ');
                    last.push_str(trimmed);
                }
                _ => lines.push(line.to_string()),
            }
        }
        lines.join("\n")
    }

    /// The first banned word `text` uses, whole and whatever its case.
    pub fn banned_word(text: &str) -> Option<&'static str> {
        let lowered = text.to_lowercase();
        BANNED.into_iter().find(|word| {
            lowered.match_indices(word).any(|(at, _)| {
                let before = lowered[..at].chars().next_back();
                let after = lowered[at + word.len()..].chars().next();
                !before.is_some_and(char::is_alphanumeric)
                    && !after.is_some_and(char::is_alphanumeric)
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SkillSeat;

    /// Every default text there is, named as the test failures name it: the
    /// system prompt of each seat, the template of each prompt kind, and the
    /// texts of a pull request.
    fn all_defaults() -> Vec<(String, &'static str)> {
        Seat::ALL
            .into_iter()
            .map(|seat| {
                (
                    format!("{} system prompt", seat.as_str()),
                    default_system_prompt(seat),
                )
            })
            .chain(
                PromptKind::ALL
                    .into_iter()
                    .map(|kind| (kind.as_str().to_string(), default_prompt_text(kind))),
            )
            .chain(pull_request_texts())
            .chain([(
                "exhausted switch notice".to_string(),
                exhausted_switch_notice(),
            )])
            .collect()
    }

    /// The texts of a review session, of a keeper session and of a
    /// request's news, named for a failure.
    fn pull_request_texts() -> [(String, &'static str); 3] {
        [
            (
                "pull request review briefing".into(),
                pull_request_briefing_prompt(),
            ),
            ("pull request news".into(), pull_request_news_prompt()),
            (
                "pull request keeper briefing".into(),
                pull_request_keeper_briefing_prompt(),
            ),
        ]
    }

    /// Every shipped skill, named the way a failure names it.
    fn all_skills() -> Vec<(String, &'static str)> {
        BUILTIN_SKILLS
            .iter()
            .map(|s| (format!("{} skill", s.name), s.document))
            .collect()
    }

    /// Every shipped workflow document, named the way a failure names it.
    fn all_workflows() -> Vec<(String, &'static str)> {
        BUILTIN_WORKFLOWS
            .iter()
            .map(|w| (format!("{} workflow", w.name), w.document))
            .collect()
    }

    /// `text` with its line wrapping taken out, so a test reads a marker as
    /// the sentence a skill states rather than as the fragment a wrap left on
    /// one line.
    fn unwrapped(text: &str) -> String {
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// The size a prompt may grow back to, per kind, and in total.
    ///
    /// Every text here is sent to a real agent on a real turn, so what is
    /// spent on prose is not spent on the task. The caps are what stop the
    /// texts creeping back up: a rule restated in a second layer, a procedure
    /// explained twice, a closing paragraph repeating the playbook, all show
    /// up as characters.
    ///
    /// The totals are over the prompt kinds — what a briefing costs per turn
    /// — with the seat texts pinned separately and every text counted again
    /// in a grand total, since a session pays for one of each.
    ///
    /// Their history is a long creep, one cut, and one contraction. The
    /// author's and the reviewer's seat texts grew to 1250 and 1400 for the
    /// division of the checks and the verdict rules, and their briefings,
    /// resumes and the landing procedures of three endings grew beside them
    /// to over 9000 characters of defaults. The workflow columns took every
    /// one of those rules: a column's skills say how its work is done, its
    /// gate says what proves it, and the two step calls move the task. What
    /// is left is one task seat text of 750, three step texts of 300 each,
    /// the orchestrator's three, the message, and the two texts of a pull
    /// request session. Moving a cap is a decision to argue for, never a way
    /// round a failing assertion.
    #[test]
    fn size_caps_hold() {
        // Three step texts, three orchestrator texts and the message.
        const KIND_TOTAL: usize = 1500;
        // Every seat text, every kind and the three pull request texts.
        const GRAND_TOTAL: usize = 3400;

        // A cap per seat. The orchestrator's carried its playbook up to
        // 1750; the playbook is the `orchestration` skill now, and 200 holds
        // what is left to the seat text it is. The agent's seven steps fit in
        // 750, and the pull request reviewer's and keeper's one paragraph
        // each in 400.
        let system_cap = |seat: Seat| match seat {
            Seat::Agent => 750,
            Seat::Orchestrator => 200,
            Seat::Reviewer => 400,
            Seat::Keeper => 400,
        };
        let cap = |kind: PromptKind| match kind {
            PromptKind::OrchestratorResume | PromptKind::AgentResume => 200,
            _ => 300,
        };

        for (name, text) in all_defaults() {
            println!("{:5}  {name}", text.len());
        }

        for seat in Seat::ALL {
            let text = default_system_prompt(seat);
            assert!(
                text.len() <= system_cap(seat),
                "the {} system prompt is {} characters, over its {}",
                seat.as_str(),
                text.len(),
                system_cap(seat)
            );
        }

        let mut kinds = 0;
        for kind in PromptKind::ALL {
            let text = default_prompt_text(kind);
            kinds += text.len();
            assert!(
                text.len() <= cap(kind),
                "the {} template is {} characters, over its {}",
                kind.as_str(),
                text.len(),
                cap(kind)
            );
        }
        assert!(
            kinds <= KIND_TOTAL,
            "the briefing templates total {kinds} characters, over {KIND_TOTAL}"
        );

        let mut pull_request = 0;
        for (name, text) in pull_request_texts() {
            pull_request += text.len();
            assert!(
                text.len() <= 400,
                "the {name} is {} characters, over its 400",
                text.len()
            );
        }
        assert!(
            pull_request <= 700,
            "the pull request texts total {pull_request} characters, over 700"
        );

        let grand: usize = all_defaults().iter().map(|(_, text)| text.len()).sum();
        println!("{kinds:5}  every briefing template\n{grand:5}  every default text");
        assert!(
            grand <= GRAND_TOTAL,
            "the defaults total {grand} characters, over {GRAND_TOTAL}"
        );
    }

    /// How the two rules are read off a text: a line at a time, the stop
    /// after a digit left where it is, and a banned word caught only where
    /// it stands as a word of its own.
    #[test]
    fn the_ste_rules_are_read_off_a_text_line_by_line() {
        assert_eq!(
            ste::sentences("# Head\n1. Run it. Then stop.\n- a bullet"),
            ["# Head", "1. Run it.", "Then stop.", "- a bullet"]
        );
        // A file name and the number a step opens on end no sentence; a
        // number at the end of a sentence still ends it.
        assert_eq!(
            ste::sentences("Read `AGENTS.md` first. Back to step 1. Then push."),
            ["Read `AGENTS.md` first.", "Back to step 1.", "Then push."]
        );

        // A sentence a wrap cut in two is read whole again, a list item
        // stays its own, and a fenced example is left out.
        assert_eq!(
            ste::unwrapped_lines(
                "1. Squash the branch with this:\n   `git commit`.\n2. Push.\n```\nfeat: x\n```"
            ),
            "1. Squash the branch with this: `git commit`.\n2. Push.\n\n"
        );
        // A closing quote ends with the sentence it closes.
        assert_eq!(
            ste::sentences(r#"- "It is done." -> Prove it."#),
            [r#"- "It is done.""#, "-> Prove it."]
        );

        assert_eq!(ste::banned_word("Ensure the tests pass"), Some("ensure"));
        assert_eq!(
            ste::banned_word("Rebase prior to the push"),
            Some("prior to")
        );
        // And a word that only holds one is not one.
        assert_eq!(ste::banned_word("The mayor of the branch"), None);
    }

    /// Every default text is Simplified Technical English, in the two rules
    /// of it a test can read off the text: no sentence runs past
    /// [`ste::MAX_WORDS`], and no sentence uses a word of [`ste::BANNED`].
    /// The shipped skills and the shipped workflow documents are read beside
    /// the defaults: an agent reads every one of them.
    ///
    /// The rules a test cannot read — one instruction to a sentence, the
    /// imperative, the active voice — are what the texts above are written
    /// in, and what a rewrite of one is read against.
    #[test]
    fn every_default_text_is_simplified_technical_english() {
        let skills = all_skills()
            .into_iter()
            .map(|(name, text)| (name, ste::unwrapped_lines(text)));
        for (name, text) in all_defaults()
            .into_iter()
            .chain(all_workflows())
            .map(|(name, text)| (name, text.to_string()))
            .chain(skills)
        {
            for sentence in ste::sentences(&text) {
                let words = sentence.split_whitespace().count();
                assert!(
                    words <= ste::MAX_WORDS,
                    "the {name} runs a sentence of {words} words, over {}: {sentence}",
                    ste::MAX_WORDS
                );
            }
            assert_eq!(
                ste::banned_word(&text),
                None,
                "the {name} uses a word STE has no room for"
            );
        }
    }

    /// How Ariadne is reached is the MCP server's `instructions` to say, and
    /// only its: the block that used to be pasted into every system prompt
    /// lives in one place now, and no prompt here repeats it. The skills are
    /// read beside the defaults, since a skill that restated a session rule
    /// would be a second owner of it.
    #[test]
    fn no_default_repeats_what_every_session_is_told_by_the_mcp_server() {
        for (name, text) in all_defaults().into_iter().chain(all_skills()) {
            for shared in [
                "Reach Ariadne",
                "backticked",
                "Work alone",
                "narrate progress",
                "as few turns as you can",
                "ASD-STE100",
                "before you repeat a discovery",
            ] {
                assert!(
                    !text.contains(shared),
                    "the {name} repeats \"{shared}\", which the MCP instructions state"
                );
            }
        }
    }

    /// A rule that holds for one seat is stated in that seat's prompt, and
    /// only there: the two step calls are the agent's, the plan is the
    /// orchestrator's, and the request is the pull request reviewer's.
    #[test]
    fn a_seat_rule_is_stated_in_its_own_prompt_alone() {
        for (owner, rule) in [
            (Seat::Agent, "`complete_step`"),
            (Seat::Agent, "`fail_step`"),
            (Seat::Orchestrator, "Never write code"),
            (Seat::Reviewer, "Commit nothing and push nothing"),
        ] {
            for seat in Seat::ALL {
                let prompt = default_system_prompt(seat);
                assert_eq!(
                    prompt.matches(rule).count(),
                    usize::from(seat == owner),
                    "the {} prompt and \"{rule}\"",
                    seat.as_str()
                );
            }
        }
    }

    /// The one task seat text names what the three calls that move a task
    /// are for, and the hook rule: the rules no column's skill is allowed to
    /// take away. When a turn ends is the calls' own descriptions to say.
    #[test]
    fn the_agent_seat_text_moves_the_task_with_the_two_step_calls() {
        let agent = default_system_prompt(Seat::Agent);
        for rule in [
            "Work only in the task's worktree, on its branch.",
            "Work on your current column alone.",
            "Call `complete_step` when the column's work is done.",
            "Call `fail_step` when the previous column must fix something.",
            "Call `fail_task` when nobody can do the task as written.",
            "Never pass `--no-verify` or `-n` to git.",
        ] {
            assert!(agent.contains(rule), "the agent seat text and \"{rule}\"");
        }
        for gone in ["End your turn", "Do not poll", "`wait_for_user`"] {
            assert!(!agent.contains(gone), "the agent seat text and \"{gone}\"");
        }
        // No retired call is briefed anywhere.
        for (name, text) in all_defaults().into_iter().chain(all_skills()) {
            for gone in [
                "`request_review`",
                "`submit_verdict`",
                "`pick_winner`",
                "`finish_task`",
            ] {
                assert!(!text.contains(gone), "the {name} names {gone}");
            }
        }
    }

    /// Every text this ticket gave word for word is the text that ships: the
    /// two seat texts, the last line of a step briefing, an incoming message,
    /// the end of a goal attention and the switch notice (006).
    #[test]
    fn the_agreed_texts_ship_word_for_word() {
        assert_eq!(
            default_system_prompt(Seat::Orchestrator),
            "Never write code. Plan the goal with the user. Decide no open point alone."
        );
        assert_eq!(
            default_system_prompt(Seat::Agent),
            "Work only in the task's worktree, on its branch. Work on your current column alone. Read its skills first.\n\
             - Call `complete_step` when the column's work is done.\n\
             - Call `fail_step` when the previous column must fix something.\n\
             - Call `fail_task` when nobody can do the task as written.\n\
             - Never pass `--no-verify` or `-n` to git. A refused hook names the fix. Make it and commit again."
        );
        assert!(
            default_prompt_text(PromptKind::StepBriefing)
                .ends_with("Dependencies: {dependencies}\nDo this column now.")
        );
        assert_eq!(
            default_prompt_text(PromptKind::IncomingMessage),
            "Message from {from}:\n\n{body}\n\nAnswer only a question{answer_to}. Then go on with your work."
        );
        assert!(default_prompt_text(PromptKind::GoalAttention).ends_with(
            "Read them with `list_tasks`. Retry, rewrite or cancel each one. Call `complete_goal` when every task is done."
        ));
        assert_eq!(
            exhausted_switch_notice(),
            "The {column} agent of \"{task_title}\" moved from {old} to {new}. The old model ran out. Tell the user."
        );
    }

    /// The review column starts scoped checks before it reads the diff and
    /// task, then ends on one of the two step calls: a must-fix finding
    /// fails the step, and an optional one never does.
    #[test]
    fn the_review_skill_starts_its_checks_before_the_read_and_ends_on_a_step_call() {
        let text = unwrapped(default_skill_document("code-review").unwrap());
        let checks = text
            .find("Start the checks of what the diff changed, once for this verdict.")
            .expect("the code-review skill does not start scoped checks once");
        let read = text
            .find("Read the task")
            .expect("the code-review skill does not read the task");
        assert!(
            checks < read,
            "the code-review skill reads before it starts the checks"
        );
        assert_eq!(
            text.matches("once for this verdict").count(),
            1,
            "the code-review skill does not bind one check run to one verdict"
        );
        for rule in [
            "`git checkout --detach <branch>` in the task worktree",
            "the SHA you judged, from `git rev-parse HEAD`",
            "On any must-fix finding, call `fail_step` with the verdict.",
            "Else call `complete_step` with the verdict, and list the optional findings in it.",
            "An optional finding never fails the step.",
            "A must-fix finding breaks a criterion or a written repository rule.",
            "Judge each test by reading its setup, action and assertions",
            "Never change code to see whether a test fails",
            "Edit no file",
            "`git merge-base --is-ancestor <sha> HEAD`",
            "If HEAD is not after that SHA, use `get_diff`",
        ] {
            assert!(text.contains(rule), "the code-review skill and {rule}");
        }
        assert!(
            !text.contains("Complete the step only when both axes pass."),
            "an optional finding fails no step"
        );
        assert!(
            !text.contains("`read_messages`"),
            "a second review reads the judged SHA from the conversation, not the channel"
        );
    }

    /// A skill scopes its checks to what its step changed.
    #[test]
    fn a_skill_scopes_its_own_checks_to_what_the_task_changed() {
        for (name, step) in [
            ("coding", "Run the checks of what you changed."),
            ("code-review", "Start the checks of what the diff changed"),
            ("pr-babysit", "run the checks of what you changed"),
            ("coding", "Run the one test, never the suite around it."),
            ("debugging", "The tests of the crate you changed are green."),
            ("debugging", "Run the checks of what you changed."),
            (
                "conflict-resolution",
                "Run the tests and the lint of the files you resolved.",
            ),
        ] {
            let document = unwrapped(default_skill_document(name).unwrap());
            assert!(
                document.contains(step),
                "the {name} skill does not say \"{step}\""
            );
        }
        for (name, document) in all_skills() {
            for phrase in ["whole suite", "full suite", "Start the whole test suite"] {
                assert!(
                    !document.contains(phrase),
                    "the {name} skill contains {phrase}"
                );
            }
        }
    }

    /// A task is built as one commit, and a return from review adds one on
    /// top: the merge column squashes the branch onto the base.
    ///
    /// A task is one responsibility: the orchestrator cuts it as one tracer.
    /// So a branch divided into parts buys nothing and costs the review a
    /// diff of half-built commits. No skill sends an agent to a slice or to
    /// a small commit. The amend rule is the two committing skills' alone:
    /// `coding`, whose answer to a review is a new commit, and `pr-babysit`,
    /// whose pushed request an amend would rewrite under its readers.
    #[test]
    fn a_task_is_one_commit_and_nothing_amends_a_pushed_one() {
        for (name, document) in all_skills() {
            let document = unwrapped(document).to_lowercase();
            for divided in ["slice", "small commit"] {
                assert!(
                    !document.contains(divided),
                    "the {name} divides a task into a {divided}"
                );
            }
            if name == format!("{PR_BABYSIT_SKILL} skill") || name == "coding skill" {
                continue;
            }
            assert!(
                !document.contains("amend"),
                "the {name} repeats the rule about an amend"
            );
        }

        for (name, step) in [
            (
                "coding",
                "On a return from review, add a new commit on top. Never amend. The merge column squashes.",
            ),
            (
                "refactoring",
                "Commit the whole refactor once, after every move is green.",
            ),
            ("pr-babysit", "Never amend, rebase or force a push."),
        ] {
            let document = unwrapped(default_skill_document(name).unwrap());
            assert!(
                document.contains(step),
                "the {name} skill does not say \"{step}\""
            );
        }
    }

    /// A squash never removes the work of a landing that came in between.
    ///
    /// A worktree shares its refs with the repository, so `<base>` is a name
    /// that moves while the whole suite runs. Squashed onto that name, the
    /// commit sits on the new tip with the tree of the old rebase, and takes
    /// back what landed in between; the fast-forward then succeeds, because
    /// the new tip is its parent. This runs the merge skill's own squash and
    /// guard commands on a throwaway repository, with no remote, for two
    /// tasks rebased on the same base: the first lands, then the second
    /// squashes.
    #[test]
    fn a_late_squash_keeps_what_another_landing_put_on_the_base_branch() {
        use std::path::Path;
        use std::process::Command;

        let merge = default_skill_document("merge").unwrap();
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        let editor = dir.path().join("editor.sh");
        std::fs::write(&editor, "#!/bin/sh\necho 'feat: land a task' > \"$1\"\n").unwrap();
        let sh = |cwd: &Path, script: &str| {
            Command::new("sh")
                .args(["-c", script])
                .current_dir(cwd)
                .env("GIT_EDITOR", format!("sh {}", editor.display()))
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@t")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@t")
                .output()
                .unwrap()
        };
        let run = |cwd: &Path, script: &str| {
            let out = sh(cwd, script);
            assert!(out.status.success(), "{script}: {out:?}");
            String::from_utf8(out.stdout).unwrap()
        };
        // The backticked command of the skill that holds `needle`, rendered
        // for the base branch `main`.
        let step = |needle: &str| {
            let command = merge
                .split('`')
                .skip(1)
                .step_by(2)
                .find(|command| command.contains(needle))
                .unwrap_or_else(|| panic!("the merge skill has no {needle}: {merge}"));
            command
                .replace("<upstream>", "main")
                .replace("<base>", "main")
        };

        std::fs::create_dir(&repo).unwrap();
        run(
            &repo,
            "git init -q -b main && echo base > base.txt && git add . && git commit -qm base",
        );
        for task in ["first", "second"] {
            run(
                &repo,
                &format!(
                    "git worktree add -q -b {task} ../{task} && cd ../{task} && echo {task} > {task}.txt && git add . && git commit -qm {task}"
                ),
            );
            run(&dir.path().join(task), "git rebase main");
        }

        // The first lands while the second runs its suite.
        let first = dir.path().join("first");
        run(&first, &step("reset --soft"));
        run(&repo, "git merge --ff-only first");

        let second = dir.path().join("second");
        run(&second, &step("reset --soft"));
        let landed = sh(&repo, "git merge --ff-only second");
        let tree = run(&repo, "git ls-tree --name-only main");
        if landed.status.success() {
            assert!(
                tree.contains("first.txt"),
                "the second landing removed the first: {tree}"
            );
            return;
        }

        // Refused, and the guard names why: the base moved under the squash.
        assert!(!tree.contains("second.txt"), "{tree}");
        let guard = run(&second, &step("git diff --stat"));
        assert!(guard.contains("first.txt"), "{guard}");

        // So the second goes round again from its rebase, and lands both.
        run(&second, "git rebase main");
        run(&second, &step("reset --soft"));
        let guard = run(&second, &step("git diff --stat"));
        assert!(!guard.contains("first.txt"), "{guard}");
        run(&repo, "git merge --ff-only second");
        let tree = run(&repo, "git ls-tree --name-only main");
        assert!(
            tree.contains("first.txt") && tree.contains("second.txt"),
            "{tree}"
        );
    }

    /// The merge column's skill runs its steps in order — fetch, rebase,
    /// squash, guard, push, fast-forward, the step call — and
    /// a human merges every request: no skill names the command that would
    /// let an agent merge one itself, on either forge.
    #[test]
    fn the_merge_skill_lands_in_order_and_no_skill_merges_a_request_itself() {
        let text = unwrapped(default_skill_document("merge").unwrap());
        let mut at = 0;
        for step in [
            "`<upstream>` is `<remote>/<base>` where `git remote -v` names a remote, else `<base>`.",
            "Where a remote exists, run `git fetch <remote> <base>`.",
            "Rebase the task branch onto `<upstream>`.",
            "git reset --soft \"$(git merge-base <upstream> HEAD)\" && git commit",
            "git diff --stat <upstream> HEAD",
            "push `HEAD:<base>` to `<remote>`",
            "git -C <repo> merge --ff-only <remote>/<base>",
            "else `git -C <repo> fetch <remote> <base>:<base>`",
            "With no remote: run the checks the repository names for a landing.",
            "Call `complete_step` with the sha of `<base>` as `merge_commit`.",
        ] {
            let found = text[at..]
                .find(step)
                .unwrap_or_else(|| panic!("the merge skill has no {step} after its last step"));
            at += found + step.len();
        }
        assert!(
            text.contains("A refused push or a failed check is a fix on the task branch."),
            "the merge skill does not name a refused push as a task-branch fix"
        );
        assert!(
            text.contains("The hooks run the checks."),
            "the merge skill does not leave the push checks to the repository hooks"
        );
        assert!(
            text.contains("The primary checkout is the `Repo:` path of your briefing."),
            "the merge skill does not name the primary checkout"
        );
        for (name, text) in all_defaults().into_iter().chain(all_skills()) {
            for command in ["gh pr merge", "glab mr merge"] {
                assert!(!text.contains(command), "the {name} names {command}");
            }
        }
    }

    /// The `pr` column's agent is fed by the daemon (026, 030), so its skill
    /// names none of the ways an agent would feed itself: no forge CLI, no
    /// timer, no poll. A human closes a thread and a human merges, so it
    /// names neither the call that resolves one nor a forge merge command.
    /// The agent opens the request, filling the repository's own template
    /// or, with none, Why, What changed and How to test, pushes each tested
    /// fix, completes its step on the merge and fails the task on the
    /// close. And the turn ends when the news is handled, which is what
    /// lets the next news start a turn of its own.
    #[test]
    fn the_pr_babysit_skill_opens_the_request_and_ends_the_step_on_the_merge() {
        let doc = default_skill_document(PR_BABYSIT_SKILL).expect("the pr-babysit skill");
        let words: Vec<String> = doc
            .split(|c: char| !c.is_ascii_alphanumeric())
            .map(str::to_lowercase)
            .collect();
        for gone in [
            "gh", "glab", "sleep", "poll", "polling", "resolve", "resolved",
        ] {
            assert!(
                !words.iter().any(|word| word == gone),
                "the pr-babysit skill names {gone}"
            );
        }
        for command in ["gh pr merge", "glab mr merge", "pr merge", "mr merge"] {
            assert!(
                !doc.contains(command),
                "the pr-babysit skill names {command}"
            );
        }
        let doc = unwrapped(doc);
        for step in [
            "On your first turn, do step 1.",
            "On each later turn, Ariadne sends news: do steps 2 to 7. Then end your turn.",
            "`get_pull_request`",
            "`list_comments` with `unanswered_only`",
            "`git merge --no-edit <remote>/<base>`",
            "Push the task branch. Call `open_pull_request`.",
            "repository's commit conventions",
            "Fill its pull request template, or write three full sections: Why, What changed, How to test.",
            "run the checks of what you changed, and push;",
            "Never amend, rebase or force a push.",
            "Call `report_pull_request` only when readiness changes:",
            "`ready: true` with the `head_sha`",
            "`ready: false` when one goes back",
            "Call `complete_step` with the merge as the reason.",
            "call `fail_task`",
            "## Done",
        ] {
            assert!(doc.contains(step), "the pr-babysit skill has no {step}");
        }
        assert_eq!(SkillSeat::of(PR_BABYSIT_SKILL), SkillSeat::PullRequest);
    }

    /// The keeping agent replies only where a thread asks for a change. Its
    /// reply explains either the new commit or why the agent kept the code.
    /// It adds no unrelated forge discussion or code change.
    #[test]
    fn the_pr_babysit_skill_replies_only_to_requested_changes() {
        let doc = unwrapped(default_skill_document(PR_BABYSIT_SKILL).unwrap());
        for rule in [
            "reply once in the thread with the change and the commit.",
            "Or reply once in the thread with why you do not make it.",
            "Post only one reply in a thread that asks for a change.",
            "Post no new thread, summary, top-level comment or thanks.",
            "Commit only what a comment or a failed check asks for.",
            "Leave every thread open.",
        ] {
            assert!(doc.contains(rule), "the pr-babysit skill has no {rule}");
        }
    }

    /// The reviewer session is fed by the daemon too (029), and the user
    /// gives every approval and lands every request: the skill never
    /// approves on its own, and names no merge, sleep, poll or forge CLI.
    /// It names the three priorities, asks for changes only where a P0
    /// stands, and resolves a thread of its own once a push fixed it.
    #[test]
    fn the_pr_reviewer_skill_ranks_its_findings_and_never_approves() {
        let doc = default_skill_document(PR_REVIEWER_SKILL).expect("the pr-reviewer skill");
        let words: Vec<String> = doc
            .split(|c: char| !c.is_ascii_alphanumeric())
            .map(str::to_lowercase)
            .collect();
        for gone in ["merge", "sleep", "poll", "polling", "gh", "glab"] {
            assert!(
                !words.iter().any(|word| word == gone),
                "the pr-reviewer skill names {gone}"
            );
        }
        // Nothing in the skill names the agent's own approval.
        assert_eq!(
            words.iter().filter(|word| *word == "approve").count(),
            0,
            "the pr-reviewer skill approves on its own"
        );
        let doc = unwrapped(doc);
        for priority in ["P0: ", "P1: ", "P2: "] {
            assert!(
                doc.contains(priority),
                "the pr-reviewer skill has no {priority}"
            );
        }
        for sentence in ste::sentences(&doc) {
            if sentence.contains("`request_changes`") {
                assert!(
                    sentence.contains("P0"),
                    "`request_changes` without a P0: {sentence}"
                );
            }
        }
        for step in [
            "`get_pull_request`",
            "`get_diff`",
            "`since`",
            "`submit_review` once",
            "`comment`",
            "`report_pull_request` with `reviewed_sha`",
            "`reply_comment` once",
            "Run the checks once.",
            "On a later round, call `list_comments` first.",
            "Settle each thread you opened before you hunt new defects.",
            // The summary is one comment, rewritten whole every round as a
            // title, a header of the reviewed commits, a prose verdict on
            // the change and its risk, and a recommendation; it is nothing
            // of the work, and each finding sits on its own line of code.
            "Ariadne keeps one summary comment",
            "## Review summary",
            "Header: one line per commit the round reviewed",
            "Summary: one short paragraph of prose, not bullets",
            "Recommendation: one line",
            "\"Request changes\"",
            "\"Changes recommended\"",
            "\"No findings: this PR is ready to be merged.\"",
            "Name no file or line, and nothing of what you did",
            "only the new comments",
            // A thread nobody answered waits; an answered one is replied to
            // once, and resolved where it is fixed.
            "not answered: post nothing in it. Wait for an answer.",
            "Never post a second comment for a defect",
            "then `resolve_thread` on",
            "Resolve only a thread you opened",
            "one inline comment on the line of the defect",
            "## Do not tell yourself",
            "## Done",
        ] {
            assert!(doc.contains(step), "the pr-reviewer skill has no {step}");
        }
        assert_eq!(SkillSeat::of(PR_REVIEWER_SKILL), SkillSeat::PullRequest);
    }

    /// The four parts of the summary (029 rule 17) stand in order, and
    /// `get_pull_request` and `get_diff` answer no commit list: the header
    /// names its own source, local history, so a request of several commits
    /// still gets one line per commit.
    #[test]
    fn the_pr_reviewer_skill_writes_its_summary_as_a_header_then_prose_then_a_recommendation() {
        let doc =
            unwrapped(default_skill_document(PR_REVIEWER_SKILL).expect("the pr-reviewer skill"));
        let title_at = doc
            .find("## Review summary")
            .expect("the skill has no summary title");
        let header_at = doc.find("Header:").expect("the skill has no Header:");
        let summary_at = doc.find("Summary:").expect("the skill has no Summary:");
        let recommendation_at = doc
            .find("Recommendation:")
            .expect("the skill has no Recommendation:");
        assert!(
            title_at < header_at && header_at < summary_at && summary_at < recommendation_at,
            "the parts are not Title, then Header, then Summary, then Recommendation"
        );
        let header = &doc[header_at..summary_at];
        for phrase in [
            "read with `git log`",
            "short sha",
            "subject",
            "`git log <base>..HEAD` on the first round",
            "`git log <reviewed_sha>..HEAD` later",
            "End the header with one line: `<base>..<head>` as short shas.",
        ] {
            assert!(header.contains(phrase), "the header lacks {phrase}");
        }
    }

    /// The prose summary names what the change does, where its risk sits
    /// and how serious it is, and names every open finding in that prose
    /// rather than as a bullet (029 rule 17).
    #[test]
    fn the_pr_reviewer_skill_summarizes_the_change_and_its_risk_in_prose() {
        let doc =
            unwrapped(default_skill_document(PR_REVIEWER_SKILL).expect("the pr-reviewer skill"));
        let summary_at = doc.find("Summary:").expect("the skill has no Summary:");
        let recommendation_at = doc
            .find("Recommendation:")
            .expect("the skill has no Recommendation:");
        let summary = &doc[summary_at..recommendation_at];
        for phrase in [
            "one short paragraph of prose, not bullets",
            "where its risk sits",
            "how serious it is",
            "Name each",
            "priority and title in the prose",
        ] {
            assert!(summary.contains(phrase), "the summary lacks {phrase}");
        }
    }

    /// Each recommendation line is tied to the priority that earns it (029
    /// rule 17): a P0 blocks the request, a P1 or P2 with no P0 open asks
    /// for a fix before it lands, and nothing open says the request is
    /// ready to merge.
    #[test]
    fn the_pr_reviewer_skill_recommends_by_the_open_findings_priority() {
        let doc =
            unwrapped(default_skill_document(PR_REVIEWER_SKILL).expect("the pr-reviewer skill"));
        let recommendation_at = doc
            .find("Recommendation:")
            .expect("the skill has no Recommendation:");
        let recommendation = &doc[recommendation_at..];
        for phrase in [
            "A P0 is open: say \"Request changes\", and name the P0 findings that block it",
            "A P1 or P2 is open, with no P0: say \"Changes recommended\". Name what to fix before it lands",
            "Nothing is open: say \"No findings: this PR is ready to be merged.\"",
        ] {
            assert!(
                recommendation.contains(phrase),
                "the recommendation lacks {phrase}"
            );
        }
    }

    /// Every rule an agent is briefed with is written down once.
    ///
    /// The briefings are one prompt system — a nudge, a resume and a wake
    /// instruction are templates like the briefings that start a session, and
    /// the shipped skills are read beside them — and the way that stays
    /// readable is that each rule lives in the layer that needs it. A rule
    /// restated in a second one is a rule that goes stale in one of them.
    #[test]
    fn each_rule_is_stated_in_exactly_one_briefing() {
        // The step calls and hook rule belong to the agent seat text.
        // `finalize_plan` belongs to the orchestration skill.
        for marker in [
            "Call `complete_step` when",
            "Call `fail_step` when",
            "Never pass `--no-verify` or `-n` to git",
            "Call it no earlier.",
        ] {
            let places = all_defaults()
                .into_iter()
                .chain(all_skills())
                .filter(|(_, text)| text.contains(marker))
                .map(|(name, _)| name)
                .collect::<std::collections::BTreeSet<_>>();
            assert_eq!(
                places.len(),
                1,
                "\"{marker}\" is stated in {places:?}, not in one place"
            );
        }
    }

    /// The constants are the templates every session runs on, so the
    /// placeholders they name are the ones the assemblers fill in: a token
    /// nothing substitutes would reach an agent as literal text.
    #[test]
    fn every_default_names_only_placeholders_its_kind_can_fill_in() {
        for kind in PromptKind::ALL {
            assert_eq!(
                kind.validate_template(default_prompt_text(kind)),
                Ok(()),
                "the default {} template",
                kind.as_str()
            );
        }
    }

    /// The orchestrator playbook is a conversation, and the order of its
    /// phases is the playbook: the goal is read with its workflow, every
    /// unclear point is asked about one question at a time, the goal is
    /// split into tasks, each task is staffed one agent per column, each
    /// agent is sized on its column's rank, and only an explicit yes starts
    /// any of it.
    ///
    /// Staffing stands before the yes and the start stands after it, which is
    /// the whole of the arrangement: the user agrees to tasks that already
    /// exist — a task created while the goal is in `planning` runs nothing —
    /// and agreeing is what starts them
    /// (`plan_finalize.rs::the_tasks_of_a_plan_wait_for_the_yes_that_finalizes_it`).
    ///
    /// The phases of the playbook, in the order the conversation runs them.
    /// Named once, because two assertions read them: the skill document holds
    /// all of them in this order, and the seat text holds none.
    const PLAYBOOK_PHASES: [&str; 11] = [
        "Read the goal and its columns.",
        "Ask the user every open point",
        "Split the goal into tracer tasks.",
        "Write each ticket in STE",
        "Staff one agent on every column with `create_task`.",
        "Give each agent one model from `list_models`, at the column's rank.",
        "Show the user the tasks and the models.",
        "Revise until the user writes an explicit yes.",
        "Call `finalize_plan`.",
        "Stay up until the goal ends.",
        "Call `complete_goal` when every task is done.",
    ];

    /// The playbook is the `orchestration` skill, so the document read here
    /// is the shipped skill rather than the seat text, which carries no step
    /// of it. A phase out of order is an orchestrator that staffs a task
    /// before it knows the columns, or that starts a plan the user has not
    /// seen.
    #[test]
    fn the_orchestrator_playbook_asks_before_it_plans_and_plans_before_it_starts() {
        let prompt = default_skill_document(ORCHESTRATION_SKILL).unwrap();
        let mut at = 0;
        for phase in PLAYBOOK_PHASES {
            let found = prompt[at..]
                .find(phase)
                .unwrap_or_else(|| panic!("the orchestrator prompt has no \"{phase}\" after {at}"));
            at += found + phase.len();
        }

        // And the yes gates the start, in as many words.
        assert!(prompt.contains("Call it no earlier."), "{prompt}");
    }

    /// The workflow is settled before planning starts, not asked about task
    /// by task: the orchestrator reads its columns off its briefing, staffs
    /// one agent per column, and asks the user nothing a column already
    /// settles. The reviewer question and the final task are gone with the
    /// pipeline they belonged to.
    #[test]
    fn the_orchestration_skill_staffs_one_agent_per_column_and_asks_no_reviewer_question() {
        let prompt = unwrapped(default_skill_document(ORCHESTRATION_SKILL).unwrap());
        for gone in [
            "Ask the user which tasks to leave unreviewed",
            "Staff one reviewer on every task",
            "Leave a task unreviewed",
            "final task",
            "feature_branch",
            "author",
            "reviewer",
        ] {
            assert!(
                !prompt.contains(gone),
                "the orchestration skill still says \"{gone}\": {prompt}"
            );
        }
        for phrase in [
            "The workflow columns in your briefing are settled: do not ask for them.",
            "Ask nothing a column settles, such as review or landing.",
            "Staff one agent on every column with `create_task`.",
            "or none for the column's own.",
            "\"This task needs no review.\" -> The workflow decides; staff its column.",
        ] {
            assert!(
                prompt.contains(phrase),
                "the orchestration skill and \"{phrase}\": {prompt}"
            );
        }
    }

    /// A false `depends_on` serializes two tasks that could run together, so
    /// the skill tells the orchestrator to write a shared interface into
    /// both tickets instead. And the last column already proved the work, so
    /// the orchestrator runs no checks of its own before `complete_goal`.
    #[test]
    fn the_orchestration_skill_names_the_contract_rule_and_the_no_checks_rule() {
        let prompt = unwrapped(default_skill_document(ORCHESTRATION_SKILL).unwrap());
        for phrase in [
            "Write each contract between two tasks into both tickets: a route, a response shape, a function, a file.",
            "Add `depends_on` only where the code cannot compile or run without the other task.",
            "Run no checks yourself: the last column proved the work.",
            "\"The frontend waits for the backend to land.\" -> Write the contract into both tickets. Both run now.",
        ] {
            assert!(
                prompt.contains(phrase),
                "the orchestration skill and \"{phrase}\": {prompt}"
            );
        }
    }

    /// The seat text carries no playbook step: not one of the phases, and no
    /// numbered step at all — a step that crept back in would be a rule
    /// stated in two layers, and the skill's copy going stale under it.
    #[test]
    fn the_orchestrator_seat_text_holds_no_playbook_step() {
        let seat = default_system_prompt(Seat::Orchestrator);
        for phase in PLAYBOOK_PHASES {
            assert!(
                !seat.contains(phase),
                "the seat text carries \"{phase}\", which the playbook's"
            );
        }
        assert!(
            !seat.contains('\n') && !seat.contains("1."),
            "the seat text is one line with no step in it: {seat}"
        );
    }

    /// The orchestrator staffs each column on the rank the column prefers,
    /// and where a column names none, the lowest rank and the lowest effort
    /// that finish the work.
    ///
    /// A model description says what a model can do and nothing about what it
    /// costs, so an orchestrator sized on the description alone staffed a
    /// frontier model on a one-line fix. A workflow column carries the rank
    /// its work earns. `local` is off the ladder: a local model runs on the
    /// user's own machine, so it is a choice the user makes rather than a
    /// cheaper rung.
    #[test]
    fn the_orchestrator_staffs_each_column_on_its_rank() {
        // Read on one line, so where the document wraps holds nothing.
        let prompt = unwrapped(default_skill_document(ORCHESTRATION_SKILL).unwrap());
        for sentence in [
            "Give each agent one model from `list_models`, at the column's rank.",
            "Where a column names no rank, take the lowest rank and effort that finish the work.",
            "Staff `local` only where the user names it.",
            "State a reason for each step up.",
        ] {
            assert!(
                prompt.contains(sentence),
                "the skill and the staffing rule \"{sentence}\""
            );
        }
    }

    /// An orchestrator is nudged in the situation its goal stands in, and
    /// there are two: a plan being agreed, and a goal under way. A resume
    /// that asked for `finalize_plan` alone would push an orchestrator past a
    /// plan the user never saw; one that named the conversation alone would
    /// leave a running goal unattended.
    #[test]
    fn the_orchestrator_nudge_fits_the_conversation_and_the_goal_under_way() {
        let resume = default_prompt_text(PromptKind::OrchestratorResume);
        for phase in [
            "Without an explicit yes on the plan, stay in the conversation that gets one",
            "With one, call `finalize_plan`",
            "Once the goal is under way, read `list_tasks`",
        ] {
            assert!(
                resume.contains(phase),
                "the orchestrator resume and \"{phase}\""
            );
        }
    }

    /// A woken orchestrator is told its three choices for each task, and
    /// what ends the goal.
    #[test]
    fn the_goal_attention_names_the_three_choices_and_the_end() {
        let text = default_prompt_text(PromptKind::GoalAttention);
        for rule in [
            "Read them with `list_tasks`.",
            "Retry, rewrite or cancel each one.",
            "Call `complete_goal` when every task is done.",
        ] {
            assert!(text.contains(rule), "the goal attention and \"{rule}\"");
        }
    }

    /// `finalize_plan` is what the orchestrator calls once the plan is
    /// written, and it is the only call there is about a plan: the playbook
    /// and the resume both name it, and a text naming any other would be
    /// briefing an agent to make a call the daemon does not answer.
    #[test]
    fn the_orchestrator_is_briefed_with_finalize_plan_and_no_other_plan_call() {
        for text in [
            default_skill_document(ORCHESTRATION_SKILL).unwrap(),
            default_prompt_text(PromptKind::OrchestratorResume),
        ] {
            assert!(text.contains("`finalize_plan`"), "{text}");
        }
        for (name, text) in all_defaults().into_iter().chain(all_skills()) {
            // The backticked names are every other span of a default: what
            // the odd ones hold is what an agent is told to call.
            for call in text
                .split('`')
                .skip(1)
                .step_by(2)
                .filter(|call| call.ends_with("_plan"))
            {
                assert_eq!(call, "finalize_plan", "the {name} names `{call}`");
            }
        }
    }

    /// Step 9 of the playbook is the orchestrator's answer to an agent that
    /// is running a task into the ground: switch its session to a model the
    /// ladder gives, and tell the user. Without it, a struggling agent was an
    /// orchestrator's to notice and nobody's to act on.
    #[test]
    fn the_orchestration_skill_switches_a_struggling_agents_session() {
        let prompt = unwrapped(default_skill_document(ORCHESTRATION_SKILL).unwrap());
        for phrase in [
            "When an agent is exhausted, stuck or unsuitable, call `switch_session` with a model one rank up.",
            "Tell the user.",
        ] {
            assert!(
                prompt.contains(phrase),
                "the orchestration skill and \"{phrase}\": {prompt}"
            );
        }
    }

    /// The orchestrator's own texts name no forge: which way a task lands is
    /// the workflow's `pr` or `merge` column to say, and the procedure
    /// reaches that column's agent as its skill. A playbook that spelled out
    /// one of the two landings would be a second copy of those details,
    /// going stale on its own. The orchestration skill is one of its texts,
    /// so it is read here too.
    #[test]
    fn the_orchestrator_is_told_nothing_of_forges() {
        let orchestrator = std::iter::once(default_system_prompt(Seat::Orchestrator))
            .chain(std::iter::once(
                default_skill_document(ORCHESTRATION_SKILL).unwrap(),
            ))
            .chain(
                PromptKind::for_seat(Seat::Orchestrator)
                    .iter()
                    .map(|kind| default_prompt_text(*kind)),
            )
            .collect::<Vec<_>>()
            .join("\n");
        for forge in [
            "GitHub",
            "github",
            "GitLab",
            "gitlab",
            "`gh`",
            "gh pr",
            "`glab`",
            "glab mr",
            "pull request",
            "merge request",
        ] {
            assert!(
                !orchestrator.contains(forge),
                "the orchestrator prompts name {forge}"
            );
        }
    }

    /// The orchestrator briefing names the workflow and its columns where it
    /// used to name a landing, and nothing of the pipeline the columns took
    /// over.
    #[test]
    fn the_orchestrator_briefing_names_the_workflow_and_its_columns() {
        let briefing = default_prompt_text(PromptKind::OrchestratorBriefing);
        assert!(
            briefing.contains("Workflow: {workflow}\n{columns}"),
            "{briefing}"
        );
        for gone in ["landing", "Landing", "author", "reviewer"] {
            assert!(!briefing.contains(gone), "{briefing}");
        }
    }

    /// The catalog is the whole of what an agent can be, so it is read here
    /// as a catalog: every skill is named once, its name is the one its own
    /// frontmatter gives, and every skill says in one line what it is for.
    #[test]
    fn every_shipped_skill_is_named_once_and_describes_itself() {
        let mut names: Vec<&str> = BUILTIN_SKILLS.iter().map(|s| s.name).collect();
        let listed = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), listed, "a skill is listed twice");

        for skill in &BUILTIN_SKILLS {
            assert!(
                skill
                    .name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c == '-'),
                "{} is not kebab-case",
                skill.name
            );
            let front = skill
                .document
                .strip_prefix("---\n")
                .and_then(|rest| rest.split_once("\n---"))
                .map(|(front, _)| front)
                .unwrap_or_else(|| panic!("{} has no frontmatter", skill.name));
            assert!(
                front.contains(&format!("name: {}", skill.name)),
                "{} names itself something else in its frontmatter",
                skill.name
            );
            let summary = skill_summary(skill.document)
                .unwrap_or_else(|| panic!("{} describes itself nowhere", skill.name));
            assert!(
                !summary.is_empty() && summary.len() <= 140,
                "the {} summary is {} characters; the index carries one line",
                skill.name,
                summary.len()
            );
        }
    }

    /// The two shipped workflows are named once, each parses, and each
    /// names only skills the catalog ships: a column staged on a skill
    /// nothing ships would refuse every task of its goal.
    #[test]
    fn every_shipped_workflow_parses_and_stages_shipped_skills() {
        let mut names: Vec<&str> = BUILTIN_WORKFLOWS.iter().map(|w| w.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(
            names.len(),
            BUILTIN_WORKFLOWS.len(),
            "a workflow is listed twice"
        );
        assert!(names.contains(&DEFAULT_WORKFLOW));
        assert!(names.contains(&PULL_REQUEST_WORKFLOW));
        for workflow in &BUILTIN_WORKFLOWS {
            let parsed = ariadne_core::workflow::parse(workflow.document)
                .unwrap_or_else(|e| panic!("{} does not parse: {e}", workflow.name));
            assert_eq!(parsed.name, workflow.name);
            for step in &parsed.steps {
                for skill in &step.skills {
                    assert!(
                        default_skill_document(skill).is_some(),
                        "{} stages {skill}, which nothing ships",
                        workflow.name
                    );
                    assert_ne!(SkillSeat::of(skill), SkillSeat::Orchestrator);
                    assert_ne!(skill, PR_REVIEWER_SKILL);
                }
            }
        }
    }

    /// A background poll of a check was the largest single waste a
    /// measurement of one week's sessions found: an agent started
    /// `cargo nextest` in the background and re-sent its whole context on
    /// every no-op turn spent waiting for it. The fix — a check in the
    /// foreground, never polled, its output in a log outside the worktree —
    /// is the session rules' to state (013), which every seat reads. The
    /// skills that run checks say only which checks to run.
    #[test]
    fn the_skills_leave_how_a_check_runs_to_the_session_rules() {
        for name in ["coding", "debugging", "code-review", "pr-reviewer"] {
            let doc = unwrapped(default_skill_document(name).unwrap());
            for block in [
                "in the foreground",
                "no-op command",
                "outside the worktree",
                "tail -n 40",
                "the detail of a failure",
            ] {
                assert!(
                    !doc.contains(block),
                    "{name} repeats the check block: {block}"
                );
            }
        }
    }

    /// `coding` ends its search step on what the agent knows.
    #[test]
    fn the_coding_search_step_ends_on_what_the_agent_knows() {
        let doc = unwrapped(&skill_text(default_skill_document("coding").unwrap()));
        assert!(!doc.contains("a tool named every file"), "{doc}");
        assert!(
            doc.contains("Done when you can name each definition you change and its callers."),
            "{doc}"
        );
    }

    #[test]
    fn skill_text_returns_its_document() {
        assert_eq!(skill_text("Ask twice.\n"), "Ask twice.\n");
    }

    /// A skill is read on demand rather than on every launch, so it is capped
    /// on its own rather than against the briefings' total. The caps are still
    /// what a rewrite fits in: moving one is a decision, not a way round a
    /// failing assertion.
    ///
    /// This is that decision, taken once for the suite rather than a skill at
    /// a time. The catalog is written to one template — a description that
    /// says when to load the skill as well as what it does, a checkable bound
    /// on every step that can end early, one anchor word carried through the
    /// body, and the two or three excuses the agent talks itself into.
    ///
    /// Three tiers, by how much procedure the skill carries. `orchestration`
    /// and `debugging` get the most: one runs the ten phases of a whole goal,
    /// the other a feedback loop the agent is talked out of at every step,
    /// and both spend their length on the excuses rather than on the steps.
    /// `coding` and `code-review` get the room of the skills almost every
    /// task loads, and the two whose failure modes are worth spelling out.
    /// Every other skill gets 2400, which is a template document with room
    /// for its rules. The total is the sixteen at their tiers, so a skill
    /// that grows costs a decision here rather than a quiet raid on another
    /// skill's share.
    ///
    /// `coding` gets 5400, because `testing` merged into it. `code-review`
    /// gets 4100: the review column ends its step on one of the two step
    /// calls now, which is a step of its own. `orchestration` gets 4600: it
    /// staffs one agent per column on the column's rank, and staffs a
    /// missing column before a retry.
    #[test]
    fn skill_size_caps_hold() {
        const TOTAL: usize = 42_800;
        let cap = |name: &str| match name {
            ORCHESTRATION_SKILL => 4600,
            "debugging" => 3400,
            "code-review" => 4100,
            "coding" => 5400,
            // Seven steps, one per kind of news and the report, and the two
            // rules no other skill has: leave every thread open, and reach
            // the forge only through the session's tools.
            PR_BABYSIT_SKILL => 2900,
            // Eleven steps: the read, the checks, the hunt, the three
            // priorities, one inline comment per finding with its title and
            // its fix, the summary of four parts with no line in any of
            // them, the one review, the report and the later round, which
            // resolves each thread a push fixed.
            PR_REVIEWER_SKILL => 4700,
            _ => 2400,
        };

        let read = |skill: &BuiltinSkill| skill_text(skill.document).len();
        for skill in &BUILTIN_SKILLS {
            println!("{:5}  {}", read(skill), skill.name);
        }
        for skill in &BUILTIN_SKILLS {
            assert!(
                read(skill) <= cap(skill.name),
                "the {} skill is {} characters, over its {}",
                skill.name,
                read(skill),
                cap(skill.name)
            );
        }
        let total: usize = BUILTIN_SKILLS.iter().map(read).sum();
        assert!(
            total <= TOTAL,
            "the skills total {total} characters, over {TOTAL}"
        );
    }
}
