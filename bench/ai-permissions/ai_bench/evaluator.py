"""The evaluator contract: every way of scoring the cases is an `Evaluator` subclass.

An evaluator's life is `setup` once, `evaluate` once per case, then `teardown`. A backend
(`evaluators/kev`, `evaluators/laya`) implements `setup` and `teardown` once, loading and
releasing its model; each mode under it implements only `evaluate`, and registers itself
under a unique `key` (`ai_bench.registry.register`) so the runner can list and run it.
"""
from __future__ import annotations

from abc import ABC, abstractmethod
from dataclasses import dataclass
from typing import Any, ClassVar, Literal

Label = Literal["allow", "ask", "deny"]


class EvaluatorError(RuntimeError):
    """An evaluator could not produce the contracted result."""


@dataclass(frozen=True)
class Evaluation:
    """The score and label of one call. `safe` is P(true) for a noul mode."""

    danger: float | None
    label: Label
    safe: float | None = None
    p_allow: float | None = None
    p_deny: float | None = None


@dataclass(frozen=True)
class EvaluationResult:
    """One evaluation with its case id and latency."""

    id: str
    danger: float | None
    label: Label
    latency_ms: float
    safe: float | None = None
    p_allow: float | None = None
    p_deny: float | None = None


class Evaluator(ABC):
    """One way of deciding every case.

    `key` names a concrete evaluator uniquely, `backend` names the backend whose environment
    it runs in (the venv `run.sh` picks), and `description` is what `run.py list` prints.
    """

    key: ClassVar[str] = ""
    backend: ClassVar[str] = ""
    description: ClassVar[str] = ""

    @abstractmethod
    def setup(self) -> None:
        """Initialize the backend: load the model once, before any case."""

    @abstractmethod
    def evaluate(self, case: dict[str, Any]) -> Evaluation:
        """Decide one case."""

    @abstractmethod
    def teardown(self) -> None:
        """Stop the backend and release what `setup` loaded."""
