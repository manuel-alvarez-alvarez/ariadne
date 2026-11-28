//! A scriptable stub of the forge CLI, `gh`, as test support.
//!
//! [`stub_forge_cli`] links one shared POSIX `sh` program under its name,
//! as [`super::acp`] links its stub agent: one shared launcher, so no test
//! waits on macOS to check a new executable. Each call stays a child
//! process of its own, but `sh` starts in about a millisecond where
//! python3 and its imports took about twenty, and the forge tests make
//! dozens of calls each.
//!
//! A test programs the stub with a JSON script, a list of entries, each an
//! argument prefix (`args`), a `stdout`, an optional `stderr` and an `exit`
//! code, an optional `program` that holds the entry to `gh`, and an
//! optional `wait_for`, a file the answer waits for, so a test holds a call
//! open. [`write_script`] compiles that list into a `sh` function the stub
//! sources on each call. The stub answers the first entry whose prefix
//! matches, and an invocation no entry matches exits 1. It records every
//! invocation, its name and its arguments, in a log the test reads.
//! [`super::HarnessBuilder::forge_cli`] points the daemon's `gh_bin` at it.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

/// The stub on disk: the name the daemon runs, and the files it reads
/// and reports through.
pub(crate) struct StubForgeCli {
    pub gh: String,
    script_file: PathBuf,
    /// One record per call, in the order the calls started.
    calls: PathBuf,
    /// One record per call, in the order the calls finished.
    finished: PathBuf,
    pid_file: PathBuf,
    fetches: Fetches,
    _dir: tempfile::TempDir,
}

/// Which completed forge operation [`StubForgeCli::fetches`] counts.
#[derive(Clone, Copy)]
enum Fetches {
    Repository,
    PullRequestView,
}

/// One run of the stub, as it logged it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Invocation {
    pub program: String,
    pub args: Vec<String>,
    /// What the call piped in with `--input -`, where it did.
    pub input: Option<String>,
}

impl StubForgeCli {
    pub(crate) fn set_parent_pid(&self, pid: u32) {
        std::fs::write(self._dir.path().join("forge-parent-pid"), pid.to_string()).unwrap();
    }
    /// Every invocation, in the order they ran.
    pub(crate) fn invocations(&self) -> Vec<Invocation> {
        records(&self.calls)
            .into_iter()
            .map(|fields| {
                let [has_input, input, program, args @ ..] = &fields[..] else {
                    panic!("a call record holds its input and its program: {fields:?}");
                };
                Invocation {
                    program: program.clone(),
                    args: args.to_vec(),
                    input: (has_input == "1").then(|| input.clone()),
                }
            })
            .collect()
    }

    /// Number of calls the stub has finished: answered from the script, or
    /// refused for want of an entry. A refused call ends the same way an
    /// answered one does, so a count of calls still running is exact
    /// whatever the script holds.
    pub(crate) fn completed(&self) -> usize {
        self.finished().len()
    }

    /// Every call the stub has finished, in the order each one finished: an
    /// answer from the script, or a refusal for want of an entry.
    ///
    /// Read from the finished log, not from [`Self::invocations`] taken in
    /// start order: `Cli::run`'s `kill_on_drop(true)` can kill a detail
    /// call a failing sibling's own `try_join!` abandoned (`forge::github::
    /// details::details`) before it ever finishes, which leaves that one
    /// call's start logged with no finish to match — forever, since a
    /// killed call never runs again. A count keyed off "the last call
    /// started is the only one that can still be running" turns that one
    /// stranded start into a permanent undercount; reading what finished,
    /// whichever call it was, does not.
    fn finished(&self) -> Vec<Invocation> {
        records(&self.finished)
            .into_iter()
            .map(|fields| {
                let [_refused, program, args @ ..] = &fields[..] else {
                    panic!("a finish record holds its program: {fields:?}");
                };
                Invocation {
                    program: program.clone(),
                    args: args.to_vec(),
                    input: None,
                }
            })
            .collect()
    }

