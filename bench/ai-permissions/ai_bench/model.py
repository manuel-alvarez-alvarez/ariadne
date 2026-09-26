"""Run configurations against a local checkpoint, in-process, never starting a server.

Two backends, each its own class below, both scoring `config["question"]` against the same
`build_state`/`build_question` output and returning the same per-case result shape (state,
questions, guardrail, answer, latency_ms), so `harness.py` and `experiments.py` need not care
which one a configuration names:

- `laya` (`Predictor`): calls `Router().predict_batch` directly, in the venv at
  `~/.ariadne/ai-permissions/venv`, against the checkpoints already downloaded under
  `HF_HOME=~/.ariadne/ai-permissions/hf`.
- `kev` (`KevPredictor`): loads a Kev checkpoint through `kev.checkpoint.Checkpoint` and scores it
  with the same encoder, pointer head and (on Apple Silicon) bf16 MLX path `kev.serve` uses, in
  the venv at `~/.ariadne/ai-permissions/kev-venv`, against `HF_HOME=~/.ariadne/ai-permissions/hf`.

One interpreter ever imports one of the two (`laya` needs Python 3.14; `kev` needs 3.12/3.13 and a
different torch), so `available()` lets a caller (`experiments.py`'s answer cache) tell "not
installed here" from every other failure.
"""
from __future__ import annotations

import os
import time
from pathlib import Path
from typing import Any

from . import guardrails as guardrails_mod
from .representations import build_question, build_state

BACKENDS = ("laya", "kev")

# Where each backend's venv lives, for an error message that names the fix.
VENV_HINT = {
    "laya": "~/.ariadne/ai-permissions/venv",
    "kev": "~/.ariadne/ai-permissions/kev-venv",
}


class BackendUnavailable(RuntimeError):
    """A configuration's backend cannot be imported in this interpreter, and no cached answer covers it."""


def resolve_revision(run: str) -> str:
    """The Hub commit `run` (e.g. `jaredpalmer/kev-4b`, or pinned as `jaredpalmer/kev-4b@<rev>`)
    resolves to right now. Uses `huggingface_hub` alone, never `kev`, so a config's cache key
    resolves the same way whichever venv computes it -- including the venv that cannot import
    `kev` at all. A local run directory resolves to itself (nothing to move).

    Always asks the Hub afresh (no local caching of this call): an unpinned `run` is a moving
    target, and the only way a cache key built from it catches that move -- instead of matching
    an older answer scored under a commit the Hub no longer calls `run` -- is to re-resolve it
    every time, before the cache is read."""
    if os.path.isdir(run):
        return run
    from huggingface_hub import HfApi

    repo, _, revision = run.partition("@")
    return HfApi().model_info(repo, revision=revision or None).sha


class Predictor:
    """The `laya` backend."""

    backend = "laya"

    def __init__(self):
        self._router = None

    def available(self) -> bool:
        try:
            import laya  # noqa: F401
        except ImportError:
            return False
        return True

    def _load_router(self):
        if self._router is None:
            from laya import Router  # imported lazily: heavy (torch, transformers)

            self._router = Router()
        return self._router

    def predict(self, requests: list[dict[str, Any]], batch_size: int) -> list[dict[str, Any]]:
        return self._load_router().predict_batch(requests, batch_size=batch_size)

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
            started = time.perf_counter()
            answers = self.predict(requests, batch_size)
            elapsed_ms = (time.perf_counter() - started) * 1000.0
            per_case_ms = elapsed_ms / len(to_predict)
            for i, answer in zip(to_predict, answers):
                results[i]["answer"] = answer
                results[i]["latency_ms"] = per_case_ms

        return results  # type: ignore[return-value]


class KevPredictor:
    """The `kev` backend: a configuration's `run` (a Hub id, optionally `@revision`) loaded through
    `kev.checkpoint.Checkpoint` and scored with `model.encode` + `model.probs` -- the same scoring
    interface `kev.serve.Server` calls per request, so an in-process answer and a served one agree.
    Never starts a `kev.serve` process or binds a port.
    """

    backend = "kev"

    def __init__(self):
        self._models: dict[str, tuple[Any, Any]] = {}  # run -> (tokenizer, model)
        self.resolved_revisions: dict[str, str] = {}  # run -> the Hub commit actually loaded

    def available(self) -> bool:
        try:
            import kev.checkpoint  # noqa: F401
        except ImportError:
            return False
        return True

    def load(self, run: str) -> tuple[Any, Any]:
        """(tokenizer, model) for `run`, loaded once per run. Applies the same defaults
        `kev.serve.main` applies before serving: `attn=sdpa` on MPS, `dtype=bf16` off CPU, and
        `backend=auto`, which resolves to the bf16 MLX path for a hybrid (Qwen3.5) checkpoint on
        Apple Silicon -- the path this task measures."""
        if run not in self._models:
            from dataclasses import replace

            import torch
            from kev.checkpoint import Checkpoint, LoadOptions, is_hub_id
            from kev.device import default_device

            device = default_device()
            opts = LoadOptions.from_env()
            if device == "mps" and opts.attn is None:
                opts = replace(opts, attn="sdpa")
            if device != "cpu" and opts.dtype is None:
                opts = replace(opts, dtype=torch.bfloat16)
            if opts.backend is None:
                opts = replace(opts, backend="auto")
            checkpoint = Checkpoint(run)
            tok, model = checkpoint.load(device, opts)
            self._models[run] = (tok, model)
            self.resolved_revisions[run] = Path(checkpoint.path).name if is_hub_id(run) else run
        return self._models[run]

    def predict_one(self, run: str, state: Any, questions: dict[str, Any], checkpoint: str) -> dict[str, Any]:
        from kev.api import SystemOneRequest, to_answers, to_record
        from kev.model import SERVE_MAX_BRANCH, SERVE_MAX_STATE

        tok, model = self.load(run)
        request = SystemOneRequest(state=state, model=checkpoint, questions=questions)
        record, meta = to_record(request)
        enc = model.encode(tok, record, max_state=SERVE_MAX_STATE, max_branch=SERVE_MAX_BRANCH)
        probs = model.probs(enc)
        answers = to_answers([p.tolist() for p in probs], meta)
        return {"model": checkpoint, "answers": answers}

    def evaluate(self, config: dict[str, Any], cases: list[dict[str, Any]], batch_size: int = 16) -> list[dict[str, Any]]:
        """One result dict per case. `batch_size` is accepted for interface parity with `Predictor`
        and ignored: Kev's MLX path scores one state at a time (`kev.mlx_model.probs_batch`)."""
        rules = guardrails_mod.load_guardrails(config.get("guardrails"))
        question = build_question(config["question"])
        run = config["run"]
        checkpoint = config["checkpoint"]

        results: list[dict[str, Any]] = []
        for case in cases:
            request = case["request"]
            state = build_state(config, request, case["repository"])
            guardrail = guardrails_mod.match(rules, request)
            base = {"state": state, "questions": question, "guardrail": guardrail}
            if guardrail is not None:
                results.append({**base, "answer": None, "latency_ms": 0.0})
                continue
            started = time.perf_counter()
            answer = self.predict_one(run, state, question, checkpoint)
            latency_ms = (time.perf_counter() - started) * 1000.0
            results.append({**base, "answer": answer, "latency_ms": latency_ms})
        return results


def make_predictor(backend: str) -> Predictor | KevPredictor:
    if backend == "kev":
        return KevPredictor()
    if backend == "laya":
        return Predictor()
    raise ValueError("unknown backend %r (must be one of %s)" % (backend, ", ".join(BACKENDS)))
