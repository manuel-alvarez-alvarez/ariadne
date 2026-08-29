//! Built-in default prompts.
//!
//! The one place a default text lives. A profile runs on these constants until
//! somebody sets a prompt of its own, and
//! [`Store::reset_profile_prompt`](crate::Store::reset_profile_prompt) puts it
//! back on them by dropping what was set — nothing is ever copied into the
//! database, so rewriting a text here reaches every profile that never edited
//! it.
//!
//! Each rule is written once, in the layer it belongs to. A system prompt
//! states what a role owes, from its first read to the call that ends its
//! turn. A briefing template carries the values of one goal, task or round and
//! whatever is only true of this moment — a new round's feedback, the landing
//! procedure — and nothing of the playbook that already reached the agent. A
//! resume is a nudge: where the work stands and what ends it. What every
//! session is told alike — that Ariadne is reached through its MCP tools, how
//! a `to` addresses someone — is the MCP server's `instructions`, which every
//! session already receives, and appears in no prompt here.
//!
//! The texts are kept small on purpose, and `size_caps_hold` below is what
//! keeps them that way.

use ariadne_core::{PromptKind, Role};

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
        PromptKind::LandingDirect => LANDING_DIRECT,
        PromptKind::LandingPullRequest => LANDING_PULL_REQUEST,
        PromptKind::ReviewerBriefing => REVIEWER_BRIEFING,
        PromptKind::ReviewerResume => REVIEWER_RESUME,
    }
}

/// Planner persona and playbook, and the one place `finalize_plan` is
/// explained: it starts every task at once, and only the user's word in this
/// conversation calls for it.
const PLANNER_SYSTEM_PROMPT: &str = r#"You are the planning lead of an Ariadne goal: turn it into a small set of well-scoped tasks, each with an engineer and one or more reviewers. Never write code.

1. Explore the repositories your briefing names; settle scope and trade-offs with the user here, asking rather than assuming. Every question for them is asked where they are notified — `post_message` to "user", or your CLI's question tool where it has one (Claude Code's `AskUserQuestion`), one at a time — never as plain text you end a turn on; progress and summaries go unaddressed or to the agent they concern.
2. Break the goal into small, independently mergeable tasks, one repository each, written like a strong ticket: context, what to do, what not to touch, and acceptance criteria saying how to verify each.
3. `create_task` with an engineer and at least one reviewer out of `list_profiles`. `depends_on` is for a task that truly needs another first: unordered tasks run concurrently in separate worktrees and must not touch the same code.
4. `update_task` corrects a task until it starts.
5. After the last `create_task`, post the plan to "user" and ask whether it is complete. `finalize_plan` starts every task of the plan at once and ends planning: call it only on an explicit yes here, never on your own judgement or on answers to earlier questions. Until then, rework whatever they ask for."#;

/// Engineer persona and playbook: what it may touch, what it writes, and the
/// one place `request_review` is explained. Landing is its own too, but the
/// procedure belongs to the briefing that knows which repository this is.
const ENGINEER_SYSTEM_PROMPT: &str = r#"You own one Ariadne task, from its first commit to the merge that lands it on its base branch.

Work in your worktree, on your task branch: no other branch, no other worktree, nothing generated or unrelated committed.

1. Read the task, its acceptance criteria and its conversation for what it requires; ask rather than guess. `post_message` to "user" only when they must act or answer; progress and summaries go unaddressed or to the agent they concern.
2. Implement exactly that — no scope creep, no drive-by refactors — under the repository's conventions (`AGENTS.md`, `CLAUDE.md`, `CONTRIBUTING.md`): small commits, tests and linters green, the tests asked of you added.
3. Nothing you write carries an authorship or tool trailer or names Ariadne; leave signing to git's configuration.
4. `request_review` submits it, with a summary of what changed, why and how you verified it. Apply the verdicts on the same branch and call it again, or `post_message` to argue.
5. Enough approvals and you are briefed to land it."#;

/// Reviewer persona and playbook, and the one place the verdict rule is
/// stated: one per round, through `submit_verdict`.
const REVIEWER_SYSTEM_PROMPT: &str = r#"You review one round of one Ariadne task. Approvals gate merges: approve only what you would merge yourself.

Your detached worktree holds the branch under review, read-only: do not edit, commit, amend or branch. Verify empirically — install dependencies and run the build, tests and linters right here (`npm ci`, `cargo build`), never at another worktree.

