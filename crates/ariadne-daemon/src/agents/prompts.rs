//! Prompt assembly from the database.
//!
//! Every prompt an agent runs on belongs to its profile: the system layer is
//! the profile's own `system_prompt` (persona and playbooks folded together),
//! the task layer is one of its briefing templates — one per
//! [`PromptKind`] — with the concrete goal, task and review values put in.
//!
//! Those templates are editable, so they are also breakable. Rendering is
//! lenient by construction: an unknown `{token}`, a brace that never closes,
//! an empty template — all of them render to *something*, and nothing here
//! returns an error. A profile with a mangled briefing gets a mangled
//! briefing, never a session that refuses to start.

use ariadne_core::PromptKind;
use ariadne_store::defaults::default_prompt;
use ariadne_store::{Goal, Profile, Repository, Store, Task};

/// The profile's own text for `kind`, falling back to the built-in default
/// when there is no row to read (deleted by hand, or a profile that predates
/// the kind).
///
/// A prompt we cannot read is never a reason to leave an agent unstarted, so
/// the failure is logged and answered with the default rather than returned.
pub async fn template_for(store: &Store, profile_id: &str, kind: PromptKind) -> String {
    match store.get_profile_prompt(profile_id, kind).await {
        Ok(prompt) => prompt.content,
        Err(e) => {
            tracing::warn!(
                profile = %profile_id,
                kind = kind.as_str(),
                error = %e,
                "no stored prompt for this profile; using the built-in default"
            );
            default_prompt(kind.role(), kind).unwrap_or_default().into()
        }
    }
}

