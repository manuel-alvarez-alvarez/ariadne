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
            integrator_profile_id: ariadne_store::defaults::INTEGRATOR_ID.into(),
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

    /// One kind's rendering: what its briefing produced, and the values it was
    /// given, name by name.
    type Rendering<'a> = (PromptKind, String, Vec<(&'a str, &'a str)>);

    /// Every default briefing is its own template with this task's values put
    /// in: what an untouched profile briefs its agent with is the text the
    /// store ships, placeholder for placeholder, so a template edited by
    /// mistake is caught here rather than in a live session. The prose itself
    /// is the store's to state — spelling it out again here would only pin a
    /// copy of it.
    #[test]
    fn every_default_briefing_is_its_template_with_the_values_put_in() {
        let (task, goal, repo) = (task(), goal(), repo());
        let deps = vec![Task {
            title: "Store: per-profile prompts".into(),
            status: "merged".into(),
            branch: "ariadne/task-01depxxxxxxxxxxxxxxxxxxxxx".into(),
            ..task.clone()
        }];
        let dep_lines = deps
            .iter()
            .map(|d| format!("- {} ({}, branch {})", d.title, d.status, d.branch))
            .collect::<Vec<_>>()
            .join("\n");
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
        let repo_line = format!("- {} (base branch: {})", repo.path, repo.base_branch);
        let round = task.review_round.to_string();

        // The values every kind is rendered with, and what the briefing that
        // owns it renders.
        let filled = |template: &str, pairs: &[(&str, &str)]| {
            let mut text = template.to_string();
            for (name, value) in pairs {
                text = text.replace(&format!("{{{name}}}"), value);
            }
            text
        };
        let cases: Vec<Rendering> = vec![
            (
                PromptKind::PlannerBriefing,
                planner_briefing(
                    default(Role::Planner, PromptKind::PlannerBriefing),
                    &goal,
                    std::slice::from_ref(&repo),
                ),
                vec![
                    ("goal_title", &goal.title),
                    ("goal_description", &goal.description),
                    ("repositories", &repo_line),
                    ("max_tasks", "4"),
                    ("required_approvals", "2"),
                ],
            ),
            (
                PromptKind::EngineerBriefing,
                engineer_briefing(
                    default(Role::Engineer, PromptKind::EngineerBriefing),
                    &task,
                    &goal,
                    &repo,
                    &deps,
                ),
                vec![
                    ("task_title", &task.title),
                    ("task_description", &task.description),
                    ("goal_title", &goal.title),
                    ("worktree_path", task.worktree_path.as_deref().unwrap()),
                    ("branch", &task.branch),
                    ("base_branch", &repo.base_branch),
                    ("repo_path", &repo.path),
                    ("dependencies", &dep_lines),
                ],
            ),
            (
                PromptKind::ChangesRequested,
                changes_requested_briefing(
                    default(Role::Engineer, PromptKind::ChangesRequested),
                    &feedback,
                ),
                vec![("feedback", &items)],
            ),
            (
                PromptKind::ReviewerBriefing,
                reviewer_briefing(
                    default(Role::Reviewer, PromptKind::ReviewerBriefing),
                    &task,
                    &goal,
                    &repo,
                    None,
                ),
                vec![
                    ("task_title", &task.title),
                    ("review_round", &round),
                    ("task_description", &task.description),
                    ("goal_title", &goal.title),
                    ("branch", &task.branch),
                    ("base_branch", &repo.base_branch),
                    ("repo_path", &repo.path),
                    ("summary", "(none provided)"),
                ],
            ),
            (
                PromptKind::ReviewerResume,
                reviewer_resume_briefing(
                    default(Role::Reviewer, PromptKind::ReviewerResume),
                    &task,
                    Some("I rewrote the thing."),
                ),
                vec![
                    ("review_round", &round),
                    ("task_title", &task.title),
                    ("branch", &task.branch),
                    ("summary", "I rewrote the thing."),
                ],
            ),
            (
                PromptKind::IntegrationInstructions,
                integration_briefing(
                    default(Role::Integrator, PromptKind::IntegrationInstructions),
                    &task,
                    &goal,
                    &repo,
                    "/worktrees/task-int",
                ),
                vec![
                    ("task_title", &task.title),
                    ("task_description", &task.description),
                    ("goal_title", &goal.title),
                    ("worktree_path", "/worktrees/task-int"),
                    ("branch", &task.branch),
                    ("base_branch", &repo.base_branch),
                    ("repo_path", &repo.path),
                ],
            ),
            (
                PromptKind::IntegrationResume,
                integration_resume_briefing(
                    default(Role::Integrator, PromptKind::IntegrationResume),
                    &task,
                    &repo,
                ),
                vec![
                    ("task_title", &task.title),
                    ("branch", &task.branch),
                    ("base_branch", &repo.base_branch),
                    ("repo_path", &repo.path),
                ],
            ),
        ];

        for (kind, rendered, values) in cases {
            let template = default(kind.role(), kind);
            assert_eq!(
                rendered,
                filled(template, &values),
                "the default {} briefing, substituted",
                kind.as_str()
            );
            assert!(
                !rendered.contains('{'),
                "the {} briefing left a placeholder unfilled: {rendered}",
                kind.as_str()
            );
        }
    }

    /// And the values themselves are the ones the daemon builds: the lists it
    /// formats, the headings a briefing opens on, and the stand-in for a
    /// summary an engineer never wrote.
    #[test]
    fn the_briefings_carry_the_values_the_daemon_builds() {
        let (task, goal, repo) = (task(), goal(), repo());
        let deps = vec![Task {
            title: "Store: per-profile prompts".into(),
            status: "merged".into(),
            branch: "ariadne/task-01depxxxxxxxxxxxxxxxxxxxxx".into(),
            ..task.clone()
        }];
        let engineer = engineer_briefing(
            default(Role::Engineer, PromptKind::EngineerBriefing),
            &task,
            &goal,
            &repo,
            &deps,
        );
        assert!(engineer.starts_with(&format!("# Task: {}", task.title)));
        assert!(
            engineer.contains(&format!(
                "- {} ({}, branch {})",
                deps[0].title, deps[0].status, deps[0].branch
            )),
            "{engineer}"
        );

        let reviewer = reviewer_briefing(
            default(Role::Reviewer, PromptKind::ReviewerBriefing),
            &task,
            &goal,
            &repo,
            None,
        );
        assert!(reviewer.starts_with(&format!(
            "# Review task: {} (round {})",
            task.title, task.review_round
        )));
        assert!(reviewer.contains("- Engineer's summary: (none provided)"));

        let feedback = vec![("reviewer 01a".to_string(), "Split it.".to_string())];
        let changes = changes_requested_briefing(
            default(Role::Engineer, PromptKind::ChangesRequested),
            &feedback,
        );
        assert!(
            changes.contains("### From reviewer 01a\nSplit it."),
            "{changes}"
        );

        let integration = integration_briefing(
            default(Role::Integrator, PromptKind::IntegrationInstructions),
            &task,
            &goal,
            &repo,
            "/worktrees/task-int",
        );
        assert!(integration.starts_with(&format!("# Integrate task: {}", task.title)));
        assert!(
            integration.contains("- Worktree (your cwd): /worktrees/task-int"),
            "the integrator's own worktree, not the engineer's: {integration}"
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
