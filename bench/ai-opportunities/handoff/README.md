# Session handoff selection benchmark

Run `python3 bench/ai-opportunities/handoff/run.py` for the primary local measurement.

Run `python3 bench/ai-opportunities/handoff/run.py --metrics-only` to recompute committed prediction metrics.

The benchmark has 60 sanitized synthetic histories and 20 reserved evaluation histories.
Families and paraphrases stay in one split. Labels are frozen before scoring.

Policies receive identical character budgets, including introduction and omission framing.
The benchmark copies folded entries, whole-entry removal, and current recency behavior.
Selections keep exact rendered entry text and restore source order.

The runner locks the shared path before loading Kev and releases it when the foreground process exits.
It reads no `crates/` file at runtime. Kev failure, timeout, or invalid scores use recency fallback.

A later integration can select after `render_entry` and before `apply_budget`, retaining event ids and user-prompt provenance.
