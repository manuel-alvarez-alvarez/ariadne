"""Turns one committed Kev answer (a `label` choice, from `worker/kev_infer.py`) into the
`Decision` space every policy shares. Pure and model-free, so metrics recompute from the
committed JSONL without a rerun (common acceptance criterion).
"""
from __future__ import annotations

from .schema import Decision, Label

_LABEL_TO_DECISION: dict[Label, Decision] = {
    "useful_progress": "continue",
    "repetitive_failure": "escalate_unproductive",
    "waiting_user_input": "waiting_user",
    "insufficient_evidence": "insufficient_evidence",
}


def advisory_from_kev_answer(raw_choice: str | None, ok: bool) -> tuple[Decision | None, str]:
    """`None` is "no usable advisory signal": an inference failure, a malformed answer, or a
    choice outside the four labels. `policy_combine.combine` falls back to the baseline on it."""
    if not ok or raw_choice is None:
        return None, "kev inference failed or returned no answer"
    if raw_choice not in _LABEL_TO_DECISION:
        return None, f"kev returned an answer outside the four labels: {raw_choice!r}"
    return _LABEL_TO_DECISION[raw_choice], f"kev label: {raw_choice}"
