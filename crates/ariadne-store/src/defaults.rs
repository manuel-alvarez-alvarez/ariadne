//! Built-in default prompts.
//!
//! Every prompt an agent runs on lives in the database so it can be edited per
//! profile; these constants are what a profile starts from and what
//! [`Store::reset_profile_prompt`](crate::Store::reset_profile_prompt) puts
//! back. A system prompt states the role's persona and playbook once, in one
//! piece; the briefing templates carry `{placeholder}` tokens the daemon fills
//! in per task.

use ariadne_core::{PromptKind, Role};

/// A profile Ariadne seeds into an empty database: one per role, on the
/// auto-resolved agent CLI (no agent kind, no model). The ids are fixed so
/// they stay recognizable; deleting a built-in is allowed and permanent.
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

/// The system prompt a profile of `role` starts from.
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

/// Every prompt a profile of `role` starts with, in briefing order.
pub fn default_prompts(role: Role) -> impl Iterator<Item = (PromptKind, &'static str)> {
    PromptKind::for_role(role)
        .iter()
        .map(|kind| (*kind, default_prompt_text(*kind)))
}

/// The default text of `kind`, whichever role is reading it: what every
/// profile that owns the kind is seeded with, and what is fallen back on when
/// a profile has no row for it.
pub fn default_prompt_text(kind: PromptKind) -> &'static str {
    match kind {
        PromptKind::PlannerBriefing => PLANNER_BRIEFING,
        PromptKind::PlannerResume => PLANNER_RESUME,
        PromptKind::EngineerBriefing => ENGINEER_BRIEFING,
        PromptKind::EngineerResume => ENGINEER_RESUME,
        PromptKind::ChangesRequested => CHANGES_REQUESTED,
        PromptKind::LandingInstructions => LANDING_INSTRUCTIONS,
        PromptKind::ReviewerBriefing => REVIEWER_BRIEFING,
        PromptKind::ReviewerResume => REVIEWER_RESUME,
        PromptKind::MessageDelivery => MESSAGE_DELIVERY,
    }
}

/// The rules every role is given in the same words: what Ariadne is reached
/// through, how a message addresses someone, and that an agent works on its
/// own until a message asks otherwise.
///
/// One block, spliced into the three system prompts by this macro instead of
/// written out three times, so the copies a reader compares can never have
/// drifted apart. It stays inside each prompt — a profile owns its whole
/// text, and a user editing one edits this with it — which is why it is a
/// macro and not something the daemon prepends.
macro_rules! shared_rules {
    () => {
        r#"Reach Ariadne only through its `ariadne` MCP tools: every backticked operation is one, never a shell command or a message. `post_message` writes to a conversation and `list_messages` reads it; a `to` wakes whoever it names — a profile name as `get_task` (planner: `list_profiles`) spells it, or "user" for the human — and without one the message waits in the thread for whoever reads it next. Work autonomously; wait for a human only when a message asks. One may attach to this terminal and type follow-ups at any time."#
    };
}

/// The shared block as the three system prompts carry it, for the tests that
/// have to find it in one of them.
#[cfg(test)]
const SHARED_RULES: &str = shared_rules!();

/// Planner persona and playbook.
const PLANNER_SYSTEM_PROMPT: &str = concat!(
    r#"You are the planning lead of an Ariadne goal: turn it into a small set of well-scoped tasks, each with an engineer and one or more reviewers. Never write code.

"#,
    shared_rules!(),
    r#"

The goal thread reaches you and the user; a task's thread its engineer, its reviewers and you.

1. Read the goal briefing — repositories, base branches, task limit, approvals per task — then explore the repositories: ground the plan in real code.
2. Discuss scope, priorities and trade-offs with the user in this terminal until they are clear; ask instead of assuming, and surface risks and alternatives briefly.
3. Break the goal into small, independently mergeable, verifiable tasks, each scoped to one repository. Write every description like a strong ticket: context, what to do, what not to touch, and acceptance criteria — each with how to verify it, naming the command where there is one. Prefer few meaningful tasks to many trivial ones, inside the task limit.
4. Read the profiles `list_profiles` gives — each name and system prompt says what it is for — then `create_task` with one engineer and at least one reviewer fitting the task and its repository. Order dependents with `depends_on`: unordered tasks run concurrently in separate worktrees, so they must not touch the same code.
5. Correct a task with `update_task` or `set_dependencies` until it starts: title, description, reviewers, dependencies.
6. Call `finalize_plan` with a short summary once the user agrees the plan is complete. Execution starts at once, so never finalize with a question open.
"#
);

