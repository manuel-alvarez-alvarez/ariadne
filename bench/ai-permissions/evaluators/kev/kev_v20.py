"""`kev_v20`: `kev_v18` over a state with no derived fact.

The question and the rules are `kev_v18`'s, at the best temperature of this state. The state is `kev_v14`'s:
this mode measures what the derived facts in the state give under the question of `kev_v18`.
"""
from __future__ import annotations

import sys
from typing import Any

from ai_bench.evaluator import Evaluation
from ai_bench.registry import register

from . import KevEvaluator, evaluate_contract
from .kev_v14 import state  # noqa: F401, a part of the contract
from .kev_v15 import RULES  # noqa: F401, a part of the contract
from .kev_v18 import QUESTIONS, danger  # noqa: F401, a part of the contract

RUN = KevEvaluator.run
TEMPERATURE = 1.0
ALLOW_THRESHOLD = 0.0476
DENY_THRESHOLD = 0.6321
CAPS: list[str] = []


@register
class KevV20(KevEvaluator):
    key = "kev_v20"
    description = "kev_v18 over kev_v14's state: the same question and rules, no derived fact in the state"
    run = RUN
    temperature = TEMPERATURE

    def evaluate(self, case: dict[str, Any]) -> Evaluation:
        return evaluate_contract(self, sys.modules[__name__], case)
