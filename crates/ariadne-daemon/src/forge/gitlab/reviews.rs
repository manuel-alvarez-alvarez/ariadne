//! Posting one review of a merge request, through `glab api` (029). GitLab
//! has no review verdict: each inline comment is a discussion of its own,
//! and the review's one summary note, edited on every round, says where it
//! stands. There is no call here that approves: the user gives every
//! approval.
use serde::Deserialize;

use super::Gitlab;
use super::comments::draft_note_id;
use super::details::{note_id, project};
use crate::forge::pulls::{CONVERSATION, ReviewDraft, split_slug};
use ariadne_store::NewPullRequestComment;

/// What `glab api .../draft_notes` answers for one created draft note.
#[derive(Deserialize)]
struct CreatedDraftNote {
    id: i64,
}

#[derive(Deserialize)]
struct MergeRequest {
    diff_refs: DiffRefs,
}

#[derive(Deserialize)]
struct DiffRefs {
    base_sha: String,
    start_sha: String,
    head_sha: String,
}

/// One file of the merge request's diff: a renamed one has two paths.
#[derive(Deserialize)]
struct Change {
    old_path: String,
    new_path: String,
}

#[derive(Deserialize)]
struct Discussion {
    id: String,
    #[serde(default)]
    notes: Vec<PostedNote>,
}

#[derive(Deserialize)]
struct PostedNote {
    id: i64,
    created_at: String,
}

impl Gitlab {
    /// Post `review`: one discussion per inline comment, placed on the
    /// merge request's diff. Answers what it posted.
    pub(crate) async fn submit_review(
        &self,
        repo: &str,
        number: i64,
        review: &ReviewDraft,
        login: &str,
    ) -> Result<Vec<NewPullRequestComment>, String> {
        let (host, owner, name) = split_slug(repo)?;
        let base = format!("projects/{}/merge_requests/{number}", project(owner, name));
        let mut stored = Vec::new();
        if !review.comments.is_empty() {
            let output = self.cli.answer(&["api", &base, "--hostname", host]).await?;
            let refs = serde_json::from_str::<MergeRequest>(&output)
                .map_err(|e| format!("cannot read `glab api {base}`: {e}"))?
                .diff_refs;
            // A position names the file on both sides of the diff, and a
            // renamed file has another path on the old side. Read before
            // any comment is posted, so a failed read posts nothing.
            let changes = self
                .pages::<Change>(host, &format!("{base}/diffs?per_page=100"))
                .await?;
            for comment in &review.comments {
                let old_path = changes
                    .iter()
                    .find(|change| change.new_path == comment.path)
                    .map_or(comment.path.as_str(), |change| change.old_path.as_str());
                let output = self
                    .cli
                    .answer(&[
                        "api",
                        &format!("{base}/discussions"),
                        "--hostname",
                        host,
                        "--method",
                        "POST",
                        "-f",
                        &format!("body={}", comment.body),
                        "-f",
                        "position[position_type]=text",
                        "-f",
                        &format!("position[base_sha]={}", refs.base_sha),
                        "-f",
                        &format!("position[start_sha]={}", refs.start_sha),
                        "-f",
                        &format!("position[head_sha]={}", refs.head_sha),
                        "-f",
                        &format!("position[new_path]={}", comment.path),
                        "-f",
                        &format!("position[old_path]={old_path}"),
                        "-F",
                        &format!("position[new_line]={}", comment.line),
                    ])
                    .await?;
                let discussion: Discussion = serde_json::from_str(&output)
                    .map_err(|e| format!("cannot read the discussion GitLab stored: {e}"))?;
                let Some(note) = discussion.notes.first() else {
                    return Err("GitLab stored a discussion with no note".into());
                };
                stored.push(NewPullRequestComment {
                    forge_id: note_id(note.id),
                    thread_id: discussion.id.clone(),
                    kind: "review_comment".into(),
                    author_login: login.to_string(),
                    author_is_bot: false,
                    body: comment.body.clone(),
                    path: Some(comment.path.clone()),
                    line: Some(comment.line),
                    in_reply_to: None,
                    created_at: note.created_at.clone(),
                    resolved: false,
                    from_review: false,
                });
            }
        }
        Ok(stored)
    }

