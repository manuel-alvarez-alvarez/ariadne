//! Prompt assembly from the database.
//!
//! Every prompt an agent runs on belongs to its profile: the system layer is
//! the profile's own `system_prompt` (the role's persona and playbook),
//! the task layer is one of its briefing templates — one per
//! [`PromptKind`] — with the concrete goal, task and review values put in.
//!
//! Those templates are editable, so they are also breakable. Rendering is
//! lenient by construction: an unknown `{token}`, a brace that never closes,
//! an empty template — all of them render to *something*, and nothing here
//! returns an error. A profile with a mangled briefing gets a mangled
//! briefing, never a session that refuses to start. A `{token}` nothing here
//! fills in is caught where a template is *saved* instead — see
//! [`PromptKind::validate_template`], whose allowed names are the ones the
//! briefings below pass.

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

/// Initial prompt for an integrator session: the approved task, and how the
/// repository wants it landed.
///
/// `worktree_path` is the integrator's own, not the task's — the task row
/// carries the engineer's, which by now has been released so that the branch
/// can be checked out here instead.
pub fn integration_briefing(
    template: &str,
    task: &Task,
    goal: &Goal,
    repo: &Repository,
    worktree_path: &str,
) -> String {
    render(
        template,
        &[
            ("task_title", &task.title),
            ("task_description", &task.description),
            ("goal_title", &goal.title),
            ("worktree_path", worktree_path),
            ("branch", &task.branch),
            ("base_branch", &repo.base_branch),
            ("repo_path", &repo.path),
        ],
    )
}

/// Resume prompt for an integrator coming back to a task it already tried to
/// land: after a send-back the engineer revised the branch, and after a daemon
/// restart the base may have moved under it.
pub fn integration_resume_briefing(template: &str, task: &Task, repo: &Repository) -> String {
    render(
        template,
        &[
            ("task_title", &task.title),
            ("branch", &task.branch),
            ("base_branch", &repo.base_branch),
            ("repo_path", &repo.path),
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
            agent_kind: None,
            model: None,
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
            integrator_profile_id: None,
            agent_kind: None,
            model: None,
            branch: "ariadne/task-01taskxxxxxxxxxxxxxxxxxxxx".into(),
            worktree_path: Some("/worktrees/task-eng".into()),
            review_round: 3,
            stalled: 0,
            merge_commit: None,
            pr_number: None,
            pr_url: None,
            pr_relayed_comments: None,
            pr_approved_notified: 0,
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

    /// Save-time validation lets a template name exactly the placeholders
    /// `PromptKind::placeholders` lists, so every one of them has to be a
    /// value the briefing here actually passes: a template that saves cleanly
    /// must never reach an agent with a raw `{token}` in it.
    #[test]
    fn every_allowed_placeholder_is_one_a_briefing_fills_in() {
        let (task, goal, repo) = (task(), goal(), repo());
        let feedback = vec![("reviewer 01a".to_string(), "Split it.".to_string())];
        for kind in PromptKind::ALL {
            let template = kind
                .placeholders()
                .iter()
                .map(|name| format!("{{{name}}}"))
                .collect::<Vec<_>>()
                .join("\n");
            let rendered = match kind {
                PromptKind::PlannerBriefing => {
                    planner_briefing(&template, &goal, std::slice::from_ref(&repo))
                }
                PromptKind::EngineerBriefing => {
                    engineer_briefing(&template, &task, &goal, &repo, &[])
                }
                PromptKind::ChangesRequested => changes_requested_briefing(&template, &feedback),
                PromptKind::ReviewerBriefing => {
                    reviewer_briefing(&template, &task, &goal, &repo, Some("done"))
                }
                PromptKind::ReviewerResume => {
                    reviewer_resume_briefing(&template, &task, Some("done"))
                }
                PromptKind::IntegrationInstructions => {
                    integration_briefing(&template, &task, &goal, &repo, "/worktrees/task-int")
                }
                PromptKind::IntegrationResume => {
                    integration_resume_briefing(&template, &task, &repo)
                }
            };
            assert!(
                !rendered.contains('{'),
                "the {} briefing left a placeholder of its own unfilled: {rendered}",
                kind.as_str()
            );
        }
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
             tasks with `create_task`. Call `finalize_plan` when the user agrees \
             the plan is done.",
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
            "# Integrate task: {title}\n\n{description}\n\n## Context\n\
                 - Goal: {goal}\n- Worktree (your cwd): {worktree}\n\
                 - Branch: {branch}\n- Base branch: {base} (repo {repo})\n\n\
                 The reviewers approved this task. Land it on {base}, keeping \
                 that branch's history linear — one commit per task, no merge \
                 commits:\n\n\
                 1. In your worktree, rebase onto the latest base: \
                 `git fetch . && git rebase {base}`.\n\
                 2. If the rebase conflicts, do not resolve it yourself: \
                 `git rebase --abort`, then call `return_to_engineer` with a \
                 summary and a concrete list naming the conflicting files and \
                 what has to be reconciled. That ends your turn — the task goes \
                 back to the engineer, and you are woken again once the \
                 revision is approved.\n\
                 3. Squash the branch into a single commit on top of the base: \
                 `git reset --soft {base} && git commit -m \"<type(scope): summary>\" \
                 -m \"<what changed and why>\"`. That squash commit is the only \
                 one landing on {base}, so its message must:\n\
                 \x20  - follow Conventional Commits: a `type(scope): summary` \
                 subject line derived from the task — the task title, \
                 \"{title}\", is not necessarily one already — and a body \
                 explaining what changed and why;\n\
                 \x20  - carry no `Co-Authored-By`, `Generated with` or any other \
                 authorship or tool trailer;\n\
                 \x20  - leave signing to the repository's git configuration: sign \
                 if git is configured to sign, do not pass `--no-gpg-sign` or \
                 otherwise disable it, and do not force `-S` either.\n\
                 4. Fast-forward the base branch from the primary checkout: \
                 `git -C {repo} merge --ff-only {branch}`. If it refuses \
                 because the base moved, go back to step 1.\n\
                 5. Call `mark_merged` with the resulting commit sha \
                 (`git -C {repo} rev-parse {base}`).",
            title = task.title,
            description = task.description,
            goal = goal.title,
            worktree = "/worktrees/task-int",
            base = repo.base_branch,
            repo = repo.path,
            branch = task.branch,
        );
        assert_eq!(
            integration_briefing(
                default(Role::Integrator, PromptKind::IntegrationInstructions),
                &task,
                &goal,
                &repo,
                "/worktrees/task-int",
            ),
            expected
        );

        let expected = format!(
            "Pick the integration of \"{title}\" up again: the task is approved \
                 and yours to land.\n\n\
                 Your worktree is on {branch}, which has moved since you last \
                 read it if the engineer revised the change. Rebase onto the \
                 latest {base}, squash into one commit following the \
                 repository's commit conventions, fast-forward the base from \
                 the primary checkout ({repo}) and call `mark_merged` with the \
                 resulting sha — the integration instructions you were briefed \
                 with spell every step out. If the rebase conflicts again, \
                 abort it and call `return_to_engineer` with the files that \
                 conflicted and what has to be reconciled.",
            title = task.title,
            branch = task.branch,
            base = repo.base_branch,
            repo = repo.path,
        );
        assert_eq!(
            integration_resume_briefing(
                default(Role::Integrator, PromptKind::IntegrationResume),
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
