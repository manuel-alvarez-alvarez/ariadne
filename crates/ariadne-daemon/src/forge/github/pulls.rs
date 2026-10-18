//! Pull requests through the GitHub CLI.
use std::collections::HashMap;

use super::Github;
use crate::forge::pulls::{ForgePullRequest, split_slug};
use serde::Deserialize;

const FIELDS: &str = "number,url,title,body,author,state,isDraft,headRefName,headRefOid,headRepository,headRepositoryOwner,baseRefName,reviewDecision,mergeStateStatus,createdAt,updatedAt,mergeCommit,reviewRequests";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Pull {
    number: i64,
    url: String,
    title: String,
    #[serde(default)]
    body: Option<String>,
    author: Login,
    state: String,
    is_draft: bool,
    head_ref_name: String,
    head_ref_oid: String,
    head_repository: Option<Repository>,
    head_repository_owner: Option<Login>,
    base_ref_name: String,
    /// The request's combined check state, read apart from this `--json`
    /// read by [`Github::check_states`] and merged in after: GitHub's own
    /// `statusCheckRollup` field, taken through `--json`, always carries
    /// every check's own context too, which grows with the request's own
    /// check count and is what overruns a large rollup with a GraphQL
    /// request too large to answer (026).
    #[serde(skip)]
    check_state: Option<String>,
    review_decision: Option<String>,
    /// Absent on an older read: unknown mergeability stands for it.
    #[serde(default)]
    merge_state_status: Option<String>,
    created_at: String,
    /// Absent from an older read: the opening time stands for it.
    #[serde(default)]
    updated_at: Option<String>,
    /// The commit a merged request landed as.
    #[serde(default)]
    merge_commit: Option<Commit>,
    /// Absent on an older read: nobody asked stands for it.
    #[serde(default)]
    review_requests: Vec<ReviewRequest>,
}
#[derive(Deserialize)]
struct Commit {
    oid: String,
}
#[derive(Deserialize)]
struct Login {
    login: String,
}
/// One entry of `reviewRequests`: a user names a `login`; a team names a
/// `slug` or a `name` and no `login`, and is read as asking nobody.
#[derive(Deserialize)]
struct ReviewRequest {
    #[serde(default)]
    login: Option<String>,
}
#[derive(Deserialize)]
struct Repository {
    name: Option<String>,
    url: Option<String>,
}

