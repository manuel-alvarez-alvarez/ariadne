//! What a merge request holds beyond the list fetch's fields, through
//! `glab api` (026): its notes with their discussions, the jobs that failed
//! in its latest pipeline, and whether its head is behind its target.
use serde::Deserialize;
use serde::de::DeserializeOwned;

use super::Gitlab;
use crate::forge::pulls::{FailedCheck, ForgeDetails, split_slug};
use ariadne_store::NewPullRequestComment;

#[derive(Deserialize)]
struct Discussion {
    id: String,
    #[serde(default)]
    notes: Vec<Note>,
}

#[derive(Deserialize)]
struct Note {
    id: i64,
    #[serde(rename = "type")]
    kind: Option<String>,
    #[serde(default)]
    body: String,
    author: Author,
    created_at: String,
    #[serde(default)]
    system: bool,
    #[serde(default)]
    resolved: bool,
    position: Option<Position>,
}

#[derive(Deserialize)]
struct Author {
    username: String,
    #[serde(default)]
    bot: bool,
}

#[derive(Deserialize)]
struct Position {
    new_path: Option<String>,
    new_line: Option<i64>,
    old_path: Option<String>,
    old_line: Option<i64>,
}

#[derive(Deserialize)]
struct Pipeline {
    id: i64,
}

#[derive(Deserialize)]
struct Job {
    name: String,
    status: String,
    web_url: Option<String>,
    #[serde(default)]
    allow_failure: bool,
}

#[derive(Deserialize)]
struct Diverged {
    #[serde(default)]
    diverged_commits_count: i64,
}

/// A project as `glab api` paths name it: its full path, URL-encoded.
pub(crate) fn project(owner: &str, name: &str) -> String {
    format!("{owner}/{name}").replace('/', "%2F")
}

/// What `glab -R` takes for `host/group/name`: the project's URL. `-R` reads
/// a bare `host/group/name` as a group path on the default host, so the
/// host would be lost.
pub(crate) fn repo_url(slug: &str) -> Result<String, String> {
    let (host, owner, name) = split_slug(slug)?;
    Ok(format!("https://{host}/{owner}/{name}"))
}

pub(crate) fn note_id(id: i64) -> String {
    format!("note-{id}")
}

impl Gitlab {
    /// `glab api <path> --hostname <host> --paginate`, read as every page's
    /// items, one JSON list per page.
    pub(super) async fn pages<T: DeserializeOwned>(
        &self,
        host: &str,
        path: &str,
    ) -> Result<Vec<T>, String> {
        let output = self
            .cli
            .answer(&["api", path, "--hostname", host, "--paginate"])
            .await?;
        let mut items = Vec::new();
        for page in serde_json::Deserializer::from_str(&output).into_iter::<Vec<T>>() {
            items.extend(page.map_err(|e| format!("cannot read `glab api {path}`: {e}"))?);
        }
        Ok(items)
    }

    pub(super) async fn object<T: DeserializeOwned>(
        &self,
        host: &str,
        path: &str,
    ) -> Result<T, String> {
        let output = self.cli.answer(&["api", path, "--hostname", host]).await?;
        serde_json::from_str(&output).map_err(|e| format!("cannot read `glab api {path}`: {e}"))
    }

    pub(crate) async fn details(&self, repo: &str, number: i64) -> Result<ForgeDetails, String> {
        let (host, owner, name) = split_slug(repo)?;
        let base = format!("projects/{}/merge_requests/{number}", project(owner, name));
        let discussions = format!("{base}/discussions");
        let diverged = format!("{base}?include_diverged_commits_count=true");
        // Reads that wait on the forge, not on each other.
        let (pull, discussions, failed_checks, diverged) = tokio::try_join!(
            self.pull_request(repo, number),
            self.pages::<Discussion>(host, &discussions),
            self.failed_checks(host, owner, name, &base),
            self.object::<Diverged>(host, &diverged),
        )?;
        let mut comments = Vec::new();
        for discussion in discussions {
            let notes: Vec<Note> = discussion.notes.into_iter().filter(|n| !n.system).collect();
            let first = notes.first().map(|n| note_id(n.id));
            let resolved = notes.iter().any(|n| n.resolved);
            for note in notes {
                let id = note_id(note.id);
                let (path, line) = match note.position {
                    Some(p) => (p.new_path.or(p.old_path), p.new_line.or(p.old_line)),
                    None => (None, None),
                };
                comments.push(NewPullRequestComment {
                    in_reply_to: first.clone().filter(|root| *root != id),
                    forge_id: id,
                    thread_id: discussion.id.clone(),
                    kind: match note.kind.as_deref() {
                        Some("DiffNote") => "review_comment",
                        _ => "issue_comment",
                    }
                    .into(),
                    author_is_bot: note.author.bot,
                    author_login: note.author.username,
                    body: note.body,
                    path,
                    line,
                    created_at: note.created_at,
                    resolved,
                    from_review: false,
                });
            }
        }
        Ok(ForgeDetails {
            pull,
            comments,
            failed_checks,
            behind_base: diverged.diverged_commits_count > 0,
        })
    }

    /// The jobs that failed in the request's latest pipeline, and must not.
    async fn failed_checks(
        &self,
        host: &str,
        owner: &str,
        name: &str,
        base: &str,
    ) -> Result<Vec<FailedCheck>, String> {
        let pipelines: Vec<Pipeline> = self.object(host, &format!("{base}/pipelines")).await?;
        let Some(latest) = pipelines.first() else {
            return Ok(Vec::new());
        };
        let jobs: Vec<Job> = self
            .pages(
                host,
                &format!(
                    "projects/{}/pipelines/{}/jobs",
                    project(owner, name),
                    latest.id
                ),
            )
            .await?;
        Ok(jobs
            .into_iter()
            .filter(|job| job.status == "failed" && !job.allow_failure)
            .map(|job| FailedCheck {
                name: job.name,
                url: job.web_url.unwrap_or_default(),
                conclusion: job.status,
            })
            .collect())
    }
}
