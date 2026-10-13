//! Posting one review of a merge request, through `glab api` (029). GitLab
//! has no review verdict: each inline comment is a discussion of its own,
//! and the review's one summary note, edited on every round, says where it
//! stands. There is no call here that approves: the user gives every
//! approval.
use serde::Deserialize;

use super::Gitlab;
use super::details::{note_id, project};
use crate::forge::pulls::{ReviewDraft, split_slug};
use ariadne_store::NewPullRequestComment;

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

/// One entry of `GET .../draft_notes` (031): its own id is all
/// [`Gitlab::draft_posts`] needs, since that call only ever answers the
/// count.
#[derive(Deserialize)]
struct DraftNote {
    #[allow(dead_code)]
    id: i64,
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

    /// The draft notes count GitLab keeps on request `number` (031): held
    /// back where `supervise_posts` is on so a person finishes and
    /// publishes them themselves, directly on the forge. `GET
    /// .../draft_notes` answers only the signed-in viewer's own, so nothing
    /// here filters by login the way GitHub's pending-review read must.
    pub(crate) async fn draft_posts(&self, repo: &str, number: i64) -> Result<i64, String> {
        let (host, owner, name) = split_slug(repo)?;
        let path = format!(
            "projects/{}/merge_requests/{number}/draft_notes",
            project(owner, name)
        );
        let notes: Vec<DraftNote> = self.object(host, &path).await?;
        Ok(notes.len() as i64)
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::time::Duration;

    use super::Gitlab;
    use crate::forge::Cli;

    const DRAFT_NOTES_STUB: &str = r#"#!/usr/bin/env python3
import sys
sys.stdout.write('[{"id": 1}, {"id": 2}]')
"#;

    fn fake_glab_draft_notes(dir: &std::path::Path) -> String {
        let path = dir.join("glab");
        std::fs::write(&path, DRAFT_NOTES_STUB).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path.display().to_string()
    }

    /// `draft_posts` reaches the forge through `glab api .../draft_notes`,
    /// and answers the length of whatever it lists (031) — the whole round
    /// trip, not only the count.
    #[tokio::test]
    async fn draft_posts_counts_the_draft_notes_glab_lists() {
        let dir = tempfile::tempdir().unwrap();
        let gitlab = Gitlab::new(Cli {
            timeout: Duration::from_secs(5),
            ..Cli::new(Some(&fake_glab_draft_notes(dir.path())), "glab")
        });
        let count = gitlab
            .draft_posts("gitlab.com/group/sub/widgets", 7)
            .await
            .unwrap();
        assert_eq!(count, 2);
    }
}
