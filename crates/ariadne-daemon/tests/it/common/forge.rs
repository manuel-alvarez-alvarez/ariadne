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
    _dir: tempfile::TempDir,
}

/// One run of the stub, as it logged it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Invocation {
    pub program: String,
    pub args: Vec<String>,
}

impl StubForgeCli {
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
exec python3 \"$dir/forge-stub.py\" \"$dir\" \"$(basename \"$0\")\" \"$@\"\n";

const STUB: &str = r#"#!/usr/bin/env python3
import json, os, sys, time

dir, program, args = sys.argv[1], sys.argv[2], sys.argv[3:]
with open(os.path.join(dir, "forge-calls.jsonl"), "a") as f:
    f.write(json.dumps({"program": program, "args": args}) + "\n")
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
