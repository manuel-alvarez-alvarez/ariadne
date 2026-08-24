//! The GitHub CLI, as much of it as watching a pull request takes.
//!
//! A task published as a pull request leaves the daemon nothing to reconcile
//! locally: the branch is on GitHub, humans review it there and one of them
//! merges it. What the daemon can do is look, which is what this is —
//! `gh pr view --json …`, parsed, plus the reading of one poll that decides
//! whether anybody has to be woken for it (see [`poll_state`]).
//!
//! Shelling out to `gh` rather than talking to the API directly, for the
//! reason [`crate::gitwt`] shells out to git: `gh` already holds the user's
//! credentials, and asking it is the same thing the integrator's own
//! instructions tell it to do.
//!
//! What a poll of any forge means, and which forge a recorded URL is on at
//! all, is [`crate::forge`]'s; [`crate::glab`] is this module's opposite
//! number for a merge request on GitLab.

use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use tokio::process::Command;

use crate::forge::{self, Conflict, FailedCheck, Forge, Health, Landing};

pub use crate::forge::{
    Feedback, GITHUB_HOST, PrState, WatchedPr, parse_pages, pull_request_number,
};

/// The `gh pr view --json` fields a poll asks for.
const VIEW_FIELDS: &str = "number,state,mergedAt,mergeCommit,reviewDecision,reviews,comments,\
                           statusCheckRollup,mergeable,mergeStateStatus,baseRefName,headRefOid";

/// What GitHub answers `mergeable` with when the branch no longer merges into
/// its base. The other two are `MERGEABLE` and `UNKNOWN` — a merge it has not
/// worked out yet, which is not a conflict.
const CONFLICTING: &str = "CONFLICTING";

/// And what `mergeStateStatus` calls the same thing. The rest of its
/// vocabulary is about everything else standing in the way — `BEHIND`,
/// `BLOCKED`, `UNSTABLE` — none of which is the branch failing to merge.
const DIRTY: &str = "DIRTY";

/// The conclusions a finished check run is red with, and the states a commit
/// status is.
///
/// What is not here is as deliberate: `CANCELLED` is what a workflow that
/// cancels its own superseded runs writes on every push, `NEUTRAL` and
/// `SKIPPED` are green enough for GitHub's own merge button, and a conclusion
/// this does not know is unknown rather than red.
const FAILED_CONCLUSIONS: [&str; 5] = [
    "FAILURE",
    "TIMED_OUT",
    "STARTUP_FAILURE",
    "ACTION_REQUIRED",
    "ERROR",
];

#[derive(Debug, Clone)]
pub struct GhCli {
    bin: String,
}

/// One pull request, as much of it as `--json` was asked for.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRequest {
    /// `OPEN`, `MERGED` or `CLOSED`.
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub merged_at: Option<String>,
    /// The commit the merge landed as — a squash or rebase merge writes one
    /// that is on no branch of ours until the base is fetched.
    #[serde(default)]
    pub merge_commit: Option<Commit>,
    /// `APPROVED`, `CHANGES_REQUESTED`, `REVIEW_REQUIRED`, or absent on a
    /// repository that requires no review at all.
    #[serde(default)]
    pub review_decision: Option<String>,
    #[serde(default)]
    pub reviews: Vec<Review>,
    #[serde(default)]
    pub comments: Vec<Comment>,
    /// Every check run and commit status on the head commit, as GitHub rolls
    /// them up. Absent on a repository with no checks at all, which is no
    /// checks failing rather than a poll that could not tell.
    #[serde(default)]
    pub status_check_rollup: Vec<StatusCheck>,
    /// `MERGEABLE`, `CONFLICTING` or `UNKNOWN` — the last being a merge
    /// GitHub has not worked out yet, on a background job of its own.
    #[serde(default)]
    pub mergeable: Option<String>,
    /// `CLEAN`, `DIRTY`, `BEHIND`, `BLOCKED`, `UNSTABLE`, `UNKNOWN`: the same
    /// question asked the other way, and the one that survives on requests
    /// where `mergeable` stays unknown.
    #[serde(default)]
    pub merge_state_status: Option<String>,
    /// The branch it is open against, which is what the engineer merges in to
    /// reconcile a conflict.
    #[serde(default)]
    pub base_ref_name: Option<String>,
    /// The commit the checks ran on, and the conflict was read on: what keeps
    /// one failure to one relay, and makes the failure on the revision that
    /// answered it a new one.
    #[serde(default)]
    pub head_ref_oid: Option<String>,
}

/// One entry of the status check rollup, in either of the two shapes GitHub
/// answers with: a check run, from an app such as Actions, with a `name` and
/// a `conclusion`; or a commit status, from anything that posts one, with a
/// `context` and a `state`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusCheck {
    /// A check run's name.
    #[serde(default)]
    pub name: Option<String>,
    /// A commit status's, which is what it is called instead.
    #[serde(default)]
    pub context: Option<String>,
    /// `QUEUED`, `IN_PROGRESS`, `COMPLETED`: a check run that has not
    /// completed has no verdict yet, whatever its conclusion field says.
    #[serde(default)]
    pub status: Option<String>,
    /// A finished check run's verdict.
    #[serde(default)]
    pub conclusion: Option<String>,
    /// A commit status's, which carries `PENDING` for one still running.
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub details_url: Option<String>,
    #[serde(default)]
    pub target_url: Option<String>,
}

