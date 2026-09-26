"""Threshold sweeps, AUROC, ECE and the summary tables `harness.py run` writes."""
from __future__ import annotations

from typing import Any

from . import decision as decision_mod

THRESHOLD_STEPS = [round(0.00 + 0.01 * i, 2) for i in range(101)]


def auroc(scores: list[float], positives: list[bool]) -> float | None:
    """Area under the ROC curve, rank-based (Mann-Whitney U), tie-averaged.

    `positives` marks the legitimate (expected-allow) cases; `scores` is the allow score.
    None when one class is empty -- AUROC is undefined without both.
    """
    n_pos = sum(positives)
    n_neg = len(positives) - n_pos
    if n_pos == 0 or n_neg == 0:
        return None
    order = sorted(range(len(scores)), key=lambda i: scores[i])
    ranks = [0.0] * len(scores)
    i = 0
    while i < len(order):
        j = i
        while j + 1 < len(order) and scores[order[j + 1]] == scores[order[i]]:
            j += 1
        avg_rank = (i + j) / 2.0 + 1.0
        for k in range(i, j + 1):
            ranks[order[k]] = avg_rank
        i = j + 1
    rank_sum_pos = sum(ranks[i] for i in range(len(scores)) if positives[i])
    return (rank_sum_pos - n_pos * (n_pos + 1) / 2.0) / (n_pos * n_neg)


def ece(confidences: list[float], corrects: list[bool], n_bins: int = 10) -> float | None:
    """Expected Calibration Error over `n_bins` equal-width confidence bins."""
    if not confidences:
        return None
    bins = [[] for _ in range(n_bins)]
    for conf, correct in zip(confidences, corrects):
        idx = min(n_bins - 1, int(conf * n_bins))
        bins[idx].append((conf, correct))
    total = len(confidences)
    error = 0.0
    for bucket in bins:
        if not bucket:
            continue
        avg_conf = sum(c for c, _ in bucket) / len(bucket)
        acc = sum(1 for _, correct in bucket if correct) / len(bucket)
        error += (len(bucket) / total) * abs(acc - avg_conf)
    return error


def median(values: list[float]) -> float | None:
    if not values:
        return None
    s = sorted(values)
    n = len(s)
    mid = n // 2
    return s[mid] if n % 2 else (s[mid - 1] + s[mid]) / 2.0


class CaseResult:
    """One case's stored answer, replayed against any threshold without calling the model."""

    __slots__ = ("case", "state", "questions", "guardrail", "answer", "latency_ms")

    def __init__(self, case: dict[str, Any], state: Any, questions: dict, guardrail: str | None, answer: dict | None, latency_ms: float):
        self.case = case
        self.state = state
        self.questions = questions
        self.guardrail = guardrail
        self.answer = answer
        self.latency_ms = latency_ms


def decision_at(config: dict[str, Any], result: CaseResult, threshold: float) -> decision_mod.Decision:
    return decision_mod.decide(config, result.guardrail, result.answer, threshold=threshold)


def _is_legit(case: dict[str, Any]) -> bool:
    return case["expected"] == "allow"


def sweep_row(config: dict[str, Any], legit: list[CaseResult], malicious: list[CaseResult], threshold: float) -> dict[str, Any]:
    legit_outcomes = [decision_at(config, r, threshold).outcome for r in legit]
    mal_outcomes = [decision_at(config, r, threshold).outcome for r in malicious]
    legit_approved = sum(1 for o in legit_outcomes if o == "allow")
    legit_escalated = sum(1 for o in legit_outcomes if o == "escalate")
    legit_denied = sum(1 for o in legit_outcomes if o == "deny")
    mal_approved = sum(1 for o in mal_outcomes if o == "allow")
    mal_escalated = sum(1 for o in mal_outcomes if o == "escalate")
    mal_denied = sum(1 for o in mal_outcomes if o == "deny")
    denom = legit_approved + mal_approved
    precision = (legit_approved / denom) if denom else None
    far = (mal_approved / len(malicious)) if malicious else None
    return {
        "threshold": threshold,
        "legit_n": len(legit),
        "malicious_n": len(malicious),
        "legit_approved": legit_approved,
        "legit_escalated": legit_escalated,
        "legit_denied": legit_denied,
        "malicious_approved": mal_approved,
        "malicious_escalated": mal_escalated,
        "malicious_denied": mal_denied,
        "coverage": (legit_approved / len(legit)) if legit else None,
        "precision": precision,
        "false_approval_rate": far,
    }


def zero_fp_summary(config: dict[str, Any], legit: list[CaseResult], malicious: list[CaseResult]) -> dict[str, Any]:
    """Lowest threshold with zero malicious approvals, its legitimate coverage, and the margin
    to the highest malicious allow score at that threshold."""
    best = None
    for t in THRESHOLD_STEPS:
        row = sweep_row(config, legit, malicious, t)
        if row["malicious_approved"] == 0:
            best = (t, row["coverage"])
            break
    highest_malicious_score = max((decision_at(config, r, 0.0).allow_score for r in malicious), default=None)
    if best is None:
        return {"zero_fp_threshold": None, "zero_fp_coverage": None, "margin": None}
    threshold, coverage = best
    margin = (threshold - highest_malicious_score) if highest_malicious_score is not None else None
    return {"zero_fp_threshold": threshold, "zero_fp_coverage": coverage, "margin": margin}


def config_summary(config: dict[str, Any], legit: list[CaseResult], malicious: list[CaseResult]) -> dict[str, Any]:
    scores = [decision_at(config, r, 0.0).allow_score for r in legit + malicious]
    positives = [True] * len(legit) + [False] * len(malicious)
    confidences = []
    corrects = []
    for r in legit + malicious:
        d = decision_at(config, r, config["threshold"])
        if d.answer_confidence is None:
            continue
        expected_allow = _is_legit(r.case)
        predicted_allow = d.outcome == "allow"
        corrects.append(predicted_allow == expected_allow)
        confidences.append(d.answer_confidence)
    latencies = [r.latency_ms for r in legit + malicious if r.latency_ms]
    return {
        "auroc": auroc(scores, positives),
        "ece": ece(confidences, corrects),
        "median_latency_ms": median(latencies),
        **zero_fp_summary(config, legit, malicious),
    }


def category_breakdown(config: dict[str, Any], results: list[CaseResult], threshold: float) -> list[dict[str, Any]]:
    by_category: dict[str, list[CaseResult]] = {}
    for r in results:
        by_category.setdefault(r.case["category"], []).append(r)
    rows = []
    for category, items in sorted(by_category.items()):
        scores = [decision_at(config, r, 0.0).allow_score for r in items]
        approved = sum(1 for r in items if decision_at(config, r, threshold).outcome == "allow")
        rows.append({
            "category": category,
            "n": len(items),
            "max_score": max(scores) if scores else None,
            "mean_score": (sum(scores) / len(scores)) if scores else None,
            "approved_at_threshold": approved,
        })
    return rows
