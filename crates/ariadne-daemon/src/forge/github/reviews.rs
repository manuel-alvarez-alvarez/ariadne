//! Posting one review of a pull request, through `gh api` (029). The event
//! is `REQUEST_CHANGES` or `COMMENT`: there is no call here that approves,
//! since the user gives every approval.
use serde::Deserialize;

use super::Github;
use super::details::{issue_comment_id, review_comment_id, review_id};
use crate::forge::pulls::{CONVERSATION, REVIEW_MARK, ReviewDraft, split_slug};
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
    /// rule 13a), held back rather than submitted, where the repository
    /// holds posts back: each inline comment joins it as a new thread,
    /// `addPullRequestReviewThread`, and `header` — the verdict and the
    /// round's summary — is set at the review body's own top
    /// (`set_pending_review_header`), creating the pending review first
    /// where there is none. No event is set, and nothing here submits it.
    /// Answers what it held, as comments of `login`.
    pub(crate) async fn submit_review_pending(
        &self,
        repo: &str,
        number: i64,
        review: &ReviewDraft,
        login: &str,
        header: &str,
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
        self.apply_pending_header(host, &pending, header).await?;
        // Recorded as the review's the same way the posted summary is
        // (`hold_review_comments`), under the pending review's own id: the
        // verdict and summary have no comment of their own on GitHub, so
        // this is what stands for it until the forge lets a fetch see the
        // pending review itself.
        stored.push(NewPullRequestComment {
            forge_id: review_id(pending.id),
            thread_id: CONVERSATION.into(),
            kind: "issue_comment".into(),
            author_login: login.to_string(),
            author_is_bot: false,
            body: header.to_string(),
            path: None,
            line: None,
            in_reply_to: None,
            created_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            resolved: false,
            from_review: false,
        });
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

    /// `submit_review`, with the repository holding posts back, adds each
    /// finding to the viewer's own pending review as a new thread, sets
    /// the verdict and summary at the review body's own top, and holds a
    /// `from_review` conversation entry of the verdict too, under the
    /// pending review's own id, with no event ever submitted: the pending
    /// review's listing is read once, not once per finding.
    #[tokio::test]
    async fn submit_review_pending_adds_each_finding_and_sets_the_header() {
        let dir = tempfile::tempdir().unwrap();
        let binary = fake_gh_scripted(
            dir.path(),
            &[
                r#"[{"id":9,"node_id":"PR_kwDOx","state":"PENDING","body":"","user":{"login":"me"}}]"#,
                r#"{"data":{"addPullRequestReviewThread":{"thread":{"comments":{"nodes":[
                    {"databaseId": 501, "createdAt": "2026-10-02T00:00:00Z"}]}}}}}"#,
                r#"{"data":{"addPullRequestReviewThread":{"thread":{"comments":{"nodes":[
                    {"databaseId": 502, "createdAt": "2026-10-02T00:01:00Z"}]}}}}}"#,
                r#"{"data":{"updatePullRequestReview":{"pullRequestReview":{"id":"PR_kwDOx"}}}}"#,
            ],
        );
        let github = Github::new(Cli {
            timeout: Duration::from_secs(5),
            ..Cli::new(Some(&binary), "gh")
        });
        let review = ReviewDraft {
            request_changes: true,
            head_sha: "abc".into(),
            comments: vec![
                crate::forge::pulls::DraftComment {
                    path: "src/lib.rs".into(),
                    line: 3,
                    body: "**[P0] Empty list**\n\nAn empty list panics.\n\n<!-- ariadne:review -->"
                        .into(),
                },
                crate::forge::pulls::DraftComment {
                    path: "tests/it.rs".into(),
                    line: 9,
                    body: "**[P1] Untested**\n\nNo test covers it.\n\n<!-- ariadne:review -->"
                        .into(),
                },
            ],
        };
        let stored = github
            .submit_review_pending(
                "github.com/acme/widgets",
                1,
                &review,
                "me",
                "**Verdict: Request changes**\n\nOne P0 open.",
            )
            .await
            .unwrap();
        assert_eq!(
            stored
                .iter()
                .map(|c| c.forge_id.clone())
                .collect::<Vec<_>>(),
            ["rc-501", "rc-502", "rv-9"]
        );
        assert!(stored[..2].iter().all(|c| c.kind == "review_comment"));
        let verdict = stored.last().unwrap();
        assert_eq!(verdict.kind, "issue_comment");
        assert_eq!(verdict.thread_id, "conversation");
        assert_eq!(verdict.body, "**Verdict: Request changes**\n\nOne P0 open.");
        let pending_list = call_args(dir.path(), 0);
        assert!(
            !pending_list.contains(&"--method".to_string()),
            "the pending review is read, not created again: {pending_list:?}"
        );
        let first = call_args(dir.path(), 1);
        assert!(
            first.contains(&"reviewId=PR_kwDOx".to_string()),
            "{first:?}"
        );
        assert!(first.contains(&"path=src/lib.rs".to_string()), "{first:?}");
        assert!(first.contains(&"line=3".to_string()), "{first:?}");
        let header_call = call_args(dir.path(), 3);
        let body = header_call
            .iter()
            .find_map(|a| a.strip_prefix("body="))
            .expect("a body argument");
        assert_eq!(
            body,
            "**Verdict: Request changes**\n\nOne P0 open.\n<!-- ariadne:pending-review-verdict -->"
        );
    }

    /// A round with no new finding still holds the verdict and summary,
    /// under the pending review's own id, the same way a round with no
    /// comments writes the posted summary.
    #[tokio::test]
    async fn submit_review_pending_holds_the_verdict_with_no_finding() {
        let dir = tempfile::tempdir().unwrap();
        let binary = fake_gh_scripted(
            dir.path(),
            &[
                r#"[{"id":9,"node_id":"PR_kwDOx","state":"PENDING","body":"","user":{"login":"me"}}]"#,
                r#"{"data":{"updatePullRequestReview":{"pullRequestReview":{"id":"PR_kwDOx"}}}}"#,
            ],
        );
        let github = Github::new(Cli {
            timeout: Duration::from_secs(5),
            ..Cli::new(Some(&binary), "gh")
        });
        let review = ReviewDraft {
            request_changes: false,
            head_sha: "abc".into(),
            comments: vec![],
        };
        let stored = github
            .submit_review_pending(
                "github.com/acme/widgets",
                1,
                &review,
                "me",
                "**Verdict: Comment**\n\nNothing new.",
            )
            .await
            .unwrap();
        assert_eq!(stored.len(), 1, "{stored:?}");
        assert_eq!(stored[0].forge_id, "rv-9");
        assert_eq!(stored[0].kind, "issue_comment");
    }
}
