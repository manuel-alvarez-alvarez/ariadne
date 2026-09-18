//! Built-in default texts: the lifecycle prompts, and the skills Ariadne
//! ships.
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
//! a skill too ([`ORCHESTRATION_SKILL`]). A briefing template carries the values of one goal, task or
//! task and whatever is only true of this moment — the changes a review
//! asked for, the landing procedure — and nothing of the playbook that reached
//! the agent. A resume is a nudge: where the work stands and what ends it.
//! What every session is told alike — that Ariadne is reached through its MCP
//! tools, whom a question reaches, and how few turns to take — is the MCP
//! server's `instructions`, which every session already receives, and appears
//! in no prompt here. What each seat does when it cannot go on is one line of
//! its own: the orchestrator asks the user, the author gives the task up, the
//! reviewer asks for changes.
//!
//! Every text here is written in ASD-STE100 Simplified Technical English: one
//! instruction to a sentence, the imperative for an instruction, the active
//! voice, sentences that stay short, one meaning per word, a list for a
//! sequence of steps. It is what an agent misreads least and pays fewest
//! tokens for, and each playbook holds the agent to it in turn — the
//! orchestrator for its task descriptions, the author for its summaries,
//! commit text and failure reasons, the reviewer for its verdicts. That the
//! rule holds for every seat, and for every word an agent writes, is the MCP
//! server's session rules to say, so `STE` is all a text here spells.
//!
//! The texts are kept small on purpose. `size_caps_hold` keeps the lifecycle
//! prompts small, `skill_size_caps_hold` keeps the skills small, and
//! `every_default_text_is_simplified_technical_english` keeps the sentences
//! short across both.

use ariadne_core::{Landing, PromptKind, Seat};

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

/// The skills a fresh database is seeded with, grouped by what they are for:
/// orchestrating a goal, producing work, reviewing it, and operating what it
/// produced.
///
/// The catalog is the whole of what an agent can be, so adding a skill here is
/// adding a kind of work Ariadne knows how to staff. One skill is nobody's to
/// staff: [`ORCHESTRATION_SKILL`] belongs to the orchestrator's seat, and the
/// store refuses a task agent staffed on it.
pub const BUILTIN_SKILLS: [BuiltinSkill; 18] = [
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
    builtin("testing", include_str!("../skills/testing/SKILL.md")),
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
        "security-review",
        include_str!("../skills/security-review/SKILL.md"),
    ),
    builtin(
        "performance-review",
        include_str!("../skills/performance-review/SKILL.md"),
    ),
    builtin(
        "architecture-review",
        include_str!("../skills/architecture-review/SKILL.md"),
    ),
    // Operating.
    builtin("release", include_str!("../skills/release/SKILL.md")),
    builtin(
        "dependency-upgrade",
        include_str!("../skills/dependency-upgrade/SKILL.md"),
    ),
    builtin("migration", include_str!("../skills/migration/SKILL.md")),
    builtin("triage", include_str!("../skills/triage/SKILL.md")),
    builtin(
        "conflict-resolution",
        include_str!("../skills/conflict-resolution/SKILL.md"),
    ),
];

const fn builtin(name: &'static str, document: &'static str) -> BuiltinSkill {
    BuiltinSkill { name, document }
}

/// The document Ariadne ships under `name`, or `None` where it ships none —
/// which is every skill the user wrote, and those carry their own text.
pub fn default_skill_document(name: &str) -> Option<&'static str> {
    BUILTIN_SKILLS
        .iter()
        .find(|s| s.name == name)
        .map(|s| s.document)
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

/// The system prompt an agent in `seat` is spawned with.
///
/// It says what the seat owes and nothing about the work itself: what an
/// agent can do comes from the skills it loads, so this text is Ariadne's own
/// and no row overrides it.
pub fn default_system_prompt(seat: Seat) -> &'static str {
    match seat {
        Seat::Orchestrator => ORCHESTRATOR_SYSTEM_PROMPT,
        Seat::Author => AUTHOR_SYSTEM_PROMPT,
        Seat::Reviewer => REVIEWER_SYSTEM_PROMPT,
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
        PromptKind::AuthorBriefing => AUTHOR_BRIEFING,
        PromptKind::AuthorResume => AUTHOR_RESUME,
        PromptKind::ChangesRequested => CHANGES_REQUESTED,
        PromptKind::ReviewerBriefing => REVIEWER_BRIEFING,
        PromptKind::ReviewerResume => REVIEWER_RESUME,
        PromptKind::ReviewerPick => REVIEWER_PICK,
    }
}

/// The whole procedure that ends a task on `landing`, which is what its
/// author is handed once the task is approved.
///
/// One text per ending rather than one with three halves: a task ends one
/// way, so the author reads the procedure it runs and nothing of the other
/// two. Nothing overrides these — how a change reaches a base branch is a
/// fact about the task, agreed with the user when the task was written
/// (`Landing`), and a second answer stored anywhere else could only disagree
/// with it.
pub fn default_landing_prompt(landing: Landing) -> &'static str {
    match landing {
        Landing::Merge => LANDING_DIRECT,
        Landing::PullRequest => LANDING_PULL_REQUEST,
        Landing::None => LANDING_NONE,
    }
}

/// Orchestrator seat text: what the seat owes, and no step of the playbook.
///
/// The playbook — the ten phases from reading the goal to `complete_goal`,
/// and the one place `finalize_plan` is explained — is the
/// [`ORCHESTRATION_SKILL`] document, which the launcher loads for every
/// orchestrator session the way a task agent's skills reach it: indexed in
/// the system prompt, written into the run directory. So the playbook is
/// editable and resettable like any shipped skill, and this text is
/// Ariadne's own like the other two seats'.
///
/// What is owed is what no skill edit is allowed to take away: the plan is
/// made *with* the user — the orchestrator is the one seat that talks to
/// them — it writes no code, and a point it cannot settle goes to the user
/// rather than being decided alone.
const ORCHESTRATOR_SYSTEM_PROMPT: &str = r#"You plan one Ariadne goal into tasks, with the user. Never write code. Ask the user where you are blocked."#;

/// Author persona and playbook: what it may touch, what it writes, and the
/// one place `request_review` is explained. Landing is its own too, but the
/// procedure belongs to the briefing that knows which repository this is.
///
/// It is also the one place the division of the checks is stated. An author
/// runs the tests and the lint of what it changed, and no run of the whole
/// suite is its own: a reviewer runs it once before each verdict it gives
/// (`REVIEWER_SYSTEM_PROMPT`), and the landing runs it once after the rebase
/// ([`LANDING_DIRECT`]). How many runs a branch takes is how many verdicts it
/// takes, plus that one. A skill scopes the step it owns and names neither
/// run, so the division cannot go stale in eighteen documents.
///
/// The commits are counted the same way, and here too. A task is one
/// responsibility, cut as one tracer by the orchestrator and squashed by the
/// landing, so it is one commit; a review answer is one more commit on top.
/// An amend rewrites a commit a reviewer already judged by its SHA, so
/// nothing amends.
const AUTHOR_SYSTEM_PROMPT: &str = r#"You own one Ariadne task, from its first commit to the end. Work only in your worktree, on your task branch. Commit nothing generated or unrelated.

