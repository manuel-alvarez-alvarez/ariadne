"""The Laya backend: Laya's `Router` in process, one request at a time.

`LayaEvaluator.setup` creates the router and loads the mode's `checkpoints`; `teardown`
unloads them. It never starts `laya-serve` or binds a port. Needs the `laya-venv`
interpreter (`run.sh`).
"""
from __future__ import annotations

import gc
from typing import Any, ClassVar

from ai_bench.evaluator import Evaluator, EvaluatorError


class LayaEvaluator(Evaluator):
    """The base of every Laya mode: a mode sets `key`, `description`, `checkpoints` and
    `evaluate`."""

    backend = "laya"
    #: The checkpoints `setup` loads; the first is the one `answer` asks.
    checkpoints: ClassVar[tuple[str, ...]] = ("english",)

    def __init__(self) -> None:
        self._router: Any = None

    def setup(self) -> None:
        try:
            from laya import Router
        except ImportError as exc:
            raise EvaluatorError("%s needs the laya-venv interpreter (run.sh builds it): %s" % (self.key, exc)) from exc
        self._router = Router(max_loaded=len(self.checkpoints))
        self._router.preload(list(self.checkpoints))

    def answer(self, state: Any, questions: dict[str, Any]) -> dict[str, Any]:
        """Laya's answer to one request, from the first of `checkpoints`."""
        if self._router is None:
            raise EvaluatorError("%s: evaluate before setup" % self.key)
        return self._router.predict(state, questions, model=self.checkpoints[0])

    def teardown(self) -> None:
        if self._router is not None:
            for checkpoint in self.checkpoints:
                self._router.unload(checkpoint)
        self._router = None
        gc.collect()
