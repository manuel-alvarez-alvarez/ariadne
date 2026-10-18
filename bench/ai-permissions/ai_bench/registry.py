"""The runner's registry of evaluators, by key.

A concrete evaluator registers itself with the `@register` class decorator. `load_all` imports
every module under the `evaluators` package, which is what registers them; nothing heavy is
imported then, since backends load their models in `setup`, so listing works in any
interpreter.
"""
from __future__ import annotations

import importlib
import pkgutil

from .evaluator import Evaluator, EvaluatorError

_REGISTRY: dict[str, type[Evaluator]] = {}


def register(cls: type[Evaluator]) -> type[Evaluator]:
    """Register a concrete evaluator under its `key`, which must be unique."""
    if not cls.key:
        raise EvaluatorError("%s has no key" % cls.__name__)
    if not cls.backend:
        raise EvaluatorError("%s (%s) has no backend" % (cls.__name__, cls.key))
    existing = _REGISTRY.get(cls.key)
    if existing is not None and existing is not cls:
        raise EvaluatorError(
            "evaluator key %r is registered twice: %s and %s" % (cls.key, existing.__qualname__, cls.__qualname__)
        )
    _REGISTRY[cls.key] = cls
    return cls


def load_all() -> dict[str, type[Evaluator]]:
    """Import every evaluator module, and return every registered evaluator by key, sorted."""
    import evaluators

    for module in pkgutil.walk_packages(evaluators.__path__, evaluators.__name__ + "."):
        importlib.import_module(module.name)
    return dict(sorted(_REGISTRY.items()))


def get(key: str) -> type[Evaluator]:
    registered = load_all()
    if key not in registered:
        raise EvaluatorError("no evaluator %r; `run.py list` shows the %d registered" % (key, len(registered)))
    return registered[key]
