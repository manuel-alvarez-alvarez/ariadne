"""Recomputes every reported number from committed windows and committed predictions alone
(common acceptance criteria: a separate command recomputes metrics without a rerun). Nothing
here calls a model or reads a live session.
"""
from __future__ import annotations

from collections import defaultdict
from dataclasses import dataclass

from .schema import Decision, Prediction, Window

ESCALATIONS: tuple[Decision, ...] = ("escalate_stalled", "escalate_unproductive")


@dataclass
class PolicyMetrics:
    policy: str
    n: int
    false_escalations: int
    false_escalation_rate: float  # over useful_progress windows
    missed_loops: int
    missed_loop_rate: float  # over repetitive_failure windows
    coverage: float  # 1 - missed_loop_rate, recall over repetitive_failure
    missed_user_questions: int
    missed_user_question_rate: float  # over waiting_user_input windows
    insufficient_evidence_escalations: int
    insufficient_evidence_escalation_rate: float  # over insufficient_evidence windows
    mean_latency_ms: float
    p95_latency_ms: float
    model_calls: int  # one call per window this policy actually invoked a model for


def _rate(count: int, total: int) -> float:
    return count / total if total else 0.0


def _percentile(values: list[float], pct: float) -> float:
    if not values:
        return 0.0
    ordered = sorted(values)
    idx = min(len(ordered) - 1, int(round(pct * (len(ordered) - 1))))
    return ordered[idx]


def compute(
    windows: list[Window], predictions: list[Prediction], policy: str, model_calls: int | None = None
) -> PolicyMetrics:
    """`model_calls` is the real operational cost (task item 14): 0 for a model-free policy
    (`baseline`, `repetition`, `combined_repetition`, none of which ever call a model), and the
    number of Kev calls actually spent for a Kev-backed one. The caller must say which, since
    nothing in a `Prediction` row says whether answering it cost a model call or a microsecond
    of pure Python - `build_results.py` passes 0 explicitly for the model-free policies and the
    count of Kev calls for `kev`/`combined_kev` (the same calls: `combined_kev` spends no extra
    call beyond the `kev` prediction its own advisory reuses)."""
    by_id = {w.id: w for w in windows}
    preds = [p for p in predictions if p.policy == policy]
    n = len(preds)

    useful = [p for p in preds if by_id[p.window_id].label == "useful_progress"]
    looped = [p for p in preds if by_id[p.window_id].label == "repetitive_failure"]
    waiting = [p for p in preds if by_id[p.window_id].label == "waiting_user_input"]
    insufficient = [p for p in preds if by_id[p.window_id].label == "insufficient_evidence"]

    false_esc = sum(1 for p in useful if p.decision in ESCALATIONS)
    missed = sum(1 for p in looped if p.decision not in ESCALATIONS)
    # A "missed" question is an active, disruptive action taken over an explicit wait (a nudge
    # or an escalation) - not merely a decision that stops short of the "waiting_user" label,
    # since "continue" also leaves the person alone and is just as safe (task item 4).
    missed_q = sum(1 for p in waiting if p.decision in ("nudge",) + ESCALATIONS)
    insuff_esc = sum(1 for p in insufficient if p.decision in ESCALATIONS)

    latencies = [p.latency_ms for p in preds]

    return PolicyMetrics(
        policy=policy,
        n=n,
        false_escalations=false_esc,
        false_escalation_rate=_rate(false_esc, len(useful)),
        missed_loops=missed,
        missed_loop_rate=_rate(missed, len(looped)),
        coverage=1.0 - _rate(missed, len(looped)) if looped else 0.0,
        missed_user_questions=missed_q,
        missed_user_question_rate=_rate(missed_q, len(waiting)),
        insufficient_evidence_escalations=insuff_esc,
        insufficient_evidence_escalation_rate=_rate(insuff_esc, len(insufficient)),
        mean_latency_ms=(sum(latencies) / len(latencies)) if latencies else 0.0,
        p95_latency_ms=_percentile(latencies, 0.95),
        model_calls=model_calls if model_calls is not None else n,
    )


@dataclass
class DetectionDelay:
    family: str
    policy: str
    detected_at_elapsed_s: float | None  # None = never detected within the family's snapshots


def detection_delays(windows: list[Window], predictions: list[Prediction], policy: str) -> list[DetectionDelay]:
    """For every family of `repetitive_failure` windows tagged `multi_snapshot` (several
    decision points over time in the same loop), the earliest elapsed time at which `policy`
    first escalates. Only computed where timestamps are present on every snapshot (task item 12:
    "where timestamps permit")."""
    families: dict[str, list[Window]] = defaultdict(list)
    for w in windows:
        if w.label == "repetitive_failure" and "multi_snapshot" in w.tags:
            families[w.family].append(w)

    preds_by_window = {(p.window_id, p.policy): p for p in predictions}
    out = []
    for family, fam_windows in families.items():
        fam_windows = sorted(fam_windows, key=lambda w: w.elapsed_since_activity_s or 0.0)
        detected_at = None
        for w in fam_windows:
            pred = preds_by_window.get((w.id, policy))
            if pred and pred.decision in ESCALATIONS:
                detected_at = w.elapsed_since_activity_s
                break
        out.append(DetectionDelay(family=family, policy=policy, detected_at_elapsed_s=detected_at))
    return out


def model_call_counts(windows: list[Window]) -> dict[str, int]:
    """Model calls per replayed session/family (task item 14): one window is one call, so this
    is just how many windows exist per family, which is what an advisory rollout would spend
    per session in production."""
    counts: dict[str, int] = defaultdict(int)
    for w in windows:
        counts[w.family] += 1
    return dict(counts)