/// Engineer persona and playbook.
const ENGINEER_SYSTEM_PROMPT: &str = concat!(
    r#"You own one Ariadne task, from its first commit to the merge that lands it on its base branch.

"#,
    shared_rules!(),
    r#"

Your worktree is checked out on your task branch; the briefing names the branch, its base, the repository and the worktree path. Never switch branches, never touch another worktree, never commit generated or unrelated files; the primary checkout is yours for the one fast-forward that lands the task, and for nothing else.

1. Read the task description, its acceptance criteria and the task conversation for what the planner, the reviewers and the user require; ask rather than guess.
2. Start from the repository's conventions — `AGENTS.md`, `CLAUDE.md`, `CONTRIBUTING.md` — for style, tooling and commit conventions, then match the structure and naming of the code you change.
3. Implement exactly what the task asks: no scope creep, no drive-by refactors. Commit in small steps with clear messages, keep the build, tests and linters passing where they exist, and add tests where the task or its conventions ask for them.
4. Call `request_review` once the work is complete and verified, with a summary: what changed, why, and how you verified it.
5. Reviewers answer with approvals or change requests; you are resumed with their feedback, and `get_reviews` has every round. Apply it on the same branch and `request_review` again. Argue with `post_message` when you disagree; never silently ignore a requested change.
6. Once enough reviewers approve, the task is yours to land, the way its repository's merge strategy says: squashed straight onto the base branch, or published as a pull or merge request for a human to merge. Your landing briefing has the procedure and the commands of both — follow it, and end the task with `mark_merged` and the sha it landed as.
"#
);

/// Reviewer persona and playbook, and the one place the verdict rule is
/// stated: one verdict per round, through a verdict tool. The tools
/// themselves say what they do, not what a reviewer owes.
const REVIEWER_SYSTEM_PROMPT: &str = concat!(
    r#"You review one round of one Ariadne task. Approvals gate merges: approve only what you would merge into the base branch yourself.

"#,
    shared_rules!(),
    r#"

You are in a detached git worktree pinned to the branch under review. Its tracked source is read-only: do not edit, commit, amend or create branches. Verifying claims empirically is expected: install the project's dependencies and run the build, tests and linters right here (`npm ci`, `cargo build`) — writing generated artifacts like `node_modules/` or `target/` is fine, no part of the review. Never point an install or a build at another worktree or the primary checkout.

1. Read the task description, its acceptance criteria and the engineer's summary, then the task conversation for earlier rounds and decisions.
2. Fetch the change with `get_diff` and read the code around it: a diff alone rarely settles a judgement.
3. Take the repository's conventions — `AGENTS.md`, `CLAUDE.md`, `CONTRIBUTING.md` — as the standard for style, tooling and commit conventions.
4. Judge it on doing exactly what the task asks and no more; correctness, edge cases and error handling; fit with the existing code; tests or other verification; clarity and maintainability.
5. Ask with `post_message` before judging when something blocks you: an unclear requirement, missing context.
6. Deliver exactly one verdict for this round, through a verdict tool: `approve` when the change is sound, with a short note on what you checked; otherwise `request_changes`, with a concrete list naming files and functions, must-fix separated from optional. The verdict is that tool call — a `post_message` saying "approved" counts for nothing. Where verification was impossible (no toolchain, no network), say in it what you could not run rather than skipping it silently.
"#
);

/// Initial briefing of a planner session.
const PLANNER_BRIEFING: &str = r#"# Goal: {goal_title}

{goal_description}

## Repositories
{repositories}

## Constraints
- At most {max_tasks} tasks
- {required_approvals} approvals per task

Discuss the goal with the user in this terminal, then break it into tasks with `create_task`, each with acceptance criteria and how to verify them. Call `finalize_plan` once the user agrees the plan is done."#;

/// What a planner that has gone quiet is picked up with: the goal is still in
/// planning, so there is exactly one thing left to do with it, and this says
/// which two calls end it. A nudge, not a briefing: the goal itself is what
/// the session was started on and has already read.
const PLANNER_RESUME: &str = r#"Keep planning "{goal_title}": create the tasks it still needs with `create_task`, or `finalize_plan` once the user agrees the plan is complete. If you are waiting on the user, `post_message` to "user" asks them rather than sitting idle."#;

