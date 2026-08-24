//! The GitLab CLI, as much of it as watching a merge request takes.
//!
//! [`crate::gh`]'s opposite number, and the same shape: a task published as a
//! merge request is reviewed and merged by people on GitLab, so what the
//! daemon does about it is look — `glab mr view -F json` for the merge
//! request itself, `glab api` for the approvals and the discussions on it —
//! and read what one look says has to happen next (see [`poll_state`]).
//!
//! Two of the three reads go through `glab api` rather than through a command
//! of its own: what comes back is GitLab's REST JSON, whose shapes are
//! documented and versioned, where `glab`'s own rendering of approvals and
//! discussions is neither. The third is `glab mr view`, which answers with
//! the same REST object and saves this having to spell the project out.
//!
//! Both are run in the checkout the task belongs to, so `glab` reads the same
//! configuration and credentials the integrator ran it with by hand, and both
//! are told the host and the project the recorded URL names rather than
//! letting `glab` guess from whichever remote it likes — a checkout may have
//! several.

use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use tokio::process::Command;

use crate::forge::{self, Conflict, FailedCheck, Health, Landing, parse_pages};

pub use crate::forge::{Feedback, GITLAB_HOST, PrState, WatchedPr, pull_request_number};

/// The states GitLab spells an open merge request with. Anything else is
/// `merged`, `closed` or `locked`.
const OPENED: &str = "opened";

/// What `detailed_merge_status` spells a branch that no longer merges with.
/// Its other values are about everything else that can stand in the way —
/// `not_approved`, `ci_must_pass`, `checking` — none of which is a conflict.
const CONFLICT: &str = "conflict";

/// The one pipeline status that is a red branch. `canceled` is what a project
/// that cancels its own superseded pipelines writes on every push, `skipped`
/// and `manual` are pipelines nobody ran, and everything else — `created`,
/// `pending`, `running`, `waiting_for_resource` — has not finished.
const FAILED: &str = "failed";

/// What the engineer is told the red thing is called: GitLab runs one
/// pipeline over the merge result, so there is one of these rather than a
/// check per job.
const PIPELINE: &str = "pipeline";

#[derive(Debug, Clone)]
pub struct GlabCli {
    bin: String,
}

/// One merge request, as much of it as a poll asks for.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct MergeRequest {
    /// `opened`, `merged`, `closed` or `locked`.
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub merged_at: Option<String>,
    /// The commit the merge landed as. GitLab writes one of the two: the
    /// merge commit for a merge, the squash commit for a squashed one —
    /// neither on any branch of ours until the base is fetched.
    #[serde(default)]
    pub merge_commit_sha: Option<String>,
    #[serde(default)]
    pub squash_commit_sha: Option<String>,
    /// The head commit, which the pipeline ran on and the conflict was read
    /// on: what keeps one failure to one relay, and makes the failure on the
    /// revision that answered it a new one.
    #[serde(default)]
    pub sha: Option<String>,
    /// The branch it is open against, which is what the engineer merges in to
    /// reconcile a conflict.
    #[serde(default)]
    pub target_branch: Option<String>,
    /// Whether GitLab found the branch and the target in conflict. Absent on
    /// an instance old enough not to answer it, and on a merge status it has
    /// not worked out yet.
    #[serde(default)]
    pub has_conflicts: Option<bool>,
    /// The same question in more detail: `mergeable`, `conflict`,
    /// `not_approved`, `checking`, and a dozen more. Only `conflict` is one.
    ///
    /// What is deliberately not read is the older `merge_status`: its
    /// `cannot_be_merged` covers a conflict and several things that are not
    /// one, and an engineer sent back for a merge GitLab was merely still
    /// checking is an engineer sent back for nothing.
    #[serde(default)]
    pub detailed_merge_status: Option<String>,
    /// The pipeline of the head commit. `pipeline` is what older instances
    /// answer with instead, and it is the same reading.
    #[serde(default)]
    pub head_pipeline: Option<Pipeline>,
    #[serde(default)]
    pub pipeline: Option<Pipeline>,
}

/// One pipeline, as much of it as a poll reads: what became of it and where a
/// person reads it.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Pipeline {
    /// `created`, `waiting_for_resource`, `preparing`, `pending`, `running`,
    /// `success`, `failed`, `canceled`, `skipped`, `manual`, `scheduled`.
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub web_url: Option<String>,
}

