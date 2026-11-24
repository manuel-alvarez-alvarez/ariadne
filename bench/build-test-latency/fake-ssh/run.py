#!/usr/bin/env python3
"""Time the fake ssh test modules and the full nextest run on one revision.

Usage: run.py LABEL REP BASE. Run it from the worktree root. Before each
timed run, it builds the tests outside the timing, until a build compiles
nothing. Each row names HEAD, BASE and the SHA-256 of every file under the
daemon test directory that differs between them, as the run found it on
disk, so a row proves which fake it timed. It runs the modules, then the
full suite, once each. Each run appends one JSON row to
/tmp/fake-ssh-latency/results.jsonl, outside the worktree, because the
daemon build script reruns when a file of the worktree changes. It keeps
the JUnit file and the log of each run beside it.
"""
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import time
import xml.etree.ElementTree as ET
from pathlib import Path

LOGS = Path('/tmp/fake-ssh-latency')
JUNIT = Path('target/nextest/ci/junit.xml')
# Every module of the daemon `it` binary that calls `stub_ssh`.
MODULES = 'hosts host_health host_reconnect ssh_forward stub_lifetime'.split()
FILTER = 'binary(it) & (' + ' | '.join(f'test(/^{m}::/)' for m in MODULES) + ')'
BASE = ['cargo', 'nextest', 'run', '--workspace', '--profile', 'ci', '--no-fail-fast',
        '--status-level', 'fail', '--final-status-level', 'fail']
STAGES = {'modules': BASE + ['-E', FILTER], 'full': BASE}
WARM = ['cargo', 'nextest', 'run', '--workspace', '--profile', 'ci', '--no-run']
SUMMARY = re.compile(r'Summary \[\s*([\d.]+)s\]')
TOP5 = (
    'hosts::selected_repositories_clone_only_committed_files_and_retry_registration',
    'hosts::an_update_upgrades_a_runtime_that_setup_installed_and_leaves_one_the_user_installed',
    'hosts::selected_credentials_cross_ssh_without_entering_progress_and_retry_skips_the_copy',
    'hosts::setup_reads_the_interactive_shell_path_first_and_falls_back_in_order',
    'hosts::repository_setup_copies_this_machines_host_key_to_the_worker',
)
IT = 'crates/ariadne-daemon/tests/it'


def git(*args):
    return subprocess.run(['git', *args], capture_output=True, text=True,
                          check=True).stdout.strip()


def file_state(base):
    """The SHA-256 of each test file that BASE and HEAD hold apart, as found
    on disk, and the blob that each revision holds."""
    state = {}
    for name in git('diff', '--name-only', base, 'HEAD', '--', IT).split():
        path = Path(name)
        disk = hashlib.sha256(path.read_bytes()).hexdigest() if path.exists() else None
        def blob(rev):
            shown = subprocess.run(['git', 'show', f'{rev}:{name}'], capture_output=True)
            return hashlib.sha256(shown.stdout).hexdigest() if shown.returncode == 0 else None
        state[name] = dict(disk=disk, base=blob(base), head=blob('HEAD'))
    return state


def warm():
    """Build the tests outside the timing, until a build compiles nothing."""
    for _ in range(3):
        build = subprocess.run(WARM, capture_output=True, text=True, timeout=580,
                               check=True)
        if 'Compiling ' not in build.stderr:
            return
    raise SystemExit('the test build does not settle')


def junit(path):
    tests, failed, top = 0, [], {}
    for case in ET.parse(path).getroot().iter('testcase'):
        tests += 1
        name = case.get('name')
        if case.find('failure') is not None or case.find('error') is not None:
            failed.append(f"{case.get('classname')} {name}")
        if name in TOP5:
            top[name] = float(case.get('time'))
    return tests, failed, top


def main():
    label, rep, base = sys.argv[1], int(sys.argv[2]), sys.argv[3]
    LOGS.mkdir(exist_ok=True)
    sha = git('rev-parse', 'HEAD')
    base = git('rev-parse', base)
    files = file_state(base)
    tree = ('base' if all(f['disk'] == f['base'] for f in files.values()) else
            'head' if all(f['disk'] == f['head'] for f in files.values()) else 'mixed')
    for stage, cmd in STAGES.items():
        name = f'{label}-{stage}-{rep}-{time.strftime("%H%M%S")}'
        warm()
        JUNIT.unlink(missing_ok=True)
        load = os.getloadavg()[0]
        with open(LOGS / f'{name}.log', 'w') as log:
            start = time.monotonic()
            code = subprocess.run(cmd, stdout=log, stderr=subprocess.STDOUT,
                                  timeout=580).returncode
            elapsed = time.monotonic() - start
        shutil.copy(JUNIT, LOGS / f'{name}.xml')
        log_text = (LOGS / f'{name}.log').read_text()
        runner = SUMMARY.search(log_text)
        built = 'Compiling ' in log_text
        tests, failed, top = junit(JUNIT)
        row = dict(label=label, stage=stage, rep=rep, sha=sha, base=base,
                   tree=tree, files=files, exit=code,
                   seconds=round(elapsed, 3),
                   runner_seconds=float(runner[1]) if runner else None, compiled=built,
                   load_before=round(load, 2), tests=tests, failed=failed,
                   top5=top, log=name)
        with open(LOGS / 'results.jsonl', 'a') as out:
            out.write(json.dumps(row) + '\n')
        print(name, code, f'{elapsed:.1f}s', runner[1] if runner else None, built, tests, failed)


if __name__ == '__main__':
    main()