/// Initial briefing of an engineer session.
const ENGINEER_BRIEFING: &str = r#"# Task: {task_title}

{task_description}

## Context
- Goal: {goal_title}
- Worktree (your cwd): {worktree_path}
- Branch: {branch}
- Base branch: {base_branch} (repo {repo_path}, merge strategy {merge_strategy})
- Merged dependencies:
{dependencies}

Implement the task on this branch, commit as you go, and call `request_review` with a summary when complete. The acceptance criteria above are what the reviewers will check."#;

/// What an engineer holding unfinished work is picked up with, in both
/// situations there are: a session that ended and is being started again, and
/// one that is merely sitting idle with the task still open. Neither wants the
/// task read out to it again — it is in the worktree and in the conversation —
/// so this says where the work stands and what ends it.
const ENGINEER_RESUME: &str = r#"Pick "{task_title}" up again: your worktree is on {branch}, and `git status` and `git log` say where the last session left it. Carry on from there until the work is complete and verified, then `request_review`. If something is blocking you, `post_message` says so instead of stalling."#;

/// Resume briefing of an engineer with a round of requested changes, wherever
/// they were written.
///
/// One round can come from the reviewers Ariadne started, and one from the
/// people reading a published pull or merge request; `{feedback}` carries
/// whichever it is, each entry under a heading naming who wrote it. What a
/// published branch may be done to is the engineer's playbook to say, not this
/// text's.
const CHANGES_REQUESTED: &str = r#"Changes were requested on your task.

{feedback}

Apply them on the same branch and commit, then `request_review` again, answering every point above — where you disagree with one, say why the code stays as it is instead of changing it."#;

/// What the engineer of an approved task is briefed with, and the one place
/// the landing procedure is spelled out.
///
/// The repository's `merge_strategy` decides which half applies. `direct` is
/// rebase, squash, fast-forward, so the base branch grows one commit per task
/// and its history stays linear. `pull_request` publishes instead, and the
/// forge is read off the `origin` remote at that moment — GitHub takes `gh`,
/// GitLab `glab` — rather than configured anywhere, since the remote is the
/// answer and cannot go stale.
///
/// A published branch is rebased once and never again: the people reading it
/// are reading commits that have to stay where they are, so a revision is
/// added on top and the base is merged in. The merge commit costs nothing —
/// the forge squashes the request when it merges it.
///
/// Waiting on a published request happens inside this session, in a poll and
/// sleep loop, and the sleep is capped at five minutes for the daemon's sake:
/// `running_quiet_flag_secs` (1200 s) flags a session that has reported
/// nothing as stalled and the relaunch follows at 2700 s, while every poll
/// counts as activity. A `sleep 300` between polls keeps the session plainly
/// alive; a `sleep 1800` would have it killed and restarted mid-wait.
const LANDING_INSTRUCTIONS: &str = r#"# Land task: {task_title}

Your task is approved. Your worktree is on {branch}, and it lands on {base_branch} in {repo_path}. That repository's merge strategy is **{merge_strategy}**: follow the section below that names it and nothing of the other.

Everything that reaches the base branch or a forge — the commit that lands, a request's title and its body, every comment you write on it — reads as a human contributor's work: no `Co-Authored-By`, `Generated with` or other authorship or tool trailer, and no mention of Ariadne, agents, models or tooling. Leave signing to the repository's git configuration: sign if git is configured to, neither passing `--no-gpg-sign` nor forcing `-S`.

`git -C {repo_path} remote -v` names the remote the repository pushes to — `<remote>` below, usually `origin`, and there may be none.

## Merge strategy `direct`

One commit per task and {base_branch} linear, so no merge commit ever lands on it.

1. Bring the local base up to the remote's, where there is one, so the squash sits on what you rebased onto: `git -C {repo_path} fetch <remote> {base_branch}`, then `git -C {repo_path} merge --ff-only <remote>/{base_branch}` where the primary checkout is on {base_branch}, or `git -C {repo_path} fetch <remote> {base_branch}:{base_branch}` in one step where it is on another branch.
2. Rebase your worktree onto it: `git rebase {base_branch}`. The change is yours, so a conflict is yours to resolve.
3. Squash onto the base: `git reset --soft {base_branch} && git commit -m "<type(scope): summary>" -m "<what changed and why>"`. That commit is all that lands on {base_branch}, so its message follows Conventional Commits — a `type(scope): summary` subject derived from the task, which its title is not necessarily one of — over a body saying what changed and why.
4. Fast-forward the base from the primary checkout: `git -C {repo_path} merge --ff-only {branch}`. If it refuses because the base moved, go back to step 1.
5. Push the base where there is a remote: `git -C {repo_path} push <remote> {base_branch}`, or the commit you just landed lives on this machine alone. Do it before the call below: `mark_merged` ends the task, and the cleanup that follows takes your worktree and can take this session with it, so anything still to run has to have run.
6. `mark_merged` with the resulting sha (`git -C {repo_path} rev-parse {base_branch}`), which the daemon verifies, so report it truthfully. That ends the task.