impl MergeRequest {
    /// What GitLab says became of the branch: merged or not, and whichever
    /// commit the merge landed as.
    pub fn landing(&self) -> Landing {
        Landing {
            state: self.state.clone(),
            merged: self.state.eq_ignore_ascii_case("merged") || self.merged_at.is_some(),
            commits: [&self.merge_commit_sha, &self.squash_commit_sha]
                .into_iter()
                .flatten()
                .filter(|sha| !sha.is_empty())
                .cloned()
                .collect(),
        }
    }

    /// Whether it is still open, which is what an approval worth announcing
    /// needs: a closed merge request is nobody's to merge.
    pub fn is_open(&self) -> bool {
        self.state.eq_ignore_ascii_case(OPENED)
    }

    /// What GitLab says about the branch itself: whether it still merges into
    /// its target, and whether its pipeline is red.
    ///
    /// Both degrade to "nothing to see" rather than to a failure — an
    /// instance that answers neither conflict field, a merge status still
    /// being checked, a merge request with no pipeline at all: an engineer
    /// woken for a build nobody said had failed is an engineer woken for
    /// nothing.
    pub fn health(&self) -> Health {
        let head = self.sha.as_deref().filter(|s| !s.is_empty());
        let conflicting = self.has_conflicts.unwrap_or(false)
            || self
                .detailed_merge_status
                .as_deref()
                .is_some_and(|s| s.eq_ignore_ascii_case(CONFLICT));
        let pipeline = self.head_pipeline.as_ref().or(self.pipeline.as_ref());
        Health {
            conflict: conflicting.then(|| Conflict {
                id: forge::conflict_id(head),
                base: self
                    .target_branch
                    .as_deref()
                    .map(str::trim)
                    .filter(|b| !b.is_empty())
                    .map(str::to_string),
            }),
            failed_checks: pipeline
                .filter(|p| p.status.eq_ignore_ascii_case(FAILED))
                .map(|p| FailedCheck {
                    id: forge::check_id(head, PIPELINE),
                    name: PIPELINE.to_string(),
                    conclusion: p.status.clone(),
                    url: p
                        .web_url
                        .as_deref()
                        .map(str::trim)
                        .filter(|u| !u.is_empty())
                        .map(str::to_string),
                })
                .into_iter()
                .collect(),
        }
    }
}

/// The approval state of a merge request, as `…/approvals` answers.
///
/// `approved` is what recent GitLab answers with outright; `approved_by` is
/// the list every version carries, and a merge request with somebody in it is
/// approved whether or not the field beside it exists.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Approvals {
    #[serde(default)]
    pub approved: Option<bool>,
    #[serde(default)]
    pub approved_by: Vec<Approver>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Approver {
    #[serde(default)]
    pub user: Option<User>,
}

impl Approvals {
    pub fn is_approved(&self) -> bool {
        self.approved.unwrap_or(false) || !self.approved_by.is_empty()
    }
}

/// One discussion thread on the merge request, conversation or diff alike.
///
/// GitLab keeps both in the same place, which is the difference from GitHub:
/// there is no second endpoint holding what was written on the diff, only
/// notes carrying a `position` where they hang on one.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Discussion {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub notes: Vec<Note>,
}

