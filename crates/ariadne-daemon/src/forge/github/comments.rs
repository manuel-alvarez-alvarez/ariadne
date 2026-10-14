//! Replying to a comment on a pull request, and resolving a review thread
//! a reviewer session opened once a push fixed it, through `gh` (026, 029).
//! Where the repository holds posts back (002, 026 rule 24), a reply is
//! held in the viewer's own pending review instead (`reply_pending`):
//! GitHub allows exactly one pending review per user per request, so it is
//! found or created once and reused.
use serde::Deserialize;

use super::Github;
use super::details::{issue_comment_id, review_comment_id, review_id};
use crate::forge::PendingEditError;
use crate::forge::pulls::{CONVERSATION, split_slug};
use ariadne_store::PullRequestComment;

#[derive(Deserialize)]
struct Created {
    id: i64,
}

#[derive(Deserialize)]
struct ReviewAuthor {
    login: String,
}

/// A review of a request, as `gh api .../pulls/<n>/reviews` lists it: a
/// submitted one, or the viewer's own pending one.
#[derive(Deserialize)]
pub(super) struct ReviewSummary {
    pub(super) id: i64,
    #[serde(default)]
    pub(super) node_id: String,
    #[serde(default)]
    state: String,
    #[serde(default)]
    pub(super) body: String,
    user: ReviewAuthor,
}

/// A marker on an Ariadne-created pending review. It gives GitHub a body to
/// edit when a user later changes that pending review through the draft tools.
const PENDING_VERDICT_MARK: &str = "<!-- ariadne:pending-review-verdict -->";

/// The viewer's own pending review's comments, by their GraphQL node id:
/// what [`Github::pending_review_comment_node`] looks a numeric database
/// id up among. `gh api graphql --paginate` walks `$endCursor` through
/// every page, so a pending review past its first hundred comments is
/// still found whole.
const PENDING_REVIEW_COMMENTS: &str = "query($id: ID!, $endCursor: String) { \
     node(id: $id) { ... on PullRequestReview { comments(first: 100, after: $endCursor) { \
     pageInfo { hasNextPage endCursor } nodes { id databaseId } } } } }";

impl Github {
    /// Reply to `comment` with `body`, and answer the forge id the reply is
    /// stored under.
    ///
    /// A review comment takes the reply in its own thread, `gh api
    /// .../pulls/comments/<id>/replies`, on the comment that opened it: the
    /// forge takes no reply to a reply. Anything else is the conversation,
    /// and the reply is a comment on it, `gh pr comment`.
    pub(crate) async fn reply(
        &self,
        repo: &str,
        number: i64,
        comment: &PullRequestComment,
        body: &str,
    ) -> Result<String, String> {
        if comment.kind == "review_comment" {
            let (host, owner, name) = split_slug(repo)?;
            let root = comment
                .in_reply_to
                .as_deref()
                .unwrap_or(&comment.forge_id)
                .trim_start_matches("rc-");
            let output = self
                .cli
                .answer(&[
                    "api",
                    &format!("repos/{owner}/{name}/pulls/{number}/comments/{root}/replies"),
                    "--hostname",
                    host,
                    "--method",
                    "POST",
                    "-f",
                    &format!("body={body}"),
                ])
                .await?;
            let created: Created = serde_json::from_str(&output)
                .map_err(|e| format!("cannot read the reply GitHub stored: {e}"))?;
            return Ok(review_comment_id(created.id));
        }
        let url = self
            .cli
            .answer(&[
                "pr",
                "comment",
                &number.to_string(),
                "--repo",
                repo,
                "--body",
                body,
            ])
            .await?;
        // `gh pr comment` answers the comment's URL, which ends in its id.
        Ok(url
            .rsplit_once("#issuecomment-")
            .and_then(|(_, id)| id.trim().parse::<i64>().ok())
            .map(issue_comment_id)
            .unwrap_or(url))
    }

    /// Resolve the review thread `comment` is in, `resolveReviewThread`.
    ///
    /// The thread is GraphQL's node id, which the detail fetch reads; a
    /// comment posted since then still has its own id for one, so the
    /// request's threads are read for it first. The conversation is no
    /// thread, and nothing resolves it.
    pub(crate) async fn resolve(
        &self,
        repo: &str,
        number: i64,
        comment: &PullRequestComment,
    ) -> Result<(), String> {
        if comment.kind != "review_comment" || comment.thread_id == CONVERSATION {
            return Err("only a review thread on a line resolves".into());
        }
        let (host, owner, name) = split_slug(repo)?;
        let thread = self
            .thread_node_id(host, owner, name, number, comment)
            .await?;
        self.cli
            .answer(&[
                "api",
                "graphql",
                "--hostname",
                host,
                "-f",
                "query=mutation($id: ID!) { resolveReviewThread(input: {threadId: $id}) { thread { isResolved } } }",
                "-f",
                &format!("id={thread}"),
            ])
            .await?;
        Ok(())
    }

    /// `comment`'s review thread, as GraphQL's node id: the detail fetch's
    /// own read of it, else, for a comment posted since, a read of the
    /// request's threads for the comment's own numeric id.
    async fn thread_node_id(
        &self,
        host: &str,
        owner: &str,
        name: &str,
        number: i64,
        comment: &PullRequestComment,
    ) -> Result<String, String> {
        match comment.thread_id.strip_prefix("rc-") {
            Some(own) => {
                let id: i64 = own
                    .parse()
                    .map_err(|_| format!("{} names no review comment", comment.thread_id))?;
                self.threads(host, owner, name, number)
                    .await?
                    .remove(&id)
                    .map(|(thread, _)| thread)
                    .ok_or_else(|| format!("GitHub holds no thread of comment {id}"))
            }
            None => Ok(comment.thread_id.clone()),
        }
    }

