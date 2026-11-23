#!/usr/bin/env python3
"""Manual Linux cohort runner. No cache service access and no automatic dispatch."""
import argparse
import collections
import hashlib
import json
import os
from pathlib import Path
import platform
import resource
import selectors
import shutil
import signal
import statistics
import subprocess
import sys
import time
import urllib.parse
import urllib.request

SOURCE = '4f71da2465954107f113d639b5d7bf666c8446a9'
LIMITS = {'rust': 50, 'tauri': 30, 'frontend': 25}
CAP = 2 * 1024 ** 3
# Leave room for structured results, timing HTML and the artifact inventory.
PRIMARY_CAP = CAP - 64 * 1024 ** 2
PINS = {'rustc': '1.100.0', 'cargo': '1.100.0', 'cargo-nextest': '0.9.146',
        'sccache': '0.16.0', 'cargo-machete': '0.9.2', 'cargo-audit': '0.22.0',
        'node': '26.12.0', 'npm': '11.20.0'}
ACTIONS = {'checkout': '3d3c42e5aac5ba805825da76410c181273ba90b1',
           'setup-node': '949feb2413d6458794dcd2491c4babbbce0c15c1',
           'install-action': 'fe3acb53a98b62c772db35de75ce0e4595e7ca8c',
           'upload-artifact': 'b7c566a772e6b6bfb58ed0dc250532a479d7789f'}
CHAINS = {
    'rust': [('.', cmd.split()) for cmd in [
        'cargo machete .', 'cargo audit',
        'cargo build --locked --workspace --bins --timings',
        'cargo nextest run --workspace --profile ci --no-run',
        'cargo nextest run --workspace --profile ci', 'cargo test --doc --workspace',
        'cargo clippy --workspace --all-targets -- -D warnings',
        'scripts/check-unused-rust', 'scripts/check-migrations.sh']],
    'tauri': [('ui/src-tauri', ['cargo', 'check', '--locked', '--timings'])],
    'frontend': [(cwd, cmd.split()) for cwd in ['ui', 'ui/dashboard'] for cmd in [
        'npm ci', 'npm run check:audit', 'npm run lint', 'npm run typecheck',
        'npm run check:unused', 'npm test']],
}


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + '\n')


def read_command(argv, cwd=None, env=None):
    result = subprocess.run(argv, cwd=cwd, env=env, capture_output=True, text=True,
                            timeout=30, check=False)
    if result.returncode:
        raise RuntimeError(f'{argv}: exit {result.returncode}: {result.stderr[-1000:]}')
    return result.stdout.strip()


def validate_id(value):
    import re
    if not re.fullmatch('[a-z0-9-]{1,32}', value):
        raise ValueError('experiment_id must match [a-z0-9-]{1,32}')
    return value


def duplicate_guard(repository, experiment_id, run_id):
    """Public API only; paginate all active runs, fail closed on API errors."""
    validate_id(experiment_id)
    if len(repository.split('/')) != 2 or any(not x or not all(c.isalnum() or c in '-_.' for c in x) for x in repository.split('/')):
        raise ValueError('invalid repository')
    def request_json(url):
        request = urllib.request.Request(url, headers={'User-Agent': 'ariadne-latency', 'Accept': 'application/vnd.github+json'})
        with urllib.request.urlopen(request, timeout=30) as response:
            return json.load(response)
    current = request_json(f'https://api.github.com/repos/{repository}/actions/runs/{run_id}')
    page = 1
    while True:
        query = urllib.parse.urlencode(dict(per_page=100, page=page))
        runs = request_json(f'https://api.github.com/repos/{repository}/actions/workflows/linux-latency-experiment.yml/runs?{query}')['workflow_runs']
        for run in runs:
            # A same-ID run may have completed while this run was waiting on
            # concurrency. Compare its finish time to this dispatch's creation.
            if (run['display_title'] == f'linux-latency-{experiment_id}'
                    and int(run['id']) < int(run_id)
                    and (run['status'] != 'completed' or run['updated_at'] >= current['created_at'])):
                raise RuntimeError(f"overlapping experiment ID: run {run['id']}")
        if len(runs) < 100:
            break
        page += 1


