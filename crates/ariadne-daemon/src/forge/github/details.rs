//! What a pull request holds beyond the list fetch's fields, through
//! `gh api` (026): its comments of every kind with their threads, the
//! checks that failed on its head, and whether its head is behind its base.
use std::collections::{HashMap, HashSet};

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
    /// The review this comment belongs to: GitHub's own undocumented
    /// behavior answers a comment of a still-pending review here too, not
    /// only once it is submitted, so this is what tells the two apart
    /// (`pending_review_ids`).
    pull_request_review_id: Option<i64>,
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
    #[serde(default)]
    state: Option<String>,
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

/// The ids of every still-pending review among `reviews`: no
/// `submitted_at`, or a `state` GitHub still shows as `PENDING`. GitHub
/// answers a pending review only to the user who left it, so every one
/// `gh api` ever lists here is already the viewer's own, and filtering by
/// login again would only cost a real leak the moment that login is ever
/// unknown or wrong — read as `""` and matching nothing, with every
/// pending review then kept rather than dropped.
///
/// A pending review's own body is already kept off the conversation
/// (`details`'s own review loop below), but GitHub's `pulls/{number}/comments`
/// is undocumented for whether it also answers a pending review's own
/// inline comments and replies — it does — so every comment naming one of
/// these ids (`ReviewComment::pull_request_review_id`) is filtered the same
/// way.
fn pending_review_ids(reviews: &[Review]) -> HashSet<i64> {
    reviews
        .iter()
        .filter(|r| r.submitted_at.is_none() || r.state.as_deref() == Some("PENDING"))
        .map(|r| r.id)
        .collect()
}

impl Github {
    /// `gh api --hostname <host> --paginate <path>`, read as every page's
    /// items: `--paginate` writes one JSON value per page, back to back.
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
    pub(super) async fn object<T: DeserializeOwned>(
        &self,
        host: &str,
        path: &str,
    ) -> Result<T, String> {
        let output = self.cli.answer(&["api", path, "--hostname", host]).await?;
        serde_json::from_str(&output).map_err(|e| format!("cannot read `gh api {path}`: {e}"))
    }

