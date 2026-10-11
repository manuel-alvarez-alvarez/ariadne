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
//! [`PromptKind::validate_template`](ariadne_core::PromptKind::validate_template).

use std::path::Path;

use ariadne_core::{PromptKind, Seat};
use ariadne_store::defaults::{default_prompt_text, default_system_prompt};
use ariadne_store::{Goal, GoalStep, Message, PullRequest, Repository, Skill, Task};

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
pub(crate) fn render(template: &str, values: &[(&str, &str)]) -> String {
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
/// agent opens the one it needs itself.
pub(crate) fn system_prompt(seat: Seat, skills: &[Skill], skills_dir: Option<&Path>) -> String {
    with_skills(default_system_prompt(seat), skills, skills_dir)
}

/// The system layer of a review session (029): its own seat text, then the
/// index of its skill, as [`system_prompt`] builds a task seat's.
pub(crate) fn pull_request_system_prompt(skills: &[Skill], skills_dir: Option<&Path>) -> String {
    with_skills(
        ariadne_store::defaults::pull_request_system_prompt(),
        skills,
        skills_dir,
    )
}

fn with_skills(seat_text: &str, skills: &[Skill], skills_dir: Option<&Path>) -> String {
    let mut prompt = seat_text.trim().to_string();
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
/// is what an agent does with a skill of its own, and it is why a broad
/// set of skills costs an agent a few lines rather than a few pages.
const SKILLS_HEADER: &str =
    "\n\nYour skills. Read the document of a skill before you do the work it covers:";

/// Initial prompt for an orchestrator session.
///
/// A repository's description is what its owner wrote it down as, so it goes
/// into the briefing right after the checkout it describes.
///
/// The goal's workflow goes in too, with every column of it one line each:
/// it is what the orchestrator staffs every task against, so it has to be in
/// the one text that starts the plan. A goal with no repository is not one
/// an orchestrator is ever started for, so what that case renders only has to
/// stay readable, never to work.
pub(crate) fn orchestrator_briefing(
    template: &str,
    goal: &Goal,
    repos: &[Repository],
    steps: &[GoalStep],
) -> String {
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
    let briefing = render(
        template,
        &[
            ("goal_title", &goal.title),
            ("goal_description", &goal.description),
            ("workflow", &goal.workflow),
            ("columns", &column_lines(steps)),
            ("repositories", &repo_lines),
        ],
    );
    match &goal.issue_url {
        Some(url) => format!(
            "{briefing}\n\nThis goal comes from {url}. Every request a task opens must say `Closes {url}` in its body."
        ),
        None => briefing,
    }
}

/// One line per column of a goal's workflow, in column order: its id, its
/// title, what it does, and the skills and rank its agent is staffed on.
fn column_lines(steps: &[GoalStep]) -> String {
    steps
        .iter()
        .map(|s| {
            let skills: Vec<String> = serde_json::from_str(&s.skills).unwrap_or_default();
            let mut line = format!("- {} [{}]: {}", s.id, s.title, s.description);
            if !skills.is_empty() {
                line.push_str(&format!(" Skills: {}.", skills.join(", ")));
            }
            if let Some(rank) = &s.rank {
                line.push_str(&format!(" Rank: {rank}."));
            }
            line
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// What an orchestrator that has gone quiet is nudged with.
pub(crate) fn orchestrator_resume_briefing(template: &str, goal: &Goal) -> String {
    render(template, &[("goal_title", &goal.title)])
}

/// What the orchestrator of a goal under way is woken with: the goal, and the
/// lines the scheduler wrote about the tasks that need it.
pub(crate) fn goal_attention_briefing(template: &str, goal: &Goal, tasks: &str) -> String {
    render(template, &[("goal_title", &goal.title), ("tasks", tasks)])
}

/// What one agent said to another, as the recipient reads it: who wrote it,
/// the task and agent id an answer goes to, and what it says.
///
/// `task_title` and `skills` are the sender's, fetched by the scheduler from
/// the message's own `task_id` and `from_agent_id` — the message carries
/// neither, since that is store data rather than something it was sent with.
/// A message with no task, such as the orchestrator's, is named by `seat`
/// alone, and the briefing adds no instruction to answer an id nobody named.
pub(crate) fn incoming_message_briefing(
    template: &str,
    message: &Message,
    seat: &str,
    task_title: Option<&str>,
    skills: &[String],
) -> String {
    let from = match (&message.from_agent_id, &message.task_id, task_title) {
        (Some(agent_id), Some(task_id), Some(title)) => {
            let skills = if skills.is_empty() {
                String::new()
            } else {
                format!(" ({})", skills.join(", "))
            };
            format!(r#"the {seat} {agent_id} of task {task_id} "{title}"{skills}"#)
        }
        _ => format!("your {seat}"),
    };
    let answer_hint = if message.from_agent_id.is_some() && message.task_id.is_some() {
        " Answer with send_message to that agent id and task id."
    } else {
        ""
    };
    render(
        template,
        &[
            ("from", &from),
            ("body", &message.body),
            ("answer_hint", answer_hint),
        ],
    )
}

/// Initial prompt for a pull request session (029): the request, the
/// checkout and worktree its commands act on, its two branches and the
/// login whose request it is.
pub(crate) fn pull_request_briefing(
    template: &str,
    pull: &PullRequest,
    repo: &Repository,
    worktree: &str,
    login: &str,
) -> String {
    render(
        template,
        &[
            ("title", &pull.title),
            ("url", &pull.url),
            ("repo_path", &repo.path),
            ("worktree_path", worktree),
            ("head_branch", &pull.head_branch),
            ("base_branch", &pull.base_branch),
            ("login", login),
            ("head_sha", &pull.head_sha),
            (
                "reviewed_sha",
                pull.reviewed_sha.as_deref().unwrap_or("none"),
            ),
        ],
    )
}

/// What a pull request session is woken with: the request, and the lines
/// `forge::news` wrote about what it has not been told.
pub(crate) fn pull_request_news(template: &str, pull: &PullRequest, news: &[String]) -> String {
    render(
        template,
        &[("title", &pull.title), ("news", &news.join("\n"))],
    )
}

/// Fill every value carried by a workflow column's first briefing.
pub(crate) fn step_briefing(
    template: &str,
    task: &Task,
    goal: &Goal,
    repo: &Repository,
    step: &GoalStep,
    previous: &str,
    dependencies: &str,
) -> String {
    render(
        template,
        &[
            ("task_title", &task.title),
            ("task_description", &task.description),
            ("goal_title", &goal.title),
            ("worktree_path", task.worktree_path.as_deref().unwrap_or("")),
            ("branch", &task.branch),
            ("base_branch", &repo.base_branch),
            ("repo_path", &repo.path),
            ("step_id", &step.id),
            ("step_title", &step.title),
            ("step_description", &step.description),
            ("previous_summary", previous),
            ("dependencies", dependencies),
        ],
    )
}

/// What a column's agent is briefed with when the task comes back to its
/// column: which way it came, and why.
pub(crate) fn step_return(
    template: &str,
    task: &Task,
    step: &GoalStep,
    direction: &str,
    reason: &str,
) -> String {
    render(
        template,
        &[
            ("task_title", &task.title),
            ("step_title", &step.title),
            ("direction", direction),
            ("reason", reason),
        ],
    )
}

/// What the agent of the current column is nudged with.
pub(crate) fn agent_resume(template: &str, task: &Task, step: &GoalStep) -> String {
    render(
        template,
        &[("task_title", &task.title), ("step_title", &step.title)],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn goal() -> Goal {
        Goal {
            workflow: "develop-review-merge".into(),
            issue_url: None,
            id: "01goalxxxxxxxxxxxxxxxxxxxx".into(),
            title: "Ship the UI".into(),
            description: "The board needs swimlanes.".into(),
            status: "planning".into(),
            orchestrated: true,
            model: "stub:test-model".into(),
            effort: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
            orchestrator_answered_failed_task_ids: None,
            orchestrator_given_up_at: None,
            orchestrator_given_up_wedged: false,
        }
    }

    fn repo() -> Repository {
        Repository {
            default_workflow: "develop-review-merge".into(),
            id: "01repoxxxxxxxxxxxxxxxxxxxx".into(),
            path: "/repos/ariadne".into(),
            base_branch: "main".into(),
            description: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
            permission_mode: "auto".into(),
            forge: None,
        }
    }

    fn message() -> Message {
        Message {
            id: "01msgxxxxxxxxxxxxxxxxxxxxx".into(),
            goal_id: "01goalxxxxxxxxxxxxxxxxxxxx".into(),
            task_id: Some("01taskxxxxxxxxxxxxxxxxxxxx".into()),
            kind: "message".into(),
            from_actor: "agent".into(),
            from_agent_id: Some("01agentxxxxxxxxxxxxxxxxxxx".into()),
            from_session: None,
            to_actor: "agent".into(),
            to_agent_id: Some("01otherxxxxxxxxxxxxxxxxxxx".into()),
            body: "Why is the retry unbounded?".into(),
            delivered_at: None,
            created_at: "2026-01-01T00:00:00Z".into(),
        }
    }

    fn task() -> Task {
        Task {
            step: Some("develop".into()),
            id: "01taskxxxxxxxxxxxxxxxxxxxx".into(),
            goal_id: "01goalxxxxxxxxxxxxxxxxxxxx".into(),
            repo_id: "01repoxxxxxxxxxxxxxxxxxxxx".into(),
            title: "Render prompts from the database".into(),
            description: "Read them from the store.".into(),
            status: "in_progress".into(),
            branch: "render-prompts-from-the-database-xxxxxx".into(),
            worktree_path: Some("/worktrees/task-eng".into()),
            stalled: 0,
            merge_commit: None,
            pr_url: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
        }
    }

    fn step(ordinal: i64, id: &str, title: &str, skills: &str, rank: Option<&str>) -> GoalStep {
        GoalStep {
            goal_id: goal().id,
            ordinal,
            id: id.into(),
            title: title.into(),
            description: format!("{title} the change."),
            skills: skills.into(),
            rank: rank.map(str::to_string),
            gate: None,
        }
    }

    fn steps() -> Vec<GoalStep> {
        vec![
            step(0, "develop", "Develop", r#"["coding"]"#, Some("balanced")),
            step(
                1,
                "review",
                "Review",
                r#"["code-review"]"#,
                Some("frontier"),
            ),
            step(2, "merge", "Merge", r#"["merge"]"#, None),
        ]
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
        let steps = steps();
        for kind in PromptKind::ALL {
            let template = kind
                .placeholders()
                .iter()
                .map(|name| format!("{{{name}}}"))
                .collect::<Vec<_>>()
                .join("\n");
            let rendered = match kind {
                PromptKind::StepBriefing => {
                    step_briefing(&template, &task, &goal, &repo, &steps[0], "summary", "none")
                }
                PromptKind::StepReturn => {
                    step_return(&template, &task, &steps[0], "back", "fix it")
                }
                PromptKind::AgentResume => agent_resume(&template, &task, &steps[0]),
                PromptKind::OrchestratorBriefing => {
                    orchestrator_briefing(&template, &goal, std::slice::from_ref(&repo), &steps)
                }
                PromptKind::OrchestratorResume => orchestrator_resume_briefing(&template, &goal),
                PromptKind::GoalAttention => {
                    goal_attention_briefing(&template, &goal, "- one task failed")
                }
                PromptKind::IncomingMessage => incoming_message_briefing(
                    &template,
                    &message(),
                    "agent",
                    Some(task.title.as_str()),
                    &["code-review".to_string()],
                ),
            };
            assert!(
                !rendered.contains('{'),
                "the {} briefing left a placeholder of its own unfilled: {rendered}",
                kind.as_str()
            );
        }
    }

    /// The two texts of a pull request session name only the placeholders
    /// their builders fill in, and the builders fill every one of them: the
    /// briefing a session starts on and the news it is woken with reach it
    /// with no raw `{token}` left (026).
    #[test]
    fn the_pull_request_texts_fill_every_placeholder_they_name() {
        use ariadne_store::defaults::{
            PULL_REQUEST_PLACEHOLDERS, pull_request_briefing_prompt, pull_request_news_prompt,
        };
        let pull = PullRequest {
            id: "01pr".into(),
            repository_id: "01repoxxxxxxxxxxxxxxxxxxxx".into(),
            number: 7,
            url: "https://github.com/acme/widgets/pull/7".into(),
            title: "Fix widgets".into(),
            author_login: "me".into(),
            state: "open".into(),
            draft: false,
            head_branch: "fix".into(),
            head_sha: "abc".into(),
            head_repo: None,
            base_branch: "main".into(),
            checks: "none".into(),
            review_decision: "none".into(),
            mergeable: "unknown".into(),
            unanswered_comments: 0,
            origin_task_id: None,
            opened_at: String::new(),
            role: "reviewer".into(),
            ready: false,
            forge_updated_at: String::new(),
            created_at: String::new(),
            updated_at: String::new(),
            failed_checks: "[]".into(),
            behind_base: false,
            told_checks: "[]".into(),
            told_behind_base: false,
            told_review_decision: None,
            told_state: None,
            told_check_state: None,
            news_told_at: None,
            reviewed_sha: None,
            told_head_sha: None,
            review_requested: true,
            review_asked: false,
            body: String::new(),
            review_model: None,
            review_effort: None,
            review_skills_json: "[]".into(),
            merge_sha: None,
            summary_comment_id: None,
            reviewer_given_up_at: None,
            reviewer_given_up_wedged: false,
            ready_confirmed_at: None,
        };
        for template in [pull_request_briefing_prompt(), pull_request_news_prompt()] {
            let mut rest = template;
            while let Some(open) = rest.find('{') {
                let name = &rest[open + 1..rest[open..].find('}').unwrap() + open];
                assert!(
                    PULL_REQUEST_PLACEHOLDERS.contains(&name),
                    "{{{name}}} is no placeholder of a pull request text"
                );
                rest = &rest[open + 1..];
            }
        }
        let review = pull_request_briefing(
            pull_request_briefing_prompt(),
            &PullRequest {
                reviewed_sha: Some("abd".into()),
                ..pull.clone()
            },
            &repo(),
            "/worktrees/pr-01pr",
            "me",
        );
        for value in [
            "Fix widgets",
            "https://github.com/acme/widgets/pull/7",
            "/repos/ariadne",
            "/worktrees/pr-01pr, at abc",
            "fix onto main",
            "Your login: me",
            "Last reviewed sha: abd",
        ] {
            assert!(review.contains(value), "{value}: {review}");
        }
        let news = pull_request_news(
            pull_request_news_prompt(),
            &pull,
            &["- The request is now merged.".to_string()],
        );
        assert!(news.contains("\n- The request is now merged.\n"), "{news}");
        for text in [review, news] {
            assert!(!text.contains('{'), "{text}");
        }
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
        let steps = steps();
        let repo_line = format!("- {} (base branch: {})", repo.path, repo.base_branch);
        let columns = column_lines(&steps);
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
                    &steps,
                ),
                vec![
                    ("goal_title", &goal.title),
                    ("goal_description", &goal.description),
                    ("workflow", &goal.workflow),
                    ("columns", &columns),
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
                PromptKind::StepBriefing,
                step_briefing(
                    default(PromptKind::StepBriefing),
                    &task,
                    &goal,
                    &repo,
                    &steps[1],
                    "The change is committed.",
                    "none",
                ),
                vec![
                    ("task_title", &task.title),
                    ("task_description", &task.description),
                    ("goal_title", &goal.title),
                    ("worktree_path", task.worktree_path.as_deref().unwrap()),
                    ("branch", &task.branch),
                    ("base_branch", &repo.base_branch),
                    ("repo_path", &repo.path),
                    ("step_id", "review"),
                    ("step_title", "Review"),
                    ("step_description", "Review the change."),
                    ("previous_summary", "The change is committed."),
                    ("dependencies", "none"),
                ],
            ),
            (
                PromptKind::StepReturn,
                step_return(
                    default(PromptKind::StepReturn),
                    &task,
                    &steps[0],
                    "back",
                    "Add a test.",
                ),
                vec![
                    ("task_title", &task.title),
                    ("step_title", "Develop"),
                    ("direction", "back"),
                    ("reason", "Add a test."),
                ],
            ),
            (
                PromptKind::AgentResume,
                agent_resume(default(PromptKind::AgentResume), &task, &steps[0]),
                vec![("task_title", &task.title), ("step_title", "Develop")],
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

    /// The orchestrator is briefed with the goal's own workflow and every
    /// column of it, skills and rank included: that is what it staffs every
    /// task against, and nothing of a landing is left in the text.
    #[test]
    fn the_orchestrator_is_briefed_with_the_workflow_and_its_columns() {
        let briefing = orchestrator_briefing(
            default(PromptKind::OrchestratorBriefing),
            &goal(),
            &[repo()],
            &steps(),
        );
        assert!(
            briefing.contains("Workflow: develop-review-merge"),
            "{briefing}"
        );
        assert!(
            briefing.contains(
                "- develop [Develop]: Develop the change. Skills: coding. Rank: balanced."
            ),
            "{briefing}"
        );
        assert!(
            briefing.contains("- merge [Merge]: Merge the change. Skills: merge."),
            "a column with no rank names none: {briefing}"
        );
        assert!(!briefing.contains("Landing"), "{briefing}");
        assert!(!briefing.contains('{'), "{briefing}");
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
            &steps(),
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
        let briefing = orchestrator_briefing(
            default(PromptKind::OrchestratorBriefing),
            &goal(),
            &[],
            &steps(),
        );
        assert!(briefing.contains("# Goal: Ship the UI"), "{briefing}");
        assert!(!briefing.contains('{'), "{briefing}");
    }

    /// The orchestrator is briefed with every repository the goal works in,
    /// each with the base branch a task's last column lands onto, so it can
    /// plan against the right branch before any task exists.
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
            &steps(),
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

    /// A goal from an issue tells the orchestrator which request body line
    /// closes it, once, after the briefing proper.
    #[test]
    fn a_goal_from_an_issue_names_the_line_that_closes_it() {
        let goal = Goal {
            issue_url: Some("https://github.com/acme/widgets/issues/9".into()),
            ..goal()
        };
        let briefing = orchestrator_briefing(
            default(PromptKind::OrchestratorBriefing),
            &goal,
            &[repo()],
            &steps(),
        );
        assert!(
            briefing.ends_with(
                "This goal comes from https://github.com/acme/widgets/issues/9. Every request \
                 a task opens must say `Closes https://github.com/acme/widgets/issues/9` in its body."
            ),
            "{briefing}"
        );
    }

    /// A step briefing with no worktree yet still renders every value: the
    /// fallbacks the daemon used to inline are part of the values now.
    #[test]
    fn missing_values_keep_their_fallbacks() {
        let (goal, repo) = (goal(), repo());
        let task = Task {
            worktree_path: None,
            ..task()
        };
        let briefing = step_briefing(
            default(PromptKind::StepBriefing),
            &task,
            &goal,
            &repo,
            &steps()[0],
            "",
            "none",
        );
        assert!(briefing.contains("Worktree: \n"), "{briefing}");
        assert!(briefing.contains("Dependencies: none"), "{briefing}");
        assert!(!briefing.contains('{'), "{briefing}");
    }
}
