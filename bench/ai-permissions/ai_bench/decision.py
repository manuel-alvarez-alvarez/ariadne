"""Turn one model answer into an `Evaluation`: an allow score and a label.

An answer is the `{"answers": {"decision": ...}}` shape both backends return. One that
carries no usable decision has no allow score and escalates.
"""
from __future__ import annotations

from typing import Any, Literal

from .evaluator import Evaluation

Polarity = Literal["true_is_allow", "false_is_allow"]


def _decision(answer: dict[str, Any] | None) -> dict[str, Any] | None:
    decision = (answer or {}).get("answers", {}).get("decision")
    return decision if isinstance(decision, dict) else None


def noul(answer: dict[str, Any] | None, threshold: float, polarity: Polarity) -> Evaluation:
    """A `noul` answer: `noul` is the probability that the question's statement holds, and
    `polarity` says which side of it allows. It allows when that side is the likelier one
    and its probability, the allow score, meets `threshold`."""
    decision = _decision(answer)
    if decision is None or not isinstance(decision.get("noul"), (int, float)):
        return Evaluation(allow_score=None, label="escalate")
    p_true = float(decision["noul"])
    allow_score = p_true if polarity == "true_is_allow" else 1.0 - p_true
    allowed = allow_score > 0.5 or (allow_score == 0.5 and polarity == "true_is_allow")
    return Evaluation(allow_score, "allow" if allowed and allow_score >= threshold else "escalate")


def choice(answer: dict[str, Any] | None, allow_label: str, threshold: float) -> Evaluation:
    """A `choice` answer: it allows when `allow_label` is the chosen label and the chosen
    label's confidence (`answer_confidence`, else its own probability) meets `threshold`. The
    allow score is `allow_label`'s probability."""
    decision = _decision(answer)
    if decision is None or "choice" not in decision:
        return Evaluation(allow_score=None, label="escalate")
    probabilities: dict[str, float] = decision.get("probabilities", {})
    chosen = decision["choice"]
    confidence = decision.get("answer_confidence")
    if confidence is None:
        confidence = probabilities.get(chosen)
    allowed = chosen == allow_label and confidence is not None and confidence >= threshold
    return Evaluation(probabilities.get(allow_label, 0.0), "allow" if allowed else "escalate")
