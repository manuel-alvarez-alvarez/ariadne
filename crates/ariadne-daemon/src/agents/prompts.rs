//! Prompt assembly from the database.
//!
//! Every prompt an agent runs on belongs to its profile, and either layer may
//! be the default of the code — which is what a profile nobody edited runs on,
//! without anything having been copied into the database first.
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
use ariadne_store::defaults::default_prompt_text;
use ariadne_store::{Goal, Message, Profile, Repository, Store, Task};

/// The text `kind` is rendered from: the one set on the profile, or the
/// default of the kind, which is what the store answers while nothing is set.
///
/// A prompt we cannot read at all — a profile that has gone — is never a
/// reason to leave an agent unstarted, so the failure is logged and answered
/// with the default rather than returned.
pub async fn template_for(store: &Store, profile_id: &str, kind: PromptKind) -> String {
    match store.get_profile_prompt(profile_id, kind).await {
        Ok(prompt) => prompt.content,
        Err(e) => {
            tracing::warn!(
                profile = %profile_id,
                kind = kind.as_str(),
                error = %e,
                "this profile's prompt could not be read; using the built-in default"
            );
            default_prompt_text(kind).into()
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

/// System layer: the profile's prompt, as the profile has it — the one set on
/// it, or the default of its role.
pub fn system_prompt(profile: &Profile) -> String {
    profile.effective_system_prompt().trim().to_string()
}

/// Initial prompt for a planner session.
///
/// A repository's description is what its owner wrote it down as, so it goes
/// into the briefing right after the checkout it describes.
pub fn planner_briefing(template: &str, goal: &Goal, repos: &[Repository]) -> String {
    let repo_lines = repos
        .iter()
        .map(|r| {
            let line = format!(
                "- {} (base branch: {}, merge strategy: {})",
                r.path,
                r.base_branch,
                r.merge_strategy().as_str()
            );
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

/// What a planner that has stopped planning is nudged with.
pub fn planner_resume_briefing(template: &str, goal: &Goal) -> String {
    render(template, &[("goal_title", &goal.title)])
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
            ("merge_strategy", repo.merge_strategy().as_str()),
            ("dependencies", &dep_lines),
        ],
    )
}

/// What an engineer holding unfinished work is picked up with: the session
/// that ended and is started again, and the one that has gone quiet with the
/// task still open. Both want the same thing said, so both say it here.
pub fn engineer_resume_briefing(template: &str, task: &Task) -> String {
    render(
        template,
        &[("task_title", &task.title), ("branch", &task.branch)],
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

/// What a reviewer that owes a verdict is picked up with: a later round of a
/// task it already reviewed, and a round it has gone quiet in.
///
/// Its worktree may have moved under it while it was away, so what it is told
/// is that the diff it read may be stale — and which round the verdict it now
/// owes belongs to, since reviews are recorded per round.
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

/// Resume prompt for an engineer with a round of requested changes.
///
/// `feedback` is one entry per source, each a heading naming who asked and
/// what they wrote: the reviewers of the round, or the people reading a
/// published request, whose comments the daemon relays itself.
pub fn changes_requested_briefing(template: &str, feedback: &[(String, String)]) -> String {
    let items = feedback
        .iter()
        .map(|(who, body)| format!("### From {who}\n{body}"))
        .collect::<Vec<_>>()
        .join("\n\n");
    render(template, &[("feedback", &items)])
}

/// What the engineer of an approved task is briefed with: the branch, the base
/// and the checkout the procedure's commands act on.
///
/// Which procedure that is comes from the kind the caller read the template
/// for — [`PromptKind::landing_for`] picks it off the repository's merge
/// strategy — so the text rendered here is the one the engineer runs and
/// carries nothing of the other.
pub fn landing_briefing(template: &str, task: &Task, repo: &Repository) -> String {
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

/// What an agent of any role is woken with when a message addresses it: who
/// wrote, what they wrote, and which conversation it is in.
pub fn message_delivery(template: &str, message: &Message) -> String {
    let thread = match message.task_id {
        Some(_) => "your task conversation",
        None => "the goal's planning thread",
    };
    render(
        template,
        &[
            ("author", message.author_role().as_str()),
            ("thread", thread),
            ("body", &message.body),
        ],
    )
}

#[cfg(test)]
mod tests {
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
            merge_strategy: "direct".into(),
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
            agent_kind: None,
            model: None,
            branch: "render-prompts-from-the-database-xxxxxx".into(),
            worktree_path: Some("/worktrees/task-eng".into()),
            review_round: 3,
            stalled: 0,
            merge_commit: None,
            pr_url: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
        }
    }

    fn message() -> Message {
        Message {
            id: "01messagexxxxxxxxxxxxxxxxx".into(),
            goal_id: "01goalxxxxxxxxxxxxxxxxxxxx".into(),
            task_id: Some("01taskxxxxxxxxxxxxxxxxxxxx".into()),
            author_role: "planner".into(),
            author_session_id: None,
            recipient_kind: None,
            recipient_profile_id: None,
            body: "the scope grew: drop the second forge".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
        }
    }

    fn default(kind: PromptKind) -> &'static str {
        default_prompt_text(kind)
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
                PromptKind::PlannerResume => planner_resume_briefing(&template, &goal),
                PromptKind::EngineerBriefing => {
                    engineer_briefing(&template, &task, &goal, &repo, &[])
                }
                PromptKind::EngineerResume => engineer_resume_briefing(&template, &task),
                PromptKind::ChangesRequested => changes_requested_briefing(&template, &feedback),
                PromptKind::ReviewerBriefing => {
                    reviewer_briefing(&template, &task, &goal, &repo, Some("done"))
                }
                PromptKind::ReviewerResume => {
                    reviewer_resume_briefing(&template, &task, Some("done"))
                }
                PromptKind::LandingDirect | PromptKind::LandingPullRequest => {
                    landing_briefing(&template, &task, &repo)
                }
                PromptKind::MessageDelivery => message_delivery(&template, &message()),
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
            branch: "store-per-profile-prompts-xxxxxx".into(),
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
        let repo_line = format!(
            "- {} (base branch: {}, merge strategy: {})",
            repo.path,
            repo.base_branch,
            repo.merge_strategy().as_str()
        );
        let round = task.review_round.to_string();
        let message = message();

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
                    default(PromptKind::PlannerBriefing),
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
                PromptKind::PlannerResume,
                planner_resume_briefing(default(PromptKind::PlannerResume), &goal),
                vec![("goal_title", &goal.title)],
            ),
            (
                PromptKind::EngineerBriefing,
                engineer_briefing(
                    default(PromptKind::EngineerBriefing),
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
                    ("merge_strategy", "direct"),
                    ("dependencies", &dep_lines),
                ],
            ),
            (
                PromptKind::EngineerResume,
                engineer_resume_briefing(default(PromptKind::EngineerResume), &task),
                vec![("task_title", &task.title), ("branch", &task.branch)],
            ),
            (
                PromptKind::ChangesRequested,
                changes_requested_briefing(default(PromptKind::ChangesRequested), &feedback),
                vec![("feedback", &items)],
            ),
            (
                PromptKind::ReviewerBriefing,
                reviewer_briefing(
                    default(PromptKind::ReviewerBriefing),
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
                    default(PromptKind::ReviewerResume),
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
                PromptKind::LandingDirect,
                landing_briefing(default(PromptKind::LandingDirect), &task, &repo),
                vec![
                    ("task_title", &task.title),
                    ("branch", &task.branch),
                    ("base_branch", &repo.base_branch),
                    ("repo_path", &repo.path),
                ],
            ),
            (
                PromptKind::LandingPullRequest,
                landing_briefing(default(PromptKind::LandingPullRequest), &task, &repo),
                vec![
                    ("task_title", &task.title),
                    ("branch", &task.branch),
                    ("base_branch", &repo.base_branch),
                    ("repo_path", &repo.path),
                ],
            ),
            (
                PromptKind::MessageDelivery,
                message_delivery(default(PromptKind::MessageDelivery), &message),
                vec![
                    ("author", "planner"),
                    ("thread", "your task conversation"),
                    ("body", &message.body),
                ],
            ),
        ];

        for (kind, rendered, values) in cases {
            let template = default(kind);
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
            branch: "store-per-profile-prompts-xxxxxx".into(),
            ..task.clone()
        }];
        let engineer = engineer_briefing(
            default(PromptKind::EngineerBriefing),
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
            default(PromptKind::ReviewerBriefing),
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
        let changes = changes_requested_briefing(default(PromptKind::ChangesRequested), &feedback);
        assert!(
            changes.contains("### From reviewer 01a\nSplit it."),
            "{changes}"
        );

        let landing = landing_briefing(default(PromptKind::LandingDirect), &task, &repo);
        assert!(landing.starts_with(&format!("# Land task: {}", task.title)));
    }

    /// A repository's merge strategy picks the landing kind, and the kind is
    /// the whole of what the engineer reads: the branch, the base and the
    /// checkout its commands act on, and one procedure, not two.
    #[test]
    fn the_merge_strategy_picks_the_landing_briefing() {
        let task = task();
        let repo = repo();
        assert_eq!(
            PromptKind::landing_for(repo.merge_strategy()),
            PromptKind::LandingDirect
        );
        let published_repo = Repository {
            merge_strategy: "pull_request".into(),
            ..repo.clone()
        };
        assert_eq!(
            PromptKind::landing_for(published_repo.merge_strategy()),
            PromptKind::LandingPullRequest
        );

        let direct = landing_briefing(default(PromptKind::LandingDirect), &task, &repo);
        assert!(direct.contains("git reset --soft main"), "{direct}");
        assert!(!direct.contains("gh pr"), "{direct}");

        let published = landing_briefing(
            default(PromptKind::LandingPullRequest),
            &task,
            &published_repo,
        );
        assert!(
            published.contains("gh pr create --base main"),
            "{published}"
        );
        assert!(!published.contains("reset --soft"), "{published}");

        // The branch, the base and the checkout the commands act on.
        for value in [task.branch.as_str(), "main", "/repos/ariadne"] {
            assert!(published.contains(value), "{value}: {published}");
        }
        assert!(!published.contains('{'), "{published}");
    }

    /// The notice a woken agent reads carries the message itself, not a
    /// pointer to go and read it, and names the two calls it hands the agent.
    #[test]
    fn the_delivery_notice_quotes_the_message_and_names_its_tools() {
        let template = default(PromptKind::MessageDelivery);
        let text = message_delivery(template, &message());
        assert!(
            text.contains("New message from the planner in your task conversation"),
            "{text}"
        );
        assert!(
            text.contains("the scope grew: drop the second forge"),
            "{text}"
        );
        assert!(
            text.contains("`list_messages` reads the rest of it, `post_message` answers"),
            "{text}"
        );
        assert!(!text.contains("  "), "{text}");

        let planning = message_delivery(
            template,
            &Message {
                task_id: None,
                ..message()
            },
        );
        assert!(
            planning.contains("in the goal's planning thread"),
            "{planning}"
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
            default(PromptKind::PlannerBriefing),
            &goal(),
            &[described, blank, repo()],
        );
        assert!(
            briefing.contains(
                "- /repos/ui (base branch: main, merge strategy: direct) — the web client"
            ),
            "{briefing}"
        );
        assert!(
            briefing.contains("- /repos/api (base branch: main, merge strategy: direct)\n"),
            "a blank description adds nothing: {briefing}"
        );
        assert!(
            briefing.contains("- /repos/ariadne (base branch: main, merge strategy: direct)"),
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
            default(PromptKind::EngineerBriefing),
            &task,
            &goal,
            &repo,
            &[],
        );
        assert!(briefing.contains("- Worktree (your cwd): <worktree>"));
        assert!(briefing.contains("- Merged dependencies:\nnone"));
    }
}
