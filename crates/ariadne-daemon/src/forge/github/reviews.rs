//! Posting one review of a pull request, through `gh api` (029). The event
//! is `REQUEST_CHANGES` or `COMMENT`: there is no call here that approves,
//! since the user gives every approval.
use serde::Deserialize;

use super::Github;
use super::details::{issue_comment_id, review_comment_id};
use crate::forge::pulls::{ReviewDraft, split_slug};
use ariadne_store::NewPullRequestComment;

#[derive(Deserialize)]
struct Login {
    login: String,
}

/// One event of a request's timeline: the ones that say whether a review is
/// still asked of a login.
#[derive(Deserialize)]
struct TimelineEvent {
    #[serde(default)]
    event: String,
    requested_reviewer: Option<Login>,
}

#[derive(Deserialize)]
struct Posted {
    id: i64,
}

#[derive(Deserialize)]
struct PostedComment {
    id: i64,
    #[serde(default)]
    body: String,
    path: Option<String>,
    line: Option<i64>,
    created_at: String,
}

/// What a GitHub review that takes no body without one carries: the state
/// of the review is in its summary comment.
const POINTER: &str = "The review's state is in its summary comment.";

#[derive(Deserialize)]
struct Comment {
    id: i64,
    created_at: String,
}

impl Github {
    /// Post `review` as one review of its inline findings, `gh api
    /// repos/<owner>/<name>/pulls/<n>/reviews`, with no body of its own,
    /// then read back the comments it holds, so each is stored under the id
    /// the forge gave it. A forge that refuses a review with no body gets it
    /// again with a line that points at the summary.
    pub(crate) async fn submit_review(
        &self,
        repo: &str,
        number: i64,
        review: &ReviewDraft,
        login: &str,
    ) -> Result<Vec<NewPullRequestComment>, String> {
        let (host, owner, name) = split_slug(repo)?;
        let path = format!("repos/{owner}/{name}/pulls/{number}/reviews");
        let event = match review.request_changes {
            true => "REQUEST_CHANGES",
            false => "COMMENT",
        };
        // One JSON body on standard input: `gh`'s `-f comments[][...]`
        // fields group several comments into the wrong objects, which GitHub
        // refuses with a 422 naming fields of no comment.
        let post = async |body: Option<&str>| {
            let mut review = serde_json::json!({
                "event": event,
                "commit_id": review.head_sha,
                "comments": review.comments.iter().map(|comment| serde_json::json!({
                    "path": comment.path,
                    "line": comment.line,
                    "side": "RIGHT",
                    "body": comment.body,
                })).collect::<Vec<_>>(),
            });
            if let Some(body) = body {
                review["body"] = serde_json::json!(body);
            }
            self.cli
                .call_with_input(
                    &[
                        "api",
                        &path,
                        "--hostname",
                        host,
                        "--method",
                        "POST",
                        "--input",
                        "-",
                    ],
                    Some(&review.to_string()),
                )
                .await
        };
        let output = match post(None).await {
            Err(refusal) if refusal.answer().is_some_and(|a| refused_for_its_body(&a)) => {
                post(Some(POINTER))
                    .await
                    .map_err(|refusal| refusal.to_string())?
            }
            answered => answered.map_err(|refusal| refusal.to_string())?,
        };
        let posted: Posted = serde_json::from_str(&output)
            .map_err(|e| format!("cannot read the review GitHub stored: {e}"))?;
        let mut stored = Vec::new();
        if review.comments.is_empty() {
            return Ok(stored);
        }
        let output = self
            .cli
            .answer(&[
                "api",
                &format!("{path}/{}/comments", posted.id),
                "--hostname",
                host,
            ])
            .await?;
        let comments: Vec<PostedComment> = serde_json::from_str(&output)
            .map_err(|e| format!("cannot read the comments of the review GitHub stored: {e}"))?;
        for comment in comments {
            // The thread is the comment's own until the next detail fetch
            // reads the forge's thread id for it.
            stored.push(NewPullRequestComment {
                forge_id: review_comment_id(comment.id),
                thread_id: review_comment_id(comment.id),
                kind: "review_comment".into(),
                author_login: login.to_string(),
                author_is_bot: false,
                body: comment.body,
                path: comment.path,
                line: comment.line,
                in_reply_to: None,
                created_at: comment.created_at,
                resolved: false,
                from_review: false,
            });
        }
        Ok(stored)
    }

