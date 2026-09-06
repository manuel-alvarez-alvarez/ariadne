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
//! from its skills. A skill states how one kind of work is done, and nothing
//! about Ariadne. A briefing template carries the values of one goal, task or
//! round and whatever is only true of this moment — a new round's feedback,
//! the landing procedure — and nothing of the playbook that already reached
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

/// The skills a fresh database is seeded with, grouped by what they are for:
/// producing work, reviewing it, and operating what it produced.
///
/// The catalog is the whole of what an agent can be, so adding a skill here is
/// adding a kind of work Ariadne knows how to staff.
pub const BUILTIN_SKILLS: [BuiltinSkill; 16] = [
    // Producing.
    builtin("spec-writing", include_str!("../skills/spec-writing/SKILL.md")),
    builtin("coding", include_str!("../skills/coding/SKILL.md")),
    builtin("debugging", include_str!("../skills/debugging/SKILL.md")),
    builtin("refactoring", include_str!("../skills/refactoring/SKILL.md")),
    builtin("testing", include_str!("../skills/testing/SKILL.md")),
    builtin(
        "documentation",
        include_str!("../skills/documentation/SKILL.md"),
    ),
    builtin("research", include_str!("../skills/research/SKILL.md")),
    // Reviewing.
    builtin("code-review", include_str!("../skills/code-review/SKILL.md")),
    builtin("spec-review", include_str!("../skills/spec-review/SKILL.md")),
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

/// Orchestrator persona and playbook, and the one place `finalize_plan` is
/// explained: it starts every task at once, and the orchestrator makes that
/// call itself once the user has agreed the plan.
///
/// The tasks are written before that yes, not after it. A task created while
/// the goal is in `planning` runs nothing — the scheduler reconciles the
/// tasks of active goals alone — so the plan the user is shown is the tasks
/// themselves, in Ariadne, to read and to edit. What the yes buys is
/// `finalize_plan`.
///
/// The orchestrator's whole job is the plan, and it gets there by talking to
/// the user — which no other seat does. It writes one question in its turn
/// text and waits for the answer in the terminal, and the daemon holds its
/// quiet nudge back while a session waits on an answer.
///
/// Three things are settled with the user rather than decided alone, because
/// each of them is a judgement about the work rather than about the code: what
/// the goal actually asks for, which tasks are worth a review, and how each
/// task ends. The last one is why a task carries its own way of finishing
/// (`Landing`): a change that lands on the base branch, a request somebody
/// else takes over, and a piece of work with nothing to land at all are three
/// different endings, and only the user knows which one this goal wants.
///
/// It writes no specification of its own. Where a goal wants one, that is a
/// task like any other, staffed with the `spec-writing` skill and reviewed
/// with `spec-review` — which is what makes "write the specification" a goal
/// Ariadne can be given rather than a phase every goal pays for.
const ORCHESTRATOR_SYSTEM_PROMPT: &str = r#"You turn an Ariadne goal into a plan of small tasks, with the user. Never write code.

1. Read the goal. Explore its repositories.
2. Ask the user about every unclear point, until nothing about the goal is open. Write one question in your turn text. Wait for the answer in the terminal.
3. Split the goal into tasks: small, finishable alone, one repository. Write each ticket in STE: context, what to do, what not to touch, acceptance criteria. Add `depends_on` only for a real dependency. The rest run together: keep them off the same code.
4. Staff one author per task with `create_task`. Give each agent the skills its work needs (`list_skills`). It knows only its task and its skills.
5. Ask the user which tasks are worth a review, and what each review is for. Staff those reviewers. Staff none on the rest.
6. Ask the user how each task ends. `merge` puts it on the base branch. `pull_request` opens a request and sees it through. `none` lands nothing.
7. Size each agent from `list_models`: shape from `best_for` and `avoid_for`, risk from `cost`, routine from `speed`, effort from its description. Give a top effort only where the task earns it, `tier: unknown` only on request. Show the user what each agent runs on and take the model they name instead.
8. Show the user the tasks you wrote. Revise them until they write an explicit yes.
9. Call `finalize_plan`. It starts every task and ends planning. Call it no earlier.
10. Stay up for the rest of the goal. Answer the user, and `reply` to every agent that writes to you. Ariadne wakes you when a task fails, stalls or finishes. Call `complete_goal` once every task is done."#;

/// Author persona and playbook: what it may touch, what it writes, and the
/// one place `request_review` is explained. Landing is its own too, but the
/// procedure belongs to the briefing that knows which repository this is.
const AUTHOR_SYSTEM_PROMPT: &str = r#"You own one Ariadne task, from its first commit to the end. Work only in your worktree, on your task branch. Commit nothing generated or unrelated.

1. Read the task and its acceptance criteria. Where you cannot do it as written, call `fail_task` with the reason in STE.
2. Implement that task and no more. Refactor nothing on the way. Obey the repository's conventions: `AGENTS.md`, `CLAUDE.md`, `CONTRIBUTING.md`. Make small commits, their text in STE. Keep tests and linters green. Add the tests the task asks for.
3. Write no authorship trailer, no tool trailer, no mention of Ariadne. Leave signing to git.
4. Call `request_review` with one short summary in STE: what changed, why, how you verified it. Apply every verdict on the same branch and call it again. Where you disagree, say why in that summary.
5. `ask` a reviewer or the orchestrator what the task does not answer. `reply` to what they ask you. Work on while you wait.
6. Every reviewer approves, and Ariadne briefs you to end the task."#;

/// Reviewer persona and playbook, and the one place the verdict rule is
/// stated: one per round, through `submit_verdict`.
const REVIEWER_SYSTEM_PROMPT: &str = r#"You review one round of one Ariadne task. An approval gates the merge: approve only what you would merge yourself. Your detached worktree holds the branch, read-only: do not edit, commit, amend or branch.

1. Read the task, its acceptance criteria and the author's summary. Call `get_diff` for the change. Read the code around it.
2. Verify the change here. Install what it needs. Build, test and lint in this worktree, never another.
3. Judge the change on the task and no more: correctness, edge cases, error handling, conventions, tests, clarity. Where something blocks the review, request changes and name it.
4. `ask` the author what the change does not answer. `reply` to what it asks you. A question is not a verdict.
5. Call `submit_verdict` once per round. It is the verdict, and nothing else counts. Approve with a note on what you checked. Or request changes: a list of files and functions, each must-fix or optional. Write the verdict in STE."#;

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

/// What one agent said to another, as it arrives in the recipient's pane.
///
/// The agents talk to each other, and this is the whole of the transport: the
/// message is typed into the composer and submitted, so it reaches the agent
/// as a turn rather than as something it has to go and look for.
///
/// It carries the id an answer names, because `reply` is what sends one back
/// to whoever asked — and the sender is named by its seat and its skills,
/// which is the only thing about a generic agent that means anything to the
/// reader.
const INCOMING_MESSAGE: &str = r#"Message from your {from}, id {message_id}:

{body}

Answer it with `reply`, on that id. Go on with your work after."#;

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

/// Resume briefing of an author with a round of requested changes, wherever
/// they were written.
///
/// One round can come from the reviewers Ariadne started, and one from the
/// people reading a published pull or merge request; `{feedback}` carries
/// whichever it is, each entry under a heading naming who wrote it. What to do
/// with a verdict is the author's playbook to say, not this text's; what
/// this text says is what this round asks of the author, and a point it
/// will not act on is answered as surely as one it will.
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
const LANDING_DIRECT: &str = r#"# Land task: {task_title}

Approved. Squash {branch} onto {base_branch} in {repo_path}. `<remote>` is what `git -C {repo_path} remote -v` names, if anything.

1. `git -C {repo_path} fetch <remote> {base_branch}`. Then `merge --ff-only <remote>/{base_branch}` there, if it is on {base_branch}. Else `fetch <remote> {base_branch}:{base_branch}`.
2. `git rebase {base_branch}` in your worktree. Conflicts are yours.
3. `git reset --soft {base_branch} && git commit`. One commit lands. Give it a Conventional Commits subject and a body: what changed and why.
4. `git -C {repo_path} merge --ff-only {branch}`. Refused because the base moved: back to step 1.
5. `git -C {repo_path} push <remote> {base_branch}`. Push first: `finish_task` ends the task and the cleanup takes your worktree.
6. `finish_task` with `git -C {repo_path} rev-parse {base_branch}`."#;

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

/// Initial briefing of a reviewer session: the task, the round, and the branch
/// its worktree is pinned to.
const REVIEWER_BRIEFING: &str = r#"# Review task: {task_title} (round {review_round})

{task_description}

## Context
- Goal: {goal_title}
- Branch: {branch} onto {base_branch}
- Repo: {repo_path}
- Author's summary: {summary}"#;

/// What a reviewer that owes a verdict is picked up with, in both situations
/// there are: a later round, where the author revised the change under its
/// worktree, and a round it has simply gone quiet in. Either way the diff it
/// last read may be stale and the verdict is still outstanding.
const REVIEWER_RESUME: &str = r#"Round {review_round} of "{task_title}" needs your verdict. {branch} can carry new commits: read it again with `get_diff`.

Summary: {summary}"#;

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
    /// it: what a message looks like when it lands in a pane. Every seat is
    /// briefed with that one — anybody can be written to — and it is the
    /// transport for a thing no text could carry before, so the total went to
    /// 1500 rather than the kinds being squeezed to fit it.
    #[test]
    fn size_caps_hold() {
        const KIND_TOTAL: usize = 1500;
        // Three now rather than two: the ending that lands nothing used to be
        // counted apart, because a repository could rewrite the other two and
        // never that one. Nothing rewrites any of them now, so they are one
        // set, and the total is the two plus the third at its own cap.
        const LANDING_TOTAL: usize = 2570;
        const GRAND_TOTAL: usize = 8000;

        // A cap per seat, not one for the three: the orchestrator alone
        // carries the conversation with the user, and the two that never grew
        // stay where they were. It was raised from 1500 to 1600 when staffing
        // arrived: the orchestrator now picks the skills of every agent it
        // creates, which is a step of the playbook and not a rewording of
        // one. It went to 1650 when the agents got a channel: staying open to
        // them for the whole goal is a step of its own, and so is showing the
        // user what each agent runs on before anything starts.
        //
        // The author's and the reviewer's went to 1010 for the same channel:
        // one step each about asking and answering, which is a thing neither
        // could do before rather than a rewording of a thing it could.
        let system_cap = |seat: Seat| match seat {
            Seat::Orchestrator => 1650,
            Seat::Author | Seat::Reviewer => 1010,
        };
        let cap = |kind: PromptKind| match kind {
            PromptKind::OrchestratorResume | PromptKind::AuthorResume | PromptKind::ReviewerResume => {
                200
            }
            _ => 300,
        };
        let landing_cap = |landing: Landing| match landing {
            Landing::Merge => 880,
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
            (Seat::Orchestrator, "It starts every task and ends planning"),
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

    /// A round of requested changes asks the author for two things, and
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
    /// instruction are templates like the briefings that start a session — and
    /// the way that stays readable is that each rule lives in the layer that
    /// needs it. A rule restated in a second one is a rule that goes stale in
    /// one of them.
    ///
    /// The landing briefings count as one place between them all: a
    /// repository has one merge strategy, and a session has one seat, so the
    /// author of a task is handed one of them and the orchestrator of a goal
    /// another. No session ever reads two.
    #[test]
    fn each_rule_is_stated_in_exactly_one_briefing() {
        // What a published branch may be done to, what ends a piece of
        // engineering work, and what each of the three calls that move a task
        // along is *for* — named elsewhere, explained here.
        for marker in [
            "git merge --no-edit",
            "push plainly",
            "--ff-only",
            "Call `request_review` with one short summary",
            "Call `submit_verdict` once per round",
            "It starts every task and ends planning",
        ] {
            let places = all_defaults()
                .into_iter()
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
    /// exist, and agreeing is what starts them
    /// (`plan_finalize.rs::the_tasks_of_a_plan_wait_for_the_yes_that_finalizes_it`).
    ///
    /// A phase out of order is an orchestrator that staffs reviewers nobody
    /// asked for, or that starts a plan the user has not seen.
    #[test]
    fn the_orchestrator_playbook_asks_before_it_plans_and_plans_before_it_starts() {
        let prompt = default_system_prompt(Seat::Orchestrator);
        let mut at = 0;
        for phase in [
            "Read the goal. Explore its repositories.",
            "Ask the user about every unclear point",
            "Write one question in your turn text.",
            "Wait for the answer in the terminal.",
            "Split the goal into tasks",
            "Staff one author per task with `create_task`",
            "Ask the user which tasks are worth a review",
            "Ask the user how each task ends",
            "Revise them until they write an explicit yes.",
            "Call `finalize_plan`",
            "Stay up for the rest of the goal.",
        ] {
            let found = prompt[at..]
                .find(phase)
                .unwrap_or_else(|| panic!("the orchestrator prompt has no \"{phase}\" after {at}"));
            at += found + phase.len();
        }

        // And the yes gates the start, in as many words.
        assert!(prompt.contains("Call it no earlier."), "{prompt}");
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
            assert!(resume.contains(phase), "the orchestrator resume and \"{phase}\"");
        }
    }

    /// `finalize_plan` is what the orchestrator calls once the plan is
    /// written, and it is the only call there is about a plan: every
    /// orchestrator default names it, and a default naming any other would be
    /// briefing an agent to make a call the daemon does not answer.
    #[test]
    fn the_orchestrator_is_briefed_with_finalize_plan_and_no_other_plan_call() {
        for text in [
            default_system_prompt(Seat::Orchestrator),
            default_prompt_text(PromptKind::OrchestratorResume),
        ] {
            assert!(text.contains("`finalize_plan`"), "{text}");
        }
        for (name, text) in all_defaults() {
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
    #[test]
    fn the_orchestrator_is_told_nothing_of_forges_or_landing() {
        let orchestrator = std::iter::once(default_system_prompt(Seat::Orchestrator))
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
            assert!(!orchestrator.contains(forge), "the orchestrator prompts name {forge}");
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
                skill.name.chars().all(|c| c.is_ascii_lowercase() || c == '-'),
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

    /// A skill is read on demand rather than on every launch, so it is capped
    /// on its own rather than against the briefings' total. The caps are still
    /// what a rewrite fits in: moving one is a decision, not a way round a
    /// failing assertion.
    #[test]
    fn skill_size_caps_hold() {
        const PER_SKILL: usize = 1800;
        const TOTAL: usize = 20_000;

        for skill in &BUILTIN_SKILLS {
            println!("{:5}  {}", skill.document.len(), skill.name);
        }
        for skill in &BUILTIN_SKILLS {
            assert!(
                skill.document.len() <= PER_SKILL,
                "the {} skill is {} characters, over its {PER_SKILL}",
                skill.name,
                skill.document.len()
            );
        }
        let total: usize = BUILTIN_SKILLS.iter().map(|s| s.document.len()).sum();
        assert!(
            total <= TOTAL,
            "the skills total {total} characters, over {TOTAL}"
        );
    }
}
