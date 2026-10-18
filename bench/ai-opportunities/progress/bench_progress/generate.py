"""`python3 -m bench_progress.generate` - the one documented command that rebuilds
`cases/dev.jsonl` and `cases/eval.jsonl`. By default it is fully deterministic: the 122
synthetic windows are rebuilt from `synth.py`, and whatever real-sanitized windows are already
committed in the two case files are carried over unchanged - run it twice with no flag, get the
same two files, because nothing here reads the live database unless asked to.

`--refresh-real` additionally re-pulls the sanitized sample from `~/.ariadne/ariadne.db` (where
that file exists) and replaces the real-sanitized windows with a fresh sample. That database is
live and keeps growing while this goal's own sessions run, so a refresh is a deliberate, logged
action, never a side effect of an ordinary rebuild (the reviewed concern this flag answers: a
plain rebuild must not silently reshuffle which real windows exist, or which split they fall in,
underneath a frozen `cases/eval.jsonl`).
"""
from __future__ import annotations

import argparse
import dataclasses
import sys

from . import real_export, synth
from .dataset import DEV_PATH, EVAL_PATH, load_dataset
from .schema import Window, dump_windows, load_windows


def _assign_real_splits(real_windows: list[Window]) -> list[Window]:
    """Every third real family (by sorted session id) goes to eval, keeping each family whole
    on one side (task item 7)."""
    families = sorted({w.family for w in real_windows})
    eval_families = {f for i, f in enumerate(families) if i % 3 == 2}
    out = []
    for w in real_windows:
        split = "eval" if w.family in eval_families else "dev"
        out.append(dataclasses.replace(w, split=split, id=f"{w.id}-{split}"))
    return out


def _existing_real_windows() -> list[Window]:
    """Whatever real-sanitized windows are already committed, carried over by default so a
    plain rebuild never touches the live database or reshuffles the reserved eval split."""
    out = []
    for path in (DEV_PATH, EVAL_PATH):
        try:
            out += [w for w in load_windows(path) if w.provenance == "real_sanitized"]
        except FileNotFoundError:
            pass
    return out


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument(
        "--refresh-real",
        action="store_true",
        help="re-pull the sanitized real sample from ~/.ariadne/ariadne.db instead of carrying "
        "over whatever is already committed; may move real families between splits (task item 7 "
        "is still honoured within the new sample, just not against the old one)",
    )
    args = ap.parse_args()

    windows = synth.build_all()
    if args.refresh_real:
        real_windows = _assign_real_splits(real_export.export())
        print(f"refreshed {len(real_windows)} real-sanitized windows from the live database")
    else:
        real_windows = _existing_real_windows()
        print(f"carried over {len(real_windows)} already-committed real-sanitized windows")
    windows += real_windows

    dev = [w for w in windows if w.split == "dev"]
    ev = [w for w in windows if w.split == "eval"]

    dump_windows(dev, DEV_PATH)
    dump_windows(ev, EVAL_PATH)

    # Fail loudly here rather than silently commit a broken dataset.
    dev_loaded, eval_loaded = load_dataset()
    print(f"wrote {len(dev_loaded)} dev windows ({DEV_PATH})")
    print(f"wrote {len(eval_loaded)} eval windows ({EVAL_PATH})")
    print(f"real-sanitized windows: {len(real_windows)} (synthetic: {len(windows) - len(real_windows)})")
    if len(dev_loaded) + len(eval_loaded) < 120:
        print("error: fewer than 120 windows total", file=sys.stderr)
        return 1
    if len(eval_loaded) < 40:
        print("error: fewer than 40 reserved eval windows", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
