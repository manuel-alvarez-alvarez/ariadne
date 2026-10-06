# Agent failure diagnosis

This experiment maps a sanitized agent error payload to a diagnosis and an advisory recovery category.

Run the frozen experiment with the pinned package and shared experiment cache:

```sh
HF_HOME=/Users/malvarez/.ariadne/ai-opportunities/hf \
python3 lock_run.py /Users/malvarez/.ariadne/ai-permissions/venv/bin/python3 \
experiment.py measure
```

Recompute the committed metrics without model inference:

```sh
/Users/malvarez/.ariadne/ai-permissions/venv/bin/python3 experiment.py recompute
```

`cases.jsonl` contains the 125 frozen labeled cases. `predictions.jsonl` preserves a probability distribution and timing for each case. `results.json` is the machine-readable summary.

The corpus has 80 development cases and 45 reserved evaluation cases. A source family and session stay in one split. The current corpus is synthetic because no sanitized production transcript was available. Each case records its evidence rationale before inference.

The baseline copies the current ACP rule: `codexErrorInfo = usageLimitExceeded`, the JetBrains `limit` category, or a case-insensitive configured pattern. Kev receives one frozen five-choice diagnosis prompt. The combined policy always preserves explicit protocol exhaustion, applies narrow contextual rules, asks Kev when those rules abstain, and returns to the baseline when Kev cannot answer.

The policies only propose `switch_model`, `retry`, `fix_configuration`, `task_action`, or `abstain`. This experiment does not execute a recovery.
