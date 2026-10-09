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
use ariadne_store::{Goal, Message, PullRequest, Repository, Skill, Task};

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
/// The briefing also carries the procedure that puts the approved spec on a
/// base branch, since the orchestrator lands that spec itself. It is the one of
/// [`default_spec_landing_prompt`] the goal's first repository calls for —
/// the checkout the orchestrator is started in, and the one its commands name.
/// A goal with no repository is not one an orchestrator is ever started for, so
/// what that case renders only has to stay readable, never to work.
///
/// The goal's own landing goes in too, read with `Goal::landing`: it is
/// what tells the orchestrator whether to plan a `feature_branch` goal's
/// final task, so it has to be in the one text that starts the plan.
pub(crate) fn orchestrator_briefing(template: &str, goal: &Goal, repos: &[Repository]) -> String {
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
            ("landing", goal.landing().as_str()),
            ("repositories", &repo_lines),
        ],
    );
    match &goal.issue_url {
        Some(url) => format!(
            "{briefing}\n\nThis goal comes from {url}. Every request an author opens must say `Closes {url}` in its body."
        ),
        None => briefing,
    }
}

/// Name a stepped goal's workflow and every column in its briefing.
pub(crate) fn with_workflow(
    briefing: String,
    goal: &Goal,
    steps: &[ariadne_store::GoalStep],
) -> String {
    let Some(workflow) = &goal.workflow else {
        return briefing;
    };
    let columns = steps
        .iter()
        .map(|s| format!("- {} [{}]: {}", s.id, s.title, s.description))
        .collect::<Vec<_>>()
        .join("\n");
    let landing = format!("Landing: {}", goal.landing().as_str());
    let workflow = format!("Workflow: {workflow}\n{columns}");
    if briefing.lines().any(|line| line == landing) {
        briefing
            .lines()
            .map(|line| {
                if line == landing {
                    workflow.as_str()
                } else {
                    line
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        format!("{briefing}\n\n{workflow}")
    }
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

/// Initial prompt for an author session.
pub fn author_briefing(
    template: &str,
    task: &Task,
    goal: &Goal,
    repo: &Repository,
    base_branch: &str,
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
            ("base_branch", base_branch),
            ("repo_path", &repo.path),
            ("landing", goal.landing().as_str()),
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
pub(crate) fn reviewer_briefing(
    template: &str,
    task: &Task,
    goal: &Goal,
    repo: &Repository,
    base_branch: &str,
    summary: Option<&str>,
) -> String {
    render(
        template,
        &[
            ("task_title", &task.title),
            ("task_description", &task.description),
            ("goal_title", &goal.title),
            ("branch", &task.branch),
            ("base_branch", base_branch),
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

/// What a reviewer is asked with once every author of a several-author task
/// is approved: the task, and one line per author to pick between.
///
/// `authors` is (id, branch) per author, in the order the orchestrator
/// listed them — the id is what `pick_winner` takes, so each line leads with
/// it.
pub(crate) fn reviewer_pick_briefing(
    template: &str,
    task: &Task,
    authors: &[(String, String)],
) -> String {
    let lines = authors
        .iter()
        .map(|(id, branch)| format!("- {id}: branch {branch}"))
        .collect::<Vec<_>>()
        .join("\n");
    render(
        template,
        &[("task_title", &task.title), ("authors", &lines)],
    )
}

/// Resume prompt for an author with a round of requested changes.
///
/// `feedback` is one entry per source, each a heading naming who asked and
/// what they wrote: the reviewers of the round, or the people reading a
/// published request, whose comments the daemon relays itself.
pub(crate) fn changes_requested_briefing(template: &str, feedback: &[(String, String)]) -> String {
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
pub(crate) fn landing_briefing(
    template: &str,
    task: &Task,
    repo: &Repository,
    base_branch: &str,
) -> String {
    render(
        template,
        &[
            ("task_title", &task.title),
            ("branch", &task.branch),
            ("base_branch", base_branch),
            ("repo_path", &repo.path),
        ],
    )
}

/// Initial prompt for a pull request session (026): the request, the
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
    step: &ariadne_store::GoalStep,
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

pub(crate) fn step_return(
    template: &str,
    task: &Task,
    step: &ariadne_store::GoalStep,
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

pub(crate) fn agent_resume(template: &str, task: &Task, step: &ariadne_store::GoalStep) -> String {
    render(
        template,
        &[("task_title", &task.title), ("step_title", &step.title)],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    use ariadne_core::Landing;

    use ariadne_store::defaults::default_landing_prompt;

    fn goal() -> Goal {
        Goal {
            workflow: None,
            issue_url: None,
            id: "01goalxxxxxxxxxxxxxxxxxxxx".into(),
            title: "Ship the UI".into(),
            description: "The board needs swimlanes.".into(),
            status: "planning".into(),
            orchestrated: true,
            model: "stub:test-model".into(),
            effort: None,
            landing: "merge".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
        }
    }

    fn repo() -> Repository {
        Repository {
            default_workflow: None,
            id: "01repoxxxxxxxxxxxxxxxxxxxx".into(),
            path: "/repos/ariadne".into(),
            base_branch: "main".into(),
            description: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
            permission_mode: "auto".into(),
            default_landing: "merge".into(),
            forge: None,
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
            step: None,
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
            picked_agent_id: None,
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
            let step = ariadne_store::GoalStep {
                goal_id: goal.id.clone(),
                ordinal: 0,
                id: "develop".into(),
                title: "Develop".into(),
                description: "Build it.".into(),
                skills: "[]".into(),
                rank: None,
                gate: None,
            };
            let rendered = match kind {
                PromptKind::StepBriefing => {
                    step_briefing(&template, &task, &goal, &repo, &step, "summary", "none")
                }
                PromptKind::StepReturn => step_return(&template, &task, &step, "back", "fix it"),
                PromptKind::AgentResume => agent_resume(&template, &task, &step),
                PromptKind::OrchestratorBriefing => {
                    orchestrator_briefing(&template, &goal, std::slice::from_ref(&repo))
                }
                PromptKind::OrchestratorResume => orchestrator_resume_briefing(&template, &goal),
                PromptKind::GoalAttention => {
                    goal_attention_briefing(&template, &goal, "- one task failed")
                }
                PromptKind::IncomingMessage => incoming_message_briefing(
                    &template,
                    &message(),
                    "reviewer",
                    Some(task.title.as_str()),
                    &["code-review".to_string()],
                ),
                PromptKind::AuthorBriefing => {
                    author_briefing(&template, &task, &goal, &repo, &repo.base_branch, &[])
                }
                PromptKind::AuthorResume => author_resume_briefing(&template, &task),
                PromptKind::ChangesRequested => changes_requested_briefing(&template, &feedback),
                PromptKind::ReviewerBriefing => reviewer_briefing(
                    &template,
                    &task,
                    &goal,
                    &repo,
                    &repo.base_branch,
                    Some("done"),
                ),
                PromptKind::ReviewerResume => {
                    reviewer_resume_briefing(&template, &task, Some("done"))
                }
                PromptKind::ReviewerPick => reviewer_pick_briefing(
                    &template,
                    &task,
                    &[("01author".to_string(), "a-branch".to_string())],
                ),
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
        let rendered = landing_briefing(&template, &task, &repo, &repo.base_branch);
        assert!(
            !rendered.contains('{'),
            "the landing briefing left a placeholder of its own unfilled: {rendered}"
        );
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
                    ("landing", goal.landing().as_str()),
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
                    &repo.base_branch,
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
                    &repo.base_branch,
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
            let rendered =
                landing_briefing(task.landing_prompt_text(), &task, &repo, &repo.base_branch);
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
            &repo.base_branch,
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
            &repo.base_branch,
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

        let landing = landing_briefing(task.landing_prompt_text(), &task, &repo, &repo.base_branch);
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

        let direct = landing_briefing(
            merging.landing_prompt_text(),
            &merging,
            &repo,
            &repo.base_branch,
        );
        assert!(
            direct.contains("git reset --soft \"$(git merge-base main HEAD)\""),
            "{direct}"
        );
        assert!(!direct.contains("gh pr"), "{direct}");

        let published = landing_briefing(
            publishing.landing_prompt_text(),
            &publishing,
            &repo,
            &repo.base_branch,
        );
        assert!(published.contains("`open_pull_request`"), "{published}");
        assert!(!published.contains("reset --soft"), "{published}");
        assert!(!published.contains("gh pr"), "{published}");

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
        let landed = landing_briefing(
            nothing.landing_prompt_text(),
            &nothing,
            &repo,
            &repo.base_branch,
        );
        assert!(landed.contains("lands nothing"), "{landed}");
        assert!(!landed.contains("gh pr"), "{landed}");
        assert!(!landed.contains("reset --soft"), "{landed}");
    }

    /// The orchestrator is briefed with the goal's own landing, whatever it
    /// is: a `feature_branch` goal has to reach it, since that is the one
    /// value that tells the orchestrator to plan a final task per
    /// repository.
    #[test]
    fn the_orchestrator_is_briefed_with_the_goals_landing() {
        let goal = Goal {
            landing: "feature_branch".into(),
            ..goal()
        };
        let briefing =
            orchestrator_briefing(default(PromptKind::OrchestratorBriefing), &goal, &[repo()]);
        assert!(briefing.contains("feature_branch"), "{briefing}");
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
    /// each with the base branch a task's landing merges onto, so it can
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
            &repo.base_branch,
            &[],
        );
        assert!(briefing.contains("- Worktree (your cwd): <worktree>"));
        assert!(briefing.contains("- Finished dependencies:\nnone"));
    }
    #[test]
    fn workflow_columns_are_named_with_or_without_a_landing_line() {
        let mut goal = goal();
        goal.workflow = Some("one-step".into());
        let steps = [ariadne_store::GoalStep {
            goal_id: goal.id.clone(),
            ordinal: 0,
            id: "build".into(),
            title: "Build".into(),
            description: "Build the change.".into(),
            skills: "[]".into(),
            rank: None,
            gate: None,
        }];
        for template in [
            "Plan {goal_title}.",
            "Plan {goal_title}.\nLanding: {landing}",
        ] {
            let rendered = orchestrator_briefing(template, &goal, &[]);
            let briefing = with_workflow(rendered, &goal, &steps);
            assert_eq!(briefing.matches("Workflow: one-step").count(), 1);
            assert!(
                briefing
                    .lines()
                    .any(|line| line == "- build [Build]: Build the change.")
            );
            assert!(!briefing.lines().any(|line| line.starts_with("Landing:")));
        }
        goal.workflow = None;
        let rendered = "Plan this goal.\nLanding: merge\n".to_string();
        assert_eq!(with_workflow(rendered.clone(), &goal, &[]), rendered);
    }
}
