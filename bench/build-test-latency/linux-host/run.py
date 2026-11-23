#!/usr/bin/env python3
"""Bounded timing driver for the host-malvarez Linux latency measurement.

Each call runs one stage under one artifact condition in private target,
Cargo home and npm cache directories. It appends one JSON row to the
evidence file and never touches a shared cache.
"""
import argparse
import json
import os
from pathlib import Path
import resource
import shutil
import signal
import subprocess
import sys
import time

WORKTREE = Path(__file__).resolve().parents[3]
EVIDENCE = Path(__file__).resolve().parent
ROOT = WORKTREE / 'target' / 'latency-host'
LOGS = Path('/tmp/r9rs18-latency-logs')
MARKER = 'R9RS18_LATENCY'
LIMIT = 560

RUST = {
    'bins': ('.', 'cargo build --locked --workspace --bins'),
    'testbuild': ('.', 'cargo nextest run --workspace --profile ci --no-run'),
    'nextest': ('.', 'cargo nextest run --workspace --profile ci '
                     '--status-level fail --final-status-level fail'),
    'doctest': ('.', 'cargo test --doc --workspace'),
    'clippy': ('.', 'cargo clippy --workspace --all-targets -- -D warnings'),
    'tauri': ('ui/src-tauri', 'cargo check'),
}
# A target directory per family: nextest runs on the testbuild target.
FAMILY = {'bins': 'bins', 'testbuild': 'tests', 'nextest': 'tests',
          'doctest': 'doc', 'clippy': 'clippy', 'tauri': 'tauri'}
FRONTEND = {
    'desktop-ci': ('.', 'npm ci'),
    'dashboard-ci': ('dashboard', 'npm ci'),
    'desktop-test': ('.', 'npm test'),
    'dashboard-test': ('dashboard', 'npm test'),
    'build': ('.', 'npm run build'),
}


def environment(stage, rep):
    keep = ('HOME', 'USER', 'LOGNAME', 'SHELL', 'LANG')
    env = {k: os.environ[k] for k in keep if k in os.environ}
    home = os.environ['HOME']
    node = str(Path(shutil.which('node')).parent)
    env.update(PATH=f'{home}/.cargo/bin:{node}:/usr/local/bin:/usr/bin:/bin',
               CARGO_HOME=str(ROOT / 'cargo-home'),
               npm_config_cache=str(ROOT / 'npm-cache'),
               CARGO_TERM_COLOR='never', TERM='dumb', NO_COLOR='1')
    env[MARKER] = f'{stage}-{rep}'
    if stage in RUST:
        env['CARGO_TARGET_DIR'] = str(ROOT / f'target-{FAMILY[stage]}-{rep}')
        env['CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER'] = str(ROOT / 'linkbin' / 'cc')
        env['LATENCY_LINK_LOG'] = str(ROOT / 'links' / f'{stage}-{rep}-{time.time_ns()}')
        Path(env['LATENCY_LINK_LOG']).mkdir(parents=True)
    else:
        # The vitest configurations write JUnit only when CI is set.
        env['CI'] = 'true'
    return env


def background():
    ps = subprocess.run(['ps', '-eo', 'pid,user,pcpu,pmem,etimes,comm', '--sort=-pcpu'],
                        capture_output=True, text=True, timeout=30).stdout.splitlines()
    mem = dict(line.split(':', 1) for line in Path('/proc/meminfo').read_text().splitlines())
    return dict(load=os.getloadavg(), top=[l.strip() for l in ps[:9]],
                mem_available_kb=int(mem['MemAvailable'].split()[0]),
                disk_free_gb=round(shutil.disk_usage(ROOT).free / 1e9, 1))


def marked():
    """Processes that inherited this driver's marker, so only our own."""
    found = []
    for env in Path('/proc').glob('[0-9]*/environ'):
        try:
            if f'{MARKER}='.encode() in env.read_bytes():
                pid = int(env.parent.name)
                if pid != os.getpid():
                    found.append(pid)
        except OSError:
            pass
    return found


def reap(group):
    for sig in (signal.SIGTERM, signal.SIGKILL):
        try:
            os.killpg(group, sig)
        except ProcessLookupError:
            break
        time.sleep(.5)
    leftovers = []
    for pid in marked():
        try:
            leftovers.append(Path(f'/proc/{pid}/cmdline').read_bytes().replace(b'\0', b' ').decode()[:200])
            os.kill(pid, signal.SIGKILL)
        except OSError:
            pass
    return leftovers


