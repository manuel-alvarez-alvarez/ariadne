//! Built-in default prompts.
//!
//! The one place a default text lives. A profile runs on these constants until
//! somebody sets a prompt of its own, and
//! [`Store::reset_profile_prompt`](crate::Store::reset_profile_prompt) puts it
//! back on them by dropping what was set — nothing is ever copied into the
//! database, so rewriting a text here reaches every profile that never edited
//! it. A repository's landing briefing works the same way, off its merge
//! strategy rather than a role ([`default_landing_prompt`]).
//!
//! Each rule is written once, in the layer it belongs to. A system prompt
//! states what a role owes, from its first read to the call that ends its
//! turn. A briefing template carries the values of one goal, task or round and
//! whatever is only true of this moment — a new round's feedback, the landing
//! procedure — and nothing of the playbook that already reached the agent. A
//! resume is a nudge: where the work stands and what ends it. What every
//! session is told alike — that Ariadne is reached through its MCP tools, that
//! nobody is there to be asked, and how few turns to take — is the MCP
//! server's `instructions`, which every session already receives, and appears
//! in no prompt here. What each role does when it cannot go on is one line of
//! its own: the planner assumes, the engineer gives the task up, the reviewer
//! asks for changes.
//!
//! Every text here is written in ASD-STE100 Simplified Technical English: one
//! instruction to a sentence, the imperative for an instruction, the active
//! voice, sentences that stay short, one meaning per word, a list for a
//! sequence of steps. It is what an agent misreads least and pays fewest
//! tokens for, and each playbook holds the agent to it in turn — the planner
//! for its task descriptions, the engineer for its summaries, commit text and
//! failure reasons, the reviewer for its verdicts. That the rule holds for
//! every role, and for every word an agent writes, is the MCP server's
//! session rules to say, so `STE` is all a text here spells.
//!
//! The texts are kept small on purpose, and `size_caps_hold` below is what
//! keeps them that way; `every_default_text_is_simplified_technical_english`
//! is what keeps the sentences short.

use ariadne_core::{MergeStrategy, PromptKind, Role};

/// A profile Ariadne seeds into an empty database: one per role, on the
/// auto-resolved agent CLI (no agent kind, no model) and on every default
/// prompt of its role. The ids are fixed so they stay recognizable; deleting a
/// built-in is allowed and permanent.
pub struct BuiltinProfile {
    pub id: &'static str,
    pub name: &'static str,
    pub role: Role,
}

pub const BUILTIN_PROFILES: [BuiltinProfile; 3] = [
    BuiltinProfile {
        id: "00000000000000000000000001",
        name: "Planner",
        role: Role::Planner,
    },
    BuiltinProfile {
        id: "00000000000000000000000002",
        name: "Engineer",
        role: Role::Engineer,
    },
    BuiltinProfile {
        id: "00000000000000000000000003",
        name: "Reviewer",
        role: Role::Reviewer,
    },
];

/// The system prompt a profile of `role` runs on while it has none of its own.
pub fn default_system_prompt(role: Role) -> &'static str {
    match role {
        Role::Planner => PLANNER_SYSTEM_PROMPT,
        Role::Engineer => ENGINEER_SYSTEM_PROMPT,
        Role::Reviewer => REVIEWER_SYSTEM_PROMPT,
    }
}

/// The default text of `kind`, or `None` when a profile of `role` does not own
/// that kind of prompt.
pub fn default_prompt(role: Role, kind: PromptKind) -> Option<&'static str> {
    kind.owned_by(role).then(|| default_prompt_text(kind))
}

/// The default text of `kind`, whichever role is reading it: what every
/// profile that owns the kind is briefed with until one of its own is set.
pub fn default_prompt_text(kind: PromptKind) -> &'static str {
    match kind {
        PromptKind::PlannerBriefing => PLANNER_BRIEFING,
        PromptKind::PlannerResume => PLANNER_RESUME,
        PromptKind::EngineerBriefing => ENGINEER_BRIEFING,
        PromptKind::EngineerResume => ENGINEER_RESUME,
        PromptKind::ChangesRequested => CHANGES_REQUESTED,
        PromptKind::ReviewerBriefing => REVIEWER_BRIEFING,
        PromptKind::ReviewerResume => REVIEWER_RESUME,
    }
}

