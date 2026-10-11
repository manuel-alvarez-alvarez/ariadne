//! A scriptable stub of the forge CLIs, `gh` and `glab`, as test support.
//!
//! [`stub_forge_cli`] writes one python3 program and links it under both
//! names, as [`super::acp`] links its stub agent: one shared launcher, so no
//! test waits on macOS to check a new executable. The program reads a JSON
//! script, a list of entries, each an argument prefix (`args`), a `stdout`,
//! an optional `stderr` and an `exit` code, an optional `program` that holds
//! the entry to `gh` or to `glab`, and an optional `wait_for`, a file the
//! answer waits for, so a test holds a call open. It answers the first entry whose
//! prefix matches, and an invocation no entry matches exits 1. It appends
//! every invocation, its name and its arguments, to a log the test reads.
//! [`super::HarnessBuilder::forge_cli`] points the daemon's `gh_bin` and
//! `glab_bin` at it.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

/// The stub on disk: the two names the daemon runs, and the files it reads
/// and reports through.
pub(crate) struct StubForgeCli {
    pub gh: String,
    pub glab: String,
    script_file: PathBuf,
    log: PathBuf,
    finished: PathBuf,
    pid_file: PathBuf,
    _dir: tempfile::TempDir,
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
        let Ok(log) = std::fs::read_to_string(&self.log) else {
            return Vec::new();
        };
        log.lines()
            .map(|line| {
                let call: Value = serde_json::from_str(line).unwrap();
                Invocation {
                    program: call["program"].as_str().unwrap().to_string(),
                    args: serde_json::from_value(call["args"].clone()).unwrap(),
                    input: call["input"].as_str().map(str::to_string),
                }
            })
            .collect()
    }

    /// Number of calls whose scripted answer has been written.
    pub(crate) fn completed(&self) -> usize {
        std::fs::read_to_string(&self.finished)
            .map(|s| s.lines().count())
            .unwrap_or(0)
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
/// open, by the integration login `me`, from `head` onto `main`.
pub(crate) fn opened_pull(url: &str, head: &str) -> Value {
    let number: i64 = url.rsplit('/').next().unwrap().parse().unwrap();
    json!({"number": number, "url": url, "title": "feat: ship it",
        "author": {"login": "me"}, "state": "OPEN", "isDraft": false,
        "headRefName": head, "headRefOid": "abc",
        "headRepository": {"url": "https://github.com/acme/widgets"},
        "baseRefName": "main", "statusCheckRollup": [], "reviewDecision": "",
        "createdAt": "2026-10-01T00:00:00Z"})
}

/// Write the stub into a directory of its own, answering from `script`.
pub(crate) fn stub_forge_cli(script: Value) -> StubForgeCli {
    let dir = tempfile::tempdir().unwrap();
    let script_file = dir.path().join("forge-script.json");
    write_script(&script_file, &script);
    std::fs::write(
        dir.path().join("forge-parent-pid"),
        std::process::id().to_string(),
    )
    .unwrap();
    std::fs::write(dir.path().join("forge-stub.py"), STUB).unwrap();
    let launcher = super::shared_script(LAUNCHER);
    let link = |name: &str| {
        let bin = dir.path().join(name);
        std::os::unix::fs::symlink(&launcher, &bin).unwrap();
        bin.display().to_string()
    };
    StubForgeCli {
        gh: link("gh"),
        glab: link("glab"),
        log: dir.path().join("forge-calls.jsonl"),
        finished: dir.path().join("forge-finished.jsonl"),
        pid_file: dir.path().join("forge-pid"),
        script_file,
        _dir: dir,
    }
}

fn write_script(path: &Path, script: &Value) {
    assert!(script.is_array(), "a forge script is a list of entries");
    std::fs::write(path, serde_json::to_string_pretty(script).unwrap()).unwrap();
}

/// What the daemon runs: the stub beside the link, told which name it was
/// run under. `$0` is the link, not the shared file.
const LAUNCHER: &str = "#!/bin/sh\n\
dir=$(dirname \"$0\")\n\
exec python3 \"$dir/forge-stub.py\" \"$(cat \"$dir/forge-parent-pid\")\" \"$dir\" \"$(basename \"$0\")\" \"$@\"\n";

const STUB: &str = r#"#!/usr/bin/env python3
import json, os, subprocess, sys, threading, time

parent = int(sys.argv[1])
def parent_alive():
    state = subprocess.run(["ps", "-o", "stat=", "-p", str(parent)],
                           capture_output=True, text=True).stdout.strip()
    return bool(state) and not state.startswith("Z")
def orphaned():
    # Ends with the test process, however that one ends.
    while parent_alive():
        time.sleep(0.2)
    os._exit(0)
threading.Thread(target=orphaned, daemon=True).start()

if not parent_alive():
    os._exit(0)
dir, program, args = sys.argv[2], sys.argv[3], sys.argv[4:]
with open(os.path.join(dir, "forge-pid"), "w") as f:
    f.write(str(os.getpid()))
call = {"program": program, "args": args}
if "--input" in args and args[args.index("--input") + 1:args.index("--input") + 2] == ["-"]:
    call["input"] = sys.stdin.read()
with open(os.path.join(dir, "forge-calls.jsonl"), "a") as f:
    f.write(json.dumps(call) + "\n")
script = json.load(open(os.path.join(dir, "forge-script.json")))
for entry in script:
    if entry.get("program", program) != program:
        continue
    prefix = entry.get("args", [])
    if args[:len(prefix)] == prefix:
        while entry.get("wait_for") and not os.path.exists(entry["wait_for"]):
            time.sleep(0.02)
        sys.stdout.write(entry.get("stdout", ""))
        sys.stderr.write(entry.get("stderr", ""))
        with open(os.path.join(dir, "forge-finished.jsonl"), "a") as f:
            f.write(json.dumps({"program": program, "args": args}) + "\n")
        sys.exit(entry.get("exit", 0))
sys.stderr.write("stub %s: no entry for %s\n" % (program, " ".join(args)))
sys.exit(1)
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
        ariadne_daemon::forge::live::Live {
            pull: ariadne_daemon::forge::pulls::ForgePullRequest {
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
            details: Some(ariadne_daemon::forge::live::Details::default()),
        },
    );
}

/// An enabled GitHub integration for `repository_id`, with no review pin:
/// what a readiness item's own integration-enabled check needs to pass at
/// all, and what a test proving the disabled case flips off afterwards.
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
            review_model: None,
            review_effort: None,
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

/// A review's own finding (`from_review: true`), replied to by the
/// integration login — which `waiting_threads` reads as "answered",
/// zeroing `unanswered_comments` — but never marked resolved on the forge:
/// the trap `forge/live.rs::waiting_threads` leaves standing, which
/// readiness must read through to the thread's own `resolved` flag
/// instead.
pub(crate) fn answered_but_unresolved_review_comment(
    login: &str,
) -> Vec<ariadne_store::NewPullRequestComment> {
    vec![
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
            resolved: false,
            from_review: true,
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

pub(crate) fn seed_review_evidence(
    h: &super::Harness,
    row: &ariadne_store::PullRequestRow,
    evidence: ReviewEvidence,
) {
    h.launcher.live.set(
        &row.id,
        ariadne_daemon::forge::live::Live {
            pull: ariadne_daemon::forge::pulls::ForgePullRequest {
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
            review_requested: false,
            details: Some(ariadne_daemon::forge::live::Details {
                comments: evidence.comments,
                head_sha: evidence
                    .details_head_sha
                    .unwrap_or(evidence.head_sha)
                    .into(),
                ..Default::default()
            }),
        },
    );
}
