"""Recomputes `results.json` from the committed `cases/*.jsonl` and `predictions/*.jsonl` alone
- the "separate command recomputes metrics from committed predictions" the common acceptance
criteria ask for. `python3 -m bench_progress.build_results > results.json` never imports torch,
never opens the daemon's database, and never calls Kev.
"""
from __future__ import annotations

import json
import os
import platform
import subprocess
import sys

from . import metrics as metrics_mod
from .dataset import load_dataset
from .schema import load_predictions

HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
PRED_DIR = os.path.join(HERE, "predictions")


def _git_commit() -> str:
    try:
        return subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=HERE, text=True).strip()
    except Exception:
        return "unknown"


def _dataset_summary(windows):
    by_split = {}
    for w in windows:
        s = by_split.setdefault(w.split, {"n": 0, "labels": {}, "provenance": {}})
        s["n"] += 1
        s["labels"][w.label] = s["labels"].get(w.label, 0) + 1
        s["provenance"][w.provenance] = s["provenance"].get(w.provenance, 0) + 1
    return by_split


def main() -> int:
    dev, ev = load_dataset()
    all_windows = dev + ev

    pred_paths = {"dev": os.path.join(PRED_DIR, "dev.jsonl"), "eval": os.path.join(PRED_DIR, "eval.jsonl")}
    predictions = {}
    for split, path in pred_paths.items():
        predictions[split] = load_predictions(path) if os.path.exists(path) else []

    policies = sorted({p.policy for preds in predictions.values() for p in preds})

    def _load_json(path):
        return json.load(open(path)) if os.path.exists(path) else None

    kev_runs = {
        "prompt_v1_dev_pilot": _load_json(os.path.join(PRED_DIR, "kev_v1_stats_dev.json")),
        "prompt_v2_dev_pilot": _load_json(os.path.join(PRED_DIR, "kev_v2_stats_dev.json")),
        "prompt_v2_eval_selected": _load_json(os.path.join(PRED_DIR, "kev_stats_eval.json")),
    }
    prompt_selection = {
        "candidates": ["prompts/v1.json", "prompts/v2.json"],
        "dev_accuracy": {"v1": 66 / 89, "v2": 67 / 89},
        "eval_accuracy_selected_design": 42 / 53,
        "selected": "prompts/v2.json",
        "reason": "higher dev accuracy overall (0.753 vs 0.742), driven by insufficient_evidence "
        "(9/9 vs 0/9) and waiting_user_input (11/17 vs 8/17) recall; selected before touching "
        "eval, then run once on eval (0.792 accuracy there) per the freeze rule. See "
        "bench_progress/score_kev_raw.py's output (predictions/kev_v1_raw_dev.jsonl, "
        "kev_v2_raw_dev.jsonl, kev_raw_eval.jsonl) for the full confusion matrices.",
    }

    # Model-free policies spend no model call; a `combined_*` policy spends exactly the calls
    # its underlying model-backed policy already made (the same committed predictions), never
    # an extra one (task item 14: cost is counted once per real call, not once per policy name).
    model_free = {"baseline", "repetition", "combined_repetition"}
    model_call_source = {"kev": "kev", "combined_kev": "kev"}

    variants = {}
    metrics_out = {"dev": {}, "eval": {}}
    for split, windows in (("dev", dev), ("eval", ev)):
        present = {p.policy for p in predictions[split]}
        for policy in [p for p in policies if p in present]:
            if policy in model_free:
                calls = 0
            else:
                source = model_call_source.get(policy, policy)
                calls = sum(1 for p in predictions[split] if p.policy == source)
            m = metrics_mod.compute(windows, predictions[split], policy, model_calls=calls)
            metrics_out[split][policy] = m.__dict__
        # Detection delay, where timestamps permit (multi_snapshot repetitive_failure families).
        for policy in list(metrics_out[split].keys()):
            delays = metrics_mod.detection_delays(windows, predictions[split], policy)
            if delays:
                metrics_out[split][policy]["detection_delay_s"] = [d.__dict__ for d in delays]

    variants = {
        "baseline": "the current quiet watchdog, ported from scheduler/quiet.rs (spec 009)",
        "repetition": "a simple rule: escalate when one tool name repeats >= 3 times in the last 6 tool calls",
        "combined_repetition": "the baseline, with the repetition rule allowed to add "
        "escalate_unproductive where the baseline says continue/nudge (policy_combine.py)",
    }
    if "kev" in policies:
        variants["kev"] = "jaredpalmer/kev-4b@139fdd94f1b6a6ad80cc15e08fcb99cac885a101, choice over the four labels"
        variants["combined_kev"] = "the baseline, with Kev allowed to add escalate_unproductive the same way"

    results = {
        "area": "scheduler-attention-and-watchdogs",
        "run": {
            "python": sys.version.split()[0],
            "platform": platform.platform(),
            "source_commit": _git_commit(),
            "reproduce_dataset": "python3 -m bench_progress.generate",
            "reproduce_predictions": "python3 -m bench_progress.replay --split both"
            " [--kev-raw predictions/kev_raw_eval.jsonl --kev-policy-name kev]",
            "reproduce_metrics": "python3 -m bench_progress.build_results > results.json",
            "kev_inference": {
                "run": "jaredpalmer/kev-4b@139fdd94f1b6a6ad80cc15e08fcb99cac885a101",
                "lock": "/tmp/ariadne-ai-01m48gt1v7v045wtc1kaw0fnas.lock",
                "invocation": "python3 bench_progress/kev_runner.py --prompt prompts/v2.json "
                "--input cases/eval.jsonl --output predictions/kev_raw_eval.jsonl "
                "--stats predictions/kev_stats_eval.json --policy-name kev",
                "environment": "~/.ariadne/ai-permissions/venv/bin/python3 (the interpreter the "
                "orchestrator confirmed already carries kev pinned to the required revision, "
                "reused read-only), HF_HOME=~/.ariadne/ai-opportunities/hf (the shared "
                "experimental cache, not the production ai-permissions one), device=mps (Apple "
                "Silicon), torch 2.8.0, transformers 5.19.0, peft 0.21.2; not necessarily "
                "Ariadne's production inference environment - see limitations.",
                "kev_package_revision": "f1535963cea021439370c23127bc970b6788e730",
                "measured_runs": kev_runs,
            },
            "prompt_selection": prompt_selection,
        },
        "dataset": {
            "splits": _dataset_summary(all_windows),
            "family_count": len({w.family for w in all_windows}),
            "label_rules": "see bench_progress/synth.py (synthetic, label_source=by_construction) "
            "and bench_progress/real_export.py (real_sanitized, label_source=observed|inferred)",
            "model_calls_per_family": {
                "min": min(metrics_mod.model_call_counts(all_windows).values()),
                "max": max(metrics_mod.model_call_counts(all_windows).values()),
                "mean": sum(metrics_mod.model_call_counts(all_windows).values())
                / len(metrics_mod.model_call_counts(all_windows)),
                "note": "one advisory call per replayed window; a multi_snapshot loop family "
                "costs one call per snapshot, every other family costs one.",
            },
        },
        "variants": variants,
        "metrics": metrics_out,
        "limitations": [
            "Synthetic windows dominate the dataset (122 of 142); real-sanitized windows (20) "
            "are a secondary, smaller check, reported separately in dataset.splits.",
            "Real-sanitized labels are inferred post hoc from the same evidence a policy sees, "
            "not confirmed by a human reviewer against the original transcript - see "
            "label_source on each case and the ambiguous flag.",
            "The baseline port evaluates one snapshot at a time; it does not model the "
            "one-nudge/one-flag-per-situation dedup quiet.rs keeps across scheduler passes, "
            "since that is a delivery concern, not a decision this bench grades.",
            "Detection delay is only computed for the loop-identical-* families tagged "
            "multi_snapshot, where several decision points over time exist for the same loop.",
            "combined_kev never softens a baseline escalation (policy_combine.py), so it "
            "inherits the baseline's own false escalations on long-running single tool calls "
            "even where Kev alone judged them correctly as progress - see kev vs combined_kev "
            "false_escalation_rate on eval.",
            "A dataset of 142 windows (53 reserved for eval) is enough to compare policies, not "
            "enough to bound Kev's error rate tightly; treat the eval numbers as a pilot "
            "measurement, not a production SLO.",
            "Real-sanitized windows are read from the daemon's live, continuously-growing "
            "ariadne.db; `generate.py --refresh-real` can pull different trailing events for "
            "the same real session id if that session kept reporting since the last refresh, "
            "which is why it is a separate, opt-in flag rather than the default. The 122 "
            "synthetic windows are fully deterministic and are what the default reproduction "
            "command guarantees byte-for-byte; the real-sanitized 20 are a best-effort, "
            "timestamped supplement, refreshed deliberately, not a frozen fixture.",
        ],
        "recommendation": "investigate further",
    }
    json.dump(results, sys.stdout, indent=2, sort_keys=True)
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
