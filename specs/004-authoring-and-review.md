---
id: authoring-and-review
status: superseded
updated: 2026-10-10
superseded_by: 030
areas: [daemon, store, prompts]
commits: [ad268ee0, 2ca6dd29, 88bf39ac, da10e748, b21bd69e, a69b953f, 03f9c8b7, 29e6d84e, 1b09ac10]
tests:
  - crates/ariadne-daemon/tests/it/workflow_steps.rs
---

# Authoring and review

Superseded by [030](030-workflows.md). This file remains only so that links in
history resolve.

## Scope

The workflow columns now own this behavior.

## Behavior

1. See [030](030-workflows.md) for authoring and review.

## Acceptance criteria

1. Workflow columns move a task through authoring and review (`crates/ariadne-daemon/tests/it/workflow_steps.rs::a_task_walks_develop_review_merge_with_one_agent_per_column`).

## Sources

- [030](030-workflows.md)
