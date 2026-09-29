"""Turn one model answer into a danger score, and a danger score into an `Evaluation`.

An answer is the `{"answers": {"decision": ...}}` shape both backends return. Each `*_danger`
helper reads one kind of `decision` (`noul`, `score`, `choice`) and returns a danger score, 0
(safe) to 1 (dangerous), or `None` when the answer carries no usable decision of that kind.
`three_way` turns a danger score into the label a mode returns. `ruled` and `capped` are the
two deterministic parts of a decision: the `deny` of a hard rule, and no `allow` under a cap.
"""
from __future__ import annotations

import math
from typing import Any, Literal

from .evaluator import Evaluation

Polarity = Literal["true_is_danger", "false_is_danger"]


def _decision(answer: dict[str, Any] | None) -> dict[str, Any] | None:
    decision = (answer or {}).get("answers", {}).get("decision")
    return decision if isinstance(decision, dict) else None


def _is_finite_number(value: Any) -> bool:
    return isinstance(value, (int, float)) and not isinstance(value, bool) and math.isfinite(value)


def _is_probability(value: Any) -> bool:
    """A finite number in 0 to 1: what a probability, and a danger score, must be."""
    return _is_finite_number(value) and 0.0 <= value <= 1.0


def three_way(danger: float | None, allow_threshold: float, deny_threshold: float) -> Evaluation:
    """`allow` at or under `allow_threshold`, `deny` at or over `deny_threshold`, `ask`
    between them, and `ask` on a `danger` of `None`."""
    if danger is None:
        return Evaluation(None, "ask")
    if danger <= allow_threshold:
        return Evaluation(danger, "allow")
    if danger >= deny_threshold:
        return Evaluation(danger, "deny")
    return Evaluation(danger, "ask")


def ruled(rule: str) -> Evaluation:
    """The `deny` of a hard rule: the model is not asked, so there is no danger score."""
    return Evaluation(None, "deny", rule=rule)


def capped(evaluation: Evaluation, risk_tags: list[str], caps: list[str]) -> Evaluation:
    """`evaluation` under `caps`: a call with a tag of `caps` is never `allow`. Its `allow`
    becomes `ask`; its `ask` and its `deny` stay. `cap` is the first tag of `caps` that the
    call has."""
    cap = next((tag for tag in caps if tag in risk_tags), None)
    if cap is None:
        return evaluation
    label = "ask" if evaluation.label == "allow" else evaluation.label
    return Evaluation(evaluation.danger, label, cap=cap)


def decide(
    danger: float | None,
    allow_threshold: float,
    deny_threshold: float,
    rule: str | None = None,
    cap: str | None = None,
) -> Evaluation:
    """The label of one call: the `deny` of `rule`, else `three_way` of `danger`, with no
    `allow` under `cap`."""
    if rule is not None:
        return ruled(rule)
    evaluation = three_way(danger, allow_threshold, deny_threshold)
    return evaluation if cap is None else capped(evaluation, [cap], [cap])


def noul_danger(answer: dict[str, Any] | None, polarity: Polarity) -> float | None:
    """A `noul` answer's danger: `noul` is the probability that the question's statement
    holds, and `polarity` says which side of it is the dangerous one. A `noul` outside 0 to 1,
    non-finite, or missing makes the answer unusable."""
    decision = _decision(answer)
    if decision is None or not _is_probability(decision.get("noul")):
        return None
    p_true = float(decision["noul"])
    danger = p_true if polarity == "true_is_danger" else 1.0 - p_true
    return danger if _is_probability(danger) else None


def score_danger(answer: dict[str, Any] | None) -> float | None:
    """A `score` answer's danger: the expected level (`decision.score`, the levels' index
    weighted by their probability), divided by the last level's index, 0 to 1. A `score`
    outside 0 to the last level, a non-probability in `probabilities`, or a non-finite value,
    makes the answer unusable."""
    decision = _decision(answer)
    probabilities = decision.get("probabilities") if decision is not None else None
    if decision is None or not isinstance(probabilities, dict) or len(probabilities) < 2:
        return None
    if not all(_is_probability(probability) for probability in probabilities.values()):
        return None
    last_level = len(probabilities) - 1
    score = decision.get("score")
    if not _is_finite_number(score) or not (0.0 <= score <= last_level):
        return None
    danger = float(score) / last_level
    return danger if _is_probability(danger) else None


def choice_danger(answer: dict[str, Any] | None, weights: dict[str, float]) -> float | None:
    """A `choice` answer's danger: the sum of each option's probability times its weight (0
    to 1), from `weights`. An option the answer carries with no weight, a non-numeric weight,
    a non-finite or out-of-range probability, or a derived danger outside 0 to 1, makes the
    answer unusable."""
    decision = _decision(answer)
    if decision is None or "choice" not in decision:
        return None
    probabilities = decision.get("probabilities")
    if not isinstance(probabilities, dict) or not probabilities:
        return None
    danger = 0.0
    for label, probability in probabilities.items():
        if label not in weights or not _is_probability(probability) or not _is_finite_number(weights[label]):
            return None
        danger += float(probability) * float(weights[label])
    return danger if _is_probability(danger) else None
