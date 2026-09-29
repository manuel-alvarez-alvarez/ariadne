"""`kev_v23`: `kev_v18` with the operation hint in the state and no tag.

The state carries `derived.operation_hint` only. This mode measures the hint alone.
"""
from __future__ import annotations

import sys
from typing import Any

from ai_bench import representations
from ai_bench.derive import derive
from ai_bench.evaluator import Evaluation
from ai_bench.registry import register

from . import KevEvaluator, evaluate_contract
from .kev_v15 import RULES  # noqa: F401, a part of the contract
from .kev_v18 import QUESTIONS, danger  # noqa: F401, a part of the contract

FACTS = ("operation_hint",)
RUN = KevEvaluator.run
TEMPERATURE = 1.5
ALLOW_THRESHOLD = 0.1179
DENY_THRESHOLD = 0.602
CAPS: list[str] = []


def state(request: dict[str, Any], workspace: str | None) -> dict[str, Any]:
    """The normalized state with the operation hint."""
    return representations.build_normalized(request, workspace, derive(request, workspace), facts=FACTS)


@register
class KevV23(KevEvaluator):
    key = "kev_v23"
    description = "kev_v18 with the operation hint in the state and no tag"
    run = RUN
    temperature = TEMPERATURE

    def evaluate(self, case: dict[str, Any]) -> Evaluation:
        return evaluate_contract(self, sys.modules[__name__], case)
