//! Which forge a published task is being reviewed on, and the vocabulary
//! every watcher of one speaks.
//!
//! A task its integrator published is not the daemon's to reconcile: the
//! branch is on a forge, humans review it there and one of them merges it.
//! Watching it is done through that forge's own CLI — [`crate::gh`] for a
//! pull request on GitHub, [`crate::glab`] for a merge request on GitLab —
//! and the only thing that says which one is the URL the integrator recorded.
//! That dispatch is [`forge_of`], and around it is what the two watchers have
//! in common: what one poll of either says has to happen next ([`PrState`]),
//! what a human wrote on either that the engineer has not been told about yet
//! ([`Feedback`]), what either says about the branch itself — whether it
//! still merges and whether its checks pass ([`Health`]) — and what either
//! says became of the branch ([`Landing`]).
//!
//! A URL on neither forge is recorded on the task like any other and simply
//! never polled — the integrator that publishes to one brings its own way of
//! watching it.

use ariadne_store::Task;

/// The hosted GitHub. GitHub Enterprise is on hosts with no tell to
/// recognize it by, and is not watched.
pub const GITHUB_HOST: &str = "github.com";

/// The hosted GitLab. A self-hosted one is recognized by the shape of its
/// URLs instead — see [`forge_of`].
pub const GITLAB_HOST: &str = "gitlab.com";

/// The path segment only GitLab builds, and the reason a self-hosted one on
/// any host at all can still be watched.
const GITLAB_MR_PATH: &str = "/-/merge_requests/";

/// A forge a task can be published into for humans to review.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Forge {
    GitHub,
    GitLab,
}

impl Forge {
    /// The forge's own name, for what the daemon writes to a person.
    pub fn name(self) -> &'static str {
        match self {
            Self::GitHub => "GitHub",
            Self::GitLab => "GitLab",
        }
    }

    /// What it calls the thing being reviewed.
    pub fn noun(self) -> &'static str {
        match self {
            Self::GitHub => "pull request",
            Self::GitLab => "merge request",
        }
    }

    /// How it refers to one by number: `#12` on GitHub, `!12` on GitLab.
    pub fn reference(self, number: i64) -> String {
        match self {
            Self::GitHub => format!("#{number}"),
            Self::GitLab => format!("!{number}"),
        }
    }
}

/// The forge `url` is on, if this knows how to watch it.
///
/// The host answers for the two hosted forges. A self-hosted GitLab is on a
/// host that shares its name with nothing — `git.example.com` — so what
/// answers for it is the URL's own shape: `/-/merge_requests/<n>` is a path
/// only GitLab builds, and `gitlab.<domain>` is what most of the rest are
/// called. GitHub Enterprise has no such tell — its pull request URLs are
/// shaped exactly like a Gitea's — so it is left unwatched rather than
/// guessed at.
pub fn forge_of(url: &str) -> Option<Forge> {
    let host = host_of(url)?;
    let host = host.strip_prefix("www.").unwrap_or(&host);
    if host == GITHUB_HOST {
        return Some(Forge::GitHub);
    }
    if host == GITLAB_HOST || host.starts_with("gitlab.") || url.trim().contains(GITLAB_MR_PATH) {
        return Some(Forge::GitLab);
    }
    None
}

/// The host of an `https://host/…`, `http://host/…` or `ssh://host/…` URL.
pub fn host_of(url: &str) -> Option<String> {
    let rest = url.trim().split_once("://").map(|(_, rest)| rest)?;
    let host = rest.split(['/', '?', '#']).next()?;
    // Credentials and a port are not part of the host we compare.
    let host = host.rsplit_once('@').map_or(host, |(_, after)| after);
    let host = host.split_once(':').map_or(host, |(before, _)| before);
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

/// The number of the pull or merge request `url` names, if it names one: the
/// last path segment of a `…/pull/<n>` or `…/merge_requests/<n>` URL,
/// whatever forge it is on.
pub fn pull_request_number(url: &str) -> Option<i64> {
    let path = url.trim().trim_end_matches('/');
    let (before, number) = path.rsplit_once('/')?;
    if !before.ends_with("/pull")
        && !before.ends_with("/pulls")
        && !before.ends_with("/merge_requests")
    {
        return None;
    }
    number.parse().ok()
}

/// One review the daemon is watching: the forge it is on, the number
/// everything calls it by, and the URL it was recorded as — which names the
/// repository too, and so can never be ambiguous in a checkout with several
/// remotes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchedPr {
    pub forge: Forge,
    pub number: i64,
    pub url: String,
}

