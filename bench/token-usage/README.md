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
  --revision "$(git rev-parse HEAD)"
```

The analyzer uses Python's standard library, opens the source database in
read-only mode, and exports aggregates only. Do not commit the database copy,
raw event payloads, or raw transcript files. To update the checked-in study,
inspect the `/tmp` output and copy only aggregate artifacts after confirming
the snapshot's cutoff and SHA-256.

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

`input_tokens` already includes `cached_input_tokens`. Therefore uncached input
is `input_tokens - cached_input_tokens`; cached input is never added again.
An absent usage row means missing usage, while a present all-zero row means a
demonstrated zero report.