    /// Post `review`'s findings and `verdict` held back as draft notes
    /// (029 rule 13a), where the repository holds posts back: one draft
    /// note per inline comment, placed on the merge request's diff the
    /// same way `submit_review` does, plus one more draft note of
    /// `verdict` — the event and the round's summary — tied to no line.
    /// Nothing here publishes a note. Answers what it held, as comments of
    /// `login`.
    pub(crate) async fn submit_review_pending(
        &self,
        repo: &str,
        number: i64,
        review: &ReviewDraft,
        login: &str,
        verdict: &str,
    ) -> Result<Vec<NewPullRequestComment>, String> {
        let (host, owner, name) = split_slug(repo)?;
        let base = format!("projects/{}/merge_requests/{number}", project(owner, name));
        let drafts = format!("{base}/draft_notes");
        let mut stored = Vec::new();
        if !review.comments.is_empty() {
            let output = self.cli.answer(&["api", &base, "--hostname", host]).await?;
            let refs = serde_json::from_str::<MergeRequest>(&output)
                .map_err(|e| format!("cannot read `glab api {base}`: {e}"))?
                .diff_refs;
            let changes = self
                .pages::<Change>(host, &format!("{base}/diffs?per_page=100"))
                .await?;
            for comment in &review.comments {
                let old_path = changes
                    .iter()
                    .find(|change| change.new_path == comment.path)
                    .map_or(comment.path.as_str(), |change| change.old_path.as_str());
                let output = self
                    .cli
                    .answer(&[
                        "api",
                        &drafts,
                        "--hostname",
                        host,
                        "--method",
                        "POST",
                        "-f",
                        &format!("note={}", comment.body),
                        "-f",
                        "position[position_type]=text",
                        "-f",
                        &format!("position[base_sha]={}", refs.base_sha),
                        "-f",
                        &format!("position[start_sha]={}", refs.start_sha),
                        "-f",
                        &format!("position[head_sha]={}", refs.head_sha),
                        "-f",
                        &format!("position[new_path]={}", comment.path),
                        "-f",
                        &format!("position[old_path]={old_path}"),
                        "-F",
                        &format!("position[new_line]={}", comment.line),
                    ])
                    .await?;
                let created: CreatedDraftNote = serde_json::from_str(&output)
                    .map_err(|e| format!("cannot read the draft note GitLab stored: {e}"))?;
                stored.push(NewPullRequestComment {
                    forge_id: draft_note_id(created.id),
                    thread_id: draft_note_id(created.id),
                    kind: "review_comment".into(),
                    author_login: login.to_string(),
                    author_is_bot: false,
                    body: comment.body.clone(),
                    path: Some(comment.path.clone()),
                    line: Some(comment.line),
                    in_reply_to: None,
                    created_at: chrono::Utc::now()
                        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                    resolved: false,
                    from_review: false,
                });
            }
        }
        let output = self
            .cli
            .answer(&[
                "api",
                &drafts,
                "--hostname",
                host,
                "--method",
                "POST",
                "-f",
                &format!("note={verdict}"),
            ])
            .await?;
        let created: CreatedDraftNote = serde_json::from_str(&output)
            .map_err(|e| format!("cannot read the draft note GitLab stored: {e}"))?;
        stored.push(NewPullRequestComment {
            forge_id: draft_note_id(created.id),
            thread_id: CONVERSATION.into(),
            kind: "issue_comment".into(),
            author_login: login.to_string(),
            author_is_bot: false,
            body: verdict.to_string(),
            path: None,
            line: None,
            in_reply_to: None,
            created_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            resolved: false,
            from_review: false,
        });
        Ok(stored)
    }