def private_environment(base, identity):
    # An allowlist prevents inherited SCCACHE_*, cloud credentials, Cargo flags,
    # npm config and runtime cache tokens from reaching measured commands.
    keep = ('PATH', 'HOME', 'USER', 'LOGNAME', 'SHELL', 'RUSTUP_HOME', 'LD_LIBRARY_PATH')
    env = {k: os.environ[k] for k in keep if k in os.environ}
    env.update(CI='true', LANG='C.UTF-8', LC_ALL='C.UTF-8', TERM='dumb',
               CARGO_HOME=str(base.with_name(base.name + '-cargo')), CARGO_TARGET_DIR=str(base.with_name(base.name + '-target')),
               SCCACHE_DIR=str(base.with_name(base.name + '-sccache')), SCCACHE_GHA_ENABLED='false',
               SCCACHE_CONF=str(base / 'sccache.toml'),
               SCCACHE_SERVER_UDS=str(base / 'sccache.sock'), SCCACHE_IDLE_TIMEOUT='0',
               npm_config_cache=str(base.with_name(base.name + '-npm')), npm_config_userconfig=str(base / 'npmrc'),
               RUSTUP_TOOLCHAIN='1.100.0', CARGO_INCREMENTAL='0', RUST_BACKTRACE='1', CARGO_TERM_COLOR='never',
               CARGO_TERM_VERBOSE='true')
    if identity['lane'] == 'rust':
        env['RUSTC_WRAPPER'] = 'sccache'
    for name in ('cargo', 'target', 'sccache', 'npm'):
        base.with_name(base.name + '-' + name).mkdir()
    (base / 'sccache.toml').write_text('')
    (base / 'npmrc').write_text('')
    return env


def sample(source, base, group=None):
    result = {'monotonic': time.monotonic(), 'child_user': resource.getrusage(resource.RUSAGE_CHILDREN).ru_utime,
              'child_system': resource.getrusage(resource.RUSAGE_CHILDREN).ru_stime,
              'load': os.getloadavg(), 'disk_free_bytes': shutil.disk_usage(base).free}
    for name in ('stat', 'meminfo', 'diskstats'):
        path = Path('/proc') / name
        if path.exists():
            result[name] = path.read_text()
    result['process_group'] = []
    if group is not None:
        for stat in Path('/proc').glob('[0-9]*/stat'):
            try:
                fields = stat.read_text().rsplit(')', 1)[1].split()
                if int(fields[2]) == group:
                    values = dict(pid=int(stat.parent.name), parent=int(fields[1]),
                                  user_ticks=int(fields[11]), system_ticks=int(fields[12]),
                                  rss_pages=int(fields[21]))
                    io = stat.parent / 'io'
                    if io.exists():
                        values['io'] = io.read_text()
                    result['process_group'].append(values)
            except (OSError, ValueError, IndexError):
                pass
    # df and /proc/diskstats avoid repeatedly walking large private caches.
    result['disk_used_bytes'] = shutil.disk_usage(source).used
    return result


