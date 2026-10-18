# Benchmarks

Local experiments that measure a part of Ariadne and select its defaults.
Conventions for changing one are in [`AGENTS.md`](AGENTS.md).

- [`ai-permissions/`](ai-permissions/README.md) — how well a local model tells
  a safe coding-agent tool call from one that needs a person, for the `ai`
  permission mode: the cases, the runner, and one evaluator per model and mode.
- [`ai-opportunities/`](ai-opportunities/README.md) — measured Kev pilots for
  failure diagnosis, stalled progress, and handoff history, with scoped
  candidates for further evidence and implementation.
- [`token-usage/`](token-usage/README.md) — offline historical token and
  elapsed-time audit, its reconciliation rules, aggregate artifacts, and
  ranked candidates for separately approved work.
