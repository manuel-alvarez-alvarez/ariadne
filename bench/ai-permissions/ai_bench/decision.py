"""Turn one model answer (or a guardrail hit) into an allow score and a decision."""
from __future__ import annotations

from typing import Any, NamedTuple


class Decision(NamedTuple):
    allow_score: float
    chosen_label: str | None
    answer_confidence: float | None
    outcome: str  # "allow" | "escalate" | "deny"


def _choice_decision(question_cfg: dict[str, Any], decision_answer: dict[str, Any], threshold: float) -> Decision:
    probabilities: dict[str, float] = decision_answer.get("probabilities", {})
    chosen = decision_answer.get("choice")
    # decide.rs gates on this same value: answer_confidence, or the chosen label's own
    # probability when answer_confidence is absent. It is the chosen label's confidence,
    # which is only the allow label's probability when chosen == allow_label; gating on
    # allow_score instead would accept an allow decide.rs would have escalated (or the
    # reverse) whenever a serving backend's answer_confidence departs from the raw softmax.
    confidence = decision_answer.get("answer_confidence")
    if confidence is None:
        confidence = probabilities.get(chosen)
    allow_label = question_cfg["allow_label"]
    deny_label = question_cfg.get("deny_label")
    allow_score = probabilities.get(allow_label, 0.0)

    outcome = "escalate"
    if chosen == allow_label and confidence is not None and confidence >= threshold:
        outcome = "allow"
    elif deny_label and chosen == deny_label and confidence is not None and confidence >= threshold:
        outcome = "deny"
    return Decision(allow_score, chosen, confidence, outcome)


def _noul_decision(question_cfg: dict[str, Any], decision_answer: dict[str, Any], threshold: float) -> Decision:
    p_true = decision_answer.get("noul", 0.0)
    answer_confidence = decision_answer.get("answer_confidence")
    if answer_confidence is None:
        answer_confidence = decision_answer.get("confidence")
    if answer_confidence is None:
        # Kev's noul answer ({"type": "noul", "noul": p}) carries neither field: derive the
        # two-outcome confidence directly from p, the same way answer_confidence would report it.
        answer_confidence = max(p_true, 1.0 - p_true)
    polarity = question_cfg.get("polarity", "true_is_allow")
    if polarity == "true_is_allow":
        allow_score = p_true
        predicted_allow = p_true >= 0.5
    elif polarity == "false_is_allow":
        allow_score = 1.0 - p_true
        predicted_allow = p_true < 0.5
    else:
        raise ValueError("unknown polarity %r" % polarity)
    chosen_label = "allow" if predicted_allow else "escalate"
    outcome = "allow" if (predicted_allow and allow_score >= threshold) else "escalate"
    return Decision(allow_score, chosen_label, answer_confidence, outcome)


def decide(
    config: dict[str, Any],
    guardrail: str | None,
    answer: dict[str, Any] | None,
    threshold: float | None = None,
) -> Decision:
    """The decision for one case: `answer` is the laya result (None when a guardrail fired).

    `threshold` overrides `config["threshold"]`, so a threshold sweep can reuse one stored
    answer instead of calling the model again for every threshold.
    """
    if guardrail is not None:
        return Decision(allow_score=0.0, chosen_label=None, answer_confidence=None, outcome="escalate")
    question_cfg = config["question"]
    decision_answer = (answer or {}).get("answers", {}).get("decision")
    if not isinstance(decision_answer, dict) or (
        question_cfg["type"] == "choice" and "choice" not in decision_answer
    ) or (question_cfg["type"] == "noul" and "noul" not in decision_answer):
        return Decision(allow_score=0.0, chosen_label=None, answer_confidence=None, outcome="escalate")
    threshold = config["threshold"] if threshold is None else threshold
    if question_cfg["type"] == "choice":
        return _choice_decision(question_cfg, decision_answer, threshold)
    return _noul_decision(question_cfg, decision_answer, threshold)
