//! What a pull request holds beyond the list fetch's fields, through
//! `gh api` (026): its comments of every kind with their threads, the
//! checks that failed on its head, and whether its head is behind its base.
use std::collections::HashMap;

use serde::Deserialize;
use serde::de::DeserializeOwned;

use super::Github;
use crate::forge::pulls::{CONVERSATION, FailedCheck, ForgeDetails, split_slug};
use ariadne_store::NewPullRequestComment;

/// The conclusions of a check run that count as a failure.
const FAILED: [&str; 5] = [
    "failure",
    "timed_out",
    "cancelled",
    "action_required",
    "startup_failure",
];

/// The review threads of a request, with the ids of the comments in each.
/// `gh api graphql --paginate` walks `$endCursor` through every page. A
/// reply past a thread's first hundred comments is placed by the comment it
/// answers, which is the thread's first.
const THREADS: &str = "query($owner: String!, $name: String!, $number: Int!, $endCursor: String) {
  repository(owner: $owner, name: $name) {
    pullRequest(number: $number) {
      reviewThreads(first: 100, after: $endCursor) {
        pageInfo { hasNextPage endCursor }
        nodes { id isResolved comments(first: 100) { nodes { databaseId } } }
      }
    }
  }
}";

#[derive(Deserialize)]
struct User {
    login: String,
    #[serde(rename = "type", default)]
    kind: String,
}

#[derive(Deserialize)]
struct ReviewComment {
    id: i64,
    in_reply_to_id: Option<i64>,
    user: User,
    #[serde(default)]
    body: String,
    path: Option<String>,
    line: Option<i64>,
    original_line: Option<i64>,
    created_at: String,
}

#[derive(Deserialize)]
struct IssueComment {
    id: i64,
    user: User,
    #[serde(default)]
    body: String,
    created_at: String,
}

#[derive(Deserialize)]
struct Review {
    id: i64,
    user: User,
    #[serde(default)]
    body: String,
    submitted_at: Option<String>,
}

#[derive(Deserialize)]
struct CheckRuns {
    #[serde(default)]
    check_runs: Vec<CheckRun>,
}

#[derive(Deserialize)]
struct CheckRun {
    name: String,
    html_url: Option<String>,
    conclusion: Option<String>,
}

#[derive(Deserialize)]
struct Compare {
    #[serde(default)]
    behind_by: i64,
}

/// The forge id a review comment is stored under: the id spaces of the
/// three kinds are the forge's own, so each kind carries a prefix.
pub(crate) fn review_comment_id(id: i64) -> String {
    format!("rc-{id}")
}

pub(crate) fn issue_comment_id(id: i64) -> String {
    format!("ic-{id}")
}

pub(crate) fn review_id(id: i64) -> String {
    format!("rv-{id}")
}

fn is_bot(user: &User) -> bool {
    user.kind.eq_ignore_ascii_case("bot") || user.login.ends_with("[bot]")
}

impl Github {
    /// `gh api --hostname <host> --paginate <path>`, read as every page's
    /// items: `--paginate` writes one JSON value per page, back to back.
    async fn pages<T: DeserializeOwned>(&self, host: &str, path: &str) -> Result<Vec<T>, String> {
        let output = self
            .cli
            .answer(&["api", path, "--hostname", host, "--paginate"])
            .await?;
        let mut items = Vec::new();
        for page in serde_json::Deserializer::from_str(&output).into_iter::<Vec<T>>() {
            items.extend(page.map_err(|e| format!("cannot read `gh api {path}`: {e}"))?);
        }
        Ok(items)
    }

    /// Every page of a paginated `gh api` object, one value per page.
    async fn object_pages<T: DeserializeOwned>(
        &self,
        host: &str,
        path: &str,
    ) -> Result<Vec<T>, String> {
        let output = self
            .cli
            .answer(&["api", path, "--hostname", host, "--paginate"])
            .await?;
        serde_json::Deserializer::from_str(&output)
            .into_iter::<T>()
            .map(|page| page.map_err(|e| format!("cannot read `gh api {path}`: {e}")))
            .collect()
    }

    /// One `gh api` object.
    async fn object<T: DeserializeOwned>(&self, host: &str, path: &str) -> Result<T, String> {
        let output = self.cli.answer(&["api", path, "--hostname", host]).await?;
        serde_json::from_str(&output).map_err(|e| format!("cannot read `gh api {path}`: {e}"))
    }

