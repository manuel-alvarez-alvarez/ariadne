//! What a `task create` or `task update` line means before it is sent.
//!
//! Both are refused here rather than by the daemon where the answer is already
//! known: an update with nothing in it, and a `--repo` that names none of the
//! goal's repositories.

use anyhow::{Result, bail};

use ariadne_api::goals::GoalDto;
use ariadne_api::repositories::RepositoryDto;
use ariadne_api::tasks::UpdateTaskRequest;
use ariadne_client::Client;

/// The PATCH body of `task update`, or the reason there is nothing to send.
///
/// A flag that was not given is `None` — the field keeps what the task has.
/// The two list flags are all-or-nothing by design: they replace the list they
/// name, and `--clear-depends-on` is how an empty one is spelled, since a
/// repeatable flag cannot be given zero times on purpose.
pub fn update_request(
    title: Option<String>,
    description: Option<String>,
    reviewers: Vec<String>,
    depends_on: Vec<String>,
    clear_depends_on: bool,
) -> Result<UpdateTaskRequest> {
    let req = UpdateTaskRequest {
        title,
        description,
        reviewer_profiles: (!reviewers.is_empty()).then_some(reviewers),
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
        && req.reviewer_profiles.is_none()
        && req.depends_on.is_none()
    {
        bail!(
            "nothing to update — pass --title, --description, --reviewer \
             or --depends-on"
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

    /// The lists replace rather than extend, so an absent flag must not send an
    /// empty list and wipe what the task has — and the one thing a repeatable
    /// flag cannot say on its own is spelled `--clear-depends-on`.
    #[test]
    fn only_the_flags_that_were_given_reach_the_daemon() {
        let req = update_request(Some("new".into()), None, vec![], vec![], false).expect("body");
        assert_eq!(req.title.as_deref(), Some("new"));
        assert!(req.description.is_none());
        assert!(req.reviewer_profiles.is_none());
        assert!(req.depends_on.is_none());

        let req = update_request(
            None,
            None,
            vec!["Reviewer".into(), "rev-strict".into()],
            vec!["01TASK".into()],
            false,
        )
        .expect("body");
        assert_eq!(
            req.reviewer_profiles.as_deref(),
            Some(["Reviewer".to_string(), "rev-strict".to_string()].as_slice())
        );
        assert_eq!(
            req.depends_on.as_deref(),
            Some(["01TASK".to_string()].as_slice())
        );

        let req = update_request(None, None, vec![], vec![], true).expect("body");
        assert_eq!(req.depends_on.as_deref(), Some([].as_slice()));
    }

    #[test]
    fn an_update_with_no_flags_is_refused_before_it_is_sent() {
        let err = update_request(None, None, vec![], vec![], false).expect_err("no-op");
        assert!(err.to_string().starts_with("nothing to update"), "{err}");
    }
}
