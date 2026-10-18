"""Scores a raw `worker/kev_infer.py` output file directly against case labels - used only to
pick between the two prompt designs on dev (task: "evaluate the selected design once on the
reserved cases"). Not part of the committed metrics pipeline; `build_results.py` reads the
mapped `Decision`s via `replay.py`, not this.
"""
from __future__ import annotations

import argparse
import json

from .dataset import load_dataset


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--raw", required=True)
    ap.add_argument("--split", choices=["dev", "eval"], default="dev")
    args = ap.parse_args()

    dev, ev = load_dataset()
    windows = {w.id: w for w in (dev if args.split == "dev" else ev)}

    total = 0
    correct = 0
    confusion: dict[str, dict[str, int]] = {}
    with open(args.raw, encoding="utf-8") as fh:
        for line in fh:
            row = json.loads(line)
            w = windows.get(row["window_id"])
            if w is None:
                continue
            total += 1
            predicted = row.get("raw_choice") or "NONE"
            confusion.setdefault(w.label, {}).setdefault(predicted, 0)
            confusion[w.label][predicted] += 1
            if predicted == w.label:
                correct += 1

    print(f"accuracy: {correct}/{total} = {correct / total:.3f}")
    print("confusion (true label -> predicted -> count):")
    for true_label, preds in sorted(confusion.items()):
        print(f"  {true_label}:")
        for predicted, count in sorted(preds.items(), key=lambda kv: -kv[1]):
            print(f"    {predicted}: {count}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
