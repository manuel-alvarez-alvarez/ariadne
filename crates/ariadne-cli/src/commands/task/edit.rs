//! What a `task create` or `task update` line means before it is sent.
//!
//! Both are refused here rather than by the daemon where the answer is already
//! known: an update with nothing in it, an `--agent` or a `--reviewer` whose
//! agent is missing or is no agent CLI Ariadne runs, and a `--repo` that names
//! none of the goal's repositories.

use anyhow::{Result, bail};

use ariadne_api::goals::GoalDto;
use ariadne_api::repositories::RepositoryDto;
use ariadne_api::tasks::{ReviewerAssignment, UpdateTaskRequest};
use ariadne_client::Client;
use ariadne_core::AgentKind;

use crate::commands::{parse_agent, qualified_model};

/// The daemon's word for "the profile's own agent and model", which
/// `task update --agent` takes as the third thing it can say.
const DEFAULT: &str = "default";

/// One `task update --agent`, as the word the daemon reads it by: an agent
/// kind in its wire spelling, or `default` for the engineer profile's own
/// agent and model.
///
/// A `String` rather than an [`AgentKind`] because the field says one thing
/// more than a kind can — but only the words it may say get through, so
/// `--agent llama` is refused on the line it was typed on rather than sent to
/// the daemon to be refused there. The hyphenated spelling is normalised here
/// too, the way `--agent` normalises it everywhere else.
pub fn parse_agent_or_default(s: &str) -> Result<String, String> {
    if s == DEFAULT {
        return Ok(DEFAULT.to_string());
    }
    match parse_agent(s) {
        Ok(kind) => Ok(kind.as_str().to_string()),
        Err(_) => Err(format!(
            "unknown agent \"{s}\" — write one of {}, or \"{DEFAULT}\" to put the \
             task back on its engineer profile's own agent and model",
            kinds()
        )),
    }
}

/// One `--reviewer PROFILE[=AGENT[:MODEL]]`: who reviews, and — after an `=` —
/// the agent CLI that reviewer runs on instead of its profile's, narrowed by a
/// `:MODEL` to one model of that CLI.
///
/// The agent is the choice and the model only narrows it: a model belongs to
/// an agent CLI, and nothing here derives one from the other. The `=` splits
/// first and the `:` only after it, so an opencode id — `provider/model`,
/// which carries no colon — arrives whole. What is after the `=` is already
/// the `agent[:model]` the request carries, so it travels as one string.
pub fn parse_reviewer(s: &str) -> Result<ReviewerAssignment, String> {
    let (profile, pin) = match s.split_once('=') {
        None => (s, None),
        Some((profile, pin)) => (profile, Some(pin)),
    };
    if profile.is_empty() {
        return Err(format!("no profile in \"{s}\" — {}", accepted()));
    }
    let Some(pin) = pin else {
        return Ok(ReviewerAssignment::of(profile));
    };
    let (agent, model) = match pin.split_once(':') {
        None => (pin, None),
        Some((agent, model)) => (agent, Some(model)),
    };
    if agent.is_empty() {
        return Err(format!(
            "no agent after the = in \"{s}\" — {}, or {profile} on its own to run \
             it on its profile's agent and model",
            accepted()
        ));
    }
    if model.is_some_and(str::is_empty) {
        return Err(format!(
            "no model after the : in \"{s}\" — {}, or {profile}={agent} to run it \
             on that CLI's own default model",
            accepted()
        ));
    }
    let agent = parse_agent(agent)
        .map_err(|_| format!("unknown agent \"{agent}\" in \"{s}\" — {}", accepted()))?;
    Ok(ReviewerAssignment {
        profile: profile.to_string(),
        model: qualified_model(Some(agent.as_str()), model),
    })
}

/// What every refusal of a `--reviewer` ends with: the form it accepts, and
/// the agent CLIs that can stand in it — the two things the typo was between.
fn accepted() -> String {
    format!(
        "write PROFILE, PROFILE=AGENT or PROFILE=AGENT:MODEL, where AGENT is one of {}",
        kinds()
    )
}

