#!/usr/bin/env python3
"""Turn the private run root into committed evidence and Markdown tables."""
import csv
import json
from pathlib import Path
import re
import statistics
import shutil
import xml.etree.ElementTree as ET

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2] / 'target' / 'latency-host'
ORDER = ['bins', 'testbuild', 'nextest', 'doctest', 'clippy', 'tauri',
         'desktop-ci', 'dashboard-ci', 'desktop-test', 'dashboard-test', 'build']


def rng(values):
    values = sorted(values)
    if not values:
        return 'n/a'
    return f'{values[0]:.3f} / {statistics.median(values):.3f} / {values[-1]:.3f} (n={len(values)})'


def summary_seconds(row):
    for line in row['tail']:
        m = re.search(r'Summary \[\s*([\d.]+)s\]', line)
        if m:
            return float(m[1])
    return None


def stage_table(rows):
    out = ['| Stage | Command | Cold s, min / median / max | Warm s, min / median / max | Exits |',
           '| --- | --- | ---: | ---: | --- |']
    for stage in ORDER:
        group = [r for r in rows if r['stage'] == stage
                 and r['series'] == ('b' if stage in ('testbuild', 'nextest') else 'a')]
        if not group:
            continue
        cold = [r['wall_seconds'] for r in group if r['condition'] == 'cold']
        warm = [r['wall_seconds'] for r in group if r['condition'] == 'warm']
        exits = ','.join(str(r['exit_code']) for r in group)
        out.append(f"| {stage} | `{group[0]['command']}` | {rng(cold)} | {rng(warm)} | {exits} |")
        if stage == 'nextest':
            for cond in ('cold', 'warm'):
                run = [summary_seconds(r) for r in group if r['condition'] == cond]
                out.append(f'| nextest runner summary ({cond}) | from `Summary [..]` | '
                           + (f'{rng(run)} | | |' if cond == 'cold' else f'| {rng(run)} | |'))
    return '\n'.join(out)


def junit_cases(path, rust):
    cases = {}
    for case in ET.parse(path).getroot().iter('testcase'):
        key = (case.get('classname'), case.get('name'))
        # Two tests in one file can share a name; keep both, in file order.
        while key in cases:
            key = (key[0], key[1] + ' [duplicate name]')
        failed = case.find('failure') is not None or case.find('error') is not None
        cases[key] = (float(case.get('time')), failed)
    return cases


def durations(prefix, rust, out_csv):
    runs = sorted((ROOT / 'junit').glob(f'{prefix}-*.xml'))
    table = {}
    for index, path in enumerate(runs):
        for key, (value, failed) in junit_cases(path, rust).items():
            table.setdefault(key, [None] * len(runs))[index] = value
    names = [p.stem for p in runs]
    ranked = sorted(table.items(), key=lambda kv: -statistics.median([v for v in kv[1] if v is not None]))
    with out_csv.open('w', newline='') as f:
        w = csv.writer(f, lineterminator='\n')
        w.writerow(['rank', 'binary_or_file', 'test', 'min_s', 'median_s', 'max_s'] + names)
        for rank, ((cls, name), values) in enumerate(ranked, 1):
            present = [v for v in values if v is not None]
            w.writerow([rank, cls, name, f'{min(present):.3f}', f'{statistics.median(present):.3f}',
                        f'{max(present):.3f}'] + ['' if v is None else f'{v:.3f}' for v in values])
    return ranked, len(runs)


def ranked_table(ranked, count):
    out = ['| Rank | Binary or file | Test | Min s | Median s | Max s |', '| ---: | --- | --- | ---: | ---: | ---: |']
    for rank, ((cls, name), values) in enumerate(ranked[:count], 1):
        present = [v for v in values if v is not None]
        out.append(f'| {rank} | `{cls}` | `{name}` | {min(present):.3f} | '
                   f'{statistics.median(present):.3f} | {max(present):.3f} |')
    return '\n'.join(out)


def observation_table(rows):
    out = ['| Series | Stage | Condition | Rep | Wall s | CPU user s | CPU system s | Exit | Link calls | Link sum s | Link max s |',
           '| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |']
    for r in rows:
        link = r.get('links') or {}
        out.append(f"| {r['series']} | {r['stage']} | {r['condition']} | {r['rep']} | {r['wall_seconds']:.3f} | "
                   f"{r['user_seconds']:.2f} | {r['system_seconds']:.2f} | {r['exit_code']} | "
                   f"{link.get('calls', '')} | {link.get('driver_seconds_sum', '')} | {link.get('longest', '')} |")
    return '\n'.join(out)


def load_table(rows):
    out = ['| Series | Stage | Condition | Rep | Start UTC | Load 1/5/15 min | MemAvailable GiB | First `ps --sort=-pcpu` row (pid user %cpu %mem etimes comm) |',
           '| --- | --- | --- | ---: | --- | --- | ---: | --- |']
    for r in rows:
        load = '/'.join(f'{x:.2f}' for x in r['background']['load'])
        top = r['background']['top'][1] if len(r['background']['top']) > 1 else ''
        out.append(f"| {r['series']} | {r['stage']} | {r['condition']} | {r['rep']} | {r['started_utc']} | "
                   f"{load} | {r['background']['mem_available_kb'] / 1048576:.1f} | `{' '.join(top.split())}` |")
    return '\n'.join(out)


def main():
    rows = [dict({'series': 'a'}, **json.loads(l)) for l in (ROOT / 'results.jsonl').read_text().splitlines()]
    with (HERE / 'results.jsonl').open('w') as f:
        for r in rows:
            r = dict(r, log=Path(r['log']).name)
            f.write(json.dumps(r, sort_keys=True) + '\n')
    shutil.copyfile(ROOT / 'invalid-bins-untracked-evidence.jsonl', HERE / 'invalid-bins-untracked-evidence.jsonl')
    rust, _ = durations('nextest', True, HERE / 'rust-test-durations.csv')
    desk, _ = durations('desktop-test', False, HERE / 'desktop-test-durations.csv')
    dash, _ = durations('dashboard-test', False, HERE / 'dashboard-test-durations.csv')
    tables = ['## Stages', stage_table(rows), '## Rust tests', ranked_table(rust, 30),
              '## Desktop tests', ranked_table(desk, 20), '## Dashboard tests', ranked_table(dash, 5),
              '## Individual observations', observation_table(rows),
              '## Background load before each run', load_table(rows)]
    (HERE / 'tables.md').write_text('\n\n'.join(tables) + '\n')
    print('\n\n'.join(tables[:6]))


if __name__ == '__main__':
    main()