    /// How many selected forge reads have completed. The default counts
    /// repository fetches by their final open-issues listing. A test can
    /// select `pr view` calls instead with [`Self::pull_request_views_as_fetches`].
    ///
    /// Wait for the default count before reprogramming the stub after a
    /// wake: a fetch still under way answers the rest of its calls from the
    /// new script, and what it then publishes is neither the old situation
    /// nor the new one.
    pub(crate) fn fetches(&self) -> usize {
        match self.fetches {
            Fetches::Repository => self.repository_fetches(),
            Fetches::PullRequestView => self.completed_pull_request_views(),
        }
    }

    /// Count completed `pr view` calls as fetches instead of repository
    /// fetches. The default keeps the repository-fetch count unchanged.
    pub(crate) fn pull_request_views_as_fetches(mut self) -> Self {
        self.fetches = Fetches::PullRequestView;
        self
    }

    fn repository_fetches(&self) -> usize {
        self.finished()
            .iter()
            .filter(|call| {
                call.args.first().is_some_and(|arg| arg == "api")
                    && call.args.get(1).is_some_and(|arg| arg.contains("/issues?"))
            })
            .count()
    }

    fn completed_pull_request_views(&self) -> usize {
        self.finished()
            .iter()
            .filter(|call| {
                call.args.first().is_some_and(|arg| arg == "pr")
                    && call.args.get(1).is_some_and(|arg| arg == "view")
            })
            .count()
    }

    /// Answer from `script` from now on.
    pub(crate) fn reprogram(&self, script: Value) {
        write_script(&self.script_file, &script);
    }

    /// The pid of the most recent stub process, once it has started.
    pub(crate) fn pid(&self) -> Option<u32> {
        std::fs::read_to_string(&self.pid_file)
            .ok()?
            .trim()
            .parse()
            .ok()
    }
}

/// One entry of the script: what `args` starts with, and the answer.
pub(crate) fn answer(args: &[&str], exit: i32, stdout: &str) -> Value {
    json!({"args": args, "exit": exit, "stdout": stdout})
}

/// The request a task opened at `url`, as `gh pr view --json` answers it:
/// open, by the integration login `me`, from `head` onto `main`, with no
/// rollup of its own (026): `checks` is seeded apart from this, through
/// [`check_states`].
pub(crate) fn opened_pull(url: &str, head: &str) -> Value {
    let number: i64 = url.rsplit('/').next().unwrap().parse().unwrap();
    json!({"number": number, "url": url, "title": "feat: ship it",
        "author": {"login": "me"}, "state": "OPEN", "isDraft": false,
        "headRefName": head, "headRefOid": "abc",
        "headRepository": {"url": "https://github.com/acme/widgets"},
        "baseRefName": "main", "reviewDecision": "",
        "createdAt": "2026-10-01T00:00:00Z"})
}

/// A script entry answering `Github::check_states`'s own aliased GraphQL
/// read of the combined check state of each `(number, state)` of `pairs`,
/// `state` GitHub's own combined enum
/// (https://docs.github.com/en/graphql/reference/enums#statuscheckrollupstate)
/// (026). Matched by `-f` rather than `--paginate`, the review threads
/// read's own flag (`Github::threads`), so a script that stubs both
/// answers each from an entry of its own.
pub(crate) fn check_states(host: &str, pairs: &[(i64, &str)]) -> Value {
    let mut repository = serde_json::Map::new();
    for &(number, state) in pairs {
        repository.insert(
            format!("pr{number}"),
            json!({"statusCheckRollup": {"state": state}}),
        );
    }
    let body = json!({"data": {"repository": repository}}).to_string();
    answer(&["api", "graphql", "--hostname", host, "-f"], 0, &body)
}

/// Write the stub into a directory of its own, answering from `script`.
pub(crate) fn stub_forge_cli(script: Value) -> StubForgeCli {
    let dir = tempfile::tempdir().unwrap();
    let script_file = dir.path().join("forge-script.sh");
    write_script(&script_file, &script);
    std::fs::write(
        dir.path().join("forge-parent-pid"),
        std::process::id().to_string(),
    )
    .unwrap();
    let calls = dir.path().join("forge-calls");
    let finished = dir.path().join("forge-finished");
    std::fs::create_dir(&calls).unwrap();
    std::fs::create_dir(&finished).unwrap();
    let launcher = super::shared_script(STUB);
    let link = |name: &str| {
        let bin = dir.path().join(name);
        std::os::unix::fs::symlink(&launcher, &bin).unwrap();
        bin.display().to_string()
    };
    StubForgeCli {
        gh: link("gh"),
        calls,
        finished,
        pid_file: dir.path().join("forge-pid"),
        fetches: Fetches::Repository,
        script_file,
        _dir: dir,
    }
}

