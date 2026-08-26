//! What a `task create` or `task update` line means before it is sent.
//!
//! Both are refused here rather than by the daemon where the answer is already
//! known: an update with nothing in it, a `--reviewer` that says nothing after
//! its `=`, and a `--repo` that names none of the goal's repositories.

use anyhow::{Result, bail};

use ariadne_api::goals::GoalDto;
use ariadne_api::repositories::RepositoryDto;
use ariadne_api::tasks::{ReviewerAssignment, UpdateTaskRequest};
use ariadne_client::Client;

/// One `--reviewer PROFILE[=MODEL]`: who reviews, and — after an `=` — the
/// model that reviewer is to run on instead of its profile's.
///
/// The agent CLI is not spelled anywhere: a model belongs to one, and the
/// daemon is what places it, so a model it cannot place comes back as a
/// refusal naming the model rather than as a flag nobody could have typed.
pub fn parse_reviewer(s: &str) -> Result<ReviewerAssignment, String> {
    let (profile, model) = match s.split_once('=') {
        None => (s, None),
        Some((profile, model)) => (profile, Some(model)),
    };
    if profile.is_empty() {
        return Err(format!(
            "no profile in \"{s}\" — write PROFILE or PROFILE=MODEL"
        ));
    }
    if model.is_some_and(str::is_empty) {
        return Err(format!(
            "no model after the = in \"{s}\" — write {profile} on its own to \
             run it on its profile's model"
        ));
    }
    Ok(ReviewerAssignment {
        profile: profile.to_string(),
        model: model.map(str::to_string),
    })
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
    reviewers: Vec<ReviewerAssignment>,
    depends_on: Vec<String>,
    clear_depends_on: bool,
) -> Result<UpdateTaskRequest> {
    let req = UpdateTaskRequest {
        title,
        description,
        // "default" travels as it was typed: it is the daemon's spelling for
        // running on the engineer profile's model again, the same word
        // `profile update --model default` takes.
        model,
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
            "nothing to update — pass --title, --description, --model, \
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

    /// A reviewer is a profile, and after an `=` the model it runs on: the
    /// two forms `task create` and `task update` both take.
    #[test]
    fn a_reviewer_is_a_profile_and_the_model_it_runs_on() {
        let plain = parse_reviewer("Reviewer").expect("a profile on its own");
        assert_eq!(plain.profile, "Reviewer");
        assert_eq!(plain.model, None);

        let chosen = parse_reviewer("Reviewer=gpt-5.3-codex").expect("a model");
        assert_eq!(chosen.profile, "Reviewer");
        assert_eq!(chosen.model.as_deref(), Some("gpt-5.3-codex"));

        // An opencode id carries a `/` and a `:`; only the first `=` splits.
        let opencode = parse_reviewer("rev-strict=ollama/llama3:8b").expect("a provider id");
        assert_eq!(opencode.profile, "rev-strict");
        assert_eq!(opencode.model.as_deref(), Some("ollama/llama3:8b"));
    }

    /// Half a form is a typo, and both halves are named where they are
    /// missing rather than sent on to be refused elsewhere.
    #[test]
    fn a_reviewer_missing_a_half_is_refused_before_it_is_sent() {
        let err = parse_reviewer("Reviewer=").expect_err("no model");
        assert!(err.contains("no model after the ="), "{err}");
        assert!(err.contains("Reviewer on its own"), "{err}");

        let err = parse_reviewer("=gpt-5.3-codex").expect_err("no profile");
        assert!(err.contains("no profile"), "{err}");
    }

    /// The lists replace rather than extend, so an absent flag must not send an
    /// empty list and wipe what the task has — and the one thing a repeatable
    /// flag cannot say on its own is spelled `--clear-depends-on`.
    #[test]
    fn only_the_flags_that_were_given_reach_the_daemon() {
        let req =
            update_request(Some("new".into()), None, None, vec![], vec![], false).expect("body");
        assert_eq!(req.title.as_deref(), Some("new"));
        assert!(req.description.is_none());
        assert!(req.model.is_none());
        assert!(req.reviewers.is_none());
        assert!(req.depends_on.is_none());

        let req = update_request(
            None,
            None,
            None,
            vec![
                ReviewerAssignment::of("Reviewer"),
                parse_reviewer("rev-strict=o3").expect("a model"),
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
            Some(vec![("Reviewer", None), ("rev-strict", Some("o3"))])
        );
        assert_eq!(
            req.depends_on.as_deref(),
            Some(["01TASK".to_string()].as_slice())
        );

        let req = update_request(None, None, None, vec![], vec![], true).expect("body");
        assert_eq!(req.depends_on.as_deref(), Some([].as_slice()));
    }

    /// `--model default` is the daemon's word for the engineer profile's own
    /// model, and it travels as typed — the same word `profile update` takes.
    #[test]
    fn clearing_the_model_travels_as_the_word_default() {
        let req = update_request(None, None, Some("default".into()), vec![], vec![], false)
            .expect("body");
        assert_eq!(req.model.as_deref(), Some("default"));
        assert!(req.title.is_none(), "and nothing else was touched");

        let req = update_request(
            None,
            None,
            Some("gpt-5.3-codex".into()),
            vec![],
            vec![],
            false,
        )
        .expect("body");
        assert_eq!(req.model.as_deref(), Some("gpt-5.3-codex"));
    }

    #[test]
    fn an_update_with_no_flags_is_refused_before_it_is_sent() {
        let err = update_request(None, None, None, vec![], vec![], false).expect_err("no-op");
        assert!(err.to_string().starts_with("nothing to update"), "{err}");
        assert!(err.to_string().contains("--model"), "{err}");
    }
}
