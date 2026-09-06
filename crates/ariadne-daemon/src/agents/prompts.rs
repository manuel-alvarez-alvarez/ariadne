//! Prompt assembly.
//!
//! Every briefing an agent is started or resumed on is Ariadne's own: the
//! template of its kind is a constant of the code ([`default_prompt_text`]),
//! so a reworded briefing reaches every session at once and no profile holds
//! a lifecycle text of its own. A profile says what its agent runs as — its
//! system prompt — and nothing of the lifecycle around it.
//!
//! Rendering is lenient by construction: an unknown `{token}`, a brace that
//! never closes, an empty template — all of them render to *something*, and
//! nothing here returns an error. A mangled briefing is a bad briefing, never
//! a session that refuses to start. A `{token}` nothing here fills in is
//! caught where a template is *saved* instead — see
//! [`MergeStrategy::validate_landing_template`](ariadne_core::MergeStrategy::validate_landing_template)
//! for the one text that is still written by hand, whose allowed names are
//! the ones the landing briefing below passes.
//!
//! One briefing is not a constant: the landing procedure belongs to the
//! repository the task lands in (`Repository::landing_prompt_text`), since
//! how a change reaches a base branch is the repository's to say.

use std::path::Path;

use ariadne_core::{PromptKind, Seat};
use ariadne_store::defaults::{default_prompt_text, default_system_prompt};
use ariadne_store::{Goal, Message, Repository, Skill, Task};

/// The template `kind` is rendered from: the built-in text of that kind,
/// which every launch and every resume reads straight from the code.
pub fn template_for(kind: PromptKind) -> &'static str {
    default_prompt_text(kind)
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

/// System layer: what the seat owes, and then the index of the skills this
/// agent was staffed with.
///
/// `skills_dir` is where the documents were written
/// ([`write_skills`](super::write_skills)); each line names the file, so an
/// agent whose CLI loads no skill of its own can still open it.
pub fn system_prompt(seat: Seat, skills: &[Skill], skills_dir: Option<&Path>) -> String {
    let mut prompt = default_system_prompt(seat).trim().to_string();
    if skills.is_empty() {
        return prompt;
    }
    prompt.push_str(SKILLS_HEADER);
    for skill in skills {
        prompt.push_str(&format!("\n- {}: {}", skill.name, skill.summary()));
        if let Some(dir) = skills_dir {
            let path = dir.join(&skill.name).join("SKILL.md");
            prompt.push_str(&format!(" ({})", path.display()));
        }
    }
    prompt
}

/// What the index of an agent's skills opens with.
///
/// The index carries one line per skill and no more: the document itself is
/// on disk beside the session, and the agent reads it when it needs it. That
/// is what the agent CLIs do with a skill of their own, and it is why a broad
/// set of skills costs an agent a few lines rather than a few pages.
const SKILLS_HEADER: &str =
    "\n\nYour skills. Read the document of a skill before you do the work it covers:";

/// Initial prompt for an orchestrator session.
///
/// A repository's description is what its owner wrote it down as, so it goes
/// into the briefing right after the checkout it describes.
///
/// The briefing also carries the procedure that puts the approved spec on a
/// base branch, since the orchestrator lands that spec itself. It is the one of
/// [`default_spec_landing_prompt`] the goal's first repository calls for —
/// the checkout the orchestrator is started in, and the one its commands name.
/// A goal with no repository is not one an orchestrator is ever started for, so
/// what that case renders only has to stay readable, never to work.
pub fn orchestrator_briefing(template: &str, goal: &Goal, repos: &[Repository]) -> String {
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
    render(
        template,
        &[
            ("goal_title", &goal.title),
            ("goal_description", &goal.description),
            ("repositories", &repo_lines),
        ],
    )
}

/// What an orchestrator that has gone quiet is nudged with.
pub fn orchestrator_resume_briefing(template: &str, goal: &Goal) -> String {
    render(template, &[("goal_title", &goal.title)])
}

/// What the orchestrator of a goal under way is woken with: the goal, and the
/// lines the scheduler wrote about the tasks that need it.
pub fn goal_attention_briefing(template: &str, goal: &Goal, tasks: &str) -> String {
    render(template, &[("goal_title", &goal.title), ("tasks", tasks)])
}

/// What one agent said to another, as the recipient reads it: who wrote it,
/// the id an answer names, and what it says.
pub fn incoming_message_briefing(template: &str, message: &Message, from: &str) -> String {
    render(
        template,
        &[
            ("from", from),
            ("body", &message.body),
        ],
    )
}

