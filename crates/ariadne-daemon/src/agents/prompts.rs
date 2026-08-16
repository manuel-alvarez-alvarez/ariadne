//! Role playbooks and prompt assembly.
//!
//! System layer = profile.system_prompt + role playbook (how to use the
//! Ariadne MCP tools, git etiquette). Task layer = the initial user prompt
//! with the concrete briefing.

use ariadne_core::Role;
use ariadne_store::{Goal, GoalRepo, Profile, Task};

const COMMON_PLAYBOOK: &str = r#"
## Ariadne orchestration

You are one agent inside Ariadne, an orchestrator coordinating planner,
engineer and reviewer agents over shared goals and tasks. You interact with
Ariadne exclusively through the `ariadne` MCP tools available to you. Use
`post_message` to communicate with the other agents and the user; check
`list_messages` when you need context or are asked to reconsider something.
Work autonomously: do not wait for a human unless a message explicitly asks
you to. A human may attach to your terminal at any time and type follow-ups.
"#;

const PLANNER_PLAYBOOK: &str = r#"
## Your role: planner

Discuss the goal with the user in this terminal until the breakdown is clear,
then create tasks with the `create_task` tool. For every task pick an engineer
profile and one or more reviewer profiles (`list_profiles` shows what exists)
and express ordering constraints with `depends_on`. Keep tasks small,
independently mergeable, and scoped to a single repository. When the plan is
complete and the user agrees, call `finalize_plan` with a short summary; the
daemon then starts executing tasks automatically. Do not write code yourself.
"#;

const ENGINEER_PLAYBOOK: &str = r#"
## Your role: engineer

You own this task until it is merged. You are already inside a dedicated git
worktree on the task branch; never switch branches and never touch other
worktrees. Implement the task, commit with clear messages, and when the work
is complete call `request_review` with a summary. Reviewers will respond with
approvals or change requests; you will be resumed with their feedback — apply
requested changes on the same branch and call `request_review` again. Once
Ariadne instructs you to merge, follow the merge instructions exactly and then
call `mark_merged` with the resulting merge commit sha.
"#;

const REVIEWER_PLAYBOOK: &str = r#"
## Your role: reviewer

You are in a read-only detached worktree pinned to the branch under review.
Do not commit, amend, or modify files — review only. Use `get_diff` to see
the change against the base branch, read any code you need for context, then
deliver exactly one verdict for this round: `approve` or `request_changes`
(with concrete, actionable feedback in the body). Use `post_message` for
questions to the engineer if something blocks your judgement.
"#;

pub fn playbook(role: Role) -> &'static str {
    match role {
        Role::Planner => PLANNER_PLAYBOOK,
        Role::Engineer => ENGINEER_PLAYBOOK,
        Role::Reviewer => REVIEWER_PLAYBOOK,
    }
}

/// System layer: profile prompt + shared + role playbook.
pub fn system_prompt(profile: &Profile, role: Role) -> String {
    format!(
        "{}\n{}{}",
        profile.system_prompt.trim(),
        COMMON_PLAYBOOK,
        playbook(role)
    )
}

/// Initial prompt for a planner session.
pub fn planner_briefing(goal: &Goal, repos: &[GoalRepo]) -> String {
    let repo_lines = repos
        .iter()
        .map(|r| format!("- {} (base branch: {})", r.path, r.base_branch))
        .collect::<Vec<_>>()
        .join("\n");
    let max = goal
        .max_tasks
        .map_or("unbounded".to_string(), |m| m.to_string());
    format!(
        "# Goal: {}\n\n{}\n\n## Repositories\n{}\n\n## Constraints\n\
         - Maximum number of tasks: {}\n- Approvals required per task: {}\n\n\
         Discuss this goal with the user in this terminal, then break it into \
         tasks via the Ariadne MCP tools. Call `finalize_plan` when done.",
        goal.title, goal.description, repo_lines, max, goal.required_approvals
    )
}

/// Initial prompt for an engineer session.
pub fn engineer_briefing(task: &Task, goal: &Goal, repo: &GoalRepo, deps: &[Task]) -> String {
    let dep_lines = if deps.is_empty() {
        "none".to_string()
    } else {
        deps.iter()
            .map(|d| format!("- {} ({}, branch {})", d.title, d.status, d.branch))
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        "# Task: {}\n\n{}\n\n## Context\n- Goal: {}\n- Worktree (your cwd): {}\n\
         - Branch: {}\n- Base branch: {} (repo {})\n- Merged dependencies:\n{}\n\n\
         Implement the task on this branch, commit as you go, and call \
         `request_review` with a summary when complete.",
        task.title,
        task.description,
        goal.title,
        task.worktree_path.as_deref().unwrap_or("<worktree>"),
        task.branch,
        repo.base_branch,
        repo.path,
        dep_lines
    )
}

/// Initial prompt for a reviewer session.
pub fn reviewer_briefing(
    task: &Task,
    goal: &Goal,
    repo: &GoalRepo,
    summary: Option<&str>,
) -> String {
    format!(
        "# Review task: {} (round {})\n\n{}\n\n## Context\n- Goal: {}\n\
         - Branch under review: {} (base: {})\n- Repo: {}\n\
         - Engineer's summary: {}\n\n\
         Review the change with `get_diff` and the code around it, then submit \
         exactly one verdict: `approve` or `request_changes`.",
        task.title,
        task.review_round,
        task.description,
        goal.title,
        task.branch,
        repo.base_branch,
        repo.path,
        summary.unwrap_or("(none provided)")
    )
}

/// Resume prompt for a reviewer coming back to a task it already reviewed.
///
/// Its worktree moved under it while it was away, so the first thing it is
/// told is that what it read last round is stale — and which round the verdict
/// it now owes belongs to, since reviews are recorded per round.
pub fn reviewer_resume_briefing(task: &Task, summary: Option<&str>) -> String {
    format!(
        "The engineer revised the change: this is review round {round} of \"{title}\".\n\n\
         Your worktree has been moved to the new tip of {branch}, so the diff you \
         read last round is out of date. Fetch it again with `get_diff`, review the \
         change as it stands now — checking whether the feedback you gave was \
         addressed — and submit exactly one verdict for round {round}: `approve` or \
         `request_changes`.\n\n\
         ## Engineer's summary of this revision\n{summary}",
        round = task.review_round,
        title = task.title,
        branch = task.branch,
        summary = summary.unwrap_or("(none provided)")
    )
}

/// Resume prompt for an engineer after change requests.
pub fn changes_requested_briefing(feedback: &[(String, String)]) -> String {
    let items = feedback
        .iter()
        .map(|(who, body)| format!("### From {who}\n{body}"))
        .collect::<Vec<_>>()
        .join("\n\n");
    format!(
        "Reviewers requested changes on your task.\n\n{items}\n\n\
         Apply the requested changes on the same branch, commit, and call \
         `request_review` again with an updated summary."
    )
}

/// Resume prompt instructing the engineer to merge.
pub fn merge_briefing(task: &Task, repo: &GoalRepo) -> String {
    format!(
        "Your task has been approved. Merge it now:\n\n\
         1. In your worktree, rebase onto the latest base if needed: \
         `git fetch . && git rebase {base}` (resolve conflicts if any).\n\
         2. Merge into the base branch from the primary checkout: \
         `git -C {repo} merge --no-ff {branch} -m \"merge: {title}\"`.\n\
         3. Call `mark_merged` with the merge commit sha \
         (`git -C {repo} rev-parse {base}`).",
        base = repo.base_branch,
        repo = repo.path,
        branch = task.branch,
        title = task.title
    )
}
