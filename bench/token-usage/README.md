# Historical token-use study

`REPORT.md` is the reviewed baseline and ranked implementation candidates.
Each candidate has a stable `TU-*` identifier for a later, separately agreed
implementation task. This study changes no production behavior.

## Reproduce

Run from the repository root against a **stable copy outside this repository**.
For an active database, use SQLite's online backup API so its WAL is included:

```sh
python3 - <<'PY'
import sqlite3
source = sqlite3.connect("file:/Users/malvarez/.ariadne/ariadne.db?mode=ro", uri=True)
target = sqlite3.connect("/tmp/ariadne-token-usage-snapshot.db")
source.backup(target)
target.close()
source.close()
PY
python3 bench/token-usage/analyze.py \
  --database /tmp/ariadne-token-usage-snapshot.db \
  --output /tmp/ariadne-token-usage-report \
  --revision "$(git rev-parse HEAD)" \
  --history-dir /Users/malvarez/.ariadne \
  --source-label 'refreshed SQLite backup of ~/.ariadne/ariadne.db'
```

The analyzer uses Python's standard library, opens the source database in
read-only mode, and exports aggregates only. Do not commit the database copy,
raw event payloads, or raw transcript files. To update the checked-in study,
inspect the `/tmp` output and copy only aggregate artifacts after confirming
the snapshot's cutoff and SHA-256.

The committed artifacts describe the pinned snapshot in `snapshot.json`. They
reproduce only when that external snapshot is available. The command above
refreshes the study from current data; it does not reproduce pinned evidence.
For this baseline, the external snapshot was
`/tmp/ariadne-token-usage-snapshot.XXXXXX.db`; its SHA-256 is in
`snapshot.json`. Run the same command with that path, the pinned revision, and
the `pinned snapshot` source label to reproduce the recorded evidence.

## Checks

Run these commands from the repository root. Use a fresh output directory.

```sh
PYTHONDONTWRITEBYTECODE=1 python3 bench/token-usage/analyze.py \
  --database /tmp/ariadne-token-usage-snapshot.db \
  --output /tmp/ariadne-token-usage-check \
  --revision "$(git rev-parse HEAD)" \
  --history-dir /Users/malvarez/.ariadne \
  --source-label 'check snapshot'
python3 - <<'PY'
import csv, json
from pathlib import Path
root = Path('bench/token-usage')
s = json.loads((root / 'snapshot.json').read_text())
r = json.loads((root / 'reconciliation.json').read_text())
assert s['canonical_usage'] == r['canonical_usage']
assert r['uncached_input_tokens'] == r['canonical_usage']['input_tokens'] - r['canonical_usage']['cached_input_tokens']
assert len(list(csv.DictReader((root / 'goals.csv').open()))) == s['inventory']['goals']
assert len(list(csv.DictReader((root / 'tasks.csv').open()))) == s['inventory']['tasks']
assert json.loads((root / 'completion-summary.json').read_text())['tasks_with_selected_ending']
assert len(list(csv.DictReader((root / 'goal-analysis.csv').open()))) == s['inventory']['goals']
assert not list(root.rglob('*.db'))
print('artifact checks: ok')
PY
git diff --check
```

## Artifacts

- `snapshot.json` records source identity, cutoff, available backups, coverage,
  partial work, and unsupported timing categories.
- `goals.csv`, `tasks.csv`, and `dimensions.csv` keep input, cached input,
  uncached input, and output separate.
- `reconciliation.json` describes the canonical counter rule and deliberately
  non-additive end-fact comparison.
- `trace-selection.json` contains selected IDs and derived counts only; it
  never exports event bodies.
- `recommendations.json` is the machine-readable stable recommendation list.
- `completion-timing.csv` and `completion-summary.json` retain every task end
  and define the latest-ending completion comparison.
- `goal-analysis.csv` and `transcript-reconciliation.csv` describe per-goal
  concentration and sanitized local transcript counter coverage.

`input_tokens` already includes `cached_input_tokens`. Therefore uncached input
is `input_tokens - cached_input_tokens`; cached input is never added again.
An absent usage row means missing usage, while a present all-zero row means a
demonstrated zero report.