    /// Reply to `comment` with `body`, held in the viewer's own pending
    /// review rather than posted, while the repository holds posts back
    /// (per 026 rule 24). A reply to an inline review comment joins the
    /// pending review's own thread, `addPullRequestReviewThreadReply`,
    /// answering the new draft comment's id. A reply to a top-level
    /// conversation comment has no pending form of its own, so the pending
    /// review's body gains it instead, led by a quote of the comment it
    /// answers and a link to it, answering the pending review's own id.
    pub(crate) async fn reply_pending(
        &self,
        repo: &str,
        number: i64,
        login: &str,
        comment: &PullRequestComment,
        body: &str,
    ) -> Result<String, String> {
        let (host, owner, name) = split_slug(repo)?;
        if comment.kind == "review_comment" {
            let pending = self.pending_review(repo, number, login).await?;
            let thread = self
                .thread_node_id(host, owner, name, number, comment)
                .await?;
            let output = self
                .cli
                .answer(&[
                    "api",
                    "graphql",
                    "--hostname",
                    host,
                    "-f",
                    "query=mutation($reviewId: ID!, $threadId: ID!, $body: String!) { \
                     addPullRequestReviewThreadReply(input: {pullRequestReviewId: $reviewId, \
                     pullRequestReviewThreadId: $threadId, body: $body}) { comment { databaseId } } }",
                    "-f",
                    &format!("reviewId={}", pending.node_id),
                    "-f",
                    &format!("threadId={thread}"),
                    "-f",
                    &format!("body={body}"),
                ])
                .await?;
            let answered: serde_json::Value = serde_json::from_str(&output)
                .map_err(|e| format!("cannot read the pending reply GitHub stored: {e}"))?;
            let id = answered["data"]["addPullRequestReviewThreadReply"]["comment"]["databaseId"]
                .as_i64()
                .ok_or_else(|| "GitHub answered no id for the pending reply".to_string())?;
            return Ok(review_comment_id(id));
        }
        let url = conversation_comment_url(host, owner, name, number, comment);
        let quote = comment
            .body
            .lines()
            .map(|line| format!("> {line}"))
            .collect::<Vec<_>>()
            .join("\n");
        let text = format!("{quote}\n\n[{}]({url})\n\n{body}", comment.author_login);
        self.append_to_pending_review(repo, number, login, &text)
            .await
    }

    /// The viewer's pending review on `number`, found among the request's
    /// reviews — GitHub holds at most one per user per request — or `None`
    /// where there is none: unlike [`Self::pending_review`], this creates
    /// nothing, so a caller that only means to find one, never to start
    /// one, reads an empty answer as "not pending" (026 rule 25) rather than
    /// bringing a review into existence just to say so.
    pub(super) async fn find_pending_review(
        &self,
        repo: &str,
        number: i64,
        login: &str,
    ) -> Result<Option<ReviewSummary>, String> {
        if login.is_empty() {
            return Err("no login to find a pending review under".into());
        }
        let (host, owner, name) = split_slug(repo)?;
        let path = format!("repos/{owner}/{name}/pulls/{number}/reviews");
        let reviews: Vec<ReviewSummary> = self.pages(host, &path).await?;
        Ok(reviews.into_iter().find(|r| {
            r.state.eq_ignore_ascii_case("pending") && r.user.login.eq_ignore_ascii_case(login)
        }))
    }

    /// The current body of the viewer's own pending review, read fresh,
    /// with its own forge id, so a caller whose `reply_pending` just folded
    /// text into it can bring the live cache's own copy of that id current
    /// (026 rule 25), without changing what it answers or how it holds.
    pub(crate) async fn pending_review_body(
        &self,
        repo: &str,
        number: i64,
        login: &str,
    ) -> Result<Option<(String, String)>, String> {
        Ok(self
            .find_pending_review(repo, number, login)
            .await?
            .map(|pending| (review_id(pending.id), pending.body)))
    }

    /// The viewer's pending review on `number` — GitHub holds at most one
    /// per user per request — found among the request's reviews, or
    /// created with no event, and the hidden verdict mark as its body,
    /// where there is none: creating one is itself silent, since a pending
    /// review shows nobody but its own author until it is submitted. One
    /// found with no body at all is recovered the same way
    /// [`Self::replace_pending_review_body`]'s own find recovers it.
    pub(super) async fn pending_review(
        &self,
        repo: &str,
        number: i64,
        login: &str,
    ) -> Result<ReviewSummary, String> {
        let (host, owner, name) = split_slug(repo)?;
        let path = format!("repos/{owner}/{name}/pulls/{number}/reviews");
        match self.find_pending_review(repo, number, login).await? {
            Some(found) => {
                self.recover_bodiless_review(host, &path, number, found)
                    .await
            }
            None => self.create_pending_review(host, &path).await,
        }
    }

    /// Recover `found`'s empty body (026 rule 25): one GitHub refuses to
    /// edit through either API ("Could not edit a review with a missing
    /// body") — an earlier Ariadne's own, made with `body=`, or one begun
    /// by hand on the forge — is deleted and made again with the hidden
    /// mark where it holds no draft comment of its own, losing nothing;
    /// holding any makes it the user's to submit or discard first, named
    /// in the error rather than touched. `found` itself, unchanged, where
    /// its body already holds something.
    async fn recover_bodiless_review(
        &self,
        host: &str,
        path: &str,
        number: i64,
        found: ReviewSummary,
    ) -> Result<ReviewSummary, String> {
        if !found.body.trim().is_empty() {
            return Ok(found);
        }
        let held: Vec<serde_json::Value> = self
            .pages(host, &format!("{path}/{}/comments", found.id))
            .await?;
        if !held.is_empty() {
            return Err(format!(
                "GitHub holds a pending review of yours on #{number} with no body, which \
                 it refuses to edit, and {} draft comment(s) in it: submit or discard that \
                 review on the forge, and the next post makes one Ariadne can edit",
                held.len()
            ));
        }
        self.cli
            .answer(&[
                "api",
                &format!("{path}/{}", found.id),
                "--hostname",
                host,
                "--method",
                "DELETE",
            ])
            .await?;
        self.create_pending_review(host, path).await
    }