def kill_group(process):
    try:
        os.killpg(process.pid, signal.SIGTERM)
    except ProcessLookupError:
        return
    # Kill even after the leader exits, so descendants cannot keep running.
    try:
        process.wait(timeout=.2)
    except subprocess.TimeoutExpired:
        pass
    try:
        os.killpg(process.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    process.wait()


def timed_command(argv, cwd, env, log, samples, timeout, deadline, budget):
    start = time.monotonic()
    if start >= deadline:
        log.touch()
        return dict(status='deadline', exit_code=124, wall_seconds=0, user_seconds=0,
                    system_seconds=0, monotonic_start=start, monotonic_end=start, log=log.name)
    before = resource.getrusage(resource.RUSAGE_CHILDREN)
    status, code = 'ok', 0
    process = None
    written = 0
    try:
        process = subprocess.Popen(argv, cwd=cwd, env=env, stdout=subprocess.PIPE,
                                   stderr=subprocess.STDOUT, start_new_session=True)
        selector = selectors.DefaultSelector()
        selector.register(process.stdout, selectors.EVENT_READ)
        last_sample = 0
        with log.open('wb') as output, samples.open('a') as collector:
            while selector.get_map():
                now = time.monotonic()
                if now - last_sample >= 1:
                    collector.write(json.dumps(sample(cwd, log.parent, process.pid)) + '\n')
                    collector.flush()
                    last_sample = now
                if now >= min(start + timeout, deadline) and status == 'ok':
                    status, code = ('timeout' if now < deadline else 'deadline'), 124
                    kill_group(process)
                if total_bytes(log.parent) >= PRIMARY_CAP - 4 * 1024 ** 2 and status == 'ok':
                    status, code = 'evidence_limit', 125
                    kill_group(process)
                for key, _ in selector.select(.05):
                    data = os.read(key.fd, 65536)
                    if not data:
                        selector.unregister(key.fileobj)
                        continue
                    if written + len(data) > budget or total_bytes(log.parent) + len(data) > PRIMARY_CAP - 4 * 1024 ** 2:
                        status, code = 'evidence_limit', 125
                        kill_group(process)
                        # The omission is explicit and invalidates this observation.
                        continue
                    output.write(data)
                    output.flush()
                    written += len(data)
            process.wait()
        selector.close()
        if status == 'ok':
            code = process.returncode
            status = 'ok' if code == 0 else 'failed'
    finally:
        if process is not None:
            kill_group(process)
            process.stdout.close()
    after = resource.getrusage(resource.RUSAGE_CHILDREN)
    return dict(status=status, exit_code=code, wall_seconds=time.monotonic() - start,
                user_seconds=after.ru_utime - before.ru_utime,
                system_seconds=after.ru_stime - before.ru_stime,
                monotonic_start=start, monotonic_end=time.monotonic(),
                timestamp_utc=time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()), log=log.name)


def stats(env, destination):
    if shutil.which('sccache', path=env['PATH']):
        try:
            value = read_command(['sccache', '--show-stats', '--stats-format=json'], env=env)
            write_json(destination, json.loads(value))
        except (OSError, ValueError, RuntimeError, subprocess.TimeoutExpired) as error:
            write_json(destination, {'unavailable': str(error)})
    else:
        write_json(destination, {'unavailable': 'sccache not installed'})


def collect(source, env, evidence, prefix, argv, cwd):
    omissions = []
    target = Path(env['CARGO_TARGET_DIR'])
    candidates = list(target.glob('cargo-timings/*.html'))
    if argv[:3] == ['cargo', 'nextest', 'run'] and '--no-run' not in argv:
        candidates += list(target.glob('nextest/*/junit.xml'))
    if argv == ['npm', 'test']:
        candidates += [source / cwd / 'junit.xml']
    for index, path in enumerate(candidates):
        if path.is_file() and not path.is_symlink():
            destination = evidence / f'{prefix}-{index}-{path.name}'
            if path.stat().st_size + total_bytes(evidence) < PRIMARY_CAP:
                shutil.copyfile(path, destination)
            else:
                omissions.append(str(path.relative_to(source)) if path.is_relative_to(source) else path.name)
    # Inventory TS state by path and size; never upload dependency trees.
    state = typescript_state(source)
    write_json(evidence / f'{prefix}-typescript.json', state)
    return omissions


def typescript_state(source):
    state = []
    for root in ('ui', 'ui/dashboard'):
        for folder in (source / root, source / root / 'node_modules/.tmp'):
            for path in folder.glob('*.tsbuildinfo'):
                state.append((str(path.relative_to(source)), path.stat().st_size))
    return sorted(set(state))