    /// Write the one summary comment a review keeps on request `number`
    /// (029): `existing`, an `ic-<id>` of an earlier round, is edited in
    /// place, `gh api repos/<owner>/<name>/issues/comments/<id> --method
    /// PATCH`; with none, or one GitHub answers 404 for, a comment is posted
    /// on the conversation. Any other failed edit is the error: a second
    /// summary would leave the first behind. Answers its forge id and when
    /// it was written.
    pub(crate) async fn write_summary(
        &self,
        repo: &str,
        number: i64,
        existing: Option<&str>,
        body: &str,
    ) -> Result<(String, String), String> {
        let (host, owner, name) = split_slug(repo)?;
        let field = format!("body={body}");
        if let Some(id) = existing.and_then(|id| id.strip_prefix("ic-")) {
            let edited = self
                .cli
                .call(&[
                    "api",
                    &format!("repos/{owner}/{name}/issues/comments/{id}"),
                    "--hostname",
                    host,
                    "--method",
                    "PATCH",
                    "-f",
                    &field,
                ])
                .await;
            match edited {
                Ok(output) => return read_comment(&output),
                Err(refusal) if refusal.is_missing() => {}
                Err(refusal) => return Err(refusal.to_string()),
            }
        }
        let output = self
            .cli
            .answer(&[
                "api",
                &format!("repos/{owner}/{name}/issues/{number}/comments"),
                "--hostname",
                host,
                "--method",
                "POST",
                "-f",
                &field,
            ])
            .await?;
        read_comment(&output)
    }

    /// Whether request `number` still asks for the review of `login`, read
    /// off its timeline (029). GitHub stops listing a request as asking for a
    /// review once the reviewer reviewed it, and writes no removal event for
    /// that, so the list cannot say it. The last request or removed request
    /// for `login` decides. A review decides nothing: a review posted after a
    /// withdrawal does not ask again.
    pub(crate) async fn review_still_requested(
        &self,
        repo: &str,
        number: i64,
        login: &str,
    ) -> Result<bool, String> {
        let (host, owner, name) = split_slug(repo)?;
        let path = format!("repos/{owner}/{name}/issues/{number}/timeline");
        let output = self
            .cli
            .answer(&["api", &path, "--hostname", host, "--paginate"])
            .await?;
        let mut requested = false;
        for page in serde_json::Deserializer::from_str(&output).into_iter::<Vec<TimelineEvent>>() {
            let page = page.map_err(|e| format!("cannot read `gh api {path}`: {e}"))?;
            for event in page {
                let mine = |who: &Option<Login>| {
                    who.as_ref()
                        .is_some_and(|w| w.login.eq_ignore_ascii_case(login))
                };
                match event.event.as_str() {
                    "review_requested" if mine(&event.requested_reviewer) => requested = true,
                    "review_request_removed" if mine(&event.requested_reviewer) => {
                        requested = false
                    }
                    _ => {}
                }
            }
        }
        Ok(requested)
    }
}

/// The id and the time of a comment GitHub answered with.
fn read_comment(output: &str) -> Result<(String, String), String> {
    let comment: Comment = serde_json::from_str(output)
        .map_err(|e| format!("cannot read the comment GitHub stored: {e}"))?;
    Ok((issue_comment_id(comment.id), comment.created_at))
}

/// Whether GitHub refused a review for having no body: what its own answer
/// says, `{"message": ..., "errors": [...]}`, parsed whatever its layout. An
/// error is a string, or an object with a `message` or a `field`.
fn refused_for_its_body(answer: &serde_json::Value) -> bool {
    let names_body = |text: &str| text.to_lowercase().contains("body");
    let errors = answer["errors"].as_array().cloned().unwrap_or_default();
    errors.iter().any(|error| match error {
        serde_json::Value::String(text) => names_body(text),
        other => ["message", "field", "code"]
            .iter()
            .filter_map(|key| other[key].as_str())
            .any(names_body),
    }) || answer["message"].as_str().is_some_and(names_body)
}

#[cfg(test)]
mod tests {
    use super::refused_for_its_body;

    /// A review is posted again with a body only where GitHub's own answer
    /// says the body is what it lacks, whatever its spacing or key order.
    #[test]
    fn a_review_is_retried_with_a_body_only_when_github_asks_for_one() {
        let parse = |text: &str| serde_json::from_str::<serde_json::Value>(text).unwrap();
        assert!(!refused_for_its_body(&parse(
            r#"{"message":"Validation Failed","errors":["Line could not be resolved"]}"#
        )));
        assert!(refused_for_its_body(&parse(
            r#"{ "message": "Validation Failed", "errors": ["Body is required"] }"#
        )));
        assert!(refused_for_its_body(&parse(
            r#"{"errors": [{"resource": "PullRequestReview", "field": "body", "code": "missing_field"}], "message": "Validation Failed"}"#
        )));
    }
}
