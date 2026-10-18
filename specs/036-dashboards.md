---
id: 036
status: current
updated: 2026-10-18
areas: [store, daemon, api, cli, docs]
commits: []
tests:
  - crates/ariadne-daemon/tests/it/dashboards.rs
---

# Dashboards

## Scope

Dashboard documents are editable Perses resources. The desktop renderer is out of scope.

## Behavior

1. Ariadne ships `stats` and stores overrides in `dashboards`.
2. A document is a Perses `Dashboard` with its matching metadata name and an object spec.
3. The REST catalog creates, lists, reads, replaces, deletes and resets dashboard documents.
4. Dashboard changes publish fat created, updated and deleted events.
5. The CLI exposes `dashboard ls`, `get`, `set`, `reset` and `rm`.

## Acceptance criteria

1. `dashboards.rs` proves CRUD, validation, reset safety and the fat events.

## Sources

- Objective: Keep editable Perses dashboards in the database with a shipped default.