def annotate_links(evidence, prefix):
    import re
    proc_macros = set()
    final_binaries = set()
    with (evidence / f'{prefix}.log').open(errors='replace') as raw:
        for line in raw:
            crate = re.search(r'--crate-name ([A-Za-z0-9_]+)', line)
            if crate and '--crate-type proc-macro' in line:
                proc_macros.add(crate[1])
            if crate and '--crate-type bin' in line and '--test' not in line:
                final_binaries.add(crate[1])
    totals = collections.defaultdict(lambda: dict(count=0, wall_seconds=0, user_seconds=0, system_seconds=0))
    for path in (evidence / 'links').glob('*.json'):
        record = json.loads(path.read_text())
        if record['command'] != prefix:
            continue
        name = Path(record.get('output') or '').name
        if any(name.startswith('lib' + crate + '-') for crate in proc_macros):
            record['kind'] = 'procedural-macro'
        elif any(name == crate or name.startswith(crate + '-') for crate in final_binaries):
            record['kind'] = 'final-binary'
        write_json(path, record)
        bucket = totals[record['kind']]
        bucket['count'] += 1
        for metric in ('wall_seconds', 'user_seconds', 'system_seconds'):
            bucket[metric] += record.get(metric, 0)
    return dict(totals)


def junit_counts(evidence, prefix):
    import xml.etree.ElementTree as xml
    counts = []
    for path in evidence.glob(f'{prefix}-*-junit.xml'):
        try:
            root = xml.parse(path).getroot()
            cases = list(root.iter('testcase'))
            counts.append(dict(path=path.name, tests=len(cases),
                               failures=sum(c.find('failure') is not None for c in cases),
                               errors=sum(c.find('error') is not None for c in cases),
                               skipped=sum(c.find('skipped') is not None for c in cases)))
        except xml.ParseError as error:
            counts.append(dict(path=path.name, invalid=str(error)))
    return counts


def total_bytes(directory):
    total = 0
    for path in directory.rglob('*'):
        try:
            if path.is_file() and not path.is_symlink():
                total += path.stat().st_size
        except FileNotFoundError:
            # A driver atomically removes its temporary GNU time file.
            pass
    return total


def enforce_cap(evidence, cap=CAP):
    omissions = []
    for path in sorted(evidence.glob('*.html'), key=lambda p: p.stat().st_size, reverse=True):
        if total_bytes(evidence) <= cap:
            break
        omissions.append({'path': path.name, 'bytes': path.stat().st_size, 'reason': 'artifact cap'})
        path.unlink()
    write_json(evidence / 'omissions.json', omissions)
    if total_bytes(evidence) > cap:
        raise RuntimeError('primary evidence exceeds artifact cap; upload refused, no primary evidence silently truncated')


