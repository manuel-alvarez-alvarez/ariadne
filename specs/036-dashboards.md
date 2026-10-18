---
id: 036
status: current
updated: 2026-10-19
areas: [store, daemon, api, cli, docs, ui]
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
6. The shipped `stats` document uses Perses layouts that refer to named
   panels, with installed Prometheus, Loki and panel plugins.
7. Its sections are Overview (headline numbers), Work, Spend, Agents &
   models, Attention & permissions, Daemon health and Logs; the last two
   open collapsed. Every panel is laid out once.
8. Its `repo` and `model` variables list the label values of the token and
   task-ending metrics, and filter every panel whose metric carries that
   label. They are spliced in with `regexliteral` escaping, so a model id
   such as `claude-acp:opus[1m]` matches itself only. Its `bucket` variable
   (hour, 6 hours, day) is the width of each bar over time, set as both the
   window and the query's minimum step.
9. A total over the selected range reads `[$__range] @ end()`, so every step
   of the range query shows the whole range and a series with nothing in
   it drops out instead of showing a count from before the range.

## Acceptance criteria

1. `dashboards.rs` proves CRUD, validation, reset safety and the fat events.
2. `ui/dashboard/src/shipped-dashboard.test.ts` passes the shipped document
   through the installed Perses layout converter, lays out every panel
   once, and names only declared or built-in variables.

## Sources

- Objective: Keep editable Perses dashboards in the database with a shipped default.
