#!/usr/bin/env python3
"""Time the forge test modules and the full nextest run on one revision.

Usage: run.py SERIES LABEL REP BASE. Run it from the worktree root. Before
each timed run, it builds the tests outside the timing. Each row names HEAD, BASE and the SHA-256 of every
file under the daemon test directory that differs between them, as the
run found it on disk, so a row proves which fake it timed. It runs the forge modules, then the full suite, once
each. Each run appends one JSON row to results.jsonl and keeps its JUnit
file and log under /tmp/fake-forge-latency/.
"""
import hashlib
import json
import re
import shutil
import subprocess
import sys
import time
import xml.etree.ElementTree as ET
from pathlib import Path

EVIDENCE = Path(__file__).resolve().parent
LOGS = Path('/tmp/fake-forge-latency')
JUNIT = Path('target/nextest/ci/junit.xml')
# Every module of the daemon `it` binary that calls `stub_forge_cli`.
MODULES = ('attention forge_host_limit forge_integration forge_rate_limit hosts '
           'issues kept_requests pull_request_babysit pull_request_detail_skip '
           'pull_request_keep_retry pull_request_lists pull_request_reviews '
           'pull_requests stub_lifetime tunnel webhooks workflow_pull_request '
           'workflow_steps').split()
FILTER = 'binary(it) & (' + ' | '.join(f'test(/^{m}::/)' for m in MODULES) + ')'
BASE = ['cargo', 'nextest', 'run', '--workspace', '--profile', 'ci',
        '--status-level', 'fail', '--final-status-level', 'fail']
STAGES = {'modules': BASE + ['-E', FILTER], 'full': BASE}
WARM = ['cargo', 'nextest', 'run', '--workspace', '--profile', 'ci', '--no-run']
SUMMARY = re.compile(r'Summary \[\s*([\d.]+)s\]')
TOP5 = (
    'pull_request_babysit::asking_keeps_another_authors_request_until_the_merge',
    'pull_request_babysit::asking_keeps_a_request_of_mine_by_hand_until_the_merge',
    'pull_request_babysit::asking_keeps_a_dependabot_request_until_the_merge',
    'pull_request_reviews::a_review_round_needs_an_ask_and_runs_on_the_current_head',
    'pull_request_reviews::a_reply_in_a_review_thread_starts_one_round',
)


def idle(seconds=2.0):
    """The idle share of all CPUs over `seconds`, from /proc/stat (Linux)."""
    def sample():
        with open('/proc/stat') as stat:
            fields = [int(x) for x in stat.readline().split()[1:]]
        return fields[3] + fields[4], sum(fields)
    idle0, total0 = sample()
    time.sleep(seconds)
    idle1, total1 = sample()
    return round((idle1 - idle0) / (total1 - total0), 3)


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
    """Build the tests outside the timing, until a build compiles nothing.

    The daemon build script reruns when any file of the worktree or the git
    index changes, results.jsonl too, and its own `git status` can rewrite
    a stale index, which needs one build more.
    """
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
    series, label, rep, base = sys.argv[1], sys.argv[2], int(sys.argv[3]), sys.argv[4]
    LOGS.mkdir(exist_ok=True)
    sha = git('rev-parse', 'HEAD')
    base = git('rev-parse', base)
    files = file_state(base)
    tree = ('base' if all(f['disk'] == f['base'] for f in files.values()) else
            'head' if all(f['disk'] == f['head'] for f in files.values()) else 'mixed')
    for stage, cmd in STAGES.items():
        name = f'{series}-{label}-{stage}-{rep}-{time.strftime("%H%M%S")}'
        warm()
        JUNIT.unlink(missing_ok=True)
        before_idle = idle()
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
        row = dict(series=series, label=label, stage=stage, rep=rep, sha=sha, base=base,
                   tree=tree, files=files, exit=code,
                   seconds=round(elapsed, 3),
                   runner_seconds=float(runner[1]) if runner else None, compiled=built,
                   idle_before=before_idle, tests=tests, failed=failed,
                   top5=top)
        with open(EVIDENCE / 'results.jsonl', 'a') as out:
            out.write(json.dumps(row) + '\n')
        print(name, code, f'{elapsed:.1f}s', runner[1] if runner else None, built, tests, failed)


if __name__ == '__main__':
    main()
