"""The Kev backend: a Kev run loaded in process and scored per request.

`KevEvaluator.setup` loads `run` through `kev.checkpoint.Checkpoint` with the defaults
`kev.serve` applies before serving, and `answer` scores one request with `model.encode` and
`model.probs`, the interface `kev.serve` calls, so an answer here and a served one agree. It
never starts `kev.serve` or binds a port. Needs the `kev-venv` interpreter (`run.sh`).
"""
from __future__ import annotations

import gc
from typing import Any, ClassVar

from ai_bench import decision
from ai_bench.derive import derive
from ai_bench.evaluator import Evaluation, Evaluator, EvaluatorError


class KevEvaluator(Evaluator):
    """The base of every Kev mode: a mode sets `key`, `description` and `evaluate`, and may
    pin another `run`."""

    backend = "kev"
    #: The Hugging Face Hub run to load, `repo@revision`.
    run: ClassVar[str] = "jaredpalmer/kev-4b@139fdd94f1b6a6ad80cc15e08fcb99cac885a101"
    #: The `model` label a request carries. Kev echoes it; the run loaded decides the weights.
    model: ClassVar[str] = "kev-latest"
    #: The temperature the logits are divided by before the softmax; `None` keeps the one the
    #: checkpoint carries (or `KEV_TEMPERATURE`), the same knob `kev.serve` reads.
    temperature: ClassVar[float | None] = None

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
        if self.temperature is not None:
            options = replace(options, temperature=self.temperature)
        self._tokenizer, self._model = Checkpoint(self.run).load(device, options)

    def _probabilities(self, state: Any, questions: dict[str, Any]) -> tuple[list[list[float]], list[dict[str, Any]]]:
        if self._model is None:
            raise EvaluatorError("%s: evaluate before setup" % self.key)
        from kev.api import SystemOneRequest, to_record
        from kev.model import SERVE_MAX_BRANCH, SERVE_MAX_STATE

        request = SystemOneRequest(state=state, model=self.model, questions=questions)
        record, meta = to_record(request)
        encoded = self._model.encode(self._tokenizer, record, max_state=SERVE_MAX_STATE, max_branch=SERVE_MAX_BRANCH)
        return [p.tolist() for p in self._model.probs(encoded)], meta

    def answer(self, state: Any, questions: dict[str, Any]) -> dict[str, Any]:
        """Kev's answer to one request, in the `{"answers": {...}}` shape."""
        from kev.api import to_answers

        probabilities, meta = self._probabilities(state, questions)
        return {"model": self.model, "answers": to_answers(probabilities, meta)}

    def probabilities(self, state: Any, questions: dict[str, Any]) -> dict[str, list[float]]:
        """The probabilities of each question of one request, in option order, not rounded."""
        probabilities, meta = self._probabilities(state, questions)
        return {one["id"]: [float(p) for p in found] for one, found in zip(meta, probabilities)}

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


def evaluate_contract(evaluator: KevEvaluator, contract: Any, case: dict[str, Any]) -> Evaluation:
    """One case under the contract module `contract`: a hard rule of its `RULES` denies with no
    call to the model; else the model answers its `QUESTIONS` over its `state`, and a tag of
    its `CAPS` keeps the call from `allow`."""
    request, workspace = case["request"], case.get("repository")
    derived = derive(request, workspace)
    if derived.rule in contract.RULES:
        return decision.ruled(derived.rule)
    answer = evaluator.answer(contract.state(request, workspace), contract.QUESTIONS)
    evaluation = decision.three_way(contract.danger(answer), contract.ALLOW_THRESHOLD, contract.DENY_THRESHOLD)
    return decision.capped(evaluation, derived.risk_tags, contract.CAPS)


def evaluate_probability_contract(evaluator: KevEvaluator, contract: Any, case: dict[str, Any]) -> Evaluation:
    """One case under the probability-policy contract `contract`: a hard rule of its `RULES`
    denies with no call to the model; else the model answers its `QUESTIONS` over its `state`,
    and `contract.decision` turns the answer into the label, with no call to `contract.CAPS`
    here: `contract.decision` applies its own caps."""
    request, workspace = case["request"], case.get("repository")
    derived = derive(request, workspace)
    if derived.rule in contract.RULES:
        return decision.ruled(derived.rule)
    answer = evaluator.answer(contract.state(request, workspace), contract.QUESTIONS)
    return contract.decision(answer, derived)