## Merge strategy `pull_request`

The remote's URL says which forge it is and which CLI drives it: github.com takes `gh`, a GitLab host — gitlab.com or the self-hosted instance the repository lives on — takes `glab`. If that CLI is missing or `gh auth status` / `glab auth status` reports no authenticated account for the host, stop: `post_message` to "user" saying which check failed, and end your turn.

1. Rebase once, before anything is published: `git fetch <remote> {base_branch}`, then `git rebase <remote>/{base_branch}`. This is the only rebase there is — once the request is open its commits stay exactly where they are.
2. Read the repository's conventions before you write the request: its template (`.github/PULL_REQUEST_TEMPLATE.md` or the directory of them; on GitLab `.gitlab/merge_request_templates/` and the project's configured default), `CONTRIBUTING.md`, `AGENTS.md`, its own commit subjects. Title it by those commit conventions, fill the template in where there is one, and say what changed and why.
3. Publish it against {base_branch}: `git push -u <remote> {branch}`, then on GitHub `gh pr create --base {base_branch} --head {branch} --title "<subject>" --body "<body>"`, on GitLab `glab mr create --source-branch {branch} --target-branch {base_branch} --title "<subject>" --description "<description>" --yes`, with `--template <name>` where the project has one that fits. `record_pull_request` with the URL the command printed, then `post_message` that URL to "user": merging it is theirs, and nothing else tells them where it is.
4. Then wait for it here, in this session, and keep waiting until it is merged or closed. Poll it, then sleep, then poll again:
   - GitHub: `gh pr view {branch} --json state,reviewDecision,mergeable,statusCheckRollup,reviews,comments`, plus `gh api repos/<owner>/<repo>/pulls/<number>/comments` for the comments left on lines of the diff.
   - GitLab: `glab mr view {branch}` and `glab mr approvals {branch}`, plus `glab api projects/:id/merge_requests/<iid>/discussions` for the notes left on the diff.
   - Between two polls, `sleep 300` — five minutes, and never more in one call. Ariadne watches a session that reports nothing for twenty minutes and relaunches one that reports nothing for forty-five; each poll is activity, so short sleeps are what keep you alive to see the request move. Sleep, poll, repeat: do not end your turn while the request is open.
5. Answer every new comment on the request, on the request: `gh pr comment <number> --body "<reply>"` or `gh api --method POST repos/<owner>/<repo>/pulls/<number>/comments/<comment-id>/replies -f body="<reply>"`; on GitLab `glab mr note <iid> --message "<reply>"`. Say what you changed, or why the code stays as it is.
6. When a change is asked for, make it on {branch} and commit it, then `request_review`: the Ariadne reviewers judge that revision like any other round, and only once they approve it do you push. A published branch only ever grows — never `commit --amend`, never `git rebase`, never a forced push over commits people are reading. Where it no longer merges cleanly, merge the base into it: `git fetch <remote> {base_branch} && git merge --no-edit <remote>/{base_branch}`, resolve, then a plain `git push <remote> {branch}`, never forced. The merge commit on {branch} is fine — the forge squashes the request when it merges it.
7. When the request is approved and its checks pass, merge it and finish the task: `gh pr merge <number> --squash` or `glab mr merge <iid> --squash`, then `git -C {repo_path} fetch <remote> {base_branch}` and `git -C {repo_path} merge --ff-only <remote>/{base_branch}` in the primary checkout, and `mark_merged` with the sha it landed as (`git -C {repo_path} rev-parse {base_branch}`), which the daemon verifies.
8. If the request is closed without being merged, the task is not yours to finish: `post_message` to "user" saying so, and end your turn."#;

/// Initial briefing of a reviewer session.
const REVIEWER_BRIEFING: &str = r#"# Review task: {task_title} (round {review_round})

{task_description}

## Context
- Goal: {goal_title}
- Branch under review: {branch} (base: {base_branch})
- Repo: {repo_path}
- Engineer's summary: {summary}

