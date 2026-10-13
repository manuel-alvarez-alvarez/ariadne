//! Replying to a note on a merge request, and resolving a discussion a
//! reviewer session opened once a push fixed it, through `glab api` (026,
//! 029). Where the repository holds posts back (002 rule 13, 026 rule 24),
//! a reply is a draft note instead (`reply_pending`): only its own author
//! sees it until they publish it on GitLab.
use serde::Deserialize;

use super::Gitlab;
use super::details::{note_id, project};
use crate::forge::pulls::{CONVERSATION, split_slug};
use ariadne_store::PullRequestComment;

#[derive(Deserialize)]
struct Created {
    id: i64,
}

/// The forge id a draft note is stored under: its own id space, apart from
/// a published note's `note-<n>`.
pub(crate) fn draft_note_id(id: i64) -> String {
    format!("draft-{id}")
}

impl Gitlab {
    /// Reply to `comment` with `body` as a note on its discussion, and
    /// answer the forge id the reply is stored under.
    pub(crate) async fn reply(
        &self,
        repo: &str,
        number: i64,
        comment: &PullRequestComment,
        body: &str,
    ) -> Result<String, String> {
        let (host, owner, name) = split_slug(repo)?;
        let output = self
            .cli
            .answer(&[
                "api",
                &format!(
                    "projects/{}/merge_requests/{number}/discussions/{}/notes",
                    project(owner, name),
                    comment.thread_id
                ),
                "--hostname",
                host,
                "--method",
                "POST",
                "-f",
                &format!("body={body}"),
            ])
            .await?;
        let created: Created = serde_json::from_str(&output)
            .map_err(|e| format!("cannot read the note GitLab stored: {e}"))?;
        Ok(note_id(created.id))
    }

    /// Reply to `comment` with `body` as a draft note on its discussion
    /// (per 026 rule 24): only its own author, the signed-in user, sees it
    /// until they publish it on GitLab. Answers the draft note's own id.
    pub(crate) async fn reply_pending(
        &self,
        repo: &str,
        number: i64,
        comment: &PullRequestComment,
        body: &str,
    ) -> Result<String, String> {
        let (host, owner, name) = split_slug(repo)?;
        let output = self
            .cli
            .answer(&[
                "api",
                &format!(
                    "projects/{}/merge_requests/{number}/draft_notes",
                    project(owner, name)
                ),
                "--hostname",
                host,
                "--method",
                "POST",
                "-f",
                &format!("note={body}"),
                "-f",
                &format!("in_reply_to_discussion_id={}", comment.thread_id),
            ])
            .await?;
        let created: Created = serde_json::from_str(&output)
            .map_err(|e| format!("cannot read the draft note GitLab stored: {e}"))?;
        Ok(draft_note_id(created.id))
    }

    /// Resolve the discussion `comment` is in. The conversation of notes
    /// that start no discussion is none, and nothing resolves it.
    pub(crate) async fn resolve(
        &self,
        repo: &str,
        number: i64,
        comment: &PullRequestComment,
    ) -> Result<(), String> {
        if comment.thread_id == CONVERSATION {
            return Err("only a discussion resolves".into());
        }
        let (host, owner, name) = split_slug(repo)?;
        self.cli
            .answer(&[
                "api",
                &format!(
                    "projects/{}/merge_requests/{number}/discussions/{}",
                    project(owner, name),
                    comment.thread_id
                ),
                "--hostname",
                host,
                "--method",
                "PUT",
                "-f",
                "resolved=true",
            ])
            .await?;
        Ok(())
    }

    /// Resolve the discussion `comment` is in, held back (029 rule 16a,
    /// where the repository holds posts back): the viewer's own draft note
    /// of that discussion takes `resolve_discussion: true`, made first,
    /// with a short body, where there is none. Answers the draft note's own
    /// id.
    pub(crate) async fn resolve_pending(
        &self,
        repo: &str,
        number: i64,
        comment: &PullRequestComment,
    ) -> Result<String, String> {
        if comment.thread_id == CONVERSATION {
            return Err("only a discussion resolves".into());
        }
        let (host, owner, name) = split_slug(repo)?;
        let base = format!(
            "projects/{}/merge_requests/{number}/draft_notes",
            project(owner, name)
        );
        let notes: Vec<DraftNoteSummary> = self.pages(host, &base).await?;
        if let Some(found) = notes
            .iter()
            .find(|note| note.discussion_id.as_deref() == Some(comment.thread_id.as_str()))
        {
            self.cli
                .answer(&[
                    "api",
                    &format!("{base}/{}", found.id),
                    "--hostname",
                    host,
                    "--method",
                    "PUT",
                    "-f",
                    "resolve_discussion=true",
                ])
                .await?;
            return Ok(draft_note_id(found.id));
        }
        let output = self
            .cli
            .answer(&[
                "api",
                &base,
                "--hostname",
                host,
                "--method",
                "POST",
                "-f",
                "note=Resolved.",
                "-f",
                &format!("in_reply_to_discussion_id={}", comment.thread_id),
                "-f",
                "resolve_discussion=true",
            ])
            .await?;
        let created: Created = serde_json::from_str(&output)
            .map_err(|e| format!("cannot read the draft note GitLab stored: {e}"))?;
        Ok(draft_note_id(created.id))
    }
}

/// One entry of `GET .../draft_notes`, read for the discussion a draft
/// note already of the viewer's own is on.
#[derive(Deserialize)]
struct DraftNoteSummary {
    id: i64,
    #[serde(default)]
    discussion_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::time::Duration;

    use super::Gitlab;
    use crate::forge::Cli;
    use ariadne_store::PullRequestComment;