impl WatchedPr {
    /// How the daemon names it at the start of a sentence: "Pull request
    /// #12", "Merge request !3".
    pub fn label(&self) -> String {
        let noun = self.forge.noun();
        let (first, rest) = noun.split_at(1);
        format!(
            "{}{rest} {}",
            first.to_ascii_uppercase(),
            self.forge.reference(self.number)
        )
    }
}

/// The review the daemon may watch for this task: one its integrator
/// recorded, on a forge this knows how to read. `None` for every task landed
/// locally, and for one published anywhere else.
pub fn watched_pull_request(task: &Task) -> Option<WatchedPr> {
    let url = task.pr_url.as_deref()?;
    let forge = forge_of(url)?;
    let number = task.pr_number.or_else(|| pull_request_number(url))?;
    Some(WatchedPr {
        forge,
        number,
        url: url.to_string(),
    })
}

/// Something a human wrote on the review that the engineer has not been told
/// about yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Feedback {
    /// The id it is remembered by, so it is relayed exactly once.
    pub id: String,
    pub author: String,
    pub body: String,
    /// Where on the diff it hangs — `src/board.rs:42`, or the file alone
    /// where the forge names no line. `None` for a comment written on the
    /// conversation rather than on the code, which is the difference the
    /// engineer reads it by.
    pub file: Option<String>,
    /// Whether it came with a verdict that blocks the merge: a
    /// `CHANGES_REQUESTED` review on GitHub, an unresolved thread on GitLab.
    pub blocking: bool,
}

/// One check the forge reports as failed on the branch: a GitHub check run
/// or commit status, a GitLab pipeline.
///
/// A red branch is the engineer's, so it travels the way a comment does —
/// named, placed, and remembered by an id so that polling it again is not a
/// second round of the same failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailedCheck {
    /// The id it is remembered by. It carries the commit the check ran on, so
    /// that the same failure is relayed once and a failure on the revision
    /// that was supposed to fix it is relayed again.
    pub id: String,
    /// What the forge calls it: the check run's name, the status context, the
    /// pipeline.
    pub name: String,
    /// The verdict the forge spells it with — `FAILURE`, `failed` — for the
    /// engineer to recognize it by on the request itself.
    pub conclusion: String,
    /// Where to read it, when the forge answered with somewhere.
    pub url: Option<String>,
}

/// The branch and the base no longer merge, as the forge reports it.
///
/// Only the engineer can reconcile that: the integrator hits it during its
/// own merge, and one asleep — waiting on the humans reading the request —
/// would never hit it at all, so the poll is what notices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    /// The id it is remembered by, carrying the commit it was read on: a
    /// conflict the engineer answered with a merge is a new head, and a
    /// conflict that comes back on it is news again.
    pub id: String,
    /// The branch it conflicts with, as the forge names it. Neither forge
    /// names the conflicting files on the request itself, so the base branch
    /// is what the engineer is given to merge in and reconcile against.
    pub base: Option<String>,
}