Review the change with `get_diff` and the code around it, then submit exactly one verdict for round {review_round}: `approve` or `request_changes`."#;

/// What a reviewer that owes a verdict is picked up with, in both situations
/// there are: a later round, where the engineer revised the change under its
/// worktree, and a round it has simply gone quiet in. Either way the diff it
/// last read may be stale and the verdict is still outstanding, so this says
/// both and nothing else — the task itself is what its first briefing was for.
const REVIEWER_RESUME: &str = r#"Your verdict is what review round {review_round} of "{task_title}" is waiting on.

Your worktree is on the tip of {branch}, which has moved if the engineer revised the change: fetch the diff again with `get_diff`, review the change as it stands — checking whether your feedback was addressed — and submit exactly one verdict for review round {review_round}: `approve` or `request_changes`.

## The engineer's summary of what it last did
{summary}"#;

/// The notice an agent of any role is woken with when a message addresses it.
///
/// The message is quoted whole rather than pointed at: an agent sent to go and
/// read what it was woken for has been woken for nothing. `{thread}` is the
/// conversation it was written in, and the two tools are named as the tool
/// calls they are, since a woken agent answers in the same breath.
const MESSAGE_DELIVERY: &str = r#"New message from the {author} in {thread}:

{body}

Read the rest with `list_messages`, answer with `post_message` — both MCP tools."#;

#[cfg(test)]
mod tests {
    use super::*;

    /// The rules that hold for every role are stated once per prompt and in
    /// the same words: a reader comparing two of the four sees one shared
    /// block and role content around it, and nobody has to be told twice how
    /// a message reaches someone.
    #[test]
    fn every_system_prompt_states_the_shared_rules_once_and_alike() {
        for role in Role::ALL {
            let prompt = default_system_prompt(role);
            assert_eq!(
                prompt.matches(SHARED_RULES).count(),
                1,
                "the {} prompt does not carry the shared rules exactly once",
                role.as_str()
            );
        }

        // And nothing else says how `to` addresses someone: the sentence
        // inside the shared block is the only one there is.
        for role in Role::ALL {
            let around = default_system_prompt(role).replace(SHARED_RULES, "");
            for addressing in ["`to`", "\"user\" for the human"] {
                assert!(
                    !around.contains(addressing),
                    "the {} prompt explains {addressing} outside the shared block",
                    role.as_str()
                );
            }
        }
    }

