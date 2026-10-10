---
id: how-a-task-ends
status: superseded
updated: 2026-10-10
superseded_by: 030
areas: [daemon, store, prompts]
commits: [ad268ee0, 305ee064, 45c5e131, 8174c256, 90ac6e67, 524856c7, fdd0c5b6, a69b953f, 29e6d84e, f79c8e15, a4d7da95]
tests:
  - crates/ariadne-daemon/tests/it/workflow_steps.rs
---

# How a task ends

Superseded by [030](030-workflows.md). This file remains only so that links in
history resolve.

## Scope

The final workflow column now owns task completion.

## Behavior

1. See [030](030-workflows.md) for task completion and final-column gates.

## Acceptance criteria

1. The final workflow column completes only after its gate passes (`crates/ariadne-daemon/tests/it/workflow_steps.rs::a_step_agent_opens_its_request_and_completion_reads_the_forge_now`).

## Sources

- [030](030-workflows.md)