/// The landing briefing a repository on `strategy` runs on while it has none
/// of its own: the whole procedure of that strategy, which is what its
/// engineer is handed once the task is approved.
///
/// One text per strategy rather than one with two halves: a repository lands
/// one way, so the engineer reads the procedure it runs and nothing of the
/// other. A repository may be given a text of its own instead
/// ([`Repository::landing_prompt_text`](crate::Repository::landing_prompt_text)),
/// and clearing it puts this back in force.
pub fn default_landing_prompt(strategy: MergeStrategy) -> &'static str {
    match strategy {
        MergeStrategy::Direct => LANDING_DIRECT,
        MergeStrategy::PullRequest => LANDING_PULL_REQUEST,
    }
}

/// Planner persona and playbook, and the one place `finalize_plan` is
/// explained: it starts every task at once, and the planner makes that call
/// itself once the plan is written.
const PLANNER_SYSTEM_PROMPT: &str = r#"You plan an Ariadne goal into a few small tasks. Never write code.

1. Read the goal. Explore its repositories. Where the goal is unclear, take the smaller reading. Write the assumption into the task.
2. Call `create_task` per task: small, mergeable alone, one repository. Write the ticket in STE: context, what to do, what not to touch, acceptance criteria. Name an engineer and one or more reviewers from `list_profiles`. Add `depends_on` only for a real dependency. The rest run together: keep them off the same code.
3. Size each slot from `list_models`: shape from `best_for` and `avoid_for`, risk from `cost`, routine from `speed`, effort from its description. Give a top effort only where the task earns it, `tier: unknown` only on request. Keep a reviewer under its engineer. Else the profile's own.
4. Call `finalize_plan` once you write the whole plan. It starts every task and ends planning. Call it no earlier."#;

/// Engineer persona and playbook: what it may touch, what it writes, and the
/// one place `request_review` is explained. Landing is its own too, but the
/// procedure belongs to the briefing that knows which repository this is.
const ENGINEER_SYSTEM_PROMPT: &str = r#"You own one Ariadne task, from its first commit to its merge. Work only in your worktree, on your task branch. Commit nothing generated or unrelated.

1. Read the task and its acceptance criteria. Where you cannot do it as written, call `fail_task` with the reason in STE.
2. Implement that task and no more. Refactor nothing on the way. Obey the repository's conventions: `AGENTS.md`, `CLAUDE.md`, `CONTRIBUTING.md`. Make small commits, their text in STE. Keep tests and linters green. Add the tests the task asks for.
3. Write no authorship trailer, no tool trailer, no mention of Ariadne. Leave signing to git.
4. Call `request_review` with one short summary in STE: what changed, why, how you verified it. Apply every verdict on the same branch and call it again. Where you disagree, say why in that summary.
5. Enough approvals, and Ariadne briefs you to land it."#;

/// Reviewer persona and playbook, and the one place the verdict rule is
/// stated: one per round, through `submit_verdict`.
const REVIEWER_SYSTEM_PROMPT: &str = r#"You review one round of one Ariadne task. An approval gates the merge: approve only what you would merge yourself. Your detached worktree holds the branch, read-only: do not edit, commit, amend or branch.

1. Read the task, its acceptance criteria and the engineer's summary. Call `get_diff` for the change. Read the code around it.
2. Verify the change here. Install what it needs. Build, test and lint in this worktree, never another.
3. Judge the change on the task and no more: correctness, edge cases, error handling, conventions, tests, clarity. Where something blocks the review, request changes and name it.
4. Call `submit_verdict` once per round. It is the verdict, and nothing else counts. Approve with a note on what you checked. Or request changes: a list of files and functions, each must-fix or optional. Write the verdict in STE."#;

/// Initial briefing of a planner session: the goal, and the numbers a plan
/// has to fit inside.
const PLANNER_BRIEFING: &str = r#"# Goal: {goal_title}

{goal_description}

## Repositories
{repositories}

## Constraints
- At most {max_tasks} tasks
- {required_approvals} approvals per task"#;

/// What a planner that has gone quiet is picked up with. The goal is still in
/// planning, so there is one thing left to do with it and one call that ends
/// it; the goal itself the session has read already.
const PLANNER_RESUME: &str =
    r#"Continue "{goal_title}". Call `create_task` for what it still needs, then `finalize_plan`."#;

/// Initial briefing of an engineer session: the task, and the values its
/// commands act on.
const ENGINEER_BRIEFING: &str = r#"# Task: {task_title}