/// The agent CLIs an `AGENT` may name, as a refusal lists them.
fn kinds() -> String {
    AgentKind::ALL
        .iter()
        .map(|k| k.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// The PATCH body of `task update`, or the reason there is nothing to send.
///
/// A flag that was not given is `None` — the field keeps what the task has.
/// The two list flags are all-or-nothing by design: they replace the list they
/// name, and `--clear-depends-on` is how an empty one is spelled, since a
/// repeatable flag cannot be given zero times on purpose.
pub fn update_request(
    title: Option<String>,
    description: Option<String>,
    agent: Option<String>,
    model: Option<String>,
    reviewers: Vec<ReviewerAssignment>,
    depends_on: Vec<String>,
    clear_depends_on: bool,
) -> Result<UpdateTaskRequest> {
    let req = UpdateTaskRequest {
        title,
        description,
        // The two flags are one field on the wire, and "default" survives it:
        // it is the daemon's word for handing the pins back to the engineer
        // profile's own, the same word `profile update` takes for its "auto".
        // A `--model` never arrives on its own — clap refuses one with no
        // `--agent` beside it.
        model: qualified_model(agent.as_deref(), model.as_deref()),
        reviewers: (!reviewers.is_empty()).then_some(reviewers),
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
        && req.model.is_none()
        && req.reviewers.is_none()
        && req.depends_on.is_none()
    {
        bail!(
            "nothing to update — pass --title, --description, --agent, \
             --reviewer or --depends-on"
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

/// The id of the goal repository a `--repo` argument names, by id or by path.
fn pick_repo(repos: &[RepositoryDto], spec: &str) -> Option<String> {
    repos
        .iter()
        .find(|r| r.id == spec || r.path == spec)
        .map(|r| r.id.clone())
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
        assert_eq!(pick_repo(&repos, "/home/me/api").as_deref(), Some("01REPOAPI"));
        assert_eq!(pick_repo(&repos, "/home/me/other"), None);
    }

    /// `task update --agent` says one of exactly three things, and the two it
    /// may spell reach the daemon in its own words: an agent kind in its wire
    /// spelling, hyphens and all, or `default`.
    #[test]
    fn the_engineers_agent_is_a_kind_or_the_word_default() {
        assert_eq!(parse_agent_or_default("default").unwrap(), "default");
        assert_eq!(parse_agent_or_default("codex").unwrap(), "codex");
        assert_eq!(
            parse_agent_or_default("claude-code").unwrap(),
            "claude_code",
            "the hyphenated spelling names the same CLI, and travels as the \
             daemon spells it"
        );
    }

    /// Anything else is a typo, and it is refused where it was typed, with
    /// every word the flag does take listed.
    #[test]
    fn an_agent_that_is_no_cli_is_refused_before_it_is_sent() {
        let err = parse_agent_or_default("llama").expect_err("no such agent");
        assert!(err.contains("unknown agent \"llama\""), "{err}");
        assert!(err.contains("claude_code, codex, opencode"), "{err}");
        assert!(err.contains("\"default\""), "{err}");

        assert!(
            parse_agent_or_default("").is_err(),
            "and an empty --agent says nothing at all"
        );
    }

    /// A reviewer is a profile, and after an `=` the agent CLI it runs on,
    /// with a `:MODEL` where one model of that CLI is meant: the three forms
    /// `task create` and `task update` both take.
    #[test]
    fn a_reviewer_is_a_profile_and_what_it_runs_on() {
        let plain = parse_reviewer("Reviewer").expect("a profile on its own");
        assert_eq!(plain.profile, "Reviewer");
        assert_eq!(plain.model, None);

        let agent = parse_reviewer("Reviewer=codex").expect("an agent");
        assert_eq!(agent.profile, "Reviewer");
        assert_eq!(
            agent.model.as_deref(),
            Some("codex"),
            "codex on its own default model"
        );

        let both = parse_reviewer("Reviewer=codex:gpt-5.3-codex").expect("an agent and a model");
        assert_eq!(both.model.as_deref(), Some("codex:gpt-5.3-codex"));

        // An opencode id is `provider/model` and carries no colon of its own,
        // so the one that splits is the first and the id arrives whole.
        let opencode = parse_reviewer("rev-strict=opencode:ollama/llama3:8b").expect("an id");
        assert_eq!(opencode.profile, "rev-strict");
        assert_eq!(opencode.model.as_deref(), Some("opencode:ollama/llama3:8b"));

        // The agent CLI answers to the hyphenated spelling too, the way
        // `--agent` does, and travels as the daemon spells it.
        let hyphenated = parse_reviewer("Reviewer=claude-code").expect("a spelling");
        assert_eq!(hyphenated.model.as_deref(), Some("claude_code"));
    }

    /// Half a form is a typo, and it is refused where it was typed, with the
    /// form and the agent CLIs that stand in it — not sent on to be refused
    /// elsewhere.
    #[test]
    fn a_reviewer_missing_a_half_is_refused_before_it_is_sent() {
        let err = parse_reviewer("Reviewer=").expect_err("no agent");
        assert!(err.contains("no agent after the ="), "{err}");
        assert!(err.contains("Reviewer on its own"), "{err}");
        assert!(err.contains("PROFILE=AGENT:MODEL"), "{err}");
        assert!(err.contains("claude_code, codex, opencode"), "{err}");

        let err = parse_reviewer("Reviewer=codex:").expect_err("no model");
        assert!(err.contains("no model after the :"), "{err}");
        assert!(err.contains("Reviewer=codex"), "{err}");

        let err = parse_reviewer("Reviewer=llama").expect_err("no such agent");
        assert!(err.contains("unknown agent \"llama\""), "{err}");
        assert!(err.contains("claude_code, codex, opencode"), "{err}");

        let err = parse_reviewer("=codex").expect_err("no profile");
        assert!(err.contains("no profile"), "{err}");
    }

    /// The lists replace rather than extend, so an absent flag must not send an
    /// empty list and wipe what the task has — and the one thing a repeatable
    /// flag cannot say on its own is spelled `--clear-depends-on`.
    #[test]
    fn only_the_flags_that_were_given_reach_the_daemon() {
        let req = update_request(Some("new".into()), None, None, None, vec![], vec![], false)
            .expect("body");
        assert_eq!(req.title.as_deref(), Some("new"));
        assert!(req.description.is_none());
        assert!(req.model.is_none(), "and the pins are left alone");
        assert!(req.reviewers.is_none());
        assert!(req.depends_on.is_none());

        let req = update_request(
            None,
            None,
            None,
            None,
            vec![
                ReviewerAssignment::of("Reviewer"),
                parse_reviewer("rev-strict=codex:o3").expect("an agent and a model"),
            ],
            vec!["01TASK".into()],
            false,
        )
        .expect("body");
        assert_eq!(
            req.reviewers.as_ref().map(|r| r
                .iter()
                .map(|a| (a.profile.as_str(), a.model.as_deref()))
                .collect::<Vec<_>>()),
            Some(vec![("Reviewer", None), ("rev-strict", Some("codex:o3"))])
        );
        assert_eq!(
            req.depends_on.as_deref(),
            Some(["01TASK".to_string()].as_slice())
        );

        let req = update_request(None, None, None, None, vec![], vec![], true).expect("body");
        assert_eq!(req.depends_on.as_deref(), Some([].as_slice()));
    }

    /// What the engineer runs on is three answers, and the one field carries
    /// each of them: nothing said at all, back to the profile's own, or an
    /// agent CLI — with a model of it after the `:` where one was named.
    #[test]
    fn the_pin_travels_as_the_three_things_it_can_say() {
        let req = update_request(None, None, Some("default".into()), None, vec![], vec![], false)
            .expect("body");
        assert_eq!(req.model.as_deref(), Some("default"));
        assert!(req.title.is_none(), "and nothing else was touched");

        let req = update_request(None, None, Some("codex".into()), None, vec![], vec![], false)
            .expect("body");
        assert_eq!(
            req.model.as_deref(),
            Some("codex"),
            "codex on its own default model"
        );

        let req = update_request(
            None,
            None,
            Some("codex".into()),
            Some("gpt-5.3-codex".into()),
            vec![],
            vec![],
            false,
        )
        .expect("body");
        assert_eq!(req.model.as_deref(), Some("codex:gpt-5.3-codex"));
    }

    #[test]
    fn an_update_with_no_flags_is_refused_before_it_is_sent() {
        let err = update_request(None, None, None, None, vec![], vec![], false).expect_err("no-op");
        assert!(err.to_string().starts_with("nothing to update"), "{err}");
        assert!(err.to_string().contains("--agent"), "{err}");
    }
}