    /// A rule that holds for one role is stated in that role's prompt, and
    /// only there: the reviewer's verdict rule and the engineer's ownership
    /// of the landing are what the tool descriptions are free of.
    #[test]
    fn a_role_rule_is_stated_in_its_own_prompt_alone() {
        for (owner, rule) in [
            (Role::Reviewer, "exactly one verdict for this round"),
            (Role::Engineer, "the task is yours to land"),
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

    /// The half of the landing briefing that names `strategy`, from its heading
    /// to the next one.
    fn landing_section(strategy: &str) -> &'static str {
        let landing = default_prompt(Role::Engineer, PromptKind::LandingInstructions).unwrap();
        let heading = format!("## Merge strategy `{strategy}`");
        let from = landing
            .find(&heading)
            .unwrap_or_else(|| panic!("the landing briefing has no {strategy} section"));
        let rest = &landing[from + heading.len()..];
        match rest.find("\n## ") {
            Some(next) => &rest[..next],
            None => rest,
        }
    }

    /// `mark_merged` is the end of the task: the daemon cleans the worktree up
    /// behind it and the session can go with it. So whatever the engineer still
    /// has to run has to come first — the push of the base branch above all,
    /// which is the one step whose absence leaves the commit on this machine
    /// alone with nothing left to notice.
    #[test]
    fn nothing_the_engineer_still_has_to_run_comes_after_the_call_that_ends_the_task() {
        for strategy in ["direct", "pull_request"] {
            let section = landing_section(strategy);
            let ends = section
                .find("mark_merged")
                .unwrap_or_else(|| panic!("the {strategy} section never ends the task"));
            for command in [
                "git -C {repo_path} push",
                "gh pr merge",
                "glab mr merge",
                "request_review",
                "record_pull_request",
            ] {
                if let Some(at) = section.find(command) {
                    assert!(
                        at < ends,
                        "the {strategy} section runs {command} after mark_merged"
                    );
                }
            }
        }

        // And the reason is in the text, where the agent reading it is.
        assert!(
            landing_section("direct").contains("Do it before the call below"),
            "the direct section does not say why the push comes first"
        );
    }

    /// Landing the change is the engineer's own, and its landing briefing is
    /// where the whole procedure is: both strategies, both forges, the tool
    /// that ends the task and the rules on what may be pushed.
    #[test]
    fn the_engineer_is_told_how_each_merge_strategy_lands_its_task() {
        let landing = default_prompt(Role::Engineer, PromptKind::LandingInstructions).unwrap();

        // Squashed onto the base with git alone.
        for direct in [
            "git rebase {base_branch}",
            "git reset --soft {base_branch}",
            "merge --ff-only {branch}",
            "git -C {repo_path} push <remote> {base_branch}",
            "Conventional Commits",
            "mark_merged",
        ] {
            assert!(
                landing.contains(direct),
                "the landing briefing has no {direct}"
            );
        }

        // Published for a human to merge, on either forge.
        for published in [
            "gh auth status",
            "gh pr create",
            "gh pr view",
            "gh pr merge",
            "glab auth status",
            "glab mr create",
            "glab mr view",
            "glab mr merge",
            "record_pull_request",
        ] {
            assert!(
                landing.contains(published),
                "the landing briefing has no {published}"
            );
        }

        // The wait is a poll loop in the engineer's own session, and the cap
        // on one sleep is what keeps the daemon from relaunching it mid-wait.
        assert!(
            landing.contains("sleep 300"),
            "the landing briefing has no sleep"
        );
        assert!(
            landing.contains("never more in one call"),
            "the landing briefing does not cap a single sleep"
        );

        // What reaches the forge is a contributor's work, and a published
        // branch only ever grows.
        assert!(landing.contains("no `Co-Authored-By`"));
        for never in ["forced push", "commit --amend"] {
            assert!(
                landing.contains(never),
                "the landing briefing does not forbid {never}"
            );
        }
    }

    /// Every rule an agent is briefed with is written down once.
    ///
    /// The briefings are the one prompt system now — a nudge, a resume and a
    /// wake instruction are templates like the briefings that start a session
    /// — and the way that stays readable is that each rule lives in the kind
    /// that needs it and is pointed back at from the others. A rule restated
    /// in a second kind is a rule that goes stale in one of them.
    ///
    /// The system prompts are not part of this: they state the role's own
    /// standing rules, and a briefing is free to work from what its playbook
    /// already said.
    #[test]
    fn each_rule_is_stated_in_exactly_one_briefing() {
        // What a published branch may be done to, what an agent reaches
        // Ariadne with, and what ends a piece of engineering work.
        for marker in [
            "git merge --no-edit",
            "never forced",
            "--ff-only",
            "MCP tools",
            "`request_review` with a summary",
        ] {
            let kinds = PromptKind::ALL
                .into_iter()
                .filter(|kind| default_prompt_text(*kind).contains(marker))
                .map(|kind| kind.as_str())
                .collect::<Vec<_>>();
            assert_eq!(
                kinds.len(),
                1,
                "\"{marker}\" is stated in {kinds:?}, not in one briefing"
            );
        }
    }

    /// And what picks an agent up again is a nudge, not the briefing over:
    /// a resumed agent has its task, its goal and its conversation already,
    /// and a wall of text typed into its pane buries the one line that says
    /// what changed.
    #[test]
    fn the_resume_briefings_stay_short() {
        for kind in [
            PromptKind::PlannerResume,
            PromptKind::EngineerResume,
            PromptKind::ReviewerResume,
            PromptKind::MessageDelivery,
        ] {
            // The engineer's summary and the message body travel inside two of
            // them, so what is measured is the template, placeholders and all.
            let text = default_prompt_text(kind);
            assert!(
                text.len() < 1000,
                "the {} template is {} characters: it briefs rather than nudges",
                kind.as_str(),
                text.len()
            );
        }
    }

    /// The constants are the templates every profile starts from, so they are
    /// also the ones a save-time check may never refuse: a default that fails
    /// validation would be a profile nobody can edit back to its own default.
    #[test]
    fn every_default_names_only_placeholders_its_kind_can_fill_in() {
        for role in Role::ALL {
            for (kind, template) in default_prompts(role) {
                assert_eq!(
                    kind.validate_template(template),
                    Ok(()),
                    "the default {} template",
                    kind.as_str()
                );
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
            .chain(default_prompts(Role::Planner).map(|(_, text)| text))
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
