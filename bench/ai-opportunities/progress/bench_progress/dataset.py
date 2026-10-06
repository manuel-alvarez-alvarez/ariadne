"""Loads the committed case files and checks the one invariant the whole bench depends on: no
family is split across dev and eval (task item 7), and nothing in a window's events or session
state is timestamped after its own decision point (task item 9).
"""
from __future__ import annotations

import os
from datetime import datetime

from .schema import Window, load_windows

CASES_DIR = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "cases")
DEV_PATH = os.path.join(CASES_DIR, "dev.jsonl")
EVAL_PATH = os.path.join(CASES_DIR, "eval.jsonl")


class DatasetError(ValueError):
    pass


def _parse(ts: str) -> datetime:
    return datetime.fromisoformat(ts.replace("Z", "+00:00"))


def check_no_future_leakage(window: Window) -> None:
    now = _parse(window.session.now)
    for e in window.events:
        if _parse(e.ts) > now:
            raise DatasetError(f"{window.id}: event at {e.ts} is after the decision point {window.session.now}")
    for stamp in (window.session.last_activity_at, window.session.launched_at):
        if stamp and _parse(stamp) > now:
            raise DatasetError(f"{window.id}: session stamp {stamp} is after the decision point {window.session.now}")


def check_family_split_integrity(windows: list[Window]) -> None:
    family_splits: dict[str, str] = {}
    for w in windows:
        prior = family_splits.setdefault(w.family, w.split)
        if prior != w.split:
            raise DatasetError(f"family {w.family!r} appears in both {prior!r} and {w.split!r}")


def load_dataset(dev_path: str = DEV_PATH, eval_path: str = EVAL_PATH) -> tuple[list[Window], list[Window]]:
    dev = load_windows(dev_path)
    ev = load_windows(eval_path)
    for w in dev:
        if w.split != "dev":
            raise DatasetError(f"{w.id} is in dev.jsonl but labelled split={w.split!r}")
        check_no_future_leakage(w)
    for w in ev:
        if w.split != "eval":
            raise DatasetError(f"{w.id} is in eval.jsonl but labelled split={w.split!r}")
        check_no_future_leakage(w)
    check_family_split_integrity(dev + ev)
    dev_families = {w.family for w in dev}
    eval_families = {w.family for w in ev}
    overlap = dev_families & eval_families
    if overlap:
        raise DatasetError(f"families present in both splits: {sorted(overlap)}")
    return dev, ev
