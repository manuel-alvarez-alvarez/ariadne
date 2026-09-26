"""The common interface for AI permission benchmark evaluators."""
from __future__ import annotations

import json
import subprocess
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Literal, Protocol, Sequence

from . import decision as decision_mod
from . import model as model_mod

Label = Literal["allow", "escalate"]
REPOSITORY_ROOT = Path(__file__).resolve().parents[3]


class EvaluatorError(RuntimeError):
    """An evaluator could not return the contracted result for every case."""


@dataclass(frozen=True)
class EvaluationResult:
    """One evaluator decision for one benchmark case."""

    id: str
    allow_score: float | None
    label: Label
    guardrail: str | None
    latency_ms: float


class Evaluator(Protocol):
    def evaluate(
        self, config: dict[str, Any] | None, cases: list[dict[str, Any]]
    ) -> list[EvaluationResult]: ...


def _configured(config: dict[str, Any] | None) -> dict[str, Any]:
    configured = dict(config or {})
    guardrails = configured.get("guardrails")
    if guardrails and not Path(guardrails).is_absolute():
        configured["guardrails"] = str(REPOSITORY_ROOT / guardrails)
    return configured


def _model_results(
    predictor: model_mod.Predictor | model_mod.KevPredictor,
    config: dict[str, Any] | None,
    cases: list[dict[str, Any]],
) -> list[EvaluationResult]:
    configured = _configured(config)
    if not configured:
        raise EvaluatorError("this evaluator needs a configuration")
    raw_results = predictor.evaluate(configured, cases)
    if len(raw_results) != len(cases):
        raise EvaluatorError("evaluator returned %d results for %d cases" % (len(raw_results), len(cases)))
    results = []
    for case, raw in zip(cases, raw_results):
        decision = decision_mod.decide(configured, raw["guardrail"], raw["answer"])
        results.append(
            EvaluationResult(
                id=case["id"],
                allow_score=None if raw["guardrail"] is not None else decision.allow_score,
                label="allow" if decision.outcome == "allow" else "escalate",
                guardrail=raw["guardrail"],
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


class AriadneEvaluator:
    """Run the daemon's `ai_permission_eval` example and parse its JSON Lines output."""

    def __init__(self, command: Sequence[str] | None = None) -> None:
        self.command = list(command) if command is not None else [
            "cargo", "run", "-q", "-p", "ariadne-daemon", "--example", "ai_permission_eval", "--",
        ]

    def evaluate(self, config: dict[str, Any] | None, cases: list[dict[str, Any]]) -> list[EvaluationResult]:
        configured = config or {}
        command = list(self.command)
        endpoint = configured.get("endpoint")
        threshold = configured.get("threshold")
        if endpoint is not None:
            command.extend(["--endpoint", str(endpoint)])
        if threshold is not None:
            command.extend(["--threshold", str(threshold)])
        input_lines = "".join(json.dumps(case, ensure_ascii=False) + "\n" for case in cases)
        try:
            completed = subprocess.run(
                command, input=input_lines, text=True, capture_output=True, cwd=REPOSITORY_ROOT, check=False
            )
        except OSError as exc:
            raise EvaluatorError("could not start ariadne evaluator: %s" % exc) from exc
        if completed.returncode:
            detail = completed.stderr.strip() or completed.stdout.strip() or "no error output"
            raise EvaluatorError("ariadne evaluator failed: %s" % detail)
        return _parse_ariadne_output(completed.stdout, cases)


def _parse_ariadne_output(output: str, cases: list[dict[str, Any]]) -> list[EvaluationResult]:
    lines = [line for line in output.splitlines() if line.strip()]
    if len(lines) != len(cases):
        raise EvaluatorError("ariadne evaluator returned %d results for %d cases" % (len(lines), len(cases)))
    results = []
    for line, case in zip(lines, cases):
        try:
            value = json.loads(line)
        except json.JSONDecodeError as exc:
            raise EvaluatorError("ariadne evaluator wrote invalid JSON: %s" % exc) from exc
        if not isinstance(value, dict):
            raise EvaluatorError("ariadne evaluator wrote a non-object result")
        if value.get("id") != case["id"]:
            raise EvaluatorError("ariadne evaluator returned results out of input order")
        label = value.get("label")
        if label not in ("allow", "escalate"):
            raise EvaluatorError("ariadne evaluator returned invalid label %r" % label)
        score = value.get("allow_score")
        if score is not None and (isinstance(score, bool) or not isinstance(score, (int, float))):
            raise EvaluatorError("ariadne evaluator returned invalid allow_score")
        guardrail = value.get("guardrail")
        if guardrail is not None and not isinstance(guardrail, str):
            raise EvaluatorError("ariadne evaluator returned invalid guardrail")
        latency = value.get("latency_ms")
        if isinstance(latency, bool) or not isinstance(latency, (int, float)):
            raise EvaluatorError("ariadne evaluator returned invalid latency_ms")
        results.append(EvaluationResult(case["id"], None if score is None else float(score), label, guardrail, float(latency)))
    return results


def make_evaluator(name: str) -> Evaluator:
    if name == "laya":
        return LayaEvaluator()
    if name == "kev":
        return KevEvaluator()
    if name == "ariadne":
        return AriadneEvaluator()
    raise EvaluatorError("unknown evaluator %r (choose laya, kev, or ariadne)" % name)
