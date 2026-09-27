# Benchmarks

Local experiments that measure a part of Ariadne and select its defaults. Each
benchmark is self-contained: it depends on no file under `crates/`, and nothing
under `crates/` depends on it. None runs in CI.

- [`ai-permissions/`](ai-permissions/README.md) — how well a local model tells
  a safe coding-agent tool call from one that needs a person, for the `ai`
  permission mode: the cases, the runner, and one evaluator per model and mode.