1. Read task and criteria. If stuck, call `fail_task` with the reason in STE.
2. Implement only that task. Refactor nothing. Obey `AGENTS.md`, `CLAUDE.md` and `CONTRIBUTING.md`. Commit once in STE. Add a commit per review answer. Never amend. Add requested tests. Run changed tests and lint once before the commit. After the commit, run `git status`; leave nothing shown. Run the repository's generate step; leave no changes. Never run the whole suite: the reviewer runs it before each verdict; the landing runs it once.
3. Write no authorship trailer, no tool trailer, no mention of Ariadne. Leave signing to git.
4. Call `request_review` with a short STE summary: change, reason, verification. End your turn after it. Do not poll. Ariadne wakes you with a verdict or message. Apply every verdict on the same branch and call again. Explain disagreements in that summary.
5. Ask reviewers or the orchestrator unanswered questions with `send_message`. Answer their questions. If the task is wrong, call `fail_task` and explain.
6. Every reviewer approves, and Ariadne briefs you to end the task."#;

/// Reviewer persona and playbook, and the one place the verdict rule is
/// stated: one per review asked for, through `submit_verdict`.
const REVIEWER_SYSTEM_PROMPT: &str = r#"You review one Ariadne task. An approval gates the merge: approve only what you would merge yourself. Your detached worktree holds the branch, read-only: do not edit, commit, amend or branch.

1. Install required tools. Move this worktree to the branch named in the briefing with `git checkout --detach <branch>`. Start the whole test suite, build and linters once for this verdict, in the foreground. Record `git rev-parse HEAD` as the SHA you judge.
2. Read the task, its acceptance criteria and the author's summary. Call `get_diff` for the change. Read the code around it.
3. Judge the change on the task and no more: correctness, edge cases, error handling, conventions, tests and clarity. Judge each test by reading its setup, action and assertions. Never change code to see whether a test fails.
4. Wait for every check. Use each result in your verdict. Where something blocks the review, request changes and name it.
5. `send_message` to ask the author what the change does not answer, and to answer what it asks you. A question is not a verdict.
6. Call `submit_verdict` once per review you are asked for. Put that SHA in every verdict. It is the verdict, and nothing else counts. Approve with a note on what you checked. Or request changes: list files and functions, with each item must-fix or optional. Write the verdict in STE."#;

/// Initial briefing of an orchestrator session: the goal, and the
/// repositories it works in.
///
/// No numbers. How many tasks a goal takes is what the conversation with the
/// user settles (003), and a cap written down before that conversation could
/// only be a guess the orchestrator then has to plan around.
const ORCHESTRATOR_BRIEFING: &str = r#"# Goal: {goal_title}

{goal_description}

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
/// only the orchestrator holds the plan those choices are made against.
///
/// The tasks are rendered by the scheduler that noticed them, one line each,
/// because what happened is the daemon's to say and what to do about it is
/// not.
const GOAL_ATTENTION: &str = r#"The tasks of "{goal_title}" need you:

{tasks}

Read them with `list_tasks`. Retry, cancel or rewrite what you must. Call `complete_goal` once every task is done."#;

/// What one agent said to another, as it reaches the recipient's agent.
///
/// The agents talk to each other, and this is the whole of the transport: the
/// runtime hands the message to the agent as a prompt, so it reaches the agent
/// as a turn rather than as something it has to go and look for.
///
/// The sender is named by its seat and its skills, which is the only thing
/// about a generic agent that means anything to the reader. It carries no id:
/// an answer is a message to that sender like any other, and nothing threads.
const INCOMING_MESSAGE: &str = r#"Message from your {from}:

{body}

Answer it where it asks you something. Add nothing else. Go on with your work."#;

/// Initial briefing of an author session: the task, and the values its
/// commands act on.
const AUTHOR_BRIEFING: &str = r#"# Task: {task_title}

{task_description}

## Context
- Goal: {goal_title}
- Worktree (your cwd): {worktree_path}
- Branch: {branch} onto {base_branch}, ending in {landing}
- Repo: {repo_path}
- Finished dependencies:
{dependencies}"#;

/// What an author holding unfinished work is picked up with, in both
/// situations there are: a session that ended and is being started again, and
/// one that is merely sitting idle with the task still open. Neither wants the
/// task read out to it again — it is in the worktree it is standing in.
const AUTHOR_RESUME: &str = r#"Continue "{task_title}" on {branch}. `git status` and `git log` say what the last session left. Work until the task is complete and verified."#;

/// Resume briefing of an author whose review asked for changes, wherever they
/// were written.
///
/// They can come from the reviewers Ariadne started, or from the people
/// reading a published pull or merge request; `{feedback}` carries whichever
/// it is, each entry under a heading naming who wrote it. What to do with a
/// verdict is the author's playbook to say, not this text's; what this text
/// says is what the review asks of the author, and a point it will not act on
/// is answered as surely as one it will.
const CHANGES_REQUESTED: &str = r#"A review requests changes.

{feedback}

Answer every point. Where you disagree, say why the code stays."#;

/// What the author of an approved task in a `direct` repository is briefed
/// with, unless the repository was given a landing briefing of its own:
/// rebase, squash, fast-forward, so the base branch grows one commit per task
/// and its history stays linear.
///
/// The push comes before `finish_task` because that call ends the task, and
/// the cleanup behind it takes the worktree the push would have run from.
///
/// When the rebase changes nothing, HEAD keeps the reviewer's approved SHA and
/// the whole-suite checks skip. Otherwise, the author runs them after the
/// rebase, before the fast-forward, while failures remain fixable. The commits
/// before them — the task's one, and one per review answer — were proven by the
/// checks of what they changed alone ([`AUTHOR_SYSTEM_PROMPT`]).
const LANDING_DIRECT: &str = r#"# Land task: {task_title}

Approved. Squash {branch} onto {base_branch} in {repo_path}. `<remote>` is what `git -C {repo_path} remote -v` names, if anything.

1. `git -C {repo_path} fetch <remote> {base_branch}`. Then `merge --ff-only <remote>/{base_branch}` if on {base_branch}. Else `fetch <remote> {base_branch}:{base_branch}`.
2. `git rebase {base_branch}` in your worktree. Conflicts are yours.
3. If the rebase changed nothing, keep the reviewer-approved SHA in HEAD. Skip checks. Otherwise, run the whole suite, build and linters once. Fix failures on {branch}; return to step 2.
4. `git reset --soft {base_branch} && git commit`. Land one commit. Use a Conventional Commits subject and explain what changed and why.
5. `git -C {repo_path} merge --ff-only {branch}`. If base moved, go to step 1.
6. `git -C {repo_path} push <remote> {base_branch}`. Push first: `finish_task` ends the task and the cleanup takes your worktree.
7. `finish_task` with `git -C {repo_path} rev-parse {base_branch}`."#;

