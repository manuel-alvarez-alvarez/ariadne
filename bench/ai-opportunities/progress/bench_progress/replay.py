"""Replays every window in a split through every policy and commits one prediction per
(window, policy) pair - never a live session, never a write back to the daemon (task item 11).
`python3 -m bench_progress.replay --split dev` runs the two policies that need no model; add
`--kev prompts/v1.json` to also spend one Kev-4B call per window, under the shared advisory
lock (task items 15-ish infra, and the common acceptance criterion that one command reproduces
the experiment).
"""
from __future__ import annotations

import argparse
import json
import os
import time

from . import policy_baseline, policy_combine, policy_kev, policy_repetition
from .dataset import CASES_DIR, load_dataset
from .schema import Prediction, Window, dump_predictions

PRED_DIR = os.path.join(os.path.dirname(CASES_DIR), "predictions")


def _time_ms(fn):
    t0 = time.perf_counter()
    result = fn()
    return result, (time.perf_counter() - t0) * 1000.0


def replay_cheap_policies(windows: list[Window]) -> list[Prediction]:
    preds = []
    for w in windows:
        (decision, detail), latency = _time_ms(lambda: policy_baseline.decide(w.session))
        preds.append(Prediction(w.id, "baseline", decision, latency, detail))

        (rep_decision, rep_detail), latency = _time_ms(lambda: policy_repetition.decide(w.session, w.events))
        preds.append(Prediction(w.id, "repetition", rep_decision, latency, rep_detail))

        (combined_decision, combined_detail), latency = _time_ms(
            lambda: policy_combine.combine(w.session, rep_decision, rep_detail)
        )
        preds.append(Prediction(w.id, "combined_repetition", combined_decision, latency, combined_detail))
    return preds


def attach_kev_predictions(windows: list[Window], kev_raw_path: str, policy_name: str) -> list[Prediction]:
    """Turns a committed `worker/kev_infer.py` output file into `Prediction`s for the raw Kev
    policy and the policy that combines it with the baseline, under the integration rule in
    `policy_combine` (task item 15)."""
    raw_by_id = {}
    with open(kev_raw_path, encoding="utf-8") as fh:
        for line in fh:
            line = line.strip()
            if line:
                row = json.loads(line)
                raw_by_id[row["window_id"]] = row

    by_id = {w.id: w for w in windows}
    preds = []
    for wid, row in raw_by_id.items():
        w = by_id.get(wid)
        if w is None:
            continue
        advisory, detail = policy_kev.advisory_from_kev_answer(row.get("raw_choice"), row.get("ok", False))
        kev_decision = advisory if advisory is not None else "insufficient_evidence"
        preds.append(Prediction(wid, policy_name, kev_decision, row["latency_ms"], detail))
        combined_decision, combined_detail = policy_combine.combine(w.session, advisory, detail)
        preds.append(Prediction(wid, f"combined_{policy_name}", combined_decision, row["latency_ms"], combined_detail))
    return preds


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--split", choices=["dev", "eval", "both"], default="both")
    ap.add_argument("--kev-raw", help="a worker/kev_infer.py output file to fold in as predictions")
    ap.add_argument("--kev-policy-name", default="kev")
    args = ap.parse_args()

    os.makedirs(PRED_DIR, exist_ok=True)
    dev, ev = load_dataset()
    splits = {"dev": dev, "eval": ev} if args.split == "both" else {args.split: dev if args.split == "dev" else ev}

    for split_name, windows in splits.items():
        preds = replay_cheap_policies(windows)
        if args.kev_raw:
            preds += attach_kev_predictions(windows, args.kev_raw, args.kev_policy_name)
        out_path = os.path.join(PRED_DIR, f"{split_name}.jsonl")
        dump_predictions(preds, out_path)
        print(f"{split_name}: {len(preds)} predictions -> {out_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
