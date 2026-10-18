//! What a `task create` or `task update` line means before it is sent.
//!
//! Both are refused here rather than by the daemon where the answer is already
//! known: an update with nothing in it, an agent slot that names no column,
//! one whose model is missing or names no agent, one whose `@` is followed by
//! no effort, and a `--repo` that names none of the goal's repositories.

use anyhow::{Result, bail};

use ariadne_api::goals::GoalDto;
use ariadne_api::repositories::RepositoryDto;
use ariadne_api::tasks::{AgentAssignment, UpdateTaskRequest};
use ariadne_client::Client;

use crate::commands::{parse_effort, parse_model, resolve};

/// One `--agent STEP[:SKILLS]=MODEL[@EFFORT]` argument: the workflow column
/// this agent staffs, then — after a `:`, optionally — what it knows, then —
/// after the `=`, required — what it runs on, in the one spelling a model is
/// chosen by, `<agent>:<model>`, and — after an `@` — the effort that model
/// is reasoned at. `review:code-review=codex-acp:o3@high` staffs `review` on
/// `code-review`, run at `high`; `review=codex-acp:o3` staffs it on nothing
/// but the column's own skills.
///
/// The skills are comma-separated and in the order they reach the agent:
/// `develop:coding,testing=…` is an agent that codes and tests, and that is
/// the whole of what it is. A name no skill answers to is the daemon's to
/// refuse, which is where the list of them lives.
///
/// The `=` splits first and the last `@` after it splits the effort off. What
/// is in front of the `=` is the column id alone, or the column id and its
/// skills split on the first `:` — a model id's own `:` and `/` are on the
/// other side of the `=` and never reach this split, so
/// `merge=opencode-acp:ollama/llama3:8b` reaches the request as that one id,
/// tag and all. The `=MODEL` half is mandatory — a model is required, and no
/// agent default stands in for one — so a column on its own, with or without
/// an `@EFFORT`, is refused.
///
/// What is after the `=` is the same string `goal create --model` takes, and
/// it is refused here in the same words; what is after the `@` is only
/// checked for being something, since which efforts a model takes is the
/// daemon's to know.
pub(crate) fn parse_agent_slot(s: &str) -> Result<AgentAssignment, String> {
    let Some((head, model)) = s.split_once('=') else {
        return Err(format!(
            "no model in \"{s}\" — a model is required, so {}",
            accepted()
        ));
    };
    let (model, effort) = split_effort(model);
    let (step, skills) = match head.split_once(':') {
        Some((step, skills)) => (
            step,
            skills
                .split(',')
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_string)
                .collect(),
        ),
        None => (head, Vec::new()),
    };
    if step.is_empty() {
        return Err(format!("no column in \"{s}\" — {}", accepted()));
    }
    let effort = match effort {
        Some(effort) => Some(parse_effort(effort).map_err(|e| format!("in \"{s}\": {e}"))?),
        None => None,
    };
    if model.is_empty() {
        return Err(format!(
            "no model after the = in \"{s}\" — a model is required, so {}",
            accepted()
        ));
    }
    let model = parse_model(model).map_err(|e| format!("in \"{s}\": {e}"))?;
    Ok(AgentAssignment {
        step: step.to_string(),
        skills,
        model,
        effort,
        brief: None,
    })
}

/// What an `--agent` argument missing one of its halves is told it may
/// write: the two forms, and the spelling each half is in.
fn accepted() -> String {
    "write STEP=MODEL or STEP:SKILLS=MODEL@EFFORT, where STEP is the id of a workflow \
     column, SKILLS is optionally one or more skill names separated by commas — empty \
     means the column's own — MODEL is the id of an agent of the ACP registry and, \
     after a colon, one model of it, and EFFORT is one of the efforts `ariadne models \
     ls` lists for that model"
        .to_string()
}

/// One half of a slot with its effort taken off: the **last** `@` splits, so
/// everything before it is what it always was — a model id may hold an `@` of
/// its own, and the effort is what a person wrote at the end of the line.
fn split_effort(half: &str) -> (&str, Option<&str>) {
    match half.rsplit_once('@') {
        Some((head, effort)) => (head, Some(effort)),
        None => (half, None),
    }
}