/// What the forge says about the branch itself, beside what people wrote on
/// it: whether it still merges into its base, and how its checks stand.
///
/// Every field is what the forge answered, not what a poll has already
/// relayed: an approval is held back for a red branch however long ago its
/// failure reached the engineer.
///
/// The three answers a check can give are kept apart, because they are asked
/// for two different things. Failing is somebody's to fix, and goes to the
/// engineer. Still running is nobody's — there is nothing to fix and nothing
/// to say — but it is not a request anybody can merge either, so it holds the
/// notice back and the next poll asks again. Passing, or nothing to ask about
/// at all, is what lets the approval through.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Health {
    /// The conflict with the base, where the forge reports one. `None` is
    /// "no conflict this poll could see" — a mergeability the forge answered
    /// as unknown, or did not answer at all, is not a conflict.
    pub conflict: Option<Conflict>,
    /// The checks the forge reports as failed. Empty is "nothing failing this
    /// poll could see": a check still running, cancelled or reported with a
    /// verdict this does not know is not a failure.
    pub failed_checks: Vec<FailedCheck>,
    /// Whether the forge is still working a check out: one queued or in
    /// progress on GitHub, a pipeline created, pending, running or waiting for
    /// a runner on GitLab. Not a failure, and nobody is told about it — it is
    /// simply not an answer yet.
    pub checks_pending: bool,
}

impl Health {
    /// Whether nothing at all stands between the request and its merge: no
    /// conflict, no failing check, and none still to come.
    ///
    /// This is what an approval is announced over, and the reason a check
    /// that has not finished counts: "approved and ready for you to merge" is
    /// a thing to send a person to, and a request whose build is still
    /// running is not ready however approved it is. Nothing is said about the
    /// wait — the next poll either finds it green and says so, or finds it red
    /// and sends it to the engineer.
    pub fn is_ready(&self) -> bool {
        self.conflict.is_none() && self.failed_checks.is_empty() && !self.checks_pending
    }
}

/// What one poll of a pull or merge request says has to happen next.
///
/// In the order they are read: a merged review is finished with whatever else
/// it also says, feedback comes before everything unmerged because relaying it
/// is what moves the task, a branch that does not merge or does not build is
/// the engineer's before it is anybody's, and an approval is announced once —
/// and only over a branch none of that is true of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrState {
    /// Nothing anybody has to be woken for.
    Quiet,
    /// Merged on the forge: the integrator finishes the task locally.
    Merged,
    /// Comments and change requests nobody has relayed yet.
    Feedback(Vec<Feedback>),
    /// The branch no longer merges into its base, and nobody has been told.
    Conflicting(Conflict),
    /// Checks the forge reports as failed that nobody has relayed yet.
    ChecksFailed(Vec<FailedCheck>),
    /// Approved, green, conflict-free, and waiting for a human to press the
    /// button.
    Approved,
}

/// Read one poll, out of everything either forge answers with.
///
/// `approved_notified` is whether the user has already been told this one is
/// ready to merge, and `feedback` is only what has not reached the engineer
/// (see [`unrelayed`]) — both come off the task, so a daemon that restarts
/// mid-review picks up where it left off rather than repeating itself.
/// `health` is what the forge said about the branch and `relayed` the ids
/// already handed over, which is the same bookkeeping the comments use: a
/// failure is announced once, and holds the approval back for as long as it
/// lasts whether or not it was announced this poll. A check the forge has not
/// finished holds it back too, without being announced to anybody (see
/// [`Health`]).
///
/// A conflict is read before the checks it is likely to have caused: GitLab
/// runs the pipeline on the merge result, and telling an engineer its build
/// is red when what is red is a merge that no longer applies sends it after
/// the wrong thing.
pub fn poll_state(
    merged: bool,
    feedback: Vec<Feedback>,
    health: &Health,
    relayed: &[String],
    approved: bool,
    approved_notified: bool,
) -> PrState {
    if merged {
        return PrState::Merged;
    }
    if !feedback.is_empty() {
        return PrState::Feedback(feedback);
    }
    if let Some(conflict) = health
        .conflict
        .as_ref()
        .filter(|c| !relayed.contains(&c.id))
    {
        return PrState::Conflicting(conflict.clone());
    }
    let failed: Vec<FailedCheck> = health
        .failed_checks
        .iter()
        .filter(|c| !relayed.contains(&c.id))
        .cloned()
        .collect();
    if !failed.is_empty() {
        return PrState::ChecksFailed(failed);
    }
    if approved && !approved_notified && health.is_ready() {
        return PrState::Approved;
    }
    PrState::Quiet
}