    /// Create the viewer's pending review fresh, the hidden verdict mark
    /// alone as its body, which renders as nothing: GitHub refuses to
    /// edit a review it holds with no body at all ("Could not edit a
    /// review with a missing body"), and every later change of the
    /// pending review — a verdict, a reply, a resolve — is an edit of its
    /// body. No `event` field still leaves the review pending.
    async fn create_pending_review(&self, host: &str, path: &str) -> Result<ReviewSummary, String> {
        let output = self
            .cli
            .answer(&[
                "api",
                path,
                "--hostname",
                host,
                "--method",
                "POST",
                "-f",
                &format!("body={PENDING_VERDICT_MARK}"),
            ])
            .await?;
        serde_json::from_str(&output)
            .map_err(|e| format!("cannot read the pending review GitHub stored: {e}"))
    }

    /// Append `text` to the viewer's pending review body, creating the
    /// review first where there is none. Answers the review's own id.
    pub(super) async fn append_to_pending_review(
        &self,
        repo: &str,
        number: i64,
        login: &str,
        text: &str,
    ) -> Result<String, String> {
        let (host, _, _) = split_slug(repo)?;
        let pending = self.pending_review(repo, number, login).await?;
        let body = match pending.body.trim().is_empty() {
            true => text.to_string(),
            false => format!("{}\n\n{text}", pending.body),
        };
        self.update_pending_review_body(host, &pending, &body).await
    }

    /// Overwrite the viewer's pending review body outright, through
    /// `updatePullRequestReview`. Answers the review's own id.
    async fn update_pending_review_body(
        &self,
        host: &str,
        pending: &ReviewSummary,
        body: &str,
    ) -> Result<String, String> {
        self.cli
            .answer(&[
                "api",
                "graphql",
                "--hostname",
                host,
                "-f",
                "query=mutation($id: ID!, $body: String!) { \
                 updatePullRequestReview(input: {pullRequestReviewId: $id, body: $body}) { \
                 pullRequestReview { id } } }",
                "-f",
                &format!("id={}", pending.node_id),
                "-f",
                &format!("body={body}"),
            ])
            .await?;
        Ok(review_id(pending.id))
    }

    /// Edit the body of one of the viewer's own still-pending review
    /// comments, `updatePullRequestReviewComment` (026 rule 25). Refused with
    /// [`PendingEditError::NotPending`] where `forge_id` names no comment
    /// of the viewer's own pending review: the user already submitted it,
    /// or there never was one.
    pub(crate) async fn edit_pending_comment(
        &self,
        repo: &str,
        number: i64,
        login: &str,
        forge_id: &str,
        body: &str,
    ) -> Result<String, PendingEditError> {
        let (host, _, _) = split_slug(repo)?;
        let id = pending_review_comment_numeric_id(forge_id)?;
        let pending = self
            .find_pending_review(repo, number, login)
            .await?
            .ok_or(PendingEditError::NotPending)?;
        let node = self.pending_review_comment_node(host, &pending, id).await?;
        self.cli
            .answer(&[
                "api",
                "graphql",
                "--hostname",
                host,
                "-f",
                "query=mutation($id: ID!, $body: String!) { \
                 updatePullRequestReviewComment(input: {pullRequestReviewCommentId: $id, body: \
                 $body}) { pullRequestReviewComment { databaseId } } }",
                "-f",
                &format!("id={node}"),
                "-f",
                &format!("body={body}"),
            ])
            .await?;
        Ok(forge_id.to_string())
    }

    /// Delete one of the viewer's own still-pending review comments,
    /// `deletePullRequestReviewComment` (026 rule 25). Refused the same way
    /// [`Self::edit_pending_comment`] is.
    pub(crate) async fn delete_pending_comment(
        &self,
        repo: &str,
        number: i64,
        login: &str,
        forge_id: &str,
    ) -> Result<(), PendingEditError> {
        let (host, _, _) = split_slug(repo)?;
        let id = pending_review_comment_numeric_id(forge_id)?;
        let pending = self
            .find_pending_review(repo, number, login)
            .await?
            .ok_or(PendingEditError::NotPending)?;
        let node = self.pending_review_comment_node(host, &pending, id).await?;
        self.cli
            .answer(&[
                "api",
                "graphql",
                "--hostname",
                host,
                "-f",
                "query=mutation($id: ID!) { deletePullRequestReviewComment(input: \
                 {pullRequestReviewCommentId: $id}) { pullRequestReview { id } } }",
                "-f",
                &format!("id={node}"),
            ])
            .await?;
        Ok(())
    }

    /// Replace the viewer's pending review body outright (026 rule 25).
    /// Refused with [`PendingEditError::NotPending`] where there is none,
    /// rather than creating one just to hold the edit.
    pub(crate) async fn replace_pending_review_body(
        &self,
        repo: &str,
        number: i64,
        login: &str,
        body: &str,
    ) -> Result<String, PendingEditError> {
        let (host, owner, name) = split_slug(repo)?;
        let path = format!("repos/{owner}/{name}/pulls/{number}/reviews");
        let found = self
            .find_pending_review(repo, number, login)
            .await?
            .ok_or(PendingEditError::NotPending)?;
        // Recovered the same way `pending_review` recovers it (026 rule
        // 25): an older review found with no body at all is still one
        // GitHub refuses to edit, whatever this call's own new body is,
        // since the refusal reads the review's current state, not the
        // mutation's.
        let pending = self
            .recover_bodiless_review(host, &path, number, found)
            .await?;
        Ok(self
            .update_pending_review_body(host, &pending, body)
            .await?)
    }