/// The flags of `task update`, as clap parsed them: one field per flag, in
/// the order the help screen lists them.
///
/// A struct rather than five positional arguments, because three of them are
/// an `Option` or a `bool` and a caller that swapped two would still compile.
#[derive(Debug, Default)]
pub(crate) struct Edits {
    pub title: Option<String>,
    pub description: Option<String>,
    pub agents: Vec<AgentAssignment>,
    pub depends_on: Vec<String>,
    pub clear_depends_on: bool,
}

/// The PATCH body of `task update`, or the reason there is nothing to send.
///
/// A flag that was not given is `None` — the field keeps what the task has.
/// The two list flags are all-or-nothing by design: they replace the list they
/// name. The dependencies have a flag of their own for the empty list —
/// `--clear-depends-on` — since a repeatable flag cannot be given zero times
/// on purpose; a staffing has no empty list to ask for, since every column
/// wants an agent.
pub(crate) fn update_request(edits: Edits) -> Result<UpdateTaskRequest> {
    let Edits {
        title,
        description,
        agents,
        depends_on,
        clear_depends_on,
    } = edits;
    let req = UpdateTaskRequest {
        // The whole staffing, replaced: `task update --agent` restages every
        // column at once, which is how a column a task lacks is staffed
        // before it is retried.
        agents: (!agents.is_empty()).then_some(agents),
        title,
        description,
        depends_on: match (clear_depends_on, depends_on.is_empty()) {
            (true, _) => Some(Vec::new()),
            (false, true) => None,
            (false, false) => Some(depends_on),
        },
    };
    // An empty PATCH would still reach the daemon and still be refused on a
    // started task, which reads as a failure the caller never asked for.
    if req.title.is_none()
        && req.description.is_none()
        && req.agents.is_none()
        && req.depends_on.is_none()
    {
        bail!("nothing to update — pass --title, --description, --agent or --depends-on");
    }
    Ok(req)
}

