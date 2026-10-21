---
id: 036
status: current
updated: 2026-10-21
areas: [store, daemon, api, cli, docs, ui]
commits: []
tests:
  - crates/ariadne-daemon/tests/it/dashboards.rs
  - ui/dashboard/src/format-value.test.ts
  - ui/dashboard/src/shipped-dashboard.test.ts
---

# Dashboards

## Scope

Dashboard documents are editable Perses resources. In: the renderer's view
and edit mode, since it decides whether an edit reaches the daemon at all
(rule 10). Out: the rest of the desktop renderer — its layout, wiring and
message protocol are spec 015's.

## Behavior

1. Ariadne ships `default` and stores overrides in `dashboards`.
2. A document is a Perses `Dashboard` with its matching metadata name and an object spec.
3. The REST catalog creates, lists, reads, replaces, deletes and resets dashboard documents.
4. Dashboard changes publish fat created, updated and deleted events.
5. The CLI exposes `dashboard ls`, `get`, `set`, `reset` and `rm`.
6. The shipped `default` document uses Perses layouts that refer to named
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
10. The desktop renderer opens a dashboard in view mode: Perses's own Edit
    button, not a prop the renderer sets, is what switches it into edit
    mode, and Perses's own Save and Cancel both return it to view mode —
    Save after writing the document through the daemon's `PUT`, Cancel by
    discarding the edit. A legend shows a value where one helps the reader:
    each pie chart's legend is a table with its slice value and percent; the
    per-bucket time series (tasks, tokens, turns and attention over time)
    show the sum of each series; the lead-time trend shows the mean and the
    max of each series. The daemon-health charts keep a plain list legend,
    with no values.
11. The shipped dashboard has display name `Default`. The desktop app uses a
    dashboard document's `spec.display.name` wherever it presents the
    dashboard to the user, falling back to its metadata name when absent;
    routes and mutations continue to use the metadata name. Copying a
    dashboard replaces its display name with the copy's metadata name.
12. No query in the shipped `default` document gives `NaN` over an empty
    range: a division whose denominator can be zero guards it with
    `> 0`, and a quantile over a histogram guards its result with an `and`
    over its own `+Inf` bucket's `increase()`. A query a guard leaves with
    no series answers with the renderer's own empty state, the same way a
    panel already filters out a zero with `> 0` elsewhere in the document.
    The desktop renderer turns a `NaN` panel value into that same empty
    state in any dashboard, including one the user wrote: `formatValue`,
    the one function every panel kind formats its numbers through, is
    patched in `@perses-dev/components` to answer a `NaN` with "No data"
    before any unit formatter sees it.
13. The Logs section's shipped table selects every available app service
    and level. A saved override keeps its existing table until reset restores
    the current shipped document.

## Acceptance criteria

1. `dashboards.rs` proves CRUD, validation, reset safety and the fat events.
2. `ui/dashboard/src/shipped-dashboard.test.ts` passes the shipped document
   through the installed Perses layout converter, lays out every panel
   once, names only declared or built-in variables, and proves the legend
   values of rule 10.
3. The renderer opens `ViewDashboard` out of edit mode, on first load and
   again after a remount
   (`ui/dashboard/src/page.test.tsx::opens the dashboard in view mode, not
   Perses's own editing mode`, `::remounts ViewDashboard with the fresh
   document on a reload message`).
4. `ui/src/routes/dashboards.test.tsx::lists display names in the picker, falls
   back to metadata names, and keeps the id in the iframe` and `::uses the
   selected dashboard's display name in Reset and Delete confirmations`,
   `ui/src/features/dashboard/new-dashboard-dialog.test.tsx::offers Empty and
   Copy of the current dashboard's display name when one is given`, and
   `::replaces the copied dashboard's display name with its new name`,
   `ui/src/features/dashboard/rename-dashboard-dialog.test.tsx::titles the
   dialog with the dashboard's display name`, and
   `ui/dashboard/src/shipped-dashboard.test.ts::uses Default as its display
   name while retaining default as its metadata name` prove the shipped name,
   desktop labels, and fallback of rule 11.
5. `dashboards.rs::no_panel_query_in_the_shipped_dashboard_gives_nan_over_an_empty_range`
   sweeps every Prometheus query of the shipped document over a range with
   every counter held flat rather than absent, and proves none answers
   `NaN`. `ui/dashboard/src/format-value.test.ts` proves the patched
   `formatValue` of rule 12 answers a `NaN` with "No data".
6. `ui/dashboard/src/shipped-dashboard.test.ts::shows every service and level in its operational log table`
   proves rule 13's shipped query. `dashboards.rs::resetting_a_built_in_dashboard_restores_its_shipped_document`
   proves reset restores the shipped document.

## Sources

- Objective: Keep editable Perses dashboards in the database with a shipped default.