    pub(crate) async fn details(&self, repo: &str, number: i64) -> Result<ForgeDetails, String> {
        let pull = self.pull_request(repo, number).await?;
        let (host, owner, name) = split_slug(repo)?;
        let base = format!("repos/{owner}/{name}");
        let threads = self.threads(host, owner, name, number).await?;
        let mut comments = Vec::new();
        for c in self
            .pages::<ReviewComment>(host, &format!("{base}/pulls/{number}/comments"))
            .await?
        {
            let root = c.in_reply_to_id.unwrap_or(c.id);
            let (thread_id, resolved) = threads
                .get(&c.id)
                .or_else(|| threads.get(&root))
                .cloned()
                .unwrap_or_else(|| (review_comment_id(root), false));
            comments.push(NewPullRequestComment {
                forge_id: review_comment_id(c.id),
                thread_id,
                kind: "review_comment".into(),
                author_is_bot: is_bot(&c.user),
                author_login: c.user.login,
                body: c.body,
                path: c.path,
                line: c.line.or(c.original_line),
                in_reply_to: c.in_reply_to_id.map(review_comment_id),
                created_at: c.created_at,
                resolved,
            });
        }
        for c in self
            .pages::<IssueComment>(host, &format!("{base}/issues/{number}/comments"))
            .await?
        {
            comments.push(NewPullRequestComment {
                forge_id: issue_comment_id(c.id),
                thread_id: CONVERSATION.into(),
                kind: "issue_comment".into(),
                author_is_bot: is_bot(&c.user),
                author_login: c.user.login,
                body: c.body,
                path: None,
                line: None,
                in_reply_to: None,
                created_at: c.created_at,
                resolved: false,
            });
        }
        for r in self
            .pages::<Review>(host, &format!("{base}/pulls/{number}/reviews"))
            .await?
        {
            // An approval with nothing written asks nothing.
            let (Some(at), false) = (r.submitted_at, r.body.trim().is_empty()) else {
                continue;
            };
            comments.push(NewPullRequestComment {
                forge_id: review_id(r.id),
                thread_id: CONVERSATION.into(),
                kind: "review".into(),
                author_is_bot: is_bot(&r.user),
                author_login: r.user.login,
                body: r.body,
                path: None,
                line: None,
                in_reply_to: None,
                created_at: at,
                resolved: false,
            });
        }
        let failed_checks = self
            .object_pages::<CheckRuns>(
                host,
                &format!("{base}/commits/{}/check-runs", pull.head_sha),
            )
            .await?
            .into_iter()
            .flat_map(|page| page.check_runs)
            .filter_map(|run| {
                let conclusion = run.conclusion?.to_lowercase();
                FAILED.contains(&conclusion.as_str()).then(|| FailedCheck {
                    name: run.name,
                    url: run.html_url.unwrap_or_default(),
                    conclusion,
                })
            })
            .collect();
        let compare: Compare = self
            .object(
                host,
                &format!("{base}/compare/{}...{}", pull.base_branch, pull.head_sha),
            )
            .await?;
        Ok(ForgeDetails {
            pull,
            comments,
            failed_checks,
            behind_base: compare.behind_by > 0,
        })
    }

    /// The review thread of each review comment, by the comment's numeric
    /// id: its GraphQL node id and whether it is resolved.
    pub(super) async fn threads(
        &self,
        host: &str,
        owner: &str,
        name: &str,
        number: i64,
    ) -> Result<HashMap<i64, (String, bool)>, String> {
        let output = self
            .cli
            .answer(&[
                "api",
                "graphql",
                "--hostname",
                host,
                "--paginate",
                "-f",
                &format!("query={THREADS}"),
                "-F",
                &format!("owner={owner}"),
                "-F",
                &format!("name={name}"),
                "-F",
                &format!("number={number}"),
            ])
            .await?;
        let mut threads = HashMap::new();
        let mut nodes = Vec::new();
        for page in serde_json::Deserializer::from_str(&output).into_iter::<serde_json::Value>() {
            let page = page.map_err(|e| format!("cannot read the review threads: {e}"))?;
            nodes.extend(
                page["data"]["repository"]["pullRequest"]["reviewThreads"]["nodes"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default(),
            );
        }
        for thread in nodes {
            let (Some(id), resolved) = (thread["id"].as_str(), thread["isResolved"] == true) else {
                continue;
            };
            for comment in thread["comments"]["nodes"].as_array().into_iter().flatten() {
                if let Some(database_id) = comment["databaseId"].as_i64() {
                    threads.insert(database_id, (id.to_string(), resolved));
                }
            }
        }
        Ok(threads)
    }
}
