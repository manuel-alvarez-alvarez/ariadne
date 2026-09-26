"""Run configurations against the local laya checkpoints, in-process.

Never starts a server: this calls `Router().predict_batch` directly, in the venv at
`~/.ariadne/ai-permissions/venv`, against the checkpoints already downloaded under
`HF_HOME=~/.ariadne/ai-permissions/hf`.
"""
from __future__ import annotations

import time
from typing import Any

from . import guardrails as guardrails_mod
from .representations import build_question, build_state


class Predictor:
    def __init__(self):
        self._router = None

    def _load_router(self):
        if self._router is None:
            from laya import Router  # imported lazily: heavy (torch, transformers)

            self._router = Router()
        return self._router

    def evaluate(self, config: dict[str, Any], cases: list[dict[str, Any]], batch_size: int = 16) -> list[dict[str, Any]]:
        """One result dict per case, in the same order: state, questions, guardrail, answer, latency_ms."""
        rules = guardrails_mod.load_guardrails(config.get("guardrails"))
        question = build_question(config["question"])

        results: list[dict[str, Any] | None] = [None] * len(cases)
        to_predict: list[int] = []
        requests: list[dict[str, Any]] = []

        for i, case in enumerate(cases):
            request = case["request"]
            repository = case["repository"]
            state = build_state(config, request, repository)
            guardrail = guardrails_mod.match(rules, request)
            base = {"state": state, "questions": question, "guardrail": guardrail}
            if guardrail is not None:
                results[i] = {**base, "answer": None, "latency_ms": 0.0}
            else:
                to_predict.append(i)
                requests.append({"state": state, "questions": question, "model": config["checkpoint"]})
                results[i] = base

        if to_predict:
            router = self._load_router()
            started = time.perf_counter()
            answers = router.predict_batch(requests, batch_size=batch_size)
            elapsed_ms = (time.perf_counter() - started) * 1000.0
            per_case_ms = elapsed_ms / len(to_predict)
            for i, answer in zip(to_predict, answers):
                results[i]["answer"] = answer
                results[i]["latency_ms"] = per_case_ms

        return results  # type: ignore[return-value]