{task_description}

## Context
- Goal: {goal_title}
- Worktree (your cwd): {worktree_path}
- Branch: {branch} onto {base_branch}, merge strategy {merge_strategy}
- Repo: {repo_path}
- Merged dependencies:
{dependencies}"#;

/// What an engineer holding unfinished work is picked up with, in both
/// situations there are: a session that ended and is being started again, and
/// one that is merely sitting idle with the task still open. Neither wants the
/// task read out to it again — it is in the worktree it is standing in.
const ENGINEER_RESUME: &str = r#"Continue "{task_title}" on {branch}. `git status` and `git log` say what the last session left. Work until the task is complete and verified."#;

/// Resume briefing of an engineer with a round of requested changes, wherever
/// they were written.
///
/// One round can come from the reviewers Ariadne started, and one from the
/// people reading a published pull or merge request; `{feedback}` carries
/// whichever it is, each entry under a heading naming who wrote it. What to do
/// with a verdict is the engineer's playbook to say, not this text's; what
/// this text says is what this round asks of the engineer, and a point it
/// will not act on is answered as surely as one it will.
const CHANGES_REQUESTED: &str = r#"A review requests changes.

{feedback}

Answer every point. Where you disagree, say why the code stays."#;

/// What the engineer of an approved task in a `direct` repository is briefed
/// with, unless the repository was given a landing briefing of its own: rebase, squash, fast-forward, so the base branch grows one commit per
/// task and its history stays linear.
///
/// The push comes before `mark_merged` because that call ends the task, and
/// the cleanup behind it takes the worktree the push would have run from.
const LANDING_DIRECT: &str = r#"# Land task: {task_title}

Approved. Squash {branch} onto {base_branch} in {repo_path}. `<remote>` is what `git -C {repo_path} remote -v` names, if anything.

1. `git -C {repo_path} fetch <remote> {base_branch}`. Then `merge --ff-only <remote>/{base_branch}` there, if it is on {base_branch}. Else `fetch <remote> {base_branch}:{base_branch}`.
2. `git rebase {base_branch}` in your worktree. Conflicts are yours.
3. `git reset --soft {base_branch} && git commit`. One commit lands. Give it a Conventional Commits subject and a body: what changed and why.
4. `git -C {repo_path} merge --ff-only {branch}`. Refused because the base moved: back to step 1.
5. `git -C {repo_path} push <remote> {base_branch}`. Push first: `mark_merged` ends the task and the cleanup takes your worktree.
6. `mark_merged` with `git -C {repo_path} rev-parse {base_branch}`."#;

/// What the engineer of an approved task in a `pull_request` repository is
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
/// reported nothing for 900 s is flagged as stalled and the relaunch follows
/// at 2700 s, while every poll counts as activity.
const LANDING_PULL_REQUEST: &str = r#"# Land task: {task_title}

Approved. Publish {branch} against {base_branch}. `<remote>` is what `git -C {repo_path} remote -v` names. github.com takes `gh`, GitLab `glab`. Neither, or `auth status` shows no account: `fail_task` with the failed check.

1. `git fetch <remote> {base_branch} && git rebase <remote>/{base_branch}`. The only rebase, and it comes before the push.
2. `git push -u <remote> {branch}`. Then `gh pr create --base {base_branch}` or `glab mr create --target-branch {base_branch}`. Title it by the repository's commit conventions. Fill its template. Call `record_pull_request` with the URL.
3. Poll it and its comments (`gh pr view`, `glab mr view`). `sleep 300` between polls, never longer in one call. Never end your turn while it is open.
4. Answer every comment. Commit a change on {branch}. Put it through `request_review`. Push it once approved. A published branch only grows: no `commit --amend`, no rebase, no forced push. If it stops merging cleanly, `git merge --no-edit <remote>/{base_branch}` and push plainly.
5. Merged: `gh pr merge --squash` or `glab mr merge --squash`. In {repo_path}, fetch and `git merge --ff-only <remote>/{base_branch}`. Then `mark_merged` with `git rev-parse {base_branch}`. Closed unmerged: `fail_task` with that."#;

/// Initial briefing of a reviewer session: the task, the round, and the branch
/// its worktree is pinned to.
const REVIEWER_BRIEFING: &str = r#"# Review task: {task_title} (round {review_round})