def run(stage, cond, rep):
    if stage in RUST:
        cwd, cmd = RUST[stage]
        cwd = WORKTREE / cwd
    else:
        sub, cmd = FRONTEND[stage]
        cwd = ROOT / f'frontend-{rep}' / sub
    target = ROOT / f'target-{FAMILY.get(stage)}-{rep}'
    if cond == 'cold' and stage in RUST and stage != 'nextest' and target.exists():
        sys.exit(f'cold {stage} needs an absent {target}')
    env = environment(stage, rep)
    LOGS.mkdir(exist_ok=True)
    log = LOGS / f'{stage}-{cond}-{rep}.log'
    row = dict(series=os.environ.get('LATENCY_SERIES', 'a'), stage=stage, condition=cond, rep=rep, cwd=str(cwd.relative_to(WORKTREE if stage in RUST else ROOT)),
               command=cmd, limit_seconds=LIMIT, target=env.get('CARGO_TARGET_DIR', '').replace(str(ROOT), '$ROOT'),
               started_utc=time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()), background=background())
    before = resource.getrusage(resource.RUSAGE_CHILDREN)
    start = time.monotonic()
    # GNU timeout signals the whole new process group, even if this driver dies.
    argv = ['timeout', '-k', '10', str(LIMIT)] + cmd.split()
    with log.open('wb') as out:
        proc = subprocess.Popen(argv, cwd=cwd, env=env, stdout=out, stderr=subprocess.STDOUT,
                                start_new_session=True)
        try:
            code = proc.wait(timeout=LIMIT + 20)
        except BaseException:
            code = 124
        finally:
            leftovers = reap(proc.pid)
            proc.wait()
    wall = time.monotonic() - start
    after = resource.getrusage(resource.RUSAGE_CHILDREN)
    row.update(exit_code=code, wall_seconds=round(wall, 3),
               user_seconds=round(after.ru_utime - before.ru_utime, 2),
               system_seconds=round(after.ru_stime - before.ru_stime, 2),
               leftover_processes=leftovers, log=str(log))
    row['links'] = links(env.get('LATENCY_LINK_LOG'))
    lines = log.read_text(errors='replace').splitlines()
    row['tail'] = [l[:300] for l in lines[-25:]]
    junit = copy_junit(stage, cond, rep, env, cwd)
    if junit:
        row['junit'] = junit
    with (ROOT / 'results.jsonl').open('a') as f:
        f.write(json.dumps(row) + '\n')
    print(json.dumps({k: row[k] for k in ('stage', 'condition', 'rep', 'exit_code', 'wall_seconds',
                                          'user_seconds', 'leftover_processes')}))
    print('\n'.join(row['tail'][-12:]))


def links(directory):
    if not directory:
        return None
    total, count, longest = 0.0, 0, 0.0
    for path in Path(directory).glob('*.time'):
        for line in path.read_text().splitlines():
            if line.startswith('real '):
                value = float(line.split()[1])
                total, count, longest = total + value, count + 1, max(longest, value)
    return dict(calls=count, driver_seconds_sum=round(total, 2), longest=round(longest, 2))


def copy_junit(stage, cond, rep, env, cwd):
    if stage == 'nextest':
        # Nextest keeps its store under the workspace target, not CARGO_TARGET_DIR.
        source = WORKTREE / 'target' / 'nextest' / 'ci' / 'junit.xml'
    elif stage in ('desktop-test', 'dashboard-test'):
        source = cwd / 'junit.xml'
    else:
        return None
    if not source.exists():
        return 'missing'
    destination = ROOT / 'junit' / f"{stage}-{cond}-{rep}-{os.environ.get('LATENCY_SERIES', 'a')}.xml"
    destination.parent.mkdir(exist_ok=True)
    shutil.copyfile(source, destination)
    return str(destination).replace(str(ROOT), '$ROOT')


def snapshot(rep):
    """Copy ui/ without installed or built state, like the macOS method."""
    dest = ROOT / f'frontend-{rep}'
    if dest.exists():
        sys.exit(f'{dest} exists')
    shutil.copytree(WORKTREE / 'ui', dest, symlinks=True, ignore=shutil.ignore_patterns(
        'node_modules', 'dist', 'target', '*.tsbuildinfo', 'junit.xml'))
    crates = ROOT / 'crates'
    if not crates.exists():
        crates.symlink_to(WORKTREE / 'crates')


def main():
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest='action', required=True)
    r = sub.add_parser('run')
    r.add_argument('stage', choices=sorted({**RUST, **FRONTEND}))
    r.add_argument('condition', choices=['cold', 'warm'])
    r.add_argument('rep', type=int)
    s = sub.add_parser('snapshot')
    s.add_argument('rep', type=int)
    args = parser.parse_args()
    if args.action == 'snapshot':
        snapshot(args.rep)
    else:
        run(args.stage, args.condition, args.rep)


if __name__ == '__main__':
    main()