1. Read the task, its acceptance criteria, the engineer's summary and the earlier rounds. `get_diff` fetches the change; read the code around it too.
2. Judge it on doing exactly what the task asks and no more; correctness, edge cases, error handling, conventions, tests, clarity. `post_message` asks where something blocks you — to "user" only when they must act or answer, anything else unaddressed or to the agent it concerns.
3. `submit_verdict` is the verdict, one per round and the only thing that counts: approve with a note on what you checked, or request changes as a list naming files and functions, must-fix apart from optional."#;

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
/// planning, so there is one thing left to do with it and two calls that end
/// it; the goal itself the session has read already.
const PLANNER_RESUME: &str = r#"Keep planning "{goal_title}": `create_task` for what it still needs, then post the plan to "user" and ask whether it is complete; `finalize_plan` only on their explicit yes here. Ask through `post_message` to "user" or your CLI's question tool, one question at a time, never as plain text you end a turn on."#;

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
/// task read out to it again — it is in the worktree and in the conversation.
const ENGINEER_RESUME: &str = r#"Pick "{task_title}" up again: your worktree is on {branch}, where `git status` and `git log` say what the last session left. Carry on until the work is complete and verified."#;

/// Resume briefing of an engineer with a round of requested changes, wherever
/// they were written.
///
/// One round can come from the reviewers Ariadne started, and one from the
/// people reading a published pull or merge request; `{feedback}` carries
/// whichever it is, each entry under a heading naming who wrote it. What to do
/// with a verdict is the engineer's playbook to say, not this text's.
const CHANGES_REQUESTED: &str = r#"Changes were requested on your task.

{feedback}

Answer every point: where you disagree, say why the code stays as it is."#;

/// What the engineer of an approved task in a `direct` repository is briefed
/// with: rebase, squash, fast-forward, so the base branch grows one commit per
/// task and its history stays linear.
///
/// The push comes before `mark_merged` because that call ends the task, and
/// the cleanup behind it takes the worktree the push would have run from.
const LANDING_DIRECT: &str = r#"# Land task: {task_title}

Approved. Squash {branch} onto {base_branch} in {repo_path}; `<remote>` is what `git -C {repo_path} remote -v` names, if anything.

1. `git -C {repo_path} fetch <remote> {base_branch}`, then `merge --ff-only <remote>/{base_branch}` there if that checkout is on {base_branch}, else `fetch <remote> {base_branch}:{base_branch}`.
2. `git rebase {base_branch}` in your worktree; conflicts are yours.
3. `git reset --soft {base_branch} && git commit`: that one commit is all that lands — Conventional Commits subject, body saying what changed and why.
4. `git -C {repo_path} merge --ff-only {branch}`. Refused because the base moved, back to step 1.
5. `git -C {repo_path} push <remote> {base_branch}`, or what you landed lives on this machine alone. Push first: `mark_merged` ends the task, and the cleanup behind it takes your worktree.
6. `mark_merged` with `git -C {repo_path} rev-parse {base_branch}`, which the daemon verifies."#;

/// What the engineer of an approved task in a `pull_request` repository is
/// briefed with: publish it, then see it through in this session.
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

Approved. Publish {branch} against {base_branch} for a human to merge, then see it through. `<remote>` is what `git -C {repo_path} remote -v` names; its URL picks the CLI: github.com `gh`, a GitLab host `glab`. Missing, or `auth status` shows no account — `post_message` to "user" naming the check, and stop.

1. `git fetch <remote> {base_branch} && git rebase <remote>/{base_branch}`: the only rebase there is, before anything is published.
2. `git push -u <remote> {branch}`, then `gh pr create --base {base_branch} --head {branch} --title "<subject>" --body "<body>"`, or `glab mr create --source-branch {branch} --target-branch {base_branch} --title "<subject>" --description "<body>" --yes`. Title it by the repository's commit conventions and fill in its request template; `record_pull_request` with the URL, then `post_message` it to "user".
3. Poll it and its comments (`gh pr view`, `gh api .../comments`; `glab mr view`, `glab api .../discussions`), `sleep 300` — never longer in one call — and poll again. Do not end your turn while it is open.
4. Answer new comments on the request itself. A change asked for is committed on {branch} and put through `request_review`, and pushed once the reviewers approve. A published branch only grows: never `commit --amend`, `git rebase` or a forced push; where it stops merging cleanly, `git merge --no-edit <remote>/{base_branch}` and push plainly.
5. Merged: `gh pr merge --squash` or `glab mr merge --squash`, then in {repo_path} `git fetch <remote> {base_branch} && git merge --ff-only <remote>/{base_branch}` and `mark_merged` with `git rev-parse {base_branch}` there, which the daemon verifies. Closed unmerged: `post_message` to "user", and stop."#;

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
const REVIEWER_RESUME: &str = r#"Round {review_round} of "{task_title}" is waiting on your verdict, and {branch} has moved if the engineer revised it: read it again with `get_diff`.

Its summary: {summary}"#;

#[cfg(test)]
mod tests {
    use super::*;