def run_pair(source, private_root, identity, commands, verify, timeout=600, deadline=None):
    """Run a complete pair and Rust replay. Inject only tiny commands in fixture tests."""
    validate_id(identity['experiment_id'])
    key = '-'.join(str(identity[k]) for k in ('experiment_id', 'run_id', 'run_attempt', 'source_sha', 'image_os', 'image_version', 'lane', 'repetition'))
    key = 'latency-linux-v1-' + key
    if any(c not in 'abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-._' for c in key):
        raise ValueError('invalid key component')
    base = private_root / key
    base.mkdir(parents=True, exist_ok=False)
    evidence = private_root / (key + '-evidence')
    evidence.mkdir()
    env = private_environment(base, identity)
    for name in ('cargo', 'target', 'sccache', 'npm'):
        write_json(base.with_name(base.name + '-' + name) / 'manifest.json', identity)
    result = dict(key=key, identity=identity, evidence=str(evidence), commands=[], omissions=[])
    write_json(evidence / 'manifest.json', identity)
    env['LATENCY_LINK_DIR'] = str(evidence / 'links')
    (evidence / 'links').mkdir()
    env['CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER'] = str(Path(__file__).with_name('linker.sh').resolve())
    good = True
    deadline = deadline if deadline is not None else time.monotonic() + (LIMITS[identity['lane']] - 5) * 60
    try:
        for phase in (['cold', 'warm', 'replay'] if identity['lane'] == 'rust' else ['cold', 'warm']):
            if good:
                verify()
                if phase == 'cold':
                    if any((source / p).exists() for p in ('target', 'ui/src-tauri/target', 'ui/node_modules', 'ui/dashboard/node_modules')):
                        raise RuntimeError('cold checkout already contains application artifacts')
                    if any(list((source / root).rglob('*.tsbuildinfo')) for root in ('ui', 'ui/dashboard')):
                        raise RuntimeError('unexpected TypeScript state')
                if phase == 'replay':
                    base.with_name(base.name + '-replay-target').mkdir()
                    env['CARGO_TARGET_DIR'] = str(base.with_name(base.name + '-replay-target'))
            for index, (cwd, argv) in enumerate(commands):
                prefix = f'{phase}-{index:02}'
                row = dict(phase=phase, index=index, cwd=cwd, argv=argv)
                if not good:
                    row.update(status='invalid', exit_code=None, reason='required preceding observation failed')
                elif time.monotonic() >= deadline:
                    row.update(status='deadline', exit_code=124, reason='five-minute evidence reserve reached')
                    good = False
                else:
                    state_before = typescript_state(source)
                    env['LATENCY_COMMAND'] = prefix
                    if identity['lane'] == 'rust':
                        stats(env, evidence / f'{prefix}-sccache-before.json')
                    row.update(timed_command(argv, source / cwd, env, evidence / f'{prefix}.log',
                                             evidence / 'resources.jsonl', timeout, deadline,
                                             max(0, PRIMARY_CAP - total_bytes(evidence))))
                    if identity['lane'] == 'rust':
                        stats(env, evidence / f'{prefix}-sccache-after.json')
                    result['omissions'].extend(collect(source, env, evidence, prefix, argv, cwd))
                    import re
                    row['test_summaries'], row['download_evidence'] = [], []
                    with (evidence / row['log']).open(errors='replace') as raw:
                        for line in raw:
                            if re.search(r'Summary|Test Files|Tests\s|test result:|Finished .* in ', line):
                                if len(row['test_summaries']) < 100:
                                    row['test_summaries'].append(line.rstrip()[:1024])
                            if re.search(r'Downloaded|Downloading|Updating .*index|added .* packages', line):
                                if len(row['download_evidence']) < 100:
                                    row['download_evidence'].append(line.rstrip()[:1024])
                    row['linker_driver'] = annotate_links(evidence, prefix)
                    row['junit'] = junit_counts(evidence, prefix)
                    after_paths = {p for p, _ in typescript_state(source)}
                    row['removed_typescript_state'] = [p for p, _ in state_before if p not in after_paths]
                    good = row['status'] == 'ok'
                result['commands'].append(row)
                write_json(evidence / 'result.json', result)
    except BaseException as error:
        result['error'] = f'{type(error).__name__}: {error}'
        good = False
        if isinstance(error, (KeyboardInterrupt, SystemExit)):
            result['cancelled'] = True
    finally:
        if identity['lane'] == 'rust' and shutil.which('sccache', path=env['PATH']):
            try:
                read_command(['sccache', '--stop-server'], env=env)
            except (RuntimeError, subprocess.TimeoutExpired):
                pass
        observed = {(row['phase'], row['index']) for row in result['commands']}
        for phase in (['cold', 'warm', 'replay'] if identity['lane'] == 'rust' else ['cold', 'warm']):
            for index, (cwd, argv) in enumerate(commands):
                if (phase, index) not in observed:
                    result['commands'].append(dict(phase=phase, index=index, cwd=cwd, argv=argv,
                                                   status='invalid', exit_code=None,
                                                   reason='required preceding observation failed'))
        result['valid'] = good
        write_json(evidence / 'result.json', result)
        with (evidence / 'timings.tsv').open('w') as table:
            table.write('phase\tindex\tstatus\texit_code\twall_seconds\tuser_seconds\tsystem_seconds\n')
            for row in result['commands']:
                table.write('\t'.join(str(row.get(k, '')) for k in ('phase', 'index', 'status', 'exit_code', 'wall_seconds', 'user_seconds', 'system_seconds')) + '\n')
        enforce_cap(evidence)
    return result


