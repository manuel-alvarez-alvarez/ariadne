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

use crate::forge::{self, Forge, Landing};

pub use crate::forge::{
    Feedback, GITHUB_HOST, PrState, WatchedPr, parse_pages, pull_request_number,
};

/// The `gh pr view --json` fields a poll asks for.
const VIEW_FIELDS: &str = "number,state,mergedAt,mergeCommit,reviewDecision,reviews,comments";

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

    /// Whether the reviewers approved it, as `reviewDecision` reports —
    /// absent or empty on a repository that gates nothing.
    pub fn is_approved(&self) -> bool {
        self.review_decision
            .as_deref()
            .is_some_and(|d| d.eq_ignore_ascii_case("APPROVED"))
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
        pr.is_approved(),
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

    /// `gh pr view --json number,state,mergedAt,mergeCommit,reviewDecision,reviews,comments`
    /// on an open pull request nobody has touched, verbatim in shape.
    fn open_pr() -> serde_json::Value {
        serde_json::json!({
            "number": 12,
            "state": "OPEN",
            "mergedAt": null,
            "mergeCommit": null,
            "reviewDecision": "REVIEW_REQUIRED",
            "reviews": [],
            "comments": [],
        })
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
