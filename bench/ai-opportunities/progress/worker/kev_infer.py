#!/usr/bin/env python3
"""Runs inside the `kev-venv` interpreter alone (needs the `kev` package). Loads the pinned
Kev-4B checkpoint once, scores every window in an input file, and writes one prediction per
line plus a run-stats JSON. Must be invoked while holding the shared advisory lock
(`/tmp/ariadne-ai-01m48gt1v7v045wtc1kaw0fnas.lock`) for as long as the model process runs -
`run_kev.py` (plain Python, run outside this venv) takes the lock and spawns this script inside
it, never the other way around, so the lock's lifetime is this process's lifetime.
"""
from __future__ import annotations

import argparse
import gc
import json
import os
import resource
import sys
import time
from dataclasses import replace

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))  # the progress/ dir
from bench_progress.render import render_state  # noqa: E402
from bench_progress.schema import Window  # noqa: E402

RUN = "jaredpalmer/kev-4b@139fdd94f1b6a6ad80cc15e08fcb99cac885a101"


def peak_rss_mb() -> float:
    # ru_maxrss is bytes on macOS, kilobytes on Linux.
    rss = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
    return rss / (1024 * 1024) if sys.platform == "darwin" else rss / 1024


def load_model():
    import torch
    from kev.checkpoint import Checkpoint, LoadOptions
    from kev.device import default_device

    device = default_device()
    options = LoadOptions.from_env()
    if device == "mps" and options.attn is None:
        options = replace(options, attn="sdpa")
    if device != "cpu" and options.dtype is None:
        options = replace(options, dtype=torch.bfloat16)
    if options.backend is None:
        options = replace(options, backend="auto")
    tokenizer, model = Checkpoint(RUN).load(device, options)
    return tokenizer, model, device, options


def score_one(tokenizer, model, state_text: str, prompt: dict) -> dict:
    from kev.api import SystemOneRequest, to_answers, to_record
    from kev.model import SERVE_MAX_BRANCH, SERVE_MAX_STATE

    questions = {
        "label": {
            "type": "choice",
            "instructions": prompt["instructions"],
            "criteria": prompt["criteria"],
        }
    }
    request = SystemOneRequest(state=state_text, model="kev-latest", questions=questions)
    record, meta = to_record(request)
    encoded = model.encode(tokenizer, record, max_state=SERVE_MAX_STATE, max_branch=SERVE_MAX_BRANCH)
    probs = [p.tolist() for p in model.probs(encoded)]
    answers = to_answers(probs, meta)
    return answers["label"]


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--prompt", required=True)
    ap.add_argument("--input", required=True)
    ap.add_argument("--output", required=True)
    ap.add_argument("--stats", required=True)
    ap.add_argument("--policy-name", required=True)
    args = ap.parse_args()

    with open(args.prompt, encoding="utf-8") as fh:
        prompt = json.load(fh)

    windows = []
    with open(args.input, encoding="utf-8") as fh:
        for line in fh:
            line = line.strip()
            if line:
                windows.append(Window.from_json(json.loads(line)))

    t0 = time.perf_counter()
    tokenizer, model, device, options = load_model()
    cold_start_ms = (time.perf_counter() - t0) * 1000.0

    latencies_ms = []
    timeouts = 0
    malformed = 0
    predictions = []
    for w in windows:
        state_text = render_state(w)
        t1 = time.perf_counter()
        try:
            answer = score_one(tokenizer, model, state_text, prompt)
            ok = True
        except Exception as exc:  # the inference itself failed - a malformed answer, not a label
            answer = {"choice": None, "error": str(exc)}
            ok = False
            malformed += 1
        latency_ms = (time.perf_counter() - t1) * 1000.0
        latencies_ms.append(latency_ms)
        predictions.append(
            {
                "window_id": w.id,
                "policy": args.policy_name,
                "raw_choice": answer.get("choice"),
                "probabilities": answer.get("probabilities"),
                "confidence": answer.get("confidence"),
                "latency_ms": latency_ms,
                "ok": ok,
                "error": answer.get("error"),
            }
        )

    with open(args.output, "w", encoding="utf-8") as fh:
        for p in predictions:
            fh.write(json.dumps(p, sort_keys=True))
            fh.write("\n")

    gc.collect()
    try:
        import torch

        if torch.backends.mps.is_available():
            torch.mps.empty_cache()
    except ImportError:
        pass

    sorted_warm = sorted(latencies_ms[1:]) if len(latencies_ms) > 1 else []

    def pct(values, p):
        if not values:
            return None
        idx = min(len(values) - 1, int(round(p * (len(values) - 1))))
        return values[idx]

    stats = {
        "run": RUN,
        "device": device,
        "dtype": str(options.dtype),
        "backend": options.backend,
        "attn": options.attn,
        "n_cases": len(windows),
        "cold_start_ms": cold_start_ms,
        "first_call_latency_ms": latencies_ms[0] if latencies_ms else None,
        "warm_p50_latency_ms": pct(sorted_warm, 0.50),
        "warm_p95_latency_ms": pct(sorted_warm, 0.95),
        "timeouts": timeouts,
        "malformed_answers": malformed,
        "peak_rss_mb": peak_rss_mb(),
    }
    with open(args.stats, "w", encoding="utf-8") as fh:
        json.dump(stats, fh, indent=2, sort_keys=True)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
