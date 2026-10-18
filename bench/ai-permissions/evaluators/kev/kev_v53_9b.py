"""Kev mode 53 on the 9b run: `kev_v44_9b` at temperature 0.5.

The question, criteria, state, danger and cap are `kev_v44_9b`'s. The temperature is the one
of the offline scan of the log-odds danger that allows the most safe development cases.
"""
from __future__ import annotations

import sys
from typing import Any

from ai_bench.evaluator import Evaluation
from ai_bench.registry import register

from . import KevEvaluator, evaluate_contract
from .kev_v44_9b import CAPS, QUESTIONS, RUN, danger, state

TEMPERATURE = 0.5
#: The strict four-decimal pair of the development run, margin 0.05. Every set gives the same pair.
ALLOW_THRESHOLD = 0.0198
DENY_THRESHOLD = 0.6437


@register
class KevV53_9b(KevEvaluator):
    key = "kev_v53_9b"
    description = "kev_v44_9b at temperature 0.5"
    run = RUN
    temperature = TEMPERATURE

    def evaluate(self, case: dict[str, Any]) -> Evaluation:
        return evaluate_contract(self, sys.modules[__name__], case)
