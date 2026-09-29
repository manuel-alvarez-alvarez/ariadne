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
    """What one `evaluate` call decided: the danger score, 0 to 1 (None when the model gave no
    usable answer, which gives `ask`), and the label, `allow`, `ask` or `deny`. `rule` names
    the hard rule that denied the call with no model answer, and `cap` the tag that keeps the
    call from `allow`; each is `None` on a call that the model alone decides."""

    danger: float | None
    label: Label
    rule: str | None = None
    cap: str | None = None


@dataclass(frozen=True)
class EvaluationResult:
    """One evaluation as the runner records it: the case it was for and how long it took."""

    id: str
    danger: float | None
    label: Label
    latency_ms: float
    rule: str | None = None
    cap: str | None = None


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