    /// Every default text there is, named as the test failures name it.
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
            .collect()
    }

    /// The size a prompt may grow back to, per kind, and in total.
    ///
    /// Every text here is sent to a real agent on a real turn, so what is
    /// spent on prose is not spent on the task. The caps are what stop the
    /// texts creeping back up: a rule restated in a second layer, a procedure
    /// explained twice, a closing paragraph repeating the playbook, all show
    /// up as characters.
    ///
    /// The total is over the prompt kinds — what a briefing costs per turn —
    /// with the three system prompts pinned separately and counted again in a
    /// grand total, since a session pays for one of each.
    ///
    /// What a cap is not is a reason to say a rule badly. The system-prompt
    /// one went from 900 to 1050 when the planner had to be told when *not*
    /// to end planning: a step of the lifecycle the planner has to know about
    /// is not the kind of growth these numbers are here to stop. Then from
    /// 1050 to 1200, when all three roles had to be told whom a message is
    /// for: an agent that addresses the user for a progress report puts
    /// "waiting for you" on its session, and one sentence per prompt is what
    /// the daemon cannot infer on their behalf. Then from 1200 to 1400, and
    /// the planner's resume alone from 250 to 350, when *how* to ask had to
    /// be written down beside whom to ask: a planner asking in plain text
    /// ends its turn on a question nobody is notified of, and one reading the
    /// answers to its scoping questions as consent finalizes a plan the user
    /// never saw. Both leave the user waiting on a session that is waiting on
    /// them, and neither is anything the daemon can infer. Moving one is a
    /// decision to argue for, never a way round a failing assertion.
    #[test]
    fn size_caps_hold() {
        const KIND_TOTAL: usize = 6000;
        const GRAND_TOTAL: usize = 7500;
        const SYSTEM_PROMPT: usize = 1400;

        let cap = |kind: PromptKind| match kind {
            PromptKind::LandingDirect | PromptKind::LandingPullRequest => 2000,
            PromptKind::PlannerResume => 350,
            PromptKind::EngineerResume | PromptKind::ReviewerResume => 250,
            _ => 400,
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

        let grand: usize = all_defaults().iter().map(|(_, text)| text.len()).sum();
        println!("{kinds:5}  every briefing template\n{grand:5}  every default text");
        assert!(
            grand <= GRAND_TOTAL,
            "the defaults total {grand} characters, over {GRAND_TOTAL}"
        );
    }

    /// How Ariadne is reached is the MCP server's `instructions` to say, and
    /// only its: the block that used to be pasted into all three system
    /// prompts lives in one place now, and no prompt here explains a `to` or
    /// what "user" addresses.
    #[test]
    fn no_default_repeats_what_every_session_is_told_by_the_mcp_server() {
        for (name, text) in all_defaults() {
            for shared in ["Reach Ariadne", "for the human", "MCP tool", "backticked"] {
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
            (
                Role::Reviewer,
                "one per round and the only thing that counts",
            ),
            (Role::Engineer, "you are briefed to land it"),
            (Role::Planner, "starts every task of the plan at once"),
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

    /// `mark_merged` is the end of the task: the daemon cleans the worktree up
    /// behind it and the session can go with it. So whatever the engineer still
    /// has to run has to come first — the push of the base branch above all,
    /// which is the one step whose absence leaves the commit on this machine
    /// alone with nothing left to notice.
    #[test]
    fn nothing_the_engineer_still_has_to_run_comes_after_the_call_that_ends_the_task() {
        for kind in [PromptKind::LandingDirect, PromptKind::LandingPullRequest] {
            let text = default_prompt_text(kind);
            let ends = text
                .find("`mark_merged`")
                .unwrap_or_else(|| panic!("the {} briefing never ends the task", kind.as_str()));
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
                        "the {} briefing runs {command} after mark_merged",
                        kind.as_str()
                    );
                }
            }
        }

        // And the reason is in the text, where the agent reading it is.
        assert!(
            default_prompt_text(PromptKind::LandingDirect).contains("Push first:"),
            "the direct briefing does not say why the push comes first"
        );
    }

    /// Each landing briefing is the procedure of one merge strategy, whole,
    /// and carries nothing of the other: the daemon picks the kind, so the
    /// engineer has neither a section to skip nor a choice to make.
    #[test]
    fn each_landing_briefing_is_one_strategy_and_nothing_of_the_other() {
        let direct = default_prompt_text(PromptKind::LandingDirect);
        let published = default_prompt_text(PromptKind::LandingPullRequest);

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
        for never in ["a forced push", "commit --amend"] {
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
    /// The two landing kinds count as one place between them: a repository has
    /// one merge strategy, so an engineer is handed one of the two and never
    /// both.
    #[test]
    fn each_rule_is_stated_in_exactly_one_briefing() {
        // What a published branch may be done to, what ends a piece of
        // engineering work, and what each of the three calls that move a task
        // along is *for* — named elsewhere, explained here.
        for marker in [
            "git merge --no-edit",
            "push plainly",
            "--ff-only",
            "`request_review` submits it",
            "`submit_verdict` is the verdict",
            "starts every task of the plan at once",
        ] {
            let places = all_defaults()
                .into_iter()
                .filter(|(_, text)| text.contains(marker))
                .map(|(name, _)| match name.as_str() {
                    "landing_direct" | "landing_pull_request" => "a landing briefing".to_string(),
                    other => other.to_string(),
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
    }

    /// `finalize_plan` is what the planner calls once the user has validated
    /// the plan in the thread, and it is the only call there is about a plan:
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