    const STUB: &str = r#"#!/usr/bin/env python3
import json, os, sys
here = os.path.dirname(os.path.abspath(sys.argv[0]))
open(os.path.join(here, "call.json"), "w").write(json.dumps(sys.argv[1:]))
sys.stdout.write(open(os.path.join(here, "answer")).read())
"#;

    fn fake_glab(dir: &std::path::Path, answer: &str) -> String {
        let path = dir.join("glab");
        std::fs::write(&path, STUB).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(dir.join("answer"), answer).unwrap();
        path.display().to_string()
    }

    fn call_args(dir: &std::path::Path) -> Vec<String> {
        serde_json::from_str(&std::fs::read_to_string(dir.join("call.json")).unwrap()).unwrap()
    }

    /// A reply held back posts a draft note on the comment's own discussion,
    /// `in_reply_to_discussion_id`, never a published note, and answers the
    /// draft note's own id, `draft-<n>`.
    #[tokio::test]
    async fn a_pending_reply_posts_a_draft_note_on_the_comments_discussion() {
        let dir = tempfile::tempdir().unwrap();
        let gitlab = Gitlab::new(Cli {
            timeout: Duration::from_secs(5),
            ..Cli::new(Some(&fake_glab(dir.path(), r#"{"id": 55}"#)), "glab")
        });
        let comment = PullRequestComment {
            id: "note-9".into(),
            pull_request_id: "p".into(),
            forge_id: "note-9".into(),
            thread_id: "d1".into(),
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
        let id = gitlab
            .reply_pending("gitlab.example/acme/widgets", 1, &comment, "Fixed it.")
            .await
            .unwrap();
        assert_eq!(id, "draft-55");
        let args = call_args(dir.path());
        assert!(args.contains(&"note=Fixed it.".to_string()), "{args:?}");
        assert!(
            args.contains(&"in_reply_to_discussion_id=d1".to_string()),
            "{args:?}"
        );
        assert!(!args.iter().any(|a| a.contains("draft_notes/")), "{args:?}");
        assert!(
            args.iter()
                .any(|a| a.contains("merge_requests/1/draft_notes")),
            "{args:?}"
        );
    }

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

    fn scripted_call_args(dir: &std::path::Path, n: usize) -> Vec<String> {
        serde_json::from_str(&std::fs::read_to_string(dir.join(format!("call-{n}.json"))).unwrap())
            .unwrap()
    }

    fn discussion_comment(thread_id: &str) -> PullRequestComment {
        PullRequestComment {
            id: "note-9".into(),
            pull_request_id: "p".into(),
            forge_id: "note-9".into(),
            thread_id: thread_id.into(),
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
        }
    }

    /// `resolve_thread` (029 rule 16a), with the repository holding posts
    /// back, sets `resolve_discussion: true` on the viewer's own draft
    /// note already in that discussion, rather than publishing a note or
    /// resolving the discussion directly.
    #[tokio::test]
    async fn resolve_pending_marks_its_own_draft_note_of_the_discussion() {
        let dir = tempfile::tempdir().unwrap();
        let gitlab = Gitlab::new(Cli {
            timeout: Duration::from_secs(5),
            ..Cli::new(
                Some(&fake_glab_scripted(
                    dir.path(),
                    &[
                        r#"[{"id": 55, "discussion_id": "d1"}, {"id": 56, "discussion_id": "d2"}]"#,
                        r#"{}"#,
                    ],
                )),
                "glab",
            )
        });
        let id = gitlab
            .resolve_pending("gitlab.example/acme/widgets", 1, &discussion_comment("d1"))
            .await
            .unwrap();
        assert_eq!(id, "draft-55");
        let update = scripted_call_args(dir.path(), 1);
        assert!(
            update.iter().any(|a| a.contains("draft_notes/55")),
            "{update:?}"
        );
        assert!(
            update.contains(&"resolve_discussion=true".to_string()),
            "{update:?}"
        );
        assert!(update.contains(&"PUT".to_string()), "{update:?}");
    }

    /// With no draft note of the viewer's own already in the discussion,
    /// `resolve_thread` makes one, held back, with `resolve_discussion:
    /// true` set on it from the start.
    #[tokio::test]
    async fn resolve_pending_creates_a_draft_note_with_no_draft_note_of_its_own_yet() {
        let dir = tempfile::tempdir().unwrap();
        let gitlab = Gitlab::new(Cli {
            timeout: Duration::from_secs(5),
            ..Cli::new(
                Some(&fake_glab_scripted(dir.path(), &["[]", r#"{"id": 70}"#])),
                "glab",
            )
        });
        let id = gitlab
            .resolve_pending("gitlab.example/acme/widgets", 1, &discussion_comment("d3"))
            .await
            .unwrap();
        assert_eq!(id, "draft-70");
        let create = scripted_call_args(dir.path(), 1);
        assert!(create.contains(&"POST".to_string()), "{create:?}");
        assert!(
            create.contains(&"in_reply_to_discussion_id=d3".to_string()),
            "{create:?}"
        );
        assert!(
            create.contains(&"resolve_discussion=true".to_string()),
            "{create:?}"
        );
    }

    /// The conversation of notes that start no discussion is no discussion
    /// of its own, and nothing resolves it.
    #[tokio::test]
    async fn resolve_pending_refuses_the_conversation() {
        let dir = tempfile::tempdir().unwrap();
        let gitlab = Gitlab::new(Cli {
            timeout: Duration::from_secs(5),
            ..Cli::new(Some(&fake_glab_scripted(dir.path(), &[])), "glab")
        });
        assert!(
            gitlab
                .resolve_pending(
                    "gitlab.example/acme/widgets",
                    1,
                    &discussion_comment("conversation")
                )
                .await
                .is_err()
        );
    }
}