impl Pull {
    /// Whether `login` is among this request's requested reviewers. A team
    /// request names no login of its own, so it never matches one.
    fn requests_review(&self, login: &str) -> bool {
        self.review_requests.iter().any(|r| {
            r.login
                .as_deref()
                .is_some_and(|l| l.eq_ignore_ascii_case(login))
        })
    }
    fn normalized(self) -> ForgePullRequest {
        // GitHub's own combined enum
        // (https://docs.github.com/en/graphql/reference/enums#statuscheckrollupstate):
        // `EXPECTED` and `PENDING` are both still running, `ERROR` and
        // `FAILURE` both end the rollup failed, and a request with no
        // rollup at all — nothing has ever run on its head — is `none`.
        let check_state = match self.check_state.as_deref() {
            Some("EXPECTED" | "PENDING") => "pending",
            Some("ERROR" | "FAILURE") => "failure",
            Some("SUCCESS") => "success",
            _ => "none",
        };
        let head_repo = self.head_repository.and_then(|repo| {
            repo.url
                .map(|url| format!("{}.git", url.trim_end_matches(".git").trim_end_matches('/')))
                .or_else(|| {
                    let host = reqwest::Url::parse(&self.url).ok()?.host_str()?.to_owned();
                    Some(format!(
                        "https://{host}/{}/{}.git",
                        self.head_repository_owner?.login, repo.name?
                    ))
                })
        });
        ForgePullRequest {
            number: self.number,
            url: self.url,
            title: self.title,
            body: self.body.unwrap_or_default(),
            author_login: self.author.login,
            state: self.state.to_lowercase(),
            draft: self.is_draft,
            head_branch: self.head_ref_name,
            head_sha: self.head_ref_oid,
            head_repo,
            base_branch: self.base_ref_name,
            checks: check_state.into(),
            review_decision: match self.review_decision.as_deref() {
                Some("APPROVED") => "approved",
                Some("CHANGES_REQUESTED") => "changes_requested",
                Some("REVIEW_REQUIRED") => "review_required",
                _ => "none",
            }
            .into(),
            // GitHub's own `mergeStateStatus`
            // (https://docs.github.com/en/graphql/reference/enums#mergestatestatus):
            // `CLEAN` is mergeable outright, and so, by GitHub's own
            // documented meaning, are `HAS_HOOKS` ("mergeable with passing
            // commit status and pre-receive hooks") and `UNSTABLE`
            // ("mergeable with non-passing commit status") — `checks` is
            // this producer's own gate on commit status passing, so
            // reading `UNSTABLE` as `clean` here never lets a non-passing
            // check through on its own. `UNKNOWN` is the forge still
            // computing it, never read as clean. `BEHIND`, `BLOCKED`,
            // `DIRTY` and `DRAFT` block a merge right now, each for its
            // own reason.
            mergeable: match self.merge_state_status.as_deref() {
                Some("CLEAN" | "HAS_HOOKS" | "UNSTABLE") => "clean",
                Some("UNKNOWN") | None => "unknown",
                Some(_) => "blocked",
            }
            .into(),
            updated_at: self.updated_at.unwrap_or_else(|| self.created_at.clone()),
            opened_at: self.created_at,
            merge_sha: self
                .merge_commit
                .filter(|_| self.state == "MERGED")
                .map(|commit| commit.oid),
        }
    }
}
impl Github {
    async fn rows(&self, repo: &str, filter: &[&str]) -> Result<Vec<Pull>, String> {
        // Increase the limit until the complete result fits. `gh pr list` has
        // no page flag; its limit drives the client's GraphQL pagination.
        let mut limit = 100;
        let mut rows = loop {
            let size = limit.to_string();
            let mut args = vec![
                "pr", "list", "--repo", repo, "--state", "open", "--json", FIELDS, "--limit", &size,
            ];
            args.extend_from_slice(filter);
            let output = self.cli.answer(&args).await?;
            let rows: Vec<Pull> = serde_json::from_str(&output)
                .map_err(|e| format!("cannot read GitHub pull requests: {e}"))?;
            if rows.len() < limit {
                break rows;
            }
            limit *= 2;
        };
        let numbers: Vec<i64> = rows.iter().map(|p| p.number).collect();
        let states = self.check_states(repo, &numbers).await?;
        for row in &mut rows {
            row.check_state = states.get(&row.number).cloned().flatten();
        }
        Ok(rows)
    }

    /// The combined check state of every one of `numbers`, by number, read
    /// apart from the request's own fields: one `pullRequest(number: …)`
    /// alias per number, so the one call answers every one of them, and
    /// each asks GitHub's own combined enum alone
    /// (https://docs.github.com/en/graphql/reference/enums#statuscheckrollupstate),
    /// never the context list behind it, which grows with the request's
    /// own check count and is what overruns a large rollup with a GraphQL
    /// request too large to answer (026). `None` of a number stands for no
    /// rollup at all: nothing has ever run on that head.
    async fn check_states(
        &self,
        repo: &str,
        numbers: &[i64],
    ) -> Result<HashMap<i64, Option<String>>, String> {
        if numbers.is_empty() {
            return Ok(HashMap::new());
        }
        let (host, owner, name) = split_slug(repo)?;
        let query = check_states_query(numbers);
        let output = self
            .cli
            .answer(&[
                "api",
                "graphql",
                "--hostname",
                host,
                "-f",
                &format!("query={query}"),
                "-F",
                &format!("owner={owner}"),
                "-F",
                &format!("name={name}"),
            ])
            .await?;
        let value: serde_json::Value = serde_json::from_str(&output)
            .map_err(|e| format!("cannot read GitHub check states: {e}"))?;
        let repository = &value["data"]["repository"];
        Ok(numbers
            .iter()
            .map(|&n| {
                let state = repository[format!("pr{n}")]["statusCheckRollup"]["state"]
                    .as_str()
                    .map(str::to_owned);
                (n, state)
            })
            .collect())
    }
    async fn pulls(&self, repo: &str, filter: &[&str]) -> Result<Vec<ForgePullRequest>, String> {
        Ok(self
            .rows(repo, filter)
            .await?
            .into_iter()
            .map(Pull::normalized)
            .collect())
    }
    pub(crate) async fn list_open_pull_requests(
        &self,
        repo: &str,
        login: &str,
    ) -> Result<crate::forge::pulls::Listed, String> {
        let rows = self.rows(repo, &[]).await?;
        let requested = rows
            .iter()
            .filter(|p| p.requests_review(login))
            .map(|p| p.number)
            .collect();
        let open = rows.into_iter().map(Pull::normalized).collect();
        Ok(crate::forge::pulls::Listed { open, requested })
    }
    pub(crate) async fn search_pull_requests(
        &self,
        repo: &str,
        query: &str,
    ) -> Result<Vec<ForgePullRequest>, String> {
        let mut rows = self.pulls(repo, &["--search", query]).await?;
        // Forge free-text search does not match every author or request number.
        // Complete those matches from the repository's open requests.
        rows.extend(self.pulls(repo, &[]).await?);
        let query = query.trim().trim_start_matches('#').to_lowercase();
        rows.retain(|p| {
            p.title.to_lowercase().contains(&query)
                || p.author_login.to_lowercase().contains(&query)
                || p.number.to_string() == query
        });
        rows.sort_by_key(|p| p.number);
        rows.dedup_by_key(|p| p.number);
        Ok(rows)
    }
    pub(crate) async fn pull_request(
        &self,
        repo: &str,
        number: i64,
    ) -> Result<ForgePullRequest, String> {
        let output = self
            .cli
            .answer(&[
                "pr",
                "view",
                &number.to_string(),
                "--repo",
                repo,
                "--json",
                FIELDS,
            ])
            .await?;
        let mut pull: Pull = serde_json::from_str(&output)
            .map_err(|e| format!("cannot read GitHub pull request: {e}"))?;
        pull.check_state = self
            .check_states(repo, &[number])
            .await?
            .remove(&number)
            .flatten();
        Ok(pull.normalized())
    }
}