    /// The GraphQL node id of comment `id` of `pending`'s own comments, read
    /// fresh so a comment the user already submitted away is told apart
    /// from one still in the viewer's pending review (026 rule 25): the mutations
    /// `edit_pending_comment` and `delete_pending_comment` take a node id,
    /// never the database id `addPullRequestReviewThread` itself answers.
    async fn pending_review_comment_node(
        &self,
        host: &str,
        pending: &ReviewSummary,
        id: i64,
    ) -> Result<String, PendingEditError> {
        let output = self
            .cli
            .answer(&[
                "api",
                "graphql",
                "--hostname",
                host,
                "--paginate",
                "-f",
                &format!("query={PENDING_REVIEW_COMMENTS}"),
                "-f",
                &format!("id={}", pending.node_id),
            ])
            .await?;
        for page in serde_json::Deserializer::from_str(&output).into_iter::<serde_json::Value>() {
            let page = page.map_err(|e| {
                PendingEditError::Forge(format!(
                    "cannot read the pending review's own comments GitHub stored: {e}"
                ))
            })?;
            if let Some(node_id) = page["data"]["node"]["comments"]["nodes"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|node| node["databaseId"].as_i64() == Some(id))
                .and_then(|node| node["id"].as_str())
            {
                return Ok(node_id.to_string());
            }
        }
        Err(PendingEditError::NotPending)
    }
}

/// `forge_id` as the numeric id of a review comment, `rc-<n>`: what
/// [`Github::edit_pending_comment`] and [`Github::delete_pending_comment`]
/// look up among the viewer's own pending review's comments.
fn pending_review_comment_numeric_id(forge_id: &str) -> Result<i64, PendingEditError> {
    forge_id
        .strip_prefix("rc-")
        .and_then(|id| id.parse().ok())
        .ok_or_else(|| PendingEditError::BadForgeId(format!("{forge_id} names no review comment")))
}

