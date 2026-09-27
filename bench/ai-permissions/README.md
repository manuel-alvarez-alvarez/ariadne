# The AI permission benchmark

This benchmark runs permission cases through one or more evaluators and prints
their scores. It is a local measurement tool; it does not run in CI.

## Setup

`run.sh` builds what it needs on first use; `--setup` only builds it:

```sh
bench/ai-permissions/run.sh --setup
```

Laya and Kev each run in a virtual environment of their own under
`~/.ariadne/ai-permissions` and share its Hugging Face cache (`HF_HOME`,
`~/.ariadne/ai-permissions/hf`):

- `laya-venv`: Python 3.14 and the wheel of Laya's latest GitHub release
  (`LAYA_WHEEL` installs another).
- `kev-venv`: Python 3.13 (or 3.12) and `kev[serve]` from the default branch
  of its repository.

Neither is on PyPI; the `laya` there is another project. A venv that already
has its package is used as it is, and one that does not is built with the
latest version. `--rebuild` rebuilds both, which is how to update them. The
latest Kev can be newer than the commit the daemon pins in
`crates/ariadne-daemon/src/ai_permissions/install.rs`. `LAYA_PYTHON` and
`KEV_PYTHON` pick the interpreter each is built with.

## Case format

Each `cases/*.jsonl` line is one JSON object with `id`, `set`, `expected`,
`category`, `note`, `repository`, and `request`. `request` has a `toolCall`
and non-empty `options` list. `expected` is `allow` or `escalate`; elevated and
adversarial cases always expect `escalate`.

Validate the complete dataset:

```sh
python3 bench/ai-permissions/run.py validate bench/ai-permissions/cases/
```

## Evaluators

Every evaluator accepts an optional JSON config and a list of cases, then
returns one result per input case in the same order:

```text
id, allow_score|null, label (allow|escalate), latency_ms
```

The evaluators are `laya` and `kev`, and both run their models in process.
Their configurations hold the representation, fields, question and
threshold. Nothing decides a case before the model: every case is scored.

The bundled `laya` and `kev` configurations are selected automatically when
their evaluators are named. Pass another configuration with
`--evaluator name=path/to/config.json`.

## Run

Run every evaluator over the development cases and print one table:

```sh
bench/ai-permissions/run.sh
```

Each evaluator runs `run.py run` in its own interpreter, in turn. The per-case
CSVs, each evaluator's log and the combined `report.txt` go to
`bench/ai-permissions/out/runs/<UTC time>/`, and `out/latest` links to the most
recent run. An evaluator that fails does not stop the others; the script
reports it and exits non-zero after printing the table of the rest.

```sh
run.sh -e kev                     # one evaluator only
run.sh -e laya=my-laya.json       # another configuration
run.sh --heldout                  # the held-out cases
run.sh -c cases/safe.jsonl --real --sweep
```

`run.py report <dir or CSVs> [--sweep]` prints the table again from CSVs an
earlier run wrote.

To run one evaluator by hand, use its venv's interpreter:

```sh
HF_HOME=~/.ariadne/ai-permissions/hf \
~/.ariadne/ai-permissions/kev-venv/bin/python3 bench/ai-permissions/run.py run \
  --evaluator kev
```

The default cases are `safe.jsonl`, `elevated.jsonl`, and
`adversarial-dev.jsonl`. Held-out files run only when named directly:

```sh
HF_HOME=~/.ariadne/ai-permissions/hf \
~/.ariadne/ai-permissions/kev-venv/bin/python3 bench/ai-permissions/run.py run \
  --evaluator kev --cases bench/ai-permissions/cases/safe-heldout.jsonl
```

`--real` also reads approved requests from `~/.ariadne/ariadne.db` in read-only
mode and adds real coverage to the table. `--sweep` prints each evaluator at
thresholds from 0.00 through 1.00. Per-case CSV files go to
`bench/ai-permissions/out/` by default; set another directory with `--out`.
