---
id: how-a-task-ends
status: superseded
updated: 2026-10-09
superseded_by: 030
areas: [daemon, store, prompts]
commits: [ad268ee0, 305ee064, 45c5e131, 8174c256, 90ac6e67, 524856c7, fdd0c5b6, a69b953f, 29e6d84e, f79c8e15, a4d7da95]
tests: []
---

# How a task ends

Superseded by [030](030-workflows.md). The number stays so that a reference in
the git history still points here.

This spec settled the four landings a goal chose once — `merge`,
`pull_request`, `none` and `feature_branch` — the landing briefing that ran
each, `finish_task` and its merge verification, the goal branch of a
`feature_branch` goal and the final task that took it onto the base branch.

Every goal now runs on a workflow, and how a task ends is the gate of its
last column:

- `merge` is the `develop-review-merge` workflow. Its `merge` column stages
  the `merge` skill, which rebases, runs the whole suite once, squashes,
  fast-forwards the base branch and pushes; its gate `merged` proves the
  merge commit is on the base branch before the task finishes (030 rules 4
  and 5).
- `pull_request` is the `develop-review-pr` workflow. Its `pr` column stages
  `pr-babysit`, opens the request once with `open_pull_request` and keeps it;
  its gate `request_merged` reads the forge at the call (030 rule 10, 026).
- `none` and `feature_branch` map onto `develop-review-merge` (030 rule 9).
  There is no goal branch and no final task: a workflow of the user's own
  can gate a column on `pushed` and stop there.
- `finish_task` is gone: `complete_step` on the last column finishes the
  task, and the gate is the verification (001 rule 5).

A repository's `default_landing` is now its `default_workflow`, and a goal's
`landing` is its `workflow`. Migration `0023_workflows_only.sql` maps every
old row (030 rule 9).