/// A `--repo` argument as the repo id the API wants.
///
/// The goal's repositories answer to their id or to their registered path —
/// the two spellings `goal inspect` prints — because nobody types a ULID they
/// have not been given.
pub(crate) async fn resolve_repo(client: &Client, goal_id: &str, spec: &str) -> Result<String> {
    let g: GoalDto = client.get_json(&format!("/v1/goals/{goal_id}")).await?;
    match pick_repo(&g.repos, spec) {
        Some(id) => Ok(id),
        None => bail!(
            "goal {goal_id} has no repo \"{spec}\" — it has {}",
            g.repos
                .iter()
                .map(|r| format!("{} ({})", r.path, r.id))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// The id of the goal repository a `--repo` argument names: by path, or by
/// its id in any of the spellings one is shown in — a whole one, the head of
/// one, or the `…last8` a table prints.
fn pick_repo(repos: &[RepositoryDto], spec: &str) -> Option<String> {
    if let Some(repo) = repos.iter().find(|r| r.path == spec) {
        return Some(repo.id.clone());
    }
    resolve::among(
        resolve::Kind::Repo,
        repos.iter().map(|r| resolve::row(&r.id, &r.path)),
    )
    .pick(spec)
    .ok()
    .map(|row| row.id.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::commands::fixtures::repository;

    #[test]
    fn a_repo_is_named_by_id_or_by_path() {
        let repos = [
            repository("01REPOAPI", "/home/me/api", "main"),
            repository("01REPOUI", "/home/me/ui", "main"),
        ];
        assert_eq!(pick_repo(&repos, "01REPOUI").as_deref(), Some("01REPOUI"));
        assert_eq!(
            pick_repo(&repos, "/home/me/api").as_deref(),
            Some("01REPOAPI")
        );
        assert_eq!(pick_repo(&repos, "/home/me/other"), None);
        // And by the tail of an id, which is all a table of them shows.
        assert_eq!(pick_repo(&repos, "REPOUI").as_deref(), Some("01REPOUI"));
    }

    /// An `--agent` slot names its column, its skills, its model and its
    /// effort: `review:code-review=codex-acp:o3@high` staffs the `review`
    /// column on `code-review`, run at `high`; the skills half is optional,
    /// and left out it is empty — the column's own, once the daemon fills
    /// it in.
    #[test]
    fn an_agent_slot_names_its_column_skills_model_and_effort() {
        let full = parse_agent_slot("review:code-review=codex-acp:o3@high").expect("a full slot");
        assert_eq!(full.step, "review");
        assert_eq!(full.skills, ["code-review"]);
        assert_eq!(full.model, "codex-acp:o3");
        assert_eq!(full.effort.as_deref(), Some("high"));
        assert_eq!(full.brief, None, "the brief is the MCP tools' alone");

        let bare = parse_agent_slot("review=codex-acp:o3").expect("no skills named");
        assert_eq!(bare.step, "review");
        assert!(bare.skills.is_empty(), "empty means the column's own");
        assert_eq!(bare.model, "codex-acp:o3");
        assert_eq!(bare.effort, None, "no effort at all is the agent's own");

        // The skills are what an agent is, in the order they were written.
        let several = parse_agent_slot("review:code-review,security-review=opencode-acp:x")
            .expect("two skills");
        assert_eq!(several.skills, ["code-review", "security-review"]);

        // A model id of its own `:` never reaches the column/skills split:
        // that split is on the other side of the `=`.
        let provider =
            parse_agent_slot("merge=opencode-acp:ollama/llama3:8b").expect("a provider model id");
        assert_eq!(provider.model, "opencode-acp:ollama/llama3:8b");
    }

    /// The other half an agent may carry: the effort after the `@` — and the
    /// `@` is the last one, so a model id keeps every `:` and `/` it came
    /// with.
    #[test]
    fn an_agent_runs_at_the_effort_after_the_at_sign() {
        let both = parse_agent_slot("develop:coding=codex-acp:gpt-5.6-sol@xhigh")
            .expect("a model and an effort");
        assert_eq!(both.skills, ["coding"]);
        assert_eq!(both.model, "codex-acp:gpt-5.6-sol");
        assert_eq!(both.effort.as_deref(), Some("xhigh"));

        // The model half is cut at the last `@` and nothing else: a model id
        // may be `provider/model` with a tag of its own, and all of it is model.
        let opencode =
            parse_agent_slot("review=opencode-acp:openrouter/x/y:z@high").expect("a provider id");
        assert_eq!(opencode.model, "opencode-acp:openrouter/x/y:z");
        assert_eq!(opencode.effort.as_deref(), Some("high"));

        // Which efforts a model takes is the daemon's to know: anything that
        // is an effort at all travels, and is refused where the model is.
        let unknown =
            parse_agent_slot("review=claude-agent-acp:claude-opus-5@ultra").expect("an effort");
        assert_eq!(unknown.effort.as_deref(), Some("ultra"));
    }

    /// Half a form is a typo, and it is refused where it was typed — with the
    /// forms it accepts, and, for the model half, the same words `--model`
    /// would have been refused in. A model is required, so a slot with no
    /// `=MODEL` at all is half a form too.
    #[test]
    fn an_agent_slot_missing_a_half_is_refused_before_it_is_sent() {
        let err = parse_agent_slot("review:code-review").expect_err("no model at all");
        assert!(err.contains("no model in \"review:code-review\""), "{err}");
        assert!(err.contains("a model is required"), "{err}");
        assert!(err.contains("STEP=MODEL"), "{err}");

        // A column at an effort still names no model, and an agent needs one.
        let err = parse_agent_slot("review@high").expect_err("no model at all");
        assert!(err.contains("a model is required"), "{err}");

        let err = parse_agent_slot("review=").expect_err("no model");
        assert!(err.contains("no model after the ="), "{err}");
        assert!(err.contains("a model is required"), "{err}");
        assert!(err.contains("STEP=MODEL"), "{err}");
        assert!(err.contains("ACP registry"), "{err}");

        let err = parse_agent_slot("review=codex-acp:").expect_err("no model after the colon");
        assert!(err.contains("in \"review=codex-acp:\""), "{err}");
        assert!(err.contains("no model after the `:`"), "{err}");

        // Whitespace after the colon is an empty model too.
        let err = parse_agent_slot("review=codex-acp: ").expect_err("whitespace is no model");
        assert!(err.contains("no model after the `:`"), "{err}");
        assert!(err.contains("a model is required"), "{err}");

        let err = parse_agent_slot("review=llama").expect_err("no agent");
        assert!(err.contains("`llama` names no agent"), "{err}");
        assert!(err.contains("`<agent>:llama`"), "{err}");

        // A slot has to say which column it staffs.
        let err = parse_agent_slot("=codex-acp:o3").expect_err("no column");
        assert!(err.contains("no column"), "{err}");
        let err = parse_agent_slot(":code-review=codex-acp:o3").expect_err("no column");
        assert!(err.contains("no column"), "{err}");

        // An `@` that says nothing after it is the same kind of typo, and the
        // refusal names the forms one of which was meant.
        let err = parse_agent_slot("review=codex-acp:o3@").expect_err("no effort");
        assert!(err.contains("in \"review=codex-acp:o3@\""), "{err}");
        assert!(err.contains("no effort was named"), "{err}");
        assert!(err.contains("ariadne models ls"), "{err}");

        // An effort without the model it belongs to is still a missing model:
        // the `=` was written, so something was meant to follow it.
        let err = parse_agent_slot("review=@high").expect_err("no model");
        assert!(err.contains("no model after the ="), "{err}");
        assert!(err.contains("STEP:SKILLS=MODEL@EFFORT"), "{err}");
    }

    /// The lists replace rather than extend, so an absent flag must not send an
    /// empty list and wipe what the task has — and the one thing a repeatable
    /// flag cannot say on its own is spelled `--clear-depends-on`.
    #[test]
    fn only_the_flags_that_were_given_reach_the_daemon() {
        let req = update_request(Edits {
            title: Some("new".into()),
            ..Edits::default()
        })
        .expect("body");
        assert_eq!(req.title.as_deref(), Some("new"));
        assert!(req.description.is_none());
        assert!(req.agents.is_none(), "and the staffing is left alone");
        assert!(req.depends_on.is_none());

        // `--agent` restages every column at once.
        let req = update_request(Edits {
            agents: vec![
                parse_agent_slot("develop=codex-acp:o3").expect("a slot"),
                parse_agent_slot("review=claude-agent-acp:claude-opus-5@high").expect("a slot"),
            ],
            depends_on: vec!["01TASK".into()],
            ..Edits::default()
        })
        .expect("body");
        assert_eq!(
            req.agents.as_ref().map(|agents| agents
                .iter()
                .map(|a| (a.step.as_str(), a.model.as_str(), a.effort.as_deref()))
                .collect::<Vec<_>>()),
            Some(vec![
                ("develop", "codex-acp:o3", None),
                ("review", "claude-agent-acp:claude-opus-5", Some("high")),
            ])
        );
        assert_eq!(
            req.depends_on.as_deref(),
            Some(["01TASK".to_string()].as_slice())
        );
        assert!(req.title.is_none(), "and nothing else was touched");

        let req = update_request(Edits {
            clear_depends_on: true,
            ..Edits::default()
        })
        .expect("body");
        assert_eq!(req.depends_on.as_deref(), Some([].as_slice()));
        assert!(req.agents.is_none(), "and nothing else was touched");
    }

    #[test]
    fn an_update_with_no_flags_is_refused_before_it_is_sent() {
        let err = update_request(Edits::default()).expect_err("no-op");
        assert!(err.to_string().starts_with("nothing to update"), "{err}");
        assert!(err.to_string().contains("--agent"), "{err}");
        assert!(err.to_string().contains("--depends-on"), "{err}");
    }
}
