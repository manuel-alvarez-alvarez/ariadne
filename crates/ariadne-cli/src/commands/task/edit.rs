//! What a `task create` or `task update` line means before it is sent.
//!
//! Both are refused here rather than by the daemon where the answer is already
//! known: an update with nothing in it, an agent with no skills, one whose
//! model is missing or names no agent, one whose `@` is
//! followed by no effort, and a `--repo` that names none of the goal's
//! repositories.

use anyhow::{Result, bail};

use ariadne_api::goals::GoalDto;
use ariadne_api::repositories::RepositoryDto;
use ariadne_api::tasks::{AgentAssignment, UpdateTaskRequest};
use ariadne_client::Client;

use ariadne_core::{Landing, Seat};

use crate::commands::{parse_effort, parse_model, resolve};

/// One `--author` or `--reviewer` argument, `SKILLS=MODEL[@EFFORT]`: what
/// the agent knows, then — after the `=`, required — what it runs on, in the
/// one spelling a model is chosen by, `<agent>:<model>`, and — after an
/// `@` — the effort that model is reasoned at.
///
/// The skills are comma-separated and in the order they reach the agent:
/// `--author coding,testing=…` is an agent that codes and tests, and that is
/// the whole of what it is. A name no skill answers to is the daemon's to
/// refuse, which is where the list of them lives.
///
/// The `=` splits first and the last `@` after it splits the effort off, and
/// nothing else splits at all: a model id may carry `/` and `:` of its own,
/// so `code-review=opencode-acp:ollama/llama3:8b` reaches the request as that
/// one id, tag and all. The `=MODEL` half is mandatory — a model is required,
/// and no agent default stands in for one — so skills on their own, with or without
/// an `@EFFORT`, are refused.
///
/// What is after the `=` is the same string `--model` takes, and it is refused
/// here in the same words; what is after the `@` is only checked for being
/// something, since which efforts a model takes is the daemon's to know.
fn parse_agent(seat: Seat, s: &str) -> Result<AgentAssignment, String> {
    let Some((skills, model)) = s.split_once('=') else {
        return Err(format!(
            "no model in \"{s}\" — a model is required, so {}",
            accepted()
        ));
    };
    let (model, effort) = split_effort(model);
    let skills: Vec<String> = skills
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .collect();
    if skills.is_empty() {
        return Err(format!("no skills in \"{s}\" — {}", accepted()));
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
        seat,
        skills,
        model,
        effort,
        brief: None,
    })
}

/// One `--author SKILLS=MODEL[@EFFORT]`.
pub fn parse_author(s: &str) -> Result<AgentAssignment, String> {
    parse_agent(Seat::Author, s)
}

