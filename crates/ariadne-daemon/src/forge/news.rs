//! What a pull request's session has not been told yet (026).
//!
//! After a detail fetch stores what the forge holds, the news is the
//! difference between that and what the session was last told: comments by
//! another login nobody told it of, checks that turned to failure, checks
//! whose rolled-up state moved, a head that fell behind its base, a review
//! decision that changed, and a state that turned `merged` or `closed`. Each
//! change is told once (009 rule 4): a comment carries `told_at`, and the row
//! carries a told mark for the rest. The ACP driver writes both right before
//! the prompt goes out, and gives them back where it never did (018).
use ariadne_store::{PullRequest, PullRequestComment, PullRequestTold, Store};

use super::pulls::FailedCheck;

/// The news of one request: a line per item, and what to mark told once
/// the lines went out.
#[derive(Debug, Clone)]
pub struct News {
    pub lines: Vec<String>,
    /// The stored ids of the comments the lines name.
    pub comment_ids: Vec<String>,
    /// The row's told mark after this news.
    pub told: PullRequestTold,
    /// The row's told mark before it, which a prompt that never went out
    /// puts back.
    pub before: PullRequestTold,
}

impl News {
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// The mark to write with no prompt: where nothing is news but a check
    /// turned green again or the head caught up with its base, the mark
    /// drops them, so their next turn is news again.
    pub fn recovered(&self) -> Option<&PullRequestTold> {
        (self.is_empty() && self.told != self.before).then_some(&self.told)
    }
}

/// The news of `pull`, given the comments its session has not been told of.
pub fn of(pull: &PullRequest, untold: &[PullRequestComment]) -> News {
    let mut lines = Vec::new();
    for comment in untold {
        let what = match comment.kind.as_str() {
            "review" => "Review",
            _ => "Comment",
        };
        let place = match (&comment.path, comment.line) {
            (Some(path), Some(line)) => format!("on {path}:{line}"),
            (Some(path), None) => format!("on {path}"),
            _ => "on the conversation".to_string(),
        };
        lines.push(format!(
            "- {what} {} by {} {place}: {}",
            comment.id,
            comment.author_login,
            first_line(&comment.body)
        ));
    }
    let failed: Vec<FailedCheck> = serde_json::from_str(&pull.failed_checks).unwrap_or_default();
    let told_checks: Vec<String> = serde_json::from_str(&pull.told_checks).unwrap_or_default();
    for check in failed.iter().filter(|c| !told_checks.contains(&c.name)) {
        let at = match check.url.is_empty() {
            true => String::new(),
            false => format!(": {}", check.url),
        };
        lines.push(format!(
            "- Check {} turned to {}{at}",
            check.name, check.conclusion
        ));
    }
    if pull.behind_base && !pull.told_behind_base {
        lines.push(format!("- The head is behind {}.", pull.base_branch));
    }
    // What a ready report is decided on: approvals and the rolled-up checks.
    // Checks that finished green after an approval, or went back to pending
    // after a ready report, are news, since the session cannot look itself.
    let check_state = pull.told_check_state.as_deref().unwrap_or("none");
    if pull.checks != check_state {
        lines.push(format!("- The checks now read {}.", pull.checks));
    }
    let decision = pull.told_review_decision.as_deref().unwrap_or("none");
    if pull.review_decision != decision {
        lines.push(format!(
            "- The review decision is now {}.",
            pull.review_decision
        ));
    }
    let state = pull.told_state.as_deref().unwrap_or("open");
    if pull.state != state {
        lines.push(format!("- The request is now {}.", pull.state));
    }
    News {
        lines,
        comment_ids: untold.iter().map(|c| c.id.clone()).collect(),
        told: PullRequestTold {
            // A check that is green again is dropped, so a later failure of
            // it is news again; so is a head that caught up with its base.
            checks: failed.into_iter().map(|c| c.name).collect(),
            behind_base: pull.behind_base,
            review_decision: pull.review_decision.clone(),
            state: pull.state.clone(),
            check_state: pull.checks.clone(),
        },
        before: PullRequestTold {
            checks: told_checks,
            behind_base: pull.told_behind_base,
            review_decision: decision.to_string(),
            state: state.to_string(),
            check_state: check_state.to_string(),
        },
    }
}

/// The news of the request `pull`, read off the store: `login` is the
/// integration's, whose own comments are no news.
pub async fn untold(store: &Store, pull: &PullRequest, login: &str) -> ariadne_store::Result<News> {
    let untold = store.untold_pull_request_comments(&pull.id, login).await?;
    Ok(of(pull, &untold))
}