    /// `_login` is unread: GitHub answers a pending review only to the
    /// user who left it, so every review `pulls/{number}/reviews` lists
    /// with no `submitted_at` is already the viewer's own
    /// (`pending_review_ids`), with nothing here to tell apart by it.
    pub(crate) async fn details(
        &self,
        repo: &str,
        number: i64,
        _login: &str,
    ) -> Result<ForgeDetails, String> {
        let (host, owner, name) = split_slug(repo)?;
        let base = format!("repos/{owner}/{name}");
        let review_comments = format!("{base}/pulls/{number}/comments");
        let issue_comments = format!("{base}/issues/{number}/comments");
        let reviews = format!("{base}/pulls/{number}/reviews");
        // The checks and the comparison name the head and the base, so they
        // wait on the request's own read; the rest wait on the forge alone.
        let head = async {
            let pull = self.pull_request(repo, number).await?;
            let checks = format!("{base}/commits/{}/check-runs", pull.head_sha);
            let compare = format!("{base}/compare/{}...{}", pull.base_branch, pull.head_sha);
            let (checks, compare) = tokio::try_join!(
                self.object_pages::<CheckRuns>(host, &checks),
                self.object::<Compare>(host, &compare),
            )?;
            Ok::<_, String>((pull, checks, compare))
        };
        let ((pull, checks, compare), threads, review_comments, issue_comments, reviews) = tokio::try_join!(
            head,
            self.threads(host, owner, name, number),
            self.pages::<ReviewComment>(host, &review_comments),
            self.pages::<IssueComment>(host, &issue_comments),
            self.pages::<Review>(host, &reviews),
        )?;
        // The viewer's own pending reviews (031): `reviews`, already read
        // above, already lists each (`submitted_at` absent, or `state`
        // still `PENDING`) for no further cost. Ignored here (the human
        // must finish and submit them directly on the forge): no comment,
        // reply or thread of theirs is ever analyzed, told as news, or
        // woken for.
        let pending = pending_review_ids(&reviews);
        let mut comments = Vec::new();
        for c in review_comments {
            // GitHub's `pulls/{number}/comments` is undocumented for
            // whether it also answers a pending review's own inline
            // comments and replies — it does — so each is dropped here
            // rather than entering the conversation half-published.
            if c.pull_request_review_id
                .is_some_and(|id| pending.contains(&id))
            {
                continue;
            }
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
                from_review: false,
            });
        }
        for c in issue_comments {
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
                from_review: false,
            });
        }
        for r in reviews {
            // A review still pending — `submitted_at` absent, or `state`
            // still `PENDING` — is dropped the same way its own comments
            // and replies are, whatever its body: a review GitHub still
            // shows as pending, with a `submitted_at` value of its own,
            // is not "submitted" for this purpose either.
            if pending.contains(&r.id) {
                continue;
            }
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
                from_review: false,
            });
        }
        let failed_checks = checks
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
        Ok(ForgeDetails {
            pull,
            comments,
            failed_checks,
            behind_base: compare.behind_by > 0,
            has_pending_posts: !pending.is_empty(),
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

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::time::Duration;

    use super::*;
    use crate::forge::Cli;

    fn review(id: i64, login: &str, submitted: bool) -> Review {
        Review {
            id,
            user: User {
                login: login.into(),
                kind: String::new(),
            },
            body: "a finding".into(),
            submitted_at: submitted.then(|| "2026-10-01T00:00:00Z".into()),
            state: None,
        }
    }

    /// Every still-pending review is read, whoever left it — GitHub
    /// answers a pending review only to its own author, so a login check
    /// here would only risk a leak the moment it is ever wrong or unknown
    /// — and a submitted review counts for nothing, whatever body it
    /// carries. GitHub can still show `state: "PENDING"` once a body is
    /// given a pending review of its own, with `submitted_at` absent
    /// either way; the state alone is still read as pending.
    #[test]
    fn pending_review_ids_reads_every_still_pending_review() {
        let reviews = vec![
            review(1, "me", false),
            review(2, "me", true),
            review(3, "someone-else", false),
        ];
        assert_eq!(pending_review_ids(&reviews), HashSet::from([1, 3]));
        assert!(pending_review_ids(&[]).is_empty());
        assert!(
            pending_review_ids(&[review(2, "me", true)]).is_empty(),
            "a submitted review counts for nothing"
        );
        let still_pending = Review {
            state: Some("PENDING".into()),
            ..review(4, "me", false)
        };
        assert_eq!(pending_review_ids(&[still_pending]), HashSet::from([4]));
    }

    const SCRIPTED_STUB: &str = r#"#!/usr/bin/env python3
import json, os, sys
here = os.path.dirname(os.path.abspath(sys.argv[0]))
args = sys.argv[1:]
answers = json.load(open(os.path.join(here, "answers.json")))
for entry in answers:
    prefix = entry["args"]
    if args[:len(prefix)] == prefix:
        sys.stdout.write(entry["stdout"])
        sys.exit(0)
sys.stderr.write(f"no entry for {args}")
sys.exit(1)
"#;

    fn fake_gh(dir: &std::path::Path, answers: &[(&[&str], &str)]) -> String {
        let path = dir.join("gh");
        std::fs::write(&path, SCRIPTED_STUB).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        let answers: Vec<_> = answers
            .iter()
            .map(|(args, stdout)| serde_json::json!({"args": args, "stdout": stdout}))
            .collect();
        std::fs::write(
            dir.join("answers.json"),
            serde_json::to_string(&answers).unwrap(),
        )
        .unwrap();
        path.display().to_string()
    }

    fn github(dir: &std::path::Path, answers: &[(&[&str], &str)]) -> Github {
        Github::new(Cli {
            timeout: Duration::from_secs(5),
            ..Cli::new(Some(&fake_gh(dir, answers)), "gh")
        })
    }

    /// A detail read whose viewer holds a pending review drops that
    /// review's own body, its inline comments and its replies from the
    /// conversation outright: GitHub's `pulls/{number}/comments` answers
    /// them the same as a submitted review's own (undocumented, but real —
    /// the shape pull request #110 showed waking the reviewer and keeper
    /// agents), so the filter reads `pull_request_review_id` against the
    /// pending set rather than trusting the endpoint to withhold them.
    #[tokio::test]
    async fn a_pending_reviews_body_comments_and_replies_are_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let pull = r#"{"number":1,"url":"https://github.com/acme/widgets/pull/1",
            "title":"Fix widgets","author":{"login":"other"},"state":"OPEN","isDraft":false,
            "headRefName":"fix","headRefOid":"abc",
            "headRepository":{"url":"https://github.com/acme/widgets"},
            "baseRefName":"main","statusCheckRollup":[],"reviewDecision":"",
            "createdAt":"2026-10-01T00:00:00Z"}"#;
        // The thread of the pending review's own comment and its reply is
        // listed here too, the way GitHub's own GraphQL read would: its
        // only comments are both pending, so it must carry the thread into
        // the final list with it, through neither.
        let threads = r#"{"data":{"repository":{"pullRequest":{"reviewThreads":{"nodes":[
            {"id":"RT_1","isResolved":false,"comments":{"nodes":[
                {"databaseId":10},{"databaseId":11}]}}
        ]}}}}}"#;
        // Review 9 is still pending by `submitted_at` alone; review 13 is
        // still pending by `state` alone, carrying a `submitted_at` value
        // GitHub can leave behind on one it has not actually submitted
        // (029, 031); review 8 is a submitted review of a different
        // user's. Comment 10 is the pending review's own; comment 11 is a
        // reply to it, also under the pending review's id; comment 12
        // belongs to the submitted review and stays.
        let comments = r#"[
            {"id":10,"in_reply_to_id":null,"user":{"login":"me"},"body":"a finding",
             "path":"src/lib.rs","line":1,"created_at":"2026-10-01T00:00:00Z",
             "pull_request_review_id":9},
            {"id":11,"in_reply_to_id":10,"user":{"login":"me"},"body":"a reply",
             "path":"src/lib.rs","line":1,"created_at":"2026-10-01T00:01:00Z",
             "pull_request_review_id":9},
            {"id":12,"in_reply_to_id":null,"user":{"login":"alice"},"body":"another finding",
             "path":"src/lib.rs","line":2,"created_at":"2026-10-01T00:02:00Z",
             "pull_request_review_id":8}
        ]"#;
        let reviews = r#"[
            {"id":8,"user":{"login":"alice"},"body":"Looks good.",
             "submitted_at":"2026-10-01T00:00:00Z"},
            {"id":9,"user":{"login":"me"},"body":"a pending verdict","submitted_at":null},
            {"id":13,"user":{"login":"me"},"body":"an odd pending state","state":"PENDING",
             "submitted_at":"2026-10-01T00:03:00Z"}
        ]"#;
        let github = github(
            dir.path(),
            &[
                (&["pr", "view"], pull),
                (&["api", "repos/acme/widgets/pulls/1/comments"], comments),
                (&["api", "repos/acme/widgets/issues/1/comments"], "[]"),
                (&["api", "repos/acme/widgets/pulls/1/reviews"], reviews),
                (&["api", "graphql"], threads),
                (
                    &["api", "repos/acme/widgets/commits/abc/check-runs"],
                    r#"{"check_runs": []}"#,
                ),
                (
                    &["api", "repos/acme/widgets/compare/main...abc"],
                    r#"{"behind_by": 0}"#,
                ),
            ],
        );
        let details = github
            .details("github.com/acme/widgets", 1, "me")
            .await
            .unwrap();
        let ids: Vec<&str> = details
            .comments
            .iter()
            .map(|c| c.forge_id.as_str())
            .collect();
        assert_eq!(
            ids,
            ["rc-12", "rv-8"],
            "the pending review's own comment, its reply and its verdict are dropped, \
             so is the odd pending-by-state review's own verdict; \
             the submitted review's comment and its own verdict stay"
        );
        assert!(
            details.has_pending_posts,
            "the pending set is still open for merge_fresh_details to read"
        );
        assert!(
            details.comments.iter().all(|c| c.thread_id != "RT_1"),
            "a thread whose only comments were pending does not appear: {:?}",
            details.comments
        );
    }
}