/// Substitute `{name}` tokens in `template` from `values`.
///
/// Deliberately lenient, because the templates are the developer's to edit: a
/// `{token}` with no value travels through verbatim, so does a `{` that never
/// closes (or closes only after another `{`), and a template that is empty or
/// pure noise renders to itself. There is no error case.
pub fn render(template: &str, values: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        // A placeholder name runs to the next `}` with no `{` in between;
        // anything else is not a placeholder and is copied as it stands.
        match after.find(['{', '}']) {
            Some(end) if after.as_bytes()[end] == b'}' => {
                let name = &after[..end];
                match values.iter().find(|(k, _)| *k == name) {
                    Some((_, value)) => out.push_str(value),
                    None => {
                        out.push('{');
                        out.push_str(name);
                        out.push('}');
                    }
                }
                rest = &after[end + 1..];
            }
            _ => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// System layer: the profile's prompt, as the profile has it.
pub fn system_prompt(profile: &Profile) -> String {
    profile.system_prompt.trim().to_string()
}

/// Initial prompt for a planner session.
///
/// A repository's description is what its owner wrote it down as, so it goes
/// into the briefing right after the checkout it describes.
pub fn planner_briefing(template: &str, goal: &Goal, repos: &[Repository]) -> String {
    let repo_lines = repos
        .iter()
        .map(|r| {
            let line = format!("- {} (base branch: {})", r.path, r.base_branch);
            match r.description.as_deref().map(str::trim) {
                Some(d) if !d.is_empty() => format!("{line} — {d}"),
                _ => line,
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let max = goal
        .max_tasks
        .map_or("unbounded".to_string(), |m| m.to_string());
    let approvals = goal.required_approvals.to_string();
    render(
        template,
        &[
            ("goal_title", &goal.title),
            ("goal_description", &goal.description),
            ("repositories", &repo_lines),
            ("max_tasks", &max),
            ("required_approvals", &approvals),
        ],
    )
}

/// Initial prompt for an engineer session.
pub fn engineer_briefing(
    template: &str,
    task: &Task,
    goal: &Goal,
    repo: &Repository,
    deps: &[Task],
) -> String {
    let dep_lines = if deps.is_empty() {
        "none".to_string()
    } else {
        deps.iter()
            .map(|d| format!("- {} ({}, branch {})", d.title, d.status, d.branch))
            .collect::<Vec<_>>()
            .join("\n")
    };
    render(
        template,
        &[
            ("task_title", &task.title),
            ("task_description", &task.description),
            ("goal_title", &goal.title),
            (
                "worktree_path",
                task.worktree_path.as_deref().unwrap_or("<worktree>"),
            ),
            ("branch", &task.branch),
            ("base_branch", &repo.base_branch),
            ("repo_path", &repo.path),
            ("dependencies", &dep_lines),
        ],
    )
}

/// Initial prompt for a reviewer session.
pub fn reviewer_briefing(
    template: &str,
    task: &Task,
    goal: &Goal,
    repo: &Repository,
    summary: Option<&str>,
) -> String {
    let round = task.review_round.to_string();
    render(
        template,
        &[
            ("task_title", &task.title),
            ("review_round", &round),
            ("task_description", &task.description),
            ("goal_title", &goal.title),
            ("branch", &task.branch),
            ("base_branch", &repo.base_branch),
            ("repo_path", &repo.path),
            ("summary", summary.unwrap_or("(none provided)")),
        ],
    )
}

/// Resume prompt for a reviewer coming back to a task it already reviewed.
///
/// Its worktree moved under it while it was away, so the first thing it is
/// told is that what it read last round is stale — and which round the verdict
/// it now owes belongs to, since reviews are recorded per round.
pub fn reviewer_resume_briefing(template: &str, task: &Task, summary: Option<&str>) -> String {
    let round = task.review_round.to_string();
    render(
        template,
        &[
            ("review_round", &round),
            ("task_title", &task.title),
            ("branch", &task.branch),
            ("summary", summary.unwrap_or("(none provided)")),
        ],
    )
}

/// Resume prompt for an engineer after change requests.
pub fn changes_requested_briefing(template: &str, feedback: &[(String, String)]) -> String {
    let items = feedback
        .iter()
        .map(|(who, body)| format!("### From {who}\n{body}"))
        .collect::<Vec<_>>()
        .join("\n\n");
    render(template, &[("feedback", &items)])
}

/// Resume prompt instructing the engineer to merge.
pub fn merge_briefing(template: &str, task: &Task, repo: &Repository) -> String {
    render(
        template,
        &[
            ("base_branch", &repo.base_branch),
            ("repo_path", &repo.path),
            ("branch", &task.branch),
            ("task_title", &task.title),
        ],
    )
}

#[cfg(test)]
mod tests {
    use ariadne_core::Role;

    use super::*;

    fn goal() -> Goal {
        Goal {
            id: "01goalxxxxxxxxxxxxxxxxxxxx".into(),
            title: "Ship the UI".into(),
            description: "The board needs swimlanes.".into(),
            status: "planning".into(),
            max_tasks: Some(4),
            required_approvals: 2,
            planner_profile_id: "01plannerxxxxxxxxxxxxxxxxx".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
        }
    }

    fn repo() -> Repository {
        Repository {
            id: "01repoxxxxxxxxxxxxxxxxxxxx".into(),
            path: "/repos/ariadne".into(),
            base_branch: "main".into(),
            description: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
        }
    }

    fn task() -> Task {
        Task {
            id: "01taskxxxxxxxxxxxxxxxxxxxx".into(),
            goal_id: "01goalxxxxxxxxxxxxxxxxxxxx".into(),
            repo_id: "01repoxxxxxxxxxxxxxxxxxxxx".into(),
            title: "Render prompts from the database".into(),
            description: "Read them from `profile_prompts`.".into(),
            status: "in_progress".into(),
            engineer_profile_id: "01engineerxxxxxxxxxxxxxxxx".into(),
            branch: "ariadne/task-01taskxxxxxxxxxxxxxxxxxxxx".into(),
            worktree_path: Some("/worktrees/task-eng".into()),
            review_round: 3,
            stalled: 0,
            merge_commit: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
        }
    }

    fn default(role: Role, kind: PromptKind) -> &'static str {
        default_prompt(role, kind).expect("the role owns the kind")
    }

    #[test]
    fn placeholders_are_substituted() {
        assert_eq!(
            render("# {title}\n\nby {who}", &[("title", "Goal"), ("who", "me")]),
            "# Goal\n\nby me"
        );
    }

    #[test]
    fn an_unknown_placeholder_travels_verbatim() {
        assert_eq!(
            render("{known} and {unknown}", &[("known", "this")]),
            "this and {unknown}"
        );
    }

    #[test]
    fn an_empty_template_renders_to_nothing() {
        assert_eq!(render("", &[("task_title", "T")]), "");
    }

    #[test]
    fn a_template_without_placeholders_is_itself() {
        assert_eq!(
            render("Just read the diff.", &[("task_title", "T")]),
            "Just read the diff."
        );
    }

    /// Whatever a developer's editing leaves behind still renders: unclosed
    /// braces, stray closers, a name interrupted by another brace, empty
    /// names. Nothing panics, and nothing is silently dropped.
    #[test]
    fn broken_syntax_passes_through() {
        assert_eq!(render("{task_title", &[("task_title", "T")]), "{task_title");
        assert_eq!(render("} {task_title}", &[("task_title", "T")]), "} T");
        assert_eq!(
            render("{oops {task_title}", &[("task_title", "T")]),
            "{oops T"
        );
        assert_eq!(render("{}", &[("", "empty")]), "empty");
        assert_eq!(render("{{{{", &[]), "{{{{");
        assert_eq!(render("{ü}", &[]), "{ü}");
    }

    /// The defaults in the database say what the daemon used to say in code:
    /// an untouched engineer profile briefs its agent byte for byte as the
    /// hardcoded `format!` did.
    #[test]
    fn the_default_engineer_briefing_is_what_the_hardcoded_one_was() {
        let (task, goal, repo) = (task(), goal(), repo());
        let deps = vec![Task {
            title: "Store: per-profile prompts".into(),
            status: "merged".into(),
            branch: "ariadne/task-01depxxxxxxxxxxxxxxxxxxxxx".into(),
            ..task.clone()
        }];
        let rendered = engineer_briefing(
            default(Role::Engineer, PromptKind::EngineerBriefing),
            &task,
            &goal,
            &repo,
            &deps,
        );
        let dep_lines = deps
            .iter()
            .map(|d| format!("- {} ({}, branch {})", d.title, d.status, d.branch))
            .collect::<Vec<_>>()
            .join("\n");
        let expected = format!(
            "# Task: {}\n\n{}\n\n## Context\n- Goal: {}\n- Worktree (your cwd): {}\n\
             - Branch: {}\n- Base branch: {} (repo {})\n- Merged dependencies:\n{}\n\n\
             Implement the task on this branch, commit as you go, and call \
             `request_review` with a summary when complete.",
            task.title,
            task.description,
            goal.title,
            task.worktree_path.as_deref().unwrap(),
            task.branch,
            repo.base_branch,
            repo.path,
            dep_lines
        );
        assert_eq!(rendered, expected);
    }

    #[test]
    fn the_default_reviewer_resume_briefing_is_what_the_hardcoded_one_was() {
        let task = task();
        let rendered = reviewer_resume_briefing(
            default(Role::Reviewer, PromptKind::ReviewerResume),
            &task,
            Some("I rewrote the thing."),
        );
        let expected = format!(
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
            summary = "I rewrote the thing."
        );
        assert_eq!(rendered, expected);
    }

    /// The other four defaults, against the same `format!`s the daemon
    /// carried, so a template edited by mistake is caught here rather than in
    /// a live session.
    #[test]
    fn the_other_defaults_are_what_the_hardcoded_ones_were() {
        let (task, goal, repo) = (task(), goal(), repo());

        let repos = vec![repo.clone()];
        let repo_lines = format!("- {} (base branch: {})", repo.path, repo.base_branch);
        let expected = format!(
            "# Goal: {}\n\n{}\n\n## Repositories\n{}\n\n## Constraints\n\
             - Maximum number of tasks: {}\n- Approvals required per task: {}\n\n\
             Discuss this goal with the user in this terminal, then break it into \
             tasks via the Ariadne MCP tools. Call `finalize_plan` when done.",
            goal.title,
            goal.description,
            repo_lines,
            goal.max_tasks.unwrap(),
            goal.required_approvals
        );
        assert_eq!(
            planner_briefing(
                default(Role::Planner, PromptKind::PlannerBriefing),
                &goal,
                &repos
            ),
            expected
        );

        let expected = format!(
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
            "(none provided)"
        );
        assert_eq!(
            reviewer_briefing(
                default(Role::Reviewer, PromptKind::ReviewerBriefing),
                &task,
                &goal,
                &repo,
                None
            ),
            expected
        );

        let feedback = vec![
            (
                "reviewer 01a".to_string(),
                "Split the function.".to_string(),
            ),
            ("reviewer 01b".to_string(), "Add a test.".to_string()),
        ];
        let items = feedback
            .iter()
            .map(|(who, body)| format!("### From {who}\n{body}"))
            .collect::<Vec<_>>()
            .join("\n\n");
        let expected = format!(
            "Reviewers requested changes on your task.\n\n{items}\n\n\
             Apply the requested changes on the same branch, commit, and call \
             `request_review` again with an updated summary."
        );
        assert_eq!(
            changes_requested_briefing(
                default(Role::Engineer, PromptKind::ChangesRequested),
                &feedback
            ),
            expected
        );

        let expected = format!(
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
        );
        assert_eq!(
            merge_briefing(
                default(Role::Engineer, PromptKind::MergeInstructions),
                &task,
                &repo
            ),
            expected
        );
    }

    /// A repository is registered with a description; the planner is told it,
    /// since it is the one line saying what the checkout is for. A repository
    /// without one reads exactly as it did before descriptions existed.
    #[test]
    fn the_planner_is_told_what_each_repository_is() {
        let described = Repository {
            path: "/repos/ui".into(),
            description: Some("the web client".into()),
            ..repo()
        };
        let blank = Repository {
            path: "/repos/api".into(),
            description: Some("   ".into()),
            ..repo()
        };
        let briefing = planner_briefing(
            default(Role::Planner, PromptKind::PlannerBriefing),
            &goal(),
            &[described, blank, repo()],
        );
        assert!(
            briefing.contains("- /repos/ui (base branch: main) — the web client"),
            "{briefing}"
        );
        assert!(
            briefing.contains("- /repos/api (base branch: main)\n"),
            "a blank description adds nothing: {briefing}"
        );
        assert!(
            briefing.contains("- /repos/ariadne (base branch: main)"),
            "{briefing}"
        );
    }

    /// A dependency with no worktree still briefs: the fallbacks the daemon
    /// used to inline are part of the values now.
    #[test]
    fn missing_values_keep_their_fallbacks() {
        let (goal, repo) = (goal(), repo());
        let task = Task {
            worktree_path: None,
            ..task()
        };
        let briefing = engineer_briefing(
            default(Role::Engineer, PromptKind::EngineerBriefing),
            &task,
            &goal,
            &repo,
            &[],
        );
        assert!(briefing.contains("- Worktree (your cwd): <worktree>"));
        assert!(briefing.contains("- Merged dependencies:\nnone"));
    }
}