def summarize(results):
    groups = collections.defaultdict(list)
    for result in results:
        identity = result['identity']
        # A moving main SHA or any tool/lock/package drift makes a new cohort.
        cohort = {k: identity[k] for k in ('experiment_id', 'source_sha', 'workflow_sha', 'image_os',
                  'image_version', 'lane', 'packages', 'locks', 'tools', 'actions', 'main_sha',
                  'kernel', 'cpu_count', 'cpu_model', 'memory_total', 'runner_version') if k in identity}
        groups[json.dumps(cohort, sort_keys=True)].append(result)
    report = []
    for cohort, pairs in sorted(groups.items()):
        observations = collections.defaultdict(list)
        ratios = []
        ratios_by_stage = collections.defaultdict(list)
        repetitions = set()
        seen = set()
        for pair in pairs:
            repetition = pair['identity']['repetition']
            if repetition in seen:
                raise ValueError('duplicate repetition in comparable cohort; do not replace observations')
            seen.add(repetition)
            valid = {(r['phase'], r['index']): r['wall_seconds'] for r in pair['commands'] if r['status'] == 'ok'}
            if pair['commands'] and all(r['status'] == 'ok' for r in pair['commands']):
                repetitions.add(repetition)
                for row in pair['commands']:
                    observations[(row['phase'], row['index'])].append(row['wall_seconds'])
                for (phase, index), duration in valid.items():
                    if phase == 'warm' and valid.get(('cold', index), 0) > 0:
                        ratios.append(duration / valid['cold', index])
                        ratios_by_stage[index].append(duration / valid['cold', index])
        report.append(dict(cohort=json.loads(cohort), matching_repetitions=len(repetitions),
                           insufficient_matching_repetitions=len(repetitions) < 3,
                           paired_warm_cold_ratios=ratios,
                           ratios_by_stage=[dict(index=k, n=len(v), minimum=min(v), median=statistics.median(v), maximum=max(v)) for k, v in sorted(ratios_by_stage.items())],
                           stages=[dict(phase=k[0], index=k[1], n=len(v), minimum=min(v), median=statistics.median(v), maximum=max(v)) for k, v in sorted(observations.items())],
                           failures=[dict(repetition=p['identity']['repetition'], error=p.get('error'), commands=[r for r in p['commands'] if r['status'] != 'ok']) for p in pairs if p.get('error') or any(r['status'] != 'ok' for r in p['commands'])]))
    return report


def runner_version():
    candidates = list(Path('/home/runner/runners').glob('*/bin/Runner.Listener'))
    candidates += [Path('/home/runner/actions-runner/bin/Runner.Listener')]
    for candidate in candidates:
        if candidate.is_file():
            return read_command([str(candidate), '--version'])
    raise RuntimeError('cannot record exact runner version; inspect hosted layout before dispatch')


