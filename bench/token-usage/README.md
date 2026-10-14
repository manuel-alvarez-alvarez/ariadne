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

Transcript reconciliation also reads the local Codex and Claude transcript
directories. It filters records later than the database cutoff and records a
SHA-256 manifest of those retained lines in `snapshot.json`. A changed
manifest means current transcript files cannot reproduce the pinned transcript
comparison. The database-only aggregates remain reproducible from the pinned
SQLite copy; acquire an external transcript copy with the recorded manifest to
reproduce transcript evidence exactly.
Use `--codex-transcripts` and `--claude-transcripts` to point the analyzer at
that preserved external copy. `transcript-manifest.csv` contains stable
hashed file identities and filtered content hashes for matched evidence.

## Request-level follow-up

`FOLLOWUP-REPORT.md` records the separate request-level study. It preserves
the baseline artifacts and canonical cumulative accounting in `REPORT.md`.
The follow-up only sums one retained native request record per supported
format; it never adds that sum to session counters.

Create a fresh SQLite backup as above, then run:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 bench/token-usage/followup.py \
  --database /tmp/ariadne-token-usage-snapshot.db \
  --output /tmp/ariadne-token-usage-followup \
  --revision "$(git rev-parse HEAD)" \
  --snapshot-location /tmp/ariadne-token-usage-snapshot.db \
  --source-label 'refreshed SQLite backup of ~/.ariadne/ariadne.db'
PYTHONDONTWRITEBYTECODE=1 python3 bench/token-usage/test_followup.py
```

`followup-snapshot.json` records the cutoff, source hash, analyzer source
hash, requested source revision, format inventory, and partial coverage.
`followup-transcript-manifest.csv` records a stable hashed source identity and
retained-content SHA-256 for every parsed source. The committed
request-level rows include only sessions with a native Ariadne mapping. They
contain identifiers and aggregate counters, never raw prompt or tool text.
Codex `token_usage_record.usage` is a per-response evidence layer. Its
cumulative `token_count` records and overlapping turn/thread totals are
excluded from request totals. Claude records are deduplicated by native
request ID. OpenCode is inventoried as unsupported because this installation
does not retain a native Ariadne join or validated deduplication rule.

The committed follow-up was generated from the external snapshot
`/tmp/token-followup.db`, SHA-256
`adfae5cd052310fedd19d3f7b2aa4742c1a6f2a60f546a65de32f7894bc2a06a`.
Reproduce it only with that database and transcript roots that match the
follow-up manifest. The frozen baseline has separate, documented inputs.

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
PYTHONDONTWRITEBYTECODE=1 python3 bench/token-usage/test_followup.py
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