/// The first line of a comment, cut short: the session reads the whole of
/// it with `list_comments`.
fn first_line(body: &str) -> String {
    const MOST: usize = 200;
    let line = body
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    match line.chars().count() > MOST {
        true => format!("{}…", line.chars().take(MOST).collect::<String>()),
        false => line.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pull() -> PullRequest {
        PullRequest {
            id: "01pr".into(),
            repository_id: "01repo".into(),
            number: 7,
            url: "https://github.com/acme/widgets/pull/7".into(),
            title: "Fix widgets".into(),
            author_login: "me".into(),
            tracked_by: "forge".into(),
            state: "open".into(),
            draft: false,
            head_branch: "fix".into(),
            head_sha: "abc".into(),
            head_repo: None,
            base_branch: "main".into(),
            checks: "none".into(),
            review_decision: "none".into(),
            unanswered_comments: 0,
            origin_task_id: None,
            opened_at: String::new(),
            role: "author".into(),
            ready: false,
            last_seen_at: String::new(),
            created_at: String::new(),
            updated_at: String::new(),
            failed_checks: "[]".into(),
            behind_base: false,
            told_checks: "[]".into(),
            told_behind_base: false,
            told_review_decision: None,
            told_state: None,
            told_check_state: None,
            news_told_at: None,
            cleaned_at: None,
        }
    }

    /// The row as it stands once `news` was told.
    fn told(pull: &PullRequest, news: &News) -> PullRequest {
        PullRequest {
            told_checks: serde_json::to_string(&news.told.checks).unwrap(),
            told_behind_base: news.told.behind_base,
            told_review_decision: Some(news.told.review_decision.clone()),
            told_state: Some(news.told.state.clone()),
            told_check_state: Some(news.told.check_state.clone()),
            ..pull.clone()
        }
    }

    /// A fresh row with nothing on it is no news: the baseline is a request
    /// open, with no review decision, every check green and level with its
    /// base.
    #[test]
    fn a_quiet_request_is_no_news() {
        assert!(of(&pull(), &[]).is_empty());
    }

    /// Each kind of change is told once: what the news marks told is
    /// what the next news leaves out, and a check that recovers is news
    /// again when it fails again.
    #[test]
    fn each_change_is_told_once() {
        let changed = PullRequest {
            failed_checks: r#"[{"name":"lint","url":"https://ci/1","conclusion":"failure"}]"#
                .into(),
            behind_base: true,
            review_decision: "changes_requested".into(),
            state: "merged".into(),
            ..pull()
        };
        let news = of(&changed, &[]);
        assert_eq!(
            news.lines,
            [
                "- Check lint turned to failure: https://ci/1",
                "- The head is behind main.",
                "- The review decision is now changes_requested.",
                "- The request is now merged.",
            ]
        );
        let mut changed = told(&changed, &news);
        assert!(of(&changed, &[]).is_empty(), "nothing is told twice");

        // A check that turns green and a head that catches up are no news,
        // but they leave the mark, so their next turn is news again.
        changed.failed_checks = "[]".into();
        changed.behind_base = false;
        let green = of(&changed, &[]);
        assert!(green.is_empty());
        let recovered = green.recovered().expect("a mark to persist");
        assert!(recovered.checks.is_empty());
        assert!(!recovered.behind_base);
        let changed = PullRequest {
            told_checks: "[]".into(),
            told_behind_base: false,
            failed_checks: r#"[{"name":"lint","url":"","conclusion":"failure"}]"#.into(),
            behind_base: true,
            ..changed
        };
        assert_eq!(
            of(&changed, &[]).lines,
            [
                "- Check lint turned to failure",
                "- The head is behind main."
            ],
            "a check that fails again and a head behind again are news again"
        );
    }

    /// An approval that lands while checks still run is told, and so is the
    /// checks finishing green after it: that is when the session reports the
    /// request ready. Checks that go back to pending after it are told too,
    /// so a ready report is taken back.
    #[test]
    fn a_change_of_the_checks_is_told_for_the_ready_report() {
        let approved = PullRequest {
            review_decision: "approved".into(),
            checks: "pending".into(),
            ..pull()
        };
        let news = of(&approved, &[]);
        assert_eq!(
            news.lines,
            [
                "- The checks now read pending.",
                "- The review decision is now approved.",
            ]
        );
        let green = PullRequest {
            checks: "success".into(),
            ..told(&approved, &news)
        };
        let news = of(&green, &[]);
        assert_eq!(news.lines, ["- The checks now read success."]);
        let pending = PullRequest {
            checks: "pending".into(),
            ..told(&green, &news)
        };
        assert_eq!(of(&pending, &[]).lines, ["- The checks now read pending."]);
    }
}