/// The URL of a top-level conversation comment: a plain comment on the
/// request, or a review's own body, which the forge anchors by its numeric
/// id the same way its own UI links to either.
fn conversation_comment_url(
    host: &str,
    owner: &str,
    name: &str,
    number: i64,
    comment: &PullRequestComment,
) -> String {
    let anchor = match comment.forge_id.strip_prefix("ic-") {
        Some(id) => format!("#issuecomment-{id}"),
        None => match comment.forge_id.strip_prefix("rv-") {
            Some(id) => format!("#pullrequestreview-{id}"),
            None => String::new(),
        },
    };
    format!("https://{host}/{owner}/{name}/pull/{number}{anchor}")
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::time::Duration;

    use super::{Github, conversation_comment_url};
    use crate::forge::Cli;
    use ariadne_store::PullRequestComment;

    /// A `gh` stub that answers, in order, one scripted `stdout` per call,
    /// and keeps each call's own arguments beside it, numbered, so a test
    /// reads exactly what each step sent.
    const STUB: &str = r#"#!/usr/bin/env python3
import json, os, sys
here = os.path.dirname(os.path.abspath(sys.argv[0]))
count_file = os.path.join(here, "count")
count = int(open(count_file).read()) if os.path.exists(count_file) else 0
open(count_file, "w").write(str(count + 1))
open(os.path.join(here, f"call-{count}.json"), "w").write(json.dumps(sys.argv[1:]))
answers = json.load(open(os.path.join(here, "answers.json")))
sys.stdout.write(answers[count])
"#;

    fn fake_gh(dir: &std::path::Path, answers: &[&str]) -> String {
        let path = dir.join("gh");
        std::fs::write(&path, STUB).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(
            dir.join("answers.json"),
            serde_json::to_string(answers).unwrap(),
        )
        .unwrap();
        path.display().to_string()
    }

    /// The arguments call `n` (0-indexed) was run with.
    fn call_args(dir: &std::path::Path, n: usize) -> Vec<String> {
        serde_json::from_str(&std::fs::read_to_string(dir.join(format!("call-{n}.json"))).unwrap())
            .unwrap()
    }

    fn github(binary: &str) -> Github {
        Github::new(Cli {
            timeout: Duration::from_secs(5),
            ..Cli::new(Some(binary), "gh")
        })
    }

    fn review_comment(thread_id: &str) -> PullRequestComment {
        PullRequestComment {
            id: "rc-101".into(),
            pull_request_id: "p".into(),
            forge_id: "rc-101".into(),
            thread_id: thread_id.into(),
            kind: "review_comment".into(),
            author_login: "alice".into(),
            author_is_bot: false,
            body: "What about this case?".into(),
            path: Some("src/lib.rs".into()),
            line: Some(3),
            in_reply_to: None,
            created_at: "2026-10-01T00:00:00Z".into(),
            answered: false,
            resolved: false,
            told_at: None,
            from_review: false,
        }
    }

    /// A reply to an inline review comment, with no pending review yet,
    /// creates one — its body the hidden verdict mark alone, so GitHub lets
    /// every later edit of the body through — and joins its thread through
    /// `addPullRequestReviewThreadReply`, answering the new draft comment's
    /// own id.
    #[tokio::test]
    async fn a_pending_review_comment_reply_creates_a_pending_review_and_joins_its_thread() {
        let dir = tempfile::tempdir().unwrap();
        let github = github(&fake_gh(
            dir.path(),
            &[
                "[]",
                r#"{"id":9,"node_id":"PR_kwDOx","state":"PENDING","body":"<!-- ariadne:pending-review-verdict -->","user":{"login":"me"}}"#,
                r#"{"data":{"addPullRequestReviewThreadReply":{"comment":{"databaseId":501}}}}"#,
            ],
        ));
        let id = github
            .reply_pending(
                "github.com/acme/widgets",
                1,
                "me",
                &review_comment("T1"),
                "Fixed.",
            )
            .await
            .unwrap();
        assert_eq!(id, "rc-501");
        let create = call_args(dir.path(), 1);
        assert!(create.contains(&"--method".to_string()) && create.contains(&"POST".to_string()));
        assert!(
            create.contains(&format!("body={}", super::PENDING_VERDICT_MARK)),
            "the review is created with the hidden mark as its body: {create:?}"
        );
        let mutate = call_args(dir.path(), 2);
        assert!(
            mutate.contains(&"reviewId=PR_kwDOx".to_string()),
            "{mutate:?}"
        );
        assert!(mutate.contains(&"threadId=T1".to_string()), "{mutate:?}");
        assert!(mutate.contains(&"body=Fixed.".to_string()), "{mutate:?}");
    }

    /// GitHub allows exactly one pending review per user per request: a
    /// second reply finds the one the first already created and joins its
    /// thread again, with no second creation call.
    #[tokio::test]
    async fn a_second_pending_reply_reuses_the_same_pending_review() {
        let dir = tempfile::tempdir().unwrap();
        let github = github(&fake_gh(
            dir.path(),
            &[
                "[]",
                r#"{"id":9,"node_id":"PR_kwDOx","state":"PENDING","body":"<!-- ariadne:pending-review-verdict -->","user":{"login":"me"}}"#,
                r#"{"data":{"addPullRequestReviewThreadReply":{"comment":{"databaseId":501}}}}"#,
                r#"[{"id":9,"node_id":"PR_kwDOx","state":"PENDING","body":"<!-- ariadne:pending-review-verdict -->","user":{"login":"me"}}]"#,
                r#"{"data":{"addPullRequestReviewThreadReply":{"comment":{"databaseId":502}}}}"#,
            ],
        ));
        let first = github
            .reply_pending(
                "github.com/acme/widgets",
                1,
                "me",
                &review_comment("T1"),
                "Fixed.",
            )
            .await
            .unwrap();
        assert_eq!(first, "rc-501");
        let second = github
            .reply_pending(
                "github.com/acme/widgets",
                1,
                "me",
                &review_comment("T2"),
                "Also fixed.",
            )
            .await
            .unwrap();
        assert_eq!(second, "rc-502");
        // Exactly one creation call (`--method POST` on the listing path)
        // across both replies: the second reply's own GET (call 3) finds
        // the review the first already created, rather than making another.
        let creations = (0..5)
            .map(|n| call_args(dir.path(), n))
            .filter(|args| {
                args.contains(&"--method".to_string()) && args.contains(&"POST".to_string())
            })
            .count();
        assert_eq!(creations, 1, "a second reply never creates a second review");
        let second_mutate = call_args(dir.path(), 4);
        assert!(
            second_mutate.contains(&"reviewId=PR_kwDOx".to_string()),
            "{second_mutate:?}"
        );
    }

    /// A pending review found with no body at all — one an earlier Ariadne
    /// created with `body=` — is one GitHub refuses to edit. Holding no
    /// draft comment, it is deleted and made again with the hidden mark as
    /// its body, and the reply joins the new one.
    #[tokio::test]
    async fn a_bodiless_pending_review_holding_no_draft_is_deleted_and_made_again() {
        let dir = tempfile::tempdir().unwrap();
        let github = github(&fake_gh(
            dir.path(),
            &[
                r#"[{"id":9,"node_id":"PR_kwDOx","state":"PENDING","body":"","user":{"login":"me"}}]"#,
                "[]",
                "",
                r#"{"id":10,"node_id":"PR_kwDOy","state":"PENDING","body":"<!-- ariadne:pending-review-verdict -->","user":{"login":"me"}}"#,
                r#"{"data":{"addPullRequestReviewThreadReply":{"comment":{"databaseId":501}}}}"#,
            ],
        ));
        let id = github
            .reply_pending(
                "github.com/acme/widgets",
                1,
                "me",
                &review_comment("T1"),
                "Fixed.",
            )
            .await
            .unwrap();
        assert_eq!(id, "rc-501");
        let drafts = call_args(dir.path(), 1);
        assert!(
            drafts
                .iter()
                .any(|a| a == "repos/acme/widgets/pulls/1/reviews/9/comments"),
            "the bodiless review's draft comments are read first: {drafts:?}"
        );
        let delete = call_args(dir.path(), 2);
        assert!(
            delete
                .iter()
                .any(|a| a == "repos/acme/widgets/pulls/1/reviews/9")
                && delete.contains(&"DELETE".to_string()),
            "{delete:?}"
        );
        let create = call_args(dir.path(), 3);
        assert!(
            create.contains(&"POST".to_string())
                && create.contains(&format!("body={}", super::PENDING_VERDICT_MARK)),
            "{create:?}"
        );
        let mutate = call_args(dir.path(), 4);
        assert!(
            mutate.contains(&"reviewId=PR_kwDOy".to_string()),
            "the reply joins the new review: {mutate:?}"
        );
    }

    /// A bodiless pending review that holds draft comments is the user's
    /// to submit or discard: the reply is refused, saying so, and nothing
    /// of theirs is deleted.
    #[tokio::test]
    async fn a_bodiless_pending_review_holding_drafts_is_the_users_to_settle() {
        let dir = tempfile::tempdir().unwrap();
        let github = github(&fake_gh(
            dir.path(),
            &[
                r#"[{"id":9,"node_id":"PR_kwDOx","state":"PENDING","body":"","user":{"login":"me"}}]"#,
                r#"[{"id":301,"body":"Theirs.","path":"src/lib.rs","line":3}]"#,
            ],
        ));
        let refused = github
            .reply_pending(
                "github.com/acme/widgets",
                1,
                "me",
                &review_comment("T1"),
                "Fixed.",
            )
            .await
            .unwrap_err();
        assert!(
            refused.contains("submit or discard") && refused.contains("1 draft comment"),
            "{refused}"
        );
        assert!(
            !dir.path().join("call-2.json").exists(),
            "nothing is deleted or created behind the user's drafts"
        );
    }

    /// A reply to a top-level conversation comment has no pending form of
    /// its own, so it is appended to the viewer's existing pending review
    /// body, led by a quote of the comment it answers and a link to it,
    /// answering the pending review's own id, reused rather than created
    /// again.
    #[tokio::test]
    async fn a_pending_conversation_reply_quotes_the_comment_and_appends_to_the_pending_review() {
        let dir = tempfile::tempdir().unwrap();
        let github = github(&fake_gh(
            dir.path(),
            &[
                r#"[{"id":9,"node_id":"PR_kwDOx","state":"PENDING","body":"Earlier note.","user":{"login":"me"}}]"#,
                r#"{"data":{"updatePullRequestReview":{"pullRequestReview":{"id":"PR_kwDOx"}}}}"#,
            ],
        ));
        let comment = PullRequestComment {
            id: "ic-42".into(),
            pull_request_id: "p".into(),
            forge_id: "ic-42".into(),
            thread_id: "conversation".into(),
            kind: "issue_comment".into(),
            author_login: "alice".into(),
            author_is_bot: false,
            body: "Why not here?".into(),
            path: None,
            line: None,
            in_reply_to: None,
            created_at: "2026-10-01T00:00:00Z".into(),
            answered: false,
            resolved: false,
            told_at: None,
            from_review: false,
        };
        let id = github
            .reply_pending("github.com/acme/widgets", 1, "me", &comment, "Fixed it.")
            .await
            .unwrap();
        assert_eq!(id, "rv-9");
        let mutate = call_args(dir.path(), 1);
        let body = mutate
            .iter()
            .find_map(|a| a.strip_prefix("body="))
            .expect("a body argument");
        assert_eq!(
            body,
            "Earlier note.\n\n> Why not here?\n\n\
             [alice](https://github.com/acme/widgets/pull/1#issuecomment-42)\n\n\
             Fixed it."
        );
    }

    /// A conversation reply, like a resolve, folds straight into the
    /// pending review's own body: `pending_review_body`, read right after,
    /// answers the body the reply left, quote and all, not the one the
    /// review held before it — what a caller echoes into the live cache so
    /// `list_comments` never answers stale (026 rule 25).
    #[tokio::test]
    async fn pending_review_body_reads_what_a_conversation_reply_just_folded_into_it() {
        let dir = tempfile::tempdir().unwrap();
        let github = github(&fake_gh(
            dir.path(),
            &[
                r#"[{"id":9,"node_id":"PR_kwDOx","state":"PENDING","body":"Earlier note.","user":{"login":"me"}}]"#,
                r#"{"data":{"updatePullRequestReview":{"pullRequestReview":{"id":"PR_kwDOx"}}}}"#,
                r#"[{"id":9,"node_id":"PR_kwDOx","state":"PENDING",
                    "body":"Earlier note.\n\n> Why not here?\n\nFixed it.","user":{"login":"me"}}]"#,
            ],
        ));
        let comment = PullRequestComment {
            id: "ic-42".into(),
            pull_request_id: "p".into(),
            forge_id: "ic-42".into(),
            thread_id: "conversation".into(),
            kind: "issue_comment".into(),
            author_login: "alice".into(),
            author_is_bot: false,
            body: "Why not here?".into(),
            path: None,
            line: None,
            in_reply_to: None,
            created_at: "2026-10-01T00:00:00Z".into(),
            answered: false,
            resolved: false,
            told_at: None,
            from_review: false,
        };
        let id = github
            .reply_pending("github.com/acme/widgets", 1, "me", &comment, "Fixed it.")
            .await
            .unwrap();
        let (fresh_id, body) = github
            .pending_review_body("github.com/acme/widgets", 1, "me")
            .await
            .unwrap()
            .expect("the pending review is still there");
        assert_eq!(fresh_id, id);
        assert!(
            body.contains("Fixed it."),
            "the reply's own text, not the note the review held before it: {body:?}"
        );
    }

    /// The conversation URL anchors a plain comment by its issue-comment id
    /// and a review's own body by its review id, the way GitHub's own UI
    /// links to either.
    #[test]
    fn conversation_comment_url_anchors_by_the_comments_own_kind() {
        let issue = PullRequestComment {
            forge_id: "ic-7".into(),
            ..review_comment("conversation")
        };
        assert_eq!(
            conversation_comment_url("github.com", "acme", "widgets", 1, &issue),
            "https://github.com/acme/widgets/pull/1#issuecomment-7"
        );
        let review = PullRequestComment {
            forge_id: "rv-7".into(),
            ..review_comment("conversation")
        };
        assert_eq!(
            conversation_comment_url("github.com", "acme", "widgets", 1, &review),
            "https://github.com/acme/widgets/pull/1#pullrequestreview-7"
        );
    }

    /// `edit_pending_comment` (026 rule 25) looks up the viewer's own still-pending
    /// review, reads the comment's node id among that review's own
    /// comments, and edits it by node id, `updatePullRequestReviewComment`
    /// — never the database id `addPullRequestReviewThread` answers.
    #[tokio::test]
    async fn edit_pending_comment_edits_a_comment_still_in_the_viewers_pending_review() {
        let dir = tempfile::tempdir().unwrap();
        let github = github(&fake_gh(
            dir.path(),
            &[
                r#"[{"id":9,"node_id":"PR_kwDOx","state":"PENDING","body":"","user":{"login":"me"}}]"#,
                r#"{"data":{"node":{"comments":{"nodes":[
                    {"id":"PRRC_1","databaseId":501},{"id":"PRRC_2","databaseId":502}]}}}}"#,
                r#"{"data":{"updatePullRequestReviewComment":{"pullRequestReviewComment":{"databaseId":501}}}}"#,
            ],
        ));
        let id = github
            .edit_pending_comment("github.com/acme/widgets", 1, "me", "rc-501", "Fixed.")
            .await
            .unwrap();
        assert_eq!(id, "rc-501");
        let mutate = call_args(dir.path(), 2);
        assert!(mutate.contains(&"id=PRRC_1".to_string()), "{mutate:?}");
        assert!(mutate.contains(&"body=Fixed.".to_string()), "{mutate:?}");
    }

    /// With no pending review of the viewer's own at all, `edit_pending_comment`
    /// answers [`PendingEditError::NotPending`] off the one listing call,
    /// making no mutation: there is nothing pending to edit.
    #[tokio::test]
    async fn edit_pending_comment_refuses_with_no_pending_review() {
        let dir = tempfile::tempdir().unwrap();
        let github = github(&fake_gh(dir.path(), &["[]"]));
        let error = github
            .edit_pending_comment("github.com/acme/widgets", 1, "me", "rc-501", "Fixed.")
            .await
            .unwrap_err();
        assert!(
            matches!(error, super::PendingEditError::NotPending),
            "{error:?}"
        );
    }

    /// A `forge_id` of the wrong shape — an `ic-` conversation id, say,
    /// handed to `edit_pending_comment` rather than `rc-` — is the
    /// caller's own mistake, not the forge's: refused with
    /// [`PendingEditError::BadForgeId`], never reaching the forge at all.
    #[tokio::test]
    async fn edit_pending_comment_refuses_a_forge_id_of_the_wrong_shape() {
        let dir = tempfile::tempdir().unwrap();
        let github = github(&fake_gh(dir.path(), &[]));
        let error = github
            .edit_pending_comment("github.com/acme/widgets", 1, "me", "ic-501", "Fixed.")
            .await
            .unwrap_err();
        assert!(
            matches!(error, super::PendingEditError::BadForgeId(_)),
            "{error:?}"
        );
    }

    /// A comment no longer among the viewer's pending review's own comments
    /// — the round it was in has since been submitted — is refused the
    /// same way, after the node lookup finds it missing.
    #[tokio::test]
    async fn edit_pending_comment_refuses_a_comment_the_pending_review_no_longer_holds() {
        let dir = tempfile::tempdir().unwrap();
        let github = github(&fake_gh(
            dir.path(),
            &[
                r#"[{"id":9,"node_id":"PR_kwDOx","state":"PENDING","body":"","user":{"login":"me"}}]"#,
                r#"{"data":{"node":{"comments":{"nodes":[{"id":"PRRC_2","databaseId":502}]}}}}"#,
            ],
        ));
        let error = github
            .edit_pending_comment("github.com/acme/widgets", 1, "me", "rc-501", "Fixed.")
            .await
            .unwrap_err();
        assert!(
            matches!(error, super::PendingEditError::NotPending),
            "{error:?}"
        );
    }

    /// A pending review with more than a hundred comments still has its
    /// hundred-and-first found: `--paginate` walks `$endCursor` through
    /// every page of `PENDING_REVIEW_COMMENTS`, the same way `threads`
    /// walks every page of a request's review threads.
    #[tokio::test]
    async fn edit_pending_comment_finds_a_comment_past_the_first_hundred() {
        let dir = tempfile::tempdir().unwrap();
        let first_page = r#"{"data":{"node":{"comments":{"pageInfo":{"hasNextPage":true,"endCursor":"c1"},"nodes":[{"id":"PRRC_1","databaseId":501}]}}}}"#;
        let second_page = r#"{"data":{"node":{"comments":{"pageInfo":{"hasNextPage":false,"endCursor":null},"nodes":[{"id":"PRRC_2","databaseId":502}]}}}}"#;
        let github = github(&fake_gh(
            dir.path(),
            &[
                r#"[{"id":9,"node_id":"PR_kwDOx","state":"PENDING","body":"","user":{"login":"me"}}]"#,
                &format!("{first_page}{second_page}"),
                r#"{"data":{"updatePullRequestReviewComment":{"pullRequestReviewComment":{"databaseId":502}}}}"#,
            ],
        ));
        let id = github
            .edit_pending_comment("github.com/acme/widgets", 1, "me", "rc-502", "Fixed.")
            .await
            .unwrap();
        assert_eq!(id, "rc-502");
        let mutate = call_args(dir.path(), 2);
        assert!(mutate.contains(&"id=PRRC_2".to_string()), "{mutate:?}");
    }

    /// `delete_pending_comment` (026 rule 25) deletes a comment still in the
    /// viewer's pending review by its node id, `deletePullRequestReviewComment`.
    #[tokio::test]
    async fn delete_pending_comment_deletes_a_comment_still_in_the_viewers_pending_review() {
        let dir = tempfile::tempdir().unwrap();
        let github = github(&fake_gh(
            dir.path(),
            &[
                r#"[{"id":9,"node_id":"PR_kwDOx","state":"PENDING","body":"","user":{"login":"me"}}]"#,
                r#"{"data":{"node":{"comments":{"nodes":[{"id":"PRRC_1","databaseId":501}]}}}}"#,
                r#"{"data":{"deletePullRequestReviewComment":{"pullRequestReview":{"id":"PR_kwDOx"}}}}"#,
            ],
        ));
        github
            .delete_pending_comment("github.com/acme/widgets", 1, "me", "rc-501")
            .await
            .unwrap();
        let mutate = call_args(dir.path(), 2);
        assert!(mutate.contains(&"id=PRRC_1".to_string()), "{mutate:?}");
        assert!(
            mutate
                .iter()
                .any(|a| a.contains("deletePullRequestReviewComment")),
            "{mutate:?}"
        );
    }

    /// `replace_pending_review_body` (026 rule 25) overwrites the viewer's
    /// pending review body outright: whatever text the body held is gone.
    #[tokio::test]
    async fn replace_pending_review_body_replaces_the_whole_body_outright() {
        let dir = tempfile::tempdir().unwrap();
        let github = github(&fake_gh(
            dir.path(),
            &[
                r#"[{"id":9,"node_id":"PR_kwDOx","state":"PENDING","body":"Old note.","user":{"login":"me"}}]"#,
                r#"{"data":{"updatePullRequestReview":{"pullRequestReview":{"id":"PR_kwDOx"}}}}"#,
            ],
        ));
        let id = github
            .replace_pending_review_body("github.com/acme/widgets", 1, "me", "New body.")
            .await
            .unwrap();
        assert_eq!(id, "rv-9");
        let mutate = call_args(dir.path(), 1);
        let body = mutate
            .iter()
            .find_map(|a| a.strip_prefix("body="))
            .expect("a body argument");
        assert_eq!(body, "New body.");
    }

    /// A review `pending_review` creates fresh, body the hidden verdict
    /// mark alone, is still a review `replace_pending_review_body` can edit
    /// right after: GitHub refuses a `updatePullRequestReview` mutation
    /// against a review it holds with a missing body, and the mark is
    /// never that.
    #[tokio::test]
    async fn a_freshly_created_pending_review_is_editable_right_after() {
        let dir = tempfile::tempdir().unwrap();
        let github = github(&fake_gh(
            dir.path(),
            &[
                "[]",
                &format!(
                    r#"{{"id":9,"node_id":"PR_kwDOx","state":"PENDING","body":"{}","user":{{"login":"me"}}}}"#,
                    super::PENDING_VERDICT_MARK
                ),
                r#"[{"id":9,"node_id":"PR_kwDOx","state":"PENDING","body":"<!-- ariadne:pending-review-verdict -->","user":{"login":"me"}}]"#,
                r#"{"data":{"updatePullRequestReview":{"pullRequestReview":{"id":"PR_kwDOx"}}}}"#,
            ],
        ));
        github
            .pending_review("github.com/acme/widgets", 1, "me")
            .await
            .unwrap();
        let create = call_args(dir.path(), 1);
        assert!(
            create.contains(&format!("body={}", super::PENDING_VERDICT_MARK)),
            "the review is created with the hidden mark as its body: {create:?}"
        );
        let id = github
            .replace_pending_review_body("github.com/acme/widgets", 1, "me", "New body.")
            .await
            .unwrap();
        assert_eq!(id, "rv-9");
        let mutate = call_args(dir.path(), 3);
        assert!(mutate.contains(&"body=New body.".to_string()), "{mutate:?}");
    }

    /// An older pending review found with no body at all — one from
    /// before the hidden-mark fix, or begun by hand on the forge — is
    /// recovered the same way `pending_review` recovers it, holding no
    /// draft comment of its own: deleted and made again, then replaced
    /// with the caller's own new body outright, never left for GitHub to
    /// refuse ("Could not edit a review with a missing body").
    #[tokio::test]
    async fn replace_pending_review_body_recovers_an_older_bodiless_review() {
        let dir = tempfile::tempdir().unwrap();
        let github = github(&fake_gh(
            dir.path(),
            &[
                r#"[{"id":9,"node_id":"PR_kwDOx","state":"PENDING","body":"","user":{"login":"me"}}]"#,
                "[]",
                "",
                r#"{"id":10,"node_id":"PR_kwDOy","state":"PENDING","body":"<!-- ariadne:pending-review-verdict -->","user":{"login":"me"}}"#,
                r#"{"data":{"updatePullRequestReview":{"pullRequestReview":{"id":"PR_kwDOy"}}}}"#,
            ],
        ));
        let id = github
            .replace_pending_review_body("github.com/acme/widgets", 1, "me", "New body.")
            .await
            .unwrap();
        assert_eq!(id, "rv-10");
        let drafts = call_args(dir.path(), 1);
        assert!(
            drafts
                .iter()
                .any(|a| a == "repos/acme/widgets/pulls/1/reviews/9/comments"),
            "the bodiless review's draft comments are read first: {drafts:?}"
        );
        let delete = call_args(dir.path(), 2);
        assert!(
            delete
                .iter()
                .any(|a| a == "repos/acme/widgets/pulls/1/reviews/9")
                && delete.contains(&"DELETE".to_string()),
            "{delete:?}"
        );
        let create = call_args(dir.path(), 3);
        assert!(
            create.contains(&"POST".to_string())
                && create.contains(&format!("body={}", super::PENDING_VERDICT_MARK)),
            "{create:?}"
        );
        let mutate = call_args(dir.path(), 4);
        assert!(mutate.contains(&"body=New body.".to_string()), "{mutate:?}");
    }

    /// With no pending review, `replace_pending_review_body` answers
    /// [`PendingEditError::NotPending`] rather than creating one just to
    /// hold the edit.
    #[tokio::test]
    async fn replace_pending_review_body_refuses_with_no_pending_review() {
        let dir = tempfile::tempdir().unwrap();
        let github = github(&fake_gh(dir.path(), &["[]"]));
        let error = github
            .replace_pending_review_body("github.com/acme/widgets", 1, "me", "New body.")
            .await
            .unwrap_err();
        assert!(
            matches!(error, super::PendingEditError::NotPending),
            "{error:?}"
        );
    }
}
