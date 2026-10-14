//! Replying to a comment on a pull request, and resolving a review thread
//! a reviewer session opened once a push fixed it, through `gh` (026, 029).
//! Where the repository holds posts back (002, 026 rule 24), a reply is
//! held in the viewer's own pending review instead (`reply_pending`):
//! GitHub allows exactly one pending review per user per request, so it is
//! found or created once and reused.
use serde::Deserialize;

use super::Github;
use super::details::{issue_comment_id, review_comment_id, review_id};
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

/// What `submit_review` (029) and `resolve_thread` (029 rule 16b) write at
/// the top of the viewer's pending review body where the repository holds
/// posts back (026 rule 24): the ending of the verdict and summary
/// `set_pending_review_header` writes, so a later round replaces only the
/// text above it, leaving any reply text `reply_pending` appended below it
/// in place.
const PENDING_VERDICT_MARK: &str = "<!-- ariadne:pending-review-verdict -->";

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

    /// The viewer's pending review on `number` — GitHub holds at most one
    /// per user per request — found among the request's reviews, or
    /// created with no event, and the hidden verdict mark as its body,
    /// where there is none: creating one is itself silent, since a pending
    /// review shows nobody but its own author until it is submitted.
    pub(super) async fn pending_review(
        &self,
        repo: &str,
        number: i64,
        login: &str,
    ) -> Result<ReviewSummary, String> {
        if login.is_empty() {
            // With no login to match an existing pending review against,
            // every reply would create another: GitHub refuses a second
            // pending review of the same user with a 422, rather than
            // reusing the first the way a real login would.
            return Err("no login to find or create a pending review under".into());
        }
        let (host, owner, name) = split_slug(repo)?;
        let path = format!("repos/{owner}/{name}/pulls/{number}/reviews");
        let reviews: Vec<ReviewSummary> = self.pages(host, &path).await?;
        if let Some(found) = reviews.into_iter().find(|r| {
            r.state.eq_ignore_ascii_case("pending") && r.user.login.eq_ignore_ascii_case(login)
        }) {
            if !found.body.trim().is_empty() {
                return Ok(found);
            }
            // A pending review with no body at all — one an earlier Ariadne
            // created with `body=`, or one begun by hand on the forge — is
            // one GitHub refuses to edit through either API ("Could not
            // edit a review with a missing body"), and every pending post
            // is an edit of it. Holding no draft comment, it is deleted and
            // made again with a body, losing nothing; holding any, it is
            // the user's to submit or discard first.
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
        }
        // Created with a body that is the hidden verdict mark alone, which
        // renders as nothing: GitHub refuses to edit a review it holds with
        // no body at all ("Could not edit a review with a missing body"),
        // and every later change of the pending review — a verdict, a
        // reply, a resolve — is an edit of its body. No `event` field still
        // leaves the review pending.
        let output = self
            .cli
            .answer(&[
                "api",
                &path,
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

    /// Merge `header` into `pending`'s own body, replacing only the text up
    /// to [`PENDING_VERDICT_MARK`] and keeping any reply text
    /// `reply_pending` appended below it in place, then overwrite the body
    /// outright. Answers the review's own id.
    pub(super) async fn apply_pending_header(
        &self,
        host: &str,
        pending: &ReviewSummary,
        header: &str,
    ) -> Result<String, String> {
        let rest = match pending.body.find(PENDING_VERDICT_MARK) {
            Some(at) => pending.body[at + PENDING_VERDICT_MARK.len()..]
                .trim_start_matches('\n')
                .trim()
                .to_string(),
            None => pending.body.trim().to_string(),
        };
        let mut body = format!("{header}\n{PENDING_VERDICT_MARK}");
        if !rest.is_empty() {
            body.push_str("\n\n");
            body.push_str(&rest);
        }
        self.update_pending_review_body(host, pending, &body).await
    }

    /// Resolve the thread `comment` is in, held back (029 rule 16b, where
    /// the repository holds posts back): a line naming it, under a
    /// "Threads to resolve" heading of the viewer's pending review body,
    /// created once rather than on every call. Creates the pending review
    /// first where there is none. Answers the review's own id.
    pub(crate) async fn resolve_pending(
        &self,
        repo: &str,
        number: i64,
        login: &str,
        comment: &PullRequestComment,
    ) -> Result<String, String> {
        if comment.kind != "review_comment" || comment.thread_id == CONVERSATION {
            return Err("only a review thread on a line resolves".into());
        }
        let (host, owner, name) = split_slug(repo)?;
        let pending = self.pending_review(repo, number, login).await?;
        let line = format!(
            "- {}",
            review_thread_url(host, owner, name, number, comment)
        );
        let body = with_section_line(&pending.body, "Threads to resolve", &line);
        self.update_pending_review_body(host, &pending, &body).await
    }
}

/// The permalink of a review thread, anchored at its first comment, the
/// way GitHub's own UI links to one.
fn review_thread_url(
    host: &str,
    owner: &str,
    name: &str,
    number: i64,
    comment: &PullRequestComment,
) -> String {
    let id = comment.forge_id.trim_start_matches("rc-");
    format!("https://{host}/{owner}/{name}/pull/{number}#discussion_r{id}")
}

/// `line` under a markdown `## {heading}` section of `body`: an existing
/// heading takes the line at its own section's end, so a second call never
/// writes the heading twice, and a reply `reply_pending` appends after it
/// (a blank line away, the same way every append here separates one piece
/// from the next) never pushes an earlier line out from under its
/// heading. With no heading yet, it and the line are appended at the
/// body's end instead.
fn with_section_line(body: &str, heading: &str, line: &str) -> String {
    let marker = format!("## {heading}");
    let mut blocks: Vec<String> = body.split("\n\n").map(str::to_string).collect();
    if let Some(section) = blocks.iter().position(|block| block.starts_with(&marker)) {
        let block = &mut blocks[section];
        if !block.ends_with('\n') {
            block.push('\n');
        }
        block.push_str(line);
        block.push('\n');
        return blocks.join("\n\n");
    }
    let section = format!("{marker}\n{line}\n");
    match body.trim().is_empty() {
        true => section,
        false => format!("{}\n\n{section}", body.trim_end()),
    }
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

    use super::{Github, ReviewAuthor, ReviewSummary, conversation_comment_url};
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

    fn pending(body: &str) -> ReviewSummary {
        ReviewSummary {
            id: 9,
            node_id: "PR_kwDOx".into(),
            state: "PENDING".into(),
            body: body.into(),
            user: ReviewAuthor { login: "me".into() },
        }
    }

    /// `submit_review` (029), with the repository holding posts back, sets
    /// the verdict and summary at the top of the viewer's pending review
    /// body.
    #[tokio::test]
    async fn apply_pending_header_sets_the_top_with_no_existing_body() {
        let dir = tempfile::tempdir().unwrap();
        let github = github(&fake_gh(
            dir.path(),
            &[r#"{"data":{"updatePullRequestReview":{"pullRequestReview":{"id":"PR_kwDOx"}}}}"#],
        ));
        let id = github
            .apply_pending_header(
                "github.com",
                &pending(""),
                "**Verdict: Request changes**\n\nOne P0 open.",
            )
            .await
            .unwrap();
        assert_eq!(id, "rv-9");
        let mutate = call_args(dir.path(), 0);
        let body = mutate
            .iter()
            .find_map(|a| a.strip_prefix("body="))
            .expect("a body argument");
        assert_eq!(
            body,
            "**Verdict: Request changes**\n\nOne P0 open.\n\
             <!-- ariadne:pending-review-verdict -->"
        );
    }

    /// A later round replaces only the header above
    /// `PENDING_VERDICT_MARK`, leaving the reply text `reply_pending`
    /// appended below it in place.
    #[tokio::test]
    async fn apply_pending_header_replaces_its_top_and_keeps_the_reply_text_below() {
        let dir = tempfile::tempdir().unwrap();
        let existing = "**Verdict: Comment**\n\nFirst look.\n\n\
             <!-- ariadne:pending-review-verdict -->\n\n\
             > Why not here?\n\n[alice](https://github.com/acme/widgets/pull/1#issuecomment-42)";
        let github = github(&fake_gh(
            dir.path(),
            &[r#"{"data":{"updatePullRequestReview":{"pullRequestReview":{"id":"PR_kwDOx"}}}}"#],
        ));
        github
            .apply_pending_header(
                "github.com",
                &pending(existing),
                "**Verdict: Request changes**\n\nOne P0 open now.",
            )
            .await
            .unwrap();
        let mutate = call_args(dir.path(), 0);
        let body = mutate
            .iter()
            .find_map(|a| a.strip_prefix("body="))
            .expect("a body argument");
        assert_eq!(
            body,
            "**Verdict: Request changes**\n\nOne P0 open now.\n\
             <!-- ariadne:pending-review-verdict -->\n\n\
             > Why not here?\n\n[alice](https://github.com/acme/widgets/pull/1#issuecomment-42)"
        );
    }

    /// `resolve_thread` (029 rule 16b), with the repository holding posts
    /// back, lists the thread under a "Threads to resolve" heading of the
    /// pending review body rather than resolving it, creating the heading
    /// once.
    #[tokio::test]
    async fn resolve_pending_lists_the_thread_under_a_heading_created_once() {
        let dir = tempfile::tempdir().unwrap();
        let github = github(&fake_gh(
            dir.path(),
            &[
                r###"[{"id":9,"node_id":"PR_kwDOx","state":"PENDING",
                    "body":"## Threads to resolve\n- https://github.com/acme/widgets/pull/1#discussion_r101",
                    "user":{"login":"me"}}]"###,
                r#"{"data":{"updatePullRequestReview":{"pullRequestReview":{"id":"PR_kwDOx"}}}}"#,
            ],
        ));
        let comment = PullRequestComment {
            forge_id: "rc-202".into(),
            ..review_comment("T2")
        };
        let id = github
            .resolve_pending("github.com/acme/widgets", 1, "me", &comment)
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
            "## Threads to resolve\n\
             - https://github.com/acme/widgets/pull/1#discussion_r101\n\
             - https://github.com/acme/widgets/pull/1#discussion_r202\n"
        );
    }

    /// A reply `reply_pending` appended after the heading (per
    /// `Github::append_to_pending_review`, a blank line away) does not push
    /// an earlier thread's line out from under it: a later resolve still
    /// lands its own line in the heading's own section, ahead of the reply.
    #[tokio::test]
    async fn resolve_pending_keeps_its_section_whole_around_a_reply_appended_after_it() {
        let dir = tempfile::tempdir().unwrap();
        let body_after_a_reply = "## Threads to resolve\n\
             - https://github.com/acme/widgets/pull/1#discussion_r101\n\n\
             > Why not here?\n\n\
             [alice](https://github.com/acme/widgets/pull/1#issuecomment-42)\n\n\
             Fixed it.";
        let github = github(&fake_gh(
            dir.path(),
            &[
                &format!(
                    r#"[{{"id":9,"node_id":"PR_kwDOx","state":"PENDING","body":{},"user":{{"login":"me"}}}}]"#,
                    serde_json::to_string(body_after_a_reply).unwrap()
                ),
                r#"{"data":{"updatePullRequestReview":{"pullRequestReview":{"id":"PR_kwDOx"}}}}"#,
            ],
        ));
        let comment = PullRequestComment {
            forge_id: "rc-202".into(),
            ..review_comment("T2")
        };
        github
            .resolve_pending("github.com/acme/widgets", 1, "me", &comment)
            .await
            .unwrap();
        let mutate = call_args(dir.path(), 1);
        let body = mutate
            .iter()
            .find_map(|a| a.strip_prefix("body="))
            .expect("a body argument");
        assert_eq!(
            body,
            "## Threads to resolve\n\
             - https://github.com/acme/widgets/pull/1#discussion_r101\n\
             - https://github.com/acme/widgets/pull/1#discussion_r202\n\n\n\
             > Why not here?\n\n\
             [alice](https://github.com/acme/widgets/pull/1#issuecomment-42)\n\n\
             Fixed it."
        );
    }

    /// A reply to a top-level conversation comment, and the conversation's
    /// own thread, have no review thread of their own: only a comment on a
    /// line resolves.
    #[tokio::test]
    async fn resolve_pending_refuses_the_conversation() {
        let dir = tempfile::tempdir().unwrap();
        let github = github(&fake_gh(dir.path(), &[]));
        let comment = PullRequestComment {
            kind: "issue_comment".into(),
            ..review_comment("conversation")
        };
        assert!(
            github
                .resolve_pending("github.com/acme/widgets", 1, "me", &comment)
                .await
                .is_err()
        );
    }
}