impl StatusCheck {
    /// What GitHub calls it, whichever of the two shapes it came in.
    fn name(&self) -> String {
        self.name
            .as_deref()
            .or(self.context.as_deref())
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .map_or_else(|| "a check".to_string(), str::to_string)
    }

    /// The verdict it finished with, if it has finished: a check run's
    /// conclusion once its status says `COMPLETED`, a commit status's state
    /// once that is anything but `PENDING`.
    fn verdict(&self) -> Option<&str> {
        let running =
            |s: &str| s.eq_ignore_ascii_case("QUEUED") || s.eq_ignore_ascii_case("IN_PROGRESS");
        if self.status.as_deref().is_some_and(running) {
            return None;
        }
        let verdict = self
            .conclusion
            .as_deref()
            .or(self.state.as_deref())
            .map(str::trim)
            .filter(|v| !v.is_empty())?;
        (!verdict.eq_ignore_ascii_case("PENDING") && !verdict.eq_ignore_ascii_case("EXPECTED"))
            .then_some(verdict)
    }

    /// Whether GitHub is reporting it as red. Anything it has not finished,
    /// and any verdict this does not know, is neither passing nor failing.
    fn failed(&self) -> bool {
        self.verdict().is_some_and(|verdict| {
            FAILED_CONCLUSIONS
                .iter()
                .any(|failed| verdict.eq_ignore_ascii_case(failed))
        })
    }

    fn url(&self) -> Option<String> {
        self.details_url
            .as_deref()
            .or(self.target_url.as_deref())
            .map(str::trim)
            .filter(|u| !u.is_empty())
            .map(str::to_string)
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Commit {
    pub oid: String,
}

/// One submitted review: its verdict, and whatever the reviewer wrote with it.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Review {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub author: Option<Author>,
    #[serde(default)]
    pub body: String,
    /// `APPROVED`, `CHANGES_REQUESTED`, `COMMENTED`, `DISMISSED`.
    #[serde(default)]
    pub state: String,
}

/// One comment on the conversation tab.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Comment {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub author: Option<Author>,
    #[serde(default)]
    pub body: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Author {
    #[serde(default)]
    pub login: String,
}

impl PullRequest {
    /// What GitHub says became of the branch: merged or not, and the commit
    /// the merge landed as.
    pub fn landing(&self) -> Landing {
        Landing {
            state: self.state.clone(),
            merged: self.state.eq_ignore_ascii_case("MERGED") || self.merged_at.is_some(),
            commits: self
                .merge_commit
                .as_ref()
                .map(|c| c.oid.clone())
                .into_iter()
                .collect(),
        }
    }

    /// Whether nothing stands between it and its merge, as `reviewDecision`
    /// reports — `APPROVED` where the repository requires a review, and
    /// absent or empty where it requires none at all.
    ///
    /// A repository that gates nothing is the case worth spelling out: there
    /// is no approval coming, because there is nobody the pull request is
    /// waiting on but the person who opened it. Reading that as "not
    /// approved" is how a published task went unannounced until somebody
    /// happened to look at the forge. It is also what GitLab already answers
    /// for a project with no approval rules, so the two forges now say the
    /// same thing about the same situation.
    pub fn is_approved(&self) -> bool {
        match self.review_decision.as_deref().map(str::trim) {
            None | Some("") => true,
            Some(decision) => decision.eq_ignore_ascii_case("APPROVED"),
        }
    }

    /// Whether it is still open, which is what an approval worth announcing
    /// needs: a closed pull request is nobody's to merge.
    pub fn is_open(&self) -> bool {
        self.state.eq_ignore_ascii_case("OPEN")
    }

    /// What GitHub says about the branch itself: whether it still merges into
    /// its base, and which of its checks are red.
    ///
    /// Both degrade to "nothing to see" rather than to a failure — a
    /// `mergeable` GitHub has not worked out yet, a rollup a repository with
    /// no checks answers empty, a conclusion this does not know: an engineer
    /// woken for a build nobody said had failed is an engineer woken for
    /// nothing.
    pub fn health(&self) -> Health {
        let head = self.head_ref_oid.as_deref().filter(|s| !s.is_empty());
        let conflicting = self
            .mergeable
            .as_deref()
            .is_some_and(|m| m.eq_ignore_ascii_case(CONFLICTING))
            || self
                .merge_state_status
                .as_deref()
                .is_some_and(|m| m.eq_ignore_ascii_case(DIRTY));
        Health {
            conflict: conflicting.then(|| Conflict {
                id: forge::conflict_id(head),
                base: self
                    .base_ref_name
                    .as_deref()
                    .map(str::trim)
                    .filter(|b| !b.is_empty())
                    .map(str::to_string),
            }),
            failed_checks: self
                .status_check_rollup
                .iter()
                .filter(|check| check.failed())
                .map(|check| {
                    let name = check.name();
                    FailedCheck {
                        id: forge::check_id(head, &name),
                        name,
                        conclusion: check.verdict().unwrap_or_default().to_string(),
                        url: check.url(),
                    }
                })
                .collect(),
        }
    }
}

/// One comment on the diff itself, as the REST API answers for it.
///
/// A different shape from the conversation's — a numeric id and a `user`
/// rather than an `author` — because it comes from a different place: `gh pr
/// view --json comments` has the conversation tab and nothing of the review
/// threads, which is where most of what a reviewer says actually lives.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ReviewComment {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub user: Option<Author>,
    #[serde(default)]
    pub body: String,
    /// The file it hangs on, when it still hangs on one.
    #[serde(default)]
    pub path: Option<String>,
    /// The line of that file it hangs on, in the version being reviewed —
    /// absent on a comment whose lines the diff has since moved out from
    /// under.
    #[serde(default)]
    pub line: Option<i64>,
}

