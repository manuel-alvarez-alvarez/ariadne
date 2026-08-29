//! What a `task create` or `task update` line means before it is sent.
//!
//! Both are refused here rather than by the daemon where the answer is already
//! known: an update with nothing in it, a `--reviewer` whose model is missing
//! or names no agent CLI Ariadne runs, a `--reviewer` whose `@` is followed by
//! no effort, and a `--repo` that names none of the goal's repositories.

use anyhow::{Result, bail};

use ariadne_api::goals::GoalDto;
use ariadne_api::repositories::RepositoryDto;
use ariadne_api::tasks::{ReviewerAssignment, UpdateTaskRequest};
use ariadne_client::Client;

use crate::commands::{parse_effort, parse_model, resolve};

/// One `--reviewer PROFILE[=MODEL][@EFFORT]`: who reviews, and — after an `=`
/// — what that reviewer runs on instead of what its profile is on, in the one
/// spelling a model is chosen by, `<agent_kind>[:<model>]`, and — after an `@`
/// — the effort that model is reasoned at.
///
/// The `=` splits first and the last `@` after it splits the effort off, and
/// nothing else splits at all: an opencode id carries `/` and `:` of its own,
/// so `Reviewer=opencode:ollama/llama3:8b` reaches the request as that one id,
/// tag and all, and `Reviewer=opencode:ollama/llama3:8b@thinking` is the same
/// id run at `thinking`. An `@` with no `=` before it is a profile on its own
/// model at an effort of its own: `Reviewer@high`.
///
/// What is after the `=` is the same string `--model` takes, and it is refused
/// here in the same words; what is after the `@` is only checked for being
/// something, since which efforts a model takes is the daemon's to know.
pub fn parse_reviewer(s: &str) -> Result<ReviewerAssignment, String> {
    let (profile, model) = match s.split_once('=') {
        None => (s, None),
        Some((profile, model)) => (profile, Some(model)),
    };
    // The effort is cut off whichever half ends the string, so a profile with
    // no model of its own can still be run deeper: `Reviewer@high`.
    let (profile, model, effort) = match model {
        Some(model) => {
            let (model, effort) = split_effort(model);
            (profile, Some(model), effort)
        }
        None => {
            let (profile, effort) = split_effort(profile);
            (profile, None, effort)
        }
    };
    if profile.is_empty() {
        return Err(format!("no profile in \"{s}\" — {}", accepted()));
    }
    let effort = match effort {
        Some(effort) => Some(parse_effort(effort).map_err(|e| format!("in \"{s}\": {e}"))?),
        None => None,
    };
    let Some(model) = model else {
        return Ok(ReviewerAssignment {
            profile: profile.to_string(),
            model: None,
            effort,
        });
    };
    if model.is_empty() {
        return Err(format!(
            "no model after the = in \"{s}\" — {}, or {profile} on its own to run \
             it on whatever its profile is on",
            accepted()
        ));
    }
    let model = parse_model(model).map_err(|e| format!("in \"{s}\": {e}"))?;
    Ok(ReviewerAssignment {
        profile: profile.to_string(),
        model: Some(model),
        effort,
    })
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

/// What a `--reviewer` missing one of its halves is told it may write: the
/// four forms, and the spelling each half is in.
fn accepted() -> String {
    "write PROFILE, PROFILE=MODEL, PROFILE@EFFORT or PROFILE=MODEL@EFFORT, \
     where MODEL is an agent CLI (claude_code, codex, opencode) and, after a \
     colon, one model of it, and EFFORT is one of the efforts `ariadne models \
     ls` lists for that model"
        .to_string()
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
    model: Option<String>,
    effort: Option<String>,
    reviewers: Vec<ReviewerAssignment>,
    depends_on: Vec<String>,
    clear_depends_on: bool,
) -> Result<UpdateTaskRequest> {
    let req = UpdateTaskRequest {
        title,
        description,
        // Whatever was typed, in the daemon's own spelling — `default`
        // included, which is its word for handing the pins back to the
        // engineer profile's own.
        model,
        // The same three answers the model has, about how deeply it reasons:
        // nothing said, `default` for the CLI's own, or one effort of it.
        effort,
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
        && req.effort.is_none()
        && req.reviewers.is_none()
        && req.depends_on.is_none()
    {
        bail!(
            "nothing to update — pass --title, --description, --model, \
             --effort, --reviewer or --depends-on"
        );
    }
    Ok(req)
}

/// Reviewer assignments as the daemon should receive them: the profile half
/// resolved the way every other profile argument is, and what follows the `=`
/// and the `@` — the model that reviewer runs on, and the effort it runs at —
/// carried through untouched.
pub async fn resolved_reviewers(
    profiles: &mut resolve::Profiles<'_>,
    reviewers: Vec<ReviewerAssignment>,
) -> Result<Vec<ReviewerAssignment>> {
    let mut out = Vec::with_capacity(reviewers.len());
    for reviewer in reviewers {
        out.push(ReviewerAssignment {
            profile: profiles.id(&reviewer.profile).await?,
            model: reviewer.model,
            effort: reviewer.effort,
        });
    }
    Ok(out)
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
        assert_eq!(pick_repo(&repos, "/home/me/api").as_deref(), Some("01REPOAPI"));
        assert_eq!(pick_repo(&repos, "/home/me/other"), None);
        // And by the tail of an id, which is all a table of them shows.
        assert_eq!(pick_repo(&repos, "REPOUI").as_deref(), Some("01REPOUI"));
    }

    /// The profile half is resolved like every other profile argument; what
    /// follows the `=` is a model, and nothing here may touch it.
    #[tokio::test]
    async fn a_reviewer_keeps_what_it_runs_on_when_its_profile_is_resolved() {
        let mut profiles = resolve::Profiles::List(resolve::among(
            resolve::Kind::Profile,
            [resolve::Row {
                id: "01m0prof0000000000000abcde".into(),
                label: "Reviewer (reviewer)".into(),
                alias: Some("Reviewer".into()),
            }],
        ));
        let given = vec![
            parse_reviewer("Reviewer").expect("a name"),
            parse_reviewer("0000abcde=opencode:ollama/llama3:8b@thinking").expect("a short id"),
        ];
        let resolved = resolved_reviewers(&mut profiles, given)
            .await
            .expect("both resolved");
        assert_eq!(
            resolved
                .iter()
                .map(|r| (r.profile.as_str(), r.model.as_deref(), r.effort.as_deref()))
                .collect::<Vec<_>>(),
            [
                ("01m0prof0000000000000abcde", None, None),
                (
                    "01m0prof0000000000000abcde",
                    Some("opencode:ollama/llama3:8b"),
                    Some("thinking")
                ),
            ]
        );
    }

    /// A reviewer is a profile, and after an `=` what it runs on: an agent
    /// CLI, or one model of that CLI after the colon — the three forms
    /// `task create` and `task update` both take.
    #[test]
    fn a_reviewer_is_a_profile_and_what_it_runs_on() {
        let plain = parse_reviewer("Reviewer").expect("a profile on its own");
        assert_eq!(plain.profile, "Reviewer");
        assert_eq!(plain.model, None);

        let agent = parse_reviewer("Reviewer=codex").expect("an agent CLI");
        assert_eq!(agent.profile, "Reviewer");
        assert_eq!(
            agent.model.as_deref(),
            Some("codex"),
            "codex on its own default model"
        );

        let both = parse_reviewer("Reviewer=codex:gpt-5.3-codex").expect("a model of it");
        assert_eq!(both.model.as_deref(), Some("codex:gpt-5.3-codex"));

        // An opencode id is `provider/model` and may carry a tag of its own,
        // so what splits the model off is the `=` and the id arrives whole.
        let opencode = parse_reviewer("rev-strict=opencode:ollama/llama3:8b").expect("an id");
        assert_eq!(opencode.profile, "rev-strict");
        assert_eq!(opencode.model.as_deref(), Some("opencode:ollama/llama3:8b"));

        // The agent CLI answers to the hyphenated spelling too, the way
        // `--model` does, and travels as the daemon spells it.
        let hyphenated = parse_reviewer("Reviewer=claude-code").expect("a spelling");
        assert_eq!(hyphenated.model.as_deref(), Some("claude_code"));
    }

    /// The other half a slot may carry: the effort after the `@`, on a model
    /// of its own or on whatever its profile is on — and the `@` is the last
    /// one, so a model id keeps every `:` and `/` it came with.
    #[test]
    fn a_reviewer_runs_at_the_effort_after_the_at_sign() {
        let both =
            parse_reviewer("Reviewer=codex:gpt-5.6-sol@xhigh").expect("a model and an effort");
        assert_eq!(both.profile, "Reviewer");
        assert_eq!(both.model.as_deref(), Some("codex:gpt-5.6-sol"));
        assert_eq!(both.effort.as_deref(), Some("xhigh"));

        let effort = parse_reviewer("Reviewer@high").expect("an effort on its own");
        assert_eq!(effort.profile, "Reviewer");
        assert_eq!(
            effort.model, None,
            "the profile's own model, reasoned deeper"
        );
        assert_eq!(effort.effort.as_deref(), Some("high"));

        let model = parse_reviewer("Reviewer=codex").expect("a model on its own");
        assert_eq!(model.model.as_deref(), Some("codex"));
        assert_eq!(
            model.effort, None,
            "and no effort at all is codex's own default"
        );

        // The model half is cut at the last `@` and nothing else: an opencode
        // id is `provider/model` with a tag of its own, and all of it is model.
        let opencode =
            parse_reviewer("Reviewer=opencode:openrouter/x/y:z@high").expect("an opencode id");
        assert_eq!(opencode.model.as_deref(), Some("opencode:openrouter/x/y:z"));
        assert_eq!(opencode.effort.as_deref(), Some("high"));

        // Which efforts a model takes is the daemon's to know: anything that
        // is an effort at all travels, and is refused where the model is.
        let unknown =
            parse_reviewer("Reviewer=claude_code:claude-opus-5@ultra").expect("an effort");
        assert_eq!(unknown.effort.as_deref(), Some("ultra"));
    }

    /// Half a form is a typo, and it is refused where it was typed — with the
    /// forms it accepts, and, for the model half, the same words `--model`
    /// would have been refused in.
    #[test]
    fn a_reviewer_missing_a_half_is_refused_before_it_is_sent() {
        let err = parse_reviewer("Reviewer=").expect_err("no model");
        assert!(err.contains("no model after the ="), "{err}");
        assert!(err.contains("Reviewer on its own"), "{err}");
        assert!(err.contains("PROFILE=MODEL"), "{err}");
        assert!(err.contains("claude_code, codex, opencode"), "{err}");

        let err = parse_reviewer("Reviewer=codex:").expect_err("no model after the colon");
        assert!(err.contains("in \"Reviewer=codex:\""), "{err}");
        assert!(err.contains("no model after the `:`"), "{err}");

        let err = parse_reviewer("Reviewer=llama").expect_err("no such agent");
        assert!(err.contains("names no agent CLI"), "{err}");
        assert!(err.contains("claude_code:llama"), "{err}");
        assert!(err.contains("claude_code, codex, opencode"), "{err}");

        let err = parse_reviewer("Reviewer=llama:x").expect_err("no such agent");
        assert!(err.contains("unknown agent `llama`"), "{err}");

        let err = parse_reviewer("=codex").expect_err("no profile");
        assert!(err.contains("no profile"), "{err}");

        // An `@` that says nothing after it is the same kind of typo, and the
        // refusal names the forms one of which was meant.
        let err = parse_reviewer("Reviewer@").expect_err("no effort");
        assert!(err.contains("in \"Reviewer@\""), "{err}");
        assert!(err.contains("no effort was named"), "{err}");
        assert!(err.contains("ariadne models ls"), "{err}");

        // An effort without the model it belongs to is still a missing model:
        // the `=` was written, so something was meant to follow it.
        let err = parse_reviewer("Reviewer=@high").expect_err("no model");
        assert!(err.contains("no model after the ="), "{err}");
        assert!(err.contains("PROFILE=MODEL@EFFORT"), "{err}");
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
        assert!(req.model.is_none(), "and the pin is left alone");
        assert!(req.reviewers.is_none());
        assert!(req.depends_on.is_none());

        let req = update_request(
            None,
            None,
            None,
            None,
            vec![
                ReviewerAssignment::of("Reviewer"),
                parse_reviewer("rev-strict=codex:gpt-5.6-luna@high").expect("a model"),
            ],
            vec!["01TASK".into()],
            false,
        )
        .expect("body");
        assert_eq!(
            req.reviewers.as_ref().map(|r| r
                .iter()
                .map(|a| (a.profile.as_str(), a.model.as_deref(), a.effort.as_deref()))
                .collect::<Vec<_>>()),
            Some(vec![
                ("Reviewer", None, None),
                ("rev-strict", Some("codex:gpt-5.6-luna"), Some("high"))
            ])
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
        let req = update_request(
            None,
            None,
            Some("default".into()),
            None,
            vec![],
            vec![],
            false,
        )
        .expect("body");
        assert_eq!(req.model.as_deref(), Some("default"));
        assert!(req.title.is_none(), "and nothing else was touched");

        let req = update_request(
            None,
            None,
            Some("codex".into()),
            None,
            vec![],
            vec![],
            false,
        )
        .expect("body");
        assert_eq!(
            req.model.as_deref(),
            Some("codex"),
            "codex on its own default model"
        );

        let req = update_request(
            None,
            None,
            Some("codex:gpt-5.3-codex".into()),
            None,
            vec![],
            vec![],
            false,
        )
        .expect("body");
        assert_eq!(req.model.as_deref(), Some("codex:gpt-5.3-codex"));
    }

    /// The effort travels beside the model and says the same three things:
    /// nothing at all, back to the CLI's own, or one effort of the model —
    /// and an effort on its own is an edit like any other.
    #[test]
    fn the_effort_travels_the_way_the_pin_does() {
        let req = update_request(
            None,
            None,
            None,
            Some("xhigh".into()),
            vec![],
            vec![],
            false,
        )
        .expect("body");
        assert_eq!(req.effort.as_deref(), Some("xhigh"));
        assert!(req.model.is_none(), "the model it runs at is left alone");

        let req = update_request(
            None,
            None,
            None,
            Some("default".into()),
            vec![],
            vec![],
            false,
        )
        .expect("body");
        assert_eq!(req.effort.as_deref(), Some("default"));

        let req = update_request(
            None,
            None,
            Some("claude_code:claude-opus-5".into()),
            Some("xhigh".into()),
            vec![],
            vec![],
            false,
        )
        .expect("body");
        assert_eq!(req.model.as_deref(), Some("claude_code:claude-opus-5"));
        assert_eq!(req.effort.as_deref(), Some("xhigh"));
    }

    #[test]
    fn an_update_with_no_flags_is_refused_before_it_is_sent() {
        let err = update_request(None, None, None, None, vec![], vec![], false).expect_err("no-op");
        assert!(err.to_string().starts_with("nothing to update"), "{err}");
        assert!(err.to_string().contains("--model"), "{err}");
        assert!(err.to_string().contains("--effort"), "{err}");
    }
}