/// Compile `script` into the `answer` function the stub sources, and put
/// it in place whole: a call that starts during a reprogram reads the old
/// script or the new one, never a part of each.
fn write_script(path: &Path, script: &Value) {
    let entries = script
        .as_array()
        .expect("a forge script is a list of entries");
    let mut sh = String::from("answer() {\n");
    for entry in entries {
        let prefix: Vec<&str> = entry
            .get("args")
            .map(|args| {
                args.as_array()
                    .expect("an entry's args are a list")
                    .iter()
                    .map(|arg| arg.as_str().expect("an entry's args are strings"))
                    .collect()
            })
            .unwrap_or_default();
        let mut test = vec![format!("[ $# -ge {} ]", prefix.len())];
        if let Some(program) = entry.get("program") {
            let program = program.as_str().expect("an entry's program is a string");
            test.push(format!("[ \"$program\" = {} ]", quote(program)));
        }
        for (i, arg) in prefix.iter().enumerate() {
            test.push(format!("[ \"${{{}}}\" = {} ]", i + 1, quote(arg)));
        }
        let text = |key: &str| {
            entry.get(key).map_or("", |value| {
                value
                    .as_str()
                    .unwrap_or_else(|| panic!("an entry's {key} is a string"))
            })
        };
        // `sys.exit` of the Python stub took the status modulo 256 too.
        let exit = entry.get("exit").map_or(0, |code| {
            code.as_i64().expect("an entry's exit is a number") & 0xff
        });
        sh.push_str(&format!(
            "    if {}; then\n        stdout={}\n        stderr={}\n        wait_for={}\n        code={exit}\n        return 0\n    fi\n",
            test.join(" && "),
            quote(text("stdout")),
            quote(text("stderr")),
            quote(text("wait_for")),
        ));
    }
    sh.push_str("    return 1\n}\n");
    let partial = path.with_extension("partial");
    std::fs::write(&partial, sh).unwrap();
    std::fs::rename(&partial, path).unwrap();
}