/// One `--reviewer SKILLS=MODEL[@EFFORT]`, in review order.
pub fn parse_reviewer(s: &str) -> Result<AgentAssignment, String> {
    parse_agent(Seat::Reviewer, s)
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

/// What an agent argument missing one of its halves is told it may write: the
/// two forms, and the spelling each half is in.
fn accepted() -> String {
    "write SKILLS=MODEL or SKILLS=MODEL@EFFORT, where SKILLS is one or more \
     skill names separated by commas, MODEL is the id of an agent of the ACP \
     registry and, after a colon, one model of it, and EFFORT is one of the \
     efforts `ariadne models ls` lists for that model"
        .to_string()
}

/// The flags of `task update`, as clap parsed them: one field per flag, in
/// the order the help screen lists them.
///
/// A struct rather than nine positional arguments, because five of them are
/// an `Option` or a `bool` and a caller that swapped two would still compile.
#[derive(Debug, Default)]
pub struct Edits {
    pub title: Option<String>,
    pub description: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub reviewers: Vec<AgentAssignment>,
    pub no_reviewer: bool,
    pub depends_on: Vec<String>,
    pub clear_depends_on: bool,
    pub landing: Option<Landing>,
}

/// The PATCH body of `task update`, or the reason there is nothing to send.
///
/// A flag that was not given is `None` — the field keeps what the task has.
/// The two list flags are all-or-nothing by design: they replace the list they
/// name, and each has a flag of its own for the empty list — `--no-reviewer`
/// and `--clear-depends-on` — since a repeatable flag cannot be given zero
/// times on purpose.
pub fn update_request(edits: Edits) -> Result<UpdateTaskRequest> {
    let Edits {
        title,
        description,
        model,
        effort,
        reviewers,
        no_reviewer,
        depends_on,
        clear_depends_on,
        landing,
    } = edits;
    let req = UpdateTaskRequest {
        title,
        description,
        // Whatever was typed, in the daemon's own spelling. There is no
        // `default` for a model: a model is required, so the flag is already
        // parsed as one.
        model,
        // Three answers, about how deeply the model reasons: nothing said,
        // `default` for the agent's own, or one effort of it.
        effort,
        // The author list is edited over the API and the MCP tools; the CLI
        // re-staffs a task's authors by re-creating it.
        authors: None,
        reviewers: match (no_reviewer, reviewers.is_empty()) {
            (true, _) => Some(Vec::new()),
            (false, true) => None,
            (false, false) => Some(reviewers),
        },
        depends_on: match (clear_depends_on, depends_on.is_empty()) {
            (true, _) => Some(Vec::new()),
            (false, true) => None,
            (false, false) => Some(depends_on),
        },
        landing,
    };
    // An empty PATCH would still reach the daemon and still be refused on a
    // started task, which reads as a failure the caller never asked for.
    if req.title.is_none()
        && req.description.is_none()
        && req.model.is_none()
        && req.effort.is_none()
        && req.reviewers.is_none()
        && req.depends_on.is_none()
        && req.landing.is_none()
    {
        bail!(
            "nothing to update — pass --title, --description, --model, \
             --effort, --reviewer, --no-reviewer, --landing or --depends-on"
        );
    }
    Ok(req)
}

/// A `--repo` argument as the repo id the API wants.
///
/// The goal's repositories answer to their id or to their registered path —
/// the two spellings `goal inspect` prints — because nobody types a ULID they
/// have not been given.
pub async fn resolve_repo(client: &Client, goal_id: &str, spec: &str) -> Result<String> {
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

    /// The skills are what an agent is, and what follows the `=` is a model:
    /// both travel as written, since the daemon is what knows the catalog.
    #[test]
    fn an_agent_keeps_its_skills_and_what_it_runs_on() {
        let pinned =
            parse_reviewer("code-review,security-review=opencode-acp:ollama/llama3:8b@thinking")
                .expect("skills, a model and an effort");
        assert_eq!(pinned.skills, ["code-review", "security-review"]);
        assert_eq!(pinned.model, "opencode-acp:ollama/llama3:8b");
        assert_eq!(pinned.effort.as_deref(), Some("thinking"));

        let both = parse_reviewer("code-review=codex-acp:gpt-5.3-codex").expect("a model");
        assert_eq!(both.skills, ["code-review"]);
        assert_eq!(both.model, "codex-acp:gpt-5.3-codex");
        assert_eq!(both.effort, None, "no effort at all is the agent's own");
    }

    /// The other half an agent may carry: the effort after the `@` — and the
    /// `@` is the last one, so a model id keeps every `:` and `/` it came
    /// with.
    #[test]
    fn a_reviewer_runs_at_the_effort_after_the_at_sign() {
        let both = parse_reviewer("code-review=codex-acp:gpt-5.6-sol@xhigh")
            .expect("a model and an effort");
        assert_eq!(both.skills, ["code-review"]);
        assert_eq!(both.model, "codex-acp:gpt-5.6-sol");
        assert_eq!(both.effort.as_deref(), Some("xhigh"));

        // The model half is cut at the last `@` and nothing else: a model id
        // may be `provider/model` with a tag of its own, and all of it is model.
        let opencode = parse_reviewer("code-review=opencode-acp:openrouter/x/y:z@high")
            .expect("a provider id");
        assert_eq!(opencode.model, "opencode-acp:openrouter/x/y:z");
        assert_eq!(opencode.effort.as_deref(), Some("high"));

        // Which efforts a model takes is the daemon's to know: anything that
        // is an effort at all travels, and is refused where the model is.
        let unknown =
            parse_reviewer("Reviewer=claude-agent-acp:claude-opus-5@ultra").expect("an effort");
        assert_eq!(unknown.effort.as_deref(), Some("ultra"));
    }

    /// Half a form is a typo, and it is refused where it was typed — with the
    /// forms it accepts, and, for the model half, the same words `--model`
    /// would have been refused in. A model is required, so a slot with no
    /// `=MODEL` at all is half a form too.
    #[test]
    fn a_reviewer_missing_a_half_is_refused_before_it_is_sent() {
        let err = parse_reviewer("code-review").expect_err("no model at all");
        assert!(err.contains("no model in \"code-review\""), "{err}");
        assert!(err.contains("a model is required"), "{err}");
        assert!(err.contains("SKILLS=MODEL"), "{err}");

        // Skills at an effort still name no model, and an agent needs one.
        let err = parse_reviewer("code-review@high").expect_err("no model at all");
        assert!(err.contains("a model is required"), "{err}");

        let err = parse_reviewer("code-review=").expect_err("no model");
        assert!(err.contains("no model after the ="), "{err}");
        assert!(err.contains("a model is required"), "{err}");
        assert!(err.contains("SKILLS=MODEL"), "{err}");
        assert!(err.contains("ACP registry"), "{err}");

        let err = parse_reviewer("code-review=codex-acp:").expect_err("no model after the colon");
        assert!(err.contains("in \"code-review=codex-acp:\""), "{err}");
        assert!(err.contains("no model after the `:`"), "{err}");

        // Whitespace after the colon is an empty model too.
        let err = parse_reviewer("code-review=codex-acp: ").expect_err("whitespace is no model");
        assert!(err.contains("no model after the `:`"), "{err}");
        assert!(err.contains("a model is required"), "{err}");

        let err = parse_reviewer("code-review=llama").expect_err("no agent");
        assert!(err.contains("`llama` names no agent"), "{err}");
        assert!(err.contains("`<agent>:llama`"), "{err}");

        let err = parse_reviewer("=codex-acp:o3").expect_err("no skills");
        assert!(err.contains("no skills"), "{err}");

        // An `@` that says nothing after it is the same kind of typo, and the
        // refusal names the forms one of which was meant.
        let err = parse_reviewer("code-review=codex-acp:o3@").expect_err("no effort");
        assert!(err.contains("in \"code-review=codex-acp:o3@\""), "{err}");
        assert!(err.contains("no effort was named"), "{err}");
        assert!(err.contains("ariadne models ls"), "{err}");

        // An effort without the model it belongs to is still a missing model:
        // the `=` was written, so something was meant to follow it.
        let err = parse_reviewer("code-review=@high").expect_err("no model");
        assert!(err.contains("no model after the ="), "{err}");
        assert!(err.contains("SKILLS=MODEL@EFFORT"), "{err}");
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
        assert!(req.model.is_none(), "and the pin is left alone");
        assert!(req.reviewers.is_none());
        assert!(req.depends_on.is_none());
        assert!(req.landing.is_none(), "and so is how the task ends");

        let req = update_request(Edits {
            reviewers: vec![
                parse_reviewer("code-review=claude-agent-acp:claude-sonnet-5").expect("a model"),
                parse_reviewer("security-review=codex-acp:gpt-5.6-luna@high").expect("a model"),
            ],
            depends_on: vec!["01TASK".into()],
            ..Edits::default()
        })
        .expect("body");
        assert_eq!(
            req.reviewers.as_ref().map(|r| r
                .iter()
                .map(|a| (a.skills.join(","), a.model.as_str(), a.effort.as_deref()))
                .collect::<Vec<_>>()),
            Some(vec![
                (
                    "code-review".to_string(),
                    "claude-agent-acp:claude-sonnet-5",
                    None
                ),
                (
                    "security-review".to_string(),
                    "codex-acp:gpt-5.6-luna",
                    Some("high")
                )
            ])
        );
        assert_eq!(
            req.depends_on.as_deref(),
            Some(["01TASK".to_string()].as_slice())
        );

        let req = update_request(Edits {
            clear_depends_on: true,
            ..Edits::default()
        })
        .expect("body");
        assert_eq!(req.depends_on.as_deref(), Some([].as_slice()));

        // And the reviewers have their own way of saying the empty list, for
        // the same reason: a task with nothing to review is staffed with none.
        let req = update_request(Edits {
            no_reviewer: true,
            ..Edits::default()
        })
        .expect("body");
        assert_eq!(req.reviewers.as_ref().map(Vec::len), Some(0));
        assert!(req.depends_on.is_none(), "and nothing else was touched");
    }

    /// How a task ends is an edit like any other, while it has not started.
    #[test]
    fn how_the_task_ends_travels_as_the_three_endings_there_are() {
        for landing in [Landing::Merge, Landing::PullRequest, Landing::None] {
            let req = update_request(Edits {
                landing: Some(landing),
                ..Edits::default()
            })
            .expect("body");
            assert_eq!(req.landing, Some(landing));
            assert!(req.title.is_none(), "and nothing else was touched");
        }
    }

    /// What the author runs on is three answers, and the one field carries
    /// each of them: nothing said at all, back to auto, or an agent — with a
    /// model of it after the `:` where one was named.
    #[test]
    fn the_pin_travels_as_the_three_things_it_can_say() {
        for model in ["default", "codex-acp", "codex-acp:gpt-5.3-codex"] {
            let req = update_request(Edits {
                model: Some(model.into()),
                ..Edits::default()
            })
            .expect("body");
            assert_eq!(req.model.as_deref(), Some(model));
            assert!(req.title.is_none(), "and nothing else was touched");
        }
    }

    /// The effort travels beside the model and says the same three things:
    /// nothing at all, back to the agent's own, or one effort of the model —
    /// and an effort on its own is an edit like any other.
    #[test]
    fn the_effort_travels_the_way_the_pin_does() {
        for effort in ["xhigh", "default"] {
            let req = update_request(Edits {
                effort: Some(effort.into()),
                ..Edits::default()
            })
            .expect("body");
            assert_eq!(req.effort.as_deref(), Some(effort));
            assert!(req.model.is_none(), "the model it runs at is left alone");
        }

        let req = update_request(Edits {
            model: Some("claude-agent-acp:claude-opus-5".into()),
            effort: Some("xhigh".into()),
            ..Edits::default()
        })
        .expect("body");
        assert_eq!(req.model.as_deref(), Some("claude-agent-acp:claude-opus-5"));
        assert_eq!(req.effort.as_deref(), Some("xhigh"));
    }

    #[test]
    fn an_update_with_no_flags_is_refused_before_it_is_sent() {
        let err = update_request(Edits::default()).expect_err("no-op");
        assert!(err.to_string().starts_with("nothing to update"), "{err}");
        assert!(err.to_string().contains("--model"), "{err}");
        assert!(err.to_string().contains("--effort"), "{err}");
        assert!(err.to_string().contains("--landing"), "{err}");
    }
}
