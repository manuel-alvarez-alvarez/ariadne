//! Posting one review of a pull request, through `gh api` (029). The event
//! is `REQUEST_CHANGES` or `COMMENT`: there is no call here that approves,
//! since the user gives every approval.
use serde::Deserialize;

use super::Github;
use super::details::review_comment_id;
use crate::forge::pulls::{REVIEW_MARK, ReviewDraft, split_slug};
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

impl Github {
    /// Post `review` as one review of its inline findings, `gh api
    /// repos/<owner>/<name>/pulls/<n>/reviews`, with no body of its own,
    /// then read back the comments it holds, so each is stored under the id
    /// the forge gave it. A forge that refuses a review with no body gets it
    /// again with a body that is the hidden review mark alone, so the retry
    /// still shows the reader nothing.
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
                post(Some(REVIEW_MARK))
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

    /// Post `review`'s findings into the viewer's own pending review (029
    /// rule 13a). Each inline comment joins it as a new thread. No event is
    /// set and nothing here submits it. Answers only what it held, as
    /// comments of `login`.
    pub(crate) async fn submit_review_pending(
        &self,
        repo: &str,
        number: i64,
        review: &ReviewDraft,
        login: &str,
    ) -> Result<Vec<NewPullRequestComment>, String> {
        let (host, _, _) = split_slug(repo)?;
        let pending = self.pending_review(repo, number, login).await?;
        let mut stored = Vec::new();
        for comment in &review.comments {
            let output = self
                .cli
                .answer(&[
                    "api",
                    "graphql",
                    "--hostname",
                    host,
                    "-f",
                    "query=mutation($reviewId: ID!, $path: String!, $line: Int!, $body: String!) { \
                     addPullRequestReviewThread(input: {pullRequestReviewId: $reviewId, path: \
                     $path, line: $line, side: RIGHT, body: $body}) { thread { comments(first: 1) \
                     { nodes { databaseId createdAt } } } } }",
                    "-f",
                    &format!("reviewId={}", pending.node_id),
                    "-f",
                    &format!("path={}", comment.path),
                    "-F",
                    &format!("line={}", comment.line),
                    "-f",
                    &format!("body={}", comment.body),
                ])
                .await?;
            let answered: serde_json::Value = serde_json::from_str(&output)
                .map_err(|e| format!("cannot read the pending finding GitHub stored: {e}"))?;
            let node =
                &answered["data"]["addPullRequestReviewThread"]["thread"]["comments"]["nodes"][0];
            // GitHub answers a thread on a line its diff does not show
            // with no error and no thread at all, rather than the 422 a
            // submitted review gets: named here, so a round that reaches
            // this far still says which finding it stopped on.
            let id = node["databaseId"].as_i64().ok_or_else(|| {
                format!(
                    "GitHub stored no pending finding on {}:{}: it shows no such line of the diff",
                    comment.path, comment.line
                )
            })?;
            stored.push(NewPullRequestComment {
                forge_id: review_comment_id(id),
                thread_id: review_comment_id(id),
                kind: "review_comment".into(),
                author_login: login.to_string(),
                author_is_bot: false,
                body: comment.body.clone(),
                path: Some(comment.path.clone()),
                line: Some(comment.line),
                in_reply_to: None,
                created_at: node["createdAt"].as_str().unwrap_or_default().to_string(),
                resolved: false,
                from_review: false,
            });
        }
        Ok(stored)
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
    use std::os::unix::fs::PermissionsExt;
    use std::time::Duration;

    use super::{Github, REVIEW_MARK, ReviewDraft, refused_for_its_body};
    use crate::forge::Cli;

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

    /// A `gh` stub that refuses the first `pulls/<n>/reviews` POST for
    /// having no body, as GitHub does for `REQUEST_CHANGES`, and answers
    /// the retry; it keeps each call's standard input beside it, numbered,
    /// so the test reads what the retry actually sent.
    const STUB: &str = r#"#!/usr/bin/env python3
import os, sys
here = os.path.dirname(os.path.abspath(sys.argv[0]))
count_file = os.path.join(here, "count")
count = int(open(count_file).read()) if os.path.exists(count_file) else 0
open(count_file, "w").write(str(count + 1))
open(os.path.join(here, f"input-{count}"), "w").write(sys.stdin.read())
if count == 0:
    sys.stdout.write('{"message":"Validation Failed","errors":["Body is required"]}')
    sys.exit(1)
sys.stdout.write('{"id": 77, "submitted_at": "2026-10-02T00:00:00Z"}')
"#;

    fn fake_gh(dir: &std::path::Path) -> String {
        let path = dir.join("gh");
        std::fs::write(&path, STUB).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path.display().to_string()
    }

    /// The retry a review refused for its body takes posts no visible text
    /// (031): its body is the hidden review mark alone, never a line a
    /// reader would see pointing at the summary comment too.
    #[tokio::test]
    async fn a_review_refused_for_its_body_is_retried_with_no_visible_text() {
        let dir = tempfile::tempdir().unwrap();
        let github = Github::new(Cli {
            timeout: Duration::from_secs(5),
            ..Cli::new(Some(&fake_gh(dir.path())), "gh")
        });
        let posted = github
            .submit_review(
                "github.com/acme/widgets",
                1,
                &ReviewDraft {
                    request_changes: true,
                    head_sha: "abc".into(),
                    comments: vec![],
                },
                "me",
            )
            .await
            .unwrap();
        assert!(posted.is_empty());
        let input = |n: u32| -> serde_json::Value {
            serde_json::from_str(
                &std::fs::read_to_string(dir.path().join(format!("input-{n}"))).unwrap(),
            )
            .unwrap()
        };
        assert_eq!(input(0).get("body"), None, "the first try carries no body");
        assert_eq!(input(1)["body"], REVIEW_MARK);
    }

    /// A `gh` stub that answers, in order, one scripted `stdout` per call,
    /// numbering each call's own arguments beside it, for tests that read
    /// exactly what a later step sent.
    const SCRIPTED_STUB: &str = r#"#!/usr/bin/env python3
import json, os, sys
here = os.path.dirname(os.path.abspath(sys.argv[0]))
count_file = os.path.join(here, "count")
count = int(open(count_file).read()) if os.path.exists(count_file) else 0
open(count_file, "w").write(str(count + 1))
open(os.path.join(here, f"call-{count}.json"), "w").write(json.dumps(sys.argv[1:]))
answers = json.load(open(os.path.join(here, "answers.json")))
sys.stdout.write(answers[count])
"#;

    fn fake_gh_scripted(dir: &std::path::Path, answers: &[&str]) -> String {
        let path = dir.join("gh");
        std::fs::write(&path, SCRIPTED_STUB).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(
            dir.join("answers.json"),
            serde_json::to_string(answers).unwrap(),
        )
        .unwrap();
        path.display().to_string()
    }

    fn call_args(dir: &std::path::Path, n: usize) -> Vec<String> {
        serde_json::from_str(&std::fs::read_to_string(dir.join(format!("call-{n}.json"))).unwrap())
            .unwrap()
    }

    /// A supervised review adds only its inline findings to the pending
    /// review. It leaves the pending body untouched.
    #[tokio::test]
    async fn submit_review_pending_adds_each_finding_without_changing_the_body() {
        let dir = tempfile::tempdir().unwrap();
        let binary = fake_gh_scripted(
            dir.path(),
            &[
                r#"[{"id":9,"node_id":"PR_kwDOx","state":"PENDING","body":"User draft.","user":{"login":"me"}}]"#,
                r#"{"data":{"addPullRequestReviewThread":{"thread":{"comments":{"nodes":[
                    {"databaseId": 501, "createdAt": "2026-10-02T00:00:00Z"}]}}}}}"#,
            ],
        );
        let github = Github::new(Cli {
            timeout: Duration::from_secs(5),
            ..Cli::new(Some(&binary), "gh")
        });
        let review = ReviewDraft {
            request_changes: true,
            head_sha: "abc".into(),
            comments: vec![crate::forge::pulls::DraftComment {
                path: "src/lib.rs".into(),
                line: 3,
                body: "**[P0] Empty list**\n\nAn empty list panics.\n\n<!-- ariadne:review -->"
                    .into(),
            }],
        };
        let stored = github
            .submit_review_pending("github.com/acme/widgets", 1, &review, "me")
            .await
            .unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].forge_id, "rc-501");
        let calls = (0..2).map(|n| call_args(dir.path(), n)).collect::<Vec<_>>();
        assert!(
            calls.iter().all(|call| !call
                .iter()
                .any(|arg| arg.contains("updatePullRequestReview"))),
            "the pending body stays user-authored: {calls:?}"
        );
    }
}
