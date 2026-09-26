# The AI permission benchmark

This benchmark runs permission cases through one or more evaluators and prints
their scores. It is a local measurement tool; it does not run in CI.

## Setup

Laya and Kev need separate virtual environments, but share a Hugging Face
cache. Set the cache before running either evaluator:

```sh
export HF_HOME=~/.ariadne/ai-permissions/hf
```

Install Laya in its own environment:

```sh
python3 -m venv ~/.ariadne/ai-permissions/laya-venv
~/.ariadne/ai-permissions/laya-venv/bin/pip install "laya[serve] @ <Laya wheel URL>"
```

Install Kev in a separate environment:

```sh
python3.13 -m venv ~/.ariadne/ai-permissions/kev-venv
~/.ariadne/ai-permissions/kev-venv/bin/pip install "git+https://github.com/jaredpalmer/kev.git"
```

Run the script through the environment for the in-process evaluator you use.
The `ariadne` evaluator invokes Cargo and uses the daemon's installed Kev model
unless its config gives an `endpoint`.

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
id, allow_score|null, label (allow|escalate), guardrail|null, latency_ms
```

`laya` and `kev` run their models in process. Their configurations retain the
representation, fields, question, threshold, and guardrails settings. Relative
guardrails paths resolve from the repository root. `ariadne` calls
`ai_permission_eval`; its config can set `endpoint` and `threshold`.

The bundled `laya` and `kev` configurations are selected automatically when
their evaluators are named. Pass another configuration with
`--evaluator name=path/to/config.json`.

## Run

Run Kev and the daemon implementation over the development cases:

```sh
HF_HOME=~/.ariadne/ai-permissions/hf \
~/.ariadne/ai-permissions/kev-venv/bin/python3 bench/ai-permissions/run.py run \
  --evaluator kev --evaluator ariadne
```

The default cases are `safe.jsonl`, `elevated.jsonl`, and
`adversarial-dev.jsonl`. Held-out files run only when named directly:

```sh
python3 bench/ai-permissions/run.py run --evaluator ariadne \
  --cases bench/ai-permissions/cases/safe-heldout.jsonl
```

`--real` also reads approved requests from `~/.ariadne/ariadne.db` in read-only
mode and adds real coverage to the table. `--sweep` prints each evaluator at
thresholds from 0.00 through 1.00. Per-case CSV files go to
`bench/ai-permissions/out/` by default; set another directory with `--out`.