/// What the author of an approved task in a `pull_request` repository is
/// briefed with, unless the repository was given one of its own: publish it,
/// then see it through in this session.
///
/// The forge is read off the `origin` remote at that moment — GitHub takes
/// `gh`, GitLab `glab` — rather than configured anywhere, since the remote is
/// the answer and cannot go stale.
///
/// A published branch is rebased once and never again: the people reading it
/// are reading commits that have to stay where they are, so a revision is
/// added on top and the base is merged in. The merge commit costs nothing —
/// the forge squashes the request when it merges it.
///
/// Waiting happens inside this session, in a poll and sleep loop, and the
/// sleep is capped at five minutes for the daemon's sake: a session that has
/// reported nothing for `QUIET_FLAG_SECS` is flagged as stalled and the
/// relaunch follows at `QUIET_RELAUNCH_SECS` — 600 s and 1800 s — while every
/// poll counts as activity.
const LANDING_PULL_REQUEST: &str = r#"# Land task: {task_title}

Approved. Publish {branch} against {base_branch}. `<remote>` is what `git -C {repo_path} remote -v` names. github.com takes `gh`, GitLab `glab`. Neither, or `auth status` shows no account: `fail_task` with the failed check.

1. `git fetch <remote> {base_branch} && git rebase <remote>/{base_branch}`. The only rebase, and it comes before the push.
2. `git push -u <remote> {branch}`. Then `gh pr create --base {base_branch}` or `glab mr create --target-branch {base_branch}`. Title it by the repository's commit conventions. Fill its template. Call `record_pull_request` with the URL.
3. Poll it and its comments (`gh pr view`, `glab mr view`). `sleep 300` between polls, never longer in one call. Never end your turn while it is open.
4. Answer every comment. Commit a change on {branch}. Put it through `request_review`. Push it once approved. A published branch only grows: no `commit --amend`, no rebase, no forced push. If it stops merging cleanly, `git merge --no-edit <remote>/{base_branch}` and push plainly.
5. Finished: `gh pr merge --squash` or `glab mr merge --squash`. In {repo_path}, fetch and `git merge --ff-only <remote>/{base_branch}`. Then `finish_task` with `git rev-parse {base_branch}`. Closed unmerged: `fail_task` with that."#;

/// What the author of an approved task that lands nothing is briefed with.
///
/// Not every task ends in a commit on a base branch. A release ends in a
/// published tag, an audit in a filed report, a piece of research in a
/// document somewhere else entirely. Ariadne calls all of those `finished`
/// (`Landing::None`), and what this text has to do is the one thing the two
/// landing briefings do for free: make sure nothing the task produced is left
/// only in a worktree, which is thrown away with the task.
///
/// It names no forge and no merge command, because there is nothing to merge.
const LANDING_NONE: &str = r#"# Finish task: {task_title}

Approved. This task lands nothing: {branch} is thrown away when the task ends, and so is your worktree.

1. Check what the task asked for is done, and is where the task said to put it.
2. Anything still only in this worktree is lost. Put it where it belongs now.
3. Call `finish_task`. It takes no merge commit, because nothing was merged."#;

/// Initial briefing of a reviewer session: the task and the branch its
/// worktree is pinned to.
const REVIEWER_BRIEFING: &str = r#"# Review task: {task_title}

{task_description}

## Context
- Goal: {goal_title}
- Branch: {branch} onto {base_branch}
- Repo: {repo_path}
- Author's summary: {summary}"#;

/// What a reviewer that owes a verdict is picked up with, in both situations
/// there are: an author that revised the change under its worktree, and a
/// review it has simply gone quiet in. Either way the diff it last read may
/// be stale and the verdict is still outstanding.
const REVIEWER_RESUME: &str = r#""{task_title}" needs your verdict. Move this worktree to the current branch tip with `git checkout --detach {branch}`. Start the whole test suite, build and linters once again for this verdict, in the foreground.

Read the SHA from your last verdict with `read_messages`. Confirm it with `git merge-base --is-ancestor <sha> HEAD`. If HEAD is not after that SHA, use `get_diff`. If no SHA is known, use `get_diff`. Otherwise, run `git log <sha>..HEAD` and `git diff <sha>..HEAD` here. Read only those new commits. Use every check result before your verdict.

Summary: {summary}"#;

/// What a reviewer is asked with once every author of a several-author task
/// is approved: an approval says a change is sound, and the pick says which
/// of the sound changes lands. One line per author, rendered by the scheduler
/// that saw the last approval arrive.
const REVIEWER_PICK: &str = r#"Every author of "{task_title}" is approved. Pick the one change that lands.

The authors:
{authors}

