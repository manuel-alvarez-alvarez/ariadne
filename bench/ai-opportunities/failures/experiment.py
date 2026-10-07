#!/usr/bin/env python3
"""Measure error diagnosis policies without reading the daemon source at runtime."""
from __future__ import annotations

import argparse
import json
import math
import os
import platform
import resource
import statistics
import subprocess
import sys
import time
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parent
RUN = "jaredpalmer/kev-4b@139fdd94f1b6a6ad80cc15e08fcb99cac885a101"
PATTERNS = ("hit your usage limit", "usage limit reached", "usage limit", "rate limit", "quota", "try again at")
LABELS = ("exhausted", "temporary", "auth_config", "task_error", "insufficient")
RECOVERY = {"exhausted": "switch_model", "temporary": "retry", "auth_config": "fix_configuration", "task_error": "task_action", "insufficient": "abstain"}
QUESTIONS = {"diagnosis": {"type": "choice", "instructions": "Classify this coding agent error from its explicit evidence. Do not treat a quoted example, a negated claim, or token accounting as resource exhaustion.", "criteria": {
    "exhausted": "The model or account cannot continue because its usage, credit, quota, or rate allocation is exhausted.",
    "temporary": "The request can reasonably be retried because the service has a temporary fault, overload, timeout, or transient limit.",
    "auth_config": "Credentials, permissions, account setup, model setup, or configuration must be corrected.",
    "task_error": "The task, request, tool input, repository state, or command failed and needs task-specific correction.",
    "insufficient": "The payload has no reliable evidence for one diagnosis.",
}}}

def read_cases(path: Path) -> list[dict[str, Any]]:
    return [json.loads(line) for line in path.read_text().splitlines() if line]

def baseline(case: dict[str, Any]) -> bool:
    data = case.get("data") or {}
    if data.get("codexErrorInfo") == "usageLimitExceeded": return True
    if data.get("_meta", {}).get("jetbrains", {}).get("air", {}).get("sessionFailure", {}).get("category") == "limit": return True
    text = case["message"].lower()
    return any(pattern in text for pattern in PATTERNS)

def rules(case: dict[str, Any]) -> str | None:
    data = case.get("data") or {}
    if data.get("codexErrorInfo") == "usageLimitExceeded": return "exhausted"
    if data.get("_meta", {}).get("jetbrains", {}).get("air", {}).get("sessionFailure", {}).get("category") == "limit": return "exhausted"
    text = case["message"].lower()
    if any(word in text for word in ("not a rate limit", "not quota exhaustion", "quota report", "token quota", "example error", "quoted error")):
        return None
    if any(word in text for word in ("usage limit reached", "credits are depleted", "monthly allowance exhausted", "rate allocation exhausted")):
        return "exhausted"
    if any(word in text for word in ("invalid api key", "unauthorized", "forbidden", "model is not configured")):
        return "auth_config"
    if any(word in text for word in ("temporarily unavailable", "gateway timeout", "connection reset", "overloaded")):
        return "temporary"
    return None

def kev_predict(model: Any, tokenizer: Any, case: dict[str, Any]) -> tuple[str | None, dict[str, float] | None]:
    from kev.api import SystemOneRequest, to_record
    from kev.model import SERVE_MAX_BRANCH, SERVE_MAX_STATE
    state = {"error_message": case["message"], "error_data": case.get("data") or {}}
    record, meta = to_record(SystemOneRequest(state=state, model="kev-4b", questions=QUESTIONS))
    probabilities = model.probs(model.encode(tokenizer, record, max_state=SERVE_MAX_STATE, max_branch=SERVE_MAX_BRANCH))[0].tolist()
    if len(probabilities) != len(LABELS) or not all(math.isfinite(p) for p in probabilities): return None, None
    distribution = dict(zip(LABELS, map(float, probabilities)))
    return max(distribution, key=distribution.__getitem__), distribution

def percentile(values: list[float], q: float) -> float:
    if not values: return 0.0
    ordered = sorted(values); index = max(0, min(len(ordered) - 1, math.ceil(q * len(ordered)) - 1))
    return ordered[index]

def policy(case: dict[str, Any], kev_label: str | None, variant: str) -> tuple[str, str]:
    if variant == "baseline": return ("exhausted" if baseline(case) else "insufficient", "baseline")
    if variant == "kev": return (kev_label or "insufficient", "kev" if kev_label else "abstain")
    fixed = rules(case)
    if fixed: return fixed, "rules"
    if kev_label: return kev_label, "kev"
    return ("exhausted" if baseline(case) else "insufficient", "baseline_fallback")

