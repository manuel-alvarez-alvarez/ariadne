---
id: authoring-and-review
status: superseded
updated: 2026-10-09
superseded_by: 030
areas: [daemon, store, prompts]
commits: [ad268ee0, 2ca6dd29, 88bf39ac, da10e748, b21bd69e, a69b953f, 03f9c8b7, 29e6d84e, 1b09ac10]
tests: []
---

# Authoring and review

Superseded by [030](030-workflows.md). The number stays so that a reference in
the git history still points here.

This spec settled the fixed pipeline between a task becoming `ready` and
being `approved`: one author session per staffed author, one reviewer
session per staffed reviewer in a detached read-only worktree,
`request_review`, `submit_verdict`, the `under_review`, `changes_requested`
and `approved` statuses, and the pick that named the winning author on a
task staffed with several.

Every goal now runs on a workflow. What this spec settled is now the
workflow's columns:

- The author is the agent of the `develop` column. It builds the task on the
  task branch and ends its step with `complete_step` (030 rule 4).
- The reviewer is the agent of the `review` column. It works in the same
  worktree, runs the whole suite once, and judges the change; `fail_step`
  sends the task back to `develop` with the changes to make, and
  `complete_step` sends it on (030 rules 4 and 6, the `code-review` skill).
- There are no verdict messages and no picks. A review's outcome is a step
  call, and a task has one agent per column (018 rule 1, 030 rule 2).
- The statuses `under_review`, `changes_requested` and `approved` are gone:
  a task is `in_progress` from its first column to its last (001 rule 4).

A migration mapped the rows this spec wrote onto the workflow vocabulary
when workflows shipped.