{task_description}

## Context
- Goal: {goal_title}
- Branch: {branch} onto {base_branch}
- Repo: {repo_path}
- Engineer's summary: {summary}"#;

/// What a reviewer that owes a verdict is picked up with, in both situations
/// there are: a later round, where the engineer revised the change under its
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
    /// system prompt of each role, the template of each prompt kind, and the
    /// landing briefing of each merge strategy.
    fn all_defaults() -> Vec<(String, &'static str)> {
        Role::ALL
            .into_iter()
            .map(|role| {
                (
                    format!("{} system prompt", role.as_str()),
                    default_system_prompt(role),
                )
            })
            .chain(
                PromptKind::ALL
                    .into_iter()
                    .map(|kind| (kind.as_str().to_string(), default_prompt_text(kind))),
            )
            .chain(
                MergeStrategy::ALL
                    .into_iter()
                    .map(|strategy| (landing_name(strategy), default_landing_prompt(strategy))),
            )
            .collect()
    }

    /// How a strategy's landing briefing is named in a failure.
    fn landing_name(strategy: MergeStrategy) -> String {
        format!("{} landing briefing", strategy.as_str())
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
    /// to 1400 (how the planner asks), to 1900 (sizing a task's model
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
    /// those six spellings are ~90 characters an engineer on either forge
    /// needs in front of it. Moving a cap is a decision to argue for, never a
    /// way round a failing assertion.
    ///
    /// Then the texts were written again in Simplified Technical English,
    /// which costs a sentence break where a semicolon used to join two
    /// instructions, and pays for it in two ways. The two lines telling an
    /// agent not to ask went, since the session rules already say it. And
    /// the English a role writes its own texts in is said on the instruction
    /// that writes them — `fail_task` with the reason in STE, commits whose
    /// text is in STE — rather than in a step of its own. The whole is 5825
    /// characters for the 5828 it was.
    ///
    /// The landings are where that English costs the most, since a step that
    /// joined three commands with commas is three sentences now: they are
    /// 2123 characters for the 2087 they were, and the briefing kinds pay it
    /// back at 1067 for 1078. The caps came down to what the rewrite fits
    /// in, the published landing's excepted.
    #[test]
    fn size_caps_hold() {
        const KIND_TOTAL: usize = 1080;
        const LANDING_TOTAL: usize = 2150;
        const GRAND_TOTAL: usize = 5850;
        const SYSTEM_PROMPT: usize = 950;

        let cap = |kind: PromptKind| match kind {
            PromptKind::PlannerResume | PromptKind::EngineerResume | PromptKind::ReviewerResume => {
                200
            }
            _ => 300,
        };
        let landing_cap = |strategy: MergeStrategy| match strategy {
            MergeStrategy::Direct => 880,
            MergeStrategy::PullRequest => 1300,
        };

        for (name, text) in all_defaults() {
            println!("{:5}  {name}", text.len());
        }

        for role in Role::ALL {
            let text = default_system_prompt(role);
            assert!(
                text.len() <= SYSTEM_PROMPT,
                "the {} system prompt is {} characters, over its {SYSTEM_PROMPT}",
                role.as_str(),
                text.len()
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
        for strategy in MergeStrategy::ALL {
            let text = default_landing_prompt(strategy);
            landings += text.len();
            assert!(
                text.len() <= landing_cap(strategy),
                "the {} is {} characters, over its {}",
                landing_name(strategy),
                text.len(),
                landing_cap(strategy)
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
        for (name, text) in all_defaults() {
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

    /// A rule that holds for one role is stated in that role's prompt, and
    /// only there: the reviewer's verdict rule and the engineer's ownership
    /// of the landing are what the other prompts are free of.
    #[test]
    fn a_role_rule_is_stated_in_its_own_prompt_alone() {
        for (owner, rule) in [
            (Role::Reviewer, "It is the verdict, and nothing else counts"),
            (Role::Engineer, "Ariadne briefs you to land it"),
            (Role::Planner, "It starts every task and ends planning"),
        ] {
            for role in Role::ALL {
                let prompt = default_system_prompt(role);
                assert_eq!(
                    prompt.matches(rule).count(),
                    usize::from(role == owner),
                    "the {} prompt and \"{rule}\"",
                    role.as_str()
                );
            }
        }
    }

    /// A round of requested changes asks the engineer for two things, and
    /// the briefing that carries the feedback is where both are asked: every
    /// point answered, and, for a point the engineer will not act on, why the
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

    /// `mark_merged` is the end of the task: the daemon cleans the worktree up
    /// behind it and the session can go with it. So whatever the engineer still
    /// has to run has to come first — the push of the base branch above all,
    /// which is the one step whose absence leaves the commit on this machine
    /// alone with nothing left to notice.
    #[test]
    fn nothing_the_engineer_still_has_to_run_comes_after_the_call_that_ends_the_task() {
        for strategy in MergeStrategy::ALL {
            let text = default_landing_prompt(strategy);
            let ends = text
                .find("`mark_merged`")
                .unwrap_or_else(|| panic!("the {} never ends the task", landing_name(strategy)));
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
                        "the {} runs {command} after mark_merged",
                        landing_name(strategy)
                    );
                }
            }
        }

        // And the reason is in the text, where the agent reading it is.
        assert!(
            default_landing_prompt(MergeStrategy::Direct).contains("Push first:"),
            "the direct briefing does not say why the push comes first"
        );
    }

    /// Each landing briefing is the procedure of one merge strategy, whole,
    /// and carries nothing of the other: the repository is on one strategy, so
    /// the engineer has neither a section to skip nor a choice to make.
    #[test]
    fn each_landing_briefing_is_one_strategy_and_nothing_of_the_other() {
        let direct = default_landing_prompt(MergeStrategy::Direct);
        let published = default_landing_prompt(MergeStrategy::PullRequest);

        // Squashed onto the base with git alone.
        for step in [
            "git rebase {base_branch}",
            "git reset --soft {base_branch}",
            "merge --ff-only {branch}",
            "git -C {repo_path} push <remote> {base_branch}",
            "Conventional Commits",
            "`mark_merged`",
        ] {
            assert!(direct.contains(step), "the direct briefing has no {step}");
        }

        // Published for a human to merge, on either forge.
        for step in [
            "auth status",
            "gh pr create",
            "gh pr view",
            "gh pr merge",
            "glab mr create",
            "glab mr view",
            "glab mr merge",
            "record_pull_request",
            "`mark_merged`",
        ] {
            assert!(
                published.contains(step),
                "the published briefing has no {step}"
            );
        }

        // The wait is a poll loop in the engineer's own session, and the cap
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
    /// The two landing briefings count as one place between them: a repository
    /// has one merge strategy, so an engineer is handed one of the two and
    /// never both.
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

    /// The constants are the templates every profile runs on, so they are
    /// also the ones a save-time check may never refuse: a default that fails
    /// validation would be a profile nobody can edit back to its own default.
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
        for strategy in MergeStrategy::ALL {
            assert_eq!(
                MergeStrategy::validate_landing_template(default_landing_prompt(strategy)),
                Ok(()),
                "the default {}",
                landing_name(strategy)
            );
        }
    }

    /// `finalize_plan` is what the planner calls once the plan is written,
    /// and it is the only call there is about a plan:
    /// every planner default names it, and a default naming any other would
    /// be briefing an agent to make a call the daemon does not answer.
    #[test]
    fn the_planner_is_briefed_with_finalize_plan_and_no_other_plan_call() {
        for text in [
            default_system_prompt(Role::Planner),
            default_prompt_text(PromptKind::PlannerResume),
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

    /// The planner hears nothing of forges or of landing: which way a
    /// repository takes a change is the repository's `merge_strategy` to say
    /// and the engineer's to act on, and a planner prompt naming one would be
    /// a second copy of that knowledge, going stale on its own.
    #[test]
    fn the_planner_is_told_nothing_of_forges_or_landing() {
        let planner = std::iter::once(default_system_prompt(Role::Planner))
            .chain(
                PromptKind::for_role(Role::Planner)
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
            assert!(!planner.contains(forge), "the planner prompts name {forge}");
        }
    }

    /// Three roles, three built-in profiles: one for each, and the ids stay
    /// what they have always been.
    #[test]
    fn one_builtin_profile_is_seeded_per_role() {
        assert_eq!(BUILTIN_PROFILES.len(), Role::ALL.len());
        for role in Role::ALL {
            assert_eq!(
                BUILTIN_PROFILES.iter().filter(|b| b.role == role).count(),
                1,
                "one {} is seeded",
                role.as_str()
            );
        }
    }
}