def identity_for(args):
    source = args.source.resolve()
    def verify():
        if read_command(['git', 'rev-parse', 'HEAD'], cwd=source) != SOURCE:
            raise RuntimeError('measured checkout SHA differs from fixed source')
    verify()
    if os.environ.get('ImageOS') != 'ubuntu26' or not os.environ.get('ImageVersion'):
        raise RuntimeError('requires hosted ubuntu26 and a recorded ImageVersion')
    tools = {}
    for name, version in PINS.items():
        actual = read_command([name, '--version'])
        if version not in actual.split()[1:]:
            # Node prints only v26.12.0; npm prints only the version.
            if actual not in (version, 'v' + version):
                raise RuntimeError(f'tool pin mismatch: {name}: {actual}')
        tools[name] = actual
    for name, argv in {'rustc_verbose': ['rustc', '-Vv'], 'python': ['python3', '--version'],
                       'time': ['/usr/bin/time', '--version'], 'gcc': ['/usr/bin/cc', '--version'],
                       'ld': ['/usr/bin/ld', '--version'], 'tmux': ['tmux', '-V']}.items():
        tools[name] = read_command(argv)
    locks = {path: hashlib.sha256((source / path).read_bytes()).hexdigest() for path in ('Cargo.lock', 'ui/src-tauri/Cargo.lock', 'ui/package-lock.json', 'ui/dashboard/package-lock.json')}
    packages = read_command(['dpkg-query', '-W', '-f=${binary:Package}\t${Version}\n'])
    identity = dict(experiment_id=validate_id(args.experiment_id), run_id=os.environ['GITHUB_RUN_ID'],
                    run_attempt=os.environ['GITHUB_RUN_ATTEMPT'], source_sha=SOURCE,
                    workflow_sha=os.environ['GITHUB_SHA'], image_os=os.environ['ImageOS'],
                    image_version=os.environ['ImageVersion'], lane=args.lane, repetition=args.repetition,
                    runner_version=runner_version(),
                    setup_seconds=time.monotonic() - float(args.started.read_text()),
                    main_sha=read_command(['git', 'rev-parse', 'origin/main'], cwd=source),
                    tools=tools, actions=ACTIONS, locks=locks, packages=packages,
                    kernel=platform.release(), cpu=Path('/proc/cpuinfo').read_text(),
                    memory=Path('/proc/meminfo').read_text(), cpu_count=os.cpu_count(),
                    memory_total=Path('/proc/meminfo').read_text().splitlines()[0],
                    cpu_model=next(line for line in Path('/proc/cpuinfo').read_text().splitlines() if line.startswith('model name')),
                    filesystem=read_command(['df', '-T', str(source)]))
    return identity, verify


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='operation', required=True)
    run = sub.add_parser('run')
    run.add_argument('--source', type=Path, required=True)
    run.add_argument('--private-root', type=Path, required=True)
    run.add_argument('--experiment-id', required=True)
    run.add_argument('--lane', choices=LIMITS, required=True)
    run.add_argument('--repetition', type=int, choices=(1, 2, 3), required=True)
    run.add_argument('--started', type=Path, required=True)
    guard = sub.add_parser('guard')
    guard.add_argument('experiment_id')
    report = sub.add_parser('report')
    report.add_argument('results', nargs='+', type=Path)
    cleanup = sub.add_parser('cleanup')
    cleanup.add_argument('root', type=Path)
    args = parser.parse_args()
    if args.operation == 'guard':
        duplicate_guard(os.environ['GITHUB_REPOSITORY'], args.experiment_id, os.environ['GITHUB_RUN_ID'])
    elif args.operation == 'report':
        print(json.dumps(summarize([json.loads(p.read_text()) for p in args.results]), indent=2))
    elif args.operation == 'cleanup':
        if shutil.which('sccache'):
            for base in args.root.glob('latency-linux-v1-*'):
                if not (base / 'sccache.toml').is_file():
                    continue
                env = {k: os.environ[k] for k in ('PATH', 'HOME', 'RUSTUP_HOME') if k in os.environ}
                env.update(SCCACHE_SERVER_UDS=str(base / 'sccache.sock'),
                           SCCACHE_CONF=str(base / 'sccache.toml'), SCCACHE_GHA_ENABLED='false')
                try:
                    read_command(['sccache', '--stop-server'], env=env)
                except (RuntimeError, subprocess.TimeoutExpired) as error:
                    print(f'Private server cleanup: {error}', file=sys.stderr)

    else:
        identity, verify = identity_for(args)
        started = float(args.started.read_text())
        deadline = started + (LIMITS[args.lane] - 5) * 60
        signal.signal(signal.SIGTERM, lambda *_: sys.exit(143))
        result = run_pair(args.source.resolve(), args.private_root.resolve(), identity,
                          CHAINS[args.lane], verify, deadline=deadline)
        print(json.dumps({'evidence': result['evidence'], 'valid': result['valid']}))
        return 0 if result['valid'] else 1
    return 0


if __name__ == '__main__':
    try:
        sys.exit(main())
    except Exception as error:
        if os.environ.get('RUNNER_TEMP'):
            bootstrap = Path(os.environ['RUNNER_TEMP']) / 'latency-bootstrap-evidence'
            bootstrap.mkdir(exist_ok=True)
            write_json(bootstrap / 'runner-error.json', {'error': f'{type(error).__name__}: {error}'})
        raise