impl ReviewComment {
    /// As one piece of feedback: the file and line it is about travel beside
    /// what it says, since that is what the engineer needs to find it.
    fn as_feedback(&self) -> Feedback {
        Feedback {
            id: format!("RC{}", self.id),
            author: login(&self.user),
            body: self.body.trim().to_string(),
            file: forge::location(self.path.as_deref(), self.line),
            blocking: false,
        }
    }
}

/// Read one poll: the pull request as `gh` reports it, against what the task
/// already remembers of it.
///
/// `relayed` is the comment and review ids already handed to the engineer,
/// and `approved_notified` whether the user has already been told this pull
/// request is ready to merge — both live on the task, so a daemon that
/// restarts mid-review picks up where it left off rather than repeating
/// itself.
pub fn poll_state(
    pr: &PullRequest,
    review_comments: &[ReviewComment],
    relayed: &[String],
    approved_notified: bool,
) -> PrState {
    forge::poll_state(
        pr.landing().merged,
        forge::unrelayed(feedback(pr, review_comments), relayed),
        &pr.health(),
        relayed,
        pr.is_approved() && pr.is_open(),
        approved_notified,
    )
}

/// What humans wrote on the pull request, in the order the engineer reads it.
///
/// An approving review is not feedback however warmly it is worded, and
/// neither is a review submitted with no body: a reviewer that clicked
/// "request changes" and wrote its reasons on the diff is carried by the
/// review comments themselves, which are the other half of this — the
/// conversation tab is only ever part of what was said.
fn feedback(pr: &PullRequest, review_comments: &[ReviewComment]) -> Vec<Feedback> {
    let comments = pr.comments.iter().map(|c| Feedback {
        id: c.id.clone(),
        author: login(&c.author),
        body: c.body.clone(),
        file: None,
        blocking: false,
    });
    let inline = review_comments.iter().map(ReviewComment::as_feedback);
    let reviews = pr
        .reviews
        .iter()
        .filter(|r| !r.state.eq_ignore_ascii_case("APPROVED"))
        .map(|r| Feedback {
            id: r.id.clone(),
            author: login(&r.author),
            body: r.body.clone(),
            file: None,
            blocking: r.state.eq_ignore_ascii_case("CHANGES_REQUESTED"),
        });
    comments.chain(inline).chain(reviews).collect()
}

fn login(author: &Option<Author>) -> String {
    forge::author_or_someone(author.as_ref().map(|a| a.login.as_str()))
}

/// The `owner/repo` a pull request URL names, for the API paths that want it
/// spelled out.
pub fn repo_slug(url: &str) -> Option<String> {
    let path = url.trim().trim_end_matches('/');
    let (before, _) = path.rsplit_once("/pull/")?;
    let (_, slug) = before.rsplit_once("://")?;
    let mut segments = slug.split('/').skip(1);
    let owner = segments.next().filter(|s| !s.is_empty())?;
    let repo = segments.next().filter(|s| !s.is_empty())?;
    Some(format!("{owner}/{repo}"))
}

/// Whether `url` is a pull request on github.com — the forge this module
/// watches, of the ones [`crate::forge`] dispatches between.
pub fn is_github_url(url: &str) -> bool {
    forge::forge_of(url) == Some(Forge::GitHub)
}

impl GhCli {
    pub fn new(bin: impl Into<String>) -> Self {
        Self { bin: bin.into() }
    }

    /// One pull request, by the URL it was recorded as.
    ///
    /// Run in the checkout the task belongs to, so `gh` reads the same
    /// credentials and configuration the integrator ran it with by hand.
    pub async fn pr_view(&self, repo: &Path, pr: &WatchedPr) -> Result<PullRequest> {
        let raw = self
            .run(repo, &["pr", "view", &pr.url, "--json", VIEW_FIELDS])
            .await?;
        serde_json::from_str(&raw)
            .with_context(|| format!("reading `gh pr view {}` output: {raw}", pr.url))
    }

