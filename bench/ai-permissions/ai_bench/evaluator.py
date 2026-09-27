"""The common interface for AI permission benchmark evaluators."""
from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Literal, Protocol

from . import decision as decision_mod
from . import model as model_mod

Label = Literal["allow", "escalate"]


class EvaluatorError(RuntimeError):
    """An evaluator could not return the contracted result for every case."""


@dataclass(frozen=True)
class EvaluationResult:
    """One evaluator decision for one benchmark case."""

    id: str
    allow_score: float | None
    label: Label
    latency_ms: float


class Evaluator(Protocol):
    def evaluate(
        self, config: dict[str, Any] | None, cases: list[dict[str, Any]]
    ) -> list[EvaluationResult]: ...


def _model_results(
    predictor: model_mod.Predictor | model_mod.KevPredictor,
    config: dict[str, Any] | None,
    cases: list[dict[str, Any]],
) -> list[EvaluationResult]:
    configured = dict(config or {})
    if not configured:
        raise EvaluatorError("this evaluator needs a configuration")
    raw_results = predictor.evaluate(configured, cases)
    if len(raw_results) != len(cases):
        raise EvaluatorError("evaluator returned %d results for %d cases" % (len(raw_results), len(cases)))
    results = []
    for case, raw in zip(cases, raw_results):
        decision = decision_mod.decide(configured, raw["answer"])
        results.append(
            EvaluationResult(
                id=case["id"],
                allow_score=decision.allow_score,
                label="allow" if decision.outcome == "allow" else "escalate",
                latency_ms=float(raw["latency_ms"]),
            )
        )
    return results


class LayaEvaluator:
    """Run Laya in process using the selected representation and question."""

    def __init__(self) -> None:
        self.predictor = model_mod.Predictor()

    def evaluate(self, config: dict[str, Any] | None, cases: list[dict[str, Any]]) -> list[EvaluationResult]:
        return _model_results(self.predictor, config, cases)


class KevEvaluator:
    """Run Kev in process using the selected representation and question."""

    def __init__(self) -> None:
        self.predictor = model_mod.KevPredictor()

    def evaluate(self, config: dict[str, Any] | None, cases: list[dict[str, Any]]) -> list[EvaluationResult]:
        return _model_results(self.predictor, config, cases)


def make_evaluator(name: str) -> Evaluator:
    if name == "laya":
        return LayaEvaluator()
    if name == "kev":
        return KevEvaluator()
    raise EvaluatorError("unknown evaluator %r (choose laya or kev)" % name)