/// One note in a discussion.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Note {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub author: Option<User>,
    #[serde(default)]
    pub body: String,
    /// GitLab writes its own notes into the same threads — "added 1 commit",
    /// "approved this merge request" — and marks them.
    #[serde(default)]
    pub system: bool,
    /// Whether the thread it opens can be resolved, and whether it has been:
    /// an unresolved one is GitLab's way of saying this still has to be dealt
    /// with, which is the change request the engineer is sent back with.
    #[serde(default)]
    pub resolvable: bool,
    #[serde(default)]
    pub resolved: bool,
    /// Where on the diff it hangs, when it hangs on one.
    #[serde(default)]
    pub position: Option<Position>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct User {
    #[serde(default)]
    pub username: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Position {
    #[serde(default)]
    pub new_path: Option<String>,
    #[serde(default)]
    pub old_path: Option<String>,
    /// The line it hangs on, on whichever side of the diff it was written:
    /// a note on a line that was added carries the new one, a note on a line
    /// that was deleted the old.
    #[serde(default)]
    pub new_line: Option<i64>,
    #[serde(default)]
    pub old_line: Option<i64>,
}

impl Note {
    /// As one piece of feedback: the file and line it is about travel beside
    /// what it says, since that is what the engineer needs to find it.
    fn as_feedback(&self) -> Feedback {
        let position = self.position.as_ref();
        let path = position.and_then(|p| p.new_path.as_deref().or(p.old_path.as_deref()));
        let line = position.and_then(|p| p.new_line.or(p.old_line));
        Feedback {
            id: format!("N{}", self.id),
            author: forge::author_or_someone(self.author.as_ref().map(|a| a.username.as_str())),
            body: self.body.trim().to_string(),
            file: forge::location(path, line),
            blocking: self.resolvable && !self.resolved,
        }
    }
}

/// Read one poll: the merge request as `glab` reports it, against what the
/// task already remembers of it.
///
/// `relayed` is the note ids already handed to the engineer and
/// `approved_notified` whether the user has already been told this one is
/// ready to merge — both live on the task, so a daemon that restarts
/// mid-review picks up where it left off rather than repeating itself.
pub fn poll_state(
    mr: &MergeRequest,
    approvals: &Approvals,
    discussions: &[Discussion],
    relayed: &[String],
    approved_notified: bool,
) -> PrState {
    forge::poll_state(
        mr.landing().merged,
        forge::unrelayed(feedback(discussions), relayed),
        &mr.health(),
        relayed,
        approvals.is_approved() && mr.is_open(),
        approved_notified,
    )
}

/// What humans wrote on the merge request, in the order GitLab threads it.
///
/// GitLab's own notes are not feedback: "added 1 commit" and "approved this
/// merge request" are the merge request narrating itself, and an engineer
/// sent back with one has been sent back for nothing.
fn feedback(discussions: &[Discussion]) -> Vec<Feedback> {
    discussions
        .iter()
        .flat_map(|d| d.notes.iter())
        .filter(|n| !n.system)
        .map(Note::as_feedback)
        .collect()
}

/// The project a merge request URL names — `group/subgroup/project`, however
/// many groups deep it is nested — which is everything between the host and
/// the `/-/` GitLab separates a project's own pages with.
pub fn project_path(url: &str) -> Option<String> {
    let (before, _) = url.trim().split_once("/-/merge_requests/")?;
    let (_, rest) = before.rsplit_once("://")?;
    let (_, path) = rest.split_once('/')?;
    let path = path.trim_matches('/');
    (!path.is_empty()).then(|| path.to_string())
}

/// The same, as a REST path segment: GitLab takes a project by its full path
/// with the slashes escaped, which is what `projects/:id` means everywhere in
/// its API.
fn encoded_project(url: &str) -> Option<String> {
    let path = project_path(url)?;
    Some(
        path.chars()
            .map(|c| match c {
                'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
                other => other
                    .to_string()
                    .into_bytes()
                    .iter()
                    .map(|b| format!("%{b:02X}"))
                    .collect(),
            })
            .collect(),
    )
}

impl GlabCli {
    pub fn new(bin: impl Into<String>) -> Self {
        Self { bin: bin.into() }
    }

    /// One merge request, by the number and project its URL names.
    pub async fn mr_view(&self, repo: &Path, mr: &WatchedPr) -> Result<MergeRequest> {
        let number = mr.number.to_string();
        let raw = self
            .run(
                repo,
                &[
                    "mr",
                    "view",
                    &number,
                    "-R",
                    &self.repo_arg(mr)?,
                    "-F",
                    "json",
                ],
            )
            .await?;
        serde_json::from_str(&raw)
            .with_context(|| format!("reading `glab mr view {}` output: {raw}", mr.url))
    }

    /// Whether the reviewers approved it. GitLab keeps that off the merge
    /// request itself — approval rules are their own resource — so it is its
    /// own read.
    pub async fn mr_approvals(&self, repo: &Path, mr: &WatchedPr) -> Result<Approvals> {
        let path = format!("{}/approvals", self.mr_api_path(mr)?);
        let raw = self.api(repo, mr, &path, false).await?;
        serde_json::from_str(&raw)
            .with_context(|| format!("reading `glab api {path}` output: {raw}"))
    }

    /// Every discussion on it, conversation and diff alike, and every page of
    /// them: a thread is where a reviewer says what has to change, and one
    /// note left unread is a round of feedback the engineer never gets.
    pub async fn mr_discussions(&self, repo: &Path, mr: &WatchedPr) -> Result<Vec<Discussion>> {
        // A hundred at a time, which is the most GitLab gives: fewer pages
        // for `--paginate` to walk on a merge request people have really been
        // through.
        let path = format!("{}/discussions?per_page=100", self.mr_api_path(mr)?);
        let raw = self.api(repo, mr, &path, true).await?;
        parse_pages(&raw).with_context(|| format!("reading `glab api {path}` output: {raw}"))
    }

    /// `<host>/<project>`, the way `glab --repo` takes a project on a
    /// self-hosted instance as well as on gitlab.com.
    fn repo_arg(&self, mr: &WatchedPr) -> Result<String> {
        let host =
            forge::host_of(&mr.url).with_context(|| format!("{} names no GitLab host", mr.url))?;
        let project =
            project_path(&mr.url).with_context(|| format!("{} names no project", mr.url))?;
        Ok(format!("{host}/{project}"))
    }

    /// The REST path of the merge request itself, which everything else hangs
    /// off.
    fn mr_api_path(&self, mr: &WatchedPr) -> Result<String> {
        let project =
            encoded_project(&mr.url).with_context(|| format!("{} names no project", mr.url))?;
        Ok(format!("projects/{project}/merge_requests/{}", mr.number))
    }

    async fn api(&self, repo: &Path, mr: &WatchedPr, path: &str, paginate: bool) -> Result<String> {
        let host =
            forge::host_of(&mr.url).with_context(|| format!("{} names no GitLab host", mr.url))?;
        let mut args = vec!["api", "--hostname", &host];
        if paginate {
            args.push("--paginate");
        }
        args.push(path);
        self.run(repo, &args).await
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
                "glab {} failed in {}: {}",
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

    /// `glab mr view 3 -F json` on an open merge request nobody has touched,
    /// in the shape GitLab's own API answers with: green, mergeable, and with
    /// a head commit everything about the branch is keyed by.
    fn open_mr() -> serde_json::Value {
        serde_json::json!({
            "iid": 3,
            "state": "opened",
            "merged_at": null,
            "merge_commit_sha": null,
            "squash_commit_sha": null,
            "sha": "abc123",
            "target_branch": "main",
            "has_conflicts": false,
            "detailed_merge_status": "mergeable",
            "head_pipeline": {
                "id": 4711,
                "sha": "abc123",
                "ref": "render-the-board",
                "status": "success",
                "web_url": "https://gitlab.com/owner/repo/-/pipelines/4711",
            },
            "web_url": "https://gitlab.com/owner/repo/-/merge_requests/3",
        })
    }

    /// The health of a merge request whose head pipeline is in `status`.
    fn pipeline(status: serde_json::Value) -> Health {
        let mut value = open_mr();
        value["head_pipeline"]["status"] = status;
        mr(value).health()
    }

    fn mr(value: serde_json::Value) -> MergeRequest {
        serde_json::from_value(value).expect("glab json")
    }

    /// `…/approvals`, with and without anybody on it.
    fn approvals(who: &[&str]) -> Approvals {
        let by: Vec<serde_json::Value> = who
            .iter()
            .map(|u| serde_json::json!({"user": {"username": u}}))
            .collect();
        serde_json::from_value(serde_json::json!({
            "approved": !who.is_empty(),
            "approved_by": by,
        }))
        .expect("glab json")
    }

    fn discussions(raw: &str) -> Vec<Discussion> {
        parse_pages(raw).expect("glab's own output")
    }

    #[test]
    fn an_untouched_merge_request_wakes_nobody() {
        assert_eq!(
            poll_state(&mr(open_mr()), &approvals(&[]), &[], &[], false),
            PrState::Quiet
        );
    }

    #[test]
    fn an_approval_is_read_once_and_then_is_quiet() {
        let open = mr(open_mr());
        let approved = approvals(&["maria"]);
        assert_eq!(
            poll_state(&open, &approved, &[], &[], false),
            PrState::Approved
        );
        // Once the user has been told, every further poll says nothing.
        assert_eq!(poll_state(&open, &approved, &[], &[], true), PrState::Quiet);
        // And an approval on a merge request nobody can merge any more is not
        // one to send anybody to: it is closed.
        let mut closed = open_mr();
        closed["state"] = "closed".into();
        assert_eq!(
            poll_state(&mr(closed), &approved, &[], &[], false),
            PrState::Quiet
        );
    }

    /// The whole of the discussions endpoint's shape: a conversation note, a
    /// note on the diff carrying the file it hangs on, and one GitLab wrote
    /// itself. Two of the three are feedback.
    #[test]
    fn discussion_notes_are_feedback_until_they_are_relayed() {
        let raw = r#"[
            {"id":"d1","notes":[
                {"id":101,"author":{"username":"maria"},"body":"why a new module?",
                 "system":false,"resolvable":false,"resolved":false,"type":null}]},
            {"id":"d2","notes":[
                {"id":102,"author":{"username":"jon"},"body":"this allocates per row",
                 "system":false,"resolvable":true,"resolved":false,"type":"DiffNote",
                 "position":{"new_path":"src/board.rs","old_path":"src/board.rs",
                  "new_line":42,"old_line":null}}]},
            {"id":"d3","notes":[
                {"id":103,"author":{"username":"maria"},"body":"approved this merge request",
                 "system":true,"resolvable":false,"resolved":false}]}
        ]"#;
        let threads = discussions(raw);
        let PrState::Feedback(feedback) =
            poll_state(&mr(open_mr()), &approvals(&[]), &threads, &[], false)
        else {
            panic!("the notes are feedback");
        };
        assert_eq!(
            feedback,
            vec![
                Feedback {
                    id: "N101".into(),
                    author: "maria".into(),
                    body: "why a new module?".into(),
                    file: None,
                    blocking: false,
                },
                Feedback {
                    id: "N102".into(),
                    author: "jon".into(),
                    body: "this allocates per row".into(),
                    file: Some("src/board.rs:42".into()),
                    blocking: true,
                },
            ],
            "GitLab's own note is the merge request narrating itself, not a reviewer"
        );

        // Relayed once is relayed for good, whatever the poll goes on saying.
        assert_eq!(
            poll_state(
                &mr(open_mr()),
                &approvals(&[]),
                &threads,
                &["N101".into(), "N102".into()],
                false
            ),
            PrState::Quiet
        );
        // And one already relayed leaves only what is new.
        let PrState::Feedback(later) = poll_state(
            &mr(open_mr()),
            &approvals(&[]),
            &threads,
            &["N101".into()],
            false,
        ) else {
            panic!("the unrelayed note is still feedback");
        };
        assert_eq!(later.len(), 1);
        assert_eq!(later[0].id, "N102");

        // A thread its reviewer resolved is no longer blocking, and relaying
        // it a second time is still out of the question.
        let resolved = discussions(&raw.replace(
            r#""resolved":false,"type":"DiffNote""#,
            r#""resolved":true,"type":"DiffNote""#,
        ));
        let PrState::Feedback(feedback) = poll_state(
            &mr(open_mr()),
            &approvals(&[]),
            &resolved,
            &["N101".into()],
            false,
        ) else {
            panic!("a resolved note is still something the engineer never saw");
        };
        assert!(!feedback[0].blocking);
    }

    /// Feedback comes before an approval: announcing "ready to merge" over a
    /// comment nobody has acted on would be the wrong half of the truth.
    #[test]
    fn feedback_is_read_before_an_approval() {
        let threads = discussions(
            r#"[{"id":"d1","notes":[{"id":101,"author":{"username":"maria"},
                 "body":"one nit, otherwise fine","system":false}]}]"#,
        );
        let approved = approvals(&["maria"]);
        assert!(matches!(
            poll_state(&mr(open_mr()), &approved, &threads, &[], false),
            PrState::Feedback(_)
        ));
        assert_eq!(
            poll_state(&mr(open_mr()), &approved, &threads, &["N101".into()], false),
            PrState::Approved
        );
    }

    /// A merged merge request is finished whatever else it says, and the
    /// commit it landed as is whichever of the two GitLab wrote.
    #[test]
    fn a_merged_merge_request_is_read_as_merged() {
        let mut merged = open_mr();
        merged["state"] = "merged".into();
        merged["merged_at"] = "2026-08-24T10:00:00Z".into();
        merged["squash_commit_sha"] = "abc123".into();
        let parsed = mr(merged);
        let threads = discussions(
            r#"[{"id":"d1","notes":[{"id":109,"author":{"username":"maria"},
                 "body":"merging this","system":false}]}]"#,
        );
        assert_eq!(
            poll_state(&parsed, &approvals(&["maria"]), &threads, &[], false),
            PrState::Merged
        );
        assert_eq!(parsed.landing().commits, vec!["abc123".to_string()]);
        assert!(parsed.landing().merged);
        assert!(!parsed.is_open());

        // A plain merge writes the other field, and both are checked against
        // the local base when the merge is reported.
        let mut merge_commit = open_mr();
        merge_commit["state"] = "merged".into();
        merge_commit["merge_commit_sha"] = "def456".into();
        assert_eq!(
            mr(merge_commit).landing().commits,
            vec!["def456".to_string()]
        );

        // An open one landed nothing at all.
        let landing = mr(open_mr()).landing();
        assert!(!landing.merged);
        assert_eq!(landing.state, "opened");
        assert!(landing.commits.is_empty());
    }

    /// A note with nothing written in it is a click, not a change request.
    #[test]
    fn an_empty_note_is_not_feedback() {
        let threads = discussions(
            r#"[{"id":"d1","notes":[{"id":101,"author":{"username":"jon"},"body":"   ",
                 "system":false,"resolvable":true,"resolved":false}]}]"#,
        );
        assert_eq!(
            poll_state(&mr(open_mr()), &approvals(&[]), &threads, &[], false),
            PrState::Quiet
        );
    }

    /// Every page of discussions is read: `glab api --paginate` writes one
    /// array per page, and a note on the second page is a round of feedback
    /// like any other.
    #[test]
    fn notes_from_a_later_page_are_relayed_like_any_other() {
        let threads = discussions(
            r#"[{"id":"d1","notes":[{"id":1,"author":{"username":"jon"},"body":"page one","system":false}]}]
               [{"id":"d2","notes":[{"id":2,"author":{"username":"maria"},"body":"page two","system":false}]}]"#,
        );
        let PrState::Feedback(feedback) =
            poll_state(&mr(open_mr()), &approvals(&[]), &threads, &[], false)
        else {
            panic!("notes on any page are feedback");
        };
        assert_eq!(
            feedback.iter().map(|f| f.id.as_str()).collect::<Vec<_>>(),
            vec!["N1", "N2"]
        );
        assert_eq!(
            poll_state(
                &mr(open_mr()),
                &approvals(&[]),
                &threads,
                &["N1".into(), "N2".into()],
                false
            ),
            PrState::Quiet,
            "and neither page comes round again"
        );
    }

    /// The pipeline, read the one way: red is a pipeline that failed, and
    /// everything else — running, cancelled, skipped, a status this has never
    /// heard of, no pipeline at all — is not.
    #[test]
    fn the_pipeline_is_read_as_red_only_where_gitlab_says_failed() {
        for green in ["success", "skipped", "manual"] {
            assert!(
                pipeline(green.into()).failed_checks.is_empty(),
                "a {green} pipeline was read as failing"
            );
        }
        for pending in [
            "created",
            "waiting_for_resource",
            "preparing",
            "pending",
            "running",
            "scheduled",
        ] {
            assert!(
                pipeline(pending.into()).failed_checks.is_empty(),
                "a {pending} pipeline was read as failing"
            );
        }
        // Cancelled is what a project that supersedes its own pipelines
        // writes on every push, and a status this does not know is unknown.
        for unknown in ["canceled", "something_new", ""] {
            assert!(
                pipeline(unknown.into()).failed_checks.is_empty(),
                "a {unknown} pipeline was read as failing"
            );
        }
        // And a merge request with no pipeline at all, on a project with no
        // CI: absent, not failing.
        let mut none = open_mr();
        none["head_pipeline"] = serde_json::Value::Null;
        assert!(mr(none).health().failed_checks.is_empty());
        assert!(
            mr(serde_json::json!({"state": "opened"}))
                .health()
                .failed_checks
                .is_empty(),
            "a `glab` that answered none of the fields answers no failure"
        );

        assert_eq!(
            pipeline("failed".into()).failed_checks,
            vec![FailedCheck {
                id: "CHKabc123:pipeline".into(),
                name: "pipeline".into(),
                conclusion: "failed".into(),
                url: Some("https://gitlab.com/owner/repo/-/pipelines/4711".into()),
            }]
        );

        // An instance old enough to answer `pipeline` and no `head_pipeline`
        // is read the same way.
        let mut older = open_mr();
        older["pipeline"] = older["head_pipeline"].clone();
        older["pipeline"]["status"] = "failed".into();
        older["head_pipeline"] = serde_json::Value::Null;
        assert_eq!(mr(older).health().failed_checks.len(), 1);
    }

    /// Mergeability, the same way: what GitLab calls a conflict is one, and
    /// the merge status it is still working out is not.
    #[test]
    fn a_conflicting_merge_request_is_the_only_one_read_as_conflicting() {
        let mergeability = |conflicts: serde_json::Value, detailed: serde_json::Value| {
            let mut value = open_mr();
            value["has_conflicts"] = conflicts;
            value["detailed_merge_status"] = detailed;
            mr(value).health().conflict
        };
        assert_eq!(
            mergeability(true.into(), "conflict".into()),
            Some(Conflict {
                id: "MRGabc123".into(),
                base: Some("main".into()),
            })
        );
        assert!(
            mergeability(false.into(), "conflict".into()).is_some(),
            "the detailed status says it on its own"
        );
        assert!(
            mergeability(true.into(), "mergeable".into()).is_some(),
            "and so does the flag"
        );
        for clean in [
            (serde_json::json!(false), serde_json::json!("mergeable")),
            // Everything else that can stand in the way of a merge, none of
            // which is the branch failing to merge...
            (serde_json::json!(false), serde_json::json!("not_approved")),
            (serde_json::json!(false), serde_json::json!("ci_must_pass")),
            (serde_json::json!(false), serde_json::json!("draft_status")),
            // ...and the merge GitLab has not worked out yet, or an instance
            // that answers neither field.
            (serde_json::json!(null), serde_json::json!("checking")),
            (serde_json::json!(null), serde_json::json!(null)),
        ] {
            assert_eq!(
                mergeability(clean.0.clone(), clean.1.clone()),
                None,
                "{clean:?} was read as a conflict"
            );
        }
    }

    /// What a poll makes of them: a conflict before the pipeline it is likely
    /// to have failed, each relayed once, the failure on the revision that was
    /// supposed to fix it relayed again — and no approval announced over
    /// either of them.
    #[test]
    fn a_red_or_conflicting_branch_is_relayed_once_per_commit() {
        let approved = approvals(&["maria"]);
        let mut red = open_mr();
        red["head_pipeline"]["status"] = "failed".into();
        let failed = vec![FailedCheck {
            id: "CHKabc123:pipeline".into(),
            name: "pipeline".into(),
            conclusion: "failed".into(),
            url: Some("https://gitlab.com/owner/repo/-/pipelines/4711".into()),
        }];
        assert_eq!(
            poll_state(&mr(red.clone()), &approved, &[], &[], false),
            PrState::ChecksFailed(failed.clone())
        );
        // Handed over once, the same failure says nothing more — and the
        // approval stays unannounced for as long as the branch is red.
        assert_eq!(
            poll_state(
                &mr(red.clone()),
                &approved,
                &[],
                &["CHKabc123:pipeline".into()],
                false
            ),
            PrState::Quiet
        );

        // The engineer pushed a fix, and the pipeline failed on it too.
        let mut again = red.clone();
        again["sha"] = "def456".into();
        let PrState::ChecksFailed(new) = poll_state(
            &mr(again),
            &approved,
            &[],
            &["CHKabc123:pipeline".into()],
            false,
        ) else {
            panic!("a failure on the revision that answered one is news again");
        };
        assert_eq!(new[0].id, "CHKdef456:pipeline");

        // And the conflict is read first, being what the pipeline of a merge
        // that no longer applies failed for.
        let mut conflicting = red;
        conflicting["has_conflicts"] = true.into();
        assert_eq!(
            poll_state(&mr(conflicting.clone()), &approved, &[], &[], false),
            PrState::Conflicting(Conflict {
                id: "MRGabc123".into(),
                base: Some("main".into()),
            })
        );
        assert_eq!(
            poll_state(
                &mr(conflicting.clone()),
                &approved,
                &[],
                &["MRGabc123".into()],
                false
            ),
            PrState::ChecksFailed(failed),
            "the conflict was handed over; the failing pipeline has not been"
        );
        assert_eq!(
            poll_state(
                &mr(conflicting),
                &approved,
                &[],
                &["MRGabc123".into(), "CHKabc123:pipeline".into()],
                false
            ),
            PrState::Quiet,
            "an approval announced over a branch nobody can merge is the wrong half of the truth"
        );
    }

    /// Approvals as every version of GitLab answers them: the outright flag
    /// where there is one, the list of approvers where there is not.
    #[test]
    fn a_merge_request_is_approved_when_anybody_approved_it() {
        let parse = |v: serde_json::Value| -> Approvals { serde_json::from_value(v).unwrap() };
        assert!(!parse(serde_json::json!({"approved_by": []})).is_approved());
        assert!(
            parse(serde_json::json!({"approved_by": [{"user": {"username": "maria"}}]}))
                .is_approved(),
            "an older GitLab answers with the approvers and no flag"
        );
        assert!(parse(serde_json::json!({"approved": true, "approved_by": []})).is_approved());
        assert!(!parse(serde_json::json!({"approved": false})).is_approved());
    }

    /// The project a URL names, however deep its groups nest, and the escaped
    /// form GitLab's own API paths take it in.
    #[test]
    fn the_project_a_merge_request_url_names() {
        assert_eq!(
            project_path("https://gitlab.com/owner/repo/-/merge_requests/3"),
            Some("owner/repo".to_string())
        );
        assert_eq!(
            project_path("https://gitlab.example.com/group/sub/project/-/merge_requests/17/diffs"),
            Some("group/sub/project".to_string()),
            "whatever the URL goes on to point at inside the merge request"
        );
        assert_eq!(project_path("https://gitlab.com/owner/repo"), None);

        assert_eq!(
            encoded_project("https://gitlab.com/group/sub/project/-/merge_requests/3"),
            Some("group%2Fsub%2Fproject".to_string())
        );
    }

    /// `glab api projects/…/merge_requests/3` on this project, in the shape
    /// GitLab really answers with: the fields the structs above are read
    /// from, and a heap of ones they are not.
    #[test]
    fn a_real_glab_answer_parses_into_what_a_poll_reads() {
        let raw = r#"{"id":184919,"iid":3,"project_id":29,"title":"Draft: render the board",
            "state":"merged","created_at":"2026-08-20T09:10:11.000Z",
            "merged_at":"2026-08-24T10:00:00.000Z","closed_at":null,"target_branch":"main",
            "source_branch":"render-the-board-x1y2z3","user_notes_count":2,"upvotes":0,"downvotes":0,
            "author":{"id":7,"username":"maria","name":"Maria"},"draft":false,"work_in_progress":false,
            "merge_status":"can_be_merged","detailed_merge_status":"mergeable",
            "sha":"9f1c2b7","merge_commit_sha":null,"squash_commit_sha":"5c81311ec8329",
            "discussion_locked":null,"should_remove_source_branch":true,"force_remove_source_branch":true,
            "web_url":"https://gitlab.com/owner/repo/-/merge_requests/3","squash":true,
            "task_completion_status":{"count":0,"completed_count":0},"has_conflicts":false}"#;
        let parsed: MergeRequest = serde_json::from_str(raw).expect("glab's own output");
        assert!(parsed.landing().merged);
        assert_eq!(parsed.landing().commits, vec!["5c81311ec8329".to_string()]);
        assert!(
            !parsed.health().blocks_merge(),
            "a mergeable merge request with no conflicts blocks nothing"
        );
        // Merged wins over the notes on it: what a note is relayed for is a
        // revision, and there is nothing left to revise.
        assert_eq!(
            poll_state(&parsed, &approvals(&["maria"]), &[], &[], false),
            PrState::Merged
        );
    }
}