/// Initial prompt for an author session.
pub fn author_briefing(
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
            ("landing", task.landing().as_str()),
            ("dependencies", &dep_lines),
        ],
    )
}

/// What an author holding unfinished work is picked up with: the session
/// that ended and is started again, and the one that has gone quiet with the
/// task still open. Both want the same thing said, so both say it here.
pub fn author_resume_briefing(template: &str, task: &Task) -> String {
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
    render(
        template,
        &[
            ("task_title", &task.title),
            ("task_description", &task.description),
            ("goal_title", &goal.title),
            ("branch", &task.branch),
            ("base_branch", &repo.base_branch),
            ("repo_path", &repo.path),
            ("summary", summary.unwrap_or("(none provided)")),
        ],
    )
}

/// What a reviewer that owes a verdict is picked up with: a task it already
/// reviewed and was asked to review again, and a review it has gone quiet in.
///
/// Its worktree may have moved under it while it was away, so what it is told
/// is that the diff it read may be stale.
pub fn reviewer_resume_briefing(template: &str, task: &Task, summary: Option<&str>) -> String {
    render(
        template,
        &[
            ("task_title", &task.title),
            ("branch", &task.branch),
            ("summary", summary.unwrap_or("(none provided)")),
        ],
    )
}

/// Resume prompt for an author with a round of requested changes.
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

/// What the author of an approved task is briefed with: the branch, the base
/// and the checkout the procedure's commands act on.
///
/// The template is the repository's own ([`Repository::landing_prompt_text`]),
/// which is the text set on it or the default of its merge strategy — so what
/// is rendered here is the one procedure the author runs, and nothing of the
/// other.
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

#[cfg(test)]
mod tests {
    use super::*;

    use ariadne_core::Landing;

    use ariadne_store::defaults::default_landing_prompt;

