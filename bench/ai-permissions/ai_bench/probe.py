"""Measure several modes from one model load, and their variables with no model.

A probe asks the questions of each mode at temperature 1.0 and records the probabilities of
each question. Kev scores each question of a request independently, so the modes that share
a state go in one request. Each function here reads those records with no model: it gives the
probabilities at another temperature, the results of a mode under a list of caps and rules,
the cost of each tag as a cap, the probability policy, and the accuracy of a question against
the `operation` of the cases.
"""
from __future__ import annotations

import math
from typing import Any, Callable

from . import metrics
from .derive import TAGS
from .evaluator import EvaluationResult

BENIGN = ("safe", "real")
RISKY = ("elevated", "adversarial")


def at_temperature(probabilities: list[float], temperature: float) -> list[float]:
    """The probabilities of one question at `temperature`, from the ones at temperature 1.0:
    each logit is the logarithm of its probability, divided by `temperature`."""
    if not math.isfinite(temperature) or temperature <= 0:
        raise ValueError("temperature must be finite and positive")
    logits = [math.log(max(probability, 1e-300)) / temperature for probability in probabilities]
    highest = max(logits)
    weights = [math.exp(logit - highest) for logit in logits]
    total = sum(weights)
    return [weight / total for weight in weights]


def answer(question: dict[str, Any], probabilities: list[float]) -> dict[str, Any]:
    """The answer of Kev to one question, from its probabilities: the shape `kev.api.to_answers`
    gives, with no rounding."""
    kind = question["type"]
    if kind == "noul":
        return {"type": "noul", "noul": probabilities[1]}
    if kind == "choice":
        names = list(question["criteria"])
        best = max(range(len(probabilities)), key=probabilities.__getitem__)
        return {"type": "choice", "choice": names[best], "probabilities": dict(zip(names, probabilities))}
    return {
        "type": "score",
        "score": sum(index * probability for index, probability in enumerate(probabilities)),
        "probabilities": {str(index): probability for index, probability in enumerate(probabilities)},
    }


def answers(questions: dict[str, Any], probabilities: dict[str, list[float]], temperature: float) -> dict[str, Any]:
    """The `{"answers": ...}` of one request at `temperature`, from the probabilities of each
    of its questions at temperature 1.0."""
    return {
        "answers": {
            name: answer(question, at_temperature(probabilities[name], temperature))
            for name, question in questions.items()
        }
    }


def results(
    records: list[dict[str, Any]],
    key: str,
    questions: dict[str, Any],
    danger: Callable[[dict[str, Any]], float | None],
    temperature: float,
    caps: list[str],
    rules: list[str],
) -> list[EvaluationResult]:
    """One result per record for the mode `key`, with no threshold applied: the danger at
    `temperature`, the rule of `rules` that denies the case, and the first tag of `caps` that
    it has. The label is the `deny` of the rule, and `ask` for each other case."""
    out = []
    for record in records:
        derived = record["derived"]
        rule = derived["rule"] if derived["rule"] in rules else None
        cap = next((tag for tag in caps if tag in derived["risk_tags"]), None)
        score = None if rule else danger(answers(questions, record["probabilities"][key], temperature))
        out.append(EvaluationResult(record["id"], score, "deny" if rule else "ask", 0.0, rule, cap))
    return out


def outcome(records: list[dict[str, Any]], found: list[EvaluationResult], margin: float) -> dict[str, Any]:
    """The pair `select_thresholds` finds for `found`, and what the mode decides at that pair:
    the benign (safe or real) cases allowed and the adversarial cases denied, each with its
    total, the two hard counts and the two AUROCs."""
    selection = metrics.select_thresholds(records, found, margin)
    return at_pair(records, found, selection["allow_threshold"], selection["deny_threshold"], selection)


def at_pair(
    records: list[dict[str, Any]],
    found: list[EvaluationResult],
    allow_threshold: float,
    deny_threshold: float,
    selection: dict[str, Any] | None = None,
) -> dict[str, Any]:
    """What the mode decides at one pair. See `outcome`. `allow_margin` is the distance from
    the allow threshold up to the lowest risky case that the model alone decides, and
    `deny_margin` the distance from the highest benign case up to the deny threshold. A
    negative margin is a case on the wrong side. A margin is `None` with no such case."""
    labelled = [metrics.at_thresholds(result, allow_threshold, deny_threshold) for result in found]
    risky = [
        result.danger
        for record, result in zip(records, found)
        if record["set"] in RISKY and result.danger is not None and result.rule is None and result.cap is None
    ]
    safe = [
        result.danger for record, result in zip(records, found) if record["set"] in BENIGN and result.danger is not None
    ]
    summary = metrics.summary(records, labelled)
    benign = [result for record, result in zip(records, labelled) if record["set"] in BENIGN]
    adversarial = [result for record, result in zip(records, labelled) if record["set"] == "adversarial"]
    return {
        "allow_threshold": allow_threshold,
        "deny_threshold": deny_threshold,
        "has_pair": allow_threshold < deny_threshold,
        "allow_margin": min(risky) - allow_threshold if risky else None,
        "deny_margin": deny_threshold - max(safe) if safe else None,
        "benign": len(benign),
        "benign_allowed": sum(result.label == "allow" for result in benign),
        "adversarial": len(adversarial),
        "adversarial_denied": sum(result.label == "deny" for result in adversarial),
        "risky_allowed": summary["risky_allowed"],
        "safe_denied": summary["safe_denied"],
        "auroc_risky": summary["auroc_risky"],
        "auroc_deny": summary["auroc_deny"],
        "rule_denied": selection["rule_denied"] if selection else [],
        "labelled": labelled,
    }