/// A GraphQL query asking the combined check state alone of one
/// `pullRequest(number: …)` per `numbers`, aliased by number so the one
/// response answers every one of them: `statusCheckRollup { state }`, never
/// its own context list (`Github::check_states`).
fn check_states_query(numbers: &[i64]) -> String {
    let aliases: String = numbers
        .iter()
        .map(|n| {
            format!("    pr{n}: pullRequest(number: {n}) {{ statusCheckRollup {{ state }} }}\n")
        })
        .collect();
    format!(
        "query($owner: String!, $name: String!) {{\n  repository(owner: $owner, name: $name) {{\n{aliases}  }}\n}}"
    )
}

#[cfg(test)]
mod tests {
    use super::Pull;
    use serde_json::json;

    /// A `gh pr list --json` read, exactly as `FIELDS` names it. `extra`
    /// overrides or adds fields on top of a minimal, otherwise valid read
    /// — `mergeStateStatus` above all: GitHub's own documented meaning
    /// for each of its values
    /// (https://docs.github.com/en/graphql/reference/enums#mergestatestatus)
    /// is what `mergeable` must read, not a guess from its name alone —
    /// `HAS_HOOKS` and `UNSTABLE` are both still mergeable by that
    /// documented meaning, and must normalize to `clean`, never `blocked`.
    fn provider_shaped(extra: serde_json::Value) -> Pull {
        let mut value = json!({
            "number": 1,
            "url": "https://github.com/acme/widgets/pull/1",
            "title": "Fix widgets",
            "author": {"login": "someone"},
            "state": "OPEN",
            "isDraft": false,
            "headRefName": "fix",
            "headRefOid": "abc",
            "headRepository": {"url": "https://github.com/acme/widgets"},
            "baseRefName": "main",
            "reviewDecision": "APPROVED",
            "mergeStateStatus": "CLEAN",
            "createdAt": "2026-10-01T00:00:00Z",
        });
        value
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().cloned().unwrap_or_default());
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn mergeable_reads_every_merge_state_status_by_its_documented_meaning() {
        for (status, expected) in [
            ("CLEAN", "clean"),
            ("HAS_HOOKS", "clean"),
            ("UNSTABLE", "clean"),
            ("UNKNOWN", "unknown"),
            ("BEHIND", "blocked"),
            ("BLOCKED", "blocked"),
            ("DIRTY", "blocked"),
            ("DRAFT", "blocked"),
        ] {
            let pull = provider_shaped(json!({"mergeStateStatus": status}));
            assert_eq!(Pull::normalized(pull).mergeable, expected, "{status}");
        }
    }

    /// A read from before this field existed names none at all: unknown
    /// mergeability stands for it, never a guessed `clean`.
    #[test]
    fn a_read_naming_no_merge_state_status_is_unknown() {
        let value = json!({
            "number": 1,
            "url": "https://github.com/acme/widgets/pull/1",
            "title": "Fix widgets",
            "author": {"login": "someone"},
            "state": "OPEN",
            "isDraft": false,
            "headRefName": "fix",
            "headRefOid": "abc",
            "headRepository": {"url": "https://github.com/acme/widgets"},
            "baseRefName": "main",
            "reviewDecision": "APPROVED",
            "createdAt": "2026-10-01T00:00:00Z",
        });
        let pull: Pull = serde_json::from_value(value).unwrap();
        assert_eq!(Pull::normalized(pull).mergeable, "unknown");
    }