    fn goal() -> Goal {
        Goal {
            id: "01goalxxxxxxxxxxxxxxxxxxxx".into(),
            title: "Ship the UI".into(),
            description: "The board needs swimlanes.".into(),
            status: "planning".into(),
            agent_kind: None,
            model: None,
            effort: None,
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

    fn message() -> Message {
        Message {
            id: "01msgxxxxxxxxxxxxxxxxxxxxx".into(),
            goal_id: "01goalxxxxxxxxxxxxxxxxxxxx".into(),
            task_id: Some("01taskxxxxxxxxxxxxxxxxxxxx".into()),
            kind: "question".into(),
            from_actor: "reviewer".into(),
            from_agent_id: Some("01agentxxxxxxxxxxxxxxxxxxx".into()),
            from_session: None,
            to_actor: "author".into(),
            to_agent_id: Some("01authorxxxxxxxxxxxxxxxxxx".into()),
            body: "Why is the retry unbounded?".into(),
            delivered_at: None,
            created_at: "2026-01-01T00:00:00Z".into(),
        }
    }

    fn task() -> Task {
        Task {
            id: "01taskxxxxxxxxxxxxxxxxxxxx".into(),
            goal_id: "01goalxxxxxxxxxxxxxxxxxxxx".into(),
            repo_id: "01repoxxxxxxxxxxxxxxxxxxxx".into(),
            title: "Render prompts from the database".into(),
            description: "Read them from the store.".into(),
            status: "in_progress".into(),
            branch: "render-prompts-from-the-database-xxxxxx".into(),
            landing: "merge".into(),
            worktree_path: Some("/worktrees/task-eng".into()),
            stalled: 0,
            merge_commit: None,
            pr_url: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
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
                PromptKind::OrchestratorBriefing => {
                    orchestrator_briefing(&template, &goal, std::slice::from_ref(&repo))
                }
                PromptKind::OrchestratorResume => orchestrator_resume_briefing(&template, &goal),
                PromptKind::GoalAttention => {
                    goal_attention_briefing(&template, &goal, "- one task failed")
                }
                PromptKind::IncomingMessage => {
                    incoming_message_briefing(&template, &message(), "your reviewer")
                }
                PromptKind::AuthorBriefing => author_briefing(&template, &task, &goal, &repo, &[]),
                PromptKind::AuthorResume => author_resume_briefing(&template, &task),
                PromptKind::ChangesRequested => changes_requested_briefing(&template, &feedback),
                PromptKind::ReviewerBriefing => {
                    reviewer_briefing(&template, &task, &goal, &repo, Some("done"))
                }
                PromptKind::ReviewerResume => {
                    reviewer_resume_briefing(&template, &task, Some("done"))
                }
            };
            assert!(
                !rendered.contains('{'),
                "the {} briefing left a placeholder of its own unfilled: {rendered}",
                kind.as_str()
            );
        }

        // And the landing briefing, whose allowed names belong to the ending
        // rather than to a kind.
        let template = Landing::LANDING_PLACEHOLDERS
            .iter()
            .map(|name| format!("{{{name}}}"))
            .collect::<Vec<_>>()
            .join("\n");
        let rendered = landing_briefing(&template, &task, &repo);
        assert!(
            !rendered.contains('{'),
            "the landing briefing left a placeholder of its own unfilled: {rendered}"
        );
    }

    /// One kind's rendering: what its briefing produced, and the values it was
    /// given, name by name.
    type Rendering<'a> = (PromptKind, String, Vec<(&'a str, &'a str)>);

    /// Every default briefing is its own template with this task's values put
    /// in: what a session is briefed with is the text the store ships,
    /// placeholder for placeholder, so a template edited by mistake is caught
    /// here rather than in a live session. The prose itself is the store's to
    /// state — spelling it out again here would only pin a copy of it.
    #[test]
    fn every_default_briefing_is_its_template_with_the_values_put_in() {
        let (task, goal, repo) = (task(), goal(), repo());
        let deps = vec![Task {
            title: "Store: per-profile prompts".into(),
            status: "finished".into(),
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
        let repo_line = format!("- {} (base branch: {})", repo.path, repo.base_branch);
        let attention = "- Render prompts (01task) failed".to_string();

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
                PromptKind::OrchestratorBriefing,
                orchestrator_briefing(
                    default(PromptKind::OrchestratorBriefing),
                    &goal,
                    std::slice::from_ref(&repo),
                ),
                vec![
                    ("goal_title", &goal.title),
                    ("goal_description", &goal.description),
                    ("repositories", &repo_line),
                ],
            ),
            (
                PromptKind::OrchestratorResume,
                orchestrator_resume_briefing(default(PromptKind::OrchestratorResume), &goal),
                vec![("goal_title", &goal.title)],
            ),
            (
                PromptKind::GoalAttention,
                goal_attention_briefing(default(PromptKind::GoalAttention), &goal, &attention),
                vec![("goal_title", &goal.title), ("tasks", &attention)],
            ),
            (
                PromptKind::AuthorBriefing,
                author_briefing(
                    default(PromptKind::AuthorBriefing),
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
                    ("landing", "merge"),
                    ("dependencies", &dep_lines),
                ],
            ),
            (
                PromptKind::AuthorResume,
                author_resume_briefing(default(PromptKind::AuthorResume), &task),
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
                    ("task_title", &task.title),
                    ("branch", &task.branch),
                    ("summary", "I rewrote the thing."),
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

        // And the landing briefing of each ending, the same way: the built-in
        // text with this task's values put in.
        let landing_values = vec![
            ("task_title", task.title.as_str()),
            ("branch", task.branch.as_str()),
            ("base_branch", repo.base_branch.as_str()),
            ("repo_path", repo.path.as_str()),
        ];
        for landing in Landing::ALL {
            let task = Task {
                landing: landing.as_str().into(),
                ..task.clone()
            };
            let template = default_landing_prompt(landing);
            let rendered = landing_briefing(task.landing_prompt_text(), &task, &repo);
            assert_eq!(
                rendered,
                filled(template, &landing_values),
                "the {} landing briefing, substituted",
                landing.as_str()
            );
            assert!(
                !rendered.contains('{'),
                "the {} landing briefing left a placeholder unfilled: {rendered}",
                landing.as_str()
            );
        }
    }

    /// And the values themselves are the ones the daemon builds: the lists it
    /// formats, the headings a briefing opens on, and the stand-in for a
    /// summary an author never wrote.
    #[test]
    fn the_briefings_carry_the_values_the_daemon_builds() {
        let (task, goal, repo) = (task(), goal(), repo());
        let deps = vec![Task {
            title: "Store: per-profile prompts".into(),
            status: "finished".into(),
            branch: "store-per-profile-prompts-xxxxxx".into(),
            ..task.clone()
        }];
        let author = author_briefing(
            default(PromptKind::AuthorBriefing),
            &task,
            &goal,
            &repo,
            &deps,
        );
        assert!(author.starts_with(&format!("# Task: {}", task.title)));
        assert!(
            author.contains(&format!(
                "- {} ({}, branch {})",
                deps[0].title, deps[0].status, deps[0].branch
            )),
            "{author}"
        );

        let reviewer = reviewer_briefing(
            default(PromptKind::ReviewerBriefing),
            &task,
            &goal,
            &repo,
            None,
        );
        assert!(reviewer.starts_with(&format!("# Review task: {}", task.title)));
        assert!(reviewer.contains("- Author's summary: (none provided)"));

        let feedback = vec![("reviewer 01a".to_string(), "Split it.".to_string())];
        let changes = changes_requested_briefing(default(PromptKind::ChangesRequested), &feedback);
        assert!(
            changes.contains("### From reviewer 01a\nSplit it."),
            "{changes}"
        );

        let landing = landing_briefing(task.landing_prompt_text(), &task, &repo);
        assert!(landing.starts_with(&format!("# Land task: {}", task.title)));
    }

    /// The task says what its author lands with: one procedure, not three.
    /// The repository it works in has no say — a checkout and a base branch
    /// is all a repository is.
    #[test]
    fn the_task_says_what_the_author_lands_with() {
        let repo = repo();
        let merging = task();
        let publishing = Task {
            landing: "pull_request".into(),
            ..merging.clone()
        };

        let direct = landing_briefing(merging.landing_prompt_text(), &merging, &repo);
        assert!(direct.contains("git reset --soft main"), "{direct}");
        assert!(!direct.contains("gh pr"), "{direct}");

        let published = landing_briefing(publishing.landing_prompt_text(), &publishing, &repo);
        assert!(
            published.contains("gh pr create --base main"),
            "{published}"
        );
        assert!(!published.contains("reset --soft"), "{published}");

        // The branch, the base and the checkout the commands act on.
        for value in [merging.branch.as_str(), "main", "/repos/ariadne"] {
            assert!(published.contains(value), "{value}: {published}");
        }
        assert!(!published.contains('{'), "{published}");

        // And the third ending runs neither: nothing is landed, so nothing
        // about the repository is in it.
        let nothing = Task {
            landing: "none".into(),
            ..merging.clone()
        };
        let landed = landing_briefing(nothing.landing_prompt_text(), &nothing, &repo);
        assert!(landed.contains("lands nothing"), "{landed}");
        assert!(!landed.contains("gh pr"), "{landed}");
        assert!(!landed.contains("reset --soft"), "{landed}");
    }

    /// A repository is registered with a description; the orchestrator is
    /// told it, since it is the one line saying what the checkout is for. A
    /// repository without one reads exactly as it did before descriptions
    /// existed.
    #[test]
    fn the_orchestrator_is_told_what_each_repository_is() {
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
        let briefing = orchestrator_briefing(
            default(PromptKind::OrchestratorBriefing),
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

    /// A goal is never planned without a repository, and a briefing built for
    /// one all the same reads as a briefing rather than as a broken template.
    #[test]
    fn a_goal_without_a_repository_still_briefs() {
        let briefing =
            orchestrator_briefing(default(PromptKind::OrchestratorBriefing), &goal(), &[]);
        assert!(briefing.contains("# Goal: Ship the UI"), "{briefing}");
        assert!(!briefing.contains('{'), "{briefing}");
    }

    /// The orchestrator is briefed with every repository the goal works in,
    /// each with the base branch and the way it takes a change: what a task
    /// ends with is the orchestrator's to agree with the user, so it has to
    /// know what each repository does by default.
    #[test]
    fn the_orchestrator_is_briefed_with_every_repository_and_its_base_branch() {
        let other = Repository {
            path: "/repos/web".into(),
            base_branch: "trunk".into(),
            ..repo()
        };
        let briefing = orchestrator_briefing(
            default(PromptKind::OrchestratorBriefing),
            &goal(),
            &[repo(), other],
        );
        assert!(
            briefing.contains("- /repos/ariadne (base branch: main)"),
            "{briefing}"
        );
        assert!(
            briefing.contains("- /repos/web (base branch: trunk)"),
            "{briefing}"
        );
        assert!(!briefing.contains('{'), "{briefing}");
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
        let briefing = author_briefing(
            default(PromptKind::AuthorBriefing),
            &task,
            &goal,
            &repo,
            &[],
        );
        assert!(briefing.contains("- Worktree (your cwd): <worktree>"));
        assert!(briefing.contains("- Finished dependencies:\nnone"));
    }
}