/// What of `all` has not reached the engineer: everything with something
/// written in it that `relayed` does not already name.
pub fn unrelayed(all: impl IntoIterator<Item = Feedback>, relayed: &[String]) -> Vec<Feedback> {
    all.into_iter()
        .filter(|f| !f.id.is_empty() && !f.body.trim().is_empty())
        .filter(|f| !relayed.contains(&f.id))
        .collect()
}

/// The id a failing check is remembered by: the commit it ran on and the name
/// the forge calls it, which is the pair that says whether this is the same
/// failure as last poll's.
///
/// The commit is what makes a fix relayable: an engineer that answered a red
/// build pushes, the head moves, and the build that fails on the new head is
/// news the way the first one was. A forge that answered with no head at all
/// leaves the name to key it, which is one relay for as long as the failure
/// lasts — the poll after the fix is a fresh read either way.
pub fn check_id(head: Option<&str>, name: &str) -> String {
    format!("CHK{}:{name}", head.unwrap_or_default())
}

/// The same for the conflict with the base, which is one per commit: the
/// engineer answers it by merging the base in, and that is a new head.
pub fn conflict_id(head: Option<&str>) -> String {
    format!("MRG{}", head.unwrap_or_default())
}

/// The author of a comment, or the placeholder for one the forge answered
/// without a name for.
pub fn author_or_someone(name: Option<&str>) -> String {
    name.filter(|n| !n.is_empty())
        .map_or_else(|| "someone".to_string(), str::to_string)
}

/// Where a comment on the diff hangs, as [`Feedback::file`] carries it:
/// `src/board.rs:42`, or the file alone where the forge gave no line — a
/// comment whose lines have moved out from under it still names the file the
/// engineer has to open.
pub fn location(path: Option<&str>, line: Option<i64>) -> Option<String> {
    let path = path.map(str::trim).filter(|p| !p.is_empty())?;
    Some(match line.filter(|l| *l > 0) {
        Some(line) => format!("{path}:{line}"),
        None => path.to_string(),
    })
}

/// Everything a `--paginate` wrote, however the CLI chose to write it.
///
/// Pagination has two shapes and only one of them is a JSON document. `gh`
/// merges the pages of an array endpoint into a single array today (2.97,
/// measured: seventeen pages of one came back as one array), but what it
/// documents is the other shape — "each page is a separate JSON array or
/// object" — and `glab` writes exactly that, one array per page. A review
/// people have really been through is where the difference shows, so the
/// answer is read as a stream of values and flattened, which is right either
/// way, and empty output is nothing said rather than a parse failure.
pub fn parse_pages<T: serde::de::DeserializeOwned>(raw: &str) -> anyhow::Result<Vec<T>> {
    let mut out = Vec::new();
    for page in serde_json::Deserializer::from_str(raw).into_iter::<Vec<T>>() {
        out.extend(page?);
    }
    Ok(out)
}