Compare the branches with `get_diff`. Then call `pick_winner` once, with the id of the author you pick."#;

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
    pub const BANNED: [&str; 6] = [
        "utilise",
        "prior to",
        "in order to",
        "ensure",
        "should",
        "may",
    ];

    /// The longest a sentence runs. STE holds a procedure to 20 words and a
    /// description to 25; these texts are both, so 25 is the one number.
    pub const MAX_WORDS: usize = 25;

    /// The sentences of `text`, as the rules are read on them.
    ///
    /// A line is a statement of its own — a heading, a bullet, a numbered
    /// step — and a line holding several sentences is cut at every `.`, `!`
    /// or `?` that a space or the end of the line follows. The stop inside
    /// `AGENTS.md` is followed by a letter and cuts nothing; the one after a
    /// digit is left alone too, so neither the `1.` a step opens on nor a
    /// `step 1.` it ends on starts a sentence of its own.
    pub fn sentences(text: &str) -> Vec<&str> {
        let mut out = Vec::new();
        for line in text.lines() {
            let (mut start, mut previous) = (0, None);
            for (at, ch) in line.char_indices() {
                let end = at + ch.len_utf8();
                if matches!(ch, '.' | '!' | '?')
                    && !previous.is_some_and(|c: char| c.is_ascii_digit())
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

    /// Every default text there is, named as the test failures name it: the
    /// system prompt of each seat, the template of each prompt kind, and the
    /// landing briefing of each merge strategy.
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
            .chain(
                Landing::ALL
                    .into_iter()
                    .map(|landing| (landing_name(landing), default_landing_prompt(landing))),
            )
            .collect()
    }

    /// Every shipped skill, named the way a failure names it.
    fn all_skills() -> Vec<(String, &'static str)> {
        BUILTIN_SKILLS
            .iter()
            .map(|s| (format!("{} skill", s.name), s.document))
            .collect()
    }

    /// How a strategy's landing briefing is named in a failure.
    fn landing_name(landing: Landing) -> String {
        format!("{} landing briefing", landing.as_str())
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
    /// — and over the landing briefings a repository runs on, with the three
    /// system prompts pinned separately and every text counted again in a
    /// grand total, since a session pays for one of each.
    ///
    /// Their history is a long creep and one cut. A system prompt went from
    /// 900 to 1050 (when *not* to end planning), to 1200 (whom to write to),
    /// to 1400 (how the orchestrator asks), to 1900 (sizing a task's model
    /// and effort): every step a part of the lifecycle nothing else states.
    /// Then every text was rewritten to say the same rules in fewer words —
    /// short imperatives, no restated rationale, no rule stated in two layers
    /// — and the caps came down to what that rewrite fits in: a quarter off
    /// the whole, and a system prompt back under 1000 for the first time
    /// since it was 900.
    ///
    /// What is left is rules and the commands that carry them out. The one
    /// cap above its old aim is the published landing, at 1300 for 1200: two
    /// forges spell `pr create`, `pr view` and `pr merge` differently, and
    /// those six spellings are ~90 characters an author on either forge
    /// needs in front of it. Moving a cap is a decision to argue for, never a
    /// way round a failing assertion.
    ///
    /// Then the texts were written again in Simplified Technical English,
    /// which costs a sentence break where a semicolon used to join two
    /// instructions, and pays for it in two ways. The two lines telling an
    /// agent not to ask went, since the session rules already say it. And
    /// the English a seat writes its own texts in is said on the instruction
    /// that writes them — `fail_task` with the reason in STE, commits whose
    /// text is in STE — rather than in a step of its own. The whole is 5825
    /// characters for the 5828 it was.
    ///
    /// The landings are where that English costs the most, since a step that
    /// joined three commands with commas is three sentences now: they are
    /// 2123 characters for the 2087 they were, and the briefing kinds pay it
    /// back at 1067 for 1078. The caps came down to what the rewrite fits
    /// in, the published landing's excepted.
    ///
    /// Then the orchestrator's playbook grew the phases no other seat has:
    /// a goal clarified with the user, the tasks split out of it, an author
    /// staffed on each, the review agreed task by task, the ending agreed the
    /// same way, and an explicit yes waited for. The one cap of the three
    /// system prompts became one per seat — the orchestrator's own, and the
    /// 950 the author and the reviewer already fit in, which a shared cap
    /// would have let them creep into.
    ///
    /// The briefing kinds then grew a third orchestrator text. The
    /// orchestrator outlives its own hand-off now, so there are two ways to
    /// pick it up rather than one: a nudge for the one that went quiet mid
    /// conversation, and a wake for the one whose tasks need a decision. That
    /// is 1293 characters for 1155, and no other kind paid for it — the total
    /// went to 1350 for the second text, which is a situation the daemon
    /// could not report before rather than a rewording of one it could.
    ///
    /// Then the agents got a channel to each other, and one more kind with
    /// it: what a message looks like when it reaches an agent. Every seat is
    /// briefed with that one — anybody can be written to — and it is the
    /// transport for a thing no text could carry before, so the total went to
    /// 1500 rather than the kinds being squeezed to fit it.
    ///
    /// The author's and the reviewer's went to 1060 for what a message is
    /// *not* for. An agent handed a channel, and a peer that answers on it,
    /// thanks whoever answered and is thanked back; the channel has one verb
    /// now and nothing is answered, and each seat is told so where it is told
    /// to use it. Every other text says what to do rather than what a message
    /// is for, so there was nowhere else to say it.
    ///
    /// The orchestrator's went to 1750 for the mix: staffing a task was a
    /// question about that task alone, and it is now a question about the
    /// plan as well — the agents are spread over the tasks rather than
    /// every agent going on whichever one the orchestrator likes. That is a
    /// decision nothing else in the system makes, and the two sentences it
    /// takes are the shortest it has been said in.
    ///
    /// Then the playbook moved out. The ten phases are the `orchestration`
    /// skill now, capped with the skills (`skill_size_caps_hold`), and what
    /// the seat text keeps is the three things no skill edit is allowed to
    /// take away. So the orchestrator's cap fell from 1750 to 200 — under
    /// the other two seats' for the first time — and the grand total came
    /// down from 8000 to 6500 with it.
    ///
    /// The reviewer now starts the whole suite, build and linters before the
    /// read, binds one run to each verdict and records the judged SHA. Its cap
    /// first rose from 1060 to 1300 because those are new review rules, not
    /// longer forms of existing rules. Review then found that a live
    /// reviewer's detached worktree can still point at its last verdict. The
    /// checkout that refreshes it raises the system cap to 1400. The resume
    /// carries that checkout, the commands for the new commits and an ancestry
    /// fallback, so its cap rises from 200 to 630. The kind total rises from
    /// 1750 to 2020, and the grand total rises from 6500 to 7050, to hold those
    /// additions.
    ///
    /// The other half of that division is the author's. An author used to be
    /// told to keep the tests and the linters green, which every layer under it
    /// read as the whole suite at every commit; it runs the checks of what it
    /// changed now, and it names the two seats that do run the whole thing. The
    /// division is three clauses of the author's seat text and a step of the
    /// direct landing, and it is stated nowhere else, so it is paid for once:
    /// the author's cap rises from 1060 to 1250, and the direct landing's from
    /// 880 to 1000 for the run it gained after the rebase. The landings' total
    /// rises from 2570 to 2700 with it. The grand total rises from 7050 to
    /// 7380 for the added author and landing rules.
    #[test]
    fn size_caps_hold() {
        // Raised from 1500 for the reviewer's pick briefing: a kind that did
        // not exist before several authors could share a task.
        const KIND_TOTAL: usize = 2020;
        // Three now rather than two: the ending that lands nothing used to be
        // counted apart, because a repository could rewrite the other two and
        // never that one. Nothing rewrites any of them now, so they are one
        // set, and the total is the two plus the third at its own cap.
        const LANDING_TOTAL: usize = 2700;
        const GRAND_TOTAL: usize = 7380;

        // A cap per seat, not one for the three. The orchestrator's carried
        // its playbook up to 1750; the playbook is the `orchestration` skill
        // now, and 200 holds what is left to the seat text it is.
        //
        // The author's and the reviewer's went to 1010 for the channel the
        // agents talk on: one step each about asking and answering, which is
        // a thing neither could do before rather than a rewording of a thing
        // it could. Then the author's alone went to 1250 for the division of
        // the checks, which is its text to state and no other seat's.
        let system_cap = |seat: Seat| match seat {
            Seat::Orchestrator => 200,
            Seat::Author => 1250,
            Seat::Reviewer => 1400,
        };
        let cap = |kind: PromptKind| match kind {
            PromptKind::OrchestratorResume | PromptKind::AuthorResume => 200,
            PromptKind::ReviewerResume => 630,
            _ => 300,
        };
        let landing_cap = |landing: Landing| match landing {
            Landing::Merge => 1000,
            Landing::PullRequest => 1300,
            Landing::None => 420,
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

        let mut landings = 0;
        for landing in Landing::ALL {
            let text = default_landing_prompt(landing);
            landings += text.len();
            assert!(
                text.len() <= landing_cap(landing),
                "the {} is {} characters, over its {}",
                landing_name(landing),
                text.len(),
                landing_cap(landing)
            );
        }
        assert!(
            landings <= LANDING_TOTAL,
            "the landing briefings total {landings} characters, over {LANDING_TOTAL}"
        );

        let grand: usize = all_defaults().iter().map(|(_, text)| text.len()).sum();
        println!(
            "{kinds:5}  every briefing template\n{landings:5}  every landing briefing\n\
             {grand:5}  every default text"
        );
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
        // A file name and a step number end no sentence of their own.
        assert_eq!(
            ste::sentences("Read `AGENTS.md` first. Back to step 1. Then push."),
            ["Read `AGENTS.md` first.", "Back to step 1. Then push."]
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
    ///
    /// The rules a test cannot read — one instruction to a sentence, the
    /// imperative, the active voice — are what the texts above are written
    /// in, and what a rewrite of one is read against.
    #[test]
    fn every_default_text_is_simplified_technical_english() {
        for (name, text) in all_defaults().into_iter().chain(all_skills()) {
            for sentence in ste::sentences(text) {
                let words = sentence.split_whitespace().count();
                assert!(
                    words <= ste::MAX_WORDS,
                    "the {name} runs a sentence of {words} words, over {}: {sentence}",
                    ste::MAX_WORDS
                );
            }
            assert_eq!(
                ste::banned_word(text),
                None,
                "the {name} uses a word STE has no room for"
            );
        }
    }

    /// How Ariadne is reached is the MCP server's `instructions` to say, and
    /// only its: the block that used to be pasted into all three system
    /// prompts lives in one place now, and no prompt here repeats it.
    #[test]
    fn no_default_repeats_what_every_session_is_told_by_the_mcp_server() {
        for (name, text) in all_defaults() {
            for shared in [
                "Reach Ariadne",
                "backticked",
                "Work alone",
                "narrate progress",
                "as few turns as you can",
                "ASD-STE100",
            ] {
                assert!(
                    !text.contains(shared),
                    "the {name} repeats \"{shared}\", which the MCP instructions state"
                );
            }
        }
    }

    /// A rule that holds for one seat is stated in that seat's prompt, and
    /// only there: the reviewer's verdict rule and the author's ownership
    /// of the landing are what the other prompts are free of.
    #[test]
    fn a_seat_rule_is_stated_in_its_own_prompt_alone() {
        for (owner, rule) in [
            (Seat::Reviewer, "It is the verdict, and nothing else counts"),
            (Seat::Author, "Ariadne briefs you to end the task"),
            (Seat::Orchestrator, "Never write code"),
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

    /// A reviewer starts every expensive check before reading, so the checks
    /// run while the review proceeds. Both texts say one run belongs to one
    /// verdict, which prevents a second run before that verdict.
    #[test]
    fn reviewer_checks_start_before_the_read_once_per_verdict() {
        for (name, text) in [
            (
                "reviewer system prompt",
                default_system_prompt(Seat::Reviewer),
            ),
            (
                "code-review skill",
                default_skill_document("code-review").unwrap(),
            ),
        ] {
            let checks = text
                .find("Start the whole test suite, build and linters once for this verdict")
                .unwrap_or_else(|| panic!("the {name} does not start every check once"));
            let read = text
                .find("Read the task")
                .unwrap_or_else(|| panic!("the {name} does not read the task"));
            assert!(
                checks < read,
                "the {name} reads before it starts the checks"
            );
            assert_eq!(
                text.matches("once for this verdict").count(),
                1,
                "the {name} does not bind one check run to one verdict"
            );
        }
    }

    /// A verdict identifies the exact work it judged, so the next review can
    /// start after it. The reviewer reads HEAD in its own worktree and puts
    /// that SHA in every verdict.
    #[test]
    fn every_reviewer_verdict_carries_the_sha_it_judged() {
        let prompt = default_system_prompt(Seat::Reviewer);
        for rule in ["`git rev-parse HEAD`", "Put that SHA in every verdict"] {
            assert!(prompt.contains(rule), "the reviewer prompt and {rule}");
        }
    }

    /// Test quality is visible in the test's setup, action and assertions.
    /// The reviewer reads those parts and never changes the author's code to
    /// manufacture a failure in a read-only worktree.
    #[test]
    fn a_reviewer_judges_a_test_by_reading_it_without_changing_code() {
        for (name, text) in [
            (
                "reviewer system prompt",
                default_system_prompt(Seat::Reviewer),
            ),
            (
                "code-review skill",
                default_skill_document("code-review").unwrap(),
            ),
        ] {
            for rule in [
                "Judge each test by reading its setup, action and assertions",
                "Never change code to see whether a test fails",
            ] {
                assert!(text.contains(rule), "the {name} and {rule}");
            }
        }
    }

    /// A resumed reviewer reads only work added after its last verdict. The
    /// last verdict's SHA bounds both the commit list and the diff; only a
    /// missing SHA falls back to the whole change.
    #[test]
    fn a_reviewer_resume_reads_only_commits_since_its_last_verdict_sha() {
        let resume = default_prompt_text(PromptKind::ReviewerResume);
        let checks = resume
            .find("Start the whole test suite")
            .expect("the reviewer resume does not start the checks");
        let read = resume
            .find("Read the SHA from your last verdict")
            .expect("the reviewer resume does not read the last verdict SHA");
        assert!(checks < read, "the reviewer resume reads before its checks");
        for rule in [
            "Read the SHA from your last verdict",
            "`read_messages`",
            "`git log <sha>..HEAD`",
            "`git diff <sha>..HEAD`",
            "Read only those new commits",
            "If no SHA is known, use `get_diff`",
            "once again for this verdict",
        ] {
            assert!(resume.contains(rule), "the reviewer resume and {rule}");
        }
    }

    /// A live reviewer's detached worktree can still point at the last
    /// verdict. The resume moves it to the branch named by the briefing before
    /// it starts checks, so every result covers the new branch tip.
    #[test]
    fn reviewer_texts_refresh_the_named_branch_before_checks() {
        for (name, text, command) in [
            (
                "reviewer system prompt",
                default_system_prompt(Seat::Reviewer),
                "`git checkout --detach <branch>`",
            ),
            (
                "reviewer resume",
                default_prompt_text(PromptKind::ReviewerResume),
                "`git checkout --detach {branch}`",
            ),
            (
                "code-review skill",
                default_skill_document("code-review").unwrap(),
                "`git checkout --detach <branch>`",
            ),
        ] {
            let refresh = text
                .find(command)
                .unwrap_or_else(|| panic!("the {name} does not refresh its named branch"));
            let checks = text
                .find("Start the whole test suite")
                .unwrap_or_else(|| panic!("the {name} does not start the checks"));
            assert!(refresh < checks, "the {name} checks a stale worktree");
        }
    }

    /// A last verdict can name a SHA outside the current history. Both review
    /// texts test that relationship and fall back to the whole change instead
    /// of treating an empty range as no change.
    #[test]
    fn a_reviewer_uses_the_whole_diff_when_head_does_not_follow_the_last_sha() {
        for (name, text) in [
            (
                "reviewer resume",
                default_prompt_text(PromptKind::ReviewerResume),
            ),
            (
                "code-review skill",
                default_skill_document("code-review").unwrap(),
            ),
        ] {
            for rule in [
                "`git merge-base --is-ancestor <sha> HEAD`",
                "If HEAD is not after that SHA, use `get_diff`",
            ] {
                assert!(text.contains(rule), "the {name} and {rule}");
            }
        }
    }

    /// A review that requests changes asks the author for two things, and
    /// the briefing that carries the feedback is where both are asked: every
    /// point answered, and, for a point the author will not act on, why the
    /// code stays as it is. A briefing that asked only for the answers would
    /// read as leave to drop the rest in silence.
    #[test]
    fn a_round_of_requested_changes_asks_for_every_point_and_for_a_disagreement() {
        let text = default_prompt_text(PromptKind::ChangesRequested);
        for rule in [
            "Answer every point",
            "Where you disagree, say why the code stays",
        ] {
            assert!(
                text.contains(rule),
                "the changes-requested briefing and \"{rule}\": {text}"
            );
        }
    }

    /// `finish_task` is the end of the task: the daemon cleans the worktree up
    /// behind it and the session can go with it. So whatever the author still
    /// has to run has to come first — the push of the base branch above all,
    /// which is the one step whose absence leaves the commit on this machine
    /// alone with nothing left to notice.
    #[test]
    fn nothing_the_author_still_has_to_run_comes_after_the_call_that_ends_the_task() {
        for landing in Landing::ALL {
            let text = default_landing_prompt(landing);
            let ends = text
                .find("`finish_task`")
                .unwrap_or_else(|| panic!("the {} never ends the task", landing_name(landing)));
            for command in [
                "git -C {repo_path} push",
                "git push",
                "gh pr merge",
                "glab mr merge",
                "request_review",
                "record_pull_request",
            ] {
                if let Some(at) = text.find(command) {
                    assert!(
                        at < ends,
                        "the {} runs {command} after finish_task",
                        landing_name(landing)
                    );
                }
            }
        }

        // And the reason is in the text, where the agent reading it is.
        assert!(
            default_landing_prompt(Landing::Merge).contains("Push first:"),
            "the direct briefing does not say why the push comes first"
        );
    }

    /// The author's checks are the checks of what it changed, and the seat
    /// text says so with the reason: the whole suite is run by a reviewer
    /// before each verdict and by the landing once, and neither run is the
    /// author's.
    ///
    /// An author told to keep the tests and the linters green reads that as
    /// the whole suite, and runs it at every step of the work — tens of runs
    /// a task, each one minutes the task does not spend on the work. So
    /// the seat text scopes the run and names the two seats that do run the
    /// whole thing, because a scope with nobody behind it reads as a change
    /// that nothing proves.
    #[test]
    fn the_author_scopes_its_checks_and_names_who_runs_the_whole_suite() {
        let author = default_system_prompt(Seat::Author);
        for rule in [
            "Run changed tests and lint once before the commit.",
            "After the commit, run `git status`; leave nothing shown.",
            "Run the repository's generate step; leave no changes.",
            "Never run the whole suite",
            "the reviewer runs it before each verdict; the landing runs it once",
            "End your turn after it. Do not poll.",
            "Ariadne wakes you with a verdict or message.",
        ] {
            assert!(author.contains(rule), "the author seat text and \"{rule}\"");
        }
        // And the rule it replaced is gone, rather than sitting beside it.
        assert!(
            !author.contains("Keep tests and linters green"),
            "the author seat text still asks for every linter at every commit"
        );
    }

    /// The whole suite's one run on a task branch is the landing's, and it
    /// stands between the rebase and the fast-forward.
    ///
    /// Before the rebase it proves a tree the base branch never grows, and a
    /// base that moved under it is exactly what the run is for. After the
    /// fast-forward it is too late: the base branch already carries the
    /// commit, and a failure is a revert rather than a fix. So the run sits
    /// between the two, and a check that fails goes back to the rebase.
    #[test]
    fn the_direct_landing_runs_the_whole_suite_after_the_rebase_and_before_the_fast_forward() {
        let direct = default_landing_prompt(Landing::Merge);
        let rebase = direct
            .find("`git rebase {base_branch}`")
            .expect("the direct landing never rebases");
        let suite = direct
            .find("Otherwise, run the whole suite, build and linters once.")
            .expect("the direct landing never runs the whole suite");
        let forward = direct
            .find("merge --ff-only {branch}")
            .expect("the direct landing never fast-forwards the base branch");
        assert!(
            rebase < suite && suite < forward,
            "the whole suite does not run between the rebase and the fast-forward: {direct}"
        );

        // Once, and a failure is fixed on the branch and rebased again.
        assert_eq!(direct.matches("whole suite").count(), 1, "{direct}");
        assert!(
            direct.contains("Fix failures on {branch}; return to step 2"),
            "the direct landing does not say where a failed check is fixed: {direct}"
        );
        assert!(
            direct.contains("If the rebase changed nothing, keep the reviewer-approved SHA in HEAD. Skip checks."),
            "the direct landing does not skip checks after an unchanged rebase: {direct}"
        );
    }

    /// A skill scopes the step it owns to what the task changed, and names
    /// none of the whole-suite runs: those belong to the author's seat text,
    /// which is the one place the division is stated.
    ///
    /// The five here are the skills that used to end a step on the suite. The
    /// two that are left out run nowhere near a task branch: `code-review` is
    /// the reviewer's, and `release` runs on the base branch.
    #[test]
    fn a_skill_scopes_its_own_checks_to_what_the_task_changed() {
        for (name, step) in [
            ("coding", "run the tests and the lint of what you changed"),
            ("debugging", "the tests of the crate you changed are green"),
            ("testing", "Run the one test, never the suite around it."),
            (
                "conflict-resolution",
                "Run the tests and the lint of the files you resolved.",
            ),
            (
                "dependency-upgrade",
                "Run the tests, the lint and the build of the packages that use the dependency.",
            ),
        ] {
            let document = unwrapped(default_skill_document(name).unwrap());
            assert!(
                document.contains(step),
                "the {name} skill does not say \"{step}\""
            );
            for suite in ["whole suite", "full suite", "the suite is green"] {
                assert!(
                    !document.contains(suite),
                    "the {name} skill sends an author to the {suite}"
                );
            }
        }
    }

    /// A task is one commit, and every text that tells an author when to
    /// commit says the same thing.
    ///
    /// A task is one responsibility: the orchestrator cuts it as one tracer,
    /// and the landing squashes the branch onto the base. So a branch divided
    /// into parts buys nothing and costs the review a diff of half-built
    /// commits. No skill sends an author to a slice or to a small commit, the
    /// two skills that name the commit name one, and the seat text adds the
    /// one commit each review answer takes.
    ///
    /// That last rule is the seat text's alone, and this is what holds it
    /// there. A skill names the commit of the step it owns; what a review
    /// answer costs and what an amend would break hold for every author
    /// whatever it is doing, so a skill that repeated them would be a second
    /// owner of a rule, free to go stale against the first.
    #[test]
    fn a_task_is_one_commit_and_a_review_answer_is_one_more() {
        for (name, document) in all_skills() {
            let document = unwrapped(document).to_lowercase();
            for divided in ["slice", "small commit"] {
                assert!(
                    !document.contains(divided),
                    "the {name} divides a task into a {divided}"
                );
            }
            for lifecycle in ["commit per review answer", "amend"] {
                assert!(
                    !document.contains(lifecycle),
                    "the {name} repeats the seat text's rule about \"{lifecycle}\""
                );
            }
        }

        for (name, step) in [
            ("coding", "Done when the task is one commit on your branch."),
            (
                "refactoring",
                "Commit the whole refactor once, after every move is green.",
            ),
        ] {
            let document = unwrapped(default_skill_document(name).unwrap());
            assert!(
                document.contains(step),
                "the {name} skill does not say \"{step}\""
            );
        }

        let author = default_system_prompt(Seat::Author);
        for rule in [
            "Commit once in STE.",
            "Add a commit per review answer. Never amend.",
        ] {
            assert!(author.contains(rule), "the author seat text and \"{rule}\"");
        }
        // And the rule it replaced is gone, rather than sitting beside it.
        assert!(
            !author.contains("Make small commits"),
            "the author seat text still asks for small commits"
        );
    }

    /// Each landing briefing is the procedure of one merge strategy, whole,
    /// and carries nothing of the other: the repository is on one strategy, so
    /// the author has neither a section to skip nor a choice to make.
    #[test]
    fn each_landing_briefing_is_one_strategy_and_nothing_of_the_other() {
        let direct = default_landing_prompt(Landing::Merge);
        let published = default_landing_prompt(Landing::PullRequest);

        // Squashed onto the base with git alone.
        for step in [
            "git rebase {base_branch}",
            "git reset --soft {base_branch}",
            "merge --ff-only {branch}",
            "git -C {repo_path} push <remote> {base_branch}",
            "Conventional Commits",
            "`finish_task`",
        ] {
            assert!(direct.contains(step), "the direct briefing has no {step}");
        }

        // Published, answered and merged by the author, on either forge.
        for step in [
            "auth status",
            "gh pr create",
            "gh pr view",
            "gh pr merge",
            "glab mr create",
            "glab mr view",
            "glab mr merge",
            "record_pull_request",
            "`finish_task`",
        ] {
            assert!(
                published.contains(step),
                "the published briefing has no {step}"
            );
        }

        // The wait is a poll loop in the author's own session, and the cap
        // on one sleep is what keeps the daemon from relaunching it mid-wait.
        assert!(published.contains("sleep 300"));
        assert!(published.contains("never longer in one call"));

        // A published branch only ever grows.
        for never in ["forced push", "commit --amend"] {
            assert!(
                published.contains(never),
                "the published briefing does not forbid {never}"
            );
        }

        // And neither one names the other's procedure.
        for forge in ["gh ", "glab ", "pull request", "merge request", "sleep"] {
            assert!(
                !direct.contains(forge),
                "the direct briefing names {forge}, which is the other strategy's"
            );
        }
        for squash in ["reset --soft", "merge --ff-only {branch}"] {
            assert!(
                !published.contains(squash),
                "the published briefing names {squash}, which is the other strategy's"
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
    ///
    /// The landing briefings count as one place between them all: a
    /// repository has one merge strategy, and a session has one seat, so the
    /// author of a task is handed one of them and the orchestrator of a goal
    /// another. No session ever reads two.
    #[test]
    fn each_rule_is_stated_in_exactly_one_briefing() {
        // What a published branch may be done to, what ends a piece of
        // engineering work, and what each of the three calls that move a task
        // along is *for* — named elsewhere, explained here. The last one
        // lives in the orchestration skill now, with the playbook it ends.
        //
        // The last two are the division of the checks: which agent runs the
        // whole suite is the author's seat text to say, and the run itself is
        // a step of the landing that owns it.
        for marker in [
            "git merge --no-edit",
            "push plainly",
            "--ff-only",
            "Call `request_review` with a short STE summary",
            "Call `submit_verdict` once per review you are asked for",
            "It starts every task and ends planning",
            "the reviewer runs it before each verdict; the landing runs it once",
            "Otherwise, run the whole suite, build and linters once.",
        ] {
            let places = all_defaults()
                .into_iter()
                .chain(all_skills())
                .filter(|(_, text)| text.contains(marker))
                .map(|(name, _)| match name.ends_with("landing briefing") {
                    true => "a landing briefing".to_string(),
                    false => name,
                })
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
        for landing in Landing::ALL {
            assert_eq!(
                Landing::validate_landing_template(default_landing_prompt(landing)),
                Ok(()),
                "the default {}",
                landing_name(landing)
            );
        }
    }

    /// The orchestrator playbook is a conversation, and the order of its
    /// phases is the playbook: the goal is read, every unclear point is asked
    /// about one question at a time, the goal is split into tasks, each task
    /// is staffed, the review and the ending of each are agreed with the user,
    /// and only an explicit yes starts any of it.
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
    const PLAYBOOK_PHASES: [&str; 14] = [
        "Read the goal. Explore its repositories.",
        "Ask the user about every unclear point",
        "Write one question in your turn text.",
        "Wait for the answer in the console.",
        "Split the goal into tasks",
        "Staff the authors of each task with `create_task`",
        "Staff one reviewer on every task.",
        "Ask the user which tasks to leave unreviewed",
        "Leave a task unreviewed only when nothing can be tested whole, such as a release or a report.",
        "Ask the user how each task ends",
        "Mix the agents evenly over the tasks.",
        "Revise them until they write an explicit yes.",
        "Call `finalize_plan`",
        "Stay up for the rest of the goal.",
    ];

    /// The playbook is the `orchestration` skill, so the document read here
    /// is the shipped skill rather than the seat text, which carries no step
    /// of it. A phase out of order is an orchestrator that fails to staff a
    /// reviewer by default, or that starts a plan the user has not seen.
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

    /// A false `depends_on` serializes two tasks that could run together, so
    /// the skill tells the orchestrator to write a shared interface into
    /// both tickets instead. And the landing already proved the base branch,
    /// so the orchestrator runs no checks of its own before `complete_goal`.
    #[test]
    fn the_orchestration_skill_names_the_contract_rule_and_the_no_checks_rule() {
        let prompt = default_skill_document(ORCHESTRATION_SKILL).unwrap();
        assert!(
            prompt.contains("Name what one task hands to another")
                && prompt.contains("Add no\n   `depends_on` for it."),
            "the skill has no contract rule in step 3: {prompt}"
        );
        assert!(
            prompt.contains("Run no checks yourself: the landing proved the base"),
            "the skill has no no-checks rule in step 10: {prompt}"
        );
        assert!(
            prompt.contains("\"The frontend waits for the backend to land.\"")
                && prompt.contains("Write the contract into")
                && prompt.contains("Both run now."),
            "the skill has no do-not-tell-yourself line for the contract rule: {prompt}"
        );
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
                "the seat text carries \"{phase}\", which is the playbook's"
            );
        }
        assert!(
            !seat.contains('\n') && !seat.contains("1."),
            "the seat text is one line with no step in it: {seat}"
        );
    }

    /// A plan is staffed on a mix of agents, and fit comes first.
    ///
    /// Every task on one agent is a plan that stands or falls with that
    /// agent: its rate limit, its outage, its blind spot on a kind of work.
    /// So the orchestrator is told to spread them — and told in the same
    /// breath not to spread them onto an agent the task does not suit, which
    /// is the failure an instruction to mix invites. The telling is the
    /// `orchestration` skill's, where the playbook lives.
    #[test]
    fn the_orchestrator_staffs_a_plan_on_a_mix_of_agents() {
        let prompt = default_skill_document(ORCHESTRATION_SKILL).unwrap();
        let mix = prompt
            .find("Mix the agents evenly over the tasks.")
            .expect("the orchestrator is not told to mix the agents");
        let fit = prompt
            .find("Take only an agent that suits the task.")
            .expect("the orchestrator is not told to keep the mix suitable");
        assert!(
            fit > mix,
            "the fit has to be the sentence after the mix, or the mix reads as the whole rule"
        );
        // Said where the agents are sized, not in a step of its own: which
        // agent a task runs on is one answer with which model of it.
        assert!(
            mix > prompt
                .find("Give each agent one model from `list_models`")
                .unwrap(),
            "the mix is stated before the sizing it is part of"
        );
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

    /// The orchestrator lands its spec, and still none of its own texts names a
    /// forge or a strategy: which way a repository takes a change is the
    /// repository's `merge_strategy` to say, and the procedure reaches the
    /// orchestrator as a value its briefing carries
    /// ([`default_spec_landing_prompt`]). A playbook that spelled out one of
    /// the two landings would be a second copy of that knowledge, going stale
    /// on its own, and an orchestrator running it in the wrong repository.
    /// The orchestration skill is one of its texts, so it is read here too.
    #[test]
    fn the_orchestrator_is_told_nothing_of_forges_or_landing() {
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
            "merge_strategy",
        ] {
            assert!(
                !orchestrator.contains(forge),
                "the orchestrator prompts name {forge}"
            );
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

    /// A background poll of a check was the largest single waste a
    /// measurement of this week's sessions found: an agent started
    /// `cargo nextest` in the background and re-sent its whole context on
    /// every no-op turn spent waiting for it. `testing`, `coding`,
    /// `debugging` and `code-review` each carry the fix: a check runs in
    /// the foreground and is never polled, and its full output goes to a
    /// log file outside the worktree so only the summary and the failures
    /// reach the agent's context.
    ///
    /// The reviewer's seat text starts the same checks, so it names the
    /// foreground rule too, in its own words: the detail of the fix —
    /// the timeout, the log file, the command — stays the skill's alone to
    /// state, the way spec 006 holds every rule to one place.
    ///
    /// Each document is read with its line wrapping taken out first, so a
    /// rewrap that moves a marker across a line break cannot make a rule
    /// that is still there read as gone.
    #[test]
    fn checks_run_in_the_foreground_and_print_only_failures() {
        for name in ["testing", "coding", "debugging", "code-review"] {
            let doc = unwrapped(default_skill_document(name).unwrap());
            assert!(
                doc.contains("in the foreground"),
                "{name} does not run a check in the foreground"
            );
            assert!(
                doc.contains("no-op command"),
                "{name} does not forbid polling a background run with a no-op command"
            );
            assert!(
                doc.contains("a log file outside the worktree"),
                "{name} does not send a check's output to a log file outside the worktree"
            );
            assert!(
                doc.contains(
                    "cargo nextest run --status-level fail --final-status-level fail 2>&1 | tail -n 40"
                ),
                "{name} does not print only the summary and the failures of a nextest run"
            );
            assert!(
                doc.contains("the detail of a failure"),
                "{name} does not scope a log read to the detail of a failure"
            );
        }

        for (name, text) in [
            (
                "reviewer system prompt",
                default_system_prompt(Seat::Reviewer),
            ),
            (
                "reviewer resume",
                default_prompt_text(PromptKind::ReviewerResume),
            ),
        ] {
            assert!(
                unwrapped(text).contains("in the foreground"),
                "{name} does not run a check in the foreground"
            );
        }
    }

    /// A skill is read on demand rather than on every launch, so it is capped
    /// on its own rather than against the briefings' total. The caps are still
    /// what a rewrite fits in: moving one is a decision, not a way round a
    /// failing assertion.
    ///
    /// This is that decision, taken once for the suite rather than a skill at
    /// a time. The catalog is being rewritten to one template — a description
    /// that says when to load the skill as well as what it does, a checkable
    /// bound on every step that can end early, one anchor word carried through
    /// the body, and the two or three excuses the agent talks itself into —
    /// and the old 1800 held none of that. The numbers below are the budget of
    /// that rewrite: what a document of the template costs, not what the
    /// documents happen to weigh today.
    ///
    /// Three tiers, by how much procedure the skill carries. `orchestration`
    /// and `debugging` get 3200: one runs the ten phases of a whole goal, the
    /// other a feedback loop the agent is talked out of at every step, and
    /// both spend their length on the excuses rather than on the steps.
    /// `coding`, `testing` and `code-review` get 3000, as the three skills
    /// almost every task loads and the three whose failure modes are worth
    /// spelling out. Every other skill gets 2400, which is a template document
    /// with room for its rules. The total of 50000 is the eighteen at their
    /// tiers with slack left over, so a skill that grows costs a decision here
    /// rather than a quiet raid on another skill's share.
    ///
    /// `code-review` rises from 3000 to 3500 for the new review mechanics. It
    /// starts every check before reading, refreshes a detached worktree and
    /// scopes another review from the last verdict SHA. These rules add
    /// commands and ordering that the old procedure did not hold.
    ///
    /// `debugging` rises from 3200 to 3400 and `code-review` from 3500 to
    /// 4000 for one more rule, carried in `testing`, `coding`, `debugging`
    /// and `code-review` alike: run a check in the foreground, never poll a
    /// background run, and send its output to a log file so only the
    /// summary and the failures reach the agent. A background poll of
    /// `cargo nextest` was the largest single waste a measurement of this
    /// week's sessions found, and the fix costs each of the four skills a
    /// paragraph.
    #[test]
    fn skill_size_caps_hold() {
        const TOTAL: usize = 50_000;
        let cap = |name: &str| match name {
            // The orchestration playbook grew a step-4 choice — one author
            // for most tasks, several where the reviewers pick a winner —
            // and the contract rule that keeps a frontend task and its
            // backend task off a false `depends_on`.
            ORCHESTRATION_SKILL => 3900,
            "debugging" => 3400,
            "code-review" => 4000,
            "coding" | "testing" => 3000,
            _ => 2400,
        };

        for skill in &BUILTIN_SKILLS {
            println!("{:5}  {}", skill.document.len(), skill.name);
        }
        for skill in &BUILTIN_SKILLS {
            assert!(
                skill.document.len() <= cap(skill.name),
                "the {} skill is {} characters, over its {}",
                skill.name,
                skill.document.len(),
                cap(skill.name)
            );
        }
        let total: usize = BUILTIN_SKILLS.iter().map(|s| s.document.len()).sum();
        assert!(
            total <= TOTAL,
            "the skills total {total} characters, over {TOTAL}"
        );
    }
}