    /// `gh` in the checkout, which is how it learns which repository is
    /// meant: it has no `-C` of its own, so the working directory is the
    /// whole of the addressing.
    /// The comments left on the diff, which `gh pr view` does not carry.
    ///
    /// Every page of them: a review thread is where a reviewer says what has
    /// to change, and one comment left unread is a round of feedback the
    /// engineer never gets.
    pub async fn pr_review_comments(
        &self,
        repo: &Path,
        pr: &WatchedPr,
    ) -> Result<Vec<ReviewComment>> {
        let slug = repo_slug(&pr.url)
            .with_context(|| format!("{} names no owner and repository", pr.url))?;
        // A hundred at a time, which is the most GitHub gives: fewer pages
        // for `--paginate` to walk on a pull request people have really been
        // through.
        let path = format!("repos/{slug}/pulls/{}/comments?per_page=100", pr.number);
        let raw = self.run(repo, &["api", "--paginate", &path]).await?;
        parse_pages(&raw).with_context(|| format!("reading `gh api {path}` output: {raw}"))
    }

    async fn run(&self, repo: &Path, args: &[&str]) -> Result<String> {
        let output = Command::new(&self.bin)
            .current_dir(repo)
            .args(args)
            .output()
            .await
            .with_context(|| format!("running {}", self.bin))?;
        if !output.status.success() {
            bail!(
                "gh {} failed in {}: {}",
                args.join(" "),
                repo.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `gh pr view --json <VIEW_FIELDS>` on an open pull request nobody has
    /// touched, verbatim in shape: green, mergeable, and with a head commit
    /// everything about the branch is keyed by.
    fn open_pr() -> serde_json::Value {
        serde_json::json!({
            "number": 12,
            "state": "OPEN",
            "mergedAt": null,
            "mergeCommit": null,
            "reviewDecision": "REVIEW_REQUIRED",
            "reviews": [],
            "comments": [],
            "statusCheckRollup": [],
            "mergeable": "MERGEABLE",
            "mergeStateStatus": "CLEAN",
            "baseRefName": "main",
            "headRefOid": "abc123",
        })
    }

    /// One check run, in the shape the rollup carries an Actions job in.
    fn check_run(name: &str, status: &str, conclusion: serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "__typename": "CheckRun",
            "name": name,
            "status": status,
            "conclusion": conclusion,
            "startedAt": "2026-08-24T09:55:00Z",
            "completedAt": "2026-08-24T09:58:12Z",
            "detailsUrl": format!("https://github.com/owner/repo/actions/runs/17/job/{name}"),
            "workflowName": "CI",
        })
    }

    /// And one commit status, which is the rollup's other shape entirely: a
    /// `context` where a check run has a name, a `state` where it has a
    /// conclusion.
    fn commit_status(context: &str, state: &str) -> serde_json::Value {
        serde_json::json!({
            "__typename": "StatusContext",
            "context": context,
            "state": state,
            "description": "the build",
            "targetUrl": "https://ci.example/build/7",
            "createdAt": "2026-08-24T09:58:12Z",
        })
    }

    /// The health of a pull request whose rollup is `rollup`.
    fn health(rollup: serde_json::Value) -> crate::forge::Health {
        let mut pr = open_pr();
        pr["statusCheckRollup"] = rollup;
        parse(pr).health()
    }

    fn parse(value: serde_json::Value) -> PullRequest {
        serde_json::from_value(value).expect("gh json")
    }

    #[test]
    fn an_untouched_pull_request_wakes_nobody() {
        assert_eq!(
            poll_state(&parse(open_pr()), &[], &[], false),
            PrState::Quiet
        );
    }

    /// The case a repository with no branch protection is always in: GitHub
    /// answers with no review decision at all, and a pull request nobody has
    /// to approve is the user's to merge from the moment it exists. Read as
    /// "not approved" — as it was — a task published to such a repository was
    /// never announced to anybody.
    #[test]
    fn a_pull_request_nothing_gates_is_the_users_to_merge() {
        for decision in [serde_json::Value::Null, "".into(), "   ".into()] {
            let mut pr = open_pr();
            pr["reviewDecision"] = decision.clone();
            assert_eq!(
                poll_state(&parse(pr), &[], &[], false),
                PrState::Approved,
                "reviewDecision {decision:?}"
            );
        }
    }

    /// And announcing one is only ever worth it while it is open: a pull
    /// request closed unmerged is nobody's to press a button on, however
    /// little stood between it and the merge.
    #[test]
    fn a_closed_pull_request_is_announced_to_nobody() {
        let mut pr = open_pr();
        pr["state"] = "CLOSED".into();
        pr["reviewDecision"] = "APPROVED".into();
        assert_eq!(poll_state(&parse(pr), &[], &[], false), PrState::Quiet);
    }

    #[test]
    fn an_approval_is_read_once_and_then_is_quiet() {
        let mut pr = open_pr();
        pr["reviewDecision"] = "APPROVED".into();
        pr["reviews"] = serde_json::json!([{
            "id": "R1", "author": {"login": "maria"}, "body": "", "state": "APPROVED",
        }]);
        let pr = parse(pr);
        assert_eq!(poll_state(&pr, &[], &[], false), PrState::Approved);
        // Once the user has been told, every further poll says nothing: the
        // approving review is not feedback either.
        assert_eq!(poll_state(&pr, &[], &[], true), PrState::Quiet);
    }

    #[test]
    fn comments_and_change_requests_are_feedback_until_they_are_relayed() {
        let mut pr = open_pr();
        pr["reviewDecision"] = "CHANGES_REQUESTED".into();
        pr["comments"] = serde_json::json!([{
            "id": "C1", "author": {"login": "maria"}, "body": "why a new module?",
        }]);
        pr["reviews"] = serde_json::json!([{
            "id": "R1", "author": {"login": "jon"}, "body": "split this up",
            "state": "CHANGES_REQUESTED",
        }]);
        let pr = parse(pr);
        let PrState::Feedback(feedback) = poll_state(&pr, &[], &[], false) else {
            panic!("the comments are feedback");
        };
        assert_eq!(
            feedback,
            vec![
                Feedback {
                    id: "C1".into(),
                    author: "maria".into(),
                    body: "why a new module?".into(),
                    file: None,
                    blocking: false,
                },
                Feedback {
                    id: "R1".into(),
                    author: "jon".into(),
                    body: "split this up".into(),
                    file: None,
                    blocking: true,
                },
            ]
        );
        // Relayed once is relayed for good, whatever the poll goes on saying.
        assert_eq!(
            poll_state(&pr, &[], &["C1".into(), "R1".into()], false),
            PrState::Quiet
        );
        // And a comment already relayed leaves only what is new.
        let PrState::Feedback(later) = poll_state(&pr, &[], &["C1".into()], false) else {
            panic!("the unrelayed review is still feedback");
        };
        assert_eq!(later.len(), 1);
        assert_eq!(later[0].id, "R1");
    }

    /// An approval that arrives while a comment is still unrelayed: the
    /// comment goes to the engineer first, and the approval is announced on
    /// the poll after that — announcing "ready to merge" over feedback nobody
    /// has acted on would be the wrong half of the truth.
    #[test]
    fn feedback_is_read_before_an_approval() {
        let mut pr = open_pr();
        pr["reviewDecision"] = "APPROVED".into();
        pr["comments"] = serde_json::json!([{
            "id": "C1", "author": {"login": "maria"}, "body": "one nit, otherwise fine",
        }]);
        let pr = parse(pr);
        assert!(matches!(
            poll_state(&pr, &[], &[], false),
            PrState::Feedback(_)
        ));
        assert_eq!(
            poll_state(&pr, &[], &["C1".into()], false),
            PrState::Approved
        );
    }

    /// A merged pull request is finished whatever else it says: the comments
    /// on it were relayed long ago, and the approval no longer needs pressing.
    #[test]
    fn a_merged_pull_request_is_read_as_merged() {
        let mut pr = open_pr();
        pr["state"] = "MERGED".into();
        pr["mergedAt"] = "2026-08-24T10:00:00Z".into();
        pr["mergeCommit"] = serde_json::json!({"oid": "abc123"});
        pr["reviewDecision"] = "APPROVED".into();
        pr["comments"] = serde_json::json!([{
            "id": "C9", "author": {"login": "maria"}, "body": "merging this",
        }]);
        let parsed = parse(pr);
        assert_eq!(poll_state(&parsed, &[], &[], false), PrState::Merged);
        assert_eq!(
            parsed.merge_commit.as_ref().map(|c| c.oid.as_str()),
            Some("abc123")
        );
    }

    /// A review with nothing written in it is a click, not a change request:
    /// there is nothing to send an engineer back with.
    #[test]
    fn an_empty_review_body_is_not_feedback() {
        let mut pr = open_pr();
        pr["reviews"] = serde_json::json!([{
            "id": "R1", "author": {"login": "jon"}, "body": "   ",
            "state": "CHANGES_REQUESTED",
        }]);
        assert_eq!(poll_state(&parse(pr), &[], &[], false), PrState::Quiet);
    }

    /// Which is exactly the review most reviewers submit: the reasons are on
    /// the diff, in comments `gh pr view` knows nothing about. They are read
    /// from the API beside it, and they are feedback like any other — carrying
    /// the file they hang on, since that is what the engineer needs to find
    /// them.
    #[test]
    fn a_review_that_wrote_its_reasons_on_the_diff_is_feedback() {
        let mut pr = open_pr();
        pr["reviewDecision"] = "CHANGES_REQUESTED".into();
        pr["reviews"] = serde_json::json!([{
            "id": "R1", "author": {"login": "jon"}, "body": "", "state": "CHANGES_REQUESTED",
        }]);
        // `gh api repos/o/r/pulls/12/comments`, in the shape it answers with.
        let inline: Vec<ReviewComment> = serde_json::from_str(
            r#"[{"id":2318,"user":{"login":"jon"},"body":"this allocates per row",
                 "path":"src/board.rs","line":42,"pull_request_review_id":901}]"#,
        )
        .expect("the API's own output");
        let PrState::Feedback(feedback) = poll_state(&parse(pr), &inline, &[], false) else {
            panic!("a comment on the diff is feedback");
        };
        assert_eq!(
            feedback,
            vec![Feedback {
                id: "RC2318".into(),
                author: "jon".into(),
                body: "this allocates per row".into(),
                file: Some("src/board.rs:42".into()),
                blocking: false,
            }]
        );
        assert_eq!(
            poll_state(&parse(open_pr()), &inline, &["RC2318".into()], false),
            PrState::Quiet,
            "and relayed once, like every other comment"
        );
    }

    /// Both shapes `gh api --paginate` can answer with: the single merged
    /// array it writes for an array endpoint today, and the separate page per
    /// request its own help documents. A pull request with more comments than
    /// fit on one page is the whole point of reading them, so neither shape
    /// may lose one.
    #[test]
    fn every_page_of_review_comments_is_read_whichever_way_gh_writes_them() {
        let page = |from: i64| {
            format!(
                r#"[{{"id":{from},"user":{{"login":"jon"}},"body":"first","path":"a.rs"}},
                    {{"id":{},"user":{{"login":"maria"}},"body":"second","path":"b.rs"}}]"#,
                from + 1
            )
        };
        let separate_pages = format!("{}\n{}\n", page(1), page(3));
        let comments: Vec<ReviewComment> = parse_pages(&separate_pages).expect("both pages");
        let ids: Vec<i64> = comments.iter().map(|c| c.id).collect();
        assert_eq!(ids, vec![1, 2, 3, 4]);

        let merged = r#"[{"id":1,"user":{"login":"jon"},"body":"first","path":"a.rs"},
                         {"id":2,"user":{"login":"maria"},"body":"second","path":"b.rs"}]"#;
        let comments: Vec<ReviewComment> = parse_pages(merged).expect("the one array");
        assert_eq!(comments.len(), 2);

        // No comments at all is no comments, not a failure to read them.
        assert_eq!(parse_pages::<ReviewComment>("").unwrap().len(), 0);
        assert_eq!(parse_pages::<ReviewComment>("[]\n[]").unwrap().len(), 0);
        // And a body that is not pages of anything still fails loudly.
        assert!(parse_pages::<ReviewComment>("not json").is_err());
    }

    /// Every page's comments are feedback, and each of them exactly once.
    #[test]
    fn comments_from_a_later_page_are_relayed_like_any_other() {
        let inline: Vec<ReviewComment> = parse_pages(
            r#"[{"id":1,"user":{"login":"jon"},"body":"page one","path":"a.rs"}]
               [{"id":2,"user":{"login":"maria"},"body":"page two","path":"b.rs"}]"#,
        )
        .expect("two pages");
        let PrState::Feedback(feedback) = poll_state(&parse(open_pr()), &inline, &[], false) else {
            panic!("comments on any page are feedback");
        };
        assert_eq!(
            feedback.iter().map(|f| f.id.as_str()).collect::<Vec<_>>(),
            vec!["RC1", "RC2"]
        );
        assert_eq!(
            poll_state(
                &parse(open_pr()),
                &inline,
                &["RC1".into(), "RC2".into()],
                false
            ),
            PrState::Quiet,
            "and neither page comes round again"
        );
    }

    /// Every shape the rollup answers in, read the one way: red is a check
    /// that finished red, and everything else — running, queued, cancelled,
    /// skipped, a conclusion this has never heard of — is not.
    #[test]
    fn the_check_rollup_is_read_as_red_only_where_github_says_red() {
        // Green, including the two conclusions GitHub's own merge button
        // counts as green.
        for green in [
            serde_json::json!([check_run("build", "COMPLETED", "SUCCESS".into())]),
            serde_json::json!([check_run("lint", "COMPLETED", "SKIPPED".into())]),
            serde_json::json!([check_run("audit", "COMPLETED", "NEUTRAL".into())]),
            serde_json::json!([commit_status("ci/woodpecker", "SUCCESS")]),
        ] {
            assert!(
                health(green.clone()).failed_checks.is_empty(),
                "{green} was read as failing"
            );
        }

        // Pending, in each of the ways a check can be: neither passing nor
        // failing, and nobody is woken for it.
        for pending in [
            serde_json::json!([check_run("build", "IN_PROGRESS", serde_json::Value::Null)]),
            serde_json::json!([check_run("build", "QUEUED", serde_json::Value::Null)]),
            // A queued rerun of a job that failed last time still carries the
            // old conclusion; what says it has not finished is the status.
            serde_json::json!([check_run("build", "IN_PROGRESS", "FAILURE".into())]),
            serde_json::json!([commit_status("ci/woodpecker", "PENDING")]),
            serde_json::json!([commit_status("ci/woodpecker", "EXPECTED")]),
        ] {
            assert!(
                health(pending.clone()).failed_checks.is_empty(),
                "{pending} was read as failing"
            );
        }

        // Cancelled is what a workflow that supersedes its own runs writes on
        // every push, and a conclusion this does not know is unknown: neither
        // is a branch anybody has to be sent back to.
        for unknown in [
            serde_json::json!([check_run("build", "COMPLETED", "CANCELLED".into())]),
            serde_json::json!([check_run("build", "COMPLETED", "SOMETHING_NEW".into())]),
            serde_json::json!([check_run("build", "COMPLETED", serde_json::Value::Null)]),
            // And a repository with no checks at all: absent, not failing.
            serde_json::json!([]),
        ] {
            assert!(
                health(unknown.clone()).failed_checks.is_empty(),
                "{unknown} was read as failing"
            );
        }
        assert!(
            parse(serde_json::json!({"state": "OPEN"}))
                .health()
                .failed_checks
                .is_empty(),
            "a `gh` that answered none of the fields answers no failure"
        );

        // Red, in both shapes, carrying what the engineer needs to find it.
        let failed = health(serde_json::json!([
            check_run("build", "COMPLETED", "SUCCESS".into()),
            check_run("test", "COMPLETED", "FAILURE".into()),
            check_run("e2e", "COMPLETED", "TIMED_OUT".into()),
            commit_status("ci/woodpecker", "ERROR"),
        ]))
        .failed_checks;
        assert_eq!(
            failed,
            vec![
                FailedCheck {
                    id: "CHKabc123:test".into(),
                    name: "test".into(),
                    conclusion: "FAILURE".into(),
                    url: Some("https://github.com/owner/repo/actions/runs/17/job/test".into()),
                },
                FailedCheck {
                    id: "CHKabc123:e2e".into(),
                    name: "e2e".into(),
                    conclusion: "TIMED_OUT".into(),
                    url: Some("https://github.com/owner/repo/actions/runs/17/job/e2e".into()),
                },
                FailedCheck {
                    id: "CHKabc123:ci/woodpecker".into(),
                    name: "ci/woodpecker".into(),
                    conclusion: "ERROR".into(),
                    url: Some("https://ci.example/build/7".into()),
                },
            ]
        );
    }

    /// Mergeability, the same way: `CONFLICTING` is a conflict and so is the
    /// `DIRTY` GitHub spells the same thing with, and everything else —
    /// including the `UNKNOWN` it answers while it is still working the merge
    /// out — is not.
    #[test]
    fn a_conflicting_pull_request_is_the_only_one_read_as_conflicting() {
        let mergeability = |mergeable: serde_json::Value, state: serde_json::Value| {
            let mut pr = open_pr();
            pr["mergeable"] = mergeable;
            pr["mergeStateStatus"] = state;
            parse(pr).health().conflict
        };
        assert_eq!(
            mergeability("CONFLICTING".into(), "DIRTY".into()),
            Some(Conflict {
                id: "MRGabc123".into(),
                base: Some("main".into()),
            })
        );
        assert!(
            mergeability("UNKNOWN".into(), "DIRTY".into()).is_some(),
            "a merge state GitHub calls dirty is a conflict whatever it says beside it"
        );
        for clean in [
            (serde_json::json!("MERGEABLE"), serde_json::json!("CLEAN")),
            // Behind, blocked and unstable are everything else that can stand
            // in the way of a merge, and none of them is a conflict.
            (serde_json::json!("MERGEABLE"), serde_json::json!("BEHIND")),
            (serde_json::json!("MERGEABLE"), serde_json::json!("BLOCKED")),
            (
                serde_json::json!("MERGEABLE"),
                serde_json::json!("UNSTABLE"),
            ),
            // And the merge GitHub has not worked out yet, on a background
            // job of its own: unknown is not conflicting.
            (serde_json::json!("UNKNOWN"), serde_json::json!("UNKNOWN")),
            (serde_json::Value::Null, serde_json::Value::Null),
        ] {
            assert_eq!(
                mergeability(clean.0.clone(), clean.1.clone()),
                None,
                "{clean:?} was read as a conflict"
            );
        }
    }

    /// What a poll makes of them: a conflict before the checks it is likely to
    /// have caused, each of them relayed once, and the failure on the revision
    /// that was supposed to fix it relayed again.
    #[test]
    fn a_red_or_conflicting_branch_is_relayed_once_per_commit() {
        let mut red = open_pr();
        red["reviewDecision"] = "APPROVED".into();
        red["statusCheckRollup"] =
            serde_json::json!([check_run("test", "COMPLETED", "FAILURE".into())]);
        let failed = vec![FailedCheck {
            id: "CHKabc123:test".into(),
            name: "test".into(),
            conclusion: "FAILURE".into(),
            url: Some("https://github.com/owner/repo/actions/runs/17/job/test".into()),
        }];
        assert_eq!(
            poll_state(&parse(red.clone()), &[], &[], false),
            PrState::ChecksFailed(failed.clone())
        );
        // Handed over once, the same failure says nothing more — and says
        // nothing about the approval either: a red branch is not ready to
        // merge however long ago the engineer was told about it.
        assert_eq!(
            poll_state(&parse(red.clone()), &[], &["CHKabc123:test".into()], false),
            PrState::Quiet
        );

        // The engineer pushed a fix, and it failed too: a new commit is a new
        // failure, and the engineer hears about that one as well.
        let mut again = red.clone();
        again["headRefOid"] = "def456".into();
        let PrState::ChecksFailed(new) =
            poll_state(&parse(again), &[], &["CHKabc123:test".into()], false)
        else {
            panic!("a failure on the revision that answered one is news again");
        };
        assert_eq!(new[0].id, "CHKdef456:test");

        // And a conflict is read before the checks, being what the failing
        // pipeline of a merge that no longer applies is failing for.
        let mut conflicting = red;
        conflicting["mergeable"] = "CONFLICTING".into();
        assert_eq!(
            poll_state(&parse(conflicting.clone()), &[], &[], false),
            PrState::Conflicting(Conflict {
                id: "MRGabc123".into(),
                base: Some("main".into()),
            })
        );
        assert_eq!(
            poll_state(&parse(conflicting), &[], &["MRGabc123".into()], false),
            PrState::ChecksFailed(failed),
            "the conflict was handed over; the failing check has not been"
        );
    }

    /// And the whole reason any of it is read: a pull request everybody
    /// approved is not the user's to merge while it is red or conflicting.
    #[test]
    fn an_approval_over_a_red_branch_is_not_announced() {
        let mut approved = open_pr();
        approved["reviewDecision"] = "APPROVED".into();
        assert_eq!(
            poll_state(&parse(approved.clone()), &[], &[], false),
            PrState::Approved,
            "green, mergeable and approved is the one that is announced"
        );

        let mut red = approved.clone();
        red["statusCheckRollup"] =
            serde_json::json!([check_run("test", "COMPLETED", "FAILURE".into())]);
        assert!(matches!(
            poll_state(&parse(red.clone()), &[], &[], false),
            PrState::ChecksFailed(_)
        ));
        assert_eq!(
            poll_state(&parse(red), &[], &["CHKabc123:test".into()], false),
            PrState::Quiet,
            "an approval announced over a failing check is the wrong half of the truth"
        );

        let mut conflicting = approved;
        conflicting["mergeable"] = "CONFLICTING".into();
        assert_eq!(
            poll_state(&parse(conflicting), &[], &["MRGabc123".into()], false),
            PrState::Quiet
        );
    }

    #[test]
    fn the_repository_a_pull_request_url_names() {
        assert_eq!(
            repo_slug("https://github.com/owner/repo/pull/12"),
            Some("owner/repo".to_string())
        );
        assert_eq!(
            repo_slug("https://github.com/owner/repo/pull/12/files"),
            Some("owner/repo".to_string()),
            "whatever the URL goes on to point at inside the pull request"
        );
        assert_eq!(repo_slug("https://github.com/owner/repo"), None);
    }

    /// `gh pr view 4 --json number,state,mergedAt,mergeCommit,reviewDecision,reviews,comments`
    /// on this repository, verbatim (gh 2.97): the fields the structs above
    /// are read from, in the shape GitHub really answers with — a review
    /// decision that is empty rather than absent when nothing gates the pull
    /// request, ids that are strings, and the merge commit under `oid`.
    #[test]
    fn a_real_gh_answer_parses_into_what_a_poll_reads() {
        let raw = r#"{"comments":[{"id":"IC_kwDOUAbv3M8AAAABQQFUBw","author":{"login":"github-actions"},"authorAssociation":"CONTRIBUTOR","body":"Created releases: v0.3.0","createdAt":"2026-08-23T10:40:36Z","includesCreatedEdit":false,"isMinimized":false,"minimizedReason":"","reactionGroups":[],"url":"https://github.com/owner/repo/pull/4#issuecomment-5385573383","viewerDidAuthor":false}],"mergeCommit":{"oid":"5c81311ec832970ab02c6cbb3946c17df29b3dd5"},"mergedAt":"2026-08-23T10:40:28Z","number":4,"reviewDecision":"","reviews":[],"state":"MERGED"}"#;
        let pr: PullRequest = serde_json::from_str(raw).expect("gh's own output");
        assert_eq!(pr.state, "MERGED");
        assert_eq!(
            pr.merge_commit.as_ref().map(|c| c.oid.as_str()),
            Some("5c81311ec832970ab02c6cbb3946c17df29b3dd5")
        );
        assert_eq!(pr.review_decision.as_deref(), Some(""));
        assert!(
            !pr.health().blocks_merge(),
            "the fields this answer predates are absent, which is nothing failing"
        );
        // Merged wins over the comment on it: what a comment is relayed for
        // is a revision, and there is nothing left to revise.
        assert_eq!(poll_state(&pr, &[], &[], false), PrState::Merged);
    }

    #[test]
    fn only_a_github_pull_request_url_is_one_this_watches() {
        assert!(is_github_url("https://github.com/owner/repo/pull/12"));
        assert!(is_github_url("HTTPS://GitHub.com/owner/repo/pull/12"));
        assert!(!is_github_url(
            "https://gitlab.com/owner/repo/-/merge_requests/3"
        ));
        assert!(!is_github_url(
            "https://github.example.com/owner/repo/pull/12"
        ));
        assert!(!is_github_url("not a url at all"));

        assert_eq!(
            pull_request_number("https://github.com/owner/repo/pull/12"),
            Some(12)
        );
        assert_eq!(
            pull_request_number("https://github.com/owner/repo/pull/12/"),
            Some(12)
        );
        assert_eq!(pull_request_number("https://github.com/owner/repo"), None);
        assert_eq!(
            pull_request_number("https://github.com/owner/repo/pull/twelve"),
            None
        );
    }
}
