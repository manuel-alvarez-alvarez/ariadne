"""Kev mode 28 on the 9b run: `kev_v28`'s contract, pinned to `kev-9b` instead of `kev-4b`.

The question, criteria, state, temperature and cap are `kev_v28`'s, word for word, by reusing
its module attributes directly. Only `RUN` and the two thresholds differ.
"""
from __future__ import annotations

import sys
from typing import Any

from ai_bench.evaluator import Evaluation
from ai_bench.registry import register

from . import KevEvaluator, evaluate_contract
from .kev_v28 import CAPS, QUESTIONS, danger, state

RUN = "jaredpalmer/kev-9b@2629c06a5aeb0feb3b9783bafed17ed8f39ecf5c"
TEMPERATURE = 0.6
#: The pair `select --margin 0.05` finds over every set on kev-9b, at strict four-decimal
#: precision. It holds rules 1 to 3 and is unusable: a negative allow threshold admits no case
#: (README, "Winner (2026-10-07) on kev-9b").
ALLOW_THRESHOLD = -0.0499
DENY_THRESHOLD = 0.9220


@register
class KevV28_9b(KevEvaluator):
    key = "kev_v28_9b"
    description = "kev_v28's contract, pinned to the 9b run"
    run = RUN
    temperature = TEMPERATURE

    def evaluate(self, case: dict[str, Any]) -> Evaluation:
        return evaluate_contract(self, sys.modules[__name__], case)