    /// A user named among `reviewRequests` asks for that user's review.
    #[test]
    fn a_user_review_request_names_its_login() {
        let pull = provider_shaped(json!({"reviewRequests": [{"login": "me"}]}));
        assert!(pull.requests_review("me"));
        assert!(!pull.requests_review("someone-else"));
    }

    /// A team named among `reviewRequests` has a `slug` and no `login`, so
    /// it never matches a user's own login, whoever belongs to the team.
    #[test]
    fn a_team_review_request_matches_no_login() {
        let pull = provider_shaped(json!({"reviewRequests": [{"slug": "reviewers"}]}));
        assert!(!pull.requests_review("me"));
    }

    /// A read from before this field existed names no review request at
    /// all: nobody asked stands for it.
    #[test]
    fn a_read_naming_no_review_requests_asks_for_nobody() {
        let pull = provider_shaped(json!({}));
        assert!(!pull.requests_review("me"));
    }

    /// `checks` reads GitHub's own combined rollup state alone
    /// (https://docs.github.com/en/graphql/reference/enums#statuscheckrollupstate):
    /// `EXPECTED` and `PENDING` both still running, `ERROR` and `FAILURE`
    /// both a failed rollup, `SUCCESS` a passing one, and no rollup at all
    /// — nothing has ever run on the head — `none`.
    #[test]
    fn checks_reads_every_rollup_state_by_its_documented_meaning() {
        for (state, expected) in [
            (None, "none"),
            (Some("EXPECTED"), "pending"),
            (Some("PENDING"), "pending"),
            (Some("ERROR"), "failure"),
            (Some("FAILURE"), "failure"),
            (Some("SUCCESS"), "success"),
        ] {
            let mut pull = provider_shaped(json!({}));
            pull.check_state = state.map(str::to_owned);
            assert_eq!(Pull::normalized(pull).checks, expected, "{state:?}");
        }
    }

    /// The combined-state query selects `statusCheckRollup { state }`
    /// alone, one aliased `pullRequest` per number, never a check's own
    /// context list.
    #[test]
    fn check_states_query_selects_only_the_rollup_state() {
        let query = super::check_states_query(&[1, 42]);
        assert!(query.contains("pr1: pullRequest(number: 1) { statusCheckRollup { state } }"));
        assert!(query.contains("pr42: pullRequest(number: 42) { statusCheckRollup { state } }"));
        assert!(!query.contains("contexts"));
    }
}

#[cfg(test)]
mod check_state_fetch_tests {
    use std::os::unix::fs::PermissionsExt;
    use std::time::Duration;

    use serde_json::json;

    use super::Github;
    use crate::forge::Cli;

    const STUB: &str = r#"#!/usr/bin/env python3
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
        std::fs::write(&path, STUB).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        let answers: Vec<_> = answers
            .iter()
            .map(|(args, stdout)| json!({"args": args, "stdout": stdout}))
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

    /// A single-pull read asks `pr view` for no `statusCheckRollup`, then
    /// merges in the combined state a separate, aliased GraphQL call
    /// answers: the shape `gh pr view --json statusCheckRollup` would
    /// otherwise carry, every check's own context, never crosses the wire.
    #[tokio::test]
    async fn a_single_pull_read_merges_the_combined_state_from_its_own_graphql_call() {
        let dir = tempfile::tempdir().unwrap();
        let pull = r#"{"number":7,"url":"https://github.com/acme/widgets/pull/7",
            "title":"Fix widgets","author":{"login":"someone"},"state":"OPEN","isDraft":false,
            "headRefName":"fix","headRefOid":"abc",
            "headRepository":{"url":"https://github.com/acme/widgets"},
            "baseRefName":"main","reviewDecision":"",
            "createdAt":"2026-10-01T00:00:00Z"}"#;
        let rollup = json!({"data": {"repository": {
            "pr7": {"statusCheckRollup": {"state": "FAILURE"}},
        }}})
        .to_string();
        let github = github(
            dir.path(),
            &[(&["pr", "view"], pull), (&["api", "graphql"], &rollup)],
        );
        let read = github
            .pull_request("github.com/acme/widgets", 7)
            .await
            .unwrap();
        assert_eq!(read.checks, "failure");
    }
}