/// What a forge says became of the branch, as the merge verification reads it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Landing {
    /// The state the forge spells it with, for what a refusal quotes back.
    pub state: String,
    pub merged: bool,
    /// The commits the merge landed as — a squash or a rebase writes one that
    /// is on no branch of ours until the base is fetched.
    pub commits: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole of the dispatch: which CLI a recorded URL is polled with.
    #[test]
    fn a_url_says_which_forge_watches_it() {
        for github in [
            "https://github.com/owner/repo/pull/12",
            "HTTPS://GitHub.com/owner/repo/pull/12",
            "https://www.github.com/owner/repo/pull/12",
            "https://github.com/owner/repo/pull/12/files",
        ] {
            assert_eq!(forge_of(github), Some(Forge::GitHub), "{github}");
        }
        for gitlab in [
            "https://gitlab.com/owner/repo/-/merge_requests/3",
            "https://GitLab.com/group/sub/project/-/merge_requests/3",
            // Self-hosted, by the name most of them are given...
            "https://gitlab.example.com/group/project/-/merge_requests/3",
            // ...and by the path only GitLab builds, on any host at all.
            "https://git.example.com/group/project/-/merge_requests/3",
            "https://code.acme.internal:8443/g/p/-/merge_requests/17",
        ] {
            assert_eq!(forge_of(gitlab), Some(Forge::GitLab), "{gitlab}");
        }
        // GitHub Enterprise's URLs are shaped like everybody else's, so a
        // host this does not know is left unwatched rather than guessed at.
        for unwatched in [
            "https://github.example.com/owner/repo/pull/12",
            "https://bitbucket.org/owner/repo/pull-requests/5",
            "https://codeberg.org/owner/repo/pulls/7",
            "not a url at all",
            "",
        ] {
            assert_eq!(forge_of(unwatched), None, "{unwatched}");
        }
    }

    /// And what the daemon watches for a task: the forge, the number and the
    /// URL together, or nothing at all.
    #[test]
    fn a_task_is_watched_on_the_forge_its_url_names() {
        let task = |url: Option<&str>, number: Option<i64>| Task {
            id: "t".into(),
            goal_id: "g".into(),
            repo_id: "r".into(),
            title: "t".into(),
            description: "d".into(),
            status: "integrating".into(),
            engineer_profile_id: "e".into(),
            integrator_profile_id: ariadne_store::defaults::INTEGRATOR_ID.into(),
            agent_kind: None,
            model: None,
            branch: "b".into(),
            worktree_path: None,
            review_round: 1,
            stalled: 0,
            merge_commit: None,
            pr_number: number,
            pr_url: url.map(str::to_string),
            pr_relayed_comments: None,
            pr_approved_notified: 0,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
        };

        assert_eq!(
            watched_pull_request(&task(
                Some("https://github.com/owner/repo/pull/12"),
                Some(12)
            )),
            Some(WatchedPr {
                forge: Forge::GitHub,
                number: 12,
                url: "https://github.com/owner/repo/pull/12".into(),
            })
        );
        // The number is read off the URL when the task never recorded one.
        assert_eq!(
            watched_pull_request(&task(
                Some("https://gitlab.com/group/sub/project/-/merge_requests/3"),
                None
            )),
            Some(WatchedPr {
                forge: Forge::GitLab,
                number: 3,
                url: "https://gitlab.com/group/sub/project/-/merge_requests/3".into(),
            })
        );
        // A task landed locally, and one published where this cannot look.
        assert_eq!(watched_pull_request(&task(None, None)), None);
        assert_eq!(
            watched_pull_request(&task(
                Some("https://bitbucket.org/owner/repo/pull-requests/5"),
                Some(5)
            )),
            None
        );
    }

    #[test]
    fn the_number_a_review_url_names() {
        assert_eq!(
            pull_request_number("https://github.com/owner/repo/pull/12"),
            Some(12)
        );
        assert_eq!(
            pull_request_number("https://gitlab.com/g/p/-/merge_requests/3/"),
            Some(3)
        );
        assert_eq!(pull_request_number("https://gitlab.com/g/p"), None);
        assert_eq!(
            pull_request_number("https://gitlab.com/g/p/-/merge_requests/three"),
            None
        );
    }

    /// Each forge is named the way its own users name it: the daemon writes
    /// these into what a person reads.
    #[test]
    fn each_forge_names_a_review_its_own_way() {
        let github = WatchedPr {
            forge: Forge::GitHub,
            number: 12,
            url: "https://github.com/owner/repo/pull/12".into(),
        };
        assert_eq!(github.label(), "Pull request #12");
        assert_eq!(github.forge.name(), "GitHub");

        let gitlab = WatchedPr {
            forge: Forge::GitLab,
            number: 3,
            url: "https://gitlab.com/g/p/-/merge_requests/3".into(),
        };
        assert_eq!(gitlab.label(), "Merge request !3");
        assert_eq!(gitlab.forge.noun(), "merge request");
    }

    /// The reading both watchers share, in the order it reads them.
    #[test]
    fn a_poll_is_read_merged_then_feedback_then_approved() {
        let comment = || {
            vec![Feedback {
                id: "C1".into(),
                author: "maria".into(),
                body: "why?".into(),
                file: None,
                blocking: false,
            }]
        };
        let green = Health::default();
        assert_eq!(
            poll_state(true, comment(), &green, &[], true, false),
            PrState::Merged
        );
        assert_eq!(
            poll_state(false, comment(), &green, &[], true, false),
            PrState::Feedback(comment())
        );
        assert_eq!(
            poll_state(false, vec![], &green, &[], true, false),
            PrState::Approved
        );
        assert_eq!(
            poll_state(false, vec![], &green, &[], true, true),
            PrState::Quiet
        );
        assert_eq!(
            poll_state(false, vec![], &green, &[], false, false),
            PrState::Quiet
        );
    }

    /// The whole order, in one table: merged over everything, then what people
    /// wrote, then the branch not merging, then the branch not building, then
    /// the approval — which is only ever read over a branch none of the rest
    /// is true of.
    #[test]
    fn a_poll_is_read_in_one_order_whatever_else_is_true_of_it() {
        let comment = || {
            vec![Feedback {
                id: "C1".into(),
                author: "maria".into(),
                body: "why?".into(),
                file: None,
                blocking: false,
            }]
        };
        let conflict = || Conflict {
            id: "MRGabc".into(),
            base: Some("main".into()),
        };
        let red = || {
            vec![FailedCheck {
                id: "CHKabc:build".into(),
                name: "build".into(),
                conclusion: "FAILURE".into(),
                url: Some("https://ci.example/1".into()),
            }]
        };
        let health = |conflict: Option<Conflict>, failed_checks: Vec<FailedCheck>| Health {
            conflict,
            failed_checks,
            checks_pending: false,
        };
        let everything = health(Some(conflict()), red());

        // Every case with everything true of it at once, most-read first.
        assert_eq!(
            poll_state(true, comment(), &everything, &[], true, false),
            PrState::Merged
        );
        assert_eq!(
            poll_state(false, comment(), &everything, &[], true, false),
            PrState::Feedback(comment())
        );
        assert_eq!(
            poll_state(false, vec![], &everything, &[], true, false),
            PrState::Conflicting(conflict()),
            "a conflict is read before the pipeline it is likely to have failed"
        );
        assert_eq!(
            poll_state(false, vec![], &health(None, red()), &[], true, false),
            PrState::ChecksFailed(red())
        );
        assert_eq!(
            poll_state(false, vec![], &health(None, vec![]), &[], true, false),
            PrState::Approved
        );

        // Relayed once, and never again — the ids are what say so, exactly as
        // they do for a comment.
        assert_eq!(
            poll_state(false, vec![], &everything, &["MRGabc".into()], false, false),
            PrState::ChecksFailed(red()),
            "the conflict was handed over; the failing check has not been"
        );
        assert_eq!(
            poll_state(
                false,
                vec![],
                &everything,
                &["MRGabc".into(), "CHKabc:build".into()],
                false,
                false
            ),
            PrState::Quiet
        );

        // But an approval is never announced over either of them, however
        // long ago they were relayed: what the user would be told is that a
        // request nobody can merge is theirs to merge.
        assert_eq!(
            poll_state(
                false,
                vec![],
                &everything,
                &["MRGabc".into(), "CHKabc:build".into()],
                true,
                false
            ),
            PrState::Quiet
        );
        assert_eq!(
            poll_state(
                false,
                vec![],
                &health(None, red()),
                &["CHKabc:build".into()],
                true,
                false
            ),
            PrState::Quiet
        );
        assert!(!everything.is_ready());
        assert!(!health(None, red()).is_ready());
        assert!(!health(Some(conflict()), vec![]).is_ready());
        assert!(Health::default().is_ready());
    }

    /// And the third thing a check can be, which is neither: still running.
    ///
    /// Nobody is sent back for it — there is nothing to fix — and nobody is
    /// told the request is theirs to merge either, because it is not: the
    /// poll after it either finds the branch green and says so, or finds it
    /// red and hands it over.
    #[test]
    fn a_check_that_has_not_finished_is_nobodys_and_holds_the_notice_back() {
        let pending = Health {
            checks_pending: true,
            ..Default::default()
        };
        assert!(!pending.is_ready());
        assert_eq!(
            poll_state(false, vec![], &pending, &[], true, false),
            PrState::Quiet,
            "a build still running is not a request to send anybody to"
        );
        // Nothing about it is relayable, so nothing about it is remembered:
        // the same poll on the same request says the same thing next time.
        assert_eq!(
            poll_state(
                false,
                vec![],
                &pending,
                &["CHKabc:build".into()],
                true,
                false
            ),
            PrState::Quiet
        );
        // And once it has finished green, the approval goes out.
        assert_eq!(
            poll_state(false, vec![], &Health::default(), &[], true, false),
            PrState::Approved
        );
        // A failure alongside it is still the engineer's: what has not
        // finished says nothing about what has.
        let mut red_and_running = pending.clone();
        red_and_running.failed_checks = vec![FailedCheck {
            id: "CHKabc123:build".into(),
            name: "build".into(),
            conclusion: "FAILURE".into(),
            url: None,
        }];
        assert!(matches!(
            poll_state(false, vec![], &red_and_running, &[], true, false),
            PrState::ChecksFailed(_)
        ));
    }

    /// What a check and a conflict are remembered by: the head they were read
    /// on, so the same failure is one relay and the failure on the revision
    /// that answered it is another.
    #[test]
    fn a_failure_is_keyed_by_the_commit_it_happened_on() {
        assert_eq!(check_id(Some("abc123"), "build"), "CHKabc123:build");
        assert_ne!(
            check_id(Some("abc123"), "build"),
            check_id(Some("def456"), "build"),
            "the revision that was supposed to fix it is a new failure"
        );
        assert_eq!(check_id(None, "build"), "CHK:build");
        assert_eq!(conflict_id(Some("abc123")), "MRGabc123");
        assert_ne!(conflict_id(Some("abc123")), conflict_id(Some("def456")));
        assert_eq!(conflict_id(None), "MRG");
    }

    /// Where a comment on the diff hangs, as the engineer is told it: the
    /// line where the forge still knows one, the file alone where the diff
    /// has moved out from under the comment, and nothing at all for what was
    /// written on the conversation rather than on the code.
    #[test]
    fn a_comment_on_the_diff_is_placed_by_its_file_and_line() {
        assert_eq!(
            location(Some("src/board.rs"), Some(42)),
            Some("src/board.rs:42".to_string())
        );
        assert_eq!(
            location(Some("src/board.rs"), None),
            Some("src/board.rs".to_string())
        );
        assert_eq!(
            location(Some("src/board.rs"), Some(0)),
            Some("src/board.rs".to_string()),
            "a line the forge answered with a zero for is no line"
        );
        assert_eq!(location(None, Some(42)), None);
        assert_eq!(location(Some("  "), Some(42)), None);
    }

    /// And the filter that keeps a comment to one relay: an empty one is not
    /// feedback, and neither is one already handed over.
    #[test]
    fn only_unrelayed_comments_with_something_in_them_are_feedback() {
        let all = vec![
            Feedback {
                id: "C1".into(),
                author: "maria".into(),
                body: "why?".into(),
                file: None,
                blocking: false,
            },
            Feedback {
                id: "C2".into(),
                author: "jon".into(),
                body: "   ".into(),
                file: None,
                blocking: true,
            },
            Feedback {
                id: String::new(),
                author: "nobody".into(),
                body: "unidentified".into(),
                file: None,
                blocking: false,
            },
        ];
        let ids = |f: &[Feedback]| f.iter().map(|f| f.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(&unrelayed(all.clone(), &[])), vec!["C1".to_string()]);
        assert!(unrelayed(all, &["C1".into()]).is_empty());

        assert_eq!(author_or_someone(Some("maria")), "maria");
        assert_eq!(author_or_someone(Some("")), "someone");
        assert_eq!(author_or_someone(None), "someone");
    }
}
