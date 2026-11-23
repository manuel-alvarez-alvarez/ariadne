#!/usr/bin/env python3
"""Measure the cc driver, not exclusive linker internals."""
import json
import os
from pathlib import Path
import subprocess
import sys
import time


def main():
    args = sys.argv[1:]
    directory = Path(os.environ['LATENCY_LINK_DIR'])
    pid = os.getpid()
    output = args[args.index('-o') + 1] if '-o' in args else None
    kind = 'other-library'
    if output:
        name = Path(output).name
        if 'build_script' in name or '/build/' in output:
            kind = 'build-script'
        elif name.endswith('.so'):
            kind = 'procedural-macro-or-shared-library'
        elif '/deps/' not in output:
            kind = 'final-binary'
        else:
            kind = 'test-binary-or-other'
    invocation = f'{pid}-{time.monotonic_ns()}'
    timing = directory / f'{invocation}.time'
    start = time.monotonic()
    record = dict(pid=pid, command=os.environ['LATENCY_COMMAND'], output=output,
                  kind=kind, argv=args, monotonic_start=start, monotonic_end=None,
                  status='started')
    record_path = directory / f'{invocation}.json'
    record_path.write_text(json.dumps(record, indent=2) + '\n')
    result = subprocess.run(['/usr/bin/time', '-f', '%e\t%U\t%S', '-o', str(timing),
                             '/usr/bin/cc', *args], check=False)
    end = time.monotonic()
    record = dict(pid=pid, command=os.environ['LATENCY_COMMAND'], output=output,
                  kind=kind, argv=args, monotonic_start=start, monotonic_end=end,
                  exit_code=result.returncode, wall_seconds=end - start, status='finished')
    if timing.exists():
        lines = timing.read_text().splitlines()
        try:
            elapsed, user, system = map(float, lines[-1].split('\t'))
            record.update(driver_elapsed=elapsed, user_seconds=user, system_seconds=system)
        except (ValueError, IndexError):
            record['timing_error'] = timing.read_text()
        timing.unlink()
    record_path.write_text(json.dumps(record, indent=2) + '\n')
    return result.returncode


if __name__ == '__main__':
    sys.exit(main())
