# Progress: measuring Kev against the quiet watchdog

Compares three ways of deciding, from a session's recent events alone, whether a coding agent
is making useful progress, stuck in a loop, correctly waiting on a person, or under-evidenced:
the daemon's current quiet watchdog (`scheduler/quiet.rs`, specs 009/008/018), a simplistic
repetition rule, and a local Kev-4B model. See [`report.md`](report.md) for the findings and
[`results.json`](results.json) for the numbers behind them.

## Layout

```
bench_progress/        the package: schema, the three policies, metrics, the dataset generator
cases/dev.jsonl         89 labelled windows used to build and tune (never touches final metrics)
cases/eval.jsonl        53 reserved windows, touched by Kev exactly once
prompts/v1.json         the two prompt designs tried during development (task rule: at most two)
prompts/v2.json
predictions/            committed per-policy predictions; metrics recompute from these alone
worker/kev_infer.py     runs inside the interpreter that has `kev`; the only file that imports it
tests/                  pytest: dataset separation, the baseline port, the repetition rule,
                        metrics arithmetic, and the advisory fallback rule
results.json            area / run / dataset / variants / metrics / limitations / recommendation
```

## Reproduce

Every command below runs from this directory (`bench/ai-opportunities/progress/`), in order.

**Rebuild the dataset.** By default this only rebuilds the 122 deterministic synthetic windows
and carries over whatever real-sanitized windows are already committed - it never reads the live
database unless told to, so running it twice with no flag yields byte-identical files:

```
python3 -m bench_progress.generate
```

Add `--refresh-real` to also re-pull a fresh sanitized sample of real sessions from
`~/.ariadne/ariadne.db` (where that file exists; a no-op otherwise) - a deliberate, logged
refresh, since that database keeps growing while this goal's own sessions run:

```
python3 -m bench_progress.generate --refresh-real
```

**Replay the two model-free policies** (baseline, the repetition rule, and the rule combined
with the baseline) over both splits:

```
python3 -m bench_progress.replay --split both
```

**Run Kev** reuses `~/.ariadne/ai-permissions/venv/bin/python3` - the interpreter the orchestrator
confirmed already carries `kev` pinned to the required revision
(`f1535963cea021439370c23127bc970b6788e730`) - read-only, and never installs into it; weights are
cached under `~/.ariadne/ai-opportunities/hf`, the shared experimental `HF_HOME` the three
sibling experiments under this goal provision into and read from, never the production
`ai-permissions` cache. `kev_runner.py` imports none of `bench/ai-permissions`'s own code, only
the public `kev` package, through that separate interpreter:

```
python3 bench_progress/kev_runner.py --prompt prompts/v2.json \
    --input cases/eval.jsonl --output predictions/kev_raw_eval.jsonl \
    --stats predictions/kev_stats_eval.json --policy-name kev
python3 -m bench_progress.replay --split eval --kev-raw predictions/kev_raw_eval.jsonl --kev-policy-name kev
```

`kev_runner.py` acquires `/tmp/ariadne-ai-01m48gt1v7v045wtc1kaw0fnas.lock` (an OS advisory lock,
released the instant the process exits, by any means) before it checks the installed revision or
loads any weights, and holds it for exactly as long as the model process runs; a busy lock is
waited out, logged once, never polled with a no-op. A missing interpreter, or one whose `kev` is
not at the required revision, raises a clear `RuntimeError` rather than silently falling back to
another model (`bench_progress.kev_runner.check_kev_revision`). Two prompt designs
(`prompts/v1.json`, `prompts/v2.json`) were tried on `cases/dev.jsonl` only; `v2` was selected
there (0.854 vs 0.764 dev accuracy) before `cases/eval.jsonl` was ever scored, and `v2` alone was
then run once on eval (0.830 accuracy) - see `results.json`'s `run.prompt_selection`.

**Recompute every metric from the committed files alone** (no model, no torch import, no read
of `ariadne.db`):

```
python3 -m bench_progress.build_results > results.json
```

**Run the tests:**

```
python3 -m pytest tests/ -q
```

## What this bench does not do

- It never writes to a live session, a specification or the daemon's settings; `real_export.py`
  opens `~/.ariadne/ariadne.db` with a read-only SQLite URI connection and nothing else.
- It reads no file under `crates/`.
- Real-sanitized windows are a secondary check (20 of 142 total): most of the dataset is
  synthetic, built to realize specific scenarios the task asked for (legitimate retries, a
  healthy test loop a frequency-only rule would flag, a long single tool call, idle planning,
  explicit waits, and a loop that never goes quiet by timestamp alone). `results.json`'s
  `dataset.splits[*].provenance` breaks the two apart.