    /// Write the one summary note a review keeps on merge request `number`
    /// (029): `existing`, a `note-<id>` of an earlier round, is edited in
    /// place with `PUT .../notes/<id>`; with none, or one GitLab answers 404
    /// for, a note is posted. Any other failed edit is the error. Answers
    /// its forge id and when it was written.
    pub(crate) async fn write_summary(
        &self,
        repo: &str,
        number: i64,
        existing: Option<&str>,
        body: &str,
    ) -> Result<(String, String), String> {
        let (host, owner, name) = split_slug(repo)?;
        let notes = format!(
            "projects/{}/merge_requests/{number}/notes",
            project(owner, name)
        );
        let field = format!("body={body}");
        let read = |output: &str| {
            serde_json::from_str::<PostedNote>(output)
                .map(|note| (note_id(note.id), note.created_at))
                .map_err(|e| format!("cannot read the note GitLab stored: {e}"))
        };
        if let Some(id) = existing.and_then(|id| id.strip_prefix("note-")) {
            let edited = self
                .cli
                .call(&[
                    "api",
                    &format!("{notes}/{id}"),
                    "--hostname",
                    host,
                    "--method",
                    "PUT",
                    "-f",
                    &field,
                ])
                .await;
            match edited {
                Ok(output) => return read(&output),
                Err(refusal) if refusal.is_missing() => {}
                Err(refusal) => return Err(refusal.to_string()),
            }
        }
        let output = self
            .cli
            .answer(&[
                "api",
                &notes,
                "--hostname",
                host,
                "--method",
                "POST",
                "-f",
                &field,
            ])
            .await?;
        read(&output)
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::time::Duration;

    use serde_json::json;

    use super::Gitlab;
    use crate::forge::Cli;
    use crate::forge::pulls::{DraftComment, ReviewDraft};

    /// A `glab` stub that answers, in order, one scripted `stdout` per
    /// call, numbering each call's own arguments beside it.
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

    fn fake_glab_scripted(dir: &std::path::Path, answers: &[&str]) -> String {
        let path = dir.join("glab");
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

    /// `submit_review`, with the repository holding posts back, makes one
    /// draft note per inline finding, placed on the diff the same way a
    /// posted review is, plus one more draft note of the verdict and
    /// summary, tied to no line, and publishes neither.
    #[tokio::test]
    async fn submit_review_pending_drafts_each_finding_and_the_verdict() {
        let dir = tempfile::tempdir().unwrap();
        let refs =
            json!({"diff_refs": {"base_sha": "base", "start_sha": "start", "head_sha": "head"}});
        let gitlab = Gitlab::new(Cli {
            timeout: Duration::from_secs(5),
            ..Cli::new(
                Some(&fake_glab_scripted(
                    dir.path(),
                    &[&refs.to_string(), "[]", r#"{"id": 501}"#, r#"{"id": 502}"#],
                )),
                "glab",
            )
        });
        let review = ReviewDraft {
            request_changes: true,
            head_sha: "head".into(),
            comments: vec![DraftComment {
                path: "src/lib.rs".into(),
                line: 3,
                body: "**[P0] Empty list**\n\nAn empty list panics.".into(),
            }],
        };
        let stored = gitlab
            .submit_review_pending(
                "gitlab.example/acme/widgets",
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
            ["draft-501", "draft-502"]
        );
        let finding = call_args(dir.path(), 2);
        assert!(finding.contains(&"POST".to_string()), "{finding:?}");
        assert!(
            finding.contains(&"note=**[P0] Empty list**\n\nAn empty list panics.".to_string()),
            "{finding:?}"
        );
        assert!(
            finding.contains(&"position[new_path]=src/lib.rs".to_string()),
            "{finding:?}"
        );
        let verdict = call_args(dir.path(), 3);
        assert!(verdict.contains(&"POST".to_string()), "{verdict:?}");
        assert!(
            verdict.contains(&"note=**Verdict: Request changes**\n\nOne P0 open.".to_string()),
            "{verdict:?}"
        );
        assert!(
            !verdict.iter().any(|a| a.starts_with("position[")),
            "the verdict draft note ties to no line: {verdict:?}"
        );
        assert_eq!(stored.last().unwrap().thread_id, "conversation");
        assert_eq!(stored.last().unwrap().kind, "issue_comment");
    }

    /// A round with no new finding still drafts the verdict and summary,
    /// the same way `write_summary` always writes the posted summary.
    #[tokio::test]
    async fn submit_review_pending_drafts_the_verdict_with_no_finding() {
        let dir = tempfile::tempdir().unwrap();
        let gitlab = Gitlab::new(Cli {
            timeout: Duration::from_secs(5),
            ..Cli::new(
                Some(&fake_glab_scripted(dir.path(), &[r#"{"id": 900}"#])),
                "glab",
            )
        });
        let review = ReviewDraft {
            request_changes: false,
            head_sha: "head".into(),
            comments: vec![],
        };
        let stored = gitlab
            .submit_review_pending(
                "gitlab.example/acme/widgets",
                1,
                &review,
                "me",
                "**Verdict: Comment**\n\nNothing new.",
            )
            .await
            .unwrap();
        assert_eq!(stored.len(), 1, "{stored:?}");
        assert_eq!(stored[0].forge_id, "draft-900");
    }
}