def metric(rows: list[dict[str, Any]], variant: str) -> dict[str, Any]:
    items = [row["policies"][variant] for row in rows]
    truth = [row["label"] for row in rows]
    predicted = [item["label"] for item in items]
    tp = sum(a == b == "exhausted" for a, b in zip(truth, predicted)); fp = sum(a != "exhausted" and b == "exhausted" for a, b in zip(truth, predicted)); fn = sum(a == "exhausted" and b != "exhausted" for a, b in zip(truth, predicted))
    precision = tp / (tp + fp) if tp + fp else None; recall = tp / (tp + fn) if tp + fn else None
    wrong_switch = sum(item["recovery"] == "switch_model" and row["recovery"] != "switch_model" for row, item in zip(rows, items))
    missed = fn
    confusion = {actual: {guess: sum(a == actual and p == guess for a, p in zip(truth, predicted)) for guess in LABELS} for actual in LABELS}
    abstain = sum(item["label"] == "insufficient" for item in items)
    return {"count": len(rows), "exhaustion_precision": precision, "exhaustion_recall": recall, "wrong_switch_recommendations": wrong_switch, "missed_exhaustion": missed, "abstention_coverage": abstain / len(rows), "class_confusion": confusion}

def package_revision(python: str) -> str:
    program = "import importlib.metadata; print(importlib.metadata.distribution('kev').read_text('direct_url.json') or '')"
    return subprocess.check_output([python, "-c", program], text=True).strip()

def measure(args: argparse.Namespace) -> None:
    cases = read_cases(Path(args.cases)); output = Path(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    start = time.perf_counter()
    from kev.checkpoint import Checkpoint, LoadOptions
    from kev.device import default_device
    checkpoint = Checkpoint(RUN); device = default_device()
    tokenizer, model = checkpoint.load(device, LoadOptions(backend="auto"))
    cold_ms = (time.perf_counter() - start) * 1000
    rows = []; times = []; malformed = 0
    for case in cases:
        started = time.perf_counter(); label, probabilities = kev_predict(model, tokenizer, case); elapsed = (time.perf_counter() - started) * 1000
        times.append(elapsed); malformed += label is None
        policies = {}
        for variant in ("baseline", "kev", "rules_first_kev"):
            predicted, source = policy(case, label, variant)
            policies[variant] = {"label": predicted, "recovery": RECOVERY[predicted], "source": source}
        rows.append({**case, "kev": {"label": label, "probabilities": probabilities, "latency_ms": elapsed}, "policies": policies})
    peak = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
    peak_bytes = peak if sys.platform == "darwin" else peak * 1024
    source_commit = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT.parents[2], text=True).strip()
    results = {"area": "agent_failure_diagnosis", "run": {"package_revision": package_revision(sys.executable), "checkpoint": RUN, "device": str(device), "precision": "default checkpoint settings; backend=auto", "source_commit": source_commit, "isolated_hf_home": os.environ.get("HF_HOME"), "cold_start_ms": cold_ms, "warm_latency_ms": {"count": len(times), "p50": percentile(times, .50), "p95": percentile(times, .95)}, "malformed_answers": malformed, "peak_memory_method": "resource.getrusage(RUSAGE_SELF).ru_maxrss; macOS reports bytes and Linux reports KiB", "peak_memory_bytes": peak_bytes, "commands": ["python3 make_cases.py", "HF_HOME=/Users/malvarez/.ariadne/ai-opportunities/hf python3 lock_run.py /Users/malvarez/.ariadne/ai-permissions/venv/bin/python3 experiment.py measure", "/Users/malvarez/.ariadne/ai-permissions/venv/bin/python3 experiment.py recompute"]}, "dataset": {"split_sizes": dict(Counter(case["split"] for case in cases)), "provenance": dict(Counter(case["provenance"] for case in cases)), "label_rules": "Labels use the case evidence and rationale before model scoring."}, "variants": {"baseline": "Current structured fields plus configured default substring patterns.", "kev": "Kev-4B diagnosis choice question.", "rules_first_kev": "Protocol and explicit rules first, then Kev; use the current baseline only after no Kev answer."}, "metrics": {split: {name: metric([row for row in rows if row["split"] == split], name) for name in ("baseline", "kev", "rules_first_kev")} for split in ("dev", "eval")}, "limitations": ["All committed cases are synthetic; no private transcript was available.", "This measurement does not execute proposed recoveries.", "Results apply only to this local package, checkpoint, device, and case set."], "recommendation": "investigate_further"}
    output.write_text(json.dumps(results, indent=2) + "\n")
    Path(args.predictions).write_text("\n".join(json.dumps(row) for row in rows) + "\n")

def recompute(args: argparse.Namespace) -> None:
    rows = read_cases(Path(args.predictions)); results = json.loads(Path(args.results).read_text())
    expected = {split: {name: metric([row for row in rows if row["split"] == split], name) for name in ("baseline", "kev", "rules_first_kev")} for split in ("dev", "eval")}
    if results["metrics"] != expected: raise SystemExit("metrics do not match predictions")
    print("metrics match predictions")

def main() -> None:
    parser = argparse.ArgumentParser(); sub = parser.add_subparsers(required=True)
    for name in ("measure", "recompute"):
        command = sub.add_parser(name); command.add_argument("--cases", default=str(ROOT / "cases.jsonl")); command.add_argument("--predictions", default=str(ROOT / "predictions.jsonl")); command.add_argument("--results", default=str(ROOT / "results.json")); command.add_argument("--output", default=str(ROOT / "results.json")); command.set_defaults(func=measure if name == "measure" else recompute)
    args = parser.parse_args(); args.func(args)

if __name__ == "__main__": main()
