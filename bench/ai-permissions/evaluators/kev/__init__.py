"""The Kev backend: a Kev run loaded in process and scored per request.

`KevEvaluator.setup` loads `run` through `kev.checkpoint.Checkpoint` with the defaults
`kev.serve` applies before serving, and `answer` scores one request with `model.encode` and
`model.probs`, the interface `kev.serve` calls, so an answer here and a served one agree. It
never starts `kev.serve` or binds a port. Needs the `kev-venv` interpreter (`run.sh`).
"""
from __future__ import annotations

import gc
from typing import Any, ClassVar

from ai_bench.evaluator import Evaluator, EvaluatorError


class KevEvaluator(Evaluator):
    """The base of every Kev mode: a mode sets `key`, `description` and `evaluate`, and may
    pin another `run`."""

    backend = "kev"
    #: The Hugging Face Hub run to load, `repo@revision`.
    run: ClassVar[str] = "jaredpalmer/kev-4b@139fdd94f1b6a6ad80cc15e08fcb99cac885a101"
    #: The `model` label a request carries. Kev echoes it; the run loaded decides the weights.
    model: ClassVar[str] = "kev-latest"

    def __init__(self) -> None:
        self._tokenizer: Any = None
        self._model: Any = None

    def setup(self) -> None:
        try:
            import torch
            from dataclasses import replace

            from kev.checkpoint import Checkpoint, LoadOptions
            from kev.device import default_device
        except ImportError as exc:
            raise EvaluatorError("%s needs the kev-venv interpreter (run.sh builds it): %s" % (self.key, exc)) from exc
        device = default_device()
        options = LoadOptions.from_env()
        if device == "mps" and options.attn is None:
            options = replace(options, attn="sdpa")
        if device != "cpu" and options.dtype is None:
            options = replace(options, dtype=torch.bfloat16)
        if options.backend is None:
            # `auto` resolves to the bf16 MLX path for a hybrid (Qwen3.5) run on Apple Silicon.
            options = replace(options, backend="auto")
        self._tokenizer, self._model = Checkpoint(self.run).load(device, options)

    def answer(self, state: Any, questions: dict[str, Any]) -> dict[str, Any]:
        """Kev's answer to one request, in the `{"answers": {...}}` shape."""
        if self._model is None:
            raise EvaluatorError("%s: evaluate before setup" % self.key)
        from kev.api import SystemOneRequest, to_answers, to_record
        from kev.model import SERVE_MAX_BRANCH, SERVE_MAX_STATE

        request = SystemOneRequest(state=state, model=self.model, questions=questions)
        record, meta = to_record(request)
        encoded = self._model.encode(self._tokenizer, record, max_state=SERVE_MAX_STATE, max_branch=SERVE_MAX_BRANCH)
        probabilities = self._model.probs(encoded)
        return {"model": self.model, "answers": to_answers([p.tolist() for p in probabilities], meta)}

    def teardown(self) -> None:
        self._tokenizer = None
        self._model = None
        gc.collect()
        try:
            import torch

            if torch.backends.mps.is_available():
                torch.mps.empty_cache()
        except ImportError:
            pass