/// `text` as one single-quoted `sh` word.
fn quote(text: &str) -> String {
    assert!(!text.contains('\0'), "sh cannot hold a NUL: {text:?}");
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// The records the stub wrote into `dir`, in slot order: each slot file
/// holds its field count, then its fields, each ended by a NUL, so an
/// argument may hold any other byte. A slot that is not whole is skipped:
/// its call is still writing it, or `Cli::run`'s `kill_on_drop(true)` killed
/// the call between the claim and the write, and then it stays empty.
fn records(dir: &Path) -> Vec<Vec<String>> {
    let mut records = Vec::new();
    for slot in 1.. {
        let Ok(bytes) = std::fs::read(dir.join(slot.to_string())) else {
            break;
        };
        let text = String::from_utf8_lossy(&bytes);
        let mut fields: Vec<String> = text.split('\0').map(str::to_string).collect();
        // What follows the last NUL: empty once the record is whole.
        if fields.pop().is_none_or(|rest| !rest.is_empty()) {
            continue;
        }
        let Some(count) = fields.first().and_then(|count| count.parse::<usize>().ok()) else {
            continue;
        };
        if fields.len() != count + 1 {
            continue;
        }
        fields.remove(0);
        records.push(fields);
    }
    records
}

/// The stub the daemon runs, under the name of the link. `$0` is the link,
/// not the shared file, so the directory of `$0` holds the test's files.
///
/// It records the call, then answers from `forge-script.sh` read at that
/// moment, then records the finish. A held call polls its gate every 20 ms
/// and ends once the process it names as its parent has ended. A read of
/// `--input -` runs in the background beside a watch that does the same
/// check, so a call whose input stays open still ends with its parent.
/// `$PPID` is fixed at start, and `kill -0` still succeeds for an unreaped
/// zombie, so the check also reads the current parent with `ps -o ppid=`,
/// which Linux and macOS both have. Each record takes the next free slot
/// file of its log, created with `set -C` (`O_EXCL`), so concurrent calls
/// never share one.
const STUB: &str = r#"#!/bin/sh
dir=${0%/*}
program=${0##*/}
IFS= read -r parent < "$dir/forge-parent-pid"
[ "$PPID" = "$parent" ] && kill -0 "$parent" 2>/dev/null || exit 0
alive() {
    kill -0 "$parent" 2>/dev/null || return 1
    set -- $(ps -o ppid= -p $$)
    [ "$1" = "$parent" ]
}
slots() {
    set -- "$1"/*
    if [ -e "$1" ]; then slot=$(($# + 1)); else slot=1; fi
}
record() {
    log=$1
    shift
    slots "$log"
    set -C
    while ! { printf '%s\0' "$#" "$@" > "$log/$slot"; } 2>/dev/null; do
        [ -e "$log/$slot" ] || exit 1
        slot=$((slot + 1))
    done
    set +C
}
echo $$ > "$dir/forge-pid"
has_input=0
input=
after=
for arg; do
    if [ -n "$after" ]; then
        [ "$arg" = - ] && has_input=1
        break
    fi
    [ "$arg" = --input ] && after=1
done
if [ $has_input = 1 ]; then
    # Read in the background, so a watch can end the read with the parent.
    # A background list reads /dev/null unless its input is named, so
    # name the call's own input through fd 3.
    exec 3<&0
    cat <&3 > "$dir/forge-input.$$" &
    reader=$!
    exec 3<&-
    (
        polls=0
        while sleep 0.02 && kill -0 "$reader" 2>/dev/null; do
            polls=$((polls + 1))
            [ $((polls % 10)) -ne 0 ] || alive || kill "$reader" $$ 2>/dev/null
        done
    ) > /dev/null 2>&1 &
    watch=$!
    wait "$reader"
    kill "$watch" 2>/dev/null
    input=$(cat "$dir/forge-input.$$"; printf .)
    input=${input%.}
    rm -f "$dir/forge-input.$$"
fi
record "$dir/forge-calls" "$has_input" "$input" "$program" "$@"
. "$dir/forge-script.sh"
if answer "$@"; then
    polls=0
    while [ -n "$wait_for" ] && [ ! -e "$wait_for" ]; do
        polls=$((polls + 1))
        [ $((polls % 10)) -ne 0 ] || alive || exit 0
        sleep 0.02
    done
    printf '%s' "$stdout"
    printf '%s' "$stderr" >&2
    record "$dir/forge-finished" 0 "$program" "$@"
    exit "$code"
fi
printf 'stub %s: no entry for %s\n' "$program" "$*" >&2
record "$dir/forge-finished" 1 "$program" "$@"
exit 1
"#;

/// A request Ariadne works on, as a fetch would leave its read in memory
/// (026): open on github.com/acme/widgets, by `author`, from `head` onto
/// `main` at `head_sha`, with nothing on it to tell. What a test that writes
/// the row itself seeds, since no fetch read it.
pub(crate) fn seed_live(
    h: &super::Harness,
    row: &ariadne_store::PullRequestRow,
    author: &str,
    head: &str,
    head_sha: &str,
    review_requested: bool,
) {
    h.launcher.live.set(
        &row.id,
        ariadne_daemon::forge::live::Live::new(
            ariadne_daemon::forge::pulls::ForgePullRequest {
                number: row.number,
                url: row.url.clone(),
                title: format!("Fix widgets {}", row.number),
                body: String::new(),
                author_login: author.into(),
                state: "open".into(),
                draft: false,
                head_branch: head.into(),
                head_sha: head_sha.into(),
                head_repo: None,
                base_branch: "main".into(),
                checks: "none".into(),
                review_decision: "none".into(),
                mergeable: "clean".into(),
                opened_at: "2026-10-01T00:00:00Z".into(),
                updated_at: "2026-10-01T00:00:00Z".into(),
                merge_sha: None,
            },
            review_requested,
            Some(ariadne_daemon::forge::live::Details::default()),
            true,
        ),
    );
}

/// An enabled GitHub integration for `repository_id`: what a readiness
/// item's own integration-enabled check needs to pass at all, and what a
/// test proving the disabled case flips off afterwards.
pub(crate) async fn enable_integration(h: &super::Harness, repository_id: &str, login: &str) {
    h.store
        .set_forge_integration(ariadne_store::SetForgeIntegration {
            repository_id: repository_id.into(),
            kind: ariadne_core::ForgeKind::Github,
            host: "github.com".into(),
            owner: "acme".into(),
            name: "widgets".into(),
            remote: "origin".into(),
            enabled: true,
            login: Some(login.into()),
            webhook_enabled: true,
            poll_interval_minutes: None,
        })
        .await
        .unwrap();
}

/// What `seed_review_evidence` holds for one scripted read: the forge's own
/// evidence a readiness item reads against the babysitting task's own
/// `ready` claim, every field named rather than defaulted, so a test states
/// exactly which piece of evidence it is proving withholds the item.
pub(crate) struct ReviewEvidence<'a> {
    pub checks: &'a str,
    pub review_decision: &'a str,
    pub mergeable: &'a str,
    /// The request's own current head.
    pub head_sha: &'a str,
    /// The head the comment evidence below was actually read at; `None`
    /// keeps it the same as `head_sha` (comments current for this head).
    /// `Some` of a different head simulates a failed detail refresh that
    /// left an older read's own comments standing while `head_sha` itself
    /// already moved (`forge::live::LivePulls::set_pull`).
    pub details_head_sha: Option<&'a str>,
    pub comments: Vec<ariadne_store::NewPullRequestComment>,
    /// Whether the last attempt to refresh the comment evidence actually
    /// succeeded (`Live::evidence_ok`); `false` simulates a failed detail
    /// fetch at the same head, which a head comparison alone cannot catch.
    pub evidence_ok: bool,
}

/// A review comment nobody has answered or resolved: `unanswered_comments`
/// counts it, and so does the stricter resolved-thread check.
pub(crate) fn open_review_comment() -> ariadne_store::NewPullRequestComment {
    ariadne_store::NewPullRequestComment {
        forge_id: "rc-1".into(),
        thread_id: "T1".into(),
        kind: "review_comment".into(),
        author_login: "someone".into(),
        author_is_bot: false,
        body: "What about this case?".into(),
        path: Some("src/lib.rs".into()),
        line: Some(1),
        in_reply_to: None,
        created_at: "2026-10-01T00:00:00Z".into(),
        resolved: false,
        from_review: false,
    }
}

/// A genuine human question on the conversation thread, from neither the
/// login nor a review: no later reply of the login's own side answers it,
/// so it stays a real, open ask a readiness item must still block on, the
/// same thread a review's own summary also posts to.
pub(crate) fn open_conversation_comment() -> ariadne_store::NewPullRequestComment {
    ariadne_store::NewPullRequestComment {
        forge_id: "ic-1".into(),
        thread_id: "conversation".into(),
        kind: "issue_comment".into(),
        author_login: "someone".into(),
        author_is_bot: false,
        body: "Why does this change the retry budget too?".into(),
        path: None,
        line: None,
        in_reply_to: None,
        created_at: "2026-10-01T00:00:00Z".into(),
        resolved: false,
        from_review: false,
    }
}

/// A review thread's opening finding, replied to by the integration
/// login's own side — which `waiting_threads` reads as "answered", zeroing
/// `unanswered_comments` — but never marked resolved on the forge: the
/// trap `forge/live.rs::waiting_threads` leaves standing, which readiness
/// must read through to the thread's own `resolved` flag instead,
/// whoever opened the thread. `from_ariadne` picks which: an Ariadne
/// review's own finding (`from_review: true`, posted under `login`) or a
/// human reviewer's own finding (`from_review: false`, posted under
/// `"someone"`) — a readiness item must block on either the same way.
pub(crate) fn answered_but_unresolved_review_comment(
    login: &str,
    from_ariadne: bool,
) -> Vec<ariadne_store::NewPullRequestComment> {
    let finding_author = if from_ariadne { login } else { "someone" };
    vec![
        ariadne_store::NewPullRequestComment {
            forge_id: "rc-1".into(),
            thread_id: "T1".into(),
            kind: "review_comment".into(),
            author_login: finding_author.into(),
            author_is_bot: false,
            body: "[P1] Missing a test".into(),
            path: Some("src/lib.rs".into()),
            line: Some(1),
            in_reply_to: None,
            created_at: "2026-10-01T00:00:00Z".into(),
            resolved: false,
            from_review: from_ariadne,
        },
        ariadne_store::NewPullRequestComment {
            forge_id: "rc-2".into(),
            thread_id: "T1".into(),
            kind: "review_comment".into(),
            author_login: login.into(),
            author_is_bot: false,
            body: "Added it.".into(),
            path: Some("src/lib.rs".into()),
            line: Some(1),
            in_reply_to: Some("rc-1".into()),
            created_at: "2026-10-02T00:00:00Z".into(),
            resolved: false,
            from_review: false,
        },
    ]
}

/// A completed Ariadne review exactly as `submit_review` leaves it, with
/// nobody ever answering it: one summary comment — posted as a plain
/// `issue_comment`, `from_review: true`, and never itself marked `resolved`
/// on the forge, since it names no diff-anchored thread at all, and never
/// answered either, since a summary carries nothing to answer — beside one
/// finding whose thread the forge *does* show resolved. The babysitting
/// skill never replies to a comment that asks for no change (`SKILL.md`),
/// so no fixture here may add a reply on the skill's behalf just to settle
/// the summary: a readiness item must read the summary itself as closing
/// nothing open, the same way it reads a resolved finding's thread.
pub(crate) fn completed_review_with_a_resolved_finding(
    login: &str,
) -> Vec<ariadne_store::NewPullRequestComment> {
    vec![
        ariadne_store::NewPullRequestComment {
            forge_id: "ic-301".into(),
            thread_id: "conversation".into(),
            kind: "issue_comment".into(),
            author_login: login.into(),
            author_is_bot: false,
            body: "No findings: ready for a human to approve".into(),
            path: None,
            line: None,
            in_reply_to: None,
            created_at: "2026-10-01T00:00:00Z".into(),
            resolved: false,
            from_review: true,
        },
        ariadne_store::NewPullRequestComment {
            forge_id: "rc-1".into(),
            thread_id: "T1".into(),
            kind: "review_comment".into(),
            author_login: login.into(),
            author_is_bot: false,
            body: "[P1] Missing a test".into(),
            path: Some("src/lib.rs".into()),
            line: Some(1),
            in_reply_to: None,
            created_at: "2026-10-01T00:00:00Z".into(),
            resolved: true,
            from_review: true,
        },
    ]
}

pub(crate) fn seed_review_evidence(
    h: &super::Harness,
    row: &ariadne_store::PullRequestRow,
    evidence: ReviewEvidence,
) {
    h.launcher.live.set(
        &row.id,
        ariadne_daemon::forge::live::Live::new(
            ariadne_daemon::forge::pulls::ForgePullRequest {
                number: row.number,
                url: row.url.clone(),
                title: format!("Fix widgets {}", row.number),
                body: String::new(),
                author_login: "me".into(),
                state: "open".into(),
                draft: false,
                head_branch: "fix".into(),
                head_sha: evidence.head_sha.into(),
                head_repo: None,
                base_branch: "main".into(),
                checks: evidence.checks.into(),
                review_decision: evidence.review_decision.into(),
                mergeable: evidence.mergeable.into(),
                opened_at: "2026-10-01T00:00:00Z".into(),
                updated_at: "2026-10-01T00:00:00Z".into(),
                merge_sha: None,
            },
            false,
            Some(ariadne_daemon::forge::live::Details {
                comments: evidence.comments,
                head_sha: evidence
                    .details_head_sha
                    .unwrap_or(evidence.head_sha)
                    .into(),
                ..Default::default()
            }),
            evidence.evidence_ok,
        ),
    );
}
