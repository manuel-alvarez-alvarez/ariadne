# bench/AGENTS.md

Conventions for changing a benchmark. Commit-message and history rules live in
the root [`AGENTS.md`](../AGENTS.md).

## Layout

One directory per experiment, each self-contained: it depends on no file
under `crates/`, and nothing under `crates/` depends on it. The one link
between a benchmark and the daemon is at run time, through what the daemon
installs (model weights, environments) or reads (its own database,
read-only) — never a shared source file.

None of this runs in CI.

- [`ai-permissions/`](ai-permissions/README.md) — how well a local model
  tells a safe coding-agent tool call from one that needs a person, for the
  `ai` permission mode.
- [`ai-opportunities/`](ai-opportunities/README.md) — measured pilots for
  failure diagnosis, stalled progress and handoff history selection.

## Checks

Each experiment's own README names its checks; there is no repository-wide
one. Today:

- `ai-permissions/` — `python3 -m pytest tests` (its `tests/` directory).
- `ai-opportunities/progress/` — `python3 -m pytest tests/ -q`.
- `ai-opportunities/handoff/` — `python3 -m pytest
  bench/ai-opportunities/handoff/tests -q`.

Read the experiment's own README before changing it: a benchmark's cases,
runner and evaluators are its own convention, not a shared one.