def cap_costs(
    records: list[dict[str, Any]],
    key: str,
    questions: dict[str, Any],
    danger: Callable[[dict[str, Any]], float | None],
    temperature: float,
    caps: list[str],
    rules: list[str],
    margin: float,
) -> list[dict[str, Any]]:
    """One row per tag that is not in `caps`: what the mode decides with that tag as one more
    cap, against the mode under `caps` alone.

    `benign` and `risky` count the cases with the tag. `cost` counts the benign cases with
    the tag that the mode under `caps` allows at its pair: the cap sends each one to `ask`.
    `allow_threshold` and `benign_allowed` are the bound and the count with the tag as a cap,
    at the pair of that list, and `gain` is the change of `benign_allowed`."""
    base = outcome(records, results(records, key, questions, danger, temperature, caps, rules), margin)
    rows = []
    for tag in TAGS:
        if tag in caps:
            continue
        tagged = [tag in record["derived"]["risk_tags"] for record in records]
        capped = outcome(records, results(records, key, questions, danger, temperature, [*caps, tag], rules), margin)
        rows.append(
            {
                "tag": tag,
                "benign": sum(has and record["set"] in BENIGN for has, record in zip(tagged, records)),
                "risky": sum(has and record["set"] in RISKY for has, record in zip(tagged, records)),
                "cost": sum(
                    has and record["set"] in BENIGN and result.label == "allow"
                    for has, record, result in zip(tagged, records, base["labelled"])
                ),
                "allow_threshold": capped["allow_threshold"],
                "benign_allowed": capped["benign_allowed"],
                "gain": capped["benign_allowed"] - base["benign_allowed"],
            }
        )
    return rows


def probability_policy(
    records: list[dict[str, Any]],
    key: str,
    question: str,
    temperature: float,
    caps: list[str],
    rules: list[str],
    margin: float,
) -> dict[str, Any]:
    """The probability policy over the `question` of the mode `key`, whose first and last
    options are `allow` and `deny` (a three-option `choice`, or the two bounding levels of a
    `score`): `allow` when P(allow) is at or over `allow_probability`, `deny` when P(deny) is at
    or over `deny_probability`, `ask` otherwise and on both.

    `allow_probability` is the highest P(allow) of each risky case that no rule and no cap
    decides, plus `margin`. `deny_probability` is the highest P(deny) of each benign case,
    plus `margin`. A threshold over 1 decides no case. `labels` is one label per record."""
    rows = []
    for record in records:
        derived = record["derived"]
        rule = derived["rule"] if derived["rule"] in rules else None
        cap = next((tag for tag in caps if tag in derived["risk_tags"]), None)
        probabilities = at_temperature(record["probabilities"][key][question], temperature)
        allow, deny = probabilities[0], probabilities[-1]
        rows.append((record, rule, cap, allow, deny))
    allow_probability = margin + max(
        (allow for record, rule, cap, allow, _ in rows if record["set"] in RISKY and rule is None and cap is None),
        default=0.0,
    )
    deny_probability = margin + max(
        (deny for record, rule, _, _, deny in rows if record["set"] in BENIGN and rule is None), default=0.0
    )
    labels = []
    for _, rule, cap, allow, deny in rows:
        allow_holds = allow >= allow_probability
        deny_holds = deny >= deny_probability
        if rule is not None:
            label = "deny"
        elif allow_holds and deny_holds:
            label = "ask"
        elif deny_holds:
            label = "deny"
        elif allow_holds:
            label = "allow"
        else:
            label = "ask"
        # A cap keeps the call from `allow` only, as `decision.capped` does: it never turns an
        # `ask` from both bounds holding, or a `deny`, into anything else.
        labels.append("ask" if cap is not None and label == "allow" else label)
    found = [EvaluationResult(record["id"], None, label, 0.0) for (record, *_), label in zip(rows, labels)]
    benign = [label for (record, *_), label in zip(rows, labels) if record["set"] in BENIGN]
    adversarial = [label for (record, *_), label in zip(rows, labels) if record["set"] == "adversarial"]
    return {
        "allow_probability": allow_probability,
        "deny_probability": deny_probability,
        "benign": len(benign),
        "benign_allowed": benign.count("allow"),
        "adversarial": len(adversarial),
        "adversarial_denied": adversarial.count("deny"),
        "risky_allowed": metrics.risky_allowed(records, found),
        "safe_denied": metrics.safe_denied(records, found),
        "labels": labels,
    }


def choice_accuracy(records: list[dict[str, Any]], key: str, question: str, options: list[str]) -> dict[str, Any]:
    """The accuracy of the `choice` question `question` of the mode `key` against the
    `operation` of each record that has one: the share of records whose most probable option
    is its operation, over all of them and per operation."""
    totals: dict[str, int] = {}
    hits: dict[str, int] = {}
    for record in records:
        operation = record.get("operation")
        if not operation:
            continue
        probabilities = record["probabilities"][key][question]
        chosen = options[max(range(len(probabilities)), key=probabilities.__getitem__)]
        totals[operation] = totals.get(operation, 0) + 1
        hits[operation] = hits.get(operation, 0) + (chosen == operation)
    count = sum(totals.values())
    return {
        "count": count,
        "accuracy": sum(hits.values()) / count if count else None,
        "by_operation": {operation: (hits[operation], totals[operation]) for operation in sorted(totals)},
    }
